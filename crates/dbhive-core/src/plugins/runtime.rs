//! The JavaScript runtime plugins execute in (Boa).
//!
//! Ported from the Tauri shell. What changed:
//!
//! * No `AppHandle`. The runtime is constructed with the plugin's data
//!   directory and nothing else, so it can run under any shell.
//! * Where the old runtime emitted a Tauri event (notifications, UI
//!   registration), it now records a [`HostCall`] that the caller collects
//!   with [`PluginRuntimeSync::take_outbox`]. Events were the only reason
//!   this file knew about a window.
//! * A plugin describes its interface as data (`super::ui::PluginUi`) rather
//!   than returning something a React renderer would interpret. `setUi` and
//!   `renderUi` parse and validate that descriptor here, at the boundary, so
//!   nothing downstream has to.
//! * Function names are validated before they are used to build a call
//!   expression. The old `call_function` interpolated the caller's string
//!   straight into `eval`, which was code injection into the host.
//!
//! `boa_engine::Context` is neither `Send` nor `Sync`, so a runtime lives for
//! one call on one thread: the async wrapper in `super::manager` creates it
//! inside `spawn_blocking`, runs the plugin, and drops it. That is also why
//! the outbox is a thread-local rather than shared state.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::path::PathBuf;

use boa_engine::{
    js_string, native_function::NativeFunction, object::ObjectInitializer, Context, JsArgs,
    JsNativeError, JsValue, Source,
};
use serde_json::Value as JsonValue;

use super::api::{Notification, NotificationKind};
use super::error::{PluginError, PluginResult};
use super::manifest::{Plugin, PluginPermission};
use super::ui::{self, PluginUi};

/// Cap on anything a plugin hands back through `JSON.stringify`.
pub const MAX_RESULT_BYTES: usize = 1024 * 1024;

/// Loop iterations a single plugin call may run.
///
/// Boa is an interpreter with no preemption: without this a `while (true) {}`
/// in a plugin pins a worker thread forever and the only cure is killing the
/// application.
pub const MAX_LOOP_ITERATIONS: u64 = 20_000_000;
pub const MAX_RECURSION_DEPTH: usize = 400;

/// Something a plugin asked the host to do while it was running.
///
/// The plugin cannot do any of these itself; it can only ask, and the host
/// decides. Collected with [`PluginRuntimeSync::take_outbox`] after the call
/// returns, so a plugin cannot interleave with the UI thread.
#[derive(Debug, Clone, PartialEq)]
pub enum HostCall {
    /// Show a toast.
    Notify(Notification),
    /// Replace the plugin's interface with this descriptor.
    SetUi(Box<PluginUi>),
    /// Ask for a plugin view to be opened.
    OpenTab { title: String },
    /// Message another plugin.
    Message { to: String, body: String },
}

thread_local! {
    /// Outbox for the runtime currently executing on this thread.
    ///
    /// A thread-local rather than a capture: Boa's native functions take
    /// `Copy` captures that must be traceable by its GC, and threading shared
    /// mutable state through that is far more machinery than a runtime that
    /// lives for one call on one thread needs.
    static OUTBOX: RefCell<Vec<HostCall>> = const { RefCell::new(Vec::new()) };
}

fn push_call(call: HostCall) {
    OUTBOX.with(|o| o.borrow_mut().push(call));
}

/// Enforce a manifest-declared permission at an API call site.
fn require_permission(granted: bool, permission: &str) -> boa_engine::JsResult<()> {
    if granted {
        Ok(())
    } else {
        Err(JsNativeError::error()
            .with_message(format!("Permission denied: {}", permission))
            .into())
    }
}

fn is_blocked_ipv4(ip: &Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
}

