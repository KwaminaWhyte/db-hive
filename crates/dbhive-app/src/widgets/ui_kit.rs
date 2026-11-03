//! Shared visual building blocks.
//!
//! Screens compose these instead of drawing their own frames and headers, so
//! a card looks like a card everywhere and one change restyles the app.
//!
//! Nothing here holds state. These are drawing helpers, not widgets with a
//! lifetime, which keeps them usable from inside any layout closure.

use egui::{Response, RichText, Sense, Ui};

use crate::theme::{radius, size, text, Palette};

pub use dbhive_ui_model::format::thousands;

/// The tone of a badge or banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

impl Tone {
    fn colours(self, p: &Palette) -> (egui::Color32, egui::Color32) {
        match self {
            Tone::Neutral => (p.muted_foreground, p.secondary),
            Tone::Accent => (p.primary, p.primary_soft),
            Tone::Success => (p.success, p.success_soft),
            Tone::Warning => (p.warning, p.warning_soft),
            Tone::Danger => (p.destructive, p.destructive_soft),
        }
    }
}

/// A small pill: connection state, environment, row counts, tags.
pub fn badge(ui: &mut Ui, p: &Palette, label: impl Into<String>, tone: Tone) {
    let (fg, bg) = tone.colours(p);
    egui::Frame::none()
        .fill(bg)
        .rounding(radius::PILL)
        .inner_margin(egui::Margin::symmetric(8.0_f32, 2.0_f32))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(size::SMALL).color(fg));
        });
}

/// A badge with a leading dot, for states that are worth spotting quickly.
pub fn status_badge(ui: &mut Ui, p: &Palette, label: impl Into<String>, tone: Tone) {
    let (fg, bg) = tone.colours(p);
    egui::Frame::none()
        .fill(bg)
        .rounding(radius::PILL)
        .inner_margin(egui::Margin::symmetric(8.0_f32, 2.0_f32))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(6.0_f32, 6.0_f32), Sense::hover());
                ui.painter().circle_filled(rect.center(), 3.0, fg);
                ui.label(RichText::new(label).size(size::SMALL).color(fg));
            });
        });
}

/// A content card: a raised surface with a border.
pub fn card<R>(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::none()
        .fill(p.card)
        .stroke(egui::Stroke::new(1.0_f32, p.border))
        .rounding(radius::CARD)
        .inner_margin(egui::Margin::symmetric(14.0_f32, 12.0_f32))
        .show(ui, add)
        .inner
}

/// A card that responds to the pointer, for rows the user can click.
///
/// Returns the response so callers can act on a click, and highlights on
/// hover so a list of rows does not look inert.
pub fn clickable_card<R>(
    ui: &mut Ui,
    p: &Palette,
    selected: bool,
    add: impl FnOnce(&mut Ui) -> R,
) -> (Response, R) {
    // The frame has to be drawn before its hover state is known, so the
    // response from the previous frame decides this frame's fill. On a
    // 60fps UI that is imperceptible and avoids a second layout pass.
    let id = ui.next_auto_id();
    let hovered = ui.ctx().read_response(id).is_some_and(|r| r.hovered());

    let fill = if selected {
        p.primary_soft
    } else if hovered {
        p.elevated
    } else {
        p.card
    };
    let stroke = if selected {
        egui::Stroke::new(1.0_f32, p.primary)
    } else if hovered {
        egui::Stroke::new(1.0_f32, p.border_strong)
    } else {
        egui::Stroke::new(1.0_f32, p.border)
    };

    let inner = egui::Frame::none()
        .fill(fill)
        .stroke(stroke)
        .rounding(radius::CARD)
        .inner_margin(egui::Margin::symmetric(14.0_f32, 12.0_f32))
        .show(ui, add);

    let response = ui.interact(inner.response.rect, id, Sense::click());
    (response, inner.inner)
}

/// A screen title with optional supporting text and trailing controls.
///
/// Every full-window screen starts with one of these, so they all share the
/// same rhythm instead of each inventing a header.
pub fn screen_header(
    ui: &mut Ui,
    p: &Palette,
    title: impl Into<String>,
    subtitle: Option<String>,
    trailing: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(text::heading(p, title));
            if let Some(sub) = subtitle {
                ui.label(text::muted(p, sub));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), trailing);
    });
    ui.add_space(10.0);
    soft_separator(ui, p);
    ui.add_space(12.0);
}

/// A section heading inside a screen.
pub fn section_label(ui: &mut Ui, p: &Palette, label: impl Into<String>) {
    ui.add_space(4.0);
    ui.label(text::subheading(p, label));
    ui.add_space(6.0);
}

