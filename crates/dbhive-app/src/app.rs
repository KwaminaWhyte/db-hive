//! Top-level application state and frame loop.

use std::sync::{Arc, Mutex};

use dbhive_core::state::AppState;

use crate::screens::{
    activity_monitor, backup, command_palette, connection_editor, connections, er_diagram,
    import_export, migrations, plugins, procedures, query_builder, schema_designer, settings,
    table_editor, table_inspector, workspace, Screen, Screens,
};
use crate::theme;

/// The UI state belonging to one window.
///
/// The active window's state stays inline on `DbHiveApp` as ordinary fields,
/// so every screen keeps its `app.screens` / `app.workspace` field access and
/// none of them needed changing. Other windows park their state here and are
/// swapped in for the duration of their own frame. Moving these behind a
/// `current_window` field instead would have rewritten field access in every
/// screen in the crate for no behavioural gain.
///
/// `rt` and `state` are deliberately NOT here. The Tokio runtime and
/// `AppState` (connections, profiles, history) are shared across windows,
/// which is what the Tauri shell did: two windows on the same connection
/// reuse one pool rather than opening a second.
#[derive(Default)]
pub struct WindowState {
    pub screen: Screen,
    pub workspace: Option<workspace::Workspace>,
    pub screens: Screens,
    pub connections: connections::ConnectionsScreen,
    pub toasts: crate::widgets::toast::Toasts,
    pub about_open: bool,
}

pub struct DbHiveApp {
    /// Owned so the runtime lives as long as the app; dropping it would
    /// cancel every in-flight query.
    pub rt: tokio::runtime::Runtime,
    pub state: Arc<Mutex<AppState>>,
    pub mode: theme::Mode,
    pub screen: Screen,
    pub connections: connections::ConnectionsScreen,
    pub workspace: Option<workspace::Workspace>,
    /// State for every other screen and dialog.
    pub screens: Screens,
    pub toasts: crate::widgets::toast::Toasts,
    /// Set when startup could not read the saved data. Shown in the UI
    /// rather than only printed, since it means the user's connections are
    /// missing and they need to know why.
    pub startup_error: Option<String>,
    pub about_open: bool,
    /// Profile to open at startup, from DBHIVE_AUTOCONNECT.
    autoconnect: Option<(String, String)>,
    /// Windows beyond the first. Each is swapped into the fields above while
    /// its own viewport renders.
    pub extra_windows: Vec<WindowState>,
    /// In-flight update check and its last result. App-level rather than
    /// per-window: the running version is the same in every window.
    pub update_job: crate::task::Job<
        Result<dbhive_core::service_updater::UpdateStatus, dbhive_core::models::DbError>,
    >,
    pub update_status: Option<String>,
}

impl DbHiveApp {
    /// Build the app without eframe, for tests.
    ///
    /// Skips reading the user's saved data: a test must not depend on, or
    /// be affected by, whatever is on the machine running it.
    #[cfg(test)]
    pub fn for_test() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("failed to start the Tokio runtime");
        Self {
            rt,
            state: Arc::new(Mutex::new(AppState::new())),
            mode: theme::Mode::Dark,
            screen: Screen::Connections,
            connections: connections::ConnectionsScreen::default(),
            workspace: None,
            screens: Screens::default(),
            toasts: crate::widgets::toast::Toasts::default(),
            startup_error: None,
            about_open: false,
            update_job: Default::default(),
            update_status: None,
            extra_windows: Vec::new(),
            autoconnect: None,
        }
    }

    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to start the Tokio runtime");

        let state = Arc::new(Mutex::new(AppState::new()));

        // Reads the same files the Tauri shell wrote, so an existing
        // installation arrives with its connections already present.
        let startup_error = match dbhive_core::service::load_persisted(&state) {
            Ok(()) => None,
            Err(e) => Some(e.to_string()),
        };

        let mode = theme::Mode::Dark;
        theme::apply(&cc.egui_ctx, mode);

        // Development aid: DBHIVE_AUTOCONNECT=<profile name> opens that
        // connection at startup, so the workspace can be exercised without
        // clicking through the connection list. Unset in normal use.
        let autoconnect = std::env::var("DBHIVE_AUTOCONNECT").ok().and_then(|wanted| {
            let g = state.lock().unwrap();
            g.list_profiles()
                .into_iter()
                .find(|p| p.name == wanted)
                .map(|p| (p.id.clone(), p.name.clone()))
        });

        Self {
            rt,
            state,
            mode,
            screen: Screen::Connections,
            connections: connections::ConnectionsScreen::default(),
            workspace: None,
            screens: Screens::default(),
            toasts: crate::widgets::toast::Toasts::default(),
            startup_error,
            about_open: false,
            update_job: Default::default(),
            update_status: None,
            // Dev aid: DBHIVE_AUTOWINDOWS=<n> opens n extra windows at
            // startup so multi-window can be exercised without clicking.
            extra_windows: (0..std::env::var("DBHIVE_AUTOWINDOWS")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0))
                .map(|_| WindowState::default())
                .collect(),
            autoconnect,
        }
    }
}

