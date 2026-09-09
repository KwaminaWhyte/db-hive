//! Transient notifications, replacing `sonner`.
//!
//! Toasts are drawn as a floating area in the corner and expire on a wall
//! clock, not a frame count: egui only repaints when something happens, so
//! counting frames would leave a toast on screen indefinitely on an idle
//! window. Each live toast requests a repaint so it can actually age out.

use std::time::{Duration, Instant};

use crate::theme::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Success,
    Error,
}

pub struct Toast {
    pub level: Level,
    pub text: String,
    born: Instant,
    ttl: Duration,
}

#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

impl Toasts {
    pub fn info(&mut self, text: impl Into<String>) {
        self.push(Level::Info, text, Duration::from_secs(4));
    }

    pub fn success(&mut self, text: impl Into<String>) {
        self.push(Level::Success, text, Duration::from_secs(4));
    }

    /// Errors linger: they are the ones worth reading, and a query failure
    /// that vanishes in four seconds is a support ticket.
    pub fn error(&mut self, text: impl Into<String>) {
        self.push(Level::Error, text, Duration::from_secs(10));
    }

    fn push(&mut self, level: Level, text: impl Into<String>, ttl: Duration) {
        self.items.push(Toast {
            level,
            text: text.into(),
            born: Instant::now(),
            ttl,
        });
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn show(&mut self, ctx: &egui::Context, palette: &Palette) {
        self.items.retain(|t| t.born.elapsed() < t.ttl);
        if self.items.is_empty() {
            return;
        }
        // Keep the frame loop alive so toasts expire while the app is idle.
        ctx.request_repaint_after(Duration::from_millis(250));

        let mut dismissed = None;
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
            .interactable(true)
            .show(ctx, |ui| {
                for (i, toast) in self.items.iter().enumerate() {
                    let (accent, tint, icon) = match toast.level {
                        Level::Info => (
                            palette.primary,
                            palette.primary_soft,
                            crate::theme::glyph::INFO,
                        ),
                        Level::Success => (
                            palette.success,
                            palette.success_soft,
                            crate::theme::glyph::CHECK,
                        ),
                        Level::Error => (
                            palette.destructive,
                            palette.destructive_soft,
                            crate::theme::glyph::WARNING,
                        ),
                    };
                    egui::Frame::none()
                        .fill(palette.popover)
                        .stroke(egui::Stroke::new(1.0_f32, palette.border_strong))
                        .rounding(crate::theme::radius::CARD)
                        .shadow(egui::epaint::Shadow {
                            offset: egui::vec2(0.0_f32, 4.0_f32),
                            blur: 16.0,
                            spread: 0.0,
                            color: egui::Color32::from_black_alpha(90),
                        })
                        .inner_margin(egui::Margin::symmetric(12.0_f32, 10.0_f32))
                        .show(ui, |ui| {
                            ui.set_max_width(360.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                // A tinted disc, so the level reads before
                                // the text does.
                                let (r, _) = ui.allocate_exact_size(
                                    egui::vec2(20.0_f32, 20.0_f32),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(r.center(), 10.0, tint);
                                ui.painter().text(
                                    r.center(),
                                    egui::Align2::CENTER_CENTER,
                                    icon,
                                    egui::FontId::proportional(11.0),
                                    accent,
                                );
                                ui.label(
                                    egui::RichText::new(&toast.text)
                                        .size(crate::theme::size::LABEL)
                                        .color(palette.foreground),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    egui::RichText::new(
                                                        crate::theme::glyph::CLOSE,
                                                    )
                                                    .size(9.0)
                                                    .color(palette.dim_foreground),
                                                )
                                                .fill(egui::Color32::TRANSPARENT)
                                                .stroke(egui::Stroke::NONE)
                                                .small(),
                                            )
                                            .clicked()
                                        {
                                            dismissed = Some(i);
                                        }
                                    },
                                );
                            });
                        });
                    ui.add_space(8.0);
                }
            });

        if let Some(i) = dismissed {
            self.items.remove(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_outlive_info() {
        let mut t = Toasts::default();
        t.info("a");
        t.error("b");
        assert!(t.items[1].ttl > t.items[0].ttl);
    }

    #[test]
    fn starts_empty() {
        assert!(Toasts::default().is_empty());
    }
}
