//! Create / edit a connection profile.
//!
//! Parity target: the React `ConnectionForm.tsx` dialog (see section 2.1 of
//! docs/rust-ui-migration/01-frontend-inventory.md). Every field the web form
//! exposed lives here: name, driver, host/port (or file path, or libSQL URL),
//! username, password, database, SSL mode, folder, environment, colour tag,
//! description, favourite flag and the SSH tunnel sub-form.
//!
//! The dialog is drawn as a centred `egui::Window` so the caller can host it
//! from anywhere without owning a layout slot for it.
//!
//! Everything that decides *what* the form means lives in
//! `dbhive_ui_model::connection_editor`, with its unit tests. Only the drawing
//! and the job wiring are here.

use dbhive_core::models::connection::{Environment, SshAuthMethod};
use dbhive_core::models::{ConnectionProfile, DbDriver, DbError, SslMode};
use dbhive_ui_model::connection_editor::{
    apply_connection_string, build_profile, connection_string_placeholder, database_label,
    default_port, driver_fields, environment_label, form_from_profile, parse_connection_string,
    parse_hex_color, password_label, save_profile, ssl_mode_label, test_profile, validate,
    ConnectionForm, SaveOutcome, Validation, ALL_DRIVERS,
};

use crate::app::DbHiveApp;
use crate::screens::connections::driver_label;
use crate::task::Job;

// ===========================================================================
// Screen state
// ===========================================================================


#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorAction {
    /// The profile with this id was created or updated.
    Saved(String),
    Cancelled,
}

pub struct ConnectionEditor {
    pub open: bool,
    pub form: ConnectionForm,
    /// Password / auth token. Never persisted to the profile; goes to the
    /// OS keyring on save.
    pub password: String,
    pub show_password: bool,
    pub ssh_password: String,
    pub show_ssh_password: bool,
    /// Scratch field for the "paste a connection string" shortcut.
    pub connection_string: String,
    /// True once the user has pressed Save or Test, so a pristine form does
    /// not open covered in red.
    pub submitted: bool,
    pub last_error: Option<String>,
    pub test_result: Option<Result<String, String>>,
    pub test_job: Job<Result<(), DbError>>,
    pub save_job: Job<SaveOutcome>,
}

impl Default for ConnectionEditor {
    fn default() -> Self {
        Self {
            open: false,
            form: ConnectionForm::default(),
            password: String::new(),
            show_password: false,
            ssh_password: String::new(),
            show_ssh_password: false,
            connection_string: String::new(),
            submitted: false,
            last_error: None,
            test_result: None,
            test_job: Job::default(),
            save_job: Job::default(),
        }
    }
}

impl ConnectionEditor {
    /// Open the dialog on a blank profile.
    pub fn open_new(&mut self) {
        self.reset();
        self.form = ConnectionForm::new_for(DbDriver::Postgres);
        self.open = true;
    }

    /// Open the dialog on a blank profile for a chosen driver, matching the
    /// web shell's "pick a database type, then fill the form" flow.
    pub fn open_new_with_driver(&mut self, driver: DbDriver) {
        self.reset();
        self.form = ConnectionForm::new_for(driver);
        self.open = true;
    }

    /// Open the dialog on an existing profile.
    ///
    /// The password box starts empty on purpose: the stored secret is in the
    /// OS keyring and is not read back into the UI. Leaving it empty on save
    /// keeps whatever is already in the keyring.
    pub fn open_edit(&mut self, profile: &ConnectionProfile) {
        self.reset();
        self.form = form_from_profile(profile);
        self.open = true;
    }

    fn reset(&mut self) {
        self.password.clear();
        self.ssh_password.clear();
        self.show_password = false;
        self.show_ssh_password = false;
        self.connection_string.clear();
        self.submitted = false;
        self.last_error = None;
        self.test_result = None;
    }

    pub fn is_editing(&self) -> bool {
        self.form.id.is_some()
    }

    pub fn busy(&self) -> bool {
        self.test_job.is_running() || self.save_job.is_running()
    }
}

// ===========================================================================
// Drawing
// ===========================================================================

