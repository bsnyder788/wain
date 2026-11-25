//! AI design suggestions.
//!
//! The model looks at a photo of the wall and proposes a few treatments, each
//! picked from the styles the engine knows how to build. It never decides
//! geometry: its numbers become a [`Design`], and the engine lays that out
//! and validates it like any design the user made by hand. A suggestion the
//! engine cannot build is dropped.

mod claude;

use axum::Json;
use axum::extract::{Multipart, State};
use serde::{Deserialize, Serialize};
use wain_core::{
    Design, Length, ObstructionKind, Project, Style, StyleKind, UnitSystem, format_length,
};

pub use claude::ClaudeSuggester;

use crate::AppState;
use crate::auth::CurrentUser;
use crate::credits::{self, SUGGESTION_COST};
use crate::error::ApiError;
use crate::mail::BoxFuture;

/// Largest photo accepted. The web app scales photos well below this.
const MAX_IMAGE_BYTES: usize = 6 * 1024 * 1024;
const MAX_SUGGESTIONS: usize = 4;

/// What the model is given.
pub struct SuggestInput {
    pub image: Vec<u8>,
    pub media_type: &'static str,
    /// The wall's measurements and contents, in words.
    pub brief: String,
}

/// What the model sends back, before any of it is trusted.
#[derive(Debug, Deserialize)]
pub struct RawSuggestions {
    pub observations: String,
    pub suggestions: Vec<RawSuggestion>,
}

#[derive(Debug, Deserialize)]
pub struct RawSuggestion {
    pub title: String,
    pub rationale: String,
    pub style: StyleKind,
    /// Floor to the top of the treatment.
    pub height_mm: f64,
    /// Box width, batten spacing on centre, or panel width, by style.
    pub target_spacing_mm: f64,
    /// Face width of the moulding, batten or stile.
    pub member_width_mm: f64,
    /// Face width of the chair rail or top rail.
    pub rail_width_mm: f64,
}

#[derive(Debug)]
pub enum SuggestError {
    /// The model declined to answer.
    Refused,
    /// The request could not be completed; the message is for the log.
    Failed(String),
}

/// Something that can propose designs from a photo. A trait so tests can run
/// without calling a real model.
pub trait Suggester: Send + Sync {
    fn suggest<'a>(
        &'a self,
        input: &'a SuggestInput,
    ) -> BoxFuture<'a, Result<RawSuggestions, SuggestError>>;
}

/// Development only: answers with fixed suggestions and calls no model, so
/// the feature can be exercised without an API key or any cost.
pub struct CannedSuggester;

impl Suggester for CannedSuggester {
    fn suggest<'a>(
        &'a self,
        _input: &'a SuggestInput,
    ) -> BoxFuture<'a, Result<RawSuggestions, SuggestError>> {
        let suggestion =
            |title: &str, style, height_mm, target_spacing_mm, member_width_mm| RawSuggestion {
                title: title.to_owned(),
                rationale: "A sample suggestion. No AI looked at your photo.".to_owned(),
                style,
                height_mm,
                target_spacing_mm,
                member_width_mm,
                rail_width_mm: 90.0,
            };
        Box::pin(async move {
            Ok(RawSuggestions {
                observations: "These are sample suggestions from the development server."
                    .to_owned(),
                suggestions: vec![
                    suggestion(
                        "Tall board and batten",
                        StyleKind::BoardAndBatten,
                        1500.0,
                        450.0,
                        65.0,
                    ),
                    suggestion(
                        "Classic picture frames",
                        StyleKind::PictureFrame,
                        900.0,
                        600.0,
                        30.0,
                    ),
                    suggestion(
                        "Low shaker panels",
                        StyleKind::ShakerPanel,
                        1000.0,
                        500.0,
                        90.0,
                    ),
                ],
            })
        })
    }
}

/// One proposed design, ready to apply to the project.
#[derive(Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Suggestion {
    pub title: String,
    /// Why this suits the room, in a sentence or two.
    pub rationale: String,
    pub design: Design,
}

#[derive(Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SuggestionsResponse {
    /// What the model noticed about the room.
    pub observations: String,
    pub suggestions: Vec<Suggestion>,
    /// The account's balance after this request.
    pub credits: i32,
}

