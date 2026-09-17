//! Projects `dbhive_ui_model::connection_editor` state onto the generated
//! `ConnectionFormData` struct the dialog draws from.
//!
//! The dialog only ever sees this struct: it has no notion of `DbDriver` or
//! `SslMode`, so a driver added to the core model cannot silently fail to
//! compile here. Everything a picker needs (its options, and which index the
//! current value is) is computed on this side.

use dbhive_core::models::connection::Environment;
use dbhive_core::models::{DbDriver, SslMode};
use dbhive_ui_model::connection_editor::{
    database_label, driver_fields, environment_label, password_label, ssl_mode_label,
    ConnectionForm, ALL_DRIVERS,
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
