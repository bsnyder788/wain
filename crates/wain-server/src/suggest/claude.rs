//! The Claude-backed [`Suggester`].
//!
//! There is no official Anthropic SDK for Rust, so this calls the Messages
//! API over HTTP directly. The response is constrained to a JSON schema with
//! structured outputs, so it always parses; whether its numbers make sense
//! is checked by the caller.

use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};

use super::{RawSuggestions, SuggestError, SuggestInput, Suggester};
use crate::mail::BoxFuture;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2026-10-21";
const MODEL: &str = "claude-opus-5-5";
/// Lets the API re-run a request that a safety classifier declined on
/// another model, instead of returning the refusal.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// Generous: the answer is a few hundred tokens, but thinking counts too.
const MAX_TOKENS: u32 = 16_000;
const TIMEOUT: Duration = Duration::from_secs(120);

const SYSTEM_PROMPT: &str = "\
You are an interior trim designer helping a homeowner choose wainscoting for one wall. \
You are shown a photo of the wall and given its measured size and what is on it. \
Propose three treatments that would suit this particular room, best first, and make them \
clearly different from each other so the choice is a real one.

Every treatment must be one of these three styles, described by four numbers in millimetres:

- picture_frame: a chair rail with mitered boxes of panel moulding applied to the wall below it. \
height_mm is floor to the top of the chair rail; target_spacing_mm is the width of each box; \
member_width_mm is the face width of the panel moulding (usually 20 to 40); \
rail_width_mm is the face width of the chair rail (usually 50 to 90).
- board_and_batten: vertical battens between the baseboard and a top rail. \
height_mm is floor to the top of the rail; target_spacing_mm is the batten spacing centre to centre; \
member_width_mm is the batten width (usually 50 to 100); rail_width_mm is the top rail width (usually 70 to 140).
- shaker_panel: flat stiles and rails that frame recessed panels. \
height_mm is floor to the top of the top rail; target_spacing_mm is the clear width of each panel; \
member_width_mm is the stile width (usually 70 to 110); rail_width_mm is the rail width (usually 70 to 140).

The layout software spaces everything evenly and works around doors, windows and outlets, \
so give a target spacing rather than trying to place individual pieces.

Judge from the photo: the room's character and existing trim, the ceiling height, and where \
window sills, outlets and switches fall. Heights near a third or two thirds of the ceiling height \
usually look right; avoid a height that lands awkwardly close to a sill or a row of switches. \
The treatment must sit above the baseboard and below the ceiling.

For each treatment give a short title and a rationale of one or two sentences that refers to \
something you can actually see in the photo. In observations, say in a sentence or two what you \
noticed about the room that shaped your suggestions. If the photo does not show a wall, say so \
in observations and still give your best suggestions from the measurements alone.";

pub struct ClaudeSuggester {
    http: reqwest::Client,
    api_key: String,
}

impl ClaudeSuggester {
    pub fn new(api_key: String) -> Result<ClaudeSuggester, reqwest::Error> {
        let http = reqwest::Client::builder().timeout(TIMEOUT).build()?;
        Ok(ClaudeSuggester { http, api_key })
    }

    async fn request(&self, input: &SuggestInput) -> Result<RawSuggestions, SuggestError> {
        let failed =
            |context: &str, detail: String| SuggestError::Failed(format!("{context}: {detail}"));

        let response = self
            .http
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", FALLBACK_BETA)
            .json(&request_body(input))
            .send()
            .await
            .map_err(|error| failed("request", error.to_string()))?;

        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|error| failed("response body", error.to_string()))?;
        if !status.is_success() {
            // The API's error object says what was wrong; it holds no secrets.
            return Err(failed("api", format!("{status}: {}", body["error"])));
        }
        parse_response(&body)
    }
}

impl Suggester for ClaudeSuggester {
    fn suggest<'a>(
        &'a self,
        input: &'a SuggestInput,
    ) -> BoxFuture<'a, Result<RawSuggestions, SuggestError>> {
        Box::pin(self.request(input))
    }
}

fn request_body(input: &SuggestInput) -> Value {
    let image = base64::engine::general_purpose::STANDARD.encode(&input.image);
    json!({
        "model": MODEL,
        "max_tokens": MAX_TOKENS,
        "fallbacks": "default",
        "system": SYSTEM_PROMPT,
        "output_config": {
            // This model's default; stated so a change of model does not
            // silently change the depth of the answer.
            "effort": "medium",
            "format": { "type": "json_schema", "schema": schema() },
        },
        "messages": [{
            "role": "user",
            "content": [
                { "type": "image", "source": { "type": "base64", "media_type": input.media_type, "data": image } },
                { "type": "text", "text": input.brief },
            ],
        }],
    })
}

