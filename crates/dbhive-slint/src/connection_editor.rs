//! Projects `dbhive_ui_model::connection_editor` state onto the generated
//! `ConnectionFormData` struct the dialog draws from.
//!
//! The dialog only ever sees this struct: it has no notion of `DbDriver` or
//! `SslMode`, so a driver added to the core model cannot silently fail to
//! compile here. Everything a picker needs (its options, and which index the
//! current value is) is computed on this side.

use dbhive_core::models::connection::{Environment, SshAuthMethod};
use dbhive_core::models::{DbDriver, SslMode};
use dbhive_ui_model::connection_editor::{
    color_preset_index, connection_string_placeholder, database_label, driver_fields,
    environment_label, parse_hex_color, password_label, ssl_mode_label, ConnectionForm,
    ALL_DRIVERS, COLOR_PRESETS,
};

use crate::connection_list::driver_label;
use crate::ConnectionFormData;

/// SSL modes in picker order. Kept here, not in ui-model, because "which
/// order a dropdown lists options in" is a UI decision.
pub const SSL_MODES: [SslMode; 3] = [SslMode::Prefer, SslMode::Require, SslMode::Disable];

/// Environments in picker order, with "None" first as the common case.
pub const ENVIRONMENTS: [Option<Environment>; 4] = [
    None,
    Some(Environment::Local),
    Some(Environment::Staging),
    Some(Environment::Production),
];

/// SSH auth methods in picker order.
pub const SSH_AUTH_METHODS: [SshAuthMethod; 2] =
    [SshAuthMethod::Password, SshAuthMethod::PrivateKey];

pub fn ssh_auth_label(method: &SshAuthMethod) -> &'static str {
    match method {
        SshAuthMethod::Password => "Password",
        SshAuthMethod::PrivateKey => "Private key",
    }
}

pub fn ssh_auth_options() -> Vec<slint::SharedString> {
    SSH_AUTH_METHODS
        .iter()
        .map(|m| ssh_auth_label(m).into())
        .collect()
}

pub fn ssh_auth_at(index: i32) -> SshAuthMethod {
    SSH_AUTH_METHODS
        .get(index as usize)
        .cloned()
        .unwrap_or(SshAuthMethod::Password)
}

/// The colour-tag swatches, in `COLOR_PRESETS` order.
pub fn color_options() -> Vec<slint::Color> {
    COLOR_PRESETS
        .iter()
        .filter_map(|hex| parse_hex_color(hex))
        .map(|(r, g, b)| slint::Color::from_rgb_u8(r, g, b))
        .collect()
}

/// The hex a swatch index means; empty for "no tag" (`-1`), which is also what
/// clicking the selected swatch again sends.
pub fn color_at(index: i32) -> String {
    COLOR_PRESETS
        .get(index as usize)
        .map(|s| (*s).to_string())
        .unwrap_or_default()
}

/// Every driver's display name, in `ALL_DRIVERS` order.
pub fn driver_options() -> Vec<slint::SharedString> {
    ALL_DRIVERS.iter().map(|d| driver_label(d).into()).collect()
}

pub fn ssl_options() -> Vec<slint::SharedString> {
    SSL_MODES.iter().map(|m| ssl_mode_label(m).into()).collect()
}

pub fn environment_options() -> Vec<slint::SharedString> {
    ENVIRONMENTS
        .iter()
        .map(|e| environment_label(e.as_ref()).into())
        .collect()
}

/// Which driver a driver-picker index means. Falls back to Postgres for an
/// out-of-range index, which the picker itself cannot produce but a stale
/// event from a rebuilt list in flight could.
pub fn driver_at(index: i32) -> DbDriver {
    ALL_DRIVERS
        .get(index as usize)
        .cloned()
        .unwrap_or(DbDriver::Postgres)
}

pub fn ssl_mode_at(index: i32) -> SslMode {
    SSL_MODES.get(index as usize).cloned().unwrap_or_default()
}

pub fn environment_at(index: i32) -> Option<Environment> {
    ENVIRONMENTS.get(index as usize).cloned().flatten()
}

