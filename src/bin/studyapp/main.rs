//! `sfzklearning` — native Windows desktop GUI (eframe/egui, no web view).
//!
//! Information architecture:
//! * **Workspace** — a mode rail on the left grouping subjects by their official
//!   group (卷一/卷二, 专业阶段/综合阶段), and the practice area. The header's mode
//!   button toggles the exam mode (法考 / 注会) in place; there is no launcher.
//! * **做题** is set up with filters first, then runs as a focused session.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod anim;
mod app;
mod diag;
mod glass;
mod logic;
mod theme;
mod win;

fn main() -> eframe::Result<()> {
    app::run()
}
