use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

fn profile_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        env::var("MARKDOWN_EDITOR_PROFILE")
            .ok()
            .map(|value| {
                let value = value.trim();
                !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
            })
            .unwrap_or(false)
    })
}

fn profile_min_duration() -> Duration {
    static MIN_MS: OnceLock<u64> = OnceLock::new();
    Duration::from_millis(*MIN_MS.get_or_init(|| {
        env::var("MARKDOWN_EDITOR_PROFILE_MIN_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(12)
    }))
}

fn profile_log_path() -> &'static PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        env::var("MARKDOWN_EDITOR_PROFILE_LOG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| env::temp_dir().join("markdown-editor-profile.log"))
    })
}

fn profile_log_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn write_profile_line(line: &str) {
    eprintln!("{line}");

    if let Ok(_guard) = profile_log_lock().lock()
        && let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(profile_log_path())
    {
        let _ = writeln!(file, "{line}");
    }
}

pub(crate) fn enabled() -> bool {
    profile_enabled()
}

pub(crate) fn log_event(label: &'static str, details: impl FnOnce() -> String) {
    if !profile_enabled() {
        return;
    }

    write_profile_line(&format!("[profile] {label} {}", details()));
}

pub(crate) struct ProfileScope {
    label: &'static str,
    start: Option<Instant>,
    threshold: Duration,
    note: Option<String>,
}

impl ProfileScope {
    pub(crate) fn new(label: &'static str) -> Self {
        Self::with_threshold(label, profile_min_duration())
    }

    pub(crate) fn with_threshold(label: &'static str, threshold: Duration) -> Self {
        Self {
            label,
            start: profile_enabled().then(Instant::now),
            threshold,
            note: None,
        }
    }

    pub(crate) fn set_note(&mut self, note: impl Into<String>) {
        if self.start.is_some() {
            self.note = Some(note.into());
        }
    }
}

impl Drop for ProfileScope {
    fn drop(&mut self) {
        let Some(start) = self.start else {
            return;
        };

        let elapsed = start.elapsed();
        if elapsed < self.threshold {
            return;
        }

        let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
        let suffix = self
            .note
            .as_deref()
            .map(|note| format!(" {note}"))
            .unwrap_or_default();
        write_profile_line(&format!(
            "[profile] {} took {:.2}ms{}",
            self.label, elapsed_ms, suffix
        ));
    }
}
