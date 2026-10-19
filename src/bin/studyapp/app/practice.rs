use super::*;

use crate::anim::motion;
use crate::glass::widgets::mix;

impl App {
    // ---- 做题 ----

    pub(super) fn practice_view(&mut self, ui: &mut egui::Ui) {
        if self.session.is_some() {
            self.session_view(ui);
        } else {
            self.practice_setup(ui);
        }
    }

    fn practice_setup(&mut self, ui: &mut egui::Ui) {
        let tokens = self.tokens;
        let matched = self.matching_questions().len();
        let wrong = self.wrong_count();
        let chapter_list = self
            .subject
            .as_ref()
            .map(|s| logic::chapter_counts(&s.questions))
            .unwrap_or_default();
        let real_chapters = self
            .subject
            .as_ref()
            .map(|s| {
                s.questions
                    .iter()
                    .filter_map(|q| q.chapter.as_deref())
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                    .collect::<std::collections::HashSet<_>>()
                    .len()
            })
            .unwrap_or(0);

        // One grid for the whole page: every setting control starts at
        // `LABEL_W`, every row is `ROW_H` tall, and both column headers are
        // `HEAD_H` tall, so the two columns keep the same rhythm.
        const LABEL_W: f32 = 76.0;
        const ROW_H: f32 = 38.0;
        const HEAD_H: f32 = 32.0;
        // The footer bar keeps a fixed slice of the height; the body takes all
        // the rest, so the chapter list can fill whatever is left.
        const FOOTER_H: f32 = 66.0;
        // Wide enough for the widest row — the 正确率 segmented control (5
        // items) — so every row's hairline spans its whole control; below this
        // the column is still content-sized (the divider follows the widest
        // control), only the rules would come up short.
        let left_w = 410.0f32.min((ui.available_width() - 260.0).max(300.0));

        // A hairline from the palette — the page is structured with rules,
        // never with a second surface fill and never with egui's separator.
        let hairline = |ui: &mut egui::Ui, width: f32| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 0.0, tokens.palette.glass.divider);
        };
        // Fixed label cell; its optional small tag stays inside the label
        // column, so a hint can never push a row's controls onto a second line.
        let cell = |ui: &mut egui::Ui, text: &str, tag: Option<&str>| {
            ui.allocate_ui_with_layout(
                egui::vec2(LABEL_W, ROW_H),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(
                        RichText::new(text)
                            .size(13.5)
                            .color(tokens.palette.ink.secondary),
                    );
                    if let Some(tag) = tag {
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(tag)
                                .size(10.5)
                                .color(tokens.palette.ink.muted),
                        );
                    }
                },
            );
        };

        // ---- page header: the title block is closed by a hairline and sits on
        //      the panel's own fill — no second surface anywhere ----
        ui.add_space(tokens.space.xs);
        ui.label(RichText::new("练习设置").size(18.0).strong());
        ui.add_space(2.0);
        ui.label(
            RichText::new("先筛出要练的范围，再开始做题。")
                .size(13.0)
                .color(tokens.palette.ink.secondary),
        );
        ui.add_space(tokens.space.sm);
        hairline(ui, ui.available_width());
        ui.add_space(tokens.space.md);

        // ---- body: two flush columns divided by one vertical hairline ----
        // Measured after the title block, so the footer always fits below it.
        let body_h = (ui.available_height() - FOOTER_H).max(240.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), body_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(left_w, body_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(left_w, HEAD_H),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("怎么练")
                                            .size(12.5)
                                            .strong()
                                            .color(tokens.palette.ink.secondary),
                                    );
                                },
                            );
                            ui.add_space(tokens.space.xs);

                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), ROW_H),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    cell(ui, "题型", None);
                                    let kinds = ["全部", "选择题", "记忆自测"];
                                    let sel = match self.set_kind {
                                        logic::Kind::All => 0,
                                        logic::Kind::Choice => 1,
                                        logic::Kind::Memory => 2,
                                    };
                                    if let Some(i) = glass::widgets::segmented(
                                        ui, &tokens, "set_kind", &kinds, sel,
                                    ) {
                                        self.set_kind = match i {
                                            1 => logic::Kind::Choice,
                                            2 => logic::Kind::Memory,
                                            _ => logic::Kind::All,
                                        };
                                    }
                                },
                            );
                            hairline(ui, left_w);

                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), ROW_H),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    cell(ui, "来源", Some("可多选"));
                                    let sources = self.set_sources;
                                    if glass::widgets::toggle_chip(
                                        ui,
                                        &tokens,
                                        "仅错题",
                                        sources.wrong,
                                    )
                                    .clicked()
                                    {
                                        self.set_sources.wrong = !sources.wrong;
                                    }
                                    if glass::widgets::toggle_chip(
                                        ui,
                                        &tokens,
                                        "未做过",
                                        sources.unseen,
                                    )
                                    .clicked()
                                    {
                                        self.set_sources.unseen = !sources.unseen;
                                    }
                                    if glass::widgets::toggle_chip(
                                        ui,
                                        &tokens,
                                        "做错过",
                                        sources.missed,
                                    )
                                    .clicked()
                                    {
                                        self.set_sources.missed = !sources.missed;
                                    }
                                },
                            );
                            hairline(ui, left_w);

                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), ROW_H),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    cell(ui, "收藏", None);
                                    let favs = ["不限", "已收藏", "未收藏"];
                                    let sel = match self.set_fav {
                                        logic::Fav::Any => 0,
                                        logic::Fav::Yes => 1,
                                        logic::Fav::No => 2,
                                    };
                                    if let Some(i) = glass::widgets::segmented(
                                        ui, &tokens, "set_fav", &favs, sel,
                                    ) {
                                        self.set_fav = match i {
                                            1 => logic::Fav::Yes,
                                            2 => logic::Fav::No,
                                            _ => logic::Fav::Any,
                                        };
                                    }
                                },
                            );
                            hairline(ui, left_w);

                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), ROW_H),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    cell(ui, "正确率", None);
                                    let accs = ["不限", "未作答", "低于 60%", "低于 100%", "全对"];
                                    let sel = match self.set_accuracy {
                                        logic::Accuracy::Any => 0,
                                        logic::Accuracy::Unanswered => 1,
                                        logic::Accuracy::Below60 => 2,
                                        logic::Accuracy::Below100 => 3,
                                        logic::Accuracy::Perfect => 4,
                                    };
                                    if let Some(i) = glass::widgets::segmented(
                                        ui, &tokens, "set_accuracy", &accs, sel,
                                    ) {
                                        self.set_accuracy = match i {
                                            1 => logic::Accuracy::Unanswered,
                                            2 => logic::Accuracy::Below60,
                                            3 => logic::Accuracy::Below100,
                                            4 => logic::Accuracy::Perfect,
                                            _ => logic::Accuracy::Any,
                                        };
                                    }
                                },
                            );
                            hairline(ui, left_w);

                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), ROW_H),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    cell(ui, "题量", None);
                                    let sizes = [
                                        logic::Draw::All.label(),
                                        logic::Draw::N10.label(),
                                        logic::Draw::N20.label(),
                                        logic::Draw::N50.label(),
                                        logic::Draw::Custom.label(),
                                    ];
                                    let sel = match self.set_draw {
                                        logic::Draw::All => 0,
                                        logic::Draw::N10 => 1,
                                        logic::Draw::N20 => 2,
                                        logic::Draw::N50 => 3,
                                        logic::Draw::Custom => 4,
                                    };
                                    if let Some(i) = glass::widgets::segmented(
                                        ui, &tokens, "set_draw", &sizes, sel,
                                    ) {
                                        self.set_draw = match i {
                                            1 => logic::Draw::N10,
                                            2 => logic::Draw::N20,
                                            3 => logic::Draw::N50,
                                            4 => logic::Draw::Custom,
                                            _ => logic::Draw::All,
                                        };
                                    }
                                    if self.set_draw == logic::Draw::Custom {
                                        ui.add_space(tokens.space.xs);
                                        glass::widgets::number_input(
                                            ui,
                                            &tokens,
                                            &mut self.set_draw_custom,
                                            1..=logic::MAX_CUSTOM_DRAW,
                                        );
                                    }
                                },
                            );
                            hairline(ui, left_w);

                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), ROW_H),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    cell(ui, "顺序", None);
                                    let orders = [
                                        logic::Order::Sequential.label(),
                                        logic::Order::Shuffled.label(),
                                    ];
                                    let sel = match self.set_order {
                                        logic::Order::Sequential => 0,
                                        logic::Order::Shuffled => 1,
                                    };
                                    if let Some(i) = glass::widgets::segmented(
                                        ui, &tokens, "set_order", &orders, sel,
                                    ) {
                                        self.set_order = match i {
                                            1 => logic::Order::Shuffled,
                                            _ => logic::Order::Sequential,
                                        };
                                    }
                                },
                            );
                            hairline(ui, left_w);

                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), ROW_H),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    cell(ui, "搜索", None);
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.set_query)
                                            .hint_text("题干 / 解析关键词")
                                            .margin(tokens.text_input_margin())
                                            .desired_width(260.0),
                                    );
                                },
                            );

                        },
                    );

                    let (divider, _) =
                        ui.allocate_exact_size(egui::vec2(1.0, body_h), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(divider, 0.0, tokens.palette.glass.divider);

                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), body_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), HEAD_H),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("练哪些章节")
                                            .size(12.5)
                                            .strong()
                                            .color(tokens.palette.ink.secondary),
                                    );
                                    ui.add_space(tokens.space.xs);
                                    ui.label(
                                        RichText::new(format!(
                                            "已选 {} / {} 章",
                                            self.set_chapters.len(),
                                            real_chapters
                                        ))
                                        .size(12.0)
                                        .color(tokens.palette.ink.muted),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let mut select_all = false;
                                            let mut clear_all = false;
                                            let mut invert = false;
                                            if small_button(ui, &tokens, "invert", "反选").clicked() {
                                                invert = true;
                                            }
                                            if small_button(ui, &tokens, "clear", "清空").clicked() {
                                                clear_all = true;
                                            }
                                            if small_button(ui, &tokens, "all", "全选").clicked() {
                                                select_all = true;
                                            }
                                            if select_all {
                                                self.set_chapters = chapter_list
                                                    .iter()
                                                    .map(|(n, _)| n.clone())
                                                    .collect();
                                            }
                                            if clear_all {
                                                self.set_chapters.clear();
                                            }
                                            if invert {
                                                for (name, _) in &chapter_list {
                                                    if !self.set_chapters.remove(name) {
                                                        self.set_chapters.insert(name.clone());
                                                    }
                                                }
                                            }
                                        },
                                    );
                                },
                            );
                            ui.add_space(tokens.space.xs);

                            ui.add(
                                egui::TextEdit::singleline(&mut self.set_chapter_filter)
                                    .hint_text("筛选章节")
                                    .margin(tokens.text_input_margin())
                                    .desired_width(f32::INFINITY),
                            );

                            let needle = self.set_chapter_filter.trim().to_lowercase();
                            egui::ScrollArea::vertical()
                                .id_salt("set_chapters")
                                .scroll_bar_visibility(
                                    egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                                )
                                .max_height(ui.available_height())
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let mut any = false;
                                    for (name, count) in &chapter_list {
                                        if !needle.is_empty()
                                            && !name.to_lowercase().contains(&needle)
                                        {
                                            continue;
                                        }
                                        any = true;
                                        let checked = self.set_chapters.contains(name);
                                        let row = glass::widgets::check_row(
                                            ui,
                                            &tokens,
                                            ui.id().with(name),
                                            name,
                                            &format!("{count} 题"),
                                            checked,
                                            ui.available_width(),
                                        );
                                        if row.clicked() {
                                            if checked {
                                                self.set_chapters.remove(name);
                                            } else {
                                                self.set_chapters.insert(name.clone());
                                            }
                                        }
                                    }
                                    if !any {
                                        ui.label(
                                            RichText::new("没有匹配的章节")
                                                .size(12.5)
                                                .color(tokens.palette.ink.muted),
                                        );
                                    }
                                });
                        },
                    );
                });
            },
        );

        // ---- footer: the range summary and both actions share one bar ----
        hairline(ui, ui.available_width());
        ui.add_space(tokens.space.sm);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), ROW_H),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(
                    RichText::new(format!("本范围共 {matched} 题"))
                        .size(16.0)
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if glass::widgets::primary_button(ui, &tokens, "开始练习", matched > 0).clicked()
                    {
                        self.start_session(false);
                    }
                    ui.add_space(tokens.space.sm);
                    if glass::widgets::button(
                        ui,
                        &tokens,
                        "wrong_sample",
                        RichText::new("错题随机 10 题").size(14.5),
                        egui::vec2(152.0, 38.0),
                        wrong > 0,
                    )
                    .on_hover_text("从错题里随机抽 10 道")
                    .clicked()
                    {
                        self.start_session(true);
                    }
                });
            },
        );
    }

    fn session_view(&mut self, ui: &mut egui::Ui) {
        let tokens = self.tokens;
        let now = ui.input(|i| i.time);
        let Some(q) = self.session_question() else {
            return;
        };
        let qid = q.id.clone();
        if self.note_draft_for.as_deref() != Some(qid.as_str()) {
            self.flush_note();
            self.note_draft = self.user.note(&qid).unwrap_or_default().to_string();
            self.note_draft_for = Some(qid.clone());
            self.note_open = false;
        }

        let (pos, total, label) = {
            let s = self.session.as_ref().unwrap();
            (s.pos + 1, s.order.len(), s.label.clone())
        };
        // `submitted` = graded with 提交答案; `answer_shown` = revealed with
        // 显示答案 (nothing recorded); `self_assessed` = a memory / self-test
        // question was rated 记住了 / 没记住. `revealed` is the shared "the
        // answer is visible" state that drives the same rendering for a
        // submitted and a merely revealed choice question.
        let (submitted, answer_shown, self_assessed) = self
            .session
            .as_ref()
            .map(|s| (s.submitted, s.shown, s.self_assessed))
            .unwrap_or((false, false, false));
        let revealed = submitted || answer_shown;
        let can_next = logic::can_advance(&q, submitted, answer_shown, self_assessed);

        let q_fade = motion::change_progress(
            ui.ctx(),
            egui::Id::new("session_q_fade"),
            question_key_variant(&q, pos as u64),
            motion::duration::BASE,
            motion::curve::ENTER,
        );
        let reveal = if revealed {
            motion::change_progress(
                ui.ctx(),
                egui::Id::new("session_reveal"),
                self.reveal_epoch,
                motion::duration::BASE,
                motion::curve::ENTER,
            )
        } else {
            1.0
        };

        ui.add_space(tokens.space.sm);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&label)
                    .size(13.0)
                    .color(tokens.palette.ink.secondary),
            );
            ui.add_space(tokens.space.sm);
            ui.label(
                RichText::new(format!("第 {pos} / {total} 题"))
                    .size(14.0)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if glass::widgets::button(
                    ui,
                    &tokens,
                    "end_session",
                    "结束练习",
                    egui::vec2(0.0, 0.0),
                    true,
                )
                .clicked()
                {
                    self.flush_note();
                    self.session = None;
                    self.note_draft_for = None;
                    self.dirty = true;
                }
            });
        });
        ui.add_space(tokens.space.sm);
        let frac = if total == 0 { 0.0 } else { pos as f32 / total as f32 };
        let shown = motion::ease_value(
            ui.ctx(),
            egui::Id::new("session_progress"),
            frac,
            motion::duration::BASE,
            motion::curve::ENTER,
        );
        let (bar, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 4.0),
            egui::Sense::hover(),
        );
        let painter = ui.painter();
        painter.rect_filled(
            bar,
            egui::CornerRadius::same(2),
            tokens.palette.glass.fill_strong,
        );
        let width = (bar.width() * shown).max(3.0);
        let done = egui::Rect::from_min_size(bar.min, egui::vec2(width, bar.height()));
        painter.rect_filled(done, egui::CornerRadius::same(2), tokens.palette.accent.base);

        let mut do_submit = false;
        let mut do_reveal = false;
        let mut do_assess: Option<bool> = None;
        let mut do_next = false;
        let mut do_prev = false;
        let shake = self.anim.shake(question_key(&q), now) * anim::SHAKE_AMPLITUDE;

        const FOOTER_H: f32 = 74.0;
        let body_h = (ui.available_height() - FOOTER_H).max(180.0);
        let mut area = egui::ScrollArea::vertical()
            .id_salt("session_scroll")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
        let scroll_top = self.session.as_ref().map(|s| s.pos).unwrap_or(0);
        let last_top = ui.data(|d| d.get_temp::<usize>(egui::Id::new("session_last_top")));
        if last_top != Some(scroll_top) {
            area = area.vertical_scroll_offset(0.0);
            ui.data_mut(|d| d.insert_temp(egui::Id::new("session_last_top"), scroll_top));
        }

        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), body_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                area.auto_shrink([false, false])
                    .max_height(body_h)
                    .show(ui, |ui| {
                        ui.add_space(tokens.space.lg);
                        let full_w = ui.available_width();
                        let col_w = (full_w - 96.0).clamp(320.0, 780.0);
                        let pad = ((full_w - col_w) * 0.5).max(0.0);
                        ui.horizontal(|ui| {
                            ui.add_space(pad);
                            ui.vertical(|ui| {
                                ui.set_max_width(col_w);
            ui.horizontal(|ui| {
                glass::widgets::chip(ui, &tokens, &q.type_label(), false);
                ui.add_space(tokens.space.xs);
                ui.label(
                    RichText::new(q.chapter_label())
                        .size(12.5)
                        .color(tokens.palette.ink.muted),
                );
                if let Some(y) = q.year_label() {
                    ui.label(
                        RichText::new(format!("{y} 年"))
                            .size(12.5)
                            .color(tokens.palette.ink.muted),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let note_label = if self.user.note(&qid).is_some() {
                        "备注 ●"
                    } else {
                        "备注"
                    };
                    if glass::widgets::button(
                        ui,
                        &tokens,
                        "note",
                        RichText::new(note_label),
                        egui::vec2(0.0, 0.0),
                        true,
                    )
                    .clicked()
                    {
                        self.note_open = !self.note_open;
                    }
                    ui.add_space(tokens.space.xs);
                    let fav = self.user.is_favorite(&qid);
                    let star = if fav { "★ 已收藏" } else { "☆ 收藏" };
                    let color = if fav {
                        tokens.palette.accent.warn
                    } else {
                        tokens.palette.ink.secondary
                    };
                    if glass::widgets::button(
                        ui,
                        &tokens,
                        "favorite",
                        RichText::new(star).color(color),
                        egui::vec2(0.0, 0.0),
                        true,
                    )
                    .clicked()
                    {
                        self.user.toggle_favorite(&qid);
                        self.persist_user();
                        self.dirty = true;
                    }
                });
            });
            if self.note_open || self.user.note(&qid).is_some() {
                let note = ui.add(
                    egui::TextEdit::multiline(&mut self.note_draft)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .margin(tokens.text_input_margin())
                        .hint_text("写点备注…（失焦自动保存）"),
                );
                if note.lost_focus() {
                    self.flush_note();
                }
            }

            ui.add_space(tokens.space.lg);

            // Question change: the stem and everything below it arrive with
            // a short fade. Opacity only affects painting — every control is
            // live immediately, and the scroll reset above is untouched.
            ui.multiply_opacity(q_fade);

            ui.label(
                RichText::new(q.stem.trim())
                    .size(22.0)
                    .strong()
                    .line_height(Some(34.0)),
            );
            // 刷题信息：one muted line right after the title — 看过 / 作答 /
            // 正确. Shown only once the question has actually been seen (a
            // fresh question shows nothing, so it never prints 看过 0 次). It is
            // a line of its own: the title's line height and the options'
            // geometry below are untouched.
            if let Some(stat) = self.user.stat(&qid).filter(|s| s.seen > 0) {
                ui.add_space(tokens.space.xs);
                ui.label(
                    RichText::new(format!(
                        "看过 {} 次 · 作答 {} 次 · 正确 {} 次",
                        stat.seen, stat.attempts, stat.correct
                    ))
                    .size(12.5)
                    .color(tokens.palette.ink.muted),
                );
            }
            ui.add_space(tokens.space.lg);

            // A revealed answer (submitted, or 显示答案) is no longer
            // interactive: grading after seeing the answer is meaningless.
            let interactive = q.is_choice() && !revealed;
            if interactive && shake.abs() > 0.01 {
                let rect = glass::widgets::shifted_block(
                    ui.next_widget_position(),
                    ui.max_rect().max,
                    shake,
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    self.render_options(ui, &q, reveal);
                });
            } else if q.is_choice() {
                self.render_options(ui, &q, reveal);
            } else {
                self.render_memory(ui, &q, &mut do_assess);
            }

            if q.is_choice() {
                if revealed {
                    ui.scope(|ui| {
                        ui.set_opacity(reveal);
                        self.render_explanation(ui, &q);
                    });
                }
            } else if q.answer_text.is_some() || q.explanation.is_some() {
                self.render_explanation(ui, &q);
            }

            ui.add_space(tokens.space.lg);
            let (rule, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), 1.0),
                egui::Sense::hover(),
            );
            ui.painter()
                .rect_filled(rule, 0.0, tokens.palette.glass.divider);
            ui.add_space(tokens.space.sm);
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("来源/出处：")
                        .size(12.5)
                        .color(tokens.palette.ink.secondary),
                );
                match &q.source {
                    Some(src) => {
                        // `provenance_job` — not a plain RichText: see its doc.
                        ui.label(provenance_job(
                            &src.provenance(),
                            tokens.palette.ink.muted,
                        ));
                        if let Some(u) = src.url.as_ref().filter(|s| !s.is_empty()) {
                            ui.hyperlink_to("原文", u);
                        }
                    }
                    None => {
                        ui.label(
                            RichText::new("（无出处信息）")
                                .size(12.5)
                                .color(tokens.palette.ink.muted),
                        );
                    }
                }
            });
            ui.add_space(tokens.space.lg);
                            });
                        });
                    });
            },
        );

        ui.add_space(tokens.space.xs);
        let (rule, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 1.0),
            egui::Sense::hover(),
        );
        ui.painter()
            .rect_filled(rule, 0.0, tokens.palette.glass.divider);
        ui.add_space(tokens.space.sm);

        let full_w = ui.available_width();
        let col_w = (full_w - 96.0).clamp(320.0, 780.0);
        let pad = ((full_w - col_w) * 0.5).max(0.0);
        ui.horizontal(|ui| {
            ui.add_space(pad);
            ui.allocate_ui_with_layout(
                egui::vec2(col_w, 54.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if glass::widgets::button(
                        ui,
                        &tokens,
                        "prev_q",
                        RichText::new("◀ 上一题").size(14.0),
                        egui::vec2(96.0, 34.0),
                        total > 1,
                    )
                    .clicked()
                    {
                        do_prev = true;
                    }
                    if glass::widgets::button(
                        ui,
                        &tokens,
                        "next_q",
                        RichText::new("下一题 ▶").size(14.0),
                        egui::vec2(96.0, 34.0),
                        // 下一题 waits until the current question is judged
                        // (提交答案 / 显示答案 / 自评). `←` and `Esc` stay
                        // available, so the gate is never a trap.
                        can_next,
                    )
                    .clicked()
                    {
                        do_next = true;
                    }
                    ui.add_space(tokens.space.sm);
                    ui.label(
                        RichText::new(if q.is_choice() {
                            "◀ 上一题 · Enter 提交 · 判定或显示答案后下一题"
                        } else {
                            "◀ 上一题 · 自评后才能下一题"
                        })
                        .size(11.5)
                        .color(tokens.palette.ink.muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if q.is_choice() {
                            if submitted {
                                let sel = self
                                    .session
                                    .as_ref()
                                    .map(|s| s.selection(q.is_multi()))
                                    .unwrap_or_default();
                                if q.grade(&sel) {
                                    ui.label(
                                        RichText::new("回答正确")
                                            .size(14.5)
                                            .strong()
                                            .color(
                                                tokens
                                                    .palette
                                                    .accent
                                                    .success
                                                    .gamma_multiply(reveal),
                                            ),
                                    );
                                } else {
                                    ui.label(
                                        RichText::new(logic::wrong_verdict(&q))
                                            .size(13.5)
                                            .strong()
                                            .color(
                                                tokens
                                                    .palette
                                                    .accent
                                                    .danger
                                                    .gamma_multiply(reveal),
                                            ),
                                    );
                                }
                            } else if answer_shown {
                                ui.label(
                                    RichText::new("已显示答案 · 未作答")
                                        .size(13.0)
                                        .color(tokens.palette.ink.muted),
                                );
                            } else {
                                let can = self
                                    .session
                                    .as_ref()
                                    .map(|s| !s.selection(q.is_multi()).is_empty())
                                    .unwrap_or(false);
                                if glass::widgets::primary_button(ui, &tokens, "提交答案", can)
                                    .clicked()
                                {
                                    do_submit = true;
                                }
                            }

                            // 显示答案 sits outside the branch above on purpose: it
                            // stays available whether or not the question has been
                            // submitted, so the answer can be looked at at any point.
                            // It reveals the correct options and the explanation
                            // without grading; once the answer is already on screen it
                            // only re-arms the reveal animation. It is disabled — with
                            // a hover saying why — only when the data has neither an
                            // answer nor an explanation, so no empty reveal can open.
                            ui.add_space(tokens.space.sm);
                            let revealable = logic::can_reveal(&q);
                            let reveal_btn = glass::widgets::button(
                                ui,
                                &tokens,
                                "reveal_answer",
                                RichText::new("显示答案").size(14.5),
                                egui::vec2(112.0, 38.0),
                                revealable,
                            );
                            if revealable {
                                if reveal_btn.clicked() {
                                    do_reveal = true;
                                }
                            } else {
                                reveal_btn.on_disabled_hover_text("这道题在数据里没有答案");
                            }
                        }
                    });
                },
            );
        });

        if do_submit {
            self.submit(now);
        }
        if do_reveal {
            self.reveal_answer();
        }
        if let Some(remembered) = do_assess {
            self.self_assess(remembered, now);
        }
        if do_next {
            self.next_question();
        }
        if do_prev {
            self.prev_question();
        }
    }

    fn render_options(&mut self, ui: &mut egui::Ui, q: &Question, reveal: f32) {
        let tokens = self.tokens;
        let is_multi = q.is_multi();
        // A revealed answer — submitted, or 显示答案 without grading — uses the
        // same look: correct options highlighted, the picked ones marked, and
        // the rows no longer clickable.
        let revealed = self
            .session
            .as_ref()
            .map(|s| s.submitted || s.shown)
            .unwrap_or(false);
        let correct: Vec<String> = q
            .answer
            .iter()
            .map(|s| s.trim().to_ascii_uppercase())
            .collect();

        fn alpha(color: egui::Color32, a: u8) -> egui::Color32 {
            egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a)
        }

        for o in &q.options {
            let selected = self
                .session
                .as_ref()
                .map(|s| {
                    if is_multi {
                        s.multi.iter().any(|x| x == &o.label)
                    } else {
                        s.single.as_deref() == Some(o.label.as_str())
                    }
                })
                .unwrap_or(false);
            let is_correct = correct.iter().any(|c| c.eq_ignore_ascii_case(&o.label));

            let text_color = if revealed {
                let target = if is_correct {
                    tokens.palette.accent.success
                } else if selected {
                    tokens.palette.accent.danger
                } else {
                    tokens.palette.ink.muted
                };
                mix(tokens.palette.ink.primary, target, reveal)
            } else {
                tokens.palette.ink.primary
            };
            let wrap_w = (ui.available_width()
                - 52.0
                - if revealed { 104.0 } else { 40.0 })
            .max(120.0);
            let galley = ui.painter().layout(
                o.text.clone(),
                egui::FontId::proportional(15.5),
                text_color,
                wrap_w,
            );
            let row_h = (galley.size().y + 26.0).max(50.0);
            let sense = if revealed {
                egui::Sense::hover()
            } else {
                egui::Sense::CLICK
            };
            let (rect, resp) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), sense);
            let hovered = !revealed && resp.hovered();

            let hover = motion::ease_bool(
                ui.ctx(),
                resp.id.with("hover"),
                hovered,
                motion::duration::FAST,
                motion::curve::ENTER,
            );
            let sel = motion::ease_bool(
                ui.ctx(),
                resp.id.with("selected"),
                selected,
                motion::duration::BASE,
                motion::curve::STATE,
            );

            let rest = tokens.palette.glass.fill;
            let pre_fill = {
                let hov = mix(rest, tokens.palette.glass.fill_strong, hover);
                mix(hov, tokens.palette.accent.soft, sel)
            };
            let pre_border = mix(tokens.palette.ink.muted, tokens.palette.accent.base, sel);

            let fill = if revealed {
                let target = if is_correct {
                    alpha(tokens.palette.accent.success, 30)
                } else if selected {
                    alpha(tokens.palette.accent.danger, 30)
                } else {
                    tokens.palette.glass.fill
                };
                mix(pre_fill, target, reveal)
            } else {
                pre_fill
            };
            let border = if revealed {
                let target = if is_correct {
                    tokens.palette.accent.success
                } else if selected {
                    tokens.palette.accent.danger
                } else {
                    tokens.palette.ink.muted
                };
                mix(pre_border, target, reveal)
            } else {
                pre_border
            };

            {
                let painter = ui.painter();
                let radius = egui::CornerRadius::same(5);
                painter.rect_filled(rect, radius, fill);
                painter.rect_stroke(
                    rect,
                    radius,
                    egui::Stroke::new(1.0, tokens.palette.glass.stroke),
                    egui::StrokeKind::Inside,
                );

                let badge = egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 27.0, rect.center().y),
                    egui::vec2(26.0, 26.0),
                );
                let badge_radius = if is_multi {
                    egui::CornerRadius::same(5)
                } else {
                    egui::CornerRadius::same(13)
                };
                if sel > 0.001 {
                    painter.rect_filled(badge, badge_radius, alpha(border, (34.0 * sel) as u8));
                }
                if is_multi {
                    painter.rect_stroke(
                        badge,
                        badge_radius,
                        egui::Stroke::new(1.0, border),
                        egui::StrokeKind::Inside,
                    );
                } else {
                    painter.circle_stroke(
                        badge.center(),
                        13.0,
                        egui::Stroke::new(1.0, border),
                    );
                }
                painter.text(
                    badge.center(),
                    egui::Align2::CENTER_CENTER,
                    &o.label,
                    egui::FontId::proportional(11.5),
                    border,
                );

                painter.galley(
                    egui::pos2(rect.left() + 52.0, rect.top() + 13.0),
                    galley,
                    text_color,
                );

                if revealed {
                    let tag = if is_correct {
                        Some(("正确答案", tokens.palette.accent.success))
                    } else if selected {
                        Some(("你的选择", tokens.palette.accent.danger))
                    } else {
                        None
                    };
                    if let Some((label, color)) = tag {
                        painter.text(
                            egui::pos2(rect.right() - 16.0, rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            label,
                            egui::FontId::proportional(11.5),
                            color.gamma_multiply(reveal),
                        );
                    }
                } else if sel > 0.001 {
                    painter.circle_filled(
                        egui::pos2(rect.right() - 20.0, rect.center().y),
                        4.0,
                        tokens.palette.accent.base.gamma_multiply(sel),
                    );
                }
            }

            if !revealed && resp.clicked() {
                let mut changed = false;
                if let Some(s) = self.session.as_mut() {
                    if is_multi {
                        if selected {
                            s.multi.retain(|x| x != &o.label);
                        } else {
                            s.multi.push(o.label.clone());
                        }
                        changed = true;
                    } else if s.single.as_deref() != Some(o.label.as_str()) {
                        s.single = Some(o.label.clone());
                        changed = true;
                    }
                }
                if changed {
                    self.dirty = true;
                }
            }
            if !revealed {
                resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            }
            ui.add_space(6.0);
        }
    }

    fn render_memory(&mut self, ui: &mut egui::Ui, q: &Question, assess: &mut Option<bool>) {
        let tokens = self.tokens;
        let shown = self.session.as_ref().map(|s| s.shown).unwrap_or(false);
        ui.add_space(tokens.space.sm);
        if !shown {
            ui.label(
                RichText::new("先自己回想，再核对答案。")
                    .size(13.0)
                    .color(tokens.palette.ink.secondary),
            );
            ui.add_space(tokens.space.sm);
            if glass::widgets::primary_button(ui, &tokens, "显示答案", true).clicked() {
                if let Some(s) = self.session.as_mut() {
                    s.reveal();
                }
                self.dirty = true;
            }
        } else {
            if let Some(at) = q.answer_text.as_ref().filter(|s| !s.trim().is_empty()) {
                ui.label(RichText::new("参考答案").size(14.5).strong());
                ui.label(RichText::new(at.trim()).size(15.0));
            }
            ui.add_space(tokens.space.md);
            ui.horizontal(|ui| {
                if glass::widgets::primary_button(ui, &tokens, "记住了", true).clicked() {
                    *assess = Some(true);
                }
                if glass::widgets::button(
                    ui,
                    &tokens,
                    "forgot",
                    RichText::new("没记住").size(14.0),
                    egui::vec2(0.0, 0.0),
                    true,
                )
                .clicked()
                {
                    *assess = Some(false);
                }
            });
        }
    }

    fn render_explanation(&mut self, ui: &mut egui::Ui, q: &Question) {
        // Nothing to say → draw nothing: an "empty explanation box" is noise,
        // so the whole section (rule + header + placeholder) is skipped.
        let Some(exp) = q.explanation.as_ref().filter(|s| !s.trim().is_empty()) else {
            return;
        };
        let tokens = self.tokens;
        ui.add_space(tokens.space.md);
        let (rule, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 1.0),
            egui::Sense::hover(),
        );
        ui.painter()
            .rect_filled(rule, 0.0, tokens.palette.glass.divider);
        ui.add_space(tokens.space.sm);
        ui.label(RichText::new("解析").size(15.0).strong());
        CommonMarkViewer::new().show(ui, &mut self.md_cache, exp);
    }
}

