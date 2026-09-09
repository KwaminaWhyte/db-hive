//! Plugin manager: installed plugins, the marketplace, and the interface a
//! plugin describes.
//!
//! Parity target: the React `PluginsRoute` (2.13 in
//! docs/rust-ui-migration/01-frontend-inventory.md): `PluginMarketplace`,
//! `InstalledPlugins`, `PluginToolbar` and `PluginContext`.
//!
//! # The one thing that had to change
//!
//! In the web shell a plugin extended the interface by handing the front end
//! a descriptor that React rendered. There is no React here, and "render
//! whatever the plugin sends" is not a thing a native toolkit can do safely
//! or at all. The replacement is a **schema-driven interface**: a plugin
//! returns data describing panels, labels, form fields, tables and buttons
//! (`dbhive_core::plugins::ui::PluginUi`), and this file draws that data with
//! ordinary egui widgets, the application palette, and its own event
//! handling. Presses and edits go back to the plugin as typed events.
//!
//! The plugin keeps running as JavaScript under Boa. What it lost is the
//! ability to describe *arbitrary* interface: the vocabulary is a closed enum
//! with no HTML, no script, no image, no URL to open and no colour in it.
//!
//! # What a plugin can and cannot do from here
//!
//! Can: draw from the fixed vocabulary, receive events for controls it
//! declared, read and write its own data directory, raise notifications, make
//! guarded http/https requests, use the clipboard: each only if its manifest
//! declares the matching permission.
//!
//! Cannot: reach a database, connection, credential or driver; touch any file
//! outside its own data directory; run a subprocess; open a socket; cause a
//! call to a function it did not name in the interface currently on screen;
//! or influence anything in this file beyond the data in its descriptor.
//!
//! # Structure
//!
//! Descriptor parsing, manifest parsing, marketplace filtering and the plugin
//! directory all live in `dbhive-core` and are shared with the Tauri shell.
//! Until `pub mod plugins;` is added to `crates/dbhive-core/src/lib.rs` those
//! modules are pulled in by path (see the seam below) so this screen and its
//! tests build today; nothing is duplicated.

// Nothing calls this screen yet: `app.rs` has to add `plugins::show_window`
// (see the entry points below), and that file is out of scope here. Until it
// does, every function in this file is unreachable and rustc says so about
// each one in turn. Delete this attribute once the screen is wired up, so the
// warnings become useful again.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::theme::{text, Palette};
use crate::widgets::results_grid::{self, GridState};

// ---------------------------------------------------------------------------
// The shared plugin core.
//
// These modules live in crates/dbhive-core/src/plugins/ and are the real,
// shared implementation: manifest parsing, the declarative UI schema, the
// marketplace catalogue and the on-disk plugin store.
use dbhive_core::plugins::{error, manifest, marketplace, store, ui};

/// `ui` is the module; `ui` is also every egui parameter in this file.
/// Referring to the descriptor types through this alias keeps the two apart.
use self::ui as descriptor;

use descriptor::{FieldValue, Item, PluginUi, UiEvent};
use manifest::Plugin;
use marketplace::{MarketplacePlugin, SortOption};

// ---------------------------------------------------------------------------
// The JavaScript engine
// ---------------------------------------------------------------------------

/// Calls into the plugin runtime.
///
/// Every function here is async and is driven from a `Job`. That is not
/// incidental: a plugin is untrusted JavaScript, and the sandbox bounds it by
/// loop and recursion count rather than by wall clock, so a hostile plugin
/// can legitimately run for seconds before the runtime stops it. Calling it
/// on the UI thread would freeze the window for exactly that long.
mod engine {
    use std::sync::{Arc, OnceLock};

    use dbhive_core::plugins::{PluginManager, PluginStore};

    use super::descriptor::{PluginUi, UiEvent};

    /// Is the JavaScript engine compiled into this build?
    pub const WIRED: bool = true;

    pub const REASON: &str = "The plugin JavaScript engine is not available.";

    /// What a plugin call produced: a new interface, and anything it asked
    /// the host to show.
    #[derive(Debug, Default, Clone)]
    pub struct Rendered {
        pub descriptor: Option<PluginUi>,
        /// Notifications the plugin raised, already permission-checked.
        pub notes: Vec<String>,
    }