/// `POST /suggestions`: multipart with `project` (JSON), `wall` (index) and
/// `image` (the photo of that wall). Costs [`SUGGESTION_COST`] credits, which
/// are returned if nothing usable comes back.
pub async fn create_suggestions(
    State(state): State<AppState>,
    user: CurrentUser,
    multipart: Multipart,
) -> Result<Json<SuggestionsResponse>, ApiError> {
    let suggester = state.suggester.clone().ok_or(ApiError::Unavailable(
        "AI suggestions are not set up on this server.",
    ))?;
    let (project, wall_index, image) = read_form(multipart).await?;

    // Everything is checked before the user is charged.
    wain_core::plan(&project).map_err(|invalid| ApiError::InvalidProject(invalid.issues))?;
    let wall = project
        .walls
        .get(wall_index)
        .ok_or_else(|| ApiError::BadRequest("That wall is not in the project.".to_owned()))?;
    let media_type = image_type(&image)
        .ok_or_else(|| ApiError::BadRequest("The photo must be a JPEG, PNG or WebP.".to_owned()))?;
    let input = SuggestInput {
        image,
        media_type,
        brief: brief(&project, wall),
    };

    let charge = credits::spend(&state.db, user.id, SUGGESTION_COST, "suggestion").await?;
    let outcome = match suggester.suggest(&input).await {
        Ok(raw) => {
            let suggestions = usable(&raw, &project);
            if suggestions.is_empty() {
                Err(ApiError::Upstream(
                    "No workable designs came back for this wall. You were not charged.",
                ))
            } else {
                Ok((raw.observations, suggestions))
            }
        }
        Err(SuggestError::Refused) => Err(ApiError::Upstream(
            "The AI could not analyse this photo. You were not charged.",
        )),
        Err(SuggestError::Failed(error)) => {
            tracing::error!(%error, "suggestion request failed");
            Err(ApiError::Upstream(
                "The AI service did not respond. You were not charged; try again shortly.",
            ))
        }
    };

    match outcome {
        Ok((observations, suggestions)) => Ok(Json(SuggestionsResponse {
            observations: clip(&observations, 600),
            suggestions,
            credits: credits::balance(&state.db, user.id).await?,
        })),
        Err(error) => {
            credits::refund(&state.db, charge).await?;
            Err(error)
        }
    }
}

async fn read_form(mut multipart: Multipart) -> Result<(Project, usize, Vec<u8>), ApiError> {
    let bad = |message: &str| ApiError::BadRequest(message.to_owned());
    let (mut project, mut wall, mut image) = (None, None, None);

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| bad("The upload could not be read."))?
    {
        let name = field.name().unwrap_or_default().to_owned();
        let bytes = field
            .bytes()
            .await
            .map_err(|_| bad("The upload could not be read, or the photo is too large."))?;
        match name.as_str() {
            "project" => {
                let parsed: Project = serde_json::from_slice(&bytes)
                    .map_err(|error| ApiError::BadRequest(format!("invalid project: {error}")))?;
                project = Some(parsed);
            }
            "wall" => {
                let index = std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|text| text.trim().parse().ok());
                wall = Some(index.ok_or_else(|| bad("`wall` must be a wall index."))?);
            }
            "image" => {
                if bytes.len() > MAX_IMAGE_BYTES {
                    return Err(bad("The photo is too large."));
                }
                image = Some(bytes.to_vec());
            }
            _ => {}
        }
    }

    match (project, wall, image) {
        (Some(project), Some(wall), Some(image)) => Ok((project, wall, image)),
        _ => Err(bad("`project`, `wall` and `image` are all required.")),
    }
}

