use super::*;

use crate::anim::motion;
use crate::glass::widgets::mix;

/// Left column of the 知识点 overlay, in points.
const KP_LIST_W: f32 = 250.0;
/// One knowledge-point row.
const KP_ROW_H: f32 = 26.0;
/// One chapter heading (the caret row).
const KP_HEAD_H: f32 = 26.0;

impl App {
    /// Opens the 知识点 overlay. Counts are computed once, here, from the
    /// loaded subject's stats; the overlay itself never rescans the corpus per
    /// frame. First point is selected so the right column has its content on
    /// the first frame the modal is visible.
    pub(super) fn open_knowledge(&mut self) {
        let Some(subject) = &self.subject else {
            return;
        };
        if subject.knowledge_points.is_empty() {
            return;
        }
        self.kp_counts = logic::kp_question_counts(
            &subject.knowledge_points,
            &subject.questions,
            &self.user,
        );
        self.kp_selected = Some(0);
        self.kp_collapsed.clear();
        self.kp_open = true;
        self.kp_epoch = self.kp_epoch.wrapping_add(1);
    }

    /// `egui::Modal` shell copied from [`App::settings_modal`]: opaque surface,
    /// hairline edge, no shadow, Enter fade, Esc / backdrop close.
    pub(super) fn knowledge_modal(&mut self, ctx: &egui::Context) {
        let tokens = self.tokens;
        let enter = motion::change_progress(
            ctx,
            egui::Id::new("kp_enter"),
            self.kp_epoch,
            motion::duration::SLOW,
            motion::curve::ENTER,
        );
        let frame = egui::Frame::new()
            .fill(glass::paint::with_alpha(tokens.palette.glass.solid, enter))
            .stroke(egui::Stroke::new(
                1.0,
                glass::paint::with_alpha(tokens.palette.glass.stroke, enter),
            ))
            .corner_radius(egui::CornerRadius::same(tokens.radius.md))
            .shadow(egui::Shadow::NONE)
            .inner_margin(egui::Margin::symmetric(
                tokens.space.lg as i8,
                tokens.space.md as i8,
            ));
        let area = egui::Modal::default_area(egui::Id::new("kp_modal"))
            .default_size(egui::vec2(820.0, 560.0));

        let resp = egui::Modal::new(egui::Id::new("kp_modal"))
            .area(area)
            .backdrop_color(glass::paint::with_alpha(
                tokens.palette.background.scrim,
                0.45 * enter,
            ))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_opacity(enter);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("知识点").size(17.0).strong());
                    ui.add_space(tokens.space.sm);
                    ui.label(
                        RichText::new(&self.subject_name)
                            .size(12.5)
                            .color(tokens.palette.ink.muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if glass::widgets::button(
                            ui,
                            &tokens,
                            "close_kp",
                            RichText::new("关闭").size(13.0),
                            egui::vec2(64.0, 30.0),
                            true,
                        )
                        .on_hover_text("关闭知识点")
                        .clicked()
                        {
                            self.kp_open = false;
                        }
                    });
                });
                ui.add_space(tokens.space.sm);
                ui.separator();
                ui.add_space(tokens.space.sm);
                self.kp_columns(ui);
            });

        if resp.should_close() {
            self.kp_open = false;
        }
    }

    fn kp_columns(&mut self, ui: &mut egui::Ui) {
        let tokens = self.tokens;
        let Some(subject) = self.subject.clone() else {
            return;
        };
        let kps = &subject.knowledge_points;
        let chapters = kp_chapters(kps);
        let body_h = ui.available_height().max(240.0);

        ui.horizontal_top(|ui| {
            // ---- left: 章节 / 知识点 list ----
            ui.allocate_ui_with_layout(
                egui::vec2(KP_LIST_W, body_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("kp_list")
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                        )
                        .auto_shrink([false, false])
                        .max_height(body_h)
                        .show(ui, |ui| {
                            let mut toggle: Option<String> = None;
                            let mut select: Option<usize> = None;
                            let mut start: Option<usize> = None;
                            for (chapter, indices) in &chapters {
                                let collapsed = self.kp_collapsed.contains(chapter);
                                if kp_heading(ui, &tokens, chapter, collapsed) {
                                    toggle = Some(chapter.clone());
                                }
                                if collapsed {
                                    continue;
                                }
                                for &i in indices {
                                    let (count, _) =
                                        self.kp_counts.get(i).copied().unwrap_or((0, 0));
                                    let title = &kps[i].title;
                                    let (row, arrow) = kp_row(
                                        ui,
                                        &tokens,
                                        ui.id().with(("kp", i)),
                                        title,
                                        count,
                                        self.kp_selected == Some(i),
                                    );
                                    if arrow.clicked() {
                                        start = Some(i);
                                    } else if row.clicked() {
                                        select = Some(i);
                                    }
                                }
                            }
                            if let Some(chapter) = toggle {
                                if !self.kp_collapsed.remove(&chapter) {
                                    self.kp_collapsed.insert(chapter);
                                }
                            }
                            if let Some(i) = select {
                                self.kp_selected = Some(i);
                            }
                            if let Some(i) = start {
                                self.start_kp_session(i);
                            }
                        });
                },
            );

            let (divider, _) =
                ui.allocate_exact_size(egui::vec2(1.0, body_h), egui::Sense::hover());
            ui.painter()
                .rect_filled(divider, 0.0, tokens.palette.glass.divider);

            // ---- right: the selected point ----
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), body_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("kp_detail")
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                        )
                        .auto_shrink([false, false])
                        .max_height(body_h)
                        .show(ui, |ui| {
                            ui.add_space(tokens.space.xs);
                            let Some(i) = self.kp_selected else {
                                ui.label(
                                    RichText::new("选一个知识点")
                                        .size(12.5)
                                        .color(tokens.palette.ink.muted),
                                );
                                return;
                            };
                            let Some(kp) = kps.get(i) else {
                                return;
                            };
                            ui.label(RichText::new(&kp.title).size(15.0).strong());
                            if let Some(freq) = kp.frequency_label() {
                                ui.add_space(tokens.space.xs);
                                glass::widgets::chip(ui, &tokens, freq, false);
                            }
                            ui.add_space(tokens.space.md);

                            let content = kp
                                .content
                                .as_deref()
                                .map(str::trim)
                                .filter(|s| !s.is_empty());
                            match content {
                                Some(text) => {
                                    ui.label(
                                        RichText::new(text).size(13.5).line_height(Some(22.0)),
                                    );
                                }
                                None => {
                                    ui.label(
                                        RichText::new("这条没有正文")
                                            .size(12.5)
                                            .color(tokens.palette.ink.muted),
                                    );
                                }
                            }

                            let mnemonic = kp
                                .mnemonic
                                .as_deref()
                                .map(str::trim)
                                .filter(|s| !s.is_empty());
                            if let Some(text) = mnemonic {
                                ui.add_space(tokens.space.md);
                                // A wash block, not a card: the same raised
                                // surface the rows use, no shadow, small radius.
                                egui::Frame::new()
                                    .fill(tokens.palette.glass.fill_strong)
                                    .corner_radius(egui::CornerRadius::same(tokens.radius.sm))
                                    .inner_margin(egui::Margin::symmetric(
                                        tokens.space.md as i8,
                                        tokens.space.sm as i8,
                                    ))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new("口诀")
                                                .size(11.5)
                                                .color(tokens.palette.ink.muted),
                                        );
                                        ui.add_space(2.0);
                                        ui.label(RichText::new(text).size(13.0));
                                    });
                            }

                            let (total, done) =
                                self.kp_counts.get(i).copied().unwrap_or((0, 0));
                            ui.add_space(tokens.space.md);
                            ui.label(
                                RichText::new(format!("关联真题 {total} 题 · 已做 {done} 题"))
                                    .size(11.5)
                                    .color(tokens.palette.ink.muted),
                            );
                            ui.add_space(tokens.space.sm);
                            if glass::widgets::primary_button(
                                ui,
                                &tokens,
                                "练这个知识点",
                                total > 0,
                            )
                            .clicked()
                            {
                                self.start_kp_session(i);
                            }
                            ui.add_space(tokens.space.md);
                        });
                },
            );
        });
    }

    /// Starts a session over exactly the questions tagged with this point,
    /// honouring 仅练有答案的题 but none of the other setup filters — this is
    /// "practise this point", not "practise this point within the page".
    fn start_kp_session(&mut self, index: usize) {
        let Some(subject) = self.subject.clone() else {
            return;
        };
        let Some(kp) = subject.knowledge_points.get(index) else {
            return;
        };
        let mut order = logic::kp_question_indices(&subject.questions, kp);
        if self.set_answered_only {
            order.retain(|&i| !subject.questions[i].answer.is_empty());
        }
        if order.is_empty() {
            return;
        }
        let Some(pool) = self.subject_pool() else {
            return;
        };
        self.session = Some(Session::new(
            vec![pool],
            order,
            format!("知识点 · {}", kp.title),
            false,
            0.0,
        ));
        self.exam_result = None;
        self.kp_open = false;
        self.dirty = true;
        self.record_current_seen();
    }
}

