//! Saved connections.
//!
//! Parity target: the React `Saved Connections` screen. Lists every stored
//! profile, filters by name, and opens one.

use dbhive_core::models::{ConnectionProfile, DbDriver, DbError};

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::widgets::ui_kit as kit;

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
    /// Profile awaiting delete confirmation: (id, name).
    pub deleting: Option<(String, String)>,
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

    let count = profiles.len();
    let subtitle = match count {
        0 => "No saved connections yet".to_string(),
        1 => "1 connection".to_string(),
        n => format!("{} connections", n),
    };

    ui.add_space(14.0);
    let mut new_clicked = false;
    kit::screen_header(ui, palette, "Connections", Some(subtitle), |ui| {
        if kit::primary_button(ui, palette, "New connection").clicked() {
            new_clicked = true;
        }
    });
    if new_clicked {
        app.screens.connection_editor.open_new();
    }

    ui.add(
        egui::TextEdit::singleline(&mut app.connections.search)
            .hint_text("Search by name or host")
            .desired_width(f32::INFINITY)
            .margin(egui::Margin::symmetric(10.0_f32, 7.0_f32)),
    );
    ui.add_space(12.0);

    if let Some(err) = app.connections.last_error.clone() {
        if kit::banner(
            ui,
            palette,
            kit::Tone::Danger,
            format!("Connection failed: {}", err),
        ) {
            app.connections.last_error = None;
        }
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
        let mut add = false;
        kit::empty_state(
            ui,
            palette,
            "⌘",
            "No connections yet",
            "Add a database and it will appear here.",
            |ui| {
                if kit::primary_button(ui, palette, "Add your first connection").clicked() {
                    add = true;
                }
            },
        );
        if add {
            app.screens.connection_editor.open_new();
        }
        return;
    }
    if visible.is_empty() {
        let mut clear = false;
        kit::empty_state(
            ui,
            palette,
            "🔍",
            "No matches",
            "No saved connection matches that search.",
            |ui| {
                if kit::ghost_button(ui, palette, "Clear search").clicked() {
                    clear = true;
                }
            },
        );
        if clear {
            app.connections.search.clear();
        }
        return;
    }

    // egui is immediate mode: the row cannot mutate `app` while the profile
    // list is borrowed, so the click is recorded and acted on afterwards.
    let mut to_connect: Option<String> = None;
    let mut to_disconnect: Option<String> = None;
    let mut to_edit: Option<ConnectionProfile> = None;
    let mut to_duplicate: Option<ConnectionProfile> = None;
    let mut to_favorite: Option<String> = None;
    let mut to_delete: Option<(String, String)> = None;
    let row_ctx = app_row_ctx(app);

    egui::ScrollArea::vertical().show(ui, |ui| {
        for profile in visible {
            match row(&row_ctx, ui, profile) {
                RowAction::None => {}
                RowAction::Connect => to_connect = Some(profile.id.clone()),
                RowAction::Disconnect => to_disconnect = Some(profile.id.clone()),
                RowAction::Edit => to_edit = Some(profile.clone()),
                RowAction::Duplicate => to_duplicate = Some(profile.clone()),
                RowAction::ToggleFavorite => to_favorite = Some(profile.id.clone()),
                RowAction::Delete => to_delete = Some((profile.id.clone(), profile.name.clone())),
            }
        }
    });

    if let Some(id) = to_connect {
        // Ask for a password up front when nothing is stored, rather than
        // letting the driver fail with an opaque message.
        let known = dbhive_core::credentials::CredentialManager::has_password(&id).unwrap_or(false)
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

    if let Some(profile) = to_edit {
        app.screens.connection_editor.open_edit(&profile);
    }
    if let Some(profile) = to_duplicate {
        duplicate_profile(app, &profile);
    }
    if let Some(id) = to_favorite {
        toggle_favorite(app, &id);
    }
    if let Some((id, name)) = to_delete {
        app.connections.deleting = Some((id, name));
    }

    delete_confirm(app, ui.ctx());
    password_prompt(app, ui.ctx());

    if let Some(id) = to_disconnect {
        let state = app.state.clone();
        app.connections.connected.retain(|c| c != &id);
        app.connections
            .disconnect_job
            .start(app.rt.handle(), ui.ctx(), async move {
                dbhive_core::service::disconnect(&state, &id).await
            });
    }
}

