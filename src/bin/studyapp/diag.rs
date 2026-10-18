//! Diagnostics sink for a windowed binary.
//!
//! The GUI is built with the Windows subsystem, so it has no console and
//! `eprintln!` is discarded. Warnings and the `--probe` report are appended to
//! `<state_dir>/log.txt` so a failure is still recoverable after the fact.

use std::path::Path;

/// Keeps the log from growing without bound when a write failure repeats.
const MAX_BYTES: u64 = 256 * 1024;

/// Append one timestamped line to `<state_dir>/log.txt`.
///
/// Best-effort by design: a diagnostic must never be the reason the app fails.
pub fn log(state_dir: &Path, message: &str) {
    use std::io::Write;

    let path = state_dir.join("log.txt");
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_BYTES {
        let _ = std::fs::remove_file(&path);
    }
    let _ = std::fs::create_dir_all(state_dir);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{} {message}", sfzklearning::now_iso());
    }
}