/// Identifies an image by its leading bytes, never by what the client claims.
fn image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Describes the wall for the model. Measurements go in millimetres, which is
/// also the unit it answers in, whatever the user prefers to read.
fn brief(project: &Project, wall: &wain_core::Wall) -> String {
    let mm = |length: Length| (length.as_mm()).round() as i64;
    let mut text = format!(
        "The wall in the photo is {} mm wide and the ceiling is {} mm high (measured with a tape).\n",
        mm(wall.width),
        mm(wall.height),
    );

    let baseboard = &project.design.baseboard;
    text += &format!(
        "The baseboard is {} mm tall and is {}.\n",
        mm(baseboard.height),
        if baseboard.existing {
            "staying"
        } else {
            "being replaced"
        },
    );

    if wall.obstructions.is_empty() {
        text += "Nothing has been marked on the wall.\n";
    } else {
        text += "Marked on the wall (left edge from the wall's left end, bottom edge above the floor):\n";
        for obstruction in &wall.obstructions {
            let kind = match obstruction.kind {
                ObstructionKind::Door => "door",
                ObstructionKind::Opening => "opening",
                ObstructionKind::Window => "window",
                ObstructionKind::Outlet => "outlet",
                ObstructionKind::Switch => "switch",
                ObstructionKind::Vent => "vent",
                ObstructionKind::Other => "other fixture",
            };
            let rect = &obstruction.rect;
            text += &format!(
                "- {kind}: left {} mm, bottom {} mm, {} mm wide, {} mm tall\n",
                mm(rect.x),
                mm(rect.y),
                mm(rect.width),
                mm(rect.height),
            );
        }
    }

    let example = format_length(Length::mm(900), &project.units);
    text += &format!(
        "When you mention a size in the title, rationale or observations, write it the way this \
         user reads sizes, for example {example}.\n"
    );
    text
}

/// Turns the model's answers into designs and keeps the ones the engine can
/// actually build on this project.
fn usable(raw: &RawSuggestions, project: &Project) -> Vec<Suggestion> {
    raw.suggestions
        .iter()
        .filter_map(|suggestion| {
            let design = to_design(suggestion, &project.design, project.units.system)?;
            let mut trial = project.clone();
            trial.design = design.clone();
            wain_core::plan(&trial).ok()?;
            Some(Suggestion {
                title: clip(&suggestion.title, 80),
                rationale: clip(&suggestion.rationale, 400),
                design,
            })
        })
        .take(MAX_SUGGESTIONS)
        .collect()
}

/// Builds a design from a suggestion: the style's defaults with the model's
/// four numbers applied, rounded to sizes a person would choose. The user's
/// baseboard is kept as it is.
fn to_design(raw: &RawSuggestion, current: &Design, system: UnitSystem) -> Option<Design> {
    let (coarse, fine) = match system {
        UnitSystem::Imperial => (Length::inch_fraction(1, 2), Length::inch_fraction(1, 4)),
        UnitSystem::Metric => (Length::mm(10), Length::mm(5)),
    };
    let length = |mm: f64, step: Length| -> Option<Length> {
        // Anything outside this is nonsense and is caught before rounding.
        (mm.is_finite() && (1.0..=10_000.0).contains(&mm))
            .then(|| Length::from_nm((mm * 1e6).round() as i64).round_to(step))
    };
    let height = length(raw.height_mm, coarse)?;
    let spacing = length(raw.target_spacing_mm, coarse)?;
    let member = length(raw.member_width_mm, fine)?;
    let rail = length(raw.rail_width_mm, fine)?;

    let mut design = Design::default_for(raw.style, system);
    design.baseboard = current.baseboard;
    match &mut design.style {
        Style::PictureFrame(style) => {
            style.chair_rail_height = height;
            style.target_box_width = spacing;
            style.moulding_width = member;
            style.chair_rail_width = rail;
        }
        Style::BoardAndBatten(style) => {
            style.height = height;
            style.target_spacing = spacing;
            style.batten_width = member;
            style.top_rail_width = rail;
        }
        Style::ShakerPanel(style) => {
            style.height = height;
            style.target_panel_width = spacing;
            style.stile_width = member;
            style.top_rail_width = rail;
            style.bottom_rail_width = rail;
        }
    }
    Some(design)
}