/// Loopback, link-local, RFC1918 private, unspecified and broadcast addresses
/// are off-limits for plugin HTTP requests (SSRF guard).
fn is_blocked_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_ipv4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_ipv4(&mapped);
            }
            let segments = v6.segments();
            v6.is_loopback()
                || v6.is_unspecified()
                || (segments[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (segments[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
        }
    }
}

/// Validate that a plugin-supplied URL is http(s) and does not target a
/// loopback, link-local or private host (literal IP or via DNS).
pub fn validate_outbound_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("invalid URL: {}", e))?;

    match parsed.scheme() {
        "http" | "https" => {}
        scheme => return Err(format!("scheme '{}' not allowed (http/https only)", scheme)),
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?;
    let host = host
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();

    if host == "localhost" || host.ends_with(".localhost") {
        return Err("requests to localhost are not allowed".to_string());
    }

    let port = parsed.port_or_known_default().unwrap_or(443);
    let addrs = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("failed to resolve host '{}': {}", host, e))?;

    for addr in addrs {
        if is_blocked_ip(&addr.ip()) {
            return Err(format!(
                "host '{}' resolves to blocked address {} (loopback/link-local/private)",
                host,
                addr.ip()
            ));
        }
    }

    Ok(())
}

/// Synchronous JavaScript runtime for one plugin.
pub struct PluginRuntimeSync {
    context: Context,
    plugin_id: String,
    /// Manifest permissions, as strings, for capture in native closures.
    permissions: HashSet<String>,
    data_dir: PathBuf,
    config_str: Option<String>,
}

impl PluginRuntimeSync {
    /// `data_dir` is the plugin's own writable directory; it is created if
    /// absent and is the only path the file API can reach.
    pub fn new(plugin: &Plugin, data_dir: PathBuf) -> PluginResult<Self> {
        if !plugin.may_execute() {
            return Err(PluginError::ExecutionError(format!(
                "{} is disabled or not a JavaScript plugin",
                plugin.manifest.id
            )));
        }

        let mut context = Context::default();

        // Bound the interpreter before any plugin code can reach it. If a
        // future Boa release renames these, they can be dropped: everything
        // else here still works, but an endless loop in a plugin will then
        // hang its worker thread.
        context
            .runtime_limits_mut()
            .set_loop_iteration_limit(MAX_LOOP_ITERATIONS);
        context
            .runtime_limits_mut()
            .set_recursion_limit(MAX_RECURSION_DEPTH);

        std::fs::create_dir_all(&data_dir)?;

        let permissions: HashSet<String> = plugin
            .manifest
            .permissions
            .iter()
            .map(|p| p.as_str().to_string())
            .collect();

        // Start with an empty outbox: this thread may have run another
        // plugin before us.
        OUTBOX.with(|o| o.borrow_mut().clear());

        Ok(Self {
            context,
            plugin_id: plugin.manifest.id.clone(),
            permissions,
            data_dir,
            config_str: plugin.config.as_ref().map(|c| c.to_string()),
        })
    }

    fn has(&self, permission: PluginPermission) -> bool {
        self.permissions.contains(permission.as_str())
    }

    /// Everything the plugin asked the host to do, oldest first. Draining.
    pub fn take_outbox(&self) -> Vec<HostCall> {
        OUTBOX.with(|o| std::mem::take(&mut *o.borrow_mut()))
    }

    pub fn initialize(&mut self) -> PluginResult<()> {
        self.setup_console();
        self.setup_dbhive_api()?;
        Ok(())
    }

