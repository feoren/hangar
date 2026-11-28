//! Whether a local LLM server runs on this machine and which models it has (ADR 0022):
//! Ollama on port 11434 and LM Studio on port 1234, both on `127.0.0.1` only.
//!
//! One thread probes both servers every [`POLL_INTERVAL`] ([`SAVING_INTERVAL`] while the machine
//! runs on battery) and emits [`EVENT`] only when the state changed; `local_llm_status` returns
//! the last state, `local_llm_refresh` probes at once (the window calls it when it gains focus).
//!
//! The requests never leave the machine: loopback only, no proxy, no redirects, a short time
//! limit and a bounded body. The answers are untrusted input: parsed into a fixed shape, at most
//! [`MAX_MODELS`] models per server, ids that fail [`crate::start_model::valid_id`] dropped. The
//! frontend never talks to the servers itself; the CSP stays as it is.

use std::collections::HashSet;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::lock;
use crate::start_model::{valid_id, LocalServer};

/// Event with the new [`LocalLlmState`], sent only on a change.
pub const EVENT: &str = "local-llm-changed";
/// How often the thread probes on AC power.
pub const POLL_INTERVAL: Duration = Duration::from_secs(10);
/// How often the thread probes while the machine runs on battery.
pub const SAVING_INTERVAL: Duration = Duration::from_secs(30);
/// Time limit of one request, connecting included.
pub const REQUEST_LIMIT: Duration = Duration::from_millis(800);
/// Largest answer read; a longer one counts as no answer.
pub const BODY_LIMIT: usize = 1024 * 1024;
/// Most models kept per server.
pub const MAX_MODELS: usize = 200;
/// Longest label kept, in characters.
pub const MAX_LABEL_CHARS: usize = 160;
/// The thread checks for [`LocalLlmMonitor::stop`] this often while it sleeps.
const STOP_CHECK: Duration = Duration::from_millis(500);

/// One model of a local server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModel {
    pub id: String,
    pub label: String,
    /// In memory right now; `false` when the server cannot tell.
    pub loaded: bool,
}

/// One server: whether it answers, and its models.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerState {
    pub running: bool,
    pub models: Vec<LocalModel>,
}

/// What the window gets: `{ ollama: Server, lmstudio: Server }`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalLlmState {
    pub ollama: ServerState,
    pub lmstudio: ServerState,
}

/// The answer to one GET.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    /// A complete answer within [`BODY_LIMIT`].
    Answer { status: u16, body: Vec<u8> },
    /// An answer longer than [`BODY_LIMIT`].
    TooLarge,
    /// Nothing listens, or no answer within [`REQUEST_LIMIT`].
    Unreachable,
}

impl Fetched {
    /// The body of a successful answer.
    fn ok_body(&self) -> Option<&[u8]> {
        match self {
            Self::Answer { status, body } if (200..300).contains(status) => Some(body),
            _ => None,
        }
    }

    fn answered(&self) -> bool {
        !matches!(self, Self::Unreachable)
    }
}

/// How the probe reaches a server; the app's goes over HTTP ([`HttpFetch`]), tests answer from
/// fixtures.
pub trait Fetch: Send + Sync {
    /// GET of `path` (starting with `/`) on the server's base URL.
    fn get(&self, server: LocalServer, path: &str) -> Fetched;
}

/// A label for display: control characters dropped, trimmed, at most [`MAX_LABEL_CHARS`].
fn clean_label(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_LABEL_CHARS)
        .collect()
}

/// The array `key` of a JSON object.
fn array<'a>(body: &'a [u8], key: &str, keep: &'a mut Option<Value>) -> Option<&'a Vec<Value>> {
    *keep = Some(serde_json::from_slice(body).ok()?);
    keep.as_ref()?.get(key)?.as_array()
}

/// Models from entries in order: valid ids only, each once, at most [`MAX_MODELS`].
fn collect(entries: impl Iterator<Item = LocalModel>) -> Vec<LocalModel> {
    let mut seen = HashSet::new();
    entries
        .filter(|model| valid_id(&model.id) && seen.insert(model.id.clone()))
        .take(MAX_MODELS)
        .collect()
}

