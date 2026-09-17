use slint::ComponentHandle;

fn main() -> Result<(), slint::PlatformError> {
    dbhive_slint::AppWindow::new()?.run()
}