/// Chapters in first-seen order, each with the indices of their points. The
/// corpus has both `第一章` style chapters and named ones, and both read as a
/// heading; a point without a chapter falls back to `（未分章）`.
fn kp_chapters(kps: &[KnowledgePoint]) -> Vec<(String, Vec<usize>)> {
    let mut out: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, kp) in kps.iter().enumerate() {
        let label = kp.chapter_label();
        match out.iter_mut().find(|(c, _)| *c == label) {
            Some((_, list)) => list.push(i),
            None => out.push((label, vec![i])),
        }
    }
    out
}

/// A chapter heading row: `▼` when open, `▶` when closed, plus the chapter
/// label. Clicking anywhere on it toggles. Returns `true` when clicked.
fn kp_heading(ui: &mut egui::Ui, tokens: &theme::Tokens, chapter: &str, collapsed: bool) -> bool {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, KP_HEAD_H), egui::Sense::CLICK);
    let hover = motion::ease_bool(
        ui.ctx(),
        resp.id.with("hover"),
        resp.hovered(),
        motion::duration::FAST,
        motion::curve::ENTER,
    );
    if hover > 0.001 {
        ui.painter().rect_filled(
            rect.shrink2(egui::vec2(0.0, 1.0)),
            egui::CornerRadius::same(tokens.radius.sm),
            mix(
                egui::Color32::TRANSPARENT,
                tokens.palette.glass.fill_strong,
                hover,
            ),
        );
    }
    let caret = if collapsed { "▶" } else { "▼" };
    ui.painter().text(
        egui::pos2(rect.left() + tokens.space.sm, rect.center().y),
        egui::Align2::LEFT_CENTER,
        caret,
        egui::FontId::proportional(10.0),
        tokens.palette.ink.muted,
    );
    let job = one_line(
        chapter,
        &tokens.palette.ink.secondary,
        rect.width() - 28.0,
    );
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(rect.left() + 22.0, rect.center().y - galley.size().y * 0.5),
        galley,
        tokens.palette.ink.secondary,
    );
    resp.clicked()
}

