# AGENTS.md: rules that override everything else

Applies to every agent and contributor. `CLAUDE.md` has the working instructions; when they
disagree, this file wins.

## UI rules

1. **Never switch UI toolkit.** The stack is Slint (egui shell stays until cutover). If you think a
   change is needed, stop and ask the owner. See `docs/ui-guidelines.md` section 1.
2. **Read `docs/ui-guidelines.md` before touching any `.slint` or UI Rust code.**
3. **Build screens only from `ui/kit/kit.slint`.** Add missing components to the kit first.
4. **No literal colours or px font sizes** outside the generated theme. `scripts/check-ui-tokens.sh`
   must pass.
5. **Render and look.** After every UI change, render snapshots (command in the guidelines), open the
   PNG, and check alignment, contrast and density at two sizes, light and dark. Do not report a UI
   task finished on compile success.
6. **Four states per view:** populated, empty, loading, error.
7. **Logic out of the UI.** New logic goes in `dbhive-ui-model` or `dbhive-core` with unit tests.

## Engineering rules

- No Node, npm or bundler. Native Rust only.
- Every SQL identifier through `quote_identifier`, every literal through `escape_string_literal`.
  Never concatenate user text into SQL.
- Passwords only in the OS keyring. Never in JSON stores or logs.
- Do not change `APP_IDENTIFIER`.
- Never block the UI thread; never hold the `AppState` lock across drawing or an await.
- Do not weaken the keyring tests that fail on unsigned macOS binaries.
- One agent per file when working in parallel. Shared files (`app.slint`, `app.rs`, `lib.rs`,
  `Cargo.toml`) are wired by one agent only; others report the wiring they need.

## Git and honesty

- Do not add `Co-Authored-By` trailers or Claude footers to commits or PRs in this repo.
- Report measured results and guesses separately. Say "I looked at the screenshot and saw X", not
  "should look fine".
- Progress claims need two numbers: feature coverage and "ready for real work". Do not mark a
  migration phase done because its screen exists.
