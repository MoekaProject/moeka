use std::env;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as AnyhowContext, Result, anyhow, bail};
use chrono::Local;
use serde::Serialize;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use super::config_paths;

const DEFAULT_SAMPLE_INTERVAL_MS: u64 = 500;
const DEFAULT_CPU_FREQUENCY_HZ: i32 = 99;
const MIN_SAMPLE_INTERVAL_MS: u64 = 100;
const PROFILE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub(crate) struct RuntimeProfilePaths {
    pub(crate) json: PathBuf,
    pub(crate) flamegraph: Option<PathBuf>,
}

impl RuntimeProfilePaths {
    pub(crate) fn message(&self) -> String {
        let mut message = format!("JSON timeline:\n{}", self.json.display());
        if let Some(path) = &self.flamegraph {
            message.push_str(&format!("\n\nCPU flamegraph:\n{}", path.display()));
        }
        message
    }
}

pub(crate) struct RuntimeProfiler {
    session_id: String,
    output_dir: PathBuf,
    started_at: Instant,
    started_at_unix_ms: u64,
    sample_interval: Duration,
    cpu_frequency_hz: i32,
    cpu_profiler_status: CpuProfilerStatus,
    samples: Arc<Mutex<Vec<ResourceSample>>>,
    events: Vec<ProfileEvent>,
    sampler_errors: Arc<Mutex<Vec<String>>>,
    stop_sampler: Arc<AtomicBool>,
    sampler: Option<JoinHandle<()>>,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    cpu_guard: Option<pprof::ProfilerGuard<'static>>,
}

impl RuntimeProfiler {
    pub(crate) fn start_from_env() -> Result<Option<Self>> {
        if !env_bool("MOEKA_PROFILE", false)? {
            return Ok(None);
        }

        Self::start("env").map(Some)
    }

    pub(crate) fn start_manual() -> Result<Self> {
        Self::start("manual")
    }

    pub(crate) fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    pub(crate) fn write_snapshot_with_diagnostics(
        &mut self,
        reason: &str,
        diagnostics: Option<RuntimeProfileDiagnostics>,
    ) -> Result<RuntimeProfilePaths> {
        self.record_event(reason);
        self.write_files(reason, diagnostics)
    }

    pub(crate) fn stop_with_diagnostics(
        mut self,
        reason: &str,
        diagnostics: Option<RuntimeProfileDiagnostics>,
    ) -> Result<RuntimeProfilePaths> {
        self.record_event(reason);
        self.stop_sampler.store(true, Ordering::Relaxed);
        if let Some(sampler) = self.sampler.take() {
            sampler
                .join()
                .map_err(|_| anyhow!("runtime profiler sampler thread panicked"))?;
        }
        self.write_files(reason, diagnostics)
    }

    fn start(started_by: &str) -> Result<Self> {
        let config = ProfileConfig::from_env()?;
        let session_id = format!(
            "{}-p{}",
            Local::now().format("%Y%m%d-%H%M%S"),
            std::process::id()
        );
        let output_dir = config.output_base_dir.join(&session_id);
        fs::create_dir_all(&output_dir).with_context(|| {
            format!(
                "failed to create profile directory {}",
                output_dir.display()
            )
        })?;

        let samples = Arc::new(Mutex::new(Vec::new()));
        let sampler_errors = Arc::new(Mutex::new(Vec::new()));
        let stop_sampler = Arc::new(AtomicBool::new(false));
        let started_at = Instant::now();
        let started_at_unix_ms = unix_time_ms();
        let sampler = spawn_sampler(
            started_at,
            config.sample_interval,
            Arc::clone(&samples),
            Arc::clone(&sampler_errors),
            Arc::clone(&stop_sampler),
        )?;

        let (cpu_profiler_status, cpu_guard) =
            start_cpu_profiler(config.cpu_enabled, config.cpu_frequency_hz);
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let _ = cpu_guard;
        if let CpuProfilerStatus::Unavailable { error } = &cpu_profiler_status {
            log_error!("Runtime CPU profiler unavailable: {error}");
        }

        let mut profiler = Self {
            session_id,
            output_dir,
            started_at,
            started_at_unix_ms,
            sample_interval: config.sample_interval,
            cpu_frequency_hz: config.cpu_frequency_hz,
            cpu_profiler_status,
            samples,
            events: Vec::new(),
            sampler_errors,
            stop_sampler,
            sampler: Some(sampler),
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            cpu_guard,
        };
        profiler.record_event(&format!("start:{started_by}"));
        Ok(profiler)
    }

