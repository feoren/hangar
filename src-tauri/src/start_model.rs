//! The model a session starts with (ADR 0022): one of the vendor's models from the catalog
//! (`models`, `model_args`) or a model of a local LLM server (`local`, Ollama or LM Studio).
//!
//! The window names only the source and the model id; the arguments and variables come from the
//! catalog or the user's profile, never from the window. The id is checked here before anything
//! starts, and everything it is placed into must pass a cmd.exe launcher ([`crate::batch`]).

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Deserializer, Serialize};

/// The placeholder for the model id, in `model_args` and in `local.*.args` and `env` values.
pub const MODEL: &str = "{model}";
/// The placeholder for the local server's base URL, in `local.*.args` and `env` values.
pub const BASE: &str = "{base}";
/// Longest model id, in characters.
pub const MAX_ID_CHARS: usize = 160;
/// Longest label of a vendor model in the catalog, in characters.
pub const MAX_LABEL_CHARS: usize = 80;

/// A vendor model the menu offers: `id` goes into `{model}`, `label` is shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelOption {
    pub id: String,
    pub label: String,
}

/// How a program runs against one local server.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalLaunch {
    /// Placed after the profile's arguments; must hold `{model}`.
    #[serde(default)]
    pub args: Vec<String>,
    /// Applied after the profile's variables and before its secrets. An empty value sets the
    /// variable empty.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// `local = { ollama = {...}, lmstudio = {...} }` of a catalog entry or profile.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalSpecs {
    #[serde(default)]
    pub ollama: Option<LocalLaunch>,
    #[serde(default)]
    pub lmstudio: Option<LocalLaunch>,
}

impl LocalSpecs {
    pub fn get(&self, server: LocalServer) -> Option<&LocalLaunch> {
        match server {
            LocalServer::Ollama => self.ollama.as_ref(),
            LocalServer::Lmstudio => self.lmstudio.as_ref(),
        }
    }

    /// The servers this program can run against, in menu order.
    pub fn servers(&self) -> Vec<LocalServer> {
        LocalServer::ALL
            .into_iter()
            .filter(|server| self.get(*server).is_some())
            .collect()
    }
}

/// A local LLM server Hangar knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LocalServer {
    Ollama,
    Lmstudio,
}

impl LocalServer {
    pub const ALL: [Self; 2] = [Self::Ollama, Self::Lmstudio];

    /// The server's base URL on this machine, the value of `{base}`. Loopback only.
    pub fn base(self) -> &'static str {
        match self {
            Self::Ollama => "http://127.0.0.1:11434",
            Self::Lmstudio => "http://127.0.0.1:1234",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Ollama => "Ollama",
            Self::Lmstudio => "LM Studio",
        }
    }
}

/// Where the chosen model comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelSource {
    /// One of the program's vendor models (`models`).
    Provider,
    Ollama,
    Lmstudio,
}

impl ModelSource {
    pub fn server(self) -> Option<LocalServer> {
        match self {
            Self::Provider => None,
            Self::Ollama => Some(LocalServer::Ollama),
            Self::Lmstudio => Some(LocalServer::Lmstudio),
        }
    }
}

/// The model a session starts with: `create_session`'s `model` argument, kept with the session
/// for restarts, restores and the recent sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartModel {
    pub source: ModelSource,
    pub id: String,
}

/// What a model adds to one process start.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelLaunch {
    /// Placed after the start arguments, before the sub-agent and first prompt arguments.
    pub args: Vec<String>,
    /// Applied after the profile's variables, before its secrets.
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error(
        "The model id must be 1 to 160 characters from A-Z a-z 0-9 . _ : / @ + - [ ] and must not \
         start with '-'"
    )]
    InvalidId,
    #[error("{program} offers no model '{id}'")]
    NotOffered { program: String, id: String },
    #[error("{program} cannot run against {server}")]
    NoLocal {
        program: String,
        server: &'static str,
    },
    #[error("The model's arguments for {0} would not pass its cmd.exe launcher")]
    UnsafeArgument(String),
}

/// A model id Hangar passes on: 1 to [`MAX_ID_CHARS`] characters from
/// `A-Z a-z 0-9 . _ : / @ + - [ ]`, no leading `-` (it would be read as an option).
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= MAX_ID_CHARS
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:/@+-[]".contains(c))
}