/// The shape of [`RawSuggestions`]. Structured outputs do not support numeric
/// ranges, so those are enforced after parsing.
fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["observations", "suggestions"],
        "properties": {
            "observations": { "type": "string" },
            "suggestions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": [
                        "title", "rationale", "style", "height_mm",
                        "target_spacing_mm", "member_width_mm", "rail_width_mm",
                    ],
                    "properties": {
                        "title": { "type": "string" },
                        "rationale": { "type": "string" },
                        "style": { "type": "string", "enum": ["picture_frame", "board_and_batten", "shaker_panel"] },
                        "height_mm": { "type": "number" },
                        "target_spacing_mm": { "type": "number" },
                        "member_width_mm": { "type": "number" },
                        "rail_width_mm": { "type": "number" },
                    },
                },
            },
        },
    })
}

/// Reads a Messages API response. The stop reason is checked before the
/// content: a refusal or a truncated answer does not follow the schema.
fn parse_response(body: &Value) -> Result<RawSuggestions, SuggestError> {
    match body["stop_reason"].as_str() {
        Some("end_turn") => {}
        Some("refusal") => return Err(SuggestError::Refused),
        other => {
            return Err(SuggestError::Failed(format!(
                "unexpected stop reason {other:?}"
            )));
        }
    }
    let text = body["content"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|block| block["type"] == "text")
        .and_then(|block| block["text"].as_str())
        .ok_or_else(|| SuggestError::Failed("the response had no text block".to_owned()))?;
    serde_json::from_str(text).map_err(|error| {
        SuggestError::Failed(format!("the answer did not match the schema: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wain_core::StyleKind;

    fn input() -> SuggestInput {
        SuggestInput {
            image: vec![0xFF, 0xD8, 0xFF, 0xE0],
            media_type: "image/jpeg",
            brief: "The wall is 3658 mm wide.".to_owned(),
        }
    }

    #[test]
    fn the_request_carries_the_photo_the_brief_and_the_schema() {
        let body = request_body(&input());
        assert_eq!(body["model"], "claude-opus-5-5");
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
        let content = &body["messages"][0]["content"];
        assert_eq!(content[0]["source"]["media_type"], "image/jpeg");
        assert_eq!(content[0]["source"]["data"], "/9j/4A==");
        assert_eq!(content[1]["text"], "The wall is 3658 mm wide.");
        // Thinking cannot be configured on this model, so nothing is sent.
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn every_object_in_the_schema_is_closed_and_fully_required() {
        // Structured outputs reject a schema that does not do both.
        fn check(node: &Value) {
            if node["type"] == "object" {
                assert_eq!(node["additionalProperties"], false);
                let required = node["required"].as_array().unwrap().len();
                assert_eq!(required, node["properties"].as_object().unwrap().len());
                node["properties"]
                    .as_object()
                    .unwrap()
                    .values()
                    .for_each(check);
            } else if node["type"] == "array" {
                check(&node["items"]);
            }
        }
        check(&schema());
    }

    #[test]
    fn parses_an_answer_that_follows_the_schema() {
        let answer = json!({
            "observations": "A bright room with a low window sill.",
            "suggestions": [{
                "title": "Classic board and batten",
                "rationale": "Clears the sill.",
                "style": "board_and_batten",
                "height_mm": 1370, "target_spacing_mm": 400,
                "member_width_mm": 65, "rail_width_mm": 90,
            }],
        });
        let body = json!({
            "stop_reason": "end_turn",
            // Thinking blocks come first and are skipped.
            "content": [
                { "type": "thinking", "thinking": "" },
                { "type": "text", "text": answer.to_string() },
            ],
        });
        let parsed = parse_response(&body).unwrap();
        assert_eq!(parsed.suggestions.len(), 1);
        assert_eq!(parsed.suggestions[0].style, StyleKind::BoardAndBatten);
        assert_eq!(parsed.suggestions[0].height_mm, 1370.0);
    }

    #[test]
    fn a_refusal_or_a_cut_off_answer_is_an_error() {
        let refusal = json!({ "stop_reason": "refusal", "content": [] });
        assert!(matches!(
            parse_response(&refusal),
            Err(SuggestError::Refused)
        ));
        let truncated = json!({ "stop_reason": "max_tokens", "content": [{ "type": "text", "text": "{\"obs" }] });
        assert!(matches!(
            parse_response(&truncated),
            Err(SuggestError::Failed(_))
        ));
        let empty = json!({ "stop_reason": "end_turn", "content": [] });
        assert!(matches!(
            parse_response(&empty),
            Err(SuggestError::Failed(_))
        ));
    }
}