    fn setup_console(&mut self) {
        let plugin_id = self.plugin_id.clone();

        let console = ObjectInitializer::new(&mut self.context)
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, plugin_id, ctx| {
                        println!("[Plugin:{}] {}", plugin_id, join_args(args, ctx));
                        Ok(JsValue::undefined())
                    },
                    plugin_id.clone(),
                ),
                js_string!("log"),
                0,
            )
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, plugin_id, ctx| {
                        eprintln!("[Plugin:{}] ERROR: {}", plugin_id, join_args(args, ctx));
                        Ok(JsValue::undefined())
                    },
                    plugin_id.clone(),
                ),
                js_string!("error"),
                0,
            )
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, plugin_id, ctx| {
                        eprintln!("[Plugin:{}] WARN: {}", plugin_id, join_args(args, ctx));
                        Ok(JsValue::undefined())
                    },
                    plugin_id.clone(),
                ),
                js_string!("warn"),
                0,
            )
            .build();

        self.context
            .register_global_property(
                js_string!("console"),
                console,
                boa_engine::property::Attribute::all(),
            )
            .expect("registering console on a fresh context cannot fail");
    }

    fn setup_dbhive_api(&mut self) -> PluginResult<()> {
        let plugin_id = self.plugin_id.clone();
        let data_dir_str = self.data_dir.to_string_lossy().to_string();

        let can_write = self.has(PluginPermission::WriteFiles);
        let can_read = self.has(PluginPermission::ReadFiles);
        let can_clipboard = self.has(PluginPermission::AccessClipboard);
        let can_other_plugins = self.has(PluginPermission::AccessOtherPlugins);
        let can_notify = self.has(PluginPermission::ShowNotification);
        let can_modify_ui = self.has(PluginPermission::ModifyUi);
        let can_create_tab = self.has(PluginPermission::CreateTab);
        let can_network = self.has(PluginPermission::NetworkAccess);

        let config_str = self.config_str.clone();

        let internal = ObjectInitializer::new(&mut self.context)
            // showNotification(title, message, type)
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, has_perm, ctx| {
                        require_permission(*has_perm, "ShowNotification")?;

                        let title = arg_string(args, 0, ctx)?;
                        let message = arg_string(args, 1, ctx)?;
                        let kind = NotificationKind::from_wire(&arg_string(args, 2, ctx)?);

                        push_call(HostCall::Notify(Notification {
                            title: clamp(title, 200),
                            message: clamp(message, 1000),
                            kind,
                        }));
                        Ok(JsValue::Boolean(true))
                    },
                    can_notify,
                ),
                js_string!("showNotification"),
                3,
            )
            // setUi(descriptorJson): the declarative interface.
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, has_perm, ctx| {
                        require_permission(*has_perm, "ModifyUI")?;

                        let raw = arg_string(args, 0, ctx)?;
                        match PluginUi::parse(&raw) {
                            Ok(descriptor) => {
                                push_call(HostCall::SetUi(Box::new(descriptor)));
                                Ok(JsValue::Boolean(true))
                            }
                            // Report the reason to the plugin: a rejected
                            // descriptor is almost always a plugin bug, and
                            // silently dropping it is impossible to debug.
                            Err(e) => Err(JsNativeError::error()
                                .with_message(format!("UI rejected: {}", e))
                                .into()),
                        }
                    },
                    can_modify_ui,
                ),
                js_string!("setUi"),
                1,
            )
            // writeFile(path, content)
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, (data_dir_str, has_perm), ctx| {
                        require_permission(*has_perm, "WriteFiles")?;

                        let path = arg_string(args, 0, ctx)?;
                        let content = arg_string(args, 1, ctx)?;
                        let full_path = sandboxed(data_dir_str, &path)?;

                        if let Some(parent) = full_path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        std::fs::write(&full_path, &content).map_err(|e| {
                            JsNativeError::error()
                                .with_message(format!("Failed to write file: {}", e))
                        })?;
                        Ok(JsValue::Boolean(true))
                    },
                    (data_dir_str.clone(), can_write),
                ),
                js_string!("writeFile"),
                2,
            )
            // readFile(path)
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, (data_dir_str, has_perm), ctx| {
                        require_permission(*has_perm, "ReadFiles")?;

                        let path = arg_string(args, 0, ctx)?;
                        let full_path = sandboxed(data_dir_str, &path)?;
                        let content = std::fs::read_to_string(&full_path).map_err(|e| {
                            JsNativeError::error()
                                .with_message(format!("Failed to read file: {}", e))
                        })?;
                        Ok(JsValue::String(js_string!(content)))
                    },
                    (data_dir_str.clone(), can_read),
                ),
                js_string!("readFile"),
                1,
            )
            // storeData(key, value): key/value storage inside the data dir.
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, data_dir_str, ctx| {
                        let key = arg_string(args, 0, ctx)?;
                        let value = arg_string(args, 1, ctx)?;

                        let storage_path = PathBuf::from(&data_dir_str).join(".storage.json");
                        let mut storage: HashMap<String, String> =
                            match std::fs::read_to_string(&storage_path) {
                                Ok(c) => serde_json::from_str(&c).unwrap_or_default(),
                                Err(_) => HashMap::new(),
                            };
                        storage.insert(key, value);

                        let body = serde_json::to_string_pretty(&storage)
                            .map_err(|e| JsNativeError::error().with_message(e.to_string()))?;
                        std::fs::write(&storage_path, body).map_err(|e| {
                            JsNativeError::error()
                                .with_message(format!("Failed to save storage: {}", e))
                        })?;
                        Ok(JsValue::Boolean(true))
                    },
                    data_dir_str.clone(),
                ),
                js_string!("storeData"),
                2,
            )
            // getData(key)
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, data_dir_str, ctx| {
                        let key = arg_string(args, 0, ctx)?;
                        let storage_path = PathBuf::from(&data_dir_str).join(".storage.json");
                        let storage: HashMap<String, String> =
                            match std::fs::read_to_string(&storage_path) {
                                Ok(c) => serde_json::from_str(&c).unwrap_or_default(),
                                Err(_) => HashMap::new(),
                            };
                        match storage.get(&key) {
                            Some(v) => Ok(JsValue::String(js_string!(v.clone()))),
                            None => Ok(JsValue::null()),
                        }
                    },
                    data_dir_str.clone(),
                ),
                js_string!("getData"),
                1,
            )
            // getConfig()
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, _args, config_str, _ctx| match config_str {
                        Some(ref c) => Ok(JsValue::String(js_string!(c.clone()))),
                        None => Ok(JsValue::null()),
                    },
                    config_str.clone(),
                ),
                js_string!("getConfig"),
                0,
            )
            // createTab(configJson)
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, (plugin_id, has_perm), ctx| {
                        require_permission(*has_perm, "CreateTab")?;

                        let raw = arg_string(args, 0, ctx)?;
                        let title = serde_json::from_str::<JsonValue>(&raw)
                            .ok()
                            .and_then(|v| {
                                v.get("title").and_then(|t| t.as_str()).map(String::from)
                            })
                            .unwrap_or_else(|| plugin_id.clone());
                        push_call(HostCall::OpenTab {
                            title: clamp(title, 120),
                        });
                        Ok(JsValue::Boolean(true))
                    },
                    (plugin_id.clone(), can_create_tab),
                ),
                js_string!("createTab"),
                1,
            )
            // clipboardRead()
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, _args, has_perm, _ctx| {
                        require_permission(*has_perm, "AccessClipboard")?;

                        match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                            Ok(text) => Ok(JsValue::String(js_string!(text))),
                            Err(e) => Err(JsNativeError::error()
                                .with_message(format!("Clipboard read failed: {}", e))
                                .into()),
                        }
                    },
                    can_clipboard,
                ),
                js_string!("clipboardRead"),
                0,
            )
            // clipboardWrite(content)
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, has_perm, ctx| {
                        require_permission(*has_perm, "AccessClipboard")?;

                        let content = arg_string(args, 0, ctx)?;
                        match arboard::Clipboard::new().and_then(|mut c| c.set_text(content)) {
                            Ok(()) => Ok(JsValue::Boolean(true)),
                            Err(e) => Err(JsNativeError::error()
                                .with_message(format!("Clipboard write failed: {}", e))
                                .into()),
                        }
                    },
                    can_clipboard,
                ),
                js_string!("clipboardWrite"),
                1,
            )
            // sendToPlugin(targetId, messageJson)
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, has_perm, ctx| {
                        require_permission(*has_perm, "AccessOtherPlugins")?;

                        let to = arg_string(args, 0, ctx)?;
                        let body = arg_string(args, 1, ctx)?;
                        push_call(HostCall::Message {
                            to: clamp(to, 128),
                            body: clamp(body, 64 * 1024),
                        });
                        Ok(JsValue::Boolean(true))
                    },
                    can_other_plugins,
                ),
                js_string!("sendToPlugin"),
                2,
            )
            // httpRequest(url, method, body)
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, has_perm, ctx| {
                        require_permission(*has_perm, "NetworkAccess")?;

                        let url = arg_string(args, 0, ctx)?;
                        if let Err(e) = validate_outbound_url(&url) {
                            return Err(JsNativeError::error()
                                .with_message(format!("HTTP request blocked: {}", e))
                                .into());
                        }

                        let method = arg_string(args, 1, ctx)?;
                        let body = if args.get_or_undefined(2).is_null_or_undefined() {
                            None
                        } else {
                            Some(arg_string(args, 2, ctx)?)
                        };

                        let client = reqwest::blocking::Client::builder()
                            .timeout(std::time::Duration::from_secs(30))
                            .build()
                            .map_err(|e| {
                                JsNativeError::error()
                                    .with_message(format!("HTTP client failed: {}", e))
                            })?;

                        let request = match method.to_uppercase().as_str() {
                            "POST" => client.post(&url),
                            "PUT" => client.put(&url),
                            "DELETE" => client.delete(&url),
                            "PATCH" => client.patch(&url),
                            _ => client.get(&url),
                        };
                        let request = match body {
                            Some(b) => request.header("Content-Type", "application/json").body(b),
                            None => request,
                        };

                        match request.send() {
                            Ok(response) => {
                                let status = response.status().as_u16();
                                let text = response.text().unwrap_or_default();
                                let result = serde_json::json!({
                                    "status": status,
                                    "headers": {},
                                    "body": text,
                                });
                                Ok(JsValue::String(js_string!(result.to_string())))
                            }
                            Err(e) => Err(JsNativeError::error()
                                .with_message(format!("HTTP request failed: {}", e))
                                .into()),
                        }
                    },
                    can_network,
                ),
                js_string!("httpRequest"),
                3,
            )
            .build();

        self.context
            .register_global_property(
                js_string!("__dbhive_internal__"),
                internal,
                boa_engine::property::Attribute::all(),
            )
            .map_err(|e| PluginError::ExecutionError(e.to_string()))?;

        let api_code = include_str!("js/dbhive_api.js");
        self.context
            .eval(Source::from_bytes(api_code))
            .map_err(|e| PluginError::ExecutionError(e.to_string()))?;

        Ok(())
    }

    /// Evaluate the plugin's source and capture its exports.
    pub fn execute(&mut self, code: &str) -> PluginResult<JsonValue> {
        // The plugin body runs inside an IIFE because Boa's eval scopes do
        // not persist assignments to globalThis.
        let wrapped = format!(
            "(function() {{\n  var __plugin_exports__ = {{}};\n{code}\n  return __plugin_exports__;\n}})()",
            code = code
        );

        let exports = self
            .context
            .eval(Source::from_bytes(&wrapped))
            .map_err(|e| PluginError::ExecutionError(e.to_string()))?;

        self.context
            .register_global_property(
                js_string!("__plugin_exports__"),
                exports,
                boa_engine::property::Attribute::all(),
            )
            .map_err(|e| PluginError::ExecutionError(e.to_string()))?;

        match self.stringify("__plugin_exports__") {
            Ok(Some(raw)) => Ok(serde_json::from_str(&raw).unwrap_or(JsonValue::Null)),
            _ => Ok(JsonValue::Null),
        }
    }

    /// Does the plugin export this function?
    pub fn exports(&mut self, function: &str) -> PluginResult<bool> {
        let function = checked_name(function)?;
        let src = format!(
            "typeof __plugin_exports__ !== 'undefined' && typeof __plugin_exports__.{} === 'function'",
            function
        );
        match self.context.eval(Source::from_bytes(&src)) {
            Ok(v) => Ok(v.as_boolean() == Some(true)),
            Err(e) => Err(PluginError::ExecutionError(e.to_string())),
        }
    }

    /// Call an exported function with one JSON argument, returning its result
    /// as raw JSON text.
    ///
    /// The argument is handed over as a global string and parsed by
    /// `JSON.parse` inside the runtime rather than interpolated into the
    /// source: JSON text is not JavaScript text (U+2028 and U+2029 are line
    /// terminators in JS but legal inside a JSON string), so interpolation
    /// could change the meaning of the program.
    pub fn call_raw(&mut self, function: &str, argument: &JsonValue) -> PluginResult<String> {
        let function = checked_name(function)?;

        if !self.exports(&function)? {
            return Err(PluginError::ExecutionError(format!(
                "Function '{}' not found in plugin exports",
                function
            )));
        }

        let arg_json = serde_json::to_string(argument)?;
        self.context
            .register_global_property(
                js_string!("__dbhive_arg_json__"),
                JsValue::String(js_string!(arg_json)),
                boa_engine::property::Attribute::all(),
            )
            .map_err(|e| PluginError::ExecutionError(e.to_string()))?;

        let src = format!(
            "(function() {{\n\
               var __arg__ = JSON.parse(__dbhive_arg_json__);\n\
               var __result__ = __plugin_exports__.{}(__arg__);\n\
               return JSON.stringify(__result__ === undefined ? null : __result__);\n\
             }})()",
            function
        );

        let value = self
            .context
            .eval(Source::from_bytes(&src))
            .map_err(|e| PluginError::ExecutionError(e.to_string()))?;

        let raw = value
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_else(|| "null".to_string());

        if raw.len() > MAX_RESULT_BYTES {
            return Err(PluginError::ExecutionError(format!(
                "plugin returned {} bytes, limit is {}",
                raw.len(),
                MAX_RESULT_BYTES
            )));
        }
        Ok(raw)
    }

    /// Call an exported function and parse its result.
    pub fn call(&mut self, function: &str, argument: &JsonValue) -> PluginResult<JsonValue> {
        let raw = self.call_raw(function, argument)?;
        Ok(serde_json::from_str(&raw).unwrap_or(JsonValue::String(raw)))
    }

    /// Ask the plugin to describe its interface.
    pub fn render_ui(&mut self) -> PluginResult<PluginUi> {
        let raw = self.call_raw(ui::RENDER_HANDLER, &JsonValue::Null)?;
        PluginUi::parse(&raw).map_err(|e| PluginError::ExecutionError(e.to_string()))
    }

    /// Deliver a resolved UI event. The descriptor it may return replaces the
    /// current one; `None` means "unchanged".
    pub fn dispatch(&mut self, dispatch: &ui::Dispatch) -> PluginResult<Option<PluginUi>> {
        let raw = self.call_raw(&dispatch.function, &dispatch.argument)?;
        if raw == "null" {
            return Ok(None);
        }
        match PluginUi::parse(&raw) {
            Ok(descriptor) if !descriptor.is_empty() => Ok(Some(descriptor)),
            // A handler that returns a result rather than an interface is
            // normal; the interface then arrives through setUi, or not at all.
            _ => Ok(None),
        }
    }

    /// `onLoad` lifecycle hook. Absent is fine.
    pub fn call_on_load(&mut self) -> PluginResult<JsonValue> {
        self.call_optional("onLoad")
    }

    /// `onUnload` lifecycle hook. Absent is fine.
    pub fn call_on_unload(&mut self) -> PluginResult<JsonValue> {
        self.call_optional("onUnload")
    }

    fn call_optional(&mut self, function: &str) -> PluginResult<JsonValue> {
        if !self.exports(function)? {
            return Ok(serde_json::json!({
                "success": true,
                "message": format!("No {} hook defined", function)
            }));
        }
        self.call(function, &JsonValue::Null)
    }

    fn stringify(&mut self, expression: &str) -> PluginResult<Option<String>> {
        let src = format!("JSON.stringify({})", expression);
        match self.context.eval(Source::from_bytes(&src)) {
            Ok(v) => Ok(v.as_string().map(|s| s.to_std_string_escaped())),
            Err(e) => Err(PluginError::ExecutionError(e.to_string())),
        }
    }
}