/// Does `text` hold only the placeholders in `allowed`? Any other `{...}`, or a `{` without its
/// `}`, is refused: a typo such as `{modle}` would otherwise reach the program verbatim.
fn placeholders_ok(text: &str, allowed: &[&str]) -> bool {
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let tail = &rest[start..];
        let Some(end) = tail.find('}') else {
            return false;
        };
        if !allowed.contains(&&tail[..=end]) {
            return false;
        }
        rest = &tail[end + 1..];
    }
    true
}

/// Checks `models`, `model_args` and `local` of a catalog entry or profile at load.
/// `need_model_args`: `models` without `model_args` is an error (the catalog); a user profile may
/// leave `model_args` to the catalog entry it replaces.
pub fn check(
    models: Option<&[ModelOption]>,
    model_args: Option<&[String]>,
    local: Option<&LocalSpecs>,
    need_model_args: bool,
) -> Result<(), &'static str> {
    let models = models.unwrap_or_default();
    let mut seen = HashSet::new();
    for model in models {
        if !valid_id(&model.id) {
            return Err("models: an id must be 1 to 160 characters from A-Z a-z 0-9 . _ : / @ + - [ ] without a leading -");
        }
        if !seen.insert(model.id.as_str()) {
            return Err("models: an id appears more than once");
        }
        let label = model.label.trim();
        if label.is_empty()
            || model.label.chars().count() > MAX_LABEL_CHARS
            || model.label.chars().any(char::is_control)
        {
            return Err("models: a label must be 1 to 80 characters without control characters");
        }
    }
    let model_args = model_args.filter(|args| !args.is_empty());
    if need_model_args && !models.is_empty() && model_args.is_none() {
        return Err("model_args is required when models is set");
    }
    if let Some(args) = model_args {
        if !args.iter().any(|arg| arg.contains(MODEL)) {
            return Err("model_args needs the placeholder {model}");
        }
        if !args.iter().all(|arg| placeholders_ok(arg, &[MODEL])) {
            return Err("model_args may only use the placeholder {model}");
        }
    }
    let Some(local) = local else {
        return Ok(());
    };
    for launch in [&local.ollama, &local.lmstudio].into_iter().flatten() {
        if !launch.args.iter().any(|arg| arg.contains(MODEL)) {
            return Err("local: args need the placeholder {model}");
        }
        let texts = launch.args.iter().chain(launch.env.values());
        if !texts
            .into_iter()
            .all(|text| placeholders_ok(text, &[MODEL, BASE]))
        {
            return Err(
                "local: args and env values may only use the placeholders {model} and {base}",
            );
        }
        if !launch
            .env
            .keys()
            .all(|key| crate::secrets::is_valid_name(key))
        {
            return Err("local: env names may only contain letters, digits and _");
        }
        if launch.env.values().any(|value| value.contains('\0')) {
            return Err("local: env values must not contain NUL");
        }
    }
    Ok(())
}

/// What the program of a catalog entry or profile offers for a model choice.
#[derive(Debug, Clone, Copy)]
pub struct ModelSpec<'a> {
    pub program: &'a str,
    pub models: &'a [ModelOption],
    pub model_args: Option<&'a [String]>,
    pub local: Option<&'a LocalSpecs>,
}

impl ModelSpec<'_> {
    /// The vendor models the menu offers: none without `model_args`, since they could not start.
    pub fn offered(&self) -> &[ModelOption] {
        match self.model_args {
            Some(args) if !args.is_empty() => self.models,
            _ => &[],
        }
    }
}

fn expand(text: &str, model: &str, base: &str) -> String {
    // The id holds no `{`, so a replaced value never forms a new placeholder.
    text.replace(MODEL, model).replace(BASE, base)
}

/// Checks `choice` against `spec` and returns what it adds to a start. Every expanded argument
/// must pass a cmd.exe launcher ([`crate::batch::argument_ok`]), whatever the program is: the
/// same choice must start the same way on every machine.
pub fn launch(spec: ModelSpec<'_>, choice: &StartModel) -> Result<ModelLaunch, ModelError> {
    if !valid_id(&choice.id) {
        return Err(ModelError::InvalidId);
    }
    let launch = match choice.source.server() {
        None => {
            let offered = spec.offered();
            if !offered.iter().any(|model| model.id == choice.id) {
                return Err(ModelError::NotOffered {
                    program: spec.program.to_owned(),
                    id: choice.id.clone(),
                });
            }
            let args = spec.model_args.unwrap_or_default();
            ModelLaunch {
                args: args.iter().map(|arg| expand(arg, &choice.id, "")).collect(),
                env: Vec::new(),
            }
        }
        Some(server) => {
            let local = spec
                .local
                .and_then(|local| local.get(server))
                .ok_or_else(|| ModelError::NoLocal {
                    program: spec.program.to_owned(),
                    server: server.name(),
                })?;
            let base = server.base();
            ModelLaunch {
                args: local
                    .args
                    .iter()
                    .map(|arg| expand(arg, &choice.id, base))
                    .collect(),
                env: local
                    .env
                    .iter()
                    .map(|(name, value)| (name.clone(), expand(value, &choice.id, base)))
                    .collect(),
            }
        }
    };
    if !launch.args.iter().all(|arg| crate::batch::argument_ok(arg)) {
        return Err(ModelError::UnsafeArgument(spec.program.to_owned()));
    }
    Ok(launch)
}