/// Shortens text to at most `limit` characters, on a character boundary.
fn clip(text: &str, limit: usize) -> String {
    let text = text.trim();
    match text.char_indices().nth(limit) {
        Some((end, _)) => format!("{}…", text[..end].trim_end()),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(style: StyleKind, height_mm: f64) -> RawSuggestion {
        RawSuggestion {
            title: "Title".into(),
            rationale: "Because.".into(),
            style,
            height_mm,
            target_spacing_mm: 412.0,
            member_width_mm: 63.0,
            rail_width_mm: 89.0,
        }
    }

    #[test]
    fn suggestions_round_to_sizes_a_person_would_pick() {
        let current = Design::default_for(StyleKind::PictureFrame, UnitSystem::Imperial);
        let design = to_design(
            &raw(StyleKind::BoardAndBatten, 1380.0),
            &current,
            UnitSystem::Imperial,
        )
        .unwrap();
        let Style::BoardAndBatten(style) = design.style else {
            panic!("wrong style");
        };
        // 1380 mm is 54.33": the nearest half inch.
        assert_eq!(style.height, Length::inch_fraction(109, 2));
        assert_eq!(style.target_spacing, Length::inch_fraction(32, 2));
        assert_eq!(style.batten_width, Length::inch_fraction(10, 4));
        assert_eq!(style.top_rail_width, Length::inch_fraction(14, 4));
        // The user's own baseboard is untouched.
        assert_eq!(design.baseboard, current.baseboard);

        let metric = to_design(
            &raw(StyleKind::ShakerPanel, 1003.0),
            &current,
            UnitSystem::Metric,
        )
        .unwrap();
        let Style::ShakerPanel(style) = metric.style else {
            panic!("wrong style");
        };
        assert_eq!(style.height, Length::mm(1000));
        assert_eq!(style.stile_width, Length::mm(65));
        assert_eq!(style.bottom_rail_width, Length::mm(90));
    }

    #[test]
    fn nonsense_numbers_are_rejected() {
        let current = Design::default_for(StyleKind::PictureFrame, UnitSystem::Metric);
        for bad in [f64::NAN, f64::INFINITY, -5.0, 0.0, 1e12] {
            assert!(
                to_design(
                    &raw(StyleKind::PictureFrame, bad),
                    &current,
                    UnitSystem::Metric
                )
                .is_none()
            );
        }
    }

    #[test]
    fn suggestions_the_engine_cannot_build_are_dropped() {
        let project = Project::starter(UnitSystem::Imperial);
        let suggestions = RawSuggestions {
            observations: String::new(),
            suggestions: vec![
                raw(StyleKind::BoardAndBatten, 1370.0),
                // Taller than the 8' ceiling.
                raw(StyleKind::BoardAndBatten, 3000.0),
                // So low the rail would sit inside the baseboard.
                raw(StyleKind::ShakerPanel, 150.0),
                raw(StyleKind::PictureFrame, 900.0),
            ],
        };
        let kept: Vec<StyleKind> = usable(&suggestions, &project)
            .iter()
            .map(|suggestion| suggestion.design.style.kind())
            .collect();
        assert_eq!(kept, [StyleKind::BoardAndBatten, StyleKind::PictureFrame]);
    }

    #[test]
    fn the_brief_describes_the_wall_in_millimetres() {
        let mut project = Project::starter(UnitSystem::Imperial);
        project.walls[0].obstructions.push(wain_core::Obstruction {
            kind: ObstructionKind::Window,
            rect: wain_core::Rect::new(
                Length::inches(50),
                Length::inches(36),
                Length::inches(40),
                Length::inches(48),
            ),
        });
        let text = brief(&project, &project.walls[0]);
        assert!(
            text.contains("3658 mm wide and the ceiling is 2438 mm high"),
            "{text}"
        );
        assert!(
            text.contains("- window: left 1270 mm, bottom 914 mm, 1016 mm wide, 1219 mm tall"),
            "{text}"
        );
        // An imperial user is addressed in inches.
        assert!(text.contains("for example 35 7/16\""), "{text}");
    }

    #[test]
    fn images_are_identified_by_their_bytes() {
        assert_eq!(
            image_type(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]),
            Some("image/jpeg")
        );
        assert_eq!(image_type(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(image_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(image_type(b"<svg onload=alert(1)>"), None);
        assert_eq!(image_type(b""), None);
    }

    #[test]
    fn long_text_is_clipped_on_a_character_boundary() {
        assert_eq!(clip("  short  ", 10), "short");
        assert_eq!(clip("héllo wörld", 5), "héllo…");
    }
}
