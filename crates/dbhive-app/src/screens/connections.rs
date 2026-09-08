//! Saved connections.
//!
//! Parity target: the React `Saved Connections` screen. Lists every stored
//! profile, filters by name, and opens one.

use dbhive_core::models::{ConnectionProfile, DbDriver, DbError};

use crate::app::DbHiveApp;
use crate::task::Job;

#[derive(Default)]
pub struct ConnectionsScreen {
    pub search: String,
    /// Which profile the in-flight connect belongs to, so the row can show a
    /// spinner without blocking the rest of the list.
    pub connecting: Option<String>,
    pub connect_job: Job<Result<String, DbError>>,
    pub disconnect_job: Job<Result<(), DbError>>,
    pub last_error: Option<String>,
    pub connected: Vec<String>,
}

pub fn driver_label(driver: &DbDriver) -> &'static str {
    match driver {
        DbDriver::Postgres => "PostgreSQL",
        DbDriver::MySql => "MySQL",
        DbDriver::Sqlite => "SQLite",
        DbDriver::MongoDb => "MongoDB",
        DbDriver::SqlServer => "SQL Server",
        DbDriver::Supabase => "Supabase",
        DbDriver::Neon => "Neon",
        DbDriver::Turso => "Turso",
        DbDriver::Redis => "Redis",
    }
}

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    // Collect first: the lock must not be held while drawing, or a slow
    // frame would block every background query.
    let profiles: Vec<ConnectionProfile> = {
        let guard = app.state.lock().unwrap();
        let mut v: Vec<ConnectionProfile> = guard.list_profiles().into_iter().cloned().collect();
        v.sort_by(|a, b| {
            b.is_favorite
                .cmp(&a.is_favorite)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        v
    };

    if let Some(result) = app.connections.connect_job.take_result() {
        app.connections.connecting = None;
        match result {
            Ok(id) => {
                if !app.connections.connected.contains(&id) {
                    app.connections.connected.push(id);
                }
                app.connections.last_error = None;
            }
            Err(e) => app.connections.last_error = Some(e.to_string()),
        }
    }

    let palette = app.mode.palette();

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::heading(palette, "Saved Connections"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let _ = ui.button("New");
        });
    });
    ui.add_space(4.0);

    ui.add(
        egui::TextEdit::singleline(&mut app.connections.search)
            .hint_text("Search connections...")
            .desired_width(f32::INFINITY),
    );
    ui.add_space(8.0);

    if let Some(err) = app.connections.last_error.clone() {
        ui.horizontal(|ui| {
            ui.colored_label(palette.destructive, "Connection failed:");
            ui.label(err);
            if ui.button("Dismiss").clicked() {
                app.connections.last_error = None;
            }
        });
        ui.add_space(4.0);
    }

    let needle = app.connections.search.to_lowercase();
    let visible: Vec<&ConnectionProfile> = profiles
        .iter()
        .filter(|p| {
            needle.is_empty()
                || p.name.to_lowercase().contains(&needle)
                || p.host.to_lowercase().contains(&needle)
        })
        .collect();

    if profiles.is_empty() {
        empty_state(ui, palette, "No saved connections", "Add a connection to get started.");
        return;
    }
    if visible.is_empty() {
        empty_state(
            ui,
            palette,
            "No matches",
            "No saved connection matches that search.",
        );
        return;
    }

    // egui is immediate mode: the row cannot mutate `app` while the profile
    // list is borrowed, so the click is recorded and acted on afterwards.
    let mut to_connect: Option<String> = None;
    let mut to_disconnect: Option<String> = None;
    let row_ctx = app_row_ctx(app);

    egui::ScrollArea::vertical().show(ui, |ui| {
        for profile in visible {
            match row(&row_ctx, ui, profile) {
                RowAction::None => {}
                RowAction::Connect => to_connect = Some(profile.id.clone()),
                RowAction::Disconnect => to_disconnect = Some(profile.id.clone()),
            }
        }
    });

    if let Some(id) = to_connect {
        app.connections.last_error = None;
        app.connections.connecting = Some(id.clone());
        let state = app.state.clone();
        // The password is left empty on purpose: the service falls back to
        // the OS keyring and then to this session's cache, which is what the
        // web shell's "connect" button did too.
        app.connections.connect_job.start(app.rt.handle(), ui.ctx(), async move {
            dbhive_core::service::connect(&state, id, String::new(), None).await
        });
    }

    if let Some(id) = to_disconnect {
        let state = app.state.clone();
        app.connections.connected.retain(|c| c != &id);
        app.connections.disconnect_job.start(app.rt.handle(), ui.ctx(), async move {
            dbhive_core::service::disconnect(&state, &id).await
        });
    }
}

pub enum RowAction {
    None,
    Connect,
    Disconnect,
}

/// The bits of the app a row needs, split out so the row borrow does not
/// conflict with the profile list borrowed above.
struct RowCtx {
    connecting: Option<String>,
    connected: Vec<String>,
    dark: bool,
}

fn app_row_ctx(app: &DbHiveApp) -> RowCtx {
    RowCtx {
        connecting: app.connections.connecting.clone(),
        connected: app.connections.connected.clone(),
        dark: matches!(app.mode, crate::theme::Mode::Dark),
    }
}

fn row(ctx: &RowCtx, ui: &mut egui::Ui, profile: &ConnectionProfile) -> RowAction {
    let palette = if ctx.dark { &crate::theme::DARK } else { &crate::theme::LIGHT };
    let is_connected = ctx.connected.contains(&profile.id);
    let is_connecting = ctx.connecting.as_deref() == Some(profile.id.as_str());
    let mut action = RowAction::None;

    egui::Frame::none()
        .fill(palette.card)
        .stroke(egui::Stroke::new(1.0, palette.border))
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(crate::theme::text::strong(palette, profile.name.clone()));
                        if profile.is_favorite {
                            ui.colored_label(palette.primary, "★");
                        }
                        if is_connected {
                            ui.colored_label(palette.success, "● Connected");
                        }
                    });
                    ui.label(crate::theme::text::muted(
                        palette,
                        format!(
                            "{} · {}:{}",
                            driver_label(&profile.driver),
                            profile.host,
                            profile.port
                        ),
                    ));
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if is_connecting {
                        ui.add(egui::Spinner::new());
                        ui.label(crate::theme::text::muted(palette, "Connecting"));
                    } else if is_connected {
                        if ui.button("Disconnect").clicked() {
                            action = RowAction::Disconnect;
                        }
                    } else if ui.button("Connect").clicked() {
                        action = RowAction::Connect;
                    }
                });
            });
        });
    ui.add_space(6.0);
    action
}

fn empty_state(ui: &mut egui::Ui, palette: &crate::theme::Palette, title: &str, message: &str) {
    ui.add_space(48.0);
    ui.vertical_centered(|ui| {
        ui.label(crate::theme::text::heading(palette, title));
        ui.label(crate::theme::text::muted(palette, message));
    });
}