    /// The process-wide manager.
    ///
    /// A `OnceLock` rather than a field on `DbHiveApp` so that plugin state
    /// is shared across windows, like connections are, instead of each window
    /// loading its own copy of every plugin.
    fn manager() -> Result<&'static Arc<PluginManager>, String> {
        static M: OnceLock<Arc<PluginManager>> = OnceLock::new();
        if M.get().is_none() {
            let dir = dbhive_core::storage::app_data_dir().map_err(|e| e.to_string())?;
            let _ = M.set(Arc::new(PluginManager::new(
                PluginStore::under_app_dir(&dir),
                super::app_version(),
            )));
        }
        Ok(M.get().expect("just set"))
    }

    /// Turn the plugin's outbox into things this screen can show.
    ///
    /// `SetUi` wins over the call's own return value: a plugin that asks for
    /// a specific interface should get it.
    fn absorb(
        mut descriptor: Option<PluginUi>,
        outbox: Vec<dbhive_core::plugins::HostCall>,
    ) -> Rendered {
        let mut notes = Vec::new();
        for call in outbox {
            match call {
                dbhive_core::plugins::HostCall::Notify(n) => notes.push(n.message),
                dbhive_core::plugins::HostCall::SetUi(ui) => descriptor = Some(*ui),
                // Nothing routes these yet; the Tauri shell did not either.
                dbhive_core::plugins::HostCall::OpenTab { .. }
                | dbhive_core::plugins::HostCall::Message { .. } => {}
            }
        }
        Rendered { descriptor, notes }
    }

    /// Ask a plugin to describe its interface (`renderUi`).
    pub async fn render(plugin_id: &str) -> Result<Rendered, String> {
        let out = manager()?
            .render_ui(plugin_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(absorb(Some(out.value), out.outbox))
    }

    /// Deliver an event. Resolution against `current` happens in core, so an
    /// event for a control the plugin never declared never becomes a call.
    pub async fn dispatch(
        plugin_id: &str,
        current: &PluginUi,
        event: &UiEvent,
    ) -> Result<Rendered, String> {
        let out = manager()?
            .dispatch_ui_event(plugin_id, current, event)
            .await
            .map_err(|e| e.to_string())?;
        Ok(absorb(out.value, out.outbox))
    }

    /// Load (`onLoad`) or unload a plugin's runtime.
    pub async fn set_loaded(plugin_id: &str, loaded: bool) -> Result<(), String> {
        let m = manager()?;
        if loaded {
            m.load(plugin_id).await.map(|_| ()).map_err(|e| e.to_string())
        } else {
            m.unload(plugin_id).await.map_err(|e| e.to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Installed,
    Marketplace,
}

impl Tab {
    const ALL: [Tab; 2] = [Tab::Installed, Tab::Marketplace];

    fn label(self) -> &'static str {
        match self {
            Tab::Installed => "Installed",
            Tab::Marketplace => "Marketplace",
        }
    }
}

/// The interface one plugin is currently showing, and its live field values.
pub struct View {
    pub plugin_id: String,
    pub descriptor: PluginUi,
    pub fields: descriptor::Fields,
    pub error: Option<String>,
}

/// The settings form generated from a plugin's `configSchema`. It is the same
/// descriptor type a plugin would emit, so there is one renderer, not two.
pub struct Form {
    pub plugin_id: String,
    pub plugin_name: String,
    pub descriptor: PluginUi,
    pub fields: descriptor::Fields,
}

pub struct Plugins {
    pub open: bool,
    pub tab: Tab,

    installed: Vec<Plugin>,
    problems: Vec<String>,
    selected: Option<String>,
    scan_requested: bool,
    scan_job: Job<Result<store::ScanResult, String>>,
    action_job: Job<Result<String, String>>,
    /// Engine calls run here rather than on the frame: plugin JavaScript is
    /// bounded by loop count, not wall clock, so it can legitimately take
    /// seconds. Tagged with the plugin and whether it is opening the view,
    /// since the user may have moved on by the time it lands.
    engine_job: Job<(String, bool, Result<engine::Rendered, String>)>,
    /// Plugin the in-flight action applies to, for per-row spinners.
    busy: Option<String>,

    confirm_uninstall: Option<String>,
    form: Option<Form>,
    view: Option<View>,
    /// One grid state per descriptor table, so sorting survives a redraw.
    grids: HashMap<String, GridState>,

    search: String,
    filters: marketplace::Filters,
    catalog: Vec<MarketplacePlugin>,
}

impl Default for Plugins {
    fn default() -> Self {
        Self {
            open: false,
            tab: Tab::default(),
            installed: Vec::new(),
            problems: Vec::new(),
            selected: None,
            scan_requested: false,
            scan_job: Job::default(),
            action_job: Job::default(),
            engine_job: Job::default(),
            busy: None,
            confirm_uninstall: None,
            form: None,
            view: None,
            grids: HashMap::new(),
            search: String::new(),
            filters: marketplace::Filters::default(),
            catalog: marketplace::catalog(),
        }
    }
}

impl Plugins {
    fn find(&self, id: &str) -> Option<&Plugin> {
        self.installed.iter().find(|p| p.manifest.id == id)
    }

    fn is_installed(&self, id: &str) -> bool {
        self.installed.iter().any(|p| p.manifest.id == id)
    }
}

/// Where plugins live. Same directory the Tauri shell used, so an existing
/// installation is picked up as-is.
fn plugin_store() -> Result<store::PluginStore, String> {
    let dir = dbhive_core::storage::app_data_dir().map_err(|e| e.to_string())?;
    Ok(store::PluginStore::under_app_dir(&dir))
}

fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Overlay presentation, matching Settings and Migrations.
///
/// Add to `app.rs`, next to `settings::show_window(self, ctx);`:
///
/// ```ignore
/// plugins::show_window(self, ctx);
/// ```
pub fn show_window(app: &mut DbHiveApp, ctx: &egui::Context) {
    if !app.screens.plugins.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Plugins")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(940.0, 660.0))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            show(app, ui);
        });
    if !open {
        app.screens.plugins.open = false;
    }
}

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();

    ensure_scanned(app, ui.ctx());
    poll(app);

    let mut actions: Vec<Action> = Vec::new();

    ui.horizontal(|ui| {
        ui.label(text::heading(palette, "Plugins"));
        ui.add_space(12.0);
        let current = app.screens.plugins.tab;
        for tab in Tab::ALL {
            if ui
                .selectable_label(current == tab, tab.label())
                .clicked()
            {
                app.screens.plugins.tab = tab;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let scanning = app.screens.plugins.scan_job.is_running();
            if ui
                .add_enabled(!scanning, egui::Button::new("Refresh"))
                .clicked()
            {
                actions.push(Action::Rescan);
            }
            if scanning {
                ui.add(egui::Spinner::new());
            }
        });
    });
    ui.separator();

    if !engine::WIRED {
        // Surfaced, not logged: without it every "Run" would look broken.
        banner(
            ui,
            palette,
            palette.warning,
            "Plugin execution is disabled in this build. Installing, enabling and \
             configuring plugins work; running their code does not.",
        );
    }

    for problem in app.screens.plugins.problems.clone() {
        banner(
            ui,
            palette,
            palette.destructive,
            &format!("Could not load a plugin: {}", problem),
        );
    }

    match app.screens.plugins.tab {
        Tab::Installed => installed_tab(app, ui, &mut actions),
        Tab::Marketplace => marketplace_tab(app, ui, &mut actions),
    }

    settings_form_window(app, ui.ctx(), &mut actions);
    confirm_uninstall_window(app, ui.ctx(), &mut actions);

    for action in actions {
        apply(app, ui.ctx(), action);
    }
}