/// Copy a profile under a new id and name.
///
/// The password is deliberately NOT copied: keyring entries are keyed by
/// profile id, and silently duplicating a credential into a second entry the
/// user did not ask for is not something to do quietly.
fn duplicate_profile(app: &mut DbHiveApp, profile: &ConnectionProfile) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut copy = profile.clone();
    copy.id = uuid::Uuid::new_v4().to_string();
    copy.name = format!("{} (Copy)", profile.name);
    copy.last_connected_at = None;
    copy.connection_count = 0;
    copy.created_at = now;
    copy.updated_at = now;

    let result = {
        let mut guard = app.state.lock().unwrap();
        guard.add_profile(copy);
        guard.save_profiles_to_store()
    };
    match result {
        Ok(()) => app.toasts.success("Connection duplicated"),
        Err(e) => app.toasts.error(format!("Could not duplicate: {}", e)),
    }
}

fn toggle_favorite(app: &mut DbHiveApp, id: &str) {
    let result = {
        let mut guard = app.state.lock().unwrap();
        match guard.get_profile_mut(id) {
            Some(p) => {
                p.is_favorite = !p.is_favorite;
                guard.save_profiles_to_store()
            }
            None => Ok(()),
        }
    };
    if let Err(e) = result {
        app.toasts.error(format!("Could not save: {}", e));
    }
}

/// Confirm before removing a profile, naming what is going.
fn delete_confirm(app: &mut DbHiveApp, ctx: &egui::Context) {
    let Some((id, name)) = app.connections.deleting.clone() else {
        return;
    };
    let palette = if matches!(app.mode, crate::theme::Mode::Dark) {
        &crate::theme::DARK
    } else {
        &crate::theme::LIGHT
    };

    let mut confirmed = false;
    let mut cancelled = false;
    egui::Window::new("Delete connection")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0_f32, 0.0_f32))
        .show(ctx, |ui| {
            ui.label(crate::theme::text::strong(
                palette,
                format!("Delete \"{}\"?", name),
            ));
            ui.label(crate::theme::text::muted(
                palette,
                "The saved password is removed from the system keychain too.",
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Delete").clicked() {
                    confirmed = true;
                }
                if ui.button("Cancel").clicked() {
                    cancelled = true;
                }
            });
        });

    if cancelled {
        app.connections.deleting = None;
    }
    if confirmed {
        app.connections.deleting = None;
        let result = {
            let mut guard = app.state.lock().unwrap();
            guard.remove_profile(&id);
            guard.connection_passwords.remove(&id);
            guard.save_profiles_to_store()
        };
        // Leaving the credential behind would keep a password for a profile
        // that no longer exists, which the user cannot then see or clear.
        let _ = dbhive_core::credentials::CredentialManager::delete_password(&id);
        match result {
            Ok(()) => app.toasts.success(format!("Deleted {}", name)),
            Err(e) => app.toasts.error(format!("Could not delete: {}", e)),
        }
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
            let _ =
                dbhive_core::credentials::CredentialManager::delete_password(&prompt.profile_id);
        }
        start_connect(app, ctx, prompt.profile_id, prompt.password);
    }
}

pub enum RowAction {
    None,
    Connect,
    Disconnect,
    Edit,
    Duplicate,
    ToggleFavorite,
    Delete,
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
    let palette = if ctx.dark {
        &crate::theme::DARK
    } else {
        &crate::theme::LIGHT
    };
    let is_connected = ctx.connected.contains(&profile.id);
    let is_connecting = ctx.connecting.as_deref() == Some(profile.id.as_str());
    let mut action = RowAction::None;