impl DbHiveApp {
    /// Exchange the active window's state with `other`.
    ///
    /// Every field is a move, so this costs the same regardless of how much
    /// the window is holding.
    fn swap_window(&mut self, other: &mut WindowState) {
        std::mem::swap(&mut self.screen, &mut other.screen);
        std::mem::swap(&mut self.workspace, &mut other.workspace);
        std::mem::swap(&mut self.screens, &mut other.screens);
        std::mem::swap(&mut self.connections, &mut other.connections);
        std::mem::swap(&mut self.toasts, &mut other.toasts);
        std::mem::swap(&mut self.about_open, &mut other.about_open);
    }

    /// Open another window on the same core.
    pub fn open_window(&mut self) {
        self.extra_windows.push(WindowState::default());
    }
}

impl eframe::App for DbHiveApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Settings own the theme for every window, so they load once here
        // rather than once per window.
        settings::ensure_loaded(self, ctx);
        settings::poll(self);
        settings::ensure_theme(self, ctx);

        render_window(self, ctx);

        // Extra windows render as immediate viewports, inside this same
        // update() call. That is what lets them borrow `self`; a deferred
        // viewport would require the whole app behind an Arc<Mutex>.
        let mut closed: Option<usize> = None;
        for i in 0..self.extra_windows.len() {
            let mut parked = std::mem::take(&mut self.extra_windows[i]);
            self.swap_window(&mut parked);

            let viewport_id = egui::ViewportId::from_hash_of(("dbhive-window", i));
            let builder = egui::ViewportBuilder::default()
                .with_title(format!("DB Hive ({})", i + 2))
                .with_inner_size([1280.0_f32, 800.0_f32])
                .with_min_inner_size([900.0_f32, 600.0_f32]);

            let mut close_requested = false;
            let mut reported = false;
            ctx.show_viewport_immediate(viewport_id, builder, |ctx, class| {
                // Reported once per window so multi-window can be confirmed
                // without a screenshot. `Embedded` would mean the platform
                // refused a real OS window and drew it inside the parent.
                if std::env::var("DBHIVE_LOG_VIEWPORTS").is_ok() && !reported {
                    reported = true;
                    let kind = match class {
                        egui::ViewportClass::Root => "Root",
                        egui::ViewportClass::Deferred => "Deferred",
                        egui::ViewportClass::Immediate => "Immediate (own OS window)",
                        egui::ViewportClass::Embedded => "Embedded (NOT a separate window)",
                    };
                    println!("viewport {} rendered as {}", i + 2, kind);
                }
                render_window(self, ctx);
                if ctx.input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }
            });

            self.swap_window(&mut parked);
            self.extra_windows[i] = parked;
            if close_requested {
                closed = Some(i);
            }
        }
        if let Some(i) = closed {
            self.extra_windows.remove(i);
        }
    }
}