/// Draw the dialog. Returns an action on the frame the user saves or cancels.
///
/// Takes a `Context` rather than a `Ui` because it is a floating window: it
/// does not belong to whatever layout happens to be current.
pub fn show(app: &mut DbHiveApp, ctx: &egui::Context) -> Option<EditorAction> {
    if !app.screens.connection_editor.open {
        return None;
    }

    // Results first, so a job that landed since the last frame is reflected in
    // the widgets drawn below rather than one frame later.
    let mut action = poll_jobs(app);

    let palette = app.mode.palette();
    let title = if app.screens.connection_editor.is_editing() {
        "Edit Connection"
    } else {
        "New Connection"
    };

    let mut window_open = true;
    let mut do_save = false;
    let mut do_test = false;
    let mut do_cancel = false;

    egui::Window::new(title)
        .open(&mut window_open)
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            let validation = validate(&app.screens.connection_editor.form);
            let busy = app.screens.connection_editor.busy();

            egui::ScrollArea::vertical()
                .max_height(520.0)
                .show(ui, |ui| {
                    general_section(app, ui, palette);
                    ui.add_space(10.0);
                    connection_section(app, ui, palette, &validation);
                    ui.add_space(10.0);
                    ssh_section(app, ui, palette, &validation);
                    ui.add_space(10.0);
                    feedback_section(app, ui, palette, &validation);
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                let can_test = validation.is_valid() && !busy;
                if ui
                    .add_enabled(can_test, egui::Button::new("Test connection"))
                    .on_disabled_hover_text("Fix the errors above first")
                    .clicked()
                {
                    do_test = true;
                }
                if app.screens.connection_editor.test_job.is_running() {
                    ui.add(egui::Spinner::new());
                    ui.label(crate::theme::text::muted(palette, "Testing"));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let can_save = validation.is_valid() && !busy;
                    let save_label = if app.screens.connection_editor.is_editing() {
                        "Save changes"
                    } else {
                        "Create connection"
                    };
                    if ui
                        .add_enabled(can_save, egui::Button::new(save_label))
                        .on_disabled_hover_text("Fix the errors above first")
                        .clicked()
                    {
                        do_save = true;
                    }
                    if ui.add_enabled(!busy, egui::Button::new("Cancel")).clicked() {
                        do_cancel = true;
                    }
                    if app.screens.connection_editor.save_job.is_running() {
                        ui.add(egui::Spinner::new());
                    }
                });
            });
        });

    // Act after the closure: the borrow of `app` inside it is finished, and
    // egui is immediate mode so intent has to be replayed rather than acted on
    // where it was noticed.
    if do_test {
        start_test(app, ctx);
    }
    if do_save {
        start_save(app, ctx);
    }
    if do_cancel || !window_open {
        app.screens.connection_editor.open = false;
        if action.is_none() {
            action = Some(EditorAction::Cancelled);
        }
    }

    action
}

fn poll_jobs(app: &mut DbHiveApp) -> Option<EditorAction> {
    let mut action = None;

    if let Some(result) = app.screens.connection_editor.test_job.take_result() {
        match result {
            Ok(()) => {
                app.screens.connection_editor.test_result =
                    Some(Ok("Connection successful".to_string()));
            }
            Err(e) => {
                // Keep the driver's own message: it is usually the only thing
                // that says what actually went wrong.
                app.screens.connection_editor.test_result = Some(Err(e.to_string()));
            }
        }
    }

    if let Some(result) = app.screens.connection_editor.save_job.take_result() {
        match result {
            Ok((id, warning)) => {
                let name = app.screens.connection_editor.form.name.trim().to_string();
                if let Some(w) = warning {
                    app.toasts.error(w);
                }
                app.toasts.success(format!("Saved {}", name));
                app.screens.connection_editor.open = false;
                app.screens.connection_editor.last_error = None;
                action = Some(EditorAction::Saved(id));
            }
            Err(e) => {
                let msg = e.to_string();
                app.screens.connection_editor.last_error = Some(msg.clone());
                app.toasts.error(format!("Could not save connection: {}", msg));
            }
        }
    }

    action
}

