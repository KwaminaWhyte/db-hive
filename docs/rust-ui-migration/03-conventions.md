# Native shell conventions

Read this before adding a screen to `crates/dbhive-app`. These are not
style preferences; each one exists because the alternative broke something.

## Layering

```
dbhive-core        drivers, DDL, SSH, credentials, migrations, AI, AppState
dbhive-core::service   shell-agnostic operations both shells call
dbhive-app         egui UI only
```

Business logic goes in `dbhive-core`, never in a screen. If a screen needs an
operation that only exists inside a `#[tauri::command]` in `src-tauri`, lift
the body into `dbhive_core::service` and call it from there. The Tauri shell
is the reference implementation and is deleted at parity; do not add to it.

## The async rule

`eframe::App::update` runs every frame and must never block. There is no
exception to this. A single blocking call freezes the window.

```rust
// Wrong: blocks the frame, freezes the UI for the length of the query.
let rows = rt.block_on(driver.execute_query(&sql));

// Right: spawn, and poll for the result on later frames.
job.start(app.rt.handle(), ui.ctx(), async move {
    driver.execute_query(&sql).await
});
if let Some(result) = job.take_result() { /* ... */ }
```

Use `crate::task::Job<T>` for "one in-flight operation with a result".
`Job::is_running()` drives spinners and disabled buttons.

`Job::start` requests a repaint when the work lands. Without that egui, which
only redraws on activity, would leave a finished result invisible until the
user happened to move the mouse. If you spawn work by hand, call
`ctx.request_repaint()` yourself.

## Never hold the AppState lock across drawing

```rust
// Wrong: the lock is held for the whole draw. Every background query stalls
// behind a slow frame.
let guard = app.state.lock().unwrap();
for p in guard.list_profiles() { ui.label(&p.name); }

// Right: copy what you need, drop the lock, then draw.
let profiles: Vec<ConnectionProfile> = {
    let guard = app.state.lock().unwrap();
    guard.list_profiles().into_iter().cloned().collect()
};
for p in &profiles { ui.label(&p.name); }
```

The same applies to drivers: clone the `Arc<dyn DatabaseDriver>` out of state,
drop the guard, then `await`. Holding a `std::sync::MutexGuard` across an
await point will not compile, and holding it across a draw is a stall.

## Immediate mode: record the click, act after the loop

A row cannot mutate `app` while the list it came from is borrowed. Collect the
intent, act once the borrow ends.

```rust
let mut to_open: Option<String> = None;
for item in &items {
    if ui.button(&item.name).clicked() { to_open = Some(item.id.clone()); }
}
if let Some(id) = to_open { open(app, id); }
```

## Text colour

`RichText::strong()` resolves to `widgets.active.fg_stroke`, which this
palette uses for text sitting on the honey primary. Bold text styled the
default way comes out near-invisible on dark cards.

Always use the helpers:

```rust
ui.label(crate::theme::text::strong(palette, "Saved Connections"));
ui.label(crate::theme::text::muted(palette, "3 of 40 rows"));
ui.label(crate::theme::text::heading(palette, "Settings"));
```

Never hardcode a colour. Take it from `palette` (`crate::theme::Palette`), so
light and dark both work. The honey gold primary is the brand colour; do not
substitute a stock accent.

## Reuse the widgets

- Tabular data: `widgets::results_grid`. Do not write another table. It
  virtualises, sorts numerically, filters, and renders NULL distinctly.
- SQL input: `widgets::sql_editor`.
- Notifications: `app.toasts.success(...)` / `.error(...)`. Errors linger
  longer than successes on purpose.

## Errors

Surface them in the UI. `eprintln!` alone is not acceptable for anything the
user needs to know about; a failed operation that only logs to a terminal the
user never sees is a silent failure. Keep the driver's message: it is usually
the only thing that identifies the real cause.

## Tests

Pure logic (filtering, sorting, formatting, SQL generation, parsing) gets unit
tests in the same file. Drawing code does not. If a function is hard to test
because it is tangled with `ui`, split the logic out; that is the point.

Run `cargo test -p dbhive-app` and `cargo test -p dbhive-core`.

Three `credentials::tests::*` failures are known and pre-existing: they hit
the real OS keyring, which macOS denies to an unsigned test binary. They fail
identically at commit c57b24e, before this migration started. Do not "fix"
them by weakening the tests.

## Development aids

```
DBHIVE_AUTOCONNECT="Local MySQL"   open that profile at startup
DBHIVE_AUTOQUERY="SELECT 1;"       run a statement once connected
```

Both are for exercising screens without clicking through. Leave them unset in
normal use, and do not build features that depend on them.
