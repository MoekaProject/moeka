use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use gpui::Context;
use serde::Deserialize;

use super::{Dialog, DialogAction, EditorView, config_paths};

const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/MoekaProject/community/releases/latest";
const USER_AGENT: &str = "MoekaUpdater/1.0";
const UPDATE_POLL_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);

pub(crate) const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const GIT_COMMIT: &str = env!("MOEKA_GIT_COMMIT");
pub(crate) const GIT_TAG: &str = env!("MOEKA_GIT_TAG");

#[derive(Clone, Debug)]
pub(crate) enum UpdateState {
    Idle,
    Checking,
    Ready(PreparedUpdate),
    Installing,
    Failed,
    UpToDate,
}

fn release_notes_message(language: crate::i18n::UiLanguage, release_notes: &str) -> String {
    let filtered_notes = release_notes
        .lines()
        .filter(|line| {
            !line
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("signed-off-by")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let notes = filtered_notes.trim();
    if notes.is_empty() {
        return crate::i18n::release_notes_empty(language).to_string();
    }

    const MAX_RELEASE_NOTES_CHARS: usize = 6_000;
    let mut message: String = notes.chars().take(MAX_RELEASE_NOTES_CHARS).collect();
    if notes.chars().count() > MAX_RELEASE_NOTES_CHARS {
        message.push_str("\n\n…");
    }
    message
}

impl UpdateState {
    pub(crate) fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedUpdate {
    pub version: String,
    pub release_notes: String,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    body: Option<String>,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

impl EditorView {
    /// Checks on launch and then once a day, so a release published while the
    /// app stays open is still picked up.
    pub(crate) fn start_update_polling(&mut self, cx: &mut Context<Self>) {
        if self.update_poll_task.is_some() {
            return;
        }

        self.start_update_check(cx);
        self.update_poll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(UPDATE_POLL_INTERVAL).await;
                if this
                    .update(cx, |this, cx| this.start_update_check(cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

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

    pub(crate) fn show_prepared_update_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(update) = self.prepared_update().cloned() else {
            return;
        };
        self.show_dialog(
            Dialog {
                title: crate::i18n::update_dialog_title(self.ui_language, &update.version),
                message: release_notes_message(self.ui_language, &update.release_notes),
                save_label: None,
                confirm_label: crate::i18n::install_update_label(self.ui_language).to_string(),
                cancel_label: self.t(crate::i18n::TextKey::Cancel).to_string(),
                is_danger: false,
                action: DialogAction::InstallUpdate { update },
            },
            cx,
        );
    }

    pub(crate) fn install_update(&mut self, update: PreparedUpdate) {
        self.update_state = UpdateState::Installing;
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

    let asset = select_update_asset(&release.assets).ok_or_else(|| {
        format!(
            "no compatible macOS update DMG found in release {}",
            release.tag_name
        )
    })?;
    let update_dir = config_paths::cache_dir()
        .join("updates")
        .join(safe_file_name(&release.tag_name));
    fs::create_dir_all(&update_dir).map_err(|error| error.to_string())?;
    let out_path = update_dir.join(safe_file_name(&asset.name));
    download_asset(&asset.browser_download_url, &out_path)?;

    Ok(Some(PreparedUpdate {
        version: release.tag_name,
        release_notes: release.body.unwrap_or_default(),
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
    // Temporary update-test build: compare against the hacked package version
    // even when the checkout is exactly on the published v0.1.0 tag.
    APP_VERSION
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
    select_update_asset_for_arch(assets, std::env::consts::ARCH)
}

fn select_update_asset_for_arch<'a>(
    assets: &'a [GitHubAsset],
    architecture: &str,
) -> Option<&'a GitHubAsset> {
    let preferred: &[&str] = match architecture {
        "aarch64" => &["macos-arm64.dmg", "macos-universal.dmg"],
        "x86_64" => &["macos-x86_64.dmg", "macos-amd64.dmg", "macos-universal.dmg"],
        _ => return None,
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
    let expected_arch = lipo_arch_for_rust_arch(std::env::consts::ARCH).ok_or_else(|| {
        format!(
            "unsupported update architecture: {}",
            std::env::consts::ARCH
        )
    })?;
    let log_path = config_paths::cache_dir()
        .join("updates")
        .join("install.log");
    if let Some(log_dir) = log_path.parent() {
        fs::create_dir_all(log_dir).map_err(|error| error.to_string())?;
    }
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|error| error.to_string())?;
    let log_file_for_stderr = log_file.try_clone().map_err(|error| error.to_string())?;
    let script = r#"set -e
dmg_path="$1"
bundle_path="$2"
pid="$3"
expected_arch="$4"
staging_dir=""
backup_path=""
device=""
mount_dir=""

cleanup() {
  status=$?
  trap - EXIT
    if [ -n "$device" ]; then
        hdiutil detach "$device" -quiet >/dev/null 2>&1 || true
    fi
  if [ "$status" -ne 0 ]; then
    if [ -n "$backup_path" ] && [ -d "$backup_path" ] && [ ! -d "$bundle_path" ]; then
      mv "$backup_path" "$bundle_path" || true
    fi
    if [ -d "$bundle_path" ]; then
      open "$bundle_path" || true
    fi
  fi
  if [ -n "$staging_dir" ]; then rm -rf "$staging_dir"; fi
  if [ "$status" -eq 0 ] && [ -n "$backup_path" ]; then rm -rf "$backup_path"; fi
  exit "$status"
}
trap cleanup EXIT

echo "$(date '+%Y-%m-%d %H:%M:%S') Installing Moeka update from $dmg_path into $bundle_path"
parent_dir="$(dirname "$bundle_path")"
bundle_name="$(basename "$bundle_path")"
staging_dir="$(mktemp -d "$parent_dir/.moeka-update.XXXXXX")"
staged_app="$staging_dir/$bundle_name"
backup_path="$parent_dir/.${bundle_name}.previous-update"

hdiutil verify "$dmg_path" >/dev/null
attach_output="$(hdiutil attach "$dmg_path" -readonly -noverify -noautoopen)"
attach_line="$(printf '%s\n' "$attach_output" | awk '/\/Volumes\// { print; exit }')"
device="$(printf '%s\n' "$attach_line" | awk '{ print $1 }')"
mount_dir="$(printf '%s\n' "$attach_line" | sed -E 's|^.*[[:space:]](/Volumes/.*)$|\1|')"
if [ -z "$device" ] || [ -z "$mount_dir" ] || [ ! -d "$mount_dir" ]; then
    printf '%s\n' "$attach_output" >&2
    echo "Failed to mount update DMG: $dmg_path" >&2
    exit 1
fi
new_app="$(find "$mount_dir" -maxdepth 2 -name 'Moeka.app' -type d -print -quit)"
if [ -z "$new_app" ] || [ ! -d "$new_app/Contents" ]; then
    echo "Moeka.app was not found in update DMG: $dmg_path" >&2
  exit 1
fi
ditto "$new_app" "$staged_app"
staged_executable="$staged_app/Contents/MacOS/moeka"
if [ ! -x "$staged_executable" ]; then
    echo "Moeka executable was not found in staged update: $staged_executable" >&2
    exit 1
fi
if ! lipo "$staged_executable" -verify_arch "$expected_arch"; then
    echo "Update architecture does not match this application: $expected_arch" >&2
    exit 1
fi
hdiutil detach "$device" -quiet
device=""
mount_dir=""

while kill -0 "$pid" >/dev/null 2>&1; do sleep 0.2; done
rm -rf "$backup_path"
mv "$bundle_path" "$backup_path"
mv "$staged_app" "$bundle_path"
xattr -dr com.apple.quarantine "$bundle_path" >/dev/null 2>&1 || true
open "$bundle_path"
echo "$(date '+%Y-%m-%d %H:%M:%S') Moeka update installed and relaunched"
"#;

    Command::new("/usr/bin/nohup")
        .arg("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg("moeka-update")
        .arg(&update.path)
        .arg(bundle_path)
        .arg(pid.to_string())
        .arg(expected_arch)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_for_stderr))
        .spawn()
        .map_err(|error| error.to_string())?;
    std::process::exit(0);
}

fn lipo_arch_for_rust_arch(architecture: &str) -> Option<&'static str> {
    match architecture {
        "aarch64" => Some("arm64"),
        "x86_64" => Some("x86_64"),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::{GitHubAsset, lipo_arch_for_rust_arch, select_update_asset_for_arch};

    fn asset(name: &str) -> GitHubAsset {
        GitHubAsset {
            name: name.to_string(),
            browser_download_url: format!("https://example.invalid/{name}"),
        }
    }

    #[test]
    fn update_assets_select_architecture_specific_dmgs() {
        let assets = [
            asset("Moeka-macos-arm64.zip"),
            asset("Moeka-macos-amd64.dmg"),
            asset("Moeka-macos-x86_64.dmg"),
            asset("Moeka-macos-arm64.dmg"),
        ];

        assert_eq!(
            select_update_asset_for_arch(&assets, "aarch64").map(|asset| asset.name.as_str()),
            Some("Moeka-macos-arm64.dmg")
        );
        assert_eq!(
            select_update_asset_for_arch(&assets, "x86_64").map(|asset| asset.name.as_str()),
            Some("Moeka-macos-x86_64.dmg")
        );
        assert!(select_update_asset_for_arch(&assets, "riscv64").is_none());
    }

    #[test]
    fn update_assets_ignore_zip_only_releases() {
        let assets = [asset("Moeka-macos-arm64.zip")];
        assert!(select_update_asset_for_arch(&assets, "aarch64").is_none());
    }

    #[test]
    fn rust_architectures_map_to_lipo_names() {
        assert_eq!(lipo_arch_for_rust_arch("aarch64"), Some("arm64"));
        assert_eq!(lipo_arch_for_rust_arch("x86_64"), Some("x86_64"));
        assert_eq!(lipo_arch_for_rust_arch("riscv64"), None);
    }
}