fn section_frame<R>(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    title: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) {
    ui.label(crate::theme::text::strong(palette, title));
    ui.add_space(4.0);
    egui::Frame::none()
        .fill(palette.card)
        .stroke(egui::Stroke::new(1.0_f32, palette.border))
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// A labelled input with an optional error message underneath.
fn field<R>(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    label: &str,
    error: Option<&String>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) {
    ui.label(crate::theme::text::muted(palette, label));
    add(ui);
    if let Some(err) = error {
        ui.colored_label(palette.destructive, egui::RichText::new(err).small());
    }
    ui.add_space(6.0);
}

fn text_field(ui: &mut egui::Ui, value: &mut String, hint: &str) -> egui::Response {
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    )
}

fn general_section(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &crate::theme::Palette) {
    let validation = validate(&app.screens.connection_editor.form);
    let show_errors = app.screens.connection_editor.submitted;
    let ed = &mut app.screens.connection_editor;

    section_frame(ui, palette, "General", |ui| {
        field(
            ui,
            palette,
            "Connection name",
            show_errors.then_some(validation.name.as_ref()).flatten(),
            |ui| {
                text_field(ui, &mut ed.form.name, "My database");
            },
        );

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(200.0);
                ui.label(crate::theme::text::muted(palette, "Environment"));
                egui::ComboBox::from_id_salt("conn-editor-env")
                    .width(190.0)
                    .selected_text(environment_label(ed.form.environment.as_ref()))
                    .show_ui(ui, |ui| {
                        for env in [
                            None,
                            Some(Environment::Local),
                            Some(Environment::Staging),
                            Some(Environment::Production),
                        ] {
                            let label = environment_label(env.as_ref());
                            let selected = ed.form.environment == env;
                            ui.horizontal(|ui| {
                                if let Some(dot) = environment_color(palette, env.as_ref()) {
                                    ui.colored_label(dot, "•");
                                }
                                if ui.selectable_label(selected, label).clicked() {
                                    ed.form.environment = env.clone();
                                    ui.close_menu();
                                }
                            });
                        }
                    });
            });
            ui.vertical(|ui| {
                ui.label(crate::theme::text::muted(palette, "Folder"));
                text_field(ui, &mut ed.form.folder, "Unfiled");
            });
        });
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(200.0);
                ui.label(crate::theme::text::muted(palette, "Colour tag"));
                ui.horizontal(|ui| {
                    if let Some((r, g, b)) = parse_hex_color(&ed.form.color) {
                        let c = egui::Color32::from_rgb(r, g, b);
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, egui::Rounding::same(4.0), c);
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut ed.form.color)
                            .hint_text("#3b82f6")
                            .desired_width(150.0),
                    );
                });
            });
            ui.vertical(|ui| {
                ui.label(crate::theme::text::muted(palette, " "));
                ui.checkbox(&mut ed.form.is_favorite, "Favourite");
            });
        });
        ui.add_space(6.0);

        ui.label(crate::theme::text::muted(palette, "Description"));
        ui.add(
            egui::TextEdit::multiline(&mut ed.form.description)
                .hint_text("Notes about this connection")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
    });
}

fn environment_color(
    palette: &crate::theme::Palette,
    env: Option<&Environment>,
) -> Option<egui::Color32> {
    match env {
        None => None,
        Some(Environment::Local) => Some(palette.success),
        Some(Environment::Staging) => Some(palette.warning),
        Some(Environment::Production) => Some(palette.destructive),
    }
}