/// Draw one window's whole UI into `ctx`.
///
/// Called once for the main window and once per extra viewport, with that
/// window's state swapped in.
fn render_window(app: &mut DbHiveApp, ctx: &egui::Context) {
    let self_ = app;
    top_bar(self_, ctx);

        if let Some((id, name)) = self_.autoconnect.take() {
            self_.connections.connecting = Some(id.clone());
            let state = self_.state.clone();
            let handle = self_.rt.handle().clone();
            self_.connections.pending_name = Some(name);
            self_.connections.connect_job.start(&handle, ctx, async move {
                dbhive_core::service::connect(&state, id, String::new(), None).await
            });
        }

        // Settings own the theme: loaded once, then applied every frame so a
        // saved mode or accent survives a restart even if Settings is never
        // opened. This must run before anything draws.
        settings::ensure_loaded(self_, ctx);
        settings::poll(self_);
        settings::ensure_theme(self_, ctx);

        let palette = self_.mode.palette();
        self_.toasts.show(ctx, palette);

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(err) = self_.startup_error.clone() {
                ui.horizontal(|ui| {
                    ui.colored_label(self_.mode.palette().destructive, "Could not load saved data:");
                    ui.label(err);
                    if ui.button("Dismiss").clicked() {
                        self_.startup_error = None;
                    }
                });
                ui.separator();
            }

            if self_.screens.table_editor.open {
                table_editor::show(self_, ui);
            } else if self_.screens.activity_monitor.open {
                activity_monitor::show(self_, ui);
            } else if self_.screens.procedures.open {
                procedures::show(self_, ui);
            } else if self_.screens.query_builder.open {
                if let query_builder::BuilderAction::UseSql(sql) =
                    query_builder::show(self_, ui)
                {
                    self_.screens.query_builder.close();
                    self_.screen = Screen::Workspace;
                    if let Some(ws) = self_.workspace.as_mut() {
                        ws.add_tab();
                        let tab = ws.active();
                        tab.title = "Visual Query".to_string();
                        tab.sql = sql;
                    }
                }
            } else if self_.screens.schema_designer.open {
                schema_designer::show(self_, ui);
            } else if self_.screens.er_diagram.open {
                er_diagram::show(self_, ui);
            } else if self_.screens.table_inspector.open {
                table_inspector::show(self_, ui);
            } else {
                match self_.screen {
                    Screen::Connections => connections::show(self_, ui),
                    Screen::Workspace => workspace::show(self_, ui),
                }
            }
        });
        settings::show_window(self_, ctx);
        migrations::show_window(self_, ctx);
        backup::show_window(self_, ctx);
        import_export::show_window(self_, ctx);
        plugins::show_window(self_, ctx);

        if let Some(action) = connection_editor::show(self_, ctx) {
            match action {
                connection_editor::EditorAction::Saved(id) => {
                    self_.toasts.success("Connection saved");
                    self_.connections.last_saved = Some(id);
                }
                connection_editor::EditorAction::Cancelled => {}
            }
        }

        // Last, so the palette draws above every other overlay.
        if let command_palette::PaletteAction::Run(id) = command_palette::show(self_, ctx) {
            dispatch(self_, ctx, id);
        }
}

/// Act on a command from the palette or a keyboard shortcut.
///
/// Theme, window and modal-opening commands are handled inside the palette
/// itself and arrive here only for information; everything else is dispatched
/// here because it needs app state the palette cannot reach.
fn dispatch(app: &mut DbHiveApp, ctx: &egui::Context, id: command_palette::CommandId) {
    use command_palette::CommandId as C;
    match id {
        C::GoConnections | C::ConnectionsBack => {
            app.screens.table_inspector.open = false;
            app.screen = Screen::Connections;
        }
        C::GoQueryEditor => {
            app.screens.table_inspector.open = false;
            if app.workspace.is_some() {
                app.screen = Screen::Workspace;
            } else {
                app.toasts.info("Connect to a database first");
            }
        }
        C::GoSettings => app.screens.settings.open = true,
        C::GoMigrations => app.screens.migrations.open = true,
        C::GoActivityMonitor => app.screens.activity_monitor.open = true,
        C::GoImportExport => match app.workspace.as_ref() {
            Some(ws) => {
                let schema = workspace::current_schema_or_database(ws);
                let conn = ws.connection_id.clone();
                app.screens.import_export.open_for(conn, schema);
            }
            None => app.toasts.info("Connect to a database first"),
        },
        C::GoBackup => app.screens.backup.open = true,
        C::GoPlugins => app.screens.plugins.open = true,
        C::GoAiAssistant => {
            if app.workspace.is_some() {
                app.screens.ai_assistant.open = !app.screens.ai_assistant.open;
            } else {
                app.toasts.info("Connect to a database first");
            }
        }
        C::NewTable => match app.workspace.as_ref() {
            Some(ws) => {
                let schema = workspace::current_schema_or_database(ws);
                let conn = ws.connection_id.clone();
                app.screens.table_editor.open_new(conn, schema);
            }
            None => app.toasts.info("Connect to a database first"),
        },
        C::NewConnection => app.screens.connection_editor.open_new(),
        C::GoErDiagram => match app.workspace.as_ref() {
            Some(ws) => {
                let schema = workspace::current_schema_or_database(ws);
                let conn = ws.connection_id.clone();
                app.screens.er_diagram.open_for(conn, schema);
            }
            None => app.toasts.info("Connect to a database first"),
        },
        C::NewWindow => {
            app.open_window();
            app.toasts.info("Opened a new window");
        }
        C::GoVisualQueryBuilder => match app.workspace.as_ref() {
            Some(ws) => {
                let schema = workspace::current_schema_or_database(ws);
                let conn = ws.connection_id.clone();
                app.screens.query_builder.open_for(conn, schema);
            }
            None => app.toasts.info("Connect to a database first"),
        },
        C::GoSchemaDesigner => match app.workspace.as_ref() {
            Some(ws) => {
                let schema = workspace::current_schema_or_database(ws);
                let conn = ws.connection_id.clone();
                app.screens.schema_designer.open_for(conn, schema);
            }
            None => app.toasts.info("Connect to a database first"),
        },
        C::GoTableInspector => {
            if app.workspace.is_some() {
                app.screens.table_inspector.open = true;
            } else {
                app.toasts.info("Connect to a database first");
            }
        }
        C::RunQuery => {
            if app.workspace.is_some() {
                workspace::execute(app, ctx);
            }
        }
        C::NewTab => {
            if let Some(ws) = app.workspace.as_mut() {
                ws.add_tab();
            }
        }
        C::CloseTab => {
            if let Some(ws) = app.workspace.as_mut() {
                let i = ws.active_tab;
                ws.close_tab(i);
            }
        }
        C::Disconnect => {
            if let Some(ws) = app.workspace.as_ref() {
                let id = ws.connection_id.clone();
                let state = app.state.clone();
                let handle = app.rt.handle().clone();
                app.connections.connected.retain(|c| c != &id);
                app.connections
                    .disconnect_job
                    .start(&handle, ctx, async move {
                        dbhive_core::service::disconnect(&state, &id).await
                    });
                app.workspace = None;
                app.screens.table_inspector.open = false;
                app.screen = Screen::Connections;
            }
        }
        C::RefreshMetadata => workspace::refresh_metadata(app, ctx),
        // Handled inside the palette, or belonging to a screen not yet built.
        other => {
            app.toasts
                .info(format!("{} is not wired up yet", command_palette::command(other).title));
        }
    }
}

