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
        // A resizable panel paints a line on its edge while hovered or dragged
        // (`show_separator_line(false)` only suppresses the *resting* line —
        // see egui's `panel.rs`, which picks `widgets.hovered.fg_stroke` /
        // `widgets.active.fg_stroke` for those two states). Both are the
        // "highly visible" strokes, so the rail flashed a bright bar whenever
        // the pointer came near its edge. Zeroing just those two strokes for
        // the duration of the panel keeps the drag-to-resize behaviour and
        // removes the bar; the rail's own rows paint their own hover, so
        // nothing inside it depends on them.
        let saved = ui.style().clone();
        let mut quiet = (*saved).clone();
        quiet.visuals.widgets.hovered.fg_stroke = egui::Stroke::NONE;
        quiet.visuals.widgets.active.fg_stroke = egui::Stroke::NONE;
        ui.set_style(quiet);
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

                let total = subject_question_total(&self.data(self.domain).subjects);
                let today = today_unix_day();
                let date = date_from_unix_day(today);
                let counts = today_counts(
                    &self.user.stats,
                    &self.user.favorites,
                    self.domain,
                    total,
                    today,
                    &date,
                );
                let mut start_today = false;
                self.today_block(ui, &counts, weekday_label(today), &mut start_today);
                let (rule, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 1.0),
                    egui::Sense::hover(),
                );
                ui.painter()
                    .rect_filled(rule, 0.0, tokens.palette.glass.divider);
                if start_today {
                    self.start_today_session();
                }

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
        ui.set_style(saved);
    }
    /// The 今天 block at the top of the rail. Built from `user.json` +
    /// `summary.json` only — it must not open a subject file to say how much is
    /// waiting.
    fn today_block(
        &self,
        ui: &mut egui::Ui,
        counts: &TodayCounts,
        weekday: &str,
        start: &mut bool,
    ) {
        let tokens = self.tokens;
        ui.add_space(tokens.space.sm);
        let width = ui.available_width();
        let (title, _) = ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::hover());
        ui.painter().text(
            egui::pos2(title.left(), title.center().y),
            egui::Align2::LEFT_CENTER,
            "今天",
            egui::FontId::proportional(15.0),
            tokens.palette.ink.primary,
        );
        ui.painter().text(
            egui::pos2(title.right(), title.center().y),
            egui::Align2::RIGHT_CENTER,
            weekday,
            egui::FontId::proportional(11.5),
            tokens.palette.ink.muted,
        );
        ui.add_space(tokens.space.xs);

        // Three equal columns, painted as one row so the numbers share a
        // baseline and a column width (the "tabular" reading) even though the
        // values differ in digit count.
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 42.0), egui::Sense::hover());
        let columns = [
            (counts.due, "到期"),
            (counts.new, "新题"),
            (counts.favorites, "收藏"),
        ];
        let col = width / 3.0;
        for (i, (value, caption)) in columns.iter().enumerate() {
            let x = rect.left() + col * i as f32;
            ui.painter().text(
                egui::pos2(x, rect.top()),
                egui::Align2::LEFT_TOP,
                value.to_string(),
                egui::FontId::proportional(19.0),
                tokens.palette.ink.primary,
            );
            ui.painter().text(
                egui::pos2(x, rect.top() + 24.0),
                egui::Align2::LEFT_TOP,
                *caption,
                egui::FontId::proportional(11.5),
                tokens.palette.ink.muted,
            );
        }

        let empty = counts.due + counts.new + counts.favorites == 0;
        if empty {
            ui.label(
                RichText::new("今天没有到期题")
                    .size(12.5)
                    .color(tokens.palette.ink.muted),
            );
        } else {
            let (button, resp) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), 34.0),
                egui::Sense::CLICK,
            );
            let hover = motion::ease_bool(
                ui.ctx(),
                resp.id.with("hover"),
                resp.hovered(),
                motion::duration::FAST,
                motion::curve::ENTER,
            );
            let fill = glass::widgets::mix(
                glass::widgets::mix(
                    tokens.palette.glass.fill_strong,
                    tokens.palette.accent.soft,
                    0.6,
                ),
                tokens.palette.accent.soft,
                hover,
            );
            ui.painter().rect_filled(
                button,
                egui::CornerRadius::same(tokens.radius.sm),
                fill,
            );
            ui.painter().rect_stroke(
                button,
                egui::CornerRadius::same(tokens.radius.sm),
                egui::Stroke::new(1.0, tokens.palette.glass.stroke),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                button.center(),
                egui::Align2::CENTER_CENTER,
                "开始今天的复习",
                egui::FontId::proportional(13.5),
                tokens.palette.ink.primary,
            );
            if resp.clicked() {
                *start = true;
            }

            // 今日已做 / 到期 + 新题. A done count larger than the queue only
            // means extra practice, so the bar clamps at full.
            let target = (counts.due + counts.new) as f32;
            let ratio = if target > 0.0 {
                (counts.done as f32 / target).clamp(0.0, 1.0)
            } else {
                0.0
            };
            ui.add_space(tokens.space.xs);
            let (bar, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), 3.0),
                egui::Sense::hover(),
            );
            ui.painter().rect_filled(
                bar,
                egui::CornerRadius::same(2),
                tokens.palette.glass.fill_sunken,
            );
            if ratio > 0.0 {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * ratio, 3.0)),
                    egui::CornerRadius::same(2),
                    tokens.palette.accent.base,
                );
            }
        }
        ui.add_space(tokens.space.xs);
        ui.label(
            RichText::new(format!("今日 {} 题", counts.done))
                .size(11.5)
                .color(tokens.palette.ink.muted),
        );
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
    fn subject_header(&mut self, ui: &mut egui::Ui) {
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if glass::widgets::button(
                    ui,
                    &tokens,
                    "knowledge",
                    RichText::new("知识点").size(13.0),
                    egui::vec2(0.0, 0.0),
                    true,
                )
                .on_hover_text("按知识点浏览与练习")
                .clicked()
                {
                    self.open_knowledge();
                }
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