fn small_button(
    ui: &mut egui::Ui,
    tokens: &theme::Tokens,
    salt: &str,
    label: &str,
) -> egui::Response {
    glass::widgets::button_sized(
        ui,
        tokens,
        ("small", salt),
        RichText::new(label).size(11.5),
        egui::vec2(46.0, 22.0),
    )
}

/// The 来源/出处 provenance, laid out with `break_anywhere`.
///
/// A path or a URL is one long unbreakable token, and the default line
/// breaking prefers the last *word boundary* (a space) over filling the row:
/// the whole token then drops onto the next row and the current one ends half
/// empty — visibly narrower than the block it sits in. With `break_anywhere`
/// the row fills up to the column's right edge instead, the same width the
/// hairline rule and the options get. `Label` keeps this flag and supplies the
/// wrapping-row indentation (past 「来源/出处：」) and the wrap width itself, so
/// this is the whole override.
fn provenance_job(text: &str, color: egui::Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(12.5),
            color,
            ..Default::default()
        },
    );
    job.wrap.break_anywhere = true;
    job
}

#[cfg(test)]
mod tests {
    use super::provenance_job;

    /// Lay the same text out twice inside one frame (fonts need a frame to
    /// exist) and report the first row's width for both settings.
    fn first_row_widths(text: &str, wrap_width: f32) -> (f32, f32) {
        let ctx = egui::Context::default();
        let mut default_breaking = provenance_job(text, egui::Color32::WHITE);
        default_breaking.wrap.break_anywhere = false;
        default_breaking.wrap.max_width = wrap_width;
        let mut filling = provenance_job(text, egui::Color32::WHITE);
        filling.wrap.max_width = wrap_width;

        let mut widths = (0.0, 0.0);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            widths = ui.fonts_mut(|fonts| {
                (
                    fonts.layout_job(default_breaking.clone()).rows[0].rect().width(),
                    fonts.layout_job(filling.clone()).rows[0].rect().width(),
                )
            });
        });
        // A headless frame produces font texture deltas; this test does not
        // render, so they are cleared on purpose (the same reason the known
        // headless-render tests panic if they don't).
        output.textures_delta.clear();
        widths
    }

    /// The real cause of the half-empty 来源/出处 row, pinned: with the default
    /// line breaking the first row stops at the last word boundary before the
    /// long path / URL (a big gap is left); with `break_anywhere` the same
    /// text fills the row up to the wrap width.
    #[test]
    fn a_long_provenance_token_fills_the_row_instead_of_ending_it_early() {
        let text = "Chinese-Bar-Exam-AI-Mindmap-Tool · user-data/exam/long-file-name.json · \
                    https://github.com/civilwizard/Chinese-Bar-Exam-AI-Mindmap-Tool";
        let wrap_width = 780.0;

        let (early, filled) = first_row_widths(text, wrap_width);

        assert!(
            early < wrap_width - 100.0,
            "without break_anywhere the first row ends early ({early} of {wrap_width})"
        );
        assert!(
            filled > wrap_width - 20.0,
            "with break_anywhere the first row fills the width ({filled} of {wrap_width})"
        );
    }

    /// A short provenance must not change at all: `break_anywhere` only ever
    /// matters when a token does not fit.
    #[test]
    fn a_short_provenance_is_laid_out_exactly_as_before() {
        let text = "repo · file.md";
        let wrap_width = 780.0;
        let (early, filled) = first_row_widths(text, wrap_width);
        assert_eq!(early, filled, "a fitting row is untouched");
        assert!(early < wrap_width, "and it is its natural width");
    }

    /// The integration path, exactly as the session view uses it: inside the
    /// wrapping row, after 「来源/出处：」, the provenance label must reach the
    /// column's right edge — the same width the hairline rule and the options
    /// get. This is the regression guard for the half-empty block the user saw.
    #[test]
    fn the_provenance_label_fills_its_row_in_the_session_layout() {
        let ctx = egui::Context::default();
        let text = "Chinese-Bar-Exam-AI-Mindmap-Tool · user-data/exam/long-file-name.json · \
                    https://github.com/civilwizard/Chinese-Bar-Exam-AI-Mindmap-Tool";
        let mut child_max = egui::Rect::NOTHING;
        let mut label_rect = egui::Rect::NOTHING;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_max_width(780.0);
            ui.horizontal_wrapped(|ui| {
                child_max = ui.max_rect();
                ui.label(egui::RichText::new("来源/出处：").size(12.5));
                label_rect = ui.label(provenance_job(text, egui::Color32::WHITE)).rect;
            });
        });
        output.textures_delta.clear();
        assert!(
            label_rect.right() >= child_max.right() - 20.0,
            "the provenance must fill the row: label {label_rect:?} in column {child_max:?}"
        );
    }
}
