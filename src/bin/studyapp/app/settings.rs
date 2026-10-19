use super::*;

use crate::anim::motion;

impl App {
    pub(super) fn settings_modal(&mut self, ctx: &egui::Context) {
        let tokens = self.tokens;
        // Entrance: backdrop, surface and content all fade in together over
        // the slow tier. Opacity is paint-only, so the modal is interactive
        // from its first frame.
        let enter = motion::change_progress(
            ctx,
            egui::Id::new("settings_enter"),
            self.settings_epoch,
            motion::duration::SLOW,
            motion::curve::ENTER,
        );
        // Popup rule for this design: opaque fill, hairline edge, no shadow —
        // the same one `Frame::popup` encodes, applied to this surface too.
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

        // A definite size gives the footer bar (`Panel::bottom`) bounded space
        // to dock into; an auto-sized modal would dock it at the screen edge.
        let area = egui::Modal::default_area(egui::Id::new("settings_modal"))
            .default_size(egui::vec2(560.0, 460.0));

        let resp = egui::Modal::new(egui::Id::new("settings_modal"))
            .area(area)
            .backdrop_color(glass::paint::with_alpha(
                tokens.palette.background.scrim,
                0.45 * enter,
            ))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_opacity(enter);
                ui.label(RichText::new("设置").size(17.0).strong());
                ui.add_space(tokens.space.sm);
                self.settings_tabs(ui);
                ui.add_space(tokens.space.md);
                ui.separator();
                ui.add_space(tokens.space.md);
                match self.settings_tab {
                    0 => self.settings_appearance(ui),
                    _ => self.settings_user(ui),
                }

                egui::Panel::bottom("settings_footer")
                    .show_separator_line(false)
                    .frame(glass::frame(&tokens))
                    .show(ui, |ui| {
                        glass::divider_top(ui, &tokens);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if glass::widgets::button(
                                ui,
                                &tokens,
                                "close_settings",
                                RichText::new("关闭").size(13.0),
                                egui::vec2(64.0, 30.0),
                                true,
                            )
                            .on_hover_text("关闭设置")
                            .clicked()
                            {
                                self.settings_open = false;
                            }
                        });
                    });
            });

        if resp.should_close() {
            self.settings_open = false;
        }
    }

    fn settings_tabs(&mut self, ui: &mut egui::Ui) {
        let tokens = self.tokens;
        if let Some(i) = glass::widgets::segmented(
            ui,
            &tokens,
            "settings_tabs",
            &["外观", "用户选项"],
            self.settings_tab,
        ) {
            self.settings_tab = i;
        }
    }

    fn settings_appearance(&mut self, ui: &mut egui::Ui) {
        let tokens = self.tokens;
        ui.label(
            RichText::new("主题")
                .size(13.0)
                .color(tokens.palette.ink.secondary),
        );
        ui.add_space(tokens.space.xs);
        let sel = usize::from(self.mode == theme::Mode::Light);
        if let Some(i) =
            glass::widgets::segmented(ui, &tokens, "settings_theme", &["深色", "浅色"], sel)
        {
            let mode = if i == 0 {
                theme::Mode::Dark
            } else {
                theme::Mode::Light
            };
            self.set_mode(ui.ctx(), mode);
        }
        ui.add_space(tokens.space.sm);
        ui.label(
            RichText::new("立即生效，并写入本地用户配置。")
                .size(11.5)
                .color(tokens.palette.ink.muted),
        );

        ui.add_space(tokens.space.lg);
        ui.label(
            RichText::new("毛玻璃")
                .size(13.0)
                .color(tokens.palette.ink.secondary),
        );
        ui.add_space(tokens.space.xs);

        let label_w = egui::vec2(64.0, 30.0);
        let mut changed = false;
        let mut blur_changed = false;
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                label_w,
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(
                        RichText::new("透明度")
                            .size(12.5)
                            .color(tokens.palette.ink.muted),
                    );
                },
            );
            let before = self.user.glass_transparency;
            glass::widgets::slider(
                ui,
                &tokens,
                "glass_transparency",
                &mut self.user.glass_transparency,
            );
            changed |= self.user.glass_transparency != before;
        });
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                label_w,
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(
                        RichText::new("模糊度")
                            .size(12.5)
                            .color(tokens.palette.ink.muted),
                    );
                },
            );
            let before = self.user.glass_blur;
            glass::widgets::slider(ui, &tokens, "glass_blur", &mut self.user.glass_blur);
            blur_changed = self.user.glass_blur != before;
            // 重新采样背景 — the one deliberate capture. A move / resize no
            // longer re-photographs the desktop (the stored shot follows the
            // window), so this is the refresh for when the desktop *behind*
            // the window has actually changed.
            if glass::widgets::button(
                ui,
                &tokens,
                "glass_resample",
                RichText::new("重新采样背景").size(12.5),
                egui::vec2(112.0, 26.0),
                true,
            )
            .on_hover_text(
                "重新截取一次桌面背景并重新模糊。移动 / 缩放窗口不会重截，只有桌面内容变了才需要手动刷新。",
            )
            .clicked()
            {
                self.glass_recapture = true;
                ui.ctx().request_repaint();
            }
        });
        if changed || blur_changed {
            self.persist_user();
        }
        if blur_changed {
            // `refresh_glass` runs before the views, so make sure the next
            // frame actually comes and re-blurs the stored capture with the
            // new 模糊度 (no new screen capture).
            ui.ctx().request_repaint();
        }
        ui.add_space(tokens.space.xs);
        ui.label(
            RichText::new("透明度越低越实心；模糊度 0 保持桌面清晰。立即生效，并写入本地用户配置。")
                .size(11.5)
                .color(tokens.palette.ink.muted),
        );
    }

    fn settings_user(&self, ui: &mut egui::Ui) {
        let tokens = self.tokens;
        ui.label(
            RichText::new("应用与存储位置")
                .size(13.0)
                .color(tokens.palette.ink.secondary),
        );
        ui.add_space(tokens.space.sm);
        egui::Grid::new("settings_locations")
            .num_columns(2)
            .spacing(egui::vec2(tokens.space.lg, tokens.space.sm))
            .show(ui, |ui| {
                let row = |ui: &mut egui::Ui, label: &str, value: String| {
                    ui.label(
                        RichText::new(label)
                            .size(12.5)
                            .color(tokens.palette.ink.muted),
                    );
                    ui.label(
                        RichText::new(value)
                            .size(12.5)
                            .color(tokens.palette.ink.primary),
                    );
                    ui.end_row();
                };
                row(ui, "应用名称", APP_TITLE.to_string());
                row(ui, "版本", env!("CARGO_PKG_VERSION").to_string());
                row(ui, "法考数据", self.law_root.display().to_string());
                row(ui, "注会数据", self.cpa_root.display().to_string());
                row(ui, "状态目录", self.state_dir.display().to_string());
            });
        ui.add_space(tokens.space.sm);
        ui.label(
            RichText::new("路径来自本次启动的实际解析结果，只供查看。")
                .size(11.5)
                .color(tokens.palette.ink.muted),
        );
    }
}