/// Reject anything that is not a plain identifier before it is used to build
/// a call expression.
fn checked_name(function: &str) -> PluginResult<String> {
    if ui::is_safe_handler_name(function) {
        Ok(function.to_string())
    } else {
        Err(PluginError::ExecutionError(format!(
            "'{}' is not a valid function name",
            function
        )))
    }
}

fn sandboxed(data_dir: &str, path: &str) -> boa_engine::JsResult<PathBuf> {
    if !super::manifest::is_safe_relative_path(path) {
        return Err(JsNativeError::error()
            .with_message("Invalid path: directory traversal not allowed")
            .into());
    }
    Ok(PathBuf::from(data_dir).join(path))
}

fn arg_string(args: &[JsValue], index: usize, ctx: &mut Context) -> boa_engine::JsResult<String> {
    Ok(args
        .get_or_undefined(index)
        .to_string(ctx)?
        .to_std_string_escaped())
}

fn join_args(args: &[JsValue], ctx: &mut Context) -> String {
    args.iter()
        .map(|v| {
            v.to_string(ctx)
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_else(|_| "undefined".to_string())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn clamp(mut s: String, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        s = s.chars().take(max_chars).collect();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    // These cover the pure guards only. Nothing here loads or executes a
    // plugin from disk.

    #[test]
    fn blocks_private_and_loopback_urls() {
        let blocked = [
            "http://localhost/x",
            "http://LOCALHOST:3000/",
            "http://app.localhost/",
            "http://127.0.0.1:8080/",
            "http://[::1]/",
            "http://0.0.0.0/",
            "http://10.0.0.5/",
            "http://172.16.0.1/",
            "http://172.31.255.255/",
            "http://192.168.1.1/",
            "http://169.254.169.254/latest/meta-data",
            "file:///etc/passwd",
            "ftp://example.com/",
        ];
        for url in blocked {
            assert!(validate_outbound_url(url).is_err(), "{} should be blocked", url);
        }
    }

    #[test]
    fn allows_public_addresses() {
        assert!(validate_outbound_url("https://1.1.1.1/").is_ok());
        assert!(validate_outbound_url("http://93.184.216.34/").is_ok());
        // 172.32.x.x is outside the 172.16.0.0/12 private range.
        assert!(validate_outbound_url("http://172.32.0.1/").is_ok());
    }

    #[test]
    fn function_names_are_checked_before_they_reach_eval() {
        assert!(checked_name("onUiEvent").is_ok());
        for evil in ["x();evil();//", "a.b", "", "1x", "a b"] {
            assert!(checked_name(evil).is_err(), "{:?}", evil);
        }
    }

    #[test]
    fn file_paths_stay_in_the_data_directory() {
        assert!(sandboxed("/data/p", "out.csv").is_ok());
        for bad in ["../escape", "/etc/passwd", "a/../../b"] {
            assert!(sandboxed("/data/p", bad).is_err(), "{}", bad);
        }
    }
}
