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
    /// Name of a connection started outside the list (see DBHIVE_AUTOCONNECT).
    pub pending_name: Option<String>,
    /// Profile awaiting a typed password, and the text entered so far.
    pub password_prompt: Option<PasswordPrompt>,
    /// Profile id most recently saved by the editor, for highlighting.
    pub last_saved: Option<String>,
    pub connected: Vec<String>,
}

/// Prompt shown when no stored password is available for a profile.
///
/// The keyring is the normal source, but it can legitimately have nothing:
/// a profile added on another machine, an entry the user cleared, or a build
/// that the OS will not hand credentials to. Without this the connection
/// just failed with whatever the driver said, and there was no way to
/// supply the password at all.
pub struct PasswordPrompt {
    pub profile_id: String,
    pub profile_name: String,
    pub password: String,
    pub save: bool,
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
                    app.connections.connected.push(id.clone());
                }
                app.connections.last_error = None;
                let name = profiles
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.name.clone())
                    .or_else(|| app.connections.pending_name.take())
                    .unwrap_or_else(|| id.clone());
                app.toasts.success(format!("Connected to {}", name));
                app.workspace = Some(crate::screens::workspace::Workspace::new(id, name));
                app.screen = crate::screens::Screen::Workspace;
            }
            Err(e) => {
                app.connections.last_error = Some(e.to_string());
                app.toasts.error(format!("Connection failed: {}", e));
            }
        }
    }

    let palette = app.mode.palette();

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::heading(palette, "Saved Connections"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("New").clicked() {
                app.screens.connection_editor.open_new();
            }
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
        // Ask for a password up front when nothing is stored, rather than
        // letting the driver fail with an opaque message.
        let known = dbhive_core::credentials::CredentialManager::has_password(&id)
            .unwrap_or(false)
            || app
                .state
                .lock()
                .unwrap()
                .connection_passwords
                .contains_key(&id);

        if known {
            start_connect(app, ui.ctx(), id, String::new());
        } else {
            let name = profiles
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| id.clone());
            app.connections.password_prompt = Some(PasswordPrompt {
                profile_id: id,
                profile_name: name,
                password: String::new(),
                save: true,
            });
        }
    }

    password_prompt(app, ui.ctx());

    if let Some(id) = to_disconnect {
        let state = app.state.clone();
        app.connections.connected.retain(|c| c != &id);
        app.connections.disconnect_job.start(app.rt.handle(), ui.ctx(), async move {
            dbhive_core::service::disconnect(&state, &id).await
        });
    }
}

/// Kick off a connection with an explicit password ("" defers to the
/// keyring and this session's cache).
pub fn start_connect(app: &mut DbHiveApp, ctx: &egui::Context, id: String, password: String) {
    app.connections.last_error = None;
    app.connections.connecting = Some(id.clone());
    let state = app.state.clone();
    let handle = app.rt.handle().clone();
    app.connections.connect_job.start(&handle, ctx, async move {
        dbhive_core::service::connect(&state, id, password, None).await
    });
}

/// Modal asking for a password when none is stored.
fn password_prompt(app: &mut DbHiveApp, ctx: &egui::Context) {
    let Some(prompt) = app.connections.password_prompt.as_mut() else {
        return;
    };
    let palette = if matches!(app.mode, crate::theme::Mode::Dark) {
        &crate::theme::DARK
    } else {
        &crate::theme::LIGHT
    };

    let mut submit = false;
    let mut cancel = false;
    let title = format!("Password for {}", prompt.profile_name);

    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label(crate::theme::text::muted(
                palette,
                "No saved password was found for this connection.",
            ));
            ui.add_space(6.0);
            let response = ui.add(
                egui::TextEdit::singleline(&mut prompt.password)
                    .password(true)
                    .hint_text("Password")
                    .desired_width(260.0),
            );
            response.request_focus();
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                submit = true;
            }
            ui.checkbox(&mut prompt.save, "Remember in the system keychain");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Connect").clicked() {
                    submit = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });

    if cancel {
        app.connections.password_prompt = None;
        app.connections.connecting = None;
        return;
    }
    if submit {
        let prompt = app.connections.password_prompt.take().unwrap();
        // service::connect saves a non-empty password to the keyring itself,
        // so "don't remember" means clearing it again after connecting.
        if !prompt.save {
            let _ = dbhive_core::credentials::CredentialManager::delete_password(
                &prompt.profile_id,
            );
        }
        start_connect(app, ctx, prompt.profile_id, prompt.password);
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
