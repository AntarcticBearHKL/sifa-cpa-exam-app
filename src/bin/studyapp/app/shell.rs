use super::*;

use crate::anim::motion;

impl App {
    pub(super) fn header(&mut self, ui: &mut egui::Ui) {
        let tokens = self.tokens;
        let frame = glass::frame(&tokens);
        egui::Panel::top("header")
            .show_separator_line(false)
            .frame(frame)
            .show(ui, |ui| {
            glass::panel_fill(ui, &tokens, false);
            glass::divider_bottom(ui, &tokens);
            ui.horizontal(|ui| {
                let mode = self.domain.label();
                if glass::widgets::button(
                    ui,
                    &tokens,
                    "mode",
                    RichText::new(mode).size(15.0).strong(),
                    egui::vec2(84.0, 30.0),
                    true,
                )
                .on_hover_text("切换考试模式")
                .clicked()
                {
                    let next = match self.domain {
                        Domain::Law => Domain::Cpa,
                        Domain::Cpa => Domain::Law,
                    };
                    // An empty domain would open a workspace with no subjects;
                    // `App::new` applies the same rule to the startup domain.
                    if !self.data(next).subjects.is_empty() {
                        self.enter_mode(next);
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if win::chrome_button(ui, &tokens, win::ChromeButton::Close).clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if win::chrome_button(ui, &tokens, win::ChromeButton::Minimize).clicked() {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                    ui.add_space(tokens.space.sm);
                    let sel = usize::from(self.mode == theme::Mode::Light);
                    if let Some(i) =
                        glass::widgets::segmented(ui, &tokens, "theme_mode", &["深色", "浅色"], sel)
                    {
                        let mode = if i == 0 {
                            theme::Mode::Dark
                        } else {
                            theme::Mode::Light
                        };
                        self.set_mode(ui.ctx(), mode);
                    }
                    ui.add_space(tokens.space.sm);
                    if glass::widgets::button(
                        ui,
                        &tokens,
                        "settings",
                        RichText::new("设置").size(13.5),
                        egui::vec2(64.0, 30.0),
                        true,
                    )
                    .on_hover_text("打开设置")
                    .clicked()
                    {
                        self.settings_open = true;
                        self.settings_epoch = self.settings_epoch.wrapping_add(1);
                    }
                });
            });
        });
    }

    pub(super) fn rail(&mut self, ui: &mut egui::Ui) {
        let tokens = self.tokens;
        let frame = glass::frame(&tokens);
        egui::Panel::left("rail")
            .resizable(true)
            .default_size(268.0)
            .min_size(210.0)
            .max_size(360.0)
            .show_separator_line(false)
            .frame(frame)
            .show(ui, |ui| {
                glass::panel_fill(ui, &tokens, false);
                glass::divider_right(ui, &tokens);

                let groups = self.subject_groups();
                let current = self.subject_id.clone();
                let mut pick: Option<String> = None;

                egui::ScrollArea::vertical()
                    .id_salt("rail_scroll")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (group, subjects) in &groups {
                            ui.add_space(tokens.space.md);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(group)
                                        .size(12.0)
                                        .strong()
                                        .color(tokens.palette.ink.secondary),
                                );
                                ui.label(
                                    RichText::new(format!("{} 个科目", subjects.len()))
                                        .size(11.0)
                                        .color(tokens.palette.ink.muted),
                                );
                            });
                            ui.add_space(tokens.space.xs);
                            for s in subjects {
                                let meta = s.questions.map(|n| n.to_string()).unwrap_or_default();
                                let selected = s.id == current;
                                let resp = Self::rail_row(
                                    ui,
                                    &tokens,
                                    egui::Id::new(("rail", &s.id)),
                                    &s.name,
                                    &meta,
                                    selected,
                                );
                                if resp.clicked() {
                                    pick = Some(s.id.clone());
                                }
                            }
                        }
                        ui.add_space(tokens.space.md);
                    });

                if let Some(id) = pick {
                    self.select_subject(id);
                }
            });
    }
    /// A subject row in the rail: name left, count right, accent bar when selected.
fn rail_row(
    ui: &mut egui::Ui,
    tokens: &theme::Tokens,
    id: egui::Id,
    name: &str,
    meta: &str,
    selected: bool,
) -> egui::Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::CLICK);
    let hover = motion::ease_bool(
        ui.ctx(),
        id.with("hover"),
        resp.hovered(),
        motion::duration::FAST,
        motion::curve::ENTER,
    );
    let sel = motion::ease_bool(
        ui.ctx(),
        id.with("selected"),
        selected,
        motion::duration::BASE,
        motion::curve::STATE,
    );
    let strength = hover.max(sel);

    let painter = ui.painter();
    if strength > 0.001 {
        let target = glass::widgets::mix(
            tokens.palette.glass.fill_strong,
            tokens.palette.accent.soft,
            sel,
        );
        painter.rect_filled(
            rect,
            egui::CornerRadius::same(tokens.radius.sm),
            glass::widgets::mix(egui::Color32::TRANSPARENT, target, strength),
        );
        if sel > 0.001 {
            let bar = egui::Rect::from_min_size(
                egui::pos2(rect.left() + 2.0, rect.top() + 7.0),
                egui::vec2(2.5, rect.height() - 14.0),
            );
            // The marker is a stronger part of the same tint, not a
            // full-strength accent chip against the sheet.
            painter.rect_filled(
                bar,
                egui::CornerRadius::same(2),
                glass::widgets::mix(
                    tokens.palette.accent.soft,
                    tokens.palette.accent.base,
                    0.5,
                )
                .gamma_multiply(sel),
            );
        }
    }

    painter.text(
        egui::pos2(rect.left() + tokens.space.sm, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(13.5),
        tokens.palette.ink.primary,
    );
    if !meta.is_empty() {
        painter.text(
            egui::pos2(rect.right() - tokens.space.sm, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            meta,
            egui::FontId::proportional(11.5),
            tokens.palette.ink.muted,
        );
    }
    resp
}
/// Page title: the subject name plus its functional chapter/question counts.
    fn subject_header(&self, ui: &mut egui::Ui) {
        let tokens = self.tokens;
        let (chapters, questions) = match &self.subject {
            Some(s) => {
                let distinct: std::collections::HashSet<&str> = s
                    .questions
                    .iter()
                    .filter_map(|q| q.chapter.as_deref())
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                    .collect();
                (distinct.len(), s.questions.len())
            }
            None => (0, 0),
        };

        ui.add_space(tokens.space.xs);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(&self.subject_name).size(20.0).strong());
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!("{chapters} 章 · {questions} 题"))
                        .size(12.0)
                        .color(tokens.palette.ink.muted),
                );
            });
        });
    }

    pub(super) fn workspace(&mut self, ui: &mut egui::Ui) {
        let tokens = self.tokens;
        let frame = glass::frame(&tokens);
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            glass::panel_fill(ui, &tokens, false);
            if let Some(err) = &self.subject_error {
                ui.add_space(tokens.space.lg);
                ui.colored_label(
                    tokens.palette.accent.danger,
                    RichText::new(err).size(14.0),
                );
                return;
            }
            if self.subject.is_none() {
                ui.add_space(tokens.space.lg);
                ui.label(
                    RichText::new("请选择左侧科目。")
                        .color(tokens.palette.ink.secondary)
                        .size(14.0),
                );
                return;
            }
            if self.session.is_none() {
                self.subject_header(ui);
                ui.add_space(tokens.space.sm);
                ui.separator();
                ui.add_space(tokens.space.sm);
            }

            self.practice_view(ui);
        });
    }
}