/// Toolbar buttons a plugin registered, for the application title bar.
///
/// Add to the `top_bar` in `app.rs`, after the command palette trigger:
///
/// ```ignore
/// crate::screens::plugins::toolbar(app, ui);
/// ```
///
/// The buttons come from the descriptor of the plugin whose view is open, so
/// a plugin cannot leave a button behind after it is closed or disabled.
pub fn toolbar(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    let Some(view) = app.screens.plugins.view.as_ref() else {
        return;
    };
    if view.descriptor.toolbar.is_empty() {
        return;
    }

    let plugin_id = view.plugin_id.clone();
    let items = view.descriptor.toolbar.clone();
    let mut pressed: Option<String> = None;

    ui.separator();
    for item in &items {
        let button = egui::Button::new(format!("{} {}", item.icon.glyph(), item.label)).frame(false);
        let response = ui.add_enabled(item.enabled, button);
        // The tooltip always names the plugin: a button in the application's
        // own title bar should say whose it is.
        let response = match &item.tooltip {
            Some(tip) => response.on_hover_text(format!("{}\n{}", tip, plugin_id)),
            None => response.on_hover_text(text::muted(palette, &plugin_id)),
        };
        if response.clicked() {
            pressed = Some(item.id.clone());
        }
    }

    if let Some(id) = pressed {
        let event = UiEvent::ButtonPressed {
            id,
            fields: app
                .screens
                .plugins
                .view
                .as_ref()
                .map(|v| v.fields.clone())
                .unwrap_or_default(),
        };
        apply(app, ui.ctx(), Action::UiEvent(plugin_id, event));
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

fn ensure_scanned(app: &mut DbHiveApp, ctx: &egui::Context) {
    if app.screens.plugins.scan_requested {
        return;
    }
    app.screens.plugins.scan_requested = true;
    start_scan(app, ctx);
}

fn start_scan(app: &mut DbHiveApp, ctx: &egui::Context) {
    let version = app_version().to_string();
    app.screens
        .plugins
        .scan_job
        .start(app.rt.handle(), ctx, async move {
            let store = plugin_store()?;
            store.ensure_dirs().map_err(|e| e.to_string())?;
            // Small and infrequent, but still off the frame thread.
            tokio::task::spawn_blocking(move || store.scan(&version))
                .await
                .map_err(|e| e.to_string())
        });
}

fn poll(app: &mut DbHiveApp) {
    if let Some(result) = app.screens.plugins.scan_job.take_result() {
        match result {
            Ok(scan) => {
                let s = &mut app.screens.plugins;
                s.installed = scan.plugins;
                s.problems = scan.problems;
                if let Some(selected) = s.selected.clone() {
                    if !s.is_installed(&selected) {
                        s.selected = None;
                        s.view = None;
                    }
                }
                if s.selected.is_none() {
                    s.selected = s.installed.first().map(|p| p.manifest.id.clone());
                }
            }
            Err(e) => {
                app.screens.plugins.problems = vec![e.clone()];
                app.toasts.error(format!("Could not read plugins: {}", e));
            }
        }
    }

    if let Some((plugin_id, opening, result)) = app.screens.plugins.engine_job.take_result() {
        app.screens.plugins.busy = None;
        match result {
            Ok(rendered) => {
                for note in rendered.notes {
                    app.toasts.info(note);
                }
                if opening {
                    let d = rendered.descriptor.unwrap_or_default();
                    app.screens.plugins.view = Some(View {
                        plugin_id,
                        fields: d.initial_fields(),
                        descriptor: d,
                        error: None,
                    });
                } else if let Some(next) = rendered.descriptor {
                    // Only if the user is still looking at this plugin.
                    if let Some(view) = app
                        .screens
                        .plugins
                        .view
                        .as_mut()
                        .filter(|v| v.plugin_id == plugin_id)
                    {
                        view.fields = merge_fields(&next, &view.fields);
                        view.descriptor = next;
                        view.error = None;
                    }
                }
            }
            Err(e) => {
                if let Some(view) = app
                    .screens
                    .plugins
                    .view
                    .as_mut()
                    .filter(|v| v.plugin_id == plugin_id)
                {
                    view.error = Some(e.clone());
                }
                app.toasts.error(e);
            }
        }
    }

    if let Some(result) = app.screens.plugins.action_job.take_result() {
        app.screens.plugins.busy = None;
        match result {
            Ok(message) => app.toasts.success(message),
            Err(e) => app.toasts.error(e),
        }
        app.screens.plugins.scan_requested = false;
    }
}

// ---------------------------------------------------------------------------
// Installed tab
// ---------------------------------------------------------------------------

fn installed_tab(app: &mut DbHiveApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    let palette = app.mode.palette();

    egui::SidePanel::left("plugins_list")
        .exact_width(280.0)
        .resizable(false)
        .frame(
            egui::Frame::none()
                .fill(palette.sidebar)
                .inner_margin(egui::Margin::same(10.0)),
        )
        .show_inside(ui, |ui| {
            let count = app.screens.plugins.installed.len();
            ui.label(text::strong(palette, "Installed Plugins"));
            ui.label(text::muted(
                palette,
                format!("{} plugin{} installed", count, if count == 1 { "" } else { "s" }),
            ));
            ui.add_space(8.0);

            if count == 0 {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(text::strong(palette, "No plugins installed"));
                    ui.label(text::muted(palette, "Visit the marketplace to get started"));
                    if ui.button("Open Marketplace").clicked() {
                        actions.push(Action::GoTo(Tab::Marketplace));
                    }
                });
                return;
            }

            egui::ScrollArea::vertical()
                .id_salt("plugins_list_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let selected = app.screens.plugins.selected.clone();
                    let busy = app.screens.plugins.busy.clone();
                    let rows: Vec<Plugin> = app.screens.plugins.installed.clone();

                    for plugin in &rows {
                        let id = &plugin.manifest.id;
                        let is_selected = selected.as_deref() == Some(id.as_str());
                        let toggling = busy.as_deref() == Some(id.as_str());

                        let response = egui::Frame::none()
                            .fill(if is_selected { palette.accent } else { palette.card })
                            .stroke(egui::Stroke::new(
                                1.0_f32,
                                if is_selected { palette.primary } else { palette.border },
                            ))
                            .rounding(egui::Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(text::strong(palette, &plugin.manifest.name));
                                        ui.label(text::muted(
                                            palette,
                                            format!(
                                                "v{} • {}",
                                                plugin.manifest.version,
                                                plugin.manifest.author.name
                                            ),
                                        ));
                                        ui.label(text::muted(
                                            palette,
                                            plugin.manifest.category.as_str(),
                                        ));
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let mut enabled = plugin.enabled;
                                            let toggle = ui.add_enabled(
                                                !toggling,
                                                egui::Checkbox::new(&mut enabled, ""),
                                            );
                                            if toggle.changed() {
                                                actions.push(Action::SetEnabled(
                                                    id.clone(),
                                                    enabled,
                                                ));
                                            }
                                            if toggling {
                                                ui.add(egui::Spinner::new());
                                            }
                                        },
                                    );
                                });
                            })
                            .response;

                        if response.interact(egui::Sense::click()).clicked() {
                            actions.push(Action::Select(id.clone()));
                        }
                        ui.add_space(6.0);
                    }
                });
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::none().inner_margin(egui::Margin::same(14.0)))
        .show_inside(ui, |ui| {
            let selected = app.screens.plugins.selected.clone();
            let Some(plugin) = selected.and_then(|id| app.screens.plugins.find(&id).cloned())
            else {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(text::strong(palette, "Select a plugin"));
                    ui.label(text::muted(
                        palette,
                        "Choose a plugin from the list to view details",
                    ));
                });
                return;
            };
            detail(app, ui, &plugin, actions);
        });
}

