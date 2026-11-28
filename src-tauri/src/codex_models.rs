//! Codex CLI's own list of current models, so the start menu follows OpenAI's lineup without a
//! catalog update (ADR 0022). Codex fetches the list from OpenAI and keeps it in
//! `<CODEX_HOME>/models_cache.json` (`fetched_at`, `client_version`, `models` with `slug`,
//! `display_name`, `visibility` and `priority`; read from Codex CLI 0.171.0's file). Hangar only
//! reads that file: the models Codex lists (`visibility: "list"`), in Codex's order. Without a
//! readable file, or without a listed model in it, the catalog's list stays.
//!
//! Each model also says which reasoning efforts it takes (`supported_reasoning_levels`, a list
//! of `{effort, description}`) and which one Codex uses on its own (`default_reasoning_level`);
//! those become the model's `efforts` and `default_effort`. The descriptions stay out: they are
//! English only, the window names the levels in its own languages.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::start_model::{valid_effort, valid_id, ModelOption, MAX_EFFORTS, MAX_LABEL_CHARS};

/// The cache file's name in Codex's home.
const CACHE_FILE: &str = "models_cache.json";
/// Far above a real cache (about 40 kB with a dozen models).
const MAX_CACHE_BYTES: usize = 4 * 1024 * 1024;
/// Models offered at most; the menu stays short.
const MAX_MODELS: usize = 24;

#[derive(Deserialize)]
struct Cache {
    #[serde(default)]
    models: Vec<CachedModel>,
}

#[derive(Deserialize)]
struct CachedModel {
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    visibility: Option<String>,
    #[serde(default)]
    priority: Option<i64>,
    // Read loosely: a level of an unexpected shape costs that level, never the model list.
    #[serde(default)]
    supported_reasoning_levels: Option<serde_json::Value>,
    #[serde(default)]
    default_reasoning_level: Option<serde_json::Value>,
}

/// The model's efforts in Codex's order, valid and each once, and its default when that is one
/// of them.
fn efforts_of(
    levels: Option<serde_json::Value>,
    default: Option<serde_json::Value>,
) -> (Vec<String>, Option<String>) {
    let mut efforts: Vec<String> = Vec::new();
    let levels = levels
        .as_ref()
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for level in levels {
        if efforts.len() == MAX_EFFORTS {
            break;
        }
        let Some(effort) = level.get("effort").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if valid_effort(effort) && !efforts.iter().any(|known| known == effort) {
            efforts.push(effort.to_owned());
        }
    }
    let default = default
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .filter(|effort| efforts.iter().any(|known| known == effort))
        .map(str::to_owned);
    (efforts, default)
}

/// The listed models of the cache file in `codex_home`, in Codex's order; `None` without a
/// readable file or without a listed model with a valid id.
pub fn from_home(codex_home: &Path) -> Option<Vec<ModelOption>> {
    let bytes = crate::persist::read_regular_capped(&codex_home.join(CACHE_FILE), MAX_CACHE_BYTES)
        .ok()
        .flatten()?;
    parse(&bytes)
}

/// [`from_home`] for the bytes of the file.
fn parse(bytes: &[u8]) -> Option<Vec<ModelOption>> {
    let cache: Cache = serde_json::from_slice(bytes).ok()?;
    let mut listed: Vec<(i64, usize, CachedModel)> = cache
        .models
        .into_iter()
        .enumerate()
        .filter(|(_, model)| model.visibility.as_deref() == Some("list"))
        .map(|(index, model)| (model.priority.unwrap_or(i64::MAX), index, model))
        .collect();
    listed.sort_by_key(|(priority, index, _)| (*priority, *index));
    let mut models: Vec<ModelOption> = Vec::new();
    for (_, _, model) in listed {
        let Some(id) = model.slug.filter(|id| valid_id(id)) else {
            continue;
        };
        if models.iter().any(|known| known.id == id) {
            continue;
        }
        let label = model
            .display_name
            .map(|name| name.trim().to_owned())
            .filter(|name| {
                !name.is_empty()
                    && name.chars().count() <= MAX_LABEL_CHARS
                    && !name.chars().any(char::is_control)
            })
            .unwrap_or_else(|| id.clone());
        let (efforts, default_effort) = efforts_of(
            model.supported_reasoning_levels,
            model.default_reasoning_level,
        );
        models.push(ModelOption {
            id,
            label,
            efforts,
            default_effort,
        });
        if models.len() == MAX_MODELS {
            break;
        }
    }
    (!models.is_empty()).then_some(models)
}