fn connection_section(
    app: &mut DbHiveApp,
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    validation: &Validation,
) {
    let show_errors = app.screens.connection_editor.submitted;
    let ed = &mut app.screens.connection_editor;
    let fields = driver_fields(&ed.form.driver);

    section_frame(ui, palette, "Connection", |ui| {
        ui.label(crate::theme::text::muted(palette, "Database type"));
        let mut next_driver: Option<DbDriver> = None;
        egui::ComboBox::from_id_salt("conn-editor-driver")
            .width(240.0)
            .selected_text(driver_label(&ed.form.driver))
            .show_ui(ui, |ui| {
                for driver in ALL_DRIVERS {
                    let selected = ed.form.driver == driver;
                    if ui.selectable_label(selected, driver_label(&driver)).clicked() && !selected {
                        next_driver = Some(driver.clone());
                    }
                }
            });
        if let Some(driver) = next_driver {
            ed.form.set_driver(driver);
            ed.connection_string.clear();
        }
        ui.add_space(8.0);

        // Paste-a-URI shortcut. Parsed on every edit like the web form did, so
        // a paste fills the fields without a second click.
        if !fields.file_path {
            ui.label(crate::theme::text::muted(palette, "Connection string (optional)"));
            let resp = ui.add(
                egui::TextEdit::singleline(&mut ed.connection_string)
                    .hint_text(connection_string_placeholder(&ed.form.driver))
                    .desired_width(f32::INFINITY),
            );
            if resp.changed() {
                if let Some(parsed) =
                    parse_connection_string(&ed.form.driver, &ed.connection_string)
                {
                    if let Some(pw) = apply_connection_string(&mut ed.form, &parsed) {
                        ed.password = pw;
                    }
                }
            }
            ui.add_space(8.0);
        }

        if fields.file_path {
            // No native file picker: the shell has no dialog dependency, so
            // the path is typed. See the migration notes.
            field(
                ui,
                palette,
                database_label(&ed.form.driver),
                show_errors.then_some(validation.database.as_ref()).flatten(),
                |ui| {
                    text_field(ui, &mut ed.form.database, "/path/to/database.sqlite");
                },
            );
        }

        if fields.url {
            field(
                ui,
                palette,
                "libSQL URL",
                show_errors.then_some(validation.host.as_ref()).flatten(),
                |ui| {
                    text_field(ui, &mut ed.form.host, "libsql://my-db-org.turso.io");
                },
            );
        }

        if fields.host_port {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - 120.0).max(160.0));
                    field(
                        ui,
                        palette,
                        "Host",
                        show_errors.then_some(validation.host.as_ref()).flatten(),
                        |ui| {
                            text_field(ui, &mut ed.form.host, "localhost");
                        },
                    );
                });
                ui.vertical(|ui| {
                    field(
                        ui,
                        palette,
                        "Port",
                        show_errors.then_some(validation.port.as_ref()).flatten(),
                        |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut ed.form.port)
                                    .hint_text(default_port(&ed.form.driver).to_string())
                                    .desired_width(100.0),
                            );
                        },
                    );
                });
            });
        }

        if fields.database && !fields.file_path {
            field(ui, palette, database_label(&ed.form.driver), None, |ui| {
                let hint = if ed.form.driver == DbDriver::Redis {
                    "0"
                } else {
                    "Optional"
                };
                text_field(ui, &mut ed.form.database, hint);
            });
        }

        if fields.username {
            let label = if fields.username_required {
                "Username"
            } else {
                "Username (optional)"
            };
            field(
                ui,
                palette,
                label,
                show_errors.then_some(validation.username.as_ref()).flatten(),
                |ui| {
                    text_field(ui, &mut ed.form.username, "");
                },
            );
        }

        if fields.password {
            ui.label(crate::theme::text::muted(
                palette,
                password_label(&ed.form.driver),
            ));
            ui.horizontal(|ui| {
                let show = ed.show_password;
                ui.add(
                    egui::TextEdit::singleline(&mut ed.password)
                        .password(!show)
                        .hint_text(if ed.form.id.is_some() {
                            "Leave blank to keep the saved secret"
                        } else {
                            ""
                        })
                        .desired_width(ui.available_width() - 70.0),
                );
                if ui
                    .button(if show { "Hide" } else { "Show" })
                    .on_hover_text("Toggle visibility")
                    .clicked()
                {
                    ed.show_password = !show;
                }
            });
            ui.label(crate::theme::text::muted(
                palette,
                "Stored in the OS keychain, never in the profile file.",
            ));
            ui.add_space(6.0);
        }

        if fields.ssl {
            ui.label(crate::theme::text::muted(palette, "SSL mode"));
            egui::ComboBox::from_id_salt("conn-editor-ssl")
                .width(200.0)
                .selected_text(ssl_mode_label(&ed.form.ssl_mode))
                .show_ui(ui, |ui| {
                    for mode in [SslMode::Disable, SslMode::Prefer, SslMode::Require] {
                        let selected = ed.form.ssl_mode == mode;
                        if ui.selectable_label(selected, ssl_mode_label(&mode)).clicked() {
                            ed.form.ssl_mode = mode.clone();
                        }
                    }
                });
        }
    });
}