fn detail(app: &mut DbHiveApp, ui: &mut egui::Ui, plugin: &Plugin, actions: &mut Vec<Action>) {
    let palette = app.mode.palette();
    let id = plugin.manifest.id.clone();
    let busy = app.screens.plugins.busy.as_deref() == Some(id.as_str());

    egui::ScrollArea::vertical()
        .id_salt("plugin_detail_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(text::heading(palette, &plugin.manifest.name));
                ui.label(text::muted(palette, format!("v{}", plugin.manifest.version)));
                if plugin.enabled {
                    ui.colored_label(palette.success, "Enabled");
                } else {
                    ui.label(text::muted(palette, "Disabled"));
                }
            });
            ui.label(text::muted(palette, &plugin.manifest.description));
            ui.add_space(8.0);

            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    let can_run = plugin.enabled && engine::WIRED;
                    let run = ui.add_enabled(can_run, egui::Button::new("Open interface"));
                    let run = if engine::WIRED {
                        run
                    } else {
                        run.on_disabled_hover_text(engine::REASON)
                    };
                    if run.clicked() {
                        actions.push(Action::OpenView(id.clone()));
                    }

                    if plugin.manifest.config_schema.is_some()
                        && ui.button("Settings").clicked()
                    {
                        actions.push(Action::OpenSettings(id.clone()));
                    }

                    let label = if plugin.enabled { "Disable" } else { "Enable" };
                    if ui.button(label).clicked() {
                        actions.push(Action::SetEnabled(id.clone(), !plugin.enabled));
                    }

                    if ui.button("Uninstall").clicked() {
                        actions.push(Action::AskUninstall(id.clone()));
                    }
                    if busy {
                        ui.add(egui::Spinner::new());
                    }
                });
            });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);

            ui.columns(2, |columns| {
                card(&mut columns[0], palette, "Information", |ui| {
                    row(ui, palette, "Author", &plugin.manifest.author.name);
                    row(ui, palette, "License", &plugin.manifest.license);
                    row(ui, palette, "Category", plugin.manifest.category.label());
                    row(ui, palette, "Type", plugin.manifest.plugin_type.label());
                    row(ui, palette, "Identifier", &plugin.manifest.id);
                });
                card(&mut columns[1], palette, "Statistics", |ui| {
                    row(ui, palette, "Installed", &short_date(&plugin.stats.install_date));
                    row(
                        ui,
                        palette,
                        "Executions",
                        &plugin.stats.execution_count.to_string(),
                    );
                    ui.horizontal(|ui| {
                        ui.label(text::muted(palette, "Errors"));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if plugin.stats.error_count > 0 {
                                ui.colored_label(
                                    palette.destructive,
                                    plugin.stats.error_count.to_string(),
                                );
                            } else {
                                ui.label("0");
                            }
                        });
                    });
                    if let Some(last) = &plugin.stats.last_used {
                        row(ui, palette, "Last used", &short_date(last));
                    }
                });
            });

            ui.add_space(10.0);
            card(ui, palette, "Permissions", |ui| {
                let declared = plugin.manifest.declared_permissions();
                if declared.is_empty() {
                    ui.label(text::muted(palette, "No special permissions required"));
                }
                for permission in declared {
                    ui.horizontal(|ui| {
                        if permission.is_sensitive() {
                            ui.colored_label(palette.warning, "!");
                        } else {
                            ui.label(" ");
                        }
                        ui.label(text::strong(palette, permission.label()));
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(18.0);
                        ui.label(text::muted(palette, permission.detail()));
                    });
                }
            });

            if !plugin.manifest.keywords.is_empty() {
                ui.add_space(10.0);
                card(ui, palette, "Keywords", |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for keyword in &plugin.manifest.keywords {
                            ui.label(text::muted(palette, keyword));
                        }
                    });
                });
            }

            let links = [
                ("Homepage", plugin.manifest.homepage.as_ref()),
                ("Repository", plugin.manifest.repository.as_ref()),
            ];
            if links.iter().any(|(_, url)| url.is_some()) {
                ui.add_space(10.0);
                card(ui, palette, "Links", |ui| {
                    for (label, url) in links {
                        if let Some(url) = url {
                            // The URL is shown in full: it comes from the
                            // plugin, and the user should see where a click
                            // goes before making it.
                            ui.horizontal(|ui| {
                                ui.label(text::muted(palette, label));
                                ui.hyperlink_to(url, url);
                            });
                        }
                    }
                });
            }

            // The plugin's own interface, if it has one open.
            let showing = app
                .screens
                .plugins
                .view
                .as_ref()
                .is_some_and(|v| v.plugin_id == id);
            if showing {
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(6.0);
                plugin_view(app, ui, actions);
            }
        });
}

/// Draw the descriptor a plugin returned, and collect what the user did.
fn plugin_view(app: &mut DbHiveApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    let palette = app.mode.palette();
    let Some(view) = app.screens.plugins.view.as_mut() else {
        return;
    };

    ui.horizontal(|ui| {
        let title = view
            .descriptor
            .title
            .clone()
            .unwrap_or_else(|| "Plugin interface".to_string());
        ui.label(text::strong(palette, title));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                actions.push(Action::CloseView);
            }
        });
    });

    if let Some(err) = view.error.clone() {
        banner(ui, palette, palette.destructive, &err);
    }

    let plugin_id = view.plugin_id.clone();
    let descriptor = view.descriptor.clone();
    let events = render_descriptor(
        ui,
        palette,
        &descriptor,
        &mut view.fields,
        &mut app.screens.plugins.grids,
    );

    for event in events {
        actions.push(Action::UiEvent(plugin_id.clone(), event));
    }
}

// ---------------------------------------------------------------------------
// Marketplace tab
// ---------------------------------------------------------------------------