fn top_bar(app: &mut DbHiveApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("titlebar")
        .exact_height(40.0)
        .frame(
            egui::Frame::none()
                .fill(app.mode.palette().sidebar)
                .inner_margin(egui::Margin::symmetric(12.0, 0.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(crate::theme::text::strong(app.mode.palette(), "DB Hive"));
                ui.separator();

                menu_bar(app, ui);

                // The web shell centred the command palette trigger and had
                // to fight overlap for it, ending up with the menu items
                // rendering underneath the search box on a narrow window.
                // Here it simply follows the menus and cannot collide.
                ui.add_space(4.0);

                if ui.small_button("Search or run commands…  ⌘K").clicked() {
                    app.screens.command_palette.open();
                }

                // Plugin-contributed toolbar buttons, from the descriptors
                // plugins declare. Nothing here executes plugin code.
                crate::screens::plugins::toolbar(app, ui);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Settings").clicked() {
                        app.screens.settings.open = true;
                    }
                    let (icon, next) = match app.mode {
                        theme::Mode::Dark => ("Light", theme::Mode::Light),
                        theme::Mode::Light => ("Dark", theme::Mode::Dark),
                    };
                    if ui.button(icon).on_hover_text("Toggle theme").clicked() {
                        app.mode = next;
                        theme::apply(ui.ctx(), next);
                    }
                });
            });
        });

    about_window(app, ctx);
}