fn ssh_section(
    app: &mut DbHiveApp,
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    validation: &Validation,
) {
    let show_errors = app.screens.connection_editor.submitted;
    let ed = &mut app.screens.connection_editor;
    if !driver_fields(&ed.form.driver).ssh {
        return;
    }

    section_frame(ui, palette, "SSH tunnel", |ui| {
        ui.checkbox(&mut ed.form.ssh_enabled, "Connect over an SSH tunnel");
        if !ed.form.ssh_enabled {
            ui.label(crate::theme::text::muted(
                palette,
                "The database is reached directly.",
            ));
            return;
        }
        ui.add_space(6.0);

        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - 120.0).max(160.0));
                field(
                    ui,
                    palette,
                    "SSH host",
                    show_errors.then_some(validation.ssh_host.as_ref()).flatten(),
                    |ui| {
                        text_field(ui, &mut ed.form.ssh_host, "bastion.example.com");
                    },
                );
            });
            ui.vertical(|ui| {
                field(
                    ui,
                    palette,
                    "SSH port",
                    show_errors.then_some(validation.ssh_port.as_ref()).flatten(),
                    |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut ed.form.ssh_port)
                                .hint_text("22")
                                .desired_width(100.0),
                        );
                    },
                );
            });
        });

        field(
            ui,
            palette,
            "SSH username",
            show_errors
                .then_some(validation.ssh_username.as_ref())
                .flatten(),
            |ui| {
                text_field(ui, &mut ed.form.ssh_username, "");
            },
        );

        ui.label(crate::theme::text::muted(palette, "Authentication"));
        ui.horizontal(|ui| {
            for method in [SshAuthMethod::Password, SshAuthMethod::PrivateKey] {
                let label = match method {
                    SshAuthMethod::Password => "Password",
                    SshAuthMethod::PrivateKey => "Private key",
                };
                let selected = ed.form.ssh_auth_method == method;
                if ui.selectable_label(selected, label).clicked() {
                    ed.form.ssh_auth_method = method.clone();
                }
            }
        });
        ui.add_space(6.0);

        match ed.form.ssh_auth_method {
            SshAuthMethod::Password => {
                ui.label(crate::theme::text::muted(palette, "SSH password"));
                ui.horizontal(|ui| {
                    let show = ed.show_ssh_password;
                    ui.add(
                        egui::TextEdit::singleline(&mut ed.ssh_password)
                            .password(!show)
                            .desired_width(ui.available_width() - 70.0),
                    );
                    if ui.button(if show { "Hide" } else { "Show" }).clicked() {
                        ed.show_ssh_password = !show;
                    }
                });
                ui.label(crate::theme::text::muted(
                    palette,
                    "Stored in the OS keychain.",
                ));
                ui.add_space(6.0);
            }
            SshAuthMethod::PrivateKey => {
                field(
                    ui,
                    palette,
                    "Private key path",
                    show_errors
                        .then_some(validation.ssh_private_key_path.as_ref())
                        .flatten(),
                    |ui| {
                        text_field(ui, &mut ed.form.ssh_private_key_path, "~/.ssh/id_ed25519");
                    },
                );
            }
        }

        field(ui, palette, "Local port (0 = automatic)", None, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut ed.form.ssh_local_port)
                    .hint_text("0")
                    .desired_width(100.0),
            );
        });
    });
}