fn marketplace_tab(app: &mut DbHiveApp, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    let palette = app.mode.palette();

    ui.label(text::muted(
        palette,
        "Discover and install plugins to extend DB Hive",
    ));
    ui.add_space(6.0);

    ui.horizontal_wrapped(|ui| {
        let s = &mut app.screens.plugins;
        ui.add(
            egui::TextEdit::singleline(&mut s.search)
                .hint_text("Search plugins")
                .desired_width(220.0),
        );
        s.filters.search = s.search.clone();

        let current = s
            .filters
            .category
            .map(|c| c.label())
            .unwrap_or("All categories");
        egui::ComboBox::from_id_salt("plugins_category")
            .selected_text(current)
            .width(180.0)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(s.filters.category.is_none(), "All categories")
                    .clicked()
                {
                    s.filters.category = None;
                }
                for category in manifest::PluginCategory::ALL {
                    if ui
                        .selectable_label(s.filters.category == Some(category), category.label())
                        .clicked()
                    {
                        s.filters.category = Some(category);
                    }
                }
            });

        egui::ComboBox::from_id_salt("plugins_sort")
            .selected_text(s.filters.sort.label())
            .width(170.0)
            .show_ui(ui, |ui| {
                for sort in SortOption::ALL {
                    if ui
                        .selectable_label(s.filters.sort == sort, sort.label())
                        .clicked()
                    {
                        s.filters.sort = sort;
                    }
                }
            });

        ui.checkbox(&mut s.filters.verified_only, "Verified only");
    });

    ui.add_space(8.0);

    let listing = marketplace::filter_and_sort(
        &app.screens.plugins.catalog,
        &app.screens.plugins.filters,
    );

    if listing.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(text::strong(palette, "No plugins match"));
            ui.label(text::muted(palette, "Try a different search or category"));
        });
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("marketplace_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for entry in &listing {
                let id = entry.manifest.id.clone();
                let installed = app.screens.plugins.is_installed(&id);
                let installing = app.screens.plugins.busy.as_deref() == Some(id.as_str());

                egui::Frame::none()
                    .fill(palette.card)
                    .stroke(egui::Stroke::new(1.0_f32, palette.border))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width() - 4.0);
                        ui.horizontal(|ui| {
                            ui.label(text::strong(palette, &entry.manifest.name));
                            if entry.verified {
                                ui.colored_label(palette.success, "Verified");
                            }
                            ui.label(text::muted(
                                palette,
                                format!(
                                    "v{} • {}",
                                    entry.manifest.version, entry.manifest.author.name
                                ),
                            ));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if installed {
                                        ui.label(text::muted(palette, "Installed"));
                                    } else if installing {
                                        ui.add(egui::Spinner::new());
                                        ui.label(text::muted(palette, "Installing…"));
                                    } else if ui.button("Install").clicked() {
                                        actions.push(Action::Install(id.clone()));
                                    }
                                },
                            );
                        });
                        ui.label(text::muted(palette, &entry.manifest.description));
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(text::muted(
                                palette,
                                format!(
                                    "{} downloads",
                                    marketplace::format_downloads(entry.stats.downloads)
                                ),
                            ));
                            ui.label(text::muted(
                                palette,
                                format!("{:.1} rating", entry.stats.rating),
                            ));
                            ui.label(text::muted(
                                palette,
                                marketplace::format_size(entry.size),
                            ));
                            ui.label(text::muted(palette, entry.manifest.category.label()));
                            for keyword in entry.manifest.keywords.iter().take(4) {
                                ui.label(text::muted(palette, keyword));
                            }
                        });
                        if !entry.manifest.permissions.is_empty() {
                            ui.label(text::muted(
                                palette,
                                format!(
                                    "Requests: {}",
                                    manifest::permission_summary(&entry.manifest.permissions)
                                ),
                            ));
                        }
                        if let Some(homepage) = &entry.manifest.homepage {
                            ui.hyperlink_to("Homepage", homepage);
                        }
                    });
                ui.add_space(8.0);
            }
        });
}

// ---------------------------------------------------------------------------
// The descriptor renderer
// ---------------------------------------------------------------------------

/// Draw a plugin-supplied interface and return what the user did to it.
///
/// Every value the plugin sent has already been validated and clamped by
/// `PluginUi::parse`, so this function only has to draw. It never calls into
/// the plugin; events are returned for the caller to dispatch after drawing,
/// which is also the immediate-mode rule about not mutating during a borrow.
fn render_descriptor(
    ui: &mut egui::Ui,
    palette: &Palette,
    plugin_ui: &PluginUi,
    fields: &mut descriptor::Fields,
    grids: &mut HashMap<String, GridState>,
) -> Vec<UiEvent> {
    let mut events = Vec::new();

    if !plugin_ui.menu.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.menu_button("Plugin menu", |ui| {
                for item in &plugin_ui.menu {
                    let label = match &item.shortcut_hint {
                        Some(hint) => format!("{}   {}", item.label, hint),
                        None => item.label.clone(),
                    };
                    if ui
                        .add_enabled(item.enabled, egui::Button::new(label))
                        .clicked()
                    {
                        events.push(UiEvent::MenuSelected {
                            id: item.id.clone(),
                        });
                        ui.close_menu();
                    }
                }
            });
        });
    }

    if !plugin_ui.toolbar.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for item in &plugin_ui.toolbar {
                let button = egui::Button::new(format!("{} {}", item.icon.glyph(), item.label));
                let mut response = ui.add_enabled(item.enabled, button);
                if let Some(tip) = &item.tooltip {
                    response = response.on_hover_text(tip);
                }
                if response.clicked() {
                    events.push(UiEvent::ButtonPressed {
                        id: item.id.clone(),
                        fields: fields.clone(),
                    });
                }
            }
        });
        ui.add_space(4.0);
    }

    for (index, panel) in plugin_ui.panels.iter().enumerate() {
        let mut body = |ui: &mut egui::Ui, events: &mut Vec<UiEvent>| {
            for (i, item) in panel.items.iter().enumerate() {
                render_item(ui, palette, item, fields, grids, (index, i), events);
            }
        };

        if panel.collapsible {
            egui::CollapsingHeader::new(text::strong(palette, &panel.title))
                .id_salt(("plugin_panel", index))
                .default_open(panel.open)
                .show(ui, |ui| body(ui, &mut events));
        } else {
            card(ui, palette, &panel.title, |ui| body(ui, &mut events));
        }
        ui.add_space(8.0);
    }

    events
}

