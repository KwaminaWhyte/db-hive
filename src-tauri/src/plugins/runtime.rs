//! JavaScript runtime for plugin execution using Boa engine
//!
//! This module provides a sandboxed JavaScript execution environment for plugins,
//! with access to the DBHive API.

use super::{Plugin, PluginError, PluginPermission, PluginResult};
use boa_engine::{
    js_string, native_function::NativeFunction, object::ObjectInitializer, Context, JsArgs,
    JsNativeError, JsValue, Source,
};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Convert PluginPermission to string for use in captures
fn permission_to_string(p: &PluginPermission) -> String {
    match p {
        PluginPermission::ReadFiles => "ReadFiles".to_string(),
        PluginPermission::WriteFiles => "WriteFiles".to_string(),
        PluginPermission::ExecuteQuery => "ExecuteQuery".to_string(),
        PluginPermission::ModifySchema => "ModifySchema".to_string(),
        PluginPermission::ReadMetadata => "ReadMetadata".to_string(),
        PluginPermission::CreateTab => "CreateTab".to_string(),
        PluginPermission::ModifyUI => "ModifyUI".to_string(),
        PluginPermission::ShowNotification => "ShowNotification".to_string(),
        PluginPermission::NetworkAccess => "NetworkAccess".to_string(),
        PluginPermission::RunCommand => "RunCommand".to_string(),
        PluginPermission::AccessClipboard => "AccessClipboard".to_string(),
        PluginPermission::AccessOtherPlugins => "AccessOtherPlugins".to_string(),
    }
}

/// Synchronous JavaScript runtime for plugin execution
/// This is designed to be used with `spawn_blocking` since boa_engine's Context is not Send/Sync
pub struct PluginRuntimeSync {
    /// Boa JavaScript context
    context: Context,
    /// Plugin metadata
    plugin_id: String,
    /// Plugin permissions as strings (for simpler capture in closures)
    permissions: HashSet<String>,
    /// Plugin data directory
    data_dir: PathBuf,
    /// Plugin configuration as JSON string
    config_str: Option<String>,
}

impl PluginRuntimeSync {
    /// Create a new plugin runtime (synchronous)
    pub fn new(plugin: &Plugin, app_handle: &AppHandle) -> PluginResult<Self> {
        let context = Context::default();

        // Get plugin data directory
        let data_dir = app_handle
            .path()
            .app_data_dir()
            .map_err(|e| PluginError::Other(e.to_string()))?
            .join("plugin-data")
            .join(&plugin.manifest.id);

        // Create data directory if it doesn't exist (sync version)
        std::fs::create_dir_all(&data_dir)?;

        // Convert permissions to strings for easier capture
        let permissions: HashSet<String> = plugin
            .manifest
            .permissions
            .iter()
            .map(permission_to_string)
            .collect();

        // Convert config to string
        let config_str = plugin.config.as_ref().map(|c| c.to_string());

        Ok(Self {
            context,
            plugin_id: plugin.manifest.id.clone(),
            permissions,
            data_dir,
            config_str,
        })
    }

    /// Initialize the runtime with the DBHive API
    pub fn initialize(&mut self) -> PluginResult<()> {
        // Create console object
        self.setup_console();

        // Create DBHive API object
        self.setup_dbhive_api()?;

        // Note: __plugin_exports__ is declared in dbhive_api.js, no need to declare here

        Ok(())
    }