/// Ollama `GET /api/tags`: `{ "models": [{ "name": "qwen3:8b", ... }] }`. `None` when the body
/// is not that shape: then it is no Ollama answering.
pub fn parse_ollama_tags(body: &[u8]) -> Option<Vec<LocalModel>> {
    let mut keep = None;
    let models = array(body, "models", &mut keep)?;
    Some(collect(models.iter().filter_map(|entry| {
        let name = entry.get("name")?.as_str()?;
        Some(LocalModel {
            id: name.to_owned(),
            label: clean_label(name),
            loaded: false,
        })
    })))
}

/// Ollama `GET /api/ps`: the names of the models in memory.
pub fn parse_ollama_ps(body: &[u8]) -> HashSet<String> {
    let mut keep = None;
    let Some(models) = array(body, "models", &mut keep) else {
        return HashSet::new();
    };
    models
        .iter()
        .take(MAX_MODELS)
        .filter_map(|entry| {
            entry
                .get("name")
                .or_else(|| entry.get("model"))?
                .as_str()
                .map(str::to_owned)
        })
        .collect()
}

/// LM Studio `GET /api/v1/models`: `{ "models": [{ "type": "llm", "key": ..., "display_name":
/// ..., "loaded_instances": [...] }] }`. Only language models; embedding models cannot chat.
pub fn parse_lmstudio(body: &[u8]) -> Option<Vec<LocalModel>> {
    let mut keep = None;
    let models = array(body, "models", &mut keep)?;
    Some(collect(models.iter().filter_map(|entry| {
        if entry.get("type")?.as_str()? != "llm" {
            return None;
        }
        let key = entry.get("key")?.as_str()?;
        let label = entry
            .get("display_name")
            .and_then(Value::as_str)
            .map(clean_label)
            .filter(|label| !label.is_empty())
            .unwrap_or_else(|| clean_label(key));
        let loaded = entry
            .get("loaded_instances")
            .and_then(Value::as_array)
            .is_some_and(|instances| !instances.is_empty());
        Some(LocalModel {
            id: key.to_owned(),
            label,
            loaded,
        })
    })))
}

/// The OpenAI-compatible `GET /v1/models` of older LM Studio servers: `{ "data": [{ "id": ...
/// }] }`. Whether a model is loaded is unknown there.
pub fn parse_openai_models(body: &[u8]) -> Option<Vec<LocalModel>> {
    let mut keep = None;
    let models = array(body, "data", &mut keep)?;
    Some(collect(models.iter().filter_map(|entry| {
        let id = entry.get("id")?.as_str()?;
        Some(LocalModel {
            id: id.to_owned(),
            label: clean_label(id),
            loaded: false,
        })
    })))
}

/// Ollama: running when `/api/tags` answers with its JSON; `loaded` from `/api/ps`.
pub fn probe_ollama(fetch: &dyn Fetch) -> ServerState {
    let tags = fetch.get(LocalServer::Ollama, "/api/tags");
    let Some(mut models) = tags.ok_body().and_then(parse_ollama_tags) else {
        return ServerState::default();
    };
    if !models.is_empty() {
        let loaded = fetch
            .get(LocalServer::Ollama, "/api/ps")
            .ok_body()
            .map(parse_ollama_ps)
            .unwrap_or_default();
        for model in &mut models {
            model.loaded = loaded.contains(&model.id);
        }
    }
    ServerState {
        running: true,
        models,
    }
}

/// LM Studio: `/api/v1/models`, falling back to `/v1/models` when the server answers but not
/// with the newer endpoint's shape (an older server). Nothing listening: no second request.
pub fn probe_lmstudio(fetch: &dyn Fetch) -> ServerState {
    let current = fetch.get(LocalServer::Lmstudio, "/api/v1/models");
    if let Some(models) = current.ok_body().and_then(parse_lmstudio) {
        return ServerState {
            running: true,
            models,
        };
    }
    if !current.answered() {
        return ServerState::default();
    }
    match fetch
        .get(LocalServer::Lmstudio, "/v1/models")
        .ok_body()
        .and_then(parse_openai_models)
    {
        Some(models) => ServerState {
            running: true,
            models,
        },
        None => ServerState::default(),
    }
}

/// Both servers, one after the other.
pub fn probe(fetch: &dyn Fetch) -> LocalLlmState {
    LocalLlmState {
        ollama: probe_ollama(fetch),
        lmstudio: probe_lmstudio(fetch),
    }
}

