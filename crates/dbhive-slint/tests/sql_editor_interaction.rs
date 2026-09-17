//! The SQL editor driven by real key events, headlessly.
//!
//! Rendering alignment is checked by snapshot; this checks the other half of
//! the overlay design: that typing reaches the transparent input and the
//! colour layer follows it.

use dbhive_slint::sql_highlight::{colour, to_model, Kind};
use dbhive_slint::{HighlightLine, SqlEditorSpike};
use slint::platform::WindowEvent;
use slint::{ComponentHandle, Model};

fn type_text(c: &SqlEditorSpike, text: &str) {
    for ch in text.chars() {
        let s: slint::SharedString = ch.to_string().into();
        c.window().dispatch_event(WindowEvent::KeyPressed { text: s.clone() });
        c.window().dispatch_event(WindowEvent::KeyReleased { text: s });
    }
}

fn setup() -> SqlEditorSpike {
    let c = SqlEditorSpike::new().unwrap();
    c.set_text("".into());
    c.set_lines(to_model(""));

    // The glue the real app uses: every edit recomputes the colour layer.
    let weak = c.as_weak();
    c.on_edited(move |text| {
        if let Some(c) = weak.upgrade() {
            c.set_lines(to_model(&text));
        }
    });
    c.show().unwrap();
    c.invoke_focus_editor();
    c
}

fn first_run(lines: &slint::ModelRc<HighlightLine>, line: usize) -> (String, slint::Color) {
    let l = lines.row_data(line).unwrap();
    let r = l.runs.row_data(0).unwrap();
    (r.text.to_string(), r.color)
}

fn typing_reaches_the_input_and_recolours_the_layer() {
    let c = setup();
    type_text(&c, "select 1");

    assert_eq!(c.get_text(), "select 1", "keystrokes did not reach the input");

    let lines = c.get_lines();
    assert_eq!(lines.row_count(), 1);
    let (text, color) = first_run(&lines, 0);
    assert_eq!(text, "select");
    assert_eq!(color, colour(Kind::Keyword), "keyword was not recoloured after typing");
}

fn newlines_create_rows_in_the_colour_layer() {
    let c = setup();
    type_text(&c, "select 1\nfrom t");

    let lines = c.get_lines();
    assert_eq!(lines.row_count(), 2, "a typed newline must add a row");
    assert_eq!(first_run(&lines, 1).0, "from");
}

fn an_unterminated_string_colours_the_rest_of_the_line() {
    let c = setup();
    type_text(&c, "select 'abc");

    let lines = c.get_lines();
    let row = lines.row_data(0).unwrap();
    let last = row.runs.row_data(row.runs.row_count() - 1).unwrap();
    assert_eq!(last.color, colour(Kind::String));
}

/// Slint's platform is process-wide and UI must stay on one thread, so the
/// scenarios run in sequence inside a single test rather than as parallel
/// tests that would each try to install the platform.
#[test]
fn sql_editor_interaction() {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    typing_reaches_the_input_and_recolours_the_layer();
    newlines_create_rows_in_the_colour_layer();
    an_unterminated_string_colours_the_rest_of_the_line();
}