/// A hairline that does not shout. egui's separator is heavier than the
/// borders around it, which makes layouts look striped.
pub fn soft_separator(ui: &mut Ui, p: &Palette) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0_f32), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(1.0_f32, p.border),
    );
}

/// A filled, brand-coloured button. egui has no notion of a primary action.
pub fn primary_button(ui: &mut Ui, p: &Palette, label: impl Into<String>) -> Response {
    let text = RichText::new(label)
        .size(size::LABEL)
        .strong()
        .color(p.primary_foreground);
    ui.add(
        egui::Button::new(text)
            .fill(p.primary)
            .stroke(egui::Stroke::NONE)
            .rounding(radius::CONTROL),
    )
}

/// A destructive action, styled so it cannot be mistaken for a safe one.
pub fn danger_button(ui: &mut Ui, p: &Palette, label: impl Into<String>) -> Response {
    let text = RichText::new(label).size(size::LABEL).color(p.destructive);
    ui.add(
        egui::Button::new(text)
            .fill(p.destructive_soft)
            .stroke(egui::Stroke::new(
                1.0_f32,
                p.destructive.gamma_multiply(0.5),
            ))
            .rounding(radius::CONTROL),
    )
}

/// A quiet button for secondary actions in a toolbar.
pub fn ghost_button(ui: &mut Ui, p: &Palette, label: impl Into<String>) -> Response {
    let text = RichText::new(label).size(size::LABEL).color(p.foreground);
    ui.add(
        egui::Button::new(text)
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE)
            .rounding(radius::CONTROL),
    )
}

/// A banner for an error, warning or note that belongs to the whole screen.
///
/// Returns true if the user dismissed it.
pub fn banner(ui: &mut Ui, p: &Palette, tone: Tone, message: impl Into<String>) -> bool {
    let (fg, bg) = tone.colours(p);
    let mut dismissed = false;
    egui::Frame::none()
        .fill(bg)
        .stroke(egui::Stroke::new(1.0_f32, fg.gamma_multiply(0.4)))
        .rounding(radius::CONTROL)
        .inner_margin(egui::Margin::symmetric(12.0_f32, 9.0_f32))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(message).color(fg));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("✖").size(size::SMALL).color(fg))
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE),
                        )
                        .on_hover_text("Dismiss")
                        .clicked()
                    {
                        dismissed = true;
                    }
                });
            });
        });
    ui.add_space(8.0);
    dismissed
}

/// The centred "there is nothing here" state.
///
/// Takes an action closure so the call to action sits inside the block
/// rather than being bolted on underneath it.
pub fn empty_state(
    ui: &mut Ui,
    p: &Palette,
    glyph: &str,
    title: impl Into<String>,
    message: impl Into<String>,
    action: impl FnOnce(&mut Ui),
) {
    ui.add_space(48.0);
    ui.vertical_centered(|ui| {
        // A tinted disc keeps the glyph from floating in space.
        let (rect, _) = ui.allocate_exact_size(egui::vec2(56.0_f32, 56.0_f32), Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 28.0, p.primary_soft);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            egui::FontId::proportional(24.0),
            p.primary,
        );
        ui.add_space(14.0);
        ui.label(text::subheading(p, title));
        ui.add_space(4.0);
        ui.label(text::muted(p, message));
        ui.add_space(14.0);
        action(ui);
    });
}

/// A labelled form row: caption above, control below.
pub fn field<R>(
    ui: &mut Ui,
    p: &Palette,
    label: impl Into<String>,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(text::label(p, label));
        add(ui)
    })
    .inner
}

/// A chrome strip: toolbars, tab rows, status bars.
pub fn bar<R>(ui: &mut Ui, p: &Palette, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::none()
        .fill(p.sidebar)
        .inner_margin(egui::Margin::symmetric(12.0_f32, 0.0_f32))
        .show(ui, |ui| {
            ui.set_min_height(height);
            ui.horizontal_centered(add).inner
        })
        .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tone_resolves_to_a_readable_pair() {
        // A badge whose text matches its own background is invisible.
        for p in [&crate::theme::DARK, &crate::theme::LIGHT] {
            for tone in [
                Tone::Neutral,
                Tone::Accent,
                Tone::Success,
                Tone::Warning,
                Tone::Danger,
            ] {
                let (fg, bg) = tone.colours(p);
                assert_ne!(fg, bg, "{:?} has identical text and background", tone);
            }
        }
    }
}
