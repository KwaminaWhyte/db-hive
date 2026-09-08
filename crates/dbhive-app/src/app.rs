//! Top-level application state and frame loop.

use std::sync::{Arc, Mutex};

use dbhive_core::state::AppState;

use crate::screens::{
    activity_monitor, backup, command_palette, connection_editor, connections, er_diagram,
    migrations, settings, table_editor, table_inspector, workspace, Screen, Screens,
};
use crate::theme;

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
    /// Profile to open at startup, from DBHIVE_AUTOCONNECT.
    autoconnect: Option<(String, String)>,
}

impl DbHiveApp {
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
            autoconnect,
        }
    }
}

impl eframe::App for DbHiveApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        top_bar(self, ctx);

        if let Some((id, name)) = self.autoconnect.take() {
            self.connections.connecting = Some(id.clone());
            let state = self.state.clone();
            let handle = self.rt.handle().clone();
            self.connections.pending_name = Some(name);
            self.connections.connect_job.start(&handle, ctx, async move {
                dbhive_core::service::connect(&state, id, String::new(), None).await
            });
        }

        // Settings own the theme: loaded once, then applied every frame so a
        // saved mode or accent survives a restart even if Settings is never
        // opened. This must run before anything draws.
        settings::ensure_loaded(self, ctx);
        settings::poll(self);
        settings::ensure_theme(self, ctx);

        let palette = self.mode.palette();
        self.toasts.show(ctx, palette);

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(err) = self.startup_error.clone() {
                ui.horizontal(|ui| {
                    ui.colored_label(self.mode.palette().destructive, "Could not load saved data:");
                    ui.label(err);
                    if ui.button("Dismiss").clicked() {
                        self.startup_error = None;
                    }
                });
                ui.separator();
            }

            if self.screens.table_editor.open {
                table_editor::show(self, ui);
            } else if self.screens.activity_monitor.open {
                activity_monitor::show(self, ui);
            } else if self.screens.er_diagram.open {
                er_diagram::show(self, ui);
            } else if self.screens.table_inspector.open {
                table_inspector::show(self, ui);
            } else {
                match self.screen {
                    Screen::Connections => connections::show(self, ui),
                    Screen::Workspace => workspace::show(self, ui),
                }
            }
        });
        settings::show_window(self, ctx);
        migrations::show_window(self, ctx);
        backup::show_window(self, ctx);

        if let Some(action) = connection_editor::show(self, ctx) {
            match action {
                connection_editor::EditorAction::Saved(id) => {
                    self.toasts.success("Connection saved");
                    self.connections.last_saved = Some(id);
                }
                connection_editor::EditorAction::Cancelled => {}
            }
        }

        // Last, so the palette draws above every other overlay.
        if let command_palette::PaletteAction::Run(id) = command_palette::show(self, ctx) {
            dispatch(self, ctx, id);
        }
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
        C::GoBackup => app.screens.backup.open = true,
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

                // The web shell centred the command palette trigger and had
                // to fight overlap for it. Here it simply follows the menus.
                ui.add_space(4.0);

                if ui.small_button("Search or run commands…  ⌘K").clicked() {
                    app.screens.command_palette.open();
                }

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
}