/// Build what the dialog draws from the form's current state.
pub fn form_data(form: &ConnectionForm) -> ConnectionFormData {
    let fields = driver_fields(&form.driver);
    ConnectionFormData {
        name: form.name.as_str().into(),
        driver_label: driver_label(&form.driver).into(),
        driver_options: slint::ModelRc::new(slint::VecModel::from(driver_options())),
        show_file_path: fields.file_path,
        show_url: fields.url,
        show_host_port: fields.host_port,
        show_username: fields.username,
        show_password: fields.password,
        show_database: fields.database,
        show_ssl: fields.ssl,
        host: form.host.as_str().into(),
        port: form.port.as_str().into(),
        username: form.username.as_str().into(),
        // The password box is a separate field on the controller, not part
        // of `ConnectionForm`: the secret never round-trips through a
        // profile struct that gets persisted as JSON. See `Editor::password`.
        password: "".into(),
        show_password_text: false,
        database: form.database.as_str().into(),
        database_label: database_label(&form.driver).into(),
        password_label: password_label(&form.driver).into(),
        ssl_label: ssl_mode_label(&form.ssl_mode).into(),
        ssl_options: slint::ModelRc::new(slint::VecModel::from(ssl_options())),
        environment_label: environment_label(form.environment.as_ref()).into(),
        environment_options: slint::ModelRc::new(slint::VecModel::from(environment_options())),
        description: form.description.as_str().into(),
        favorite: form.is_favorite,
        show_connection_string: fields.host_port,
        // The pasted text and its error belong to the controller, like the
        // password; `Editor` fills them in after this call.
        connection_string: "".into(),
        connection_string_error: "".into(),
        show_ssh_password_text: false,
        connection_string_placeholder: connection_string_placeholder(&form.driver).into(),
        show_ssh: fields.ssh,
        ssh_enabled: form.ssh_enabled,
        ssh_host: form.ssh_host.as_str().into(),
        ssh_port: form.ssh_port.as_str().into(),
        ssh_username: form.ssh_username.as_str().into(),
        ssh_auth_label: ssh_auth_label(&form.ssh_auth_method).into(),
        ssh_auth_options: slint::ModelRc::new(slint::VecModel::from(ssh_auth_options())),
        ssh_show_key: form.ssh_auth_method == SshAuthMethod::PrivateKey,
        ssh_key_path: form.ssh_private_key_path.as_str().into(),
        // Like `password`, held by the controller, not the profile.
        ssh_password: "".into(),
        color_options: slint::ModelRc::new(slint::VecModel::from(color_options())),
        color_index: color_preset_index(&form.color).map_or(-1, |i| i as i32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_options_line_up_with_all_drivers() {
        assert_eq!(driver_options().len(), ALL_DRIVERS.len());
    }

    #[test]
    fn driver_at_round_trips_through_the_option_list() {
        for (i, d) in ALL_DRIVERS.iter().enumerate() {
            assert_eq!(&driver_at(i as i32), d);
        }
    }

    #[test]
    fn ssl_mode_at_round_trips() {
        for (i, m) in SSL_MODES.iter().enumerate() {
            assert_eq!(&ssl_mode_at(i as i32), m);
        }
    }

    #[test]
    fn environment_at_round_trips_including_none() {
        for (i, e) in ENVIRONMENTS.iter().enumerate() {
            assert_eq!(&environment_at(i as i32), e);
        }
    }

    #[test]
    fn ssh_auth_at_round_trips() {
        for (i, m) in SSH_AUTH_METHODS.iter().enumerate() {
            assert_eq!(&ssh_auth_at(i as i32), m);
        }
    }

    #[test]
    fn color_swatches_line_up_with_presets_and_minus_one_means_none() {
        assert_eq!(color_options().len(), COLOR_PRESETS.len());
        for (i, hex) in COLOR_PRESETS.iter().enumerate() {
            assert_eq!(color_at(i as i32), *hex);
        }
        assert_eq!(color_at(-1), "");
    }

    #[test]
    fn form_data_reports_the_selected_swatch_and_ssh_state() {
        let mut form = ConnectionForm::new_for(DbDriver::Postgres);
        assert_eq!(form_data(&form).color_index, -1);
        form.color = COLOR_PRESETS[3].to_string();
        form.ssh_enabled = true;
        form.ssh_auth_method = SshAuthMethod::PrivateKey;
        let data = form_data(&form);
        assert_eq!(data.color_index, 3);
        assert!(data.show_ssh && data.ssh_enabled && data.ssh_show_key);
        assert!(data.show_connection_string);
    }

    #[test]
    fn sqlite_and_turso_offer_neither_ssh_nor_a_connection_string() {
        for driver in [DbDriver::Sqlite, DbDriver::Turso] {
            let data = form_data(&ConnectionForm::new_for(driver));
            assert!(!data.show_ssh);
            assert!(!data.show_connection_string);
        }
    }

    #[test]
    fn sqlite_hides_network_fields() {
        let form = ConnectionForm::new_for(DbDriver::Sqlite);
        let data = form_data(&form);
        assert!(data.show_file_path);
        assert!(!data.show_host_port);
        assert!(!data.show_username);
        assert!(!data.show_password);
    }

    #[test]
    fn postgres_shows_host_port_and_ssl() {
        let form = ConnectionForm::new_for(DbDriver::Postgres);
        let data = form_data(&form);
        assert!(data.show_host_port);
        assert!(data.show_ssl);
        assert!(data.show_username);
        assert!(data.show_password);
        assert!(!data.show_file_path);
        assert!(!data.show_url);
    }

    #[test]
    fn turso_shows_a_url_field_not_host_port() {
        let form = ConnectionForm::new_for(DbDriver::Turso);
        let data = form_data(&form);
        assert!(data.show_url);
        assert!(!data.show_host_port);
    }
}
