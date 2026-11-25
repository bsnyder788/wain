//! WebAssembly bindings for the wain layout engine.
//!
//! Structured values cross the boundary as JSON strings. The shapes are the
//! serde forms of the `wain-core` types, and the web app gets matching
//! TypeScript definitions generated from those same types, so the boundary
//! stays typed without a second, hand-written schema. Lengths are plain
//! numbers of nanometres on the JavaScript side.

use wain_core::{Design, Length, PlanError, Project, StyleKind, UnitPrefs, UnitSystem};
use wasm_bindgen::prelude::*;

fn from_json<T: serde::de::DeserializeOwned>(json: &str, what: &str) -> Result<T, JsError> {
    serde_json::from_str(json).map_err(|error| JsError::new(&format!("invalid {what}: {error}")))
}

fn to_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("engine types always serialize")
}

/// Lays out a project. Takes a `Project` as JSON and returns
/// `{"ok": Plan}` or `{"error": PlanError}` as JSON. Throws only if the
/// input is not a well-formed project at all.
#[wasm_bindgen]
pub fn plan(project_json: &str) -> Result<String, JsError> {
    let project: Project = from_json(project_json, "project")?;
    Ok(match wain_core::plan(&project) {
        Ok(plan) => format!(r#"{{"ok":{}}}"#, to_json(&plan)),
        Err(error) => format!(r#"{{"error":{}}}"#, to_json::<PlanError>(&error)),
    })
}

/// A one-wall starter project. `system` is `"imperial"` or `"metric"`.
#[wasm_bindgen]
pub fn starter_project(system: &str) -> Result<String, JsError> {
    let system: UnitSystem = from_json(&format!("\"{system}\""), "unit system")?;
    Ok(to_json(&Project::starter(system)))
}

/// Default dimensions for a style, e.g. `default_design("picture_frame",
/// "metric")`.
#[wasm_bindgen]
pub fn default_design(style: &str, system: &str) -> Result<String, JsError> {
    let style: StyleKind = from_json(&format!("\"{style}\""), "style")?;
    let system: UnitSystem = from_json(&format!("\"{system}\""), "unit system")?;
    Ok(to_json(&Design::default_for(style, system)))
}

/// Parses what a user typed into nanometres. A bare number is read in the
/// given unit system; an explicit unit always wins.
#[wasm_bindgen]
pub fn parse_length(input: &str, system: &str) -> Result<f64, JsError> {
    let system: UnitSystem = from_json(&format!("\"{system}\""), "unit system")?;
    wain_core::parse_length(input, system)
        .map(|length| length.nm() as f64)
        .map_err(|error| JsError::new(&error.to_string()))
}

/// Formats nanometres for display using a `UnitPrefs` given as JSON.
#[wasm_bindgen]
pub fn format_length(nm: f64, prefs_json: &str) -> Result<String, JsError> {
    let prefs: UnitPrefs = from_json(prefs_json, "unit preferences")?;
    if !nm.is_finite() {
        return Err(JsError::new("length is not a finite number"));
    }
    Ok(wain_core::format_length(
        Length::from_nm(nm.round() as i64),
        &prefs,
    ))
}
