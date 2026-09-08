//! Top-level application state and frame loop.

use std::sync::{Arc, Mutex};

use dbhive_core::state::AppState;

use crate::screens::{connections, workspace, Screen};
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

            match self.screen {
                Screen::Connections => connections::show(self, ui),
                Screen::Workspace => workspace::show(self, ui),
            }
        });
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

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