fn render_item(
    ui: &mut egui::Ui,
    palette: &Palette,
    item: &Item,
    fields: &mut descriptor::Fields,
    grids: &mut HashMap<String, GridState>,
    position: (usize, usize),
    events: &mut Vec<UiEvent>,
) {
    match item {
        Item::Text { text: body, style } => {
            use descriptor::TextStyle as S;
            match style {
                S::Body => {
                    ui.label(body);
                }
                S::Heading => {
                    ui.label(text::heading(palette, body));
                }
                S::Strong => {
                    ui.label(text::strong(palette, body));
                }
                S::Muted => {
                    ui.label(text::muted(palette, body));
                }
                S::Success => {
                    ui.colored_label(palette.success, body);
                }
                S::Warning => {
                    ui.colored_label(palette.warning, body);
                }
                S::Error => {
                    ui.colored_label(palette.destructive, body);
                }
            }
        }

        Item::Separator => {
            ui.separator();
        }

        Item::Button {
            id,
            label,
            tooltip,
            style,
            enabled,
            ..
        } => {
            use descriptor::ButtonStyle as B;
            let button = match style {
                B::Primary => egui::Button::new(
                    egui::RichText::new(label).color(palette.primary_foreground),
                )
                .fill(palette.primary),
                B::Danger => egui::Button::new(
                    egui::RichText::new(label).color(palette.destructive),
                ),
                B::Normal => egui::Button::new(label),
            };
            let mut response = ui.add_enabled(*enabled, button);
            if let Some(tip) = tooltip {
                response = response.on_hover_text(tip);
            }
            if response.clicked() {
                events.push(UiEvent::ButtonPressed {
                    id: id.clone(),
                    fields: fields.clone(),
                });
            }
        }

        Item::Input {
            id,
            label,
            placeholder,
            multiline,
            enabled,
            ..
        } => {
            let mut value = fields
                .get(id)
                .map(|v| v.as_text())
                .unwrap_or_default();
            ui.label(text::muted(palette, label));
            let response = ui.add_enabled_ui(*enabled, |ui| {
                if *multiline {
                    ui.add(
                        egui::TextEdit::multiline(&mut value)
                            .id_salt(("plugin_input", position))
                            .desired_width(f32::INFINITY),
                    )
                } else {
                    ui.add(
                        egui::TextEdit::singleline(&mut value)
                            .id_salt(("plugin_input", position))
                            .hint_text(placeholder.as_str())
                            .desired_width(f32::INFINITY),
                    )
                }
            });
            if response.inner.changed() {
                let v = FieldValue::Text(value);
                fields.insert(id.clone(), v.clone());
                events.push(UiEvent::ValueChanged {
                    id: id.clone(),
                    value: v,
                });
            }
        }

        Item::Number {
            id,
            label,
            min,
            max,
            enabled,
            ..
        } => {
            let mut value = match fields.get(id) {
                Some(FieldValue::Number(n)) => *n,
                Some(other) => other.as_text().parse().unwrap_or(0.0),
                None => 0.0,
            };
            ui.horizontal(|ui| {
                ui.label(text::muted(palette, label));
                let mut drag = egui::DragValue::new(&mut value).speed(1.0);
                if let (Some(lo), Some(hi)) = (min, max) {
                    drag = drag.range(*lo..=*hi);
                }
                if ui.add_enabled(*enabled, drag).changed() {
                    let v = FieldValue::Number(value);
                    fields.insert(id.clone(), v.clone());
                    events.push(UiEvent::ValueChanged {
                        id: id.clone(),
                        value: v,
                    });
                }
            });
        }

        Item::Checkbox {
            id, label, enabled, ..
        } => {
            let mut value = matches!(fields.get(id), Some(FieldValue::Bool(true)));
            if ui
                .add_enabled(*enabled, egui::Checkbox::new(&mut value, label))
                .changed()
            {
                let v = FieldValue::Bool(value);
                fields.insert(id.clone(), v.clone());
                events.push(UiEvent::ValueChanged {
                    id: id.clone(),
                    value: v,
                });
            }
        }

        Item::Select {
            id,
            label,
            options,
            enabled,
            ..
        } => {
            let current = fields.get(id).map(|v| v.as_text()).unwrap_or_default();
            let shown = options
                .iter()
                .find(|o| o.value == current)
                .map(|o| o.label.clone())
                .unwrap_or_else(|| current.clone());

            ui.horizontal(|ui| {
                ui.label(text::muted(palette, label));
                ui.add_enabled_ui(*enabled, |ui| {
                    egui::ComboBox::from_id_salt(("plugin_select", position))
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            for option in options {
                                if ui
                                    .selectable_label(option.value == current, &option.label)
                                    .clicked()
                                    && option.value != current
                                {
                                    let v = FieldValue::Text(option.value.clone());
                                    fields.insert(id.clone(), v.clone());
                                    events.push(UiEvent::ValueChanged {
                                        id: id.clone(),
                                        value: v,
                                    });
                                }
                            }
                        });
                });
            });
        }

        Item::Table {
            title,
            columns,
            rows,
        } => {
            if !title.is_empty() {
                ui.label(text::strong(palette, title));
            }
            // Projected into the shared results grid rather than a second
            // table implementation: it virtualises, sorts and filters.
            let result = dbhive_core::drivers::QueryResult::with_data(
                columns.clone(),
                rows.iter()
                    .map(|r| {
                        r.iter()
                            .map(|c| serde_json::Value::String(c.clone()))
                            .collect()
                    })
                    .collect(),
            );
            let key = format!("plugin_table_{}_{}", position.0, position.1);
            let state = grids.entry(key).or_default();
            results_grid::show(ui, palette, &result, state);
        }
    }
}

// ---------------------------------------------------------------------------
// Settings form and confirmations
// ---------------------------------------------------------------------------

fn settings_form_window(app: &mut DbHiveApp, ctx: &egui::Context, actions: &mut Vec<Action>) {
    let palette = app.mode.palette();
    if app.screens.plugins.form.is_none() {
        return;
    }
    let title = app
        .screens
        .plugins
        .form
        .as_ref()
        .map(|f| format!("{} Settings", f.plugin_name))
        .unwrap_or_default();

    let mut open = true;
    egui::Window::new(title)
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(520.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label(text::muted(
                palette,
                "Configure plugin settings. Changes take effect after saving.",
            ));
            ui.add_space(8.0);

            let Some(form) = app.screens.plugins.form.as_mut() else {
                return;
            };
            if form.descriptor.is_empty() {
                ui.label(text::muted(
                    palette,
                    "This plugin has no configurable settings.",
                ));
            } else {
                let d = form.descriptor.clone();
                // Same renderer as a plugin's own interface. A settings form
                // is just a descriptor the host built from configSchema.
                let _ = render_descriptor(
                    ui,
                    palette,
                    &d,
                    &mut form.fields,
                    &mut app.screens.plugins.grids,
                );
            }

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    actions.push(Action::CloseSettings);
                }
                if ui.button("Save Settings").clicked() {
                    actions.push(Action::SaveSettings);
                }
            });
        });

    if !open {
        actions.push(Action::CloseSettings);
    }
}

fn confirm_uninstall_window(app: &mut DbHiveApp, ctx: &egui::Context, actions: &mut Vec<Action>) {
    let palette = app.mode.palette();
    let Some(id) = app.screens.plugins.confirm_uninstall.clone() else {
        return;
    };
    let name = app
        .screens
        .plugins
        .find(&id)
        .map(|p| p.manifest.name.clone())
        .unwrap_or_else(|| id.clone());

    let mut open = true;
    egui::Window::new("Uninstall plugin?")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label(text::muted(
                palette,
                format!(
                    "Uninstall {}? This removes the plugin and all of its stored data, \
                     and cannot be undone.",
                    name
                ),
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    actions.push(Action::CancelUninstall);
                }
                if ui.button("Uninstall").clicked() {
                    actions.push(Action::Uninstall(id.clone()));
                }
            });
        });
    if !open {
        actions.push(Action::CancelUninstall);
    }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

enum Action {
    GoTo(Tab),
    Rescan,
    Select(String),
    SetEnabled(String, bool),
    AskUninstall(String),
    CancelUninstall,
    Uninstall(String),
    Install(String),
    OpenSettings(String),
    CloseSettings,
    SaveSettings,
    OpenView(String),
    CloseView,
    UiEvent(String, UiEvent),
}