fn feedback_section(
    app: &mut DbHiveApp,
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    validation: &Validation,
) {
    let ed = &mut app.screens.connection_editor;

    if ed.submitted && !validation.is_valid() {
        egui::Frame::none()
            .fill(palette.card)
            .stroke(egui::Stroke::new(1.0_f32, palette.destructive))
            .rounding(egui::Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.colored_label(palette.destructive, "Fix these before saving:");
                for msg in validation.messages() {
                    ui.colored_label(palette.destructive, egui::RichText::new(msg).small());
                }
            });
        ui.add_space(6.0);
    }

    if let Some(result) = ed.test_result.clone() {
        let (colour, heading, detail) = match result {
            Ok(msg) => (palette.success, msg, None),
            Err(detail) => (palette.destructive, "Connection failed".to_string(), Some(detail)),
        };
        egui::Frame::none()
            .fill(palette.card)
            .stroke(egui::Stroke::new(1.0_f32, colour))
            .rounding(egui::Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.colored_label(colour, heading);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Dismiss").clicked() {
                            ed.test_result = None;
                        }
                    });
                });
                if let Some(detail) = detail {
                    ui.label(crate::theme::text::muted(palette, detail));
                }
            });
        ui.add_space(6.0);
    }

    if let Some(err) = ed.last_error.clone() {
        ui.colored_label(palette.destructive, format!("Could not save: {}", err));
        ui.add_space(6.0);
    }
}

// ===========================================================================
// Async work
// ===========================================================================

fn start_test(app: &mut DbHiveApp, ctx: &egui::Context) {
    app.screens.connection_editor.submitted = true;
    app.screens.connection_editor.test_result = None;
    if !validate(&app.screens.connection_editor.form).is_valid() {
        return;
    }

    let profile = build_profile(&app.screens.connection_editor.form);
    let password = app.screens.connection_editor.password.clone();
    let ssh_password = match (
        profile.ssh_tunnel.as_ref().map(|s| s.auth_method.clone()),
        app.screens.connection_editor.ssh_password.clone(),
    ) {
        (Some(SshAuthMethod::Password), p) if !p.is_empty() => Some(p),
        _ => None,
    };
    let state = app.state.clone();
    let handle = app.rt.handle().clone();

    app.screens.connection_editor.test_job.start(&handle, ctx, async move {
        test_profile(state, profile, password, ssh_password).await
    });
}

fn start_save(app: &mut DbHiveApp, ctx: &egui::Context) {
    app.screens.connection_editor.submitted = true;
    app.screens.connection_editor.last_error = None;
    if !validate(&app.screens.connection_editor.form).is_valid() {
        return;
    }

    let profile = build_profile(&app.screens.connection_editor.form);
    let id = profile.id.clone();
    // Remember the id so a subsequent save in the same dialog updates rather
    // than creating a second profile.
    app.screens.connection_editor.form.id = Some(id.clone());
    app.screens.connection_editor.form.created_at = Some(profile.created_at);

    let password = app.screens.connection_editor.password.clone();
    let ssh_password = match (
        profile.ssh_tunnel.as_ref().map(|s| s.auth_method.clone()),
        app.screens.connection_editor.ssh_password.clone(),
    ) {
        (Some(SshAuthMethod::Password), p) if !p.is_empty() => Some(p),
        _ => None,
    };
    let state = app.state.clone();
    let handle = app.rt.handle().clone();

    app.screens.connection_editor.save_job.start(&handle, ctx, async move {
        save_profile(state, profile, id, password, ssh_password).await
    });
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- dialog state ---

    #[test]
    fn open_edit_loads_the_profile_and_clears_secrets() {
        let mut editor = ConnectionEditor::default();
        editor.password = "stale".to_string();
        editor.test_result = Some(Ok("old".to_string()));

        let profile = ConnectionProfile::new(
            "abc".to_string(),
            "Staging".to_string(),
            DbDriver::MySql,
            "mysql.host".to_string(),
            3306,
            "root".to_string(),
        );
        editor.open_edit(&profile);

        assert!(editor.open);
        assert!(editor.is_editing());
        assert_eq!(editor.form.id.as_deref(), Some("abc"));
        assert_eq!(editor.form.name, "Staging");
        assert_eq!(editor.form.driver, DbDriver::MySql);
        assert!(editor.password.is_empty(), "the keyring secret is not shown");
        assert!(editor.test_result.is_none());
        assert!(!editor.submitted);
    }

    #[test]
    fn open_new_starts_from_a_blank_profile() {
        let mut editor = ConnectionEditor::default();
        editor.open_new_with_driver(DbDriver::Redis);
        assert!(editor.open);
        assert!(!editor.is_editing());
        assert_eq!(editor.form.driver, DbDriver::Redis);
        assert_eq!(editor.form.port, "6379");
        assert!(editor.form.name.is_empty());
    }
}