/// Checks a stored choice (workspace, recent sessions) for its shape only; whether the program
/// still offers it is checked when the session starts again.
pub fn stored_ok(choice: &StartModel) -> bool {
    valid_id(&choice.id)
}

/// `deserialize_with` for a stored choice (`SavedSession.model`, `ClosedSession.model`): a value
/// this Hangar cannot use (an unknown source, an extra field, a bad id) costs only the model,
/// never the session or the whole file. The drop goes to the log, the value does not.
pub fn lenient_stored<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<StartModel>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.is_null() {
        return Ok(None);
    }
    match serde_json::from_value::<StartModel>(value) {
        Ok(choice) if stored_ok(&choice) => Ok(Some(choice)),
        _ => {
            eprintln!("hangar: dropping a stored model choice that is not valid");
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn option(id: &str) -> ModelOption {
        ModelOption {
            id: id.into(),
            label: id.to_uppercase(),
        }
    }

    fn local() -> LocalSpecs {
        LocalSpecs {
            ollama: Some(LocalLaunch {
                args: strings(&["--model", "{model}"]),
                env: [
                    ("ANTHROPIC_BASE_URL".to_owned(), "{base}".to_owned()),
                    ("ANTHROPIC_API_KEY".to_owned(), String::new()),
                ]
                .into(),
            }),
            lmstudio: None,
        }
    }

    fn choice(source: ModelSource, id: &str) -> StartModel {
        StartModel {
            source,
            id: id.into(),
        }
    }

    #[test]
    fn ids_follow_the_rule() {
        for good in [
            "opus",
            "gpt-6.2-sol",
            "qwen3:8b",
            "hf.co/org/model:Q4_K_M",
            "user@host+x",
            "model[1m]",
            &"a".repeat(160),
        ] {
            assert!(valid_id(good), "{good}");
        }
        for bad in [
            "",
            "-rf",
            "--model",
            "a b",
            "a&b",
            "a%b",
            "a\"b",
            "a{model}",
            "a(b)",
            "ä",
            "a\nb",
            &"a".repeat(161),
        ] {
            assert!(!valid_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn placeholders_are_only_model_and_base() {
        assert!(placeholders_ok("--model={model}", &[MODEL]));
        assert!(placeholders_ok("plain }", &[MODEL]));
        assert!(!placeholders_ok("{base}/v1", &[MODEL]));
        assert!(placeholders_ok("{base}/v1", &[MODEL, BASE]));
        assert!(!placeholders_ok("{modle}", &[MODEL, BASE]));
        assert!(!placeholders_ok("{model", &[MODEL]));
    }

    #[test]
    fn the_catalog_fields_are_checked() {
        let args = strings(&["--model", "{model}"]);
        let models = [option("opus"), option("sonnet")];
        assert_eq!(
            check(Some(&models), Some(&args), Some(&local()), true),
            Ok(())
        );
        assert_eq!(check(None, None, None, true), Ok(()));
        // models need model_args in the catalog, not in a profile.
        assert!(check(Some(&models), None, None, true).is_err());
        assert_eq!(check(Some(&models), None, None, false), Ok(()));
        // model_args without {model}, or with another placeholder.
        assert!(check(Some(&models), Some(&strings(&["--model"])), None, true).is_err());
        assert!(check(None, Some(&strings(&["{model}", "{base}"])), None, true).is_err());
        // Bad ids, duplicates, bad labels.
        assert!(check(Some(&[option("-x")]), Some(&args), None, true).is_err());
        assert!(check(Some(&[option("a"), option("a")]), Some(&args), None, true).is_err());
        let blank = ModelOption {
            id: "a".into(),
            label: " ".into(),
        };
        assert!(check(Some(&[blank]), Some(&args), None, true).is_err());
        // local: {model} required, names valid, placeholders known.
        let mut bad = local();
        bad.ollama.as_mut().unwrap().args = strings(&["--oss"]);
        assert!(check(None, None, Some(&bad), true).is_err());
        let mut bad = local();
        bad.ollama
            .as_mut()
            .unwrap()
            .env
            .insert("BAD-NAME".into(), "x".into());
        assert!(check(None, None, Some(&bad), true).is_err());
        let mut bad = local();
        bad.ollama
            .as_mut()
            .unwrap()
            .env
            .insert("URL".into(), "{host}".into());
        assert!(check(None, None, Some(&bad), true).is_err());
        let mut bad = local();
        bad.ollama
            .as_mut()
            .unwrap()
            .env
            .insert("X".into(), "a\0b".into());
        assert!(check(None, None, Some(&bad), true).is_err());
    }

    fn make_spec<'a>(
        models: &'a [ModelOption],
        args: Option<&'a [String]>,
        local: Option<&'a LocalSpecs>,
    ) -> ModelSpec<'a> {
        ModelSpec {
            program: "Claude Code",
            models,
            model_args: args,
            local,
        }
    }

    #[test]
    fn a_vendor_model_must_be_offered() {
        let args = strings(&["--model={model}"]);
        let models = [option("opus")];
        let spec = make_spec(&models, Some(&args), None);
        let launch = launch(spec, &choice(ModelSource::Provider, "opus")).unwrap();
        assert_eq!(launch.args, strings(&["--model=opus"]));
        assert!(launch.env.is_empty());
        assert!(matches!(
            super::launch(spec, &choice(ModelSource::Provider, "gpt")),
            Err(ModelError::NotOffered { .. })
        ));
        assert_eq!(
            super::launch(spec, &choice(ModelSource::Provider, "-opus")),
            Err(ModelError::InvalidId)
        );
        // Without model_args nothing is offered.
        let bare = make_spec(&models, None, None);
        assert!(bare.offered().is_empty());
        assert!(super::launch(bare, &choice(ModelSource::Provider, "opus")).is_err());
    }

    #[test]
    fn a_local_model_needs_the_local_entry() {
        let local = local();
        let spec = make_spec(&[], None, Some(&local));
        let launch = launch(spec, &choice(ModelSource::Ollama, "qwen3:8b")).unwrap();
        assert_eq!(launch.args, strings(&["--model", "qwen3:8b"]));
        assert_eq!(
            launch.env,
            vec![
                ("ANTHROPIC_API_KEY".to_owned(), String::new()),
                (
                    "ANTHROPIC_BASE_URL".to_owned(),
                    "http://127.0.0.1:11434".to_owned()
                ),
            ]
        );
        assert_eq!(
            super::launch(spec, &choice(ModelSource::Lmstudio, "qwen3:8b")),
            Err(ModelError::NoLocal {
                program: "Claude Code".into(),
                server: "LM Studio",
            })
        );
        let none = make_spec(&[], None, None);
        assert!(super::launch(none, &choice(ModelSource::Ollama, "qwen3:8b")).is_err());
    }

    #[test]
    fn an_argument_a_launcher_would_interpret_is_refused() {
        // The catalog could hold such an argument; the id itself never can.
        let local = LocalSpecs {
            ollama: None,
            lmstudio: Some(LocalLaunch {
                args: strings(&["--model", "{model}", "a(b)"]),
                env: BTreeMap::new(),
            }),
        };
        let spec = make_spec(&[], None, Some(&local));
        assert_eq!(
            launch(spec, &choice(ModelSource::Lmstudio, "m")),
            Err(ModelError::UnsafeArgument("Claude Code".into()))
        );
    }

    #[test]
    fn the_choice_serializes_as_the_window_sends_it() {
        let choice = choice(ModelSource::Lmstudio, "qwen/qwen3-8b");
        let json = serde_json::to_value(&choice).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "source": "lmstudio", "id": "qwen/qwen3-8b" })
        );
        let back: StartModel = serde_json::from_value(json).unwrap();
        assert_eq!(back, choice);
        assert!(serde_json::from_str::<StartModel>(r#"{"source":"cloud","id":"x"}"#).is_err());
        assert!(
            serde_json::from_str::<StartModel>(r#"{"source":"provider","id":"x","extra":1}"#)
                .is_err()
        );
    }
}