fn apply(app: &mut DbHiveApp, ctx: &egui::Context, action: Action) {
    match action {
        Action::GoTo(tab) => app.screens.plugins.tab = tab,

        Action::Rescan => {
            app.screens.plugins.scan_requested = true;
            start_scan(app, ctx);
        }

        Action::Select(id) => {
            if app.screens.plugins.selected.as_deref() != Some(id.as_str()) {
                app.screens.plugins.selected = Some(id);
                app.screens.plugins.view = None;
            }
        }

        Action::SetEnabled(id, enabled) => {
            // Disabling unloads the runtime first, as the web shell did: a
            // disabled plugin must not still be running.
            if !enabled {
                unload(app, &id);
                if app
                    .screens
                    .plugins
                    .view
                    .as_ref()
                    .is_some_and(|v| v.plugin_id == id)
                {
                    app.screens.plugins.view = None;
                }
            }
            let Some(plugin) = app.screens.plugins.find(&id).cloned() else {
                return;
            };
            let name = plugin.manifest.name.clone();
            app.screens.plugins.busy = Some(id.clone());
            start_action(app, ctx, move || {
                let store = plugin_store()?;
                let mut owned = plugin;
                store
                    .set_enabled(&mut owned, enabled)
                    .map_err(|e| e.to_string())?;
                // The runtime is loaded lazily on first use, so enabling
                // does not have to block on it here.
                Ok(format!(
                    "{} {}",
                    name,
                    if enabled { "enabled" } else { "disabled" }
                ))
            });
        }

        Action::AskUninstall(id) => app.screens.plugins.confirm_uninstall = Some(id),
        Action::CancelUninstall => app.screens.plugins.confirm_uninstall = None,

        Action::Uninstall(id) => {
            app.screens.plugins.confirm_uninstall = None;
            let Some(plugin) = app.screens.plugins.find(&id).cloned() else {
                return;
            };
            let name = plugin.manifest.name.clone();
            unload(app, &id);
            if app
                .screens
                .plugins
                .view
                .as_ref()
                .is_some_and(|v| v.plugin_id == id)
            {
                app.screens.plugins.view = None;
            }
            app.screens.plugins.busy = Some(id);
            start_action(app, ctx, move || {
                let store = plugin_store()?;
                store.uninstall(&plugin).map_err(|e| e.to_string())?;
                Ok(format!("{} uninstalled", name))
            });
        }

        Action::Install(id) => {
            let Some(entry) = app
                .screens
                .plugins
                .catalog
                .iter()
                .find(|p| p.manifest.id == id)
                .cloned()
            else {
                return;
            };
            app.screens.plugins.busy = Some(id);
            start_action(app, ctx, move || install(entry));
        }

        Action::OpenSettings(id) => {
            let Some(plugin) = app.screens.plugins.find(&id).cloned() else {
                return;
            };
            let schema = plugin
                .manifest
                .config_schema
                .clone()
                .unwrap_or(serde_json::Value::Null);
            let panel = descriptor::config_schema_panel(&schema, plugin.config.as_ref());
            let form_descriptor = PluginUi {
                title: None,
                toolbar: Vec::new(),
                menu: Vec::new(),
                panels: vec![panel],
            };
            app.screens.plugins.form = Some(Form {
                plugin_id: plugin.manifest.id.clone(),
                plugin_name: plugin.manifest.name.clone(),
                fields: form_descriptor.initial_fields(),
                descriptor: form_descriptor,
            });
        }

        Action::CloseSettings => app.screens.plugins.form = None,

        Action::SaveSettings => {
            let Some(form) = app.screens.plugins.form.take() else {
                return;
            };
            let Some(plugin) = app.screens.plugins.find(&form.plugin_id).cloned() else {
                return;
            };
            let config = descriptor::fields_to_config(&form.fields);
            app.screens.plugins.busy = Some(form.plugin_id.clone());
            start_action(app, ctx, move || {
                let store = plugin_store()?;
                let mut owned = plugin;
                store
                    .save_config(&mut owned, config)
                    .map_err(|e| e.to_string())?;
                Ok("Settings saved".to_string())
            });
        }

        Action::OpenView(id) => {
            app.screens.plugins.busy = Some(id.clone());
            let handle = app.rt.handle().clone();
            app.screens
                .plugins
                .engine_job
                .start(&handle, ctx, async move {
                    let out = engine::render(&id).await;
                    (id, true, out)
                });
        }

        Action::CloseView => app.screens.plugins.view = None,

        Action::UiEvent(id, event) => {
            let Some(current) = app
                .screens
                .plugins
                .view
                .as_ref()
                .filter(|v| v.plugin_id == id)
                .map(|v| v.descriptor.clone())
            else {
                return;
            };
            let handle = app.rt.handle().clone();
            app.screens
                .plugins
                .engine_job
                .start(&handle, ctx, async move {
                    let out = engine::dispatch(&id, &current, &event).await;
                    (id, false, out)
                });
        }
    }
}

/// Run a blocking plugin-directory operation off the frame thread.
/// Drop a plugin's runtime, without waiting for it.
///
/// Nothing depends on the result: the plugin is being disabled or removed
/// either way, and the store write that actually records it is a separate,
/// reported action.
fn unload(app: &DbHiveApp, plugin_id: &str) {
    let id = plugin_id.to_string();
    app.rt.spawn(async move {
        if let Err(e) = engine::set_loaded(&id, false).await {
            eprintln!("Warning: failed to unload plugin {}: {}", id, e);
        }
    });
}

fn start_action<F>(app: &mut DbHiveApp, ctx: &egui::Context, work: F)
where
    F: FnOnce() -> Result<String, String> + Send + 'static,
{
    app.screens
        .plugins
        .action_job
        .start(app.rt.handle(), ctx, async move {
            tokio::task::spawn_blocking(work)
                .await
                .unwrap_or_else(|e| Err(format!("plugin task failed: {}", e)))
        });
}

/// Install a marketplace entry from the copy bundled with the application.
///
/// There is no marketplace server. The Tauri shell wrote a fake "placeholder"
/// plugin when it could not find one; this reports that it cannot install it
/// instead, because a plugin that is not the plugin the user asked for is
/// worse than a refusal.
fn install(entry: MarketplacePlugin) -> Result<String, String> {
    entry.manifest.validate().map_err(|e| e.to_string())?;
    entry
        .manifest
        .check_compatibility(app_version())
        .map_err(|e| e.to_string())?;

    let store = plugin_store()?;
    store.ensure_dirs().map_err(|e| e.to_string())?;

    let source = bundled_source(&entry.manifest.id).ok_or_else(|| {
        format!(
            "{} is not bundled with this build, and downloading plugins is not implemented.",
            entry.manifest.name
        )
    })?;

    store
        .install_from_dir(&entry.manifest.id, &source)
        .map_err(|e| e.to_string())?;
    Ok(format!("{} installed", entry.manifest.name))
}

