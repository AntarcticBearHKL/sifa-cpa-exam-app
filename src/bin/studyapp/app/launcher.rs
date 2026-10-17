use super::*;

use crate::anim::motion;

impl App {
    // ------------------------------------------------------------------
    // screens
    // ------------------------------------------------------------------

    pub(super) fn launcher(&mut self, ui: &mut egui::Ui) {
        let tokens = self.tokens;
        let frame = glass::frame(&tokens);
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            glass::panel_fill(ui, &tokens, false);

            let modes = [
                (Domain::Law, "法考", "卷一 · 卷二"),
                (Domain::Cpa, "注会", "专业阶段"),
            ];

            let card = egui::vec2(320.0, 200.0);
            let gap = tokens.space.lg;
            let block_w = card.x * 2.0 + gap;
            let block_h = 78.0 + card.y + 54.0;

            let area = ui.available_rect_before_wrap();
            let origin = egui::pos2(
                area.center().x - block_w * 0.5,
                (area.center().y - block_h * 0.5).max(area.top() + tokens.space.lg),
            );

            ui.painter().text(
                egui::pos2(area.center().x, origin.y),
                egui::Align2::CENTER_TOP,
                APP_TITLE,
                egui::FontId::proportional(27.0),
                tokens.palette.ink.primary,
            );
            ui.painter().text(
                egui::pos2(area.center().x, origin.y + 44.0),
                egui::Align2::CENTER_TOP,
                "选择一个模式开始",
                egui::FontId::proportional(13.5),
                tokens.palette.ink.muted,
            );

            let mut clicked: Option<Domain> = None;
            for (i, (d, title, sub)) in modes.iter().enumerate() {
                let (subjects, questions, kps) = {
                    let data = self.data(*d);
                    let listed: Vec<_> = data
                        .subjects
                        .iter()
                        .filter(|s| group_of(s).is_some())
                        .collect();
                    (
                        listed.len(),
                        listed.iter().filter_map(|s| s.questions).sum::<i64>(),
                        listed.iter().filter_map(|s| s.knowledge_points).sum::<i64>(),
                    )
                };

                let rect = egui::Rect::from_min_size(
                    egui::pos2(origin.x + i as f32 * (card.x + gap), origin.y + 78.0),
                    card,
                );
                let resp = ui.interact(rect, egui::Id::new(("mode_card", i)), egui::Sense::CLICK);
                let hover = motion::ease_bool(
                    ui.ctx(),
                    egui::Id::new(("mode_card_hover", i)),
                    resp.hovered(),
                    motion::duration::FAST,
                    motion::curve::ENTER,
                );

                let painter = ui.painter();
                let cr = egui::CornerRadius::same(tokens.radius.lg);
                painter.rect_filled(
                    rect,
                    cr,
                    glass::widgets::mix(
                        tokens.palette.glass.fill,
                        tokens.palette.glass.fill_strong,
                        hover,
                    ),
                );
                painter.rect_stroke(
                    rect,
                    cr,
                    egui::Stroke::new(1.0, tokens.palette.glass.stroke),
                    egui::StrokeKind::Inside,
                );

                let left = rect.left() + 26.0;
                painter.text(
                    egui::pos2(left, rect.top() + 28.0),
                    egui::Align2::LEFT_TOP,
                    *title,
                    egui::FontId::proportional(26.0),
                    tokens.palette.ink.primary,
                );
                painter.text(
                    egui::pos2(left, rect.top() + 70.0),
                    egui::Align2::LEFT_TOP,
                    *sub,
                    egui::FontId::proportional(13.0),
                    tokens.palette.ink.muted,
                );
                let rule = rect.top() + 102.0;
                painter.line_segment(
                    [egui::pos2(left, rule), egui::pos2(rect.right() - 26.0, rule)],
                    egui::Stroke::new(1.0, tokens.palette.glass.divider),
                );
                painter.text(
                    egui::pos2(left, rule + 16.0),
                    egui::Align2::LEFT_TOP,
                    format!("{subjects} 个科目"),
                    egui::FontId::proportional(14.5),
                    tokens.palette.ink.secondary,
                );
                painter.text(
                    egui::pos2(left, rule + 44.0),
                    egui::Align2::LEFT_TOP,
                    format!("{questions} 道题 · {kps} 个知识点"),
                    egui::FontId::proportional(12.5),
                    tokens.palette.ink.muted,
                );
                painter.text(
                    egui::pos2(rect.right() - 26.0, rect.bottom() - 24.0),
                    egui::Align2::RIGHT_BOTTOM,
                    "进入 →",
                    egui::FontId::proportional(13.0),
                    glass::widgets::mix(
                        tokens.palette.ink.muted,
                        tokens.palette.accent.base,
                        hover,
                    ),
                );

                if resp.clicked() {
                    clicked = Some(*d);
                }
            }

            ui.painter().text(
                egui::pos2(area.center().x, origin.y + 78.0 + card.y + 24.0),
                egui::Align2::CENTER_TOP,
                "选中的模式会被记住，下次启动直接进入",
                egui::FontId::proportional(12.0),
                tokens.palette.ink.muted,
            );

            if let Some(d) = clicked {
                self.enter_mode(d);
            }
        });
    }
}