    let (response, _) = kit::clickable_card(ui, palette, is_connected, |ui| {
        ui.horizontal(|ui| {
            // A driver chip carries the type at a glance, which a line of
            // grey text underneath does not.
            driver_chip(ui, palette, &profile.driver);
            ui.add_space(4.0);

            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.horizontal(|ui| {
                    ui.label(crate::theme::text::strong(palette, profile.name.clone()));
                    if profile.is_favorite {
                        ui.label(
                            egui::RichText::new("★")
                                .size(crate::theme::size::SMALL)
                                .color(palette.primary),
                        );
                    }
                    if let Some(env) = environment_label(profile) {
                        kit::badge(ui, palette, env.0, env.1);
                    }
                    if is_connected {
                        kit::status_badge(ui, palette, "Connected", kit::Tone::Success);
                    }
                });
                ui.label(crate::theme::text::muted(
                    palette,
                    format!("{}:{}", profile.host, profile.port),
                ));
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("…", |ui| {
                    if ui.button("Edit…").clicked() {
                        action = RowAction::Edit;
                        ui.close_menu();
                    }
                    if ui.button("Duplicate").clicked() {
                        action = RowAction::Duplicate;
                        ui.close_menu();
                    }
                    let fav = if profile.is_favorite {
                        "Remove from favourites"
                    } else {
                        "Add to favourites"
                    };
                    if ui.button(fav).clicked() {
                        action = RowAction::ToggleFavorite;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Delete…").clicked() {
                        action = RowAction::Delete;
                        ui.close_menu();
                    }
                });

                if is_connecting {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(crate::theme::text::muted(palette, "Connecting"));
                } else if is_connected {
                    if kit::ghost_button(ui, palette, "Disconnect").clicked() {
                        action = RowAction::Disconnect;
                    }
                } else if kit::primary_button(ui, palette, "Connect").clicked() {
                    action = RowAction::Connect;
                }
            });
        });
    });

    // The whole card opens the connection, so the user does not have to aim
    // for the button. A click that landed on a control inside is consumed by
    // that control, so this cannot double-fire.
    if matches!(action, RowAction::None) && response.clicked() && !is_connected && !is_connecting {
        action = RowAction::Connect;
    }

    ui.add_space(8.0);
    action
}

/// Environment tag, if the profile carries one.
fn environment_label(profile: &ConnectionProfile) -> Option<(&'static str, kit::Tone)> {
    use dbhive_core::models::connection::Environment;
    match profile.environment.as_ref()? {
        Environment::Local => Some(("Local", kit::Tone::Neutral)),
        Environment::Staging => Some(("Staging", kit::Tone::Warning)),
        Environment::Production => Some(("Production", kit::Tone::Danger)),
    }
}

/// A square chip carrying the driver's initials.
///
/// egui ships no icon set, so the chip is drawn: a tinted square with two
/// letters reads faster in a list than a word of grey text.
fn driver_chip(ui: &mut egui::Ui, palette: &crate::theme::Palette, driver: &DbDriver) {
    let (initials, tint) = match driver {
        DbDriver::Postgres => ("PG", egui::Color32::from_rgb(56, 118, 171)),
        DbDriver::Supabase => ("SB", egui::Color32::from_rgb(62, 175, 124)),
        DbDriver::Neon => ("NE", egui::Color32::from_rgb(0, 229, 153)),
        DbDriver::MySql => ("MY", egui::Color32::from_rgb(0, 117, 143)),
        DbDriver::Sqlite => ("SL", egui::Color32::from_rgb(0, 121, 168)),
        DbDriver::MongoDb => ("MG", egui::Color32::from_rgb(77, 179, 61)),
        DbDriver::SqlServer => ("MS", egui::Color32::from_rgb(168, 61, 61)),
        DbDriver::Turso => ("TU", egui::Color32::from_rgb(78, 205, 196)),
        DbDriver::Redis => ("RD", egui::Color32::from_rgb(196, 60, 44)),
    };

    let size = egui::vec2(30.0_f32, 30.0_f32);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        crate::theme::radius::CONTROL,
        tint.gamma_multiply(0.22),
    );
    ui.painter().rect_stroke(
        rect,
        crate::theme::radius::CONTROL,
        egui::Stroke::new(1.0_f32, tint.gamma_multiply(0.55)),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initials,
        egui::FontId::proportional(11.0),
        if palette.foreground.r() > 128 {
            tint.gamma_multiply(2.0)
        } else {
            tint.gamma_multiply(0.85)
        },
    );
    let _ = driver_label(driver);
}