/// Where a plugin shipped with the application might be.
fn bundled_source(plugin_id: &str) -> Option<PathBuf> {
    let folder = plugin_id.rsplit('.').next().unwrap_or(plugin_id);
    if folder.is_empty() {
        return None;
    }
    let mut roots = vec![PathBuf::from("plugins"), PathBuf::from("../plugins")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.join("plugins"));
            roots.push(dir.join("../Resources/plugins"));
        }
    }
    roots
        .into_iter()
        .map(|root| root.join(folder))
        .find(|c| c.join("manifest.json").is_file())
}

/// Carry the user's edits across a re-render.
///
/// A plugin that returns a fresh descriptor after every keystroke would
/// otherwise reset the field being typed into. Values are kept only for
/// fields the new descriptor still declares.
fn merge_fields(next: &PluginUi, previous: &descriptor::Fields) -> descriptor::Fields {
    let mut fields = next.initial_fields();
    for (id, value) in previous {
        if next.has_field(id) {
            fields.insert(id.clone(), value.clone());
        }
    }
    fields
}

// ---------------------------------------------------------------------------
// Small drawing helpers
// ---------------------------------------------------------------------------

fn card<R>(
    ui: &mut egui::Ui,
    palette: &Palette,
    title: &str,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::none()
        .fill(palette.card)
        .stroke(egui::Stroke::new(1.0_f32, palette.border))
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width() - 4.0);
            if !title.is_empty() {
                ui.label(text::strong(palette, title));
                ui.add_space(6.0);
            }
            body(ui)
        })
        .inner
}

fn row(ui: &mut egui::Ui, palette: &Palette, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(text::muted(palette, label));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(value);
        });
    });
}

fn banner(ui: &mut egui::Ui, palette: &Palette, colour: egui::Color32, message: &str) {
    egui::Frame::none()
        .fill(palette.muted)
        .stroke(egui::Stroke::new(1.0_f32, colour))
        .rounding(egui::Rounding::same(6.0))
        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width() - 4.0);
            ui.colored_label(colour, message);
        });
    ui.add_space(6.0);
}

/// "2026-01-05T10:00:00Z" -> "2026-01-05". The React shell used date-fns;
/// the date part of an RFC 3339 stamp needs no library, and anything that is
/// not one is shown as it arrived rather than as "Invalid Date".
fn short_date(raw: &str) -> String {
    match raw.split('T').next() {
        Some(date) if date.len() == 10 => date.to_string(),
        _ => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The descriptor schema, manifest parsing, dispatch mapping, the
    // marketplace filters and the plugin store are tested in their own files
    // under crates/dbhive-core/src/plugins/, which are compiled into this
    // crate through the seam above. What is left to test here is the screen's
    // own logic.

    fn ui_with_fields() -> PluginUi {
        PluginUi::parse(
            r#"{"panels":[{"title":"p","items":[
                {"type":"input","id":"name","label":"n","value":"default"},
                {"type":"checkbox","id":"flag","label":"f","value":true}
            ]}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn re_rendering_keeps_what_the_user_typed() {
        let d = ui_with_fields();
        let mut edited = d.initial_fields();
        edited.insert("name".into(), FieldValue::Text("typed".into()));

        let merged = merge_fields(&d, &edited);
        assert_eq!(merged["name"], FieldValue::Text("typed".into()));
        assert_eq!(merged["flag"], FieldValue::Bool(true));
    }

    #[test]
    fn fields_the_new_descriptor_dropped_are_not_carried_over() {
        let before = ui_with_fields();
        let mut edited = before.initial_fields();
        edited.insert("name".into(), FieldValue::Text("typed".into()));

        let after = PluginUi::parse(
            r#"{"panels":[{"title":"p","items":[
                {"type":"checkbox","id":"flag","label":"f","value":false}
            ]}]}"#,
        )
        .unwrap();

        let merged = merge_fields(&after, &edited);
        assert!(!merged.contains_key("name"), "a removed field is dropped");
        assert_eq!(
            merged["flag"],
            FieldValue::Bool(true),
            "a surviving field keeps the user's value"
        );
    }

    #[test]
    fn a_brand_new_field_takes_the_plugins_value() {
        let after = ui_with_fields();
        let merged = merge_fields(&after, &descriptor::Fields::new());
        assert_eq!(merged["name"], FieldValue::Text("default".into()));
    }

    #[test]
    fn install_dates_render_without_a_date_library() {
        assert_eq!(short_date("2026-01-05T10:00:00Z"), "2026-01-05");
        assert_eq!(short_date("2026-01-05T10:00:00.123+02:00"), "2026-01-05");
        assert_eq!(short_date("not a date"), "not a date");
        assert_eq!(short_date(""), "");
    }

    #[test]
    fn bundled_lookup_uses_the_last_segment_of_the_id() {
        // No assertion about the filesystem: the point is that odd ids do
        // not panic or build a path out of nothing.
        assert!(bundled_source("").is_none());
        assert!(bundled_source("com.dbhive.does-not-exist").is_none());
    }

    #[test]
    fn a_settings_form_is_built_from_the_config_schema_with_no_javascript() {
        // The parity item "configure it via a dynamically-generated settings
        // form" is pure data, and works whether or not the engine is wired.
        let schema = serde_json::json!({
            "properties": {
                "delimiter": { "type": "string", "enum": [",", ";"], "default": "," },
                "headers": { "type": "boolean", "default": true }
            }
        });
        let panel = descriptor::config_schema_panel(&schema, None);
        let form = PluginUi {
            title: None,
            toolbar: vec![],
            menu: vec![],
            panels: vec![panel],
        };
        let fields = form.initial_fields();
        assert_eq!(fields["delimiter"], FieldValue::Text(",".into()));
        assert_eq!(fields["headers"], FieldValue::Bool(true));

        assert_eq!(
            descriptor::fields_to_config(&fields),
            serde_json::json!({ "delimiter": ",", "headers": true })
        );
    }

    #[test]
    fn the_engine_is_wired_and_refuses_unknown_plugins() {
        // The seam is closed: the runtime is compiled in. An unknown plugin
        // must still be an error rather than a silent empty interface.
        assert!(engine::WIRED);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            assert!(engine::render("com.does.not.exist").await.is_err());
            assert!(engine::set_loaded("com.does.not.exist", true).await.is_err());
            assert!(engine::dispatch(
                "com.does.not.exist",
                &PluginUi::default(),
                &UiEvent::Opened
            )
            .await
            .is_err());
        });
    }
}