/// One knowledge-point row: title, question count, `→`. The row selects, the
/// arrow starts a session; their click areas do not overlap (the row's own
/// response stops before the arrow), so one gesture never does both.
fn kp_row(
    ui: &mut egui::Ui,
    tokens: &theme::Tokens,
    id: egui::Id,
    title: &str,
    count: usize,
    selected: bool,
) -> (egui::Response, egui::Response) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, KP_ROW_H), egui::Sense::hover());
    let arrow_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - 22.0, rect.top()),
        rect.max,
    );
    let row_rect = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(arrow_rect.left(), rect.bottom()),
    );
    let row = ui.interact(row_rect, id.with("row"), egui::Sense::CLICK);
    let arrow = ui.interact(arrow_rect, id.with("arrow"), egui::Sense::CLICK);

    let hover = motion::ease_bool(
        ui.ctx(),
        id.with("hover"),
        row.hovered() || arrow.hovered(),
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
    if strength > 0.001 {
        let pill = rect.shrink2(egui::vec2(0.0, 1.0));
        let target = mix(
            tokens.palette.glass.fill_strong,
            tokens.palette.accent.soft,
            sel,
        );
        ui.painter().rect_filled(
            pill,
            egui::CornerRadius::same(tokens.radius.sm),
            mix(egui::Color32::TRANSPARENT, target, strength),
        );
    }

    let count_text = count.to_string();
    let count_galley = ui.painter().layout_no_wrap(
        count_text.clone(),
        egui::FontId::proportional(11.5),
        tokens.palette.ink.muted,
    );
    let count_w = count_galley.size().x;
    ui.painter().text(
        egui::pos2(arrow_rect.left() - tokens.space.xs, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        &count_text,
        egui::FontId::proportional(11.5),
        tokens.palette.ink.muted,
    );
    let name_w = (rect.width() - 22.0 - count_w - tokens.space.md).max(40.0);
    let job = one_line(title, &tokens.palette.ink.primary, name_w);
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(rect.left() + tokens.space.sm, rect.center().y - galley.size().y * 0.5),
        galley,
        tokens.palette.ink.primary,
    );
    let arrow_color = if arrow.hovered() {
        tokens.palette.ink.primary
    } else {
        tokens.palette.ink.muted
    };
    ui.painter().text(
        arrow_rect.center(),
        egui::Align2::CENTER_CENTER,
        "→",
        egui::FontId::proportional(13.0),
        arrow_color,
    );
    (row, arrow)
}

/// A single-line `LayoutJob` that truncates with `…` instead of wrapping: the
/// list's rows are a fixed height and must never grow.
fn one_line(text: &str, color: &egui::Color32, width: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(13.0),
            color: *color,
            ..Default::default()
        },
    );
    job.wrap.max_width = width;
    job.wrap.max_rows = 1;
    job.wrap.overflow_character = Some('…');
    job
}

#[cfg(test)]
mod tests {
    use super::kp_chapters;
    use sfzklearning::KnowledgePoint;

    /// The overlay's headings are the chapter labels in first-seen order; a
    /// point with no chapter lands under `（未分章）` rather than vanishing.
    #[test]
    fn knowledge_points_group_under_their_chapter() {
        let kp = |chapter: Option<&str>| KnowledgePoint {
            chapter: chapter.map(str::to_string),
            ..Default::default()
        };
        let kps = vec![
            kp(Some("第一章")),
            kp(Some("第二章")),
            kp(Some("第一章")),
            kp(None),
        ];
        let chapters = kp_chapters(&kps);
        assert_eq!(chapters.len(), 3);
        assert_eq!(chapters[0].0, "第一章");
        assert_eq!(chapters[0].1, vec![0, 2]);
        assert_eq!(chapters[1].0, "第二章");
        assert_eq!(chapters[1].1, vec![1]);
        assert_eq!(chapters[2].0, "（未分章）");
        assert_eq!(chapters[2].1, vec![3]);
    }
}
