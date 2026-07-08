use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use gpui::Context;
use serde::Deserialize;

use super::{EditorView, config_paths};

const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/MoekaProject/moeka/releases/latest";
const USER_AGENT: &str = "MoekaUpdater/1.0";

pub(crate) const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const GIT_COMMIT: &str = env!("MOEKA_GIT_COMMIT");
pub(crate) const GIT_TAG: &str = env!("MOEKA_GIT_TAG");

#[derive(Clone, Debug)]
pub(crate) enum UpdateState {
    Idle,
    Checking,
    Ready(PreparedUpdate),
    Failed,
    UpToDate,
}

impl UpdateState {
    pub(crate) fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedUpdate {
    pub version: String,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

impl EditorView {
    pub(crate) fn start_update_check(&mut self, cx: &mut Context<Self>) {
        if self.update_task.is_some() || self.update_state.is_ready() {
            return;
        }

        self.update_state = UpdateState::Checking;
        self.update_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { check_and_download_update() })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.update_task = None;
                this.update_state = match result {
                    Ok(Some(update)) => UpdateState::Ready(update),
                    Ok(None) => UpdateState::UpToDate,
                    Err(error) => {
                        eprintln!("Moeka update check failed: {error}");
                        UpdateState::Failed
                    }
                };
                cx.notify();
            });
        }));
    }

    pub(crate) fn prepared_update(&self) -> Option<&PreparedUpdate> {
        match &self.update_state {
            UpdateState::Ready(update) => Some(update),
            _ => None,
        }
    }

    pub(crate) fn install_prepared_update(&mut self) {
        let Some(update) = self.prepared_update().cloned() else {
            return;
        };
        if let Err(error) = install_update(update) {
            eprintln!("Moeka update install failed: {error}");
            self.update_state = UpdateState::Failed;
        }
    }
}

fn check_and_download_update() -> Result<Option<PreparedUpdate>, String> {
    let release = fetch_latest_release()?;
    if !version_is_newer(&release.tag_name, current_version()) {
        return Ok(None);
    }

    let asset = select_update_asset(&release.assets)
        .ok_or_else(|| format!("no macOS update zip found in release {}", release.tag_name))?;
    let update_dir = config_paths::cache_dir()
        .join("updates")
        .join(safe_file_name(&release.tag_name));
    fs::create_dir_all(&update_dir).map_err(|error| error.to_string())?;
    let out_path = update_dir.join(safe_file_name(&asset.name));
    download_asset(&asset.browser_download_url, &out_path)?;

    Ok(Some(PreparedUpdate {
        version: release.tag_name,
        path: out_path,
    }))
}

fn fetch_latest_release() -> Result<GitHubRelease, String> {
    let mut request = ureq::get(LATEST_RELEASE_URL)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", &user_agent());
    if let Ok(token) = std::env::var("MOEKA_GITHUB_TOKEN")
        && !token.trim().is_empty()
    {
        request = request.set("Authorization", &format!("Bearer {}", token.trim()));
    }
    let text = request
        .call()
        .map_err(|error| error.to_string())?
        .into_string()
        .map_err(|error| error.to_string())?;
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

fn current_version() -> &'static str {
    if GIT_TAG.is_empty() {
        APP_VERSION
    } else {
        GIT_TAG
    }
}

fn version_is_newer(latest: &str, current: &str) -> bool {
    parse_version(latest) > parse_version(current)
}

fn parse_version(value: &str) -> Vec<u64> {
    value
        .trim()
        .trim_start_matches('v')
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

fn select_update_asset(assets: &[GitHubAsset]) -> Option<&GitHubAsset> {
    let preferred = if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        ["macos-arm64.zip", "macos-universal.zip", ".zip"]
    } else {
        ["macos-universal.zip", "macos-x86_64.zip", ".zip"]
    };

    preferred.iter().find_map(|suffix| {
        assets.iter().find(|asset| {
            let name = asset.name.to_ascii_lowercase();
            name.contains("moeka") && name.ends_with(suffix)
        })
    })
}

fn download_asset(url: &str, out_path: &Path) -> Result<(), String> {
    let mut request = ureq::get(url).set("User-Agent", &user_agent());
    if let Ok(token) = std::env::var("MOEKA_GITHUB_TOKEN")
        && !token.trim().is_empty()
    {
        request = request.set("Authorization", &format!("Bearer {}", token.trim()));
    }
    let response = request.call().map_err(|error| error.to_string())?;
    let mut reader = response.into_reader();
    let mut file = fs::File::create(out_path).map_err(|error| error.to_string())?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn user_agent() -> String {
    let build = if GIT_TAG.is_empty() {
        GIT_COMMIT
    } else {
        GIT_TAG
    };
    format!("{USER_AGENT} ({APP_VERSION}; {build})")
}

fn install_update(update: PreparedUpdate) -> Result<(), String> {
    let Some(bundle_path) = current_app_bundle_path() else {
        Command::new("open")
            .arg(&update.path)
            .spawn()
            .map_err(|error| error.to_string())?;
        return Ok(());
    };

    let pid = std::process::id();
    let script = format!(
        r#"set -e
zip_path="$1"
bundle_path="$2"
pid="$3"
while kill -0 "$pid" >/dev/null 2>&1; do sleep 0.2; done
work_dir="$(mktemp -d /tmp/moeka-update.XXXXXX)"
ditto -x -k "$zip_path" "$work_dir"
new_app="$(find "$work_dir" -maxdepth 3 -name 'Moeka.app' -type d | head -n 1)"
if [ -z "$new_app" ]; then
  open "$zip_path"
  exit 0
fi
rm -rf "$bundle_path"
ditto "$new_app" "$bundle_path"
open "$bundle_path"
rm -rf "$work_dir"
"#,
    );

    Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg("moeka-update")
        .arg(&update.path)
        .arg(bundle_path)
        .arg(pid.to_string())
        .spawn()
        .map_err(|error| error.to_string())?;
    std::process::exit(0);
}

fn current_app_bundle_path() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let macos_dir = executable.parent()?;
    if macos_dir.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents_dir = macos_dir.parent()?;
    if contents_dir.file_name()?.to_str()? != "Contents" {
        return None;
    }
    let app_dir = contents_dir.parent()?;
    app_dir.extension()?.to_str().filter(|ext| *ext == "app")?;
    Some(app_dir.to_path_buf())
}

fn safe_file_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}
