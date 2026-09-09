//! Set the stored theme mode, so both palettes can be checked without
//! changing the machine's system appearance.
//!
//!     cargo run -p dbhive-app --example set_theme -- dark|light|system

fn main() {
    let want = std::env::args().nth(1).unwrap_or_else(|| "dark".into());
    let mut s = dbhive_core::settings::load().expect("load settings");
    s.theme.mode = match want.as_str() {
        "light" => dbhive_core::models::settings::ThemeMode::Light,
        "system" => dbhive_core::models::settings::ThemeMode::System,
        _ => dbhive_core::models::settings::ThemeMode::Dark,
    };
    dbhive_core::settings::save(&s).expect("save settings");
    println!("theme mode set to {}", want);
}