    fn record_event(&mut self, label: &str) {
        self.events.push(ProfileEvent {
            elapsed_ms: elapsed_ms(self.started_at),
            timestamp_unix_ms: unix_time_ms(),
            label: label.to_string(),
        });
    }

    fn write_files(
        &self,
        reason: &str,
        diagnostics: Option<RuntimeProfileDiagnostics>,
    ) -> Result<RuntimeProfilePaths> {
        fs::create_dir_all(&self.output_dir).with_context(|| {
            format!(
                "failed to ensure profile directory {}",
                self.output_dir.display()
            )
        })?;

        let file_suffix = format!("{}-{}", safe_file_label(reason), timestamp_file_stem());
        let json_path = self
            .output_dir
            .join(format!("runtime-profile-{file_suffix}.json"));
        self.write_timeline_json(&json_path, reason, diagnostics)?;
        let flamegraph = self.write_cpu_flamegraph(&file_suffix)?;

        Ok(RuntimeProfilePaths {
            json: json_path,
            flamegraph,
        })
    }

    fn write_timeline_json(
        &self,
        path: &Path,
        reason: &str,
        diagnostics: Option<RuntimeProfileDiagnostics>,
    ) -> Result<()> {
        let samples = self
            .samples
            .lock()
            .map_err(|_| anyhow!("runtime profiler samples lock was poisoned"))?
            .clone();
        let sampler_errors = self
            .sampler_errors
            .lock()
            .map_err(|_| anyhow!("runtime profiler errors lock was poisoned"))?
            .clone();
        let summary = ProfileSummary::from_samples(&samples, elapsed_ms(self.started_at));
        let output = RuntimeProfileOutput {
            schema_version: PROFILE_SCHEMA_VERSION,
            app: "moeka",
            session_id: &self.session_id,
            reason,
            pid: std::process::id(),
            started_at_unix_ms: self.started_at_unix_ms,
            written_at_unix_ms: unix_time_ms(),
            duration_ms: elapsed_ms(self.started_at),
            sample_interval_ms: self.sample_interval.as_millis() as u64,
            cpu_frequency_hz: self.cpu_frequency_hz,
            cpu_profiler: &self.cpu_profiler_status,
            samples,
            events: self.events.clone(),
            summary,
            diagnostics,
            sampler_errors,
        };
        let file = File::create(path)
            .with_context(|| format!("failed to create profile JSON {}", path.display()))?;
        serde_json::to_writer_pretty(file, &output)
            .with_context(|| format!("failed to write profile JSON {}", path.display()))
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn write_cpu_flamegraph(&self, file_suffix: &str) -> Result<Option<PathBuf>> {
        let Some(cpu_guard) = &self.cpu_guard else {
            return Ok(None);
        };

        let path = self
            .output_dir
            .join(format!("cpu-flamegraph-{file_suffix}.svg"));
        let report = cpu_guard
            .report()
            .build()
            .context("failed to build CPU profile report")?;
        let file = File::create(&path)
            .with_context(|| format!("failed to create CPU flamegraph {}", path.display()))?;
        report
            .flamegraph(file)
            .with_context(|| format!("failed to write CPU flamegraph {}", path.display()))?;
        Ok(Some(path))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn write_cpu_flamegraph(&self, _file_suffix: &str) -> Result<Option<PathBuf>> {
        Ok(None)
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct RuntimeProfileDiagnostics {
    pub(crate) active_tab_id: Option<String>,
    pub(crate) buffer_bytes: usize,
    pub(crate) buffer_lines: usize,
    pub(crate) decorations_len: usize,
    pub(crate) decorations_estimated_struct_bytes: usize,
    pub(crate) line_display_slots: usize,
    pub(crate) line_display_cached: usize,
    pub(crate) line_display_estimated_bytes: usize,
    pub(crate) tab_layout_cached_tabs: usize,
    pub(crate) tab_layout_cached_slots: usize,
    pub(crate) tab_layout_cached_lines: usize,
    pub(crate) tab_layout_estimated_bytes: usize,
    pub(crate) image_cache_entries: usize,
    pub(crate) image_cache_loaded_entries: usize,
    pub(crate) generated_preview_keys: usize,
    pub(crate) generated_preview_svg_entries: usize,
    pub(crate) generated_preview_svg_bytes: usize,
    pub(crate) generated_preview_overlay_entries: usize,
    pub(crate) generated_preview_view_entries: usize,
    pub(crate) outline_heading_count: usize,
    pub(crate) info_panel_primary_rows: usize,
    pub(crate) info_panel_reference_items: usize,
    pub(crate) sidebar_items: usize,
    pub(crate) file_list_items: usize,
    pub(crate) open_tabs: usize,
}

#[derive(Clone)]
struct ProfileConfig {
    output_base_dir: PathBuf,
    sample_interval: Duration,
    cpu_enabled: bool,
    cpu_frequency_hz: i32,
}

impl ProfileConfig {
    fn from_env() -> Result<Self> {
        let sample_interval_ms = env_u64(
            "MOEKA_PROFILE_INTERVAL_MS",
            DEFAULT_SAMPLE_INTERVAL_MS,
            MIN_SAMPLE_INTERVAL_MS,
        )?;
        let cpu_frequency_hz = env_i32("MOEKA_PROFILE_CPU_HZ", DEFAULT_CPU_FREQUENCY_HZ, 1)?;
        let output_base_dir = env::var_os("MOEKA_PROFILE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| config_paths::cache_dir().join("profiles"));
        let cpu_enabled = env_bool("MOEKA_PROFILE_CPU", true)?;

        Ok(Self {
            output_base_dir,
            sample_interval: Duration::from_millis(sample_interval_ms),
            cpu_enabled,
            cpu_frequency_hz,
        })
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum CpuProfilerStatus {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    Enabled,
    Disabled,
    Unavailable {
        error: String,
    },
}

#[derive(Clone, Debug, Serialize)]
struct RuntimeProfileOutput<'a> {
    schema_version: u32,
    app: &'a str,
    session_id: &'a str,
    reason: &'a str,
    pid: u32,
    started_at_unix_ms: u64,
    written_at_unix_ms: u64,
    duration_ms: u64,
    sample_interval_ms: u64,
    cpu_frequency_hz: i32,
    cpu_profiler: &'a CpuProfilerStatus,
    samples: Vec<ResourceSample>,
    events: Vec<ProfileEvent>,
    summary: ProfileSummary,
    diagnostics: Option<RuntimeProfileDiagnostics>,
    sampler_errors: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ResourceSample {
    elapsed_ms: u64,
    timestamp_unix_ms: u64,
    cpu_percent: f32,
    rss_bytes: u64,
    virtual_memory_bytes: u64,
    disk_total_read_bytes: u64,
    disk_total_written_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
struct ProfileEvent {
    elapsed_ms: u64,
    timestamp_unix_ms: u64,
    label: String,
}

#[derive(Clone, Debug, Serialize)]
struct ProfileSummary {
    sample_count: usize,
    duration_ms: u64,
    avg_cpu_percent: f32,
    max_cpu_percent: f32,
    start_rss_bytes: u64,
    end_rss_bytes: u64,
    peak_rss_bytes: u64,
    delta_rss_bytes: i64,
    start_virtual_memory_bytes: u64,
    end_virtual_memory_bytes: u64,
    peak_virtual_memory_bytes: u64,
    delta_virtual_memory_bytes: i64,
    total_read_bytes: u64,
    total_written_bytes: u64,
}

impl ProfileSummary {
    fn from_samples(samples: &[ResourceSample], duration_ms: u64) -> Self {
        let sample_count = samples.len();
        let avg_cpu_percent = if sample_count == 0 {
            0.0
        } else {
            samples.iter().map(|sample| sample.cpu_percent).sum::<f32>() / sample_count as f32
        };
        let max_cpu_percent = samples
            .iter()
            .map(|sample| sample.cpu_percent)
            .fold(0.0, f32::max);
        let start_rss_bytes = samples.first().map(|sample| sample.rss_bytes).unwrap_or(0);
        let end_rss_bytes = samples.last().map(|sample| sample.rss_bytes).unwrap_or(0);
        let peak_rss_bytes = samples
            .iter()
            .map(|sample| sample.rss_bytes)
            .max()
            .unwrap_or(0);
        let start_virtual_memory_bytes = samples
            .first()
            .map(|sample| sample.virtual_memory_bytes)
            .unwrap_or(0);
        let end_virtual_memory_bytes = samples
            .last()
            .map(|sample| sample.virtual_memory_bytes)
            .unwrap_or(0);
        let peak_virtual_memory_bytes = samples
            .iter()
            .map(|sample| sample.virtual_memory_bytes)
            .max()
            .unwrap_or(0);
        let first_read_bytes = samples
            .first()
            .map(|sample| sample.disk_total_read_bytes)
            .unwrap_or(0);
        let last_read_bytes = samples
            .last()
            .map(|sample| sample.disk_total_read_bytes)
            .unwrap_or(0);
        let first_written_bytes = samples
            .first()
            .map(|sample| sample.disk_total_written_bytes)
            .unwrap_or(0);
        let last_written_bytes = samples
            .last()
            .map(|sample| sample.disk_total_written_bytes)
            .unwrap_or(0);

        Self {
            sample_count,
            duration_ms,
            avg_cpu_percent,
            max_cpu_percent,
            start_rss_bytes,
            end_rss_bytes,
            peak_rss_bytes,
            delta_rss_bytes: signed_delta(end_rss_bytes, start_rss_bytes),
            start_virtual_memory_bytes,
            end_virtual_memory_bytes,
            peak_virtual_memory_bytes,
            delta_virtual_memory_bytes: signed_delta(
                end_virtual_memory_bytes,
                start_virtual_memory_bytes,
            ),
            total_read_bytes: last_read_bytes.saturating_sub(first_read_bytes),
            total_written_bytes: last_written_bytes.saturating_sub(first_written_bytes),
        }
    }
}

fn spawn_sampler(
    started_at: Instant,
    sample_interval: Duration,
    samples: Arc<Mutex<Vec<ResourceSample>>>,
    sampler_errors: Arc<Mutex<Vec<String>>>,
    stop_sampler: Arc<AtomicBool>,
) -> Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("moeka-runtime-profiler".to_string())
        .spawn(move || {
            if let Err(error) = sample_process_loop(
                started_at,
                sample_interval,
                samples,
                Arc::clone(&sampler_errors),
                stop_sampler,
            ) {
                log_error!("Runtime profiler sampler failed: {error:#}");
                if let Ok(mut errors) = sampler_errors.lock() {
                    errors.push(format!("{error:#}"));
                }
            }
        })
        .context("failed to spawn runtime profiler sampler thread")
}

fn sample_process_loop(
    started_at: Instant,
    sample_interval: Duration,
    samples: Arc<Mutex<Vec<ResourceSample>>>,
    _sampler_errors: Arc<Mutex<Vec<String>>>,
    stop_sampler: Arc<AtomicBool>,
) -> Result<()> {
    let pid = sysinfo::get_current_pid().map_err(|message| anyhow!(message))?;
    let refresh_kind = ProcessRefreshKind::new()
        .with_cpu()
        .with_memory()
        .with_disk_usage();
    let mut system = System::new();

    loop {
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), refresh_kind);
        if let Some(process) = system.process(pid) {
            let disk = process.disk_usage();
            let sample = ResourceSample {
                elapsed_ms: elapsed_ms(started_at),
                timestamp_unix_ms: unix_time_ms(),
                cpu_percent: process.cpu_usage(),
                rss_bytes: process.memory(),
                virtual_memory_bytes: process.virtual_memory(),
                disk_total_read_bytes: disk.total_read_bytes,
                disk_total_written_bytes: disk.total_written_bytes,
            };
            samples
                .lock()
                .map_err(|_| anyhow!("runtime profiler samples lock was poisoned"))?
                .push(sample);
        }

        if stop_sampler.load(Ordering::Relaxed) {
            break;
        }
        sleep_until_next_sample(sample_interval, &stop_sampler);
    }

    Ok(())
}

fn sleep_until_next_sample(sample_interval: Duration, stop_sampler: &AtomicBool) {
    let started = Instant::now();
    while !stop_sampler.load(Ordering::Relaxed) {
        let elapsed = started.elapsed();
        if elapsed >= sample_interval {
            break;
        }
        thread::sleep((sample_interval - elapsed).min(Duration::from_millis(50)));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn start_cpu_profiler(
    enabled: bool,
    frequency_hz: i32,
) -> (CpuProfilerStatus, Option<pprof::ProfilerGuard<'static>>) {
    if !enabled {
        return (CpuProfilerStatus::Disabled, None);
    }

    match pprof::ProfilerGuard::new(frequency_hz) {
        Ok(guard) => (CpuProfilerStatus::Enabled, Some(guard)),
        Err(error) => (
            CpuProfilerStatus::Unavailable {
                error: error.to_string(),
            },
            None,
        ),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn start_cpu_profiler(enabled: bool, _frequency_hz: i32) -> (CpuProfilerStatus, ()) {
    if enabled {
        (
            CpuProfilerStatus::Unavailable {
                error: "CPU flamegraph profiling is only supported on Linux and macOS".to_string(),
            },
            (),
        )
    } else {
        (CpuProfilerStatus::Disabled, ())
    }
}

fn env_bool(key: &str, default: bool) -> Result<bool> {
    let Some(value) = env::var_os(key) else {
        return Ok(default);
    };
    let value = value.to_string_lossy();
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => bail!("{key} must be one of 1/0, true/false, yes/no, or on/off"),
    }
}

fn env_u64(key: &str, default: u64, min: u64) -> Result<u64> {
    let Some(value) = env::var_os(key) else {
        return Ok(default);
    };
    let parsed = value
        .to_string_lossy()
        .parse::<u64>()
        .with_context(|| format!("{key} must be an integer"))?;
    if parsed < min {
        bail!("{key} must be at least {min}");
    }
    Ok(parsed)
}

fn env_i32(key: &str, default: i32, min: i32) -> Result<i32> {
    let Some(value) = env::var_os(key) else {
        return Ok(default);
    };
    let parsed = value
        .to_string_lossy()
        .parse::<i32>()
        .with_context(|| format!("{key} must be an integer"))?;
    if parsed < min {
        bail!("{key} must be at least {min}");
    }
    Ok(parsed)
}

fn timestamp_file_stem() -> String {
    Local::now().format("%Y%m%d-%H%M%S").to_string()
}

fn safe_file_label(label: &str) -> String {
    let sanitized: String = label
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    sanitized.trim_matches('-').to_string()
}

fn elapsed_ms(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn signed_delta(end: u64, start: u64) -> i64 {
    if end >= start {
        i64::try_from(end - start).unwrap_or(i64::MAX)
    } else {
        -i64::try_from(start - end).unwrap_or(i64::MAX)
    }
}