/// The app's [`Fetch`]: reqwest on loopback, no proxy, no redirects, [`REQUEST_LIMIT`],
/// [`BODY_LIMIT`]. Blocks the calling thread, which must not be an async task.
pub struct HttpFetch {
    client: Option<reqwest::Client>,
}

impl Default for HttpFetch {
    fn default() -> Self {
        crate::updates::ensure_crypto_provider();
        let client = reqwest::Client::builder()
            .user_agent(format!("Hangar/{}", env!("CARGO_PKG_VERSION")))
            .timeout(REQUEST_LIMIT)
            .connect_timeout(REQUEST_LIMIT)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .ok();
        Self { client }
    }
}

impl Fetch for HttpFetch {
    fn get(&self, server: LocalServer, path: &str) -> Fetched {
        let Some(client) = &self.client else {
            return Fetched::Unreachable;
        };
        let url = format!("{}{path}", server.base());
        tauri::async_runtime::block_on(get(client, &url))
    }
}

async fn get(client: &reqwest::Client, url: &str) -> Fetched {
    let Ok(mut response) = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
    else {
        return Fetched::Unreachable;
    };
    let status = response.status().as_u16();
    if response
        .content_length()
        .is_some_and(|length| length > BODY_LIMIT as u64)
    {
        return Fetched::TooLarge;
    }
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if body.len() + chunk.len() <= BODY_LIMIT => {
                body.extend_from_slice(&chunk);
            }
            Ok(Some(_)) => return Fetched::TooLarge,
            Ok(None) => return Fetched::Answer { status, body },
            Err(_) => return Fetched::Unreachable,
        }
    }
}

/// The polling thread and the last state it saw.
#[derive(Default)]
pub struct LocalLlmMonitor {
    latest: Arc<Mutex<LocalLlmState>>,
    stop: Arc<AtomicBool>,
    /// The thread's [`Fetch`], for [`Self::refresh`]; `None` until [`Self::start`].
    fetch: Mutex<Option<Arc<dyn Fetch>>>,
}

impl LocalLlmMonitor {
    /// Starts the thread: it probes at once, then every `interval()` (read anew each round, so
    /// battery power slows it down), and hands every change to `emit`.
    pub fn start(
        &self,
        fetch: Arc<dyn Fetch>,
        interval: impl Fn() -> Duration + Send + 'static,
        emit: impl Fn(&LocalLlmState) + Send + 'static,
    ) -> io::Result<()> {
        *lock(&self.fetch) = Some(Arc::clone(&fetch));
        let latest = Arc::clone(&self.latest);
        let stop = Arc::clone(&self.stop);
        thread::Builder::new()
            .name("local-llm".into())
            .spawn(move || loop {
                if let Some(state) = Self::store(&latest, probe(fetch.as_ref())) {
                    emit(&state);
                }
                let until = Instant::now() + interval();
                while Instant::now() < until {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    thread::sleep(STOP_CHECK.min(until.saturating_duration_since(Instant::now())));
                }
                if stop.load(Ordering::Relaxed) {
                    return;
                }
            })
            .map(drop)
    }

    /// Probes at once, as `local_llm_refresh` does: the new state and whether it changed (for
    /// the event). Before [`Self::start`] the last state, unchanged. Blocks for up to four
    /// requests, so never on the main thread.
    pub fn refresh(&self) -> (LocalLlmState, bool) {
        let fetch = lock(&self.fetch).clone();
        let Some(fetch) = fetch else {
            return (self.current(), false);
        };
        let state = probe(fetch.as_ref());
        let changed = Self::store(&self.latest, state.clone()).is_some();
        (state, changed)
    }

    /// The last state seen.
    pub fn current(&self) -> LocalLlmState {
        lock(&self.latest).clone()
    }

    /// Ends the thread within [`STOP_CHECK`].
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Stores `state`; `Some` when it differs from the one before.
    fn store(latest: &Mutex<LocalLlmState>, state: LocalLlmState) -> Option<LocalLlmState> {
        let mut current = lock(latest);
        if *current == state {
            return None;
        }
        *current = state.clone();
        Some(state)
    }
}