    /// Setup console logging
    fn setup_console(&mut self) {
        let plugin_id = self.plugin_id.clone();

        let console = ObjectInitializer::new(&mut self.context)
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, plugin_id, ctx| {
                        let msg: Vec<String> = args
                            .iter()
                            .map(|v| {
                                v.to_string(ctx)
                                    .map(|s| s.to_std_string_escaped())
                                    .unwrap_or_else(|_| "undefined".to_string())
                            })
                            .collect();
                        println!("[Plugin:{}] {}", plugin_id, msg.join(" "));
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
                        let msg: Vec<String> = args
                            .iter()
                            .map(|v| {
                                v.to_string(ctx)
                                    .map(|s| s.to_std_string_escaped())
                                    .unwrap_or_else(|_| "undefined".to_string())
                            })
                            .collect();
                        eprintln!("[Plugin:{}] ERROR: {}", plugin_id, msg.join(" "));
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
                        let msg: Vec<String> = args
                            .iter()
                            .map(|v| {
                                v.to_string(ctx)
                                    .map(|s| s.to_std_string_escaped())
                                    .unwrap_or_else(|_| "undefined".to_string())
                            })
                            .collect();
                        eprintln!("[Plugin:{}] WARN: {}", plugin_id, msg.join(" "));
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
            .expect("Failed to register console");
    }

    /// Setup the DBHive API object
    fn setup_dbhive_api(&mut self) -> PluginResult<()> {
        let plugin_id = self.plugin_id.clone();
        let data_dir = self.data_dir.clone();
        let data_dir_str = data_dir.to_string_lossy().to_string();

        // Get permission flags as simple booleans
        let can_write = self.permissions.contains("WriteFiles");
        let can_read = self.permissions.contains("ReadFiles");
        let can_clipboard = self.permissions.contains("AccessClipboard");
        let can_other_plugins = self.permissions.contains("AccessOtherPlugins");

        // Config as string
        let config_str = self.config_str.clone();

        // Create internal API object
        let internal = ObjectInitializer::new(&mut self.context)
            // showNotification - simplified synchronous version
            .function(
                NativeFunction::from_copy_closure(move |_this, args, ctx| {
                    let title = args
                        .get_or_undefined(0)
                        .to_string(ctx)?
                        .to_std_string_escaped();
                    let message = args
                        .get_or_undefined(1)
                        .to_string(ctx)?
                        .to_std_string_escaped();
                    let notif_type = args
                        .get_or_undefined(2)
                        .to_string(ctx)?
                        .to_std_string_escaped();

                    println!(
                        "[Plugin Notification] {}: {} (type: {})",
                        title, message, notif_type
                    );

                    Ok(JsValue::Boolean(true))
                }),
                js_string!("showNotification"),
                3,
            )
            // writeFile - synchronous
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, (data_dir_str, has_perm), ctx| {
                        if !has_perm {
                            return Err(JsNativeError::error()
                                .with_message("Permission denied: WriteFiles")
                                .into());
                        }

                        let path = args
                            .get_or_undefined(0)
                            .to_string(ctx)?
                            .to_std_string_escaped();
                        let content = args
                            .get_or_undefined(1)
                            .to_string(ctx)?
                            .to_std_string_escaped();

                        // Validate path
                        if path.contains("..")
                            || path.starts_with('/')
                            || path.starts_with('\\')
                        {
                            return Err(JsNativeError::error()
                                .with_message("Invalid path: directory traversal not allowed")
                                .into());
                        }

                        let data_dir = PathBuf::from(&data_dir_str);
                        let full_path = data_dir.join(&path);
                        if let Some(parent) = full_path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }

                        std::fs::write(&full_path, &content).map_err(|e| {
                            JsNativeError::error()
                                .with_message(format!("Failed to write file: {}", e))
                        })?;

                        println!(
                            "[Plugin] Wrote {} bytes to {:?}",
                            content.len(),
                            full_path
                        );
                        Ok(JsValue::Boolean(true))
                    },
                    (data_dir_str.clone(), can_write),
                ),
                js_string!("writeFile"),
                2,
            )
            // readFile - synchronous
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, (data_dir_str, has_perm), ctx| {
                        if !has_perm {
                            return Err(JsNativeError::error()
                                .with_message("Permission denied: ReadFiles")
                                .into());
                        }

                        let path = args
                            .get_or_undefined(0)
                            .to_string(ctx)?
                            .to_std_string_escaped();

                        // Validate path
                        if path.contains("..")
                            || path.starts_with('/')
                            || path.starts_with('\\')
                        {
                            return Err(JsNativeError::error()
                                .with_message("Invalid path: directory traversal not allowed")
                                .into());
                        }

                        let data_dir = PathBuf::from(&data_dir_str);
                        let full_path = data_dir.join(&path);
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
            // storeData - storage API (read/write directly to disk)
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, data_dir_str, ctx| {
                        let key = args
                            .get_or_undefined(0)
                            .to_string(ctx)?
                            .to_std_string_escaped();
                        let value = args
                            .get_or_undefined(1)
                            .to_string(ctx)?
                            .to_std_string_escaped();

                        let data_dir = PathBuf::from(&data_dir_str);
                        let storage_path = data_dir.join(".storage.json");

                        // Load existing storage
                        let mut storage: HashMap<String, String> =
                            if let Ok(content) = std::fs::read_to_string(&storage_path) {
                                serde_json::from_str(&content).unwrap_or_default()
                            } else {
                                HashMap::new()
                            };

                        storage.insert(key, value);

                        // Save to disk
                        let content = serde_json::to_string_pretty(&storage)
                            .map_err(|e| JsNativeError::error().with_message(e.to_string()))?;
                        std::fs::write(&storage_path, content).map_err(|e| {
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
            // getData - storage API
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, data_dir_str, ctx| {
                        let key = args
                            .get_or_undefined(0)
                            .to_string(ctx)?
                            .to_std_string_escaped();

                        let data_dir = PathBuf::from(&data_dir_str);
                        let storage_path = data_dir.join(".storage.json");

                        // Load storage
                        let storage: HashMap<String, String> =
                            if let Ok(content) = std::fs::read_to_string(&storage_path) {
                                serde_json::from_str(&content).unwrap_or_default()
                            } else {
                                HashMap::new()
                            };

                        match storage.get(&key) {
                            Some(value) => Ok(JsValue::String(js_string!(value.clone()))),
                            None => Ok(JsValue::null()),
                        }
                    },
                    data_dir_str.clone(),
                ),
                js_string!("getData"),
                1,
            )
            // getConfig
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
            // registerUiComponent - stub
            .function(
                NativeFunction::from_copy_closure(move |_this, args, ctx| {
                    let component = args
                        .get_or_undefined(0)
                        .to_string(ctx)?
                        .to_std_string_escaped();
                    println!("[Plugin] Registered UI component: {}", component);
                    Ok(JsValue::Boolean(true))
                }),
                js_string!("registerUiComponent"),
                1,
            )
            // createTab - stub
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, plugin_id, ctx| {
                        let config = args
                            .get_or_undefined(0)
                            .to_string(ctx)?
                            .to_std_string_escaped();
                        let tab_id = format!("plugin-{}-{}", plugin_id, uuid::Uuid::new_v4());
                        println!("[Plugin] Created tab: {} with config: {}", tab_id, config);
                        Ok(JsValue::String(js_string!(tab_id)))
                    },
                    plugin_id.clone(),
                ),
                js_string!("createTab"),
                1,
            )
            // clipboardRead
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, _args, has_perm, _ctx| {
                        if !has_perm {
                            return Err(JsNativeError::error()
                                .with_message("Permission denied: AccessClipboard")
                                .into());
                        }

                        match arboard::Clipboard::new() {
                            Ok(mut clipboard) => match clipboard.get_text() {
                                Ok(text) => Ok(JsValue::String(js_string!(text))),
                                Err(e) => Err(JsNativeError::error()
                                    .with_message(format!("Clipboard read failed: {}", e))
                                    .into()),
                            },
                            Err(e) => Err(JsNativeError::error()
                                .with_message(format!("Clipboard access failed: {}", e))
                                .into()),
                        }
                    },
                    can_clipboard,
                ),
                js_string!("clipboardRead"),
                0,
            )
            // clipboardWrite
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, has_perm, ctx| {
                        if !has_perm {
                            return Err(JsNativeError::error()
                                .with_message("Permission denied: AccessClipboard")
                                .into());
                        }

                        let content = args
                            .get_or_undefined(0)
                            .to_string(ctx)?
                            .to_std_string_escaped();

                        match arboard::Clipboard::new() {
                            Ok(mut clipboard) => match clipboard.set_text(content) {
                                Ok(_) => Ok(JsValue::Boolean(true)),
                                Err(e) => Err(JsNativeError::error()
                                    .with_message(format!("Clipboard write failed: {}", e))
                                    .into()),
                            },
                            Err(e) => Err(JsNativeError::error()
                                .with_message(format!("Clipboard access failed: {}", e))
                                .into()),
                        }
                    },
                    can_clipboard,
                ),
                js_string!("clipboardWrite"),
                1,
            )
            // sendToPlugin - stub
            .function(
                NativeFunction::from_copy_closure_with_captures(
                    move |_this, args, (plugin_id, has_perm), ctx| {
                        if !has_perm {
                            return Err(JsNativeError::error()
                                .with_message("Permission denied: AccessOtherPlugins")
                                .into());
                        }

                        let target_id = args
                            .get_or_undefined(0)
                            .to_string(ctx)?
                            .to_std_string_escaped();
                        let message = args
                            .get_or_undefined(1)
                            .to_string(ctx)?
                            .to_std_string_escaped();

                        println!(
                            "[Plugin:{}] Sent message to {}: {}",
                            plugin_id, target_id, message
                        );

                        Ok(JsValue::Boolean(true))
                    },
                    (plugin_id.clone(), can_other_plugins),
                ),
                js_string!("sendToPlugin"),
                2,
            )
            // onMessage - stub
            .function(
                NativeFunction::from_copy_closure(move |_this, _args, _ctx| {
                    println!("[Plugin] Message handler registered");
                    Ok(JsValue::undefined())
                }),
                js_string!("onMessage"),
                1,
            )
            .build();

        self.context
            .register_global_property(
                js_string!("__dbhive_internal__"),
                internal,
                boa_engine::property::Attribute::all(),
            )
            .map_err(|e| PluginError::ExecutionError(e.to_string()))?;

        // Inject the DBHive wrapper
        let api_code = include_str!("js/dbhive_api.js");
        self.context
            .eval(Source::from_bytes(api_code))
            .map_err(|e| PluginError::ExecutionError(e.to_string()))?;

        Ok(())
    }

    /// Execute plugin code
    pub fn execute(&mut self, code: &str) -> PluginResult<JsonValue> {
        // Wrap plugin code in IIFE to capture exports
        // This is necessary because boa_engine's eval() scopes don't persist globalThis assignments
        let wrapped_code = format!(
            r#"(function() {{
                // Create local __plugin_exports__ that plugin can assign to
                var __plugin_exports__ = {{}};

                // Execute plugin code
                {code}

                // Return exports (plugin may have reassigned __plugin_exports__)
                return __plugin_exports__;
            }})()"#,
            code = code
        );

        match self.context.eval(Source::from_bytes(&wrapped_code)) {
            Ok(exports_val) => {
                // Store the exports object globally so call_function can access it
                self.context
                    .register_global_property(
                        js_string!("__plugin_exports__"),
                        exports_val.clone(),
                        boa_engine::property::Attribute::all(),
                    )
                    .map_err(|e| PluginError::ExecutionError(e.to_string()))?;

                // Return JSON representation
                match self
                    .context
                    .eval(Source::from_bytes("JSON.stringify(__plugin_exports__)"))
                {
                    Ok(val) => {
                        if let Some(s) = val.as_string() {
                            if let Ok(json) =
                                serde_json::from_str::<JsonValue>(&s.to_std_string_escaped())
                            {
                                return Ok(json);
                            }
                        }
                        Ok(JsonValue::Null)
                    }
                    Err(_) => Ok(JsonValue::Null),
                }
            }
            Err(e) => Err(PluginError::ExecutionError(e.to_string())),
        }
    }

    /// Call a specific function on the plugin
    pub fn call_function(&mut self, function_name: &str) -> PluginResult<JsonValue> {
        // Check if function exists in __plugin_exports__
        let check_code = format!(
            "typeof __plugin_exports__ !== 'undefined' && typeof __plugin_exports__.{fn_name} === 'function'",
            fn_name = function_name
        );

        match self.context.eval(Source::from_bytes(&check_code)) {
            Ok(val) => {
                if val.as_boolean() != Some(true) {
                    return Err(PluginError::ExecutionError(format!(
                        "Function '{}' not found in plugin exports",
                        function_name
                    )));
                }
            }
            Err(e) => {
                return Err(PluginError::ExecutionError(format!(
                    "Failed to check function: {}",
                    e
                )));
            }
        }

        // Call the function and return JSON result
        let call_and_stringify = format!(
            r#"(function() {{
                var __result__ = __plugin_exports__.{fn_name}();
                return JSON.stringify(__result__);
            }})()"#,
            fn_name = function_name
        );

        match self.context.eval(Source::from_bytes(&call_and_stringify)) {
            Ok(val) => {
                if let Some(s) = val.as_string() {
                    let str_val = s.to_std_string_escaped();
                    if let Ok(json) = serde_json::from_str::<JsonValue>(&str_val) {
                        return Ok(json);
                    }
                    // If it's not valid JSON, return as string
                    return Ok(JsonValue::String(str_val));
                }
                Ok(JsonValue::Null)
            }
            Err(e) => Err(PluginError::ExecutionError(e.to_string())),
        }
    }

    /// Call the onLoad lifecycle hook
    pub fn call_on_load(&mut self) -> PluginResult<JsonValue> {
        // Check if onLoad exists
        let check_code = "typeof __plugin_exports__.onLoad === 'function'";
        match self.context.eval(Source::from_bytes(check_code)) {
            Ok(val) => {
                if val.as_boolean() == Some(true) {
                    // Call onLoad
                    let call_code = "__plugin_exports__.onLoad()";
                    match self.context.eval(Source::from_bytes(call_code)) {
                        Ok(_) => {
                            println!("[PluginRuntime] onLoad called for {}", self.plugin_id);
                            Ok(serde_json::json!({"success": true, "message": "onLoad executed"}))
                        }
                        Err(e) => {
                            eprintln!("[PluginRuntime] onLoad failed: {}", e);
                            Err(PluginError::ExecutionError(format!("onLoad failed: {}", e)))
                        }
                    }
                } else {
                    Ok(serde_json::json!({"success": true, "message": "No onLoad hook defined"}))
                }
            }
            Err(e) => Err(PluginError::ExecutionError(e.to_string())),
        }
    }
}
