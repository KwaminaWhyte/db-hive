//! The AI assistant driven end to end: the real window, the real controller, a
//! real SQLite file, and a stub provider on a local socket.
//!
//! The stub speaks the OpenAI wire format, so the real `OpenAiProvider` and
//! `service_ai` run unchanged against it; nothing here can reach a real
//! provider, because the provider's base URL is the stub's address and the
//! Ollama URL is a closed port. The API key lives in an in-memory store
//! installed with `use_ai_key_store`, so the developer's keychain is never
//! read or written.
//!
//! The flow: the command is refused before a connection is open; the panel
//! opens on "add an API key" and sends nothing; a key is saved (the field is
//! cleared at once, the key is nowhere on disk); the model list arrives; the
//! schema context is the file name and table names only; Generate sends the
//! prompt and schema and never a row; the answer is applied to the editor, not
//! run; running it fails; Fix sends the error; the fix is applied and runs;
//! Explain sends no schema; Optimize is appended and opened in a new tab; the
//! schema can be switched off; a provider failure is a failed turn; switching
//! to a closed Ollama is the guided "not running" state, and the choices
//! persist; removing the key goes back to the first state.

use std::cell::{Cell, RefCell};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dbhive_core::ai::AiProviderType;
use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::service_ai::{self, ApiKeyStore, MemoryKeyStore};
use dbhive_core::state::AppState;
use dbhive_slint::{AiState, Screen};
use slint::{ComponentHandle, Model, Timer, TimerMode};

/// Not a real key. The test looks for it on disk and in the UI afterwards.
const KEY: &str = "stub-key-0000-not-a-real-key";

// ---------------------------------------------------------------------------
// The stub provider
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Seen {
    method: String,
    path: String,
    auth: String,
    body: String,
}

struct Stub {
    base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
    fail_chat: Arc<AtomicBool>,
}

impl Stub {
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    /// The newest chat request whose system prompt contains `marker`.
    fn last_chat(&self, marker: &str) -> Option<Seen> {
        self.seen()
            .into_iter()
            .rev()
            .find(|s| s.path.ends_with("/chat/completions") && system_prompt(&s.body).contains(marker))
    }
}