/// File / Schema / View / Help, matching the web shell's menu bar.
///
/// Items that need a connection are disabled rather than hidden, so the
/// shape of the menu does not shift under the user.
fn menu_bar(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let connected = app.workspace.is_some();
    let mut action: Option<command_palette::CommandId> = None;
    use command_palette::CommandId as C;

    ui.menu_button("File", |ui| {
        if ui.button("New Connection…").clicked() {
            action = Some(C::NewConnection);
            ui.close_menu();
        }
        if ui.button("New Window").clicked() {
            action = Some(C::NewWindow);
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Settings…").clicked() {
            action = Some(C::GoSettings);
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Quit").clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });

    ui.menu_button("Schema", |ui| {
        if ui
            .add_enabled(connected, egui::Button::new("New Table…"))
            .clicked()
        {
            action = Some(C::NewTable);
            ui.close_menu();
        }
        if ui
            .add_enabled(connected, egui::Button::new("ER Diagram"))
            .clicked()
        {
            action = Some(C::GoErDiagram);
            ui.close_menu();
        }
        if ui
            .add_enabled(connected, egui::Button::new("Schema Designer"))
            .clicked()
        {
            action = Some(C::GoSchemaDesigner);
            ui.close_menu();
        }
        ui.separator();
        if ui
            .add_enabled(connected, egui::Button::new("Refresh Metadata"))
            .clicked()
        {
            action = Some(C::RefreshMetadata);
            ui.close_menu();
        }
    });

    ui.menu_button("View", |ui| {
        if ui
            .add_enabled(connected, egui::Button::new("Query Editor"))
            .clicked()
        {
            action = Some(C::GoQueryEditor);
            ui.close_menu();
        }
        if ui
            .add_enabled(connected, egui::Button::new("Table Inspector"))
            .clicked()
        {
            action = Some(C::GoTableInspector);
            ui.close_menu();
        }
        if ui
            .add_enabled(connected, egui::Button::new("Activity Monitor"))
            .clicked()
        {
            action = Some(C::GoActivityMonitor);
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Connections").clicked() {
            action = Some(C::GoConnections);
            ui.close_menu();
        }
        if ui.button("Migrations…").clicked() {
            action = Some(C::GoMigrations);
            ui.close_menu();
        }
        if ui.button("Backup Manager…").clicked() {
            action = Some(C::GoBackup);
            ui.close_menu();
        }
        if ui.button("Import / Export…").clicked() {
            action = Some(C::GoImportExport);
            ui.close_menu();
        }
        if ui.button("Plugins…").clicked() {
            action = Some(C::GoPlugins);
            ui.close_menu();
        }
    });

    ui.menu_button("Help", |ui| {
        if ui.button("Keyboard Shortcuts").clicked() {
            action = Some(C::ShowShortcuts);
            ui.close_menu();
        }
        if ui.button("Documentation").clicked() {
            // The web shell opened these in a browser; `open` does the same
            // without pulling in a dependency for two links.
            let _ = std::process::Command::new(OPEN_CMD)
                .arg("https://github.com/KwaminaWhyte/db-hive/wiki")
                .spawn();
            ui.close_menu();
        }
        if ui.button("GitHub Repository").clicked() {
            let _ = std::process::Command::new(OPEN_CMD)
                .arg("https://github.com/KwaminaWhyte/db-hive")
                .spawn();
            ui.close_menu();
        }
        if ui.button("Check for Updates…").clicked() {
            check_for_updates(app, ui.ctx());
            app.about_open = true;
            ui.close_menu();
        }
        ui.separator();
        if ui.button("About DB Hive").clicked() {
            app.about_open = true;
            ui.close_menu();
        }
    });

    if let Some(id) = action {
        let ctx = ui.ctx().clone();
        dispatch(app, &ctx, id);
    }
}

/// Platform command that opens a URL in the default browser.
#[cfg(target_os = "macos")]
const OPEN_CMD: &str = "open";
#[cfg(target_os = "windows")]
const OPEN_CMD: &str = "explorer";
#[cfg(all(unix, not(target_os = "macos")))]
const OPEN_CMD: &str = "xdg-open";

/// Ask the release manifest whether there is anything newer.
fn check_for_updates(app: &mut DbHiveApp, ctx: &egui::Context) {
    if app.update_job.is_running() {
        return;
    }
    app.update_status = Some("Checking…".into());
    let current = env!("CARGO_PKG_VERSION").to_string();
    let handle = app.rt.handle().clone();
    app.update_job.start(&handle, ctx, async move {
        dbhive_core::service_updater::check(&current).await
    });
}

fn about_window(app: &mut DbHiveApp, ctx: &egui::Context) {
    if let Some(result) = app.update_job.take_result() {
        app.update_status = Some(match result {
            Ok(dbhive_core::service_updater::UpdateStatus::UpToDate { current }) => {
                format!("You are on the latest version ({}).", current)
            }
            Ok(dbhive_core::service_updater::UpdateStatus::Available { latest, .. }) => {
                format!("Version {} is available.", latest)
            }
            Err(e) => format!("Could not check for updates: {}", e),
        });
    }
    if !app.about_open {
        return;
    }
    let palette = app.mode.palette();
    let mut open = app.about_open;
    let checking = app.update_job.is_running();
    let status = app.update_status.clone();
    let update_available = status
        .as_deref()
        .is_some_and(|s| s.starts_with("Version "));
    let mut check_now = false;
    egui::Window::new("About DB Hive")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(crate::theme::text::heading(palette, "DB Hive"));
                ui.label(crate::theme::text::muted(
                    palette,
                    format!("Version {}", env!("CARGO_PKG_VERSION")),
                ));
                ui.add_space(6.0);
                ui.label(crate::theme::text::muted(
                    palette,
                    "A native database client for PostgreSQL, MySQL, SQLite,\nMongoDB, Redis, SQL Server and Turso.",
                ));
                ui.add_space(8.0);
                ui.label(crate::theme::text::muted(
                    palette,
                    format!("{} on {}", std::env::consts::ARCH, std::env::consts::OS),
                ));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!checking, egui::Button::new("Check for updates"))
                        .clicked()
                    {
                        check_now = true;
                    }
                    if checking {
                        ui.add(egui::Spinner::new());
                    }
                });
                if let Some(msg) = status.as_ref() {
                    ui.label(crate::theme::text::muted(palette, msg.clone()));
                }
                if update_available {
                    // Installation is deliberately not automated: releases
                    // are not signed yet, and an updater that runs an
                    // unverified download is a code execution channel.
                    if ui.button("Open the releases page").clicked() {
                        let _ = std::process::Command::new(OPEN_CMD)
                            .arg(dbhive_core::service_updater::RELEASES_URL)
                            .spawn();
                    }
                }
            });
        });
    app.about_open = open;
    if check_now {
        check_for_updates(app, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draw `body` in a headless egui context and return whether it panicked.
    ///
    /// This exercises the drawing code that unit tests deliberately skip.
    /// Layout code is where the index arithmetic lives, so it is exactly
    /// where an out-of-range slice or an unwrap on empty state shows up, and
    /// a panic there takes the whole window down.
    fn draws(mut body: impl FnMut(&egui::Context)) {
        let ctx = egui::Context::default();
        // Give it a real screen rect; a zero-sized viewport hides layout bugs.
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0_f32, 900.0_f32),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| body(ctx));
    }

    /// Every screen must survive being drawn with no connection and no data.
    ///
    /// That is the state a screen is in the moment it opens, and the one
    /// most likely to trip an empty-slice panic.
    #[test]
    fn every_screen_draws_from_empty_state() {
        // (name, how to open it) so a failure names the screen.
        type Opener = fn(&mut DbHiveApp);
        let screens: Vec<(&str, Opener)> = vec![
            ("connections", |_a| {}),
            ("settings", |a| a.screens.settings.open = true),
            ("plugins", |a| a.screens.plugins.open = true),
            ("procedures", |a| a.screens.procedures.open = true),
            ("table_inspector", |a| a.screens.table_inspector.open = true),
            ("table_editor", |a| {
                a.screens.table_editor.open_new("conn", "public")
            }),
            ("er_diagram", |a| a.screens.er_diagram.open = true),
            ("schema_designer", |a| a.screens.schema_designer.open = true),
            ("query_builder", |a| a.screens.query_builder.open = true),
            ("activity_monitor", |a| a.screens.activity_monitor.open = true),
            ("migrations", |a| a.screens.migrations.open = true),
            ("backup", |a| a.screens.backup.open = true),
            ("import_export", |a| a.screens.import_export.open = true),
            ("command_palette", |a| a.screens.command_palette.open()),
            ("connection_editor", |a| a.screens.connection_editor.open_new()),
            ("about", |a| a.about_open = true),
        ];

        for (name, open) in screens {
            let mut app = DbHiveApp::for_test();
            open(&mut app);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                draws(|ctx| render_window(&mut app, ctx));
            }));
            assert!(result.is_ok(), "{} panicked while drawing", name);
        }
    }

    /// The workspace and the panels that live inside it.
    #[test]
    fn the_workspace_draws_without_a_live_connection() {
        let mut app = DbHiveApp::for_test();
        app.workspace = Some(workspace::Workspace::new(
            "conn".to_string(),
            "Test".to_string(),
        ));
        app.screen = Screen::Workspace;
        app.screens.ai_assistant.open = true;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Twice: the second frame runs with state the first frame left
            // behind, which is where "works once" bugs surface.
            draws(|ctx| render_window(&mut app, ctx));
            draws(|ctx| render_window(&mut app, ctx));
        }));
        assert!(result.is_ok(), "workspace panicked while drawing");
    }

    /// Light mode must be drawable too; the palettes are separate constants.
    #[test]
    fn light_mode_draws() {
        let mut app = DbHiveApp::for_test();
        app.mode = theme::Mode::Light;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            draws(|ctx| render_window(&mut app, ctx));
        }));
        assert!(result.is_ok(), "light mode panicked while drawing");
    }
}
