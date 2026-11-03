//! Maps the AI assistant's plain Rust view (`dbhive_ui_model::ai_panel::AiView`)
//! to the `AiState` global the panel draws.
//!
//! Kept apart from the controller so the snapshot tool can fill the same
//! properties without a controller behind them.

use dbhive_ui_model::ai_assistant::{EmptyState, StatusTone};
use dbhive_ui_model::ai_panel::{AiView, TurnView};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::history_view::strings_model;
use crate::{AiDisclosure, AiState, AiTurnRow, AppWindow};

/// The conversation, newest exchange first.
///
/// An exchange is a request and the answer to it. They stay in reading order
/// (request, then answer) inside the exchange; it is the exchanges that are
/// reversed, so the latest answer is on screen without scrolling. Each entry
/// carries its position in the transcript, which is what the controller wants
/// back when a button on it is pressed.
pub fn newest_first(turns: &[TurnView]) -> Vec<(usize, &TurnView)> {
    let mut exchanges: Vec<Vec<(usize, &TurnView)>> = Vec::new();
    for (i, t) in turns.iter().enumerate() {
        if t.user || exchanges.is_empty() {
            exchanges.push(Vec::new());
        }
        if let Some(last) = exchanges.last_mut() {
            last.push((i, t));
        }
    }
    exchanges.into_iter().rev().flatten().collect()
}

fn turn_rows(turns: &[TurnView]) -> ModelRc<AiTurnRow> {
    let rows: Vec<AiTurnRow> = newest_first(turns)
        .into_iter()
        .map(|(i, t)| AiTurnRow {
            index: i as i32,
            user: t.user,
            heading: t.heading.as_str().into(),
            body: t.body.as_str().into(),
            has_sql: t.has_sql,
            sql: t.sql.as_str().into(),
            meta: t.meta.as_str().into(),
            failed: t.failed,
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

fn empty_kind(kind: EmptyState) -> i32 {
    match kind {
        EmptyState::Checking => 0,
        EmptyState::NeedsApiKey => 1,
        EmptyState::OllamaUnavailable => 2,
        EmptyState::Unavailable => 3,
    }
}

/// What the controller adds to the view: things that are the shell's, not the
/// panel's.
#[derive(Default)]
pub struct Extras<'a> {
    /// Why the base URL could not be saved.
    pub settings_error: &'a str,
    pub saving_key: bool,
    /// Rewrite the base URL field. Off while the user is typing in it.
    pub sync_base_url: bool,
}

/// Fill the `AiState` global from `view`.
///
/// Never touches the API key field: what is typed there is the user's, and the
/// controller clears it once the key is stored.
pub fn apply(w: &AppWindow, view: &AiView, extras: &Extras<'_>) {
    let s = w.global::<AiState>();

    s.set_providers(strings_model(&view.providers));
    s.set_provider_index(view.provider_index as i32);
    s.set_status_tone(match view.status_tone {
        StatusTone::Muted => 0,
        StatusTone::Success => 1,
        StatusTone::Warning => 2,
        StatusTone::Destructive => 3,
    });
    s.set_status_text(view.status_text.as_str().into());
    s.set_needs_key(view.needs_key);
    s.set_key_saved(view.key_saved);
    s.set_key_hint(view.key_hint.as_str().into());
    s.set_saving_key(extras.saving_key);
    if extras.sync_base_url {
        s.set_base_url(view.base_url.as_str().into());
    }
    s.set_base_url_warning(view.base_url_warning.as_str().into());
    s.set_settings_error(extras.settings_error.into());
    s.set_show_settings(view.show_settings);

    s.set_models(strings_model(&view.models));
    s.set_model_text(view.model_text.as_str().into());
    s.set_models_loading(view.models_loading);
    s.set_models_error(view.models_error.as_str().into());

    match &view.empty {
        Some(e) => {
            s.set_empty_kind(empty_kind(e.kind));
            s.set_empty_title(e.title.as_str().into());
            s.set_empty_message(e.message.as_str().into());
            s.set_empty_action(e.action.as_str().into());
        }
        None => {
            s.set_empty_kind(-1);
            s.set_empty_title("".into());
            s.set_empty_message("".into());
            s.set_empty_action("".into());
        }
    }

    s.set_busy(view.busy);
    s.set_busy_label(view.busy_label.as_str().into());
    s.set_can_generate(view.can_generate);
    s.set_can_explain(view.can_explain);
    s.set_can_optimize(view.can_optimize);
    s.set_can_fix(view.can_fix);
    s.set_has_sql(view.has_sql);
    s.set_sql_preview(view.sql_preview.as_str().into());
    s.set_has_error(view.has_error);
    s.set_error_text(view.error_text.as_str().into());
    s.set_notice(view.notice.as_str().into());

    s.set_include_schema(view.include_schema);
    s.set_show_context(view.show_context);
    s.set_context_text(view.context_text.as_str().into());
    s.set_context_lines(view.context_text.lines().count().max(1) as i32);
    s.set_context_summary(view.context_summary.as_str().into());
    s.set_context_loading(view.context_loading);
    s.set_context_error(view.context_error.as_str().into());
    s.set_disclosure(ModelRc::new(VecModel::from(
        view.disclosure
            .iter()
            .map(|d| AiDisclosure {
                label: d.label.as_str().into(),
                text: d.text.as_str().into(),
            })
            .collect::<Vec<_>>(),
    )));

    s.set_turns(turn_rows(&view.turns));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(user: bool, heading: &str) -> TurnView {
        TurnView {
            user,
            heading: heading.into(),
            body: String::new(),
            has_sql: false,
            sql: String::new(),
            meta: String::new(),
            failed: false,
        }
    }

    fn order(turns: &[TurnView]) -> Vec<(usize, String)> {
        newest_first(turns)
            .into_iter()
            .map(|(i, t)| (i, t.heading.clone()))
            .collect()
    }

    #[test]
    fn exchanges_are_newest_first_and_each_reads_request_then_answer() {
        let turns = vec![
            turn(true, "You"),
            turn(false, "Generate SQL"),
            turn(true, "Explain this"),
            turn(false, "Explain"),
            turn(true, "Fix this"),
        ];
        assert_eq!(
            order(&turns),
            vec![
                (4, "Fix this".into()),
                (2, "Explain this".into()),
                (3, "Explain".into()),
                (0, "You".into()),
                (1, "Generate SQL".into()),
            ]
        );
    }

    #[test]
    fn indexes_point_back_into_the_transcript() {
        let turns = vec![
            turn(true, "a"),
            turn(false, "b"),
            turn(true, "c"),
            turn(false, "d"),
        ];
        for (i, t) in newest_first(&turns) {
            assert_eq!(turns[i].heading, t.heading);
        }
    }

    #[test]
    fn an_empty_transcript_and_a_leading_answer_do_not_panic() {
        assert!(newest_first(&[]).is_empty());
        let turns = vec![turn(false, "orphan"), turn(true, "q")];
        assert_eq!(order(&turns), vec![(1, "q".into()), (0, "orphan".into())]);
    }
}