fn system_prompt(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["messages"][0]["content"].as_str().map(str::to_string))
        .unwrap_or_default()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn chat_reply(content: &str) -> String {
    serde_json::json!({
        "id": "stub-1",
        "model": "stub-model",
        "choices": [{ "message": { "role": "assistant", "content": content }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
    })
    .to_string()
}

fn handle(mut stream: TcpStream, seen: Arc<Mutex<Vec<Seen>>>, fail_chat: Arc<AtomicBool>) {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut tmp).unwrap_or(0);
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(p) = find(&buf, b"\r\n\r\n") {
            break p + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut auth = String::new();
    let mut length = 0usize;
    for l in lines {
        let lower = l.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            length = v.trim().parse().unwrap_or(0);
        }
        if lower.starts_with("authorization:") {
            auth = l["authorization:".len()..].trim().to_string();
        }
    }
    while buf.len() < header_end + length {
        let n = stream.read(&mut tmp).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let body = String::from_utf8_lossy(&buf[header_end..(header_end + length).min(buf.len())]).to_string();
    seen.lock().unwrap().push(Seen { method: method.clone(), path: path.clone(), auth: auth.clone(), body: body.clone() });

    let authorised = auth == format!("Bearer {KEY}");
    if !authorised {
        respond(&mut stream, "401 Unauthorized", r#"{"error":{"message":"Incorrect API key provided"}}"#);
        return;
    }
    if method == "GET" && path.ends_with("/models") {
        let models = serde_json::json!({ "data": [
            { "id": "gpt-4o-mini", "created": 1, "owned_by": "stub" },
            { "id": "gpt-4o", "created": 1, "owned_by": "stub" },
            { "id": "text-embedding-ada-002", "created": 1, "owned_by": "stub" },
        ]});
        respond(&mut stream, "200 OK", &models.to_string());
        return;
    }
    if method == "POST" && path.ends_with("/chat/completions") {
        if fail_chat.swap(false, Ordering::SeqCst) {
            respond(&mut stream, "500 Internal Server Error", r#"{"error":{"message":"boom: upstream exploded"}}"#);
            return;
        }
        let system = system_prompt(&body);
        let content = if system.contains("SQL debugging expert") {
            "```sql\nSELECT name FROM widgets ORDER BY id;\n```"
        } else if system.contains("SQL expert assistant") {
            "```sql\nSELECT * FROM widgets WHERE nme = 'x';\n```"
        } else if system.contains("Explain SQL queries") {
            "This query reads every row of the widgets table."
        } else if system.contains("performance optimization") {
            "Bound the scan.\n```sql\nSELECT name FROM widgets ORDER BY id LIMIT 100;\n```\nThat reads at most 100 rows."
        } else {
            "I do not know what to do with that."
        };
        respond(&mut stream, "200 OK", &chat_reply(content));
        return;
    }
    respond(&mut stream, "404 Not Found", r#"{"error":{"message":"no such route"}}"#);
}

fn start_stub() -> Stub {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let fail_chat = Arc::new(AtomicBool::new(false));
    let (s, f) = (seen.clone(), fail_chat.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let (s, f) = (s.clone(), f.clone());
            std::thread::spawn(move || handle(stream, s, f));
        }
    });
    Stub { base: format!("http://127.0.0.1:{port}/v1"), seen, fail_chat }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn files_under(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                files_under(&p, out);
            } else {
                out.push(p);
            }
        }
    }
}

fn toast_texts(w: &dbhive_slint::AppWindow) -> Vec<String> {
    w.get_toasts().iter().map(|t| t.text.to_string()).collect()
}

#[test]
fn configure_ask_apply_and_fix_against_a_stub_provider() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-ai-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let db = dir.join("widgets.sqlite");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute("CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT)", []).unwrap();
        conn.execute("INSERT INTO widgets (name) VALUES ('sprocket'), ('gizmo')", []).unwrap();
    }
    {
        let mut profile = ConnectionProfile::new(
            "widgets-db".to_string(),
            "Widgets".to_string(),
            DbDriver::Sqlite,
            String::new(),
            0,
            String::new(),
        );
        profile.database = Some(db.to_string_lossy().to_string());
        let mut state = AppState::new();
        state.add_profile(profile);
        state.save_profiles_to_store().unwrap();
    }

    // OpenAI is the stub; Ollama is a port nothing listens on.
    let stub = start_stub();
    {
        let mut settings = service_ai::AiSettings::default();
        settings.active_provider = AiProviderType::OpenAI;
        settings.openai.base_url = stub.base.clone();
        settings.openai.default_model = "gpt-4o-mini".into();
        settings.ollama.base_url = "http://127.0.0.1:1".into();
        service_ai::save_settings(&settings).unwrap();
    }
    let store: Arc<MemoryKeyStore> = Arc::new(MemoryKeyStore::new());
    dbhive_slint::app::use_ai_key_store(store.clone());

    let w = dbhive_slint::app::build().unwrap();
    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let step = Rc::new(Cell::new(0u32));
    let started = Instant::now();
    let timer = Timer::default();

    let (weak, fail, step2, stub2, store2, dir2) =
        (w.as_weak(), failure.clone(), step.clone(), Rc::new(stub), store.clone(), dir.clone());
    let dir_text = dir.to_string_lossy().to_string();

    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let s = w.global::<AiState>();
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(40) {
            bail(format!("timed out at step {}", step2.get()));
            return;
        }
        let turns = || s.get_turns().iter().collect::<Vec<_>>();
        let turn_at = |index: i32| turns().into_iter().find(|t| t.index == index);

        match step2.get() {
            // The command needs an open connection.
            0 => {
                w.invoke_run_command("go-ai-assistant".into());
                if s.get_visible() {
                    return bail("the command opened the panel with no connection open".into());
                }
                w.invoke_connect("widgets-db".into());
                step2.set(1);
            }
            1 if w.get_screen() == Screen::Workspace && w.get_workspace_tabs().row_count() == 1 => {
                w.invoke_run_command("go-ai-assistant".into());
                if !s.get_visible() {
                    return bail("go-ai-assistant did not open the panel in a workspace".into());
                }
                step2.set(2);
            }

            // First open: no key, so the guided state, and nothing was sent.
            2 if s.get_empty_kind() == 1 => {
                if s.get_providers().row_count() != 5 || s.get_provider_index() != 1 {
                    return bail(format!(
                        "expected 5 providers with OpenAI chosen from the saved settings, got {} and {}",
                        s.get_providers().row_count(),
                        s.get_provider_index()
                    ));
                }
                if !s.get_needs_key() || s.get_key_saved() || s.get_status_tone() != 2 {
                    return bail("the status should say a key is missing".into());
                }
                if s.get_can_generate() || s.get_can_explain() {
                    return bail("nothing should be askable without a key".into());
                }
                if !stub2.seen().is_empty() {
                    return bail(format!("a request went out with no key: {:?}", stub2.seen()));
                }
                // The empty state's action reveals the key field.
                s.invoke_empty_action_clicked();
                if !s.get_show_settings() {
                    return bail("Add API key should open the settings".into());
                }
                s.set_api_key(KEY.into());
                s.invoke_save_key();
                if !s.get_api_key().is_empty() {
                    return bail("the key field was not cleared when the key was taken".into());
                }
                step2.set(3);
            }

            // Key stored, status connected, models listed, schema read.
            3 if s.get_empty_kind() == -1
                && s.get_models().row_count() == 2
                && !s.get_context_loading()
                && !s.get_context_summary().is_empty() =>
            {
                // The context text is only handed to the window while the
                // viewer is open.
                if !s.get_context_text().is_empty() {
                    return bail("the context text should not be in the window while the viewer is closed".into());
                }
                s.invoke_toggle_context();
                if store2.get(AiProviderType::OpenAI).as_deref() != Some(KEY) {
                    return bail("the key did not reach the key store".into());
                }
                if s.get_status_text() != "Connected" || !s.get_key_saved() || s.get_status_tone() != 1 {
                    return bail(format!("status should be connected, got {:?}", s.get_status_text()));
                }
                let models: Vec<String> = s.get_models().iter().map(|m| m.to_string()).collect();
                if models[0] != "GPT-4o (128K context)" || models[1] != "GPT-4o Mini (128K context)" {
                    return bail(format!("model list was {:?}", models));
                }
                if !s.get_model_text().contains("Mini") {
                    return bail(format!("the saved default model should be chosen, got {:?}", s.get_model_text()));
                }
                let listed = stub2.seen().into_iter().any(|r| r.method == "GET" && r.path.ends_with("/models") && r.auth == format!("Bearer {KEY}"));
                if !listed {
                    return bail("the model list was not fetched with the key".into());
                }

                // The schema context: file name, table and columns, no rows,
                // no path.
                let ctx = s.get_context_text().to_string();
                if !ctx.contains("Database: widgets.sqlite") || ctx.contains(&dir_text) {
                    return bail(format!("the context should name the file only, got {ctx:?}"));
                }
                if !ctx.contains("main.widgets") || !ctx.contains("name:") {
                    return bail(format!("the context should list the table and its columns, got {ctx:?}"));
                }
                if ctx.contains("sprocket") || ctx.contains("gizmo") {
                    return bail("row data reached the schema context".into());
                }
                if !s.get_context_summary().starts_with("1 table,") {
                    return bail(format!("context summary was {:?}", s.get_context_summary()));
                }
                if s.get_disclosure().row_count() != 4 {
                    return bail("every action should say what it sends".into());
                }

                // A bad base URL is refused and changes nothing.
                s.invoke_save_base_url("not a url".into());
                if s.get_settings_error().is_empty() || s.get_status_text() != "Connected" {
                    return bail("a bad base URL should be refused without losing the status".into());
                }
                // Pick the other model (the choice must persist), then save the
                // same URL again: that re-checks the provider.
                s.invoke_model_selected(0);
                s.invoke_save_base_url(stub2.base.clone().into());
                if s.get_status_text() != "Checking" {
                    return bail(format!("saving a URL should re-check the provider, got {:?}", s.get_status_text()));
                }
                step2.set(30);
            }

            // Re-checked: connected again. Generate.
            30 if s.get_empty_kind() == -1 && s.get_models().row_count() == 2 => {
                if !s.get_model_text().starts_with("GPT-4o (") {
                    return bail(format!("the picked model should survive the re-check, got {:?}", s.get_model_text()));
                }
                s.set_prompt("widgets whose name is x".into());
                s.invoke_prompt_edited("widgets whose name is x".into());
                if !s.get_can_generate() {
                    return bail("Generate should be enabled with a prompt".into());
                }
                s.invoke_generate();
                if !s.get_busy() || s.get_can_generate() {
                    return bail("a request in flight should disable the buttons".into());
                }
                step2.set(4);
            }

            4 if turns().len() == 2 && !s.get_busy() => {
                let answer = turn_at(1).unwrap();
                if turn_at(0).map(|t| t.user) != Some(true) || answer.user || answer.failed {
                    return bail("turn 0 should be the request and turn 1 the answer".into());
                }
                if !answer.has_sql || !answer.sql.contains("nme") || !answer.body.is_empty() {
                    return bail(format!("answer sql {:?} body {:?}", answer.sql, answer.body));
                }
                // Newest exchange first, request before its answer.
                let order: Vec<i32> = turns().iter().map(|t| t.index).collect();
                if order != vec![0, 1] {
                    return bail(format!("one exchange reads request then answer, got {order:?}"));
                }
                if !answer.meta.starts_with("gpt-4o") {
                    return bail(format!("answer meta was {:?}", answer.meta));
                }

                // What went over the wire.
                let Some(req) = stub2.last_chat("SQL expert assistant") else {
                    return bail("no generate request reached the stub".into());
                };
                let v: serde_json::Value = serde_json::from_str(&req.body).unwrap();
                if v["model"] != "gpt-4o" {
                    return bail(format!("the chosen model should be sent, got {}", v["model"]));
                }
                let sent = system_prompt(&req.body);
                if !sent.contains("main.widgets") || !sent.contains("Database: widgets.sqlite") {
                    return bail("the schema context was not sent".into());
                }
                if req.body.contains("sprocket") || req.body.contains("gizmo") || req.body.contains(&dir_text) {
                    return bail("row data or a local path was sent to the provider".into());
                }
                if req.auth != format!("Bearer {KEY}") {
                    return bail("the request did not carry the key".into());
                }

                // Apply: replaces the editor text, and runs nothing.
                s.invoke_apply(1, 0);
                if w.get_workspace_editor_text() != "SELECT * FROM widgets WHERE nme = 'x';" {
                    return bail(format!("editor text was {:?}", w.get_workspace_editor_text()));
                }
                if w.get_workspace_has_result() || w.get_workspace_running() {
                    return bail("applying an answer ran it".into());
                }
                if !s.get_sql_preview().contains("nme") {
                    return bail("the panel should show the editor's new query".into());
                }
                w.invoke_workspace_run_query();
                step2.set(5);
            }

            // The statement fails; the panel offers the fix.
            5 if !w.get_workspace_running() && !w.get_workspace_rows_error().is_empty() => {
                if !w.get_workspace_rows_error().contains("nme") {
                    return bail(format!("unexpected error {:?}", w.get_workspace_rows_error()));
                }
                if !s.get_has_error() || !s.get_can_fix() {
                    return bail("a failed query should enable Fix".into());
                }
                s.invoke_fix();
                step2.set(6);
            }
            6 if turns().len() == 4 && !s.get_busy() => {
                let fixed = turn_at(3).unwrap();
                if fixed.failed || !fixed.has_sql || fixed.sql != "SELECT name FROM widgets ORDER BY id;" {
                    return bail(format!("fix answer was {:?}", fixed.sql));
                }
                // Newest exchange first.
                let order: Vec<i32> = turns().iter().map(|t| t.index).collect();
                if order != vec![2, 3, 0, 1] {
                    return bail(format!("expected the fix exchange first, got {order:?}"));
                }
                let Some(req) = stub2.last_chat("SQL debugging expert") else {
                    return bail("no fix request reached the stub".into());
                };
                if !req.body.contains("no such column") || !req.body.contains("nme") {
                    return bail("the fix request should carry the statement and its error".into());
                }
                if !system_prompt(&req.body).contains("main.widgets") {
                    return bail("the fix request should carry the schema".into());
                }
                s.invoke_apply(3, 0);
                if w.get_workspace_editor_text() != "SELECT name FROM widgets ORDER BY id;" {
                    return bail(format!("editor text after the fix: {:?}", w.get_workspace_editor_text()));
                }
                if !w.get_workspace_rows_error().is_empty() || s.get_has_error() {
                    return bail("replacing the statement should clear the error that described it".into());
                }
                w.invoke_workspace_run_query();
                step2.set(7);
            }
            7 if w.get_workspace_has_result() && !w.get_workspace_running() => {
                if w.get_workspace_row_count() != 2 {
                    return bail(format!("the fixed query should return 2 rows, got {}", w.get_workspace_row_count()));
                }
                // Explain sends the query and no schema.
                s.invoke_explain();
                step2.set(8);
            }
            8 if turns().len() == 6 && !s.get_busy() => {
                let answer = turn_at(5).unwrap();
                if answer.has_sql || !answer.body.contains("widgets table") {
                    return bail(format!("explain answer {:?}", answer.body));
                }
                let Some(req) = stub2.last_chat("Explain SQL queries") else {
                    return bail("no explain request reached the stub".into());
                };
                if !req.body.contains("SELECT name FROM widgets") {
                    return bail("explain should send the query".into());
                }
                if req.body.contains("main.widgets") || req.body.contains("Database:") {
                    return bail("explain must not send the schema".into());
                }
                s.invoke_optimize();
                step2.set(9);
            }
            9 if turns().len() == 8 && !s.get_busy() => {
                let answer = turn_at(7).unwrap();
                if !answer.has_sql || answer.body != "Bound the scan.\nThat reads at most 100 rows." {
                    return bail(format!("optimize answer: body {:?} sql {:?}", answer.body, answer.sql));
                }
                // Add below, then open in a new tab.
                s.invoke_apply(7, 1);
                let want = "SELECT name FROM widgets ORDER BY id;\n\nSELECT name FROM widgets ORDER BY id LIMIT 100;";
                if w.get_workspace_editor_text() != want {
                    return bail(format!("append gave {:?}", w.get_workspace_editor_text()));
                }
                let tabs_before = w.get_workspace_tabs().row_count();
                s.invoke_apply(7, 2);
                if w.get_workspace_tabs().row_count() != tabs_before + 1
                    || w.get_workspace_editor_text() != "SELECT name FROM widgets ORDER BY id LIMIT 100;"
                {
                    return bail("New tab should open the statement alone in a new tab".into());
                }
                if w.get_workspace_has_result() {
                    return bail("no applied statement may run by itself".into());
                }

                // Turn the schema off and ask again: nothing about the schema
                // may be sent.
                s.invoke_toggle_schema();
                if s.get_include_schema() {
                    return bail("the schema toggle did not turn it off".into());
                }
                s.set_prompt("any widget".into());
                s.invoke_prompt_edited("any widget".into());
                s.invoke_generate();
                step2.set(10);
            }
            10 if turns().len() == 10 && !s.get_busy() => {
                let Some(req) = stub2.last_chat("SQL expert assistant") else {
                    return bail("no generate request".into());
                };
                if system_prompt(&req.body).contains("main.widgets") {
                    return bail("the schema was sent although it was turned off".into());
                }
                // The viewer still shows what is being held back.
                if s.get_context_text().is_empty() {
                    return bail("the context should stay visible when it is off".into());
                }

                // A provider failure is a failed turn, and the panel recovers.
                stub2.fail_chat.store(true, Ordering::SeqCst);
                s.invoke_generate();
                step2.set(11);
            }
            11 if turns().len() == 12 && !s.get_busy() => {
                let failed = turn_at(11).unwrap();
                if !failed.failed || failed.has_sql || !failed.body.contains("boom") {
                    return bail(format!("failure turn: {:?}", failed.body));
                }
                if !s.get_can_generate() {
                    return bail("the panel should be usable again after a failure".into());
                }
                s.invoke_clear();
                if s.get_turns().row_count() != 0 {
                    return bail("Clear should empty the conversation".into());
                }

                // Switch to Ollama, which is not running.
                s.invoke_provider_selected(0);
                step2.set(12);
            }
            12 if s.get_empty_kind() == 2 => {
                if !s.get_empty_title().contains("Ollama") || !s.get_empty_message().contains("127.0.0.1:1") {
                    return bail(format!("ollama state: {:?} / {:?}", s.get_empty_title(), s.get_empty_message()));
                }
                if s.get_needs_key() {
                    return bail("Ollama needs no key".into());
                }
                if s.get_can_generate() {
                    return bail("nothing is askable while Ollama is down".into());
                }
                step2.set(13);
            }
            // The provider and model choices reach disk.
            13 => {
                let saved = service_ai::load_settings().unwrap();
                if saved.active_provider == AiProviderType::Ollama && saved.openai.default_model == "gpt-4o" {
                    s.invoke_provider_selected(1);
                    step2.set(14);
                } else if started.elapsed() > Duration::from_secs(35) {
                    bail(format!(
                        "settings never saved the choices: {:?} {}",
                        saved.active_provider, saved.openai.default_model
                    ));
                }
            }
            // Back to OpenAI: the key is still there, so it connects, and
            // the remembered model is chosen.
            14 if s.get_empty_kind() == -1 && s.get_models().row_count() == 2 => {
                if s.get_model_text().contains("Mini") || !s.get_model_text().starts_with("GPT-4o") {
                    return bail(format!("the remembered model should be back, got {:?}", s.get_model_text()));
                }
                s.invoke_remove_key();
                step2.set(15);
            }
            15 if s.get_empty_kind() == 1 => {
                if store2.get(AiProviderType::OpenAI).is_some() || s.get_key_saved() {
                    return bail("the key should be gone".into());
                }
                // Nothing the user saw ever held the key.
                let toasts = toast_texts(&w).join(" | ");
                if toasts.contains(KEY) {
                    return bail("a toast showed the key".into());
                }
                // The header button's path: close, then open again.
                s.invoke_close();
                if s.get_visible() {
                    return bail("Close should hide the panel".into());
                }
                s.invoke_open_requested();
                if !s.get_visible() {
                    return bail("the header button's open request should show the panel".into());
                }
                // Leaving the workspace closes the panel.
                w.invoke_workspace_back();
                if s.get_visible() {
                    return bail("leaving the workspace should close the panel".into());
                }
                step2.set(99);
                slint::quit_event_loop().unwrap();
            }
            _ => {}
        }
    });

    w.run().unwrap();
    drop(timer);

    // The key was never written anywhere under the data directory.
    let mut files = Vec::new();
    files_under(&dir2, &mut files);
    let mut leaked = Vec::new();
    for f in &files {
        if let Ok(bytes) = std::fs::read(f) {
            if find(&bytes, KEY.as_bytes()).is_some() {
                leaked.push(f.display().to_string());
            }
        }
    }
    let ai_json = std::fs::read_to_string(dir2.join("ai.json")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);

    if let Some(msg) = failure.borrow().as_ref() {
        panic!("{msg}");
    }
    assert_eq!(step.get(), 99, "the scenario did not run to its end");
    assert!(leaked.is_empty(), "the API key was written to {leaked:?}");
    assert!(ai_json.contains("openai"), "ai.json should hold the non-secret settings");
}