/// The interval for a power state: slower on battery.
pub fn interval_for(power: crate::power::PowerState) -> Duration {
    use crate::power::PowerState;
    match power {
        PowerState::Battery | PowerState::Saver => SAVING_INTERVAL,
        PowerState::Unknown | PowerState::NoBattery | PowerState::Ac => POLL_INTERVAL,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::mpsc;

    use super::*;

    const TAGS: &str = r#"{"models":[
        {"name":"qwen3:8b","model":"qwen3:8b","size":5200000000,"details":{"family":"qwen3"}},
        {"name":"llama3.2:latest","model":"llama3.2:latest"},
        {"name":"hf.co/org/model:Q4_K_M"},
        {"name":"-bad"},
        {"name":"has space"},
        {"model":"no-name"},
        {"name":42},
        {"name":"qwen3:8b"}
    ]}"#;
    const PS: &str = r#"{"models":[{"name":"qwen3:8b","model":"qwen3:8b","size_vram":1}]}"#;
    const LMSTUDIO: &str = r#"{"models":[
        {"type":"llm","publisher":"qwen","key":"qwen/qwen3-8b","display_name":"Qwen3 8B","loaded_instances":[{"id":"qwen/qwen3-8b"}]},
        {"type":"llm","key":"google/gemma-3-12b","display_name":"","loaded_instances":[]},
        {"type":"embedding","key":"text-embedding-nomic","display_name":"Nomic"},
        {"type":"llm","key":"mistral","display_name":"Mistral\u0007 Small"}
    ]}"#;
    const OPENAI: &str =
        r#"{"object":"list","data":[{"id":"qwen3-8b","object":"model"},{"id":"bad id"}]}"#;

    /// Answers from fixtures; every request is counted.
    #[derive(Default)]
    struct Fixtures {
        answers: HashMap<(LocalServer, &'static str), Fetched>,
        asked: Mutex<Vec<String>>,
    }

    impl Fixtures {
        fn with(mut self, server: LocalServer, path: &'static str, fetched: Fetched) -> Self {
            self.answers.insert((server, path), fetched);
            self
        }

        fn json(self, server: LocalServer, path: &'static str, body: &str) -> Self {
            self.with(
                server,
                path,
                Fetched::Answer {
                    status: 200,
                    body: body.as_bytes().to_vec(),
                },
            )
        }
    }

    impl Fetch for Fixtures {
        fn get(&self, server: LocalServer, path: &str) -> Fetched {
            lock(&self.asked).push(format!("{server:?}{path}"));
            self.answers
                .iter()
                .find(|((s, p), _)| *s == server && *p == path)
                .map_or(Fetched::Unreachable, |(_, fetched)| fetched.clone())
        }
    }

    fn ids(state: &ServerState) -> Vec<&str> {
        state.models.iter().map(|m| m.id.as_str()).collect()
    }

    #[test]
    fn ollama_models_and_the_loaded_ones() {
        let fixtures = Fixtures::default()
            .json(LocalServer::Ollama, "/api/tags", TAGS)
            .json(LocalServer::Ollama, "/api/ps", PS);
        let state = probe_ollama(&fixtures);
        assert!(state.running);
        assert_eq!(
            ids(&state),
            ["qwen3:8b", "llama3.2:latest", "hf.co/org/model:Q4_K_M"]
        );
        assert!(state.models[0].loaded);
        assert!(!state.models[1].loaded);
        assert_eq!(state.models[0].label, "qwen3:8b");
    }

    #[test]
    fn ollama_without_models_or_ps_still_runs() {
        let fixtures =
            Fixtures::default().json(LocalServer::Ollama, "/api/tags", r#"{"models":[]}"#);
        let state = probe_ollama(&fixtures);
        assert_eq!(
            state,
            ServerState {
                running: true,
                models: Vec::new()
            }
        );
        // No /api/ps for an empty list.
        assert_eq!(lock(&fixtures.asked).len(), 1);
        // A broken /api/ps only loses the marks.
        let fixtures = Fixtures::default()
            .json(LocalServer::Ollama, "/api/tags", TAGS)
            .json(LocalServer::Ollama, "/api/ps", "not json");
        let state = probe_ollama(&fixtures);
        assert!(state.running && state.models.iter().all(|m| !m.loaded));
    }

    #[test]
    fn lmstudio_lists_language_models_only() {
        let fixtures = Fixtures::default().json(LocalServer::Lmstudio, "/api/v1/models", LMSTUDIO);
        let state = probe_lmstudio(&fixtures);
        assert!(state.running);
        assert_eq!(
            ids(&state),
            ["qwen/qwen3-8b", "google/gemma-3-12b", "mistral"]
        );
        assert_eq!(state.models[0].label, "Qwen3 8B");
        assert!(state.models[0].loaded);
        // An empty display name falls back to the key; control characters go.
        assert_eq!(state.models[1].label, "google/gemma-3-12b");
        assert!(!state.models[1].loaded);
        assert_eq!(state.models[2].label, "Mistral Small");
        assert_eq!(lock(&fixtures.asked).len(), 1);
    }

    #[test]
    fn an_older_lmstudio_falls_back_to_the_openai_list() {
        let fixtures = Fixtures::default()
            .with(
                LocalServer::Lmstudio,
                "/api/v1/models",
                Fetched::Answer {
                    status: 404,
                    body: b"Unexpected endpoint".to_vec(),
                },
            )
            .json(LocalServer::Lmstudio, "/v1/models", OPENAI);
        let state = probe_lmstudio(&fixtures);
        assert!(state.running);
        assert_eq!(ids(&state), ["qwen3-8b"]);
        assert!(!state.models[0].loaded);
    }

    #[test]
    fn nothing_listening_is_off_without_a_second_request() {
        let fixtures = Fixtures::default();
        assert_eq!(probe(&fixtures), LocalLlmState::default());
        assert_eq!(
            *lock(&fixtures.asked),
            ["Ollama/api/tags", "Lmstudio/api/v1/models"]
        );
    }

    #[test]
    fn junk_and_oversized_answers_are_no_server() {
        for body in [
            "",
            "<html>404</html>",
            "[]",
            r#"{"models":"x"}"#,
            r#"{"data":[]}"#,
            "null",
        ] {
            let fixtures = Fixtures::default()
                .json(LocalServer::Ollama, "/api/tags", body)
                .json(LocalServer::Lmstudio, "/api/v1/models", body)
                .json(LocalServer::Lmstudio, "/v1/models", body);
            let state = probe(&fixtures);
            assert!(!state.ollama.running, "{body:?}");
            // `{"data":[]}` is a valid (empty) OpenAI list.
            assert_eq!(state.lmstudio.running, body == r#"{"data":[]}"#, "{body:?}");
        }
        let fixtures = Fixtures::default()
            .with(LocalServer::Ollama, "/api/tags", Fetched::TooLarge)
            .with(LocalServer::Lmstudio, "/api/v1/models", Fetched::TooLarge)
            .with(LocalServer::Lmstudio, "/v1/models", Fetched::TooLarge);
        assert_eq!(probe(&fixtures), LocalLlmState::default());
        // A server error is no answer either.
        let fixtures = Fixtures::default().with(
            LocalServer::Ollama,
            "/api/tags",
            Fetched::Answer {
                status: 500,
                body: br#"{"models":[]}"#.to_vec(),
            },
        );
        assert!(!probe_ollama(&fixtures).running);
    }

    #[test]
    fn lists_and_labels_are_bounded() {
        let many: Vec<String> = (0..500).map(|i| format!(r#"{{"name":"m{i}"}}"#)).collect();
        let body = format!(r#"{{"models":[{}]}}"#, many.join(","));
        let models = parse_ollama_tags(body.as_bytes()).unwrap();
        assert_eq!(models.len(), MAX_MODELS);
        // Too long for an id: dropped, never cut into another model's name.
        let long = "a".repeat(161);
        let body = format!(r#"{{"models":[{{"name":"{long}"}}]}}"#);
        assert!(parse_ollama_tags(body.as_bytes()).unwrap().is_empty());
        let label = "L".repeat(400);
        let body = format!(r#"{{"models":[{{"type":"llm","key":"k","display_name":"{label}"}}]}}"#);
        let models = parse_lmstudio(body.as_bytes()).unwrap();
        assert_eq!(models[0].label.chars().count(), MAX_LABEL_CHARS);
    }

    #[test]
    fn the_state_serializes_as_the_contract_says() {
        let fixtures = Fixtures::default()
            .json(
                LocalServer::Ollama,
                "/api/tags",
                r#"{"models":[{"name":"qwen3:8b"}]}"#,
            )
            .json(LocalServer::Ollama, "/api/ps", PS);
        let json = serde_json::to_value(probe(&fixtures)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "ollama": { "running": true, "models": [
                    { "id": "qwen3:8b", "label": "qwen3:8b", "loaded": true }
                ] },
                "lmstudio": { "running": false, "models": [] }
            })
        );
    }

    #[test]
    fn the_thread_emits_only_changes_and_stops() {
        struct Flip(AtomicBool);
        impl Fetch for Flip {
            fn get(&self, server: LocalServer, _: &str) -> Fetched {
                if server == LocalServer::Ollama && self.0.load(Ordering::Relaxed) {
                    Fetched::Answer {
                        status: 200,
                        body: br#"{"models":[]}"#.to_vec(),
                    }
                } else {
                    Fetched::Unreachable
                }
            }
        }
        let fetch = Arc::new(Flip(AtomicBool::new(true)));
        let monitor = LocalLlmMonitor::default();
        let (tx, rx) = mpsc::channel();
        monitor
            .start(
                Arc::clone(&fetch) as Arc<dyn Fetch>,
                || Duration::from_millis(20),
                move |state| {
                    let _ = tx.send(state.clone());
                },
            )
            .unwrap();
        let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(first.ollama.running);
        // Unchanged rounds emit nothing.
        assert!(rx.recv_timeout(Duration::from_millis(150)).is_err());
        fetch.0.store(false, Ordering::Relaxed);
        let second = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!second.ollama.running);
        assert_eq!(monitor.current(), second);
        // A refresh with the same state is no change; a flip is.
        assert_eq!(monitor.refresh(), (second, false));
        fetch.0.store(true, Ordering::Relaxed);
        monitor.stop();
        let (state, changed) = monitor.refresh();
        assert!(changed && state.ollama.running);
    }

    #[test]
    fn battery_slows_the_probe_down() {
        use crate::power::PowerState;
        assert_eq!(interval_for(PowerState::Ac), POLL_INTERVAL);
        assert_eq!(interval_for(PowerState::NoBattery), POLL_INTERVAL);
        assert_eq!(interval_for(PowerState::Battery), SAVING_INTERVAL);
        assert_eq!(interval_for(PowerState::Saver), SAVING_INTERVAL);
    }

    /// The real client against a one-shot server on 127.0.0.1, so the limits are measured, not
    /// assumed. The server stands in for both ports: the test calls [`get`] directly.
    #[test]
    fn the_http_client_reads_bounded_answers() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        fn serve(reply: Vec<u8>) -> u16 {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            thread::spawn(move || {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut buffer = [0u8; 1024];
                    let _ = stream.read(&mut buffer);
                    let _ = stream.write_all(&reply);
                }
            });
            port
        }
        let client = HttpFetch::default().client.unwrap();
        let body = br#"{"models":[]}"#;
        let mut reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        reply.extend_from_slice(body);
        let port = serve(reply);
        let fetched = tauri::async_runtime::block_on(get(
            &client,
            &format!("http://127.0.0.1:{port}/api/tags"),
        ));
        assert_eq!(
            fetched,
            Fetched::Answer {
                status: 200,
                body: body.to_vec()
            }
        );
        // Too large by its header.
        let port = serve(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                BODY_LIMIT + 1
            )
            .into_bytes(),
        );
        let fetched =
            tauri::async_runtime::block_on(get(&client, &format!("http://127.0.0.1:{port}/")));
        assert_eq!(fetched, Fetched::TooLarge);
        // A redirect is not followed.
        let port = serve(
            b"HTTP/1.1 302 Found\r\nLocation: http://example.org/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec(),
        );
        let fetched =
            tauri::async_runtime::block_on(get(&client, &format!("http://127.0.0.1:{port}/")));
        assert!(matches!(fetched, Fetched::Answer { status: 302, .. }));
        // A closed port is unreachable.
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let fetched =
            tauri::async_runtime::block_on(get(&client, &format!("http://127.0.0.1:{port}/")));
        assert_eq!(fetched, Fetched::Unreachable);
    }
}