/// Codex's home as a session of the profile would see it: `CODEX_HOME` from the profile's
/// environment or Hangar's, else `~/.codex` (`codex_shell_env::codex_home`). A relative
/// `CODEX_HOME` counts from the home folder here, since no session folder is known yet.
pub fn codex_home(profile_env: &std::collections::BTreeMap<String, String>) -> Option<PathBuf> {
    let home_var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let home = std::env::var_os(home_var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    let var = |name: &str| std::env::var(name).ok();
    crate::codex_shell_env::codex_home(profile_env, &var, Some(&home), &home)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    const CACHE: &str = r#"{
        "fetched_at": "2026-11-27T17:36:19Z",
        "client_version": "0.171.0",
        "models": [
            {"slug": "gpt-6.2-astra", "display_name": "GPT-6.2-Astra", "visibility": "list", "priority": 2},
            {"slug": "gpt-reserve", "display_name": "GPT-Reserve", "visibility": "hide", "priority": 1},
            {"slug": "gpt-6.2-sol", "display_name": "GPT-6.2-Sol", "visibility": "list", "priority": 1},
            {"slug": "-x", "display_name": "Bad", "visibility": "list", "priority": 3},
            {"slug": "gpt-6.2-sol", "display_name": "Again", "visibility": "list", "priority": 4},
            {"slug": "gpt-plain", "display_name": "line\nbreak", "visibility": "list"},
            {"display_name": "No slug", "visibility": "list", "priority": 0}
        ]
    }"#;

    #[test]
    fn the_listed_models_come_in_codexs_order_with_their_names() {
        let models = parse(CACHE.as_bytes()).unwrap();
        let shown: Vec<(&str, &str)> = models
            .iter()
            .map(|model| (model.id.as_str(), model.label.as_str()))
            .collect();
        // Hidden ones, invalid ids and repeats are left out; a broken name falls back to the id.
        assert_eq!(
            shown,
            [
                ("gpt-6.2-sol", "GPT-6.2-Sol"),
                ("gpt-6.2-astra", "GPT-6.2-Astra"),
                ("gpt-plain", "gpt-plain"),
            ]
        );
    }

    #[test]
    fn without_a_usable_file_the_catalog_stays() {
        assert_eq!(parse(b"not json"), None);
        assert_eq!(parse(br#"{"models": []}"#), None);
        assert_eq!(
            parse(br#"{"models": [{"slug": "a", "visibility": "hide"}]}"#),
            None
        );
        let dir = TempDir::new("codex-models");
        assert_eq!(from_home(dir.path()), None);
        std::fs::write(dir.path().join(CACHE_FILE), CACHE).unwrap();
        assert_eq!(from_home(dir.path()).map(|models| models.len()), Some(3));
    }

    #[test]
    fn each_model_brings_its_efforts_and_default() {
        // Shaped like Codex CLI 0.171.0's file: gpt-6-luna lists no ultra.
        let text = r#"{"models": [
            {"slug": "gpt-6.2-sol", "visibility": "list", "priority": 1,
             "default_reasoning_level": "low",
             "supported_reasoning_levels": [
                {"effort": "low", "description": "Fast responses with lighter reasoning"},
                {"effort": "medium", "description": "Balances speed"},
                {"effort": "high", "description": "Greater depth"},
                {"effort": "xhigh", "description": "Extra high"},
                {"effort": "max", "description": "Maximum"},
                {"effort": "ultra", "description": "Maximum with delegation"}
             ]},
            {"slug": "gpt-6-luna", "visibility": "list", "priority": 2,
             "default_reasoning_level": "medium",
             "supported_reasoning_levels": [
                {"effort": "low"}, {"effort": "medium"}, {"effort": "high"},
                {"effort": "xhigh"}, {"effort": "max"}
             ]},
            {"slug": "odd", "visibility": "list", "priority": 3,
             "default_reasoning_level": "ultra",
             "supported_reasoning_levels": [
                {"effort": "low"}, {"effort": "low"}, {"effort": "Bad Value"}, {"effort": 3},
                {"description": "no effort"}, "high", {"effort": "max"}
             ]},
            {"slug": "plain", "visibility": "list", "priority": 4,
             "supported_reasoning_levels": "none", "default_reasoning_level": 7}
        ]}"#;
        let models = parse(text.as_bytes()).unwrap();
        let shown: Vec<(&str, Vec<&str>, Option<&str>)> = models
            .iter()
            .map(|model| {
                (
                    model.id.as_str(),
                    model.efforts.iter().map(String::as_str).collect(),
                    model.default_effort.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                (
                    "gpt-6.2-sol",
                    vec!["low", "medium", "high", "xhigh", "max", "ultra"],
                    Some("low")
                ),
                (
                    "gpt-6-luna",
                    vec!["low", "medium", "high", "xhigh", "max"],
                    Some("medium")
                ),
                // Repeats and broken levels are left out; a default outside the list too.
                ("odd", vec!["low", "max"], None),
                // Levels of the wrong shape cost the levels, not the model.
                ("plain", vec![], None),
            ]
        );
    }

    #[test]
    fn a_long_list_is_cut() {
        let models: Vec<String> = (0..40)
            .map(|index| {
                format!(r#"{{"slug": "m{index}", "visibility": "list", "priority": {index}}}"#)
            })
            .collect();
        let text = format!(r#"{{"models": [{}]}}"#, models.join(","));
        let parsed = parse(text.as_bytes()).unwrap();
        assert_eq!(parsed.len(), MAX_MODELS);
        assert_eq!(parsed[0].id, "m0");
    }
}
