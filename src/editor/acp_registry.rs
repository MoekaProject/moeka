use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use gpui::Context;
use sha2::{Digest, Sha256};

use super::{EditorView, config_paths, file_ops};

const ACP_REGISTRY_URL: &str =
    "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
const ACP_REGISTRY_USER_AGENT: &str = "MoekaACP/1.0";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AgentRegistryStatus {
    Idle,
    Loading,
    Ready,
    Offline(String),
    Failed(String),
}

#[derive(Clone, Debug)]
pub(crate) struct AgentRegistryState {
    pub status: AgentRegistryStatus,
    pub version: Option<String>,
    pub agents: Vec<RegistryAgent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AgentInstallStatus {
    Installing,
    Installed { version: String, verified: bool },
    Failed(String),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct InstalledAgentManifest {
    version: String,
    command: PathBuf,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    verified: bool,
}

impl Default for AgentRegistryState {
    fn default() -> Self {
        Self {
            status: AgentRegistryStatus::Idle,
            version: None,
            agents: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct RegistryIndex {
    pub version: String,
    #[serde(default)]
    pub agents: Vec<RegistryAgent>,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct RegistryAgent {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub distribution: RegistryDistribution,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
pub(crate) struct RegistryDistribution {
    #[serde(default)]
    pub npx: Option<PackageDistribution>,
    #[serde(default)]
    pub uvx: Option<PackageDistribution>,
    #[serde(default)]
    pub binary: Option<BTreeMap<String, BinaryDistribution>>,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct PackageDistribution {
    pub package: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct BinaryDistribution {
    pub archive: String,
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentLaunchRecipe {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl RegistryAgent {
    pub(crate) fn package_launch_recipe(&self) -> Option<AgentLaunchRecipe> {
        if let Some(distribution) = &self.distribution.npx {
            let mut args = vec!["-y".to_string(), distribution.package.clone()];
            args.extend(distribution.args.clone());
            return Some(AgentLaunchRecipe {
                command: "npx".to_string(),
                args,
                env: distribution.env.clone(),
            });
        }
        if let Some(distribution) = &self.distribution.uvx {
            let mut args = vec![distribution.package.clone()];
            args.extend(distribution.args.clone());
            return Some(AgentLaunchRecipe {
                command: "uvx".to_string(),
                args,
                env: distribution.env.clone(),
            });
        }
        None
    }

    pub(crate) fn platform_binary(&self) -> Option<&BinaryDistribution> {
        self.distribution
            .binary
            .as_ref()?
            .get(current_platform_key())
    }

    pub(crate) fn distribution_label(&self) -> &'static str {
        if self.distribution.npx.is_some() {
            "npx"
        } else if self.distribution.uvx.is_some() {
            "uvx"
        } else if self.platform_binary().is_some() {
            "binary"
        } else if self.distribution.unknown.is_empty() {
            "unavailable"
        } else {
            "unsupported"
        }
    }
}

impl EditorView {
    pub(crate) fn start_agent_registry_refresh(&mut self, cx: &mut Context<Self>) {
        if self.agent_registry_task.is_some() {
            return;
        }
        self.agent_registry_state.status = AgentRegistryStatus::Loading;
        self.agent_registry_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { load_registry() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.agent_registry_task = None;
                match result {
                    Ok((index, offline_error)) => {
                        this.agent_registry_state.version = Some(index.version);
                        this.agent_registry_state.agents = index.agents;
                        if this.selected_agent_id.is_none() {
                            this.selected_agent_id = this
                                .agent_registry_state
                                .agents
                                .first()
                                .map(|agent| agent.id.clone());
                        }
                        this.refresh_stored_acp_sessions();
                        this.agent_registry_state.status = match offline_error {
                            Some(error) => AgentRegistryStatus::Offline(error),
                            None => AgentRegistryStatus::Ready,
                        };
                    }
                    Err(error) => {
                        this.agent_registry_state.status = AgentRegistryStatus::Failed(error);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(crate) fn ensure_agent_registry_loaded(&mut self, cx: &mut Context<Self>) {
        if matches!(self.agent_registry_state.status, AgentRegistryStatus::Idle) {
            self.start_agent_registry_refresh(cx);
        }
    }

    pub(crate) fn selected_registry_agent(&self) -> Option<&RegistryAgent> {
        let selected = self.selected_agent_id.as_deref()?;
        self.agent_registry_state
            .agents
            .iter()
            .find(|agent| agent.id == selected)
    }

    pub(crate) fn agent_launch_recipe(&self, agent: &RegistryAgent) -> Option<AgentLaunchRecipe> {
        installed_launch_recipe(agent).or_else(|| agent.package_launch_recipe())
    }

    pub(crate) fn install_selected_agent(&mut self, cx: &mut Context<Self>) {
        if self.agent_install_task.is_some() {
            return;
        }
        let Some(agent) = self.selected_registry_agent().cloned() else {
            return;
        };
        let Some(binary) = agent.platform_binary().cloned() else {
            self.notify_error("This Agent has no binary for the current platform.", cx);
            return;
        };
        self.agent_install_states
            .insert(agent.id.clone(), AgentInstallStatus::Installing);
        let agent_id = agent.id.clone();
        self.agent_install_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { install_registry_binary(&agent_id, &agent.version, &binary) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.agent_install_task = None;
                let status = match result {
                    Ok(manifest) => AgentInstallStatus::Installed {
                        version: manifest.version,
                        verified: manifest.verified,
                    },
                    Err(error) => AgentInstallStatus::Failed(error),
                };
                this.agent_install_states.insert(agent.id, status);
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(crate) fn selected_agent_install_status(&self) -> Option<&AgentInstallStatus> {
        let agent_id = self.selected_agent_id.as_deref()?;
        self.agent_install_states.get(agent_id)
    }
}

fn load_registry() -> Result<(RegistryIndex, Option<String>), String> {
    match fetch_registry() {
        Ok((index, source)) => {
            file_ops::atomic_write(&registry_cache_path(), source.as_bytes())
                .map_err(|error| format!("failed to cache ACP registry: {error}"))?;
            Ok((index, None))
        }
        Err(network_error) => {
            let cached = fs::read_to_string(registry_cache_path())
                .map_err(|_| format!("failed to load ACP registry: {network_error}"))?;
            let index = parse_registry(&cached)
                .map_err(|error| format!("failed to parse cached ACP registry: {error}"))?;
            Ok((index, Some(network_error)))
        }
    }
}

fn fetch_registry() -> Result<(RegistryIndex, String), String> {
    let source = ureq::get(ACP_REGISTRY_URL)
        .set("Accept", "application/json")
        .set("User-Agent", ACP_REGISTRY_USER_AGENT)
        .call()
        .map_err(|error| error.to_string())?
        .into_string()
        .map_err(|error| error.to_string())?;
    let index = parse_registry(&source)?;
    Ok((index, source))
}

fn parse_registry(source: &str) -> Result<RegistryIndex, String> {
    let mut index: RegistryIndex =
        serde_json::from_str(source).map_err(|error| error.to_string())?;
    index.agents.sort_by_key(|agent| agent.name.to_lowercase());
    Ok(index)
}

fn registry_cache_path() -> PathBuf {
    config_paths::cache_dir().join("acp").join("registry.json")
}

fn agents_dir() -> PathBuf {
    config_paths::cache_dir().join("acp").join("agents")
}

fn installed_manifest_path(agent: &RegistryAgent) -> PathBuf {
    agents_dir()
        .join(safe_segment(&agent.id))
        .join(safe_segment(&agent.version))
        .join("installed.json")
}

fn installed_launch_recipe(agent: &RegistryAgent) -> Option<AgentLaunchRecipe> {
    let manifest_path = installed_manifest_path(agent);
    let manifest: InstalledAgentManifest =
        serde_json::from_slice(&fs::read(&manifest_path).ok()?).ok()?;
    if manifest.version != agent.version {
        return None;
    }
    let install_dir = manifest_path.parent()?;
    let command = install_dir.join(&manifest.command);
    let metadata = fs::symlink_metadata(&command).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    Some(AgentLaunchRecipe {
        command: command.to_string_lossy().into_owned(),
        args: manifest.args,
        env: manifest.env,
    })
}

fn install_registry_binary(
    agent_id: &str,
    version: &str,
    binary: &BinaryDistribution,
) -> Result<InstalledAgentManifest, String> {
    let command = safe_archive_relative(Path::new(&binary.cmd))?;
    let agent_root = agents_dir().join(safe_segment(agent_id));
    let final_dir = agent_root.join(safe_segment(version));
    let existing_manifest = final_dir.join("installed.json");
    if existing_manifest.exists() {
        let source = fs::read(&existing_manifest).map_err(|error| error.to_string())?;
        let manifest: InstalledAgentManifest =
            serde_json::from_slice(&source).map_err(|error| error.to_string())?;
        if final_dir.join(&manifest.command).is_file() {
            return Ok(manifest);
        }
    }

    fs::create_dir_all(&agent_root).map_err(|error| error.to_string())?;
    let staging = agent_root.join(format!(
        ".{}.{}.staging",
        safe_segment(version),
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    let download = staging.join("agent.download");

    let result = (|| {
        download_file(&binary.archive, &download)?;
        let verified = verify_download(&download, binary.sha256.as_deref())?;
        extract_agent_archive(&download, &binary.archive, &staging, &command)?;
        fs::remove_file(&download).map_err(|error| error.to_string())?;

        let command_path = staging.join(&command);
        let metadata = fs::symlink_metadata(&command_path).map_err(|error| {
            format!(
                "installed Agent command is missing ({}): {error}",
                command.display()
            )
        })?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("installed Agent command is not a regular file".to_string());
        }
        set_executable(&command_path)?;

        let manifest = InstalledAgentManifest {
            version: version.to_string(),
            command: command.clone(),
            args: binary.args.clone(),
            env: binary.env.clone(),
            verified,
        };
        let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
        file_ops::atomic_write(&staging.join("installed.json"), &manifest_json)
            .map_err(|error| error.to_string())?;

        let backup = agent_root.join(format!(".{}.previous", safe_segment(version)));
        let _ = fs::remove_dir_all(&backup);
        if final_dir.exists() {
            fs::rename(&final_dir, &backup).map_err(|error| error.to_string())?;
        }
        if let Err(error) = fs::rename(&staging, &final_dir) {
            if backup.exists() {
                let _ = fs::rename(&backup, &final_dir);
            }
            return Err(error.to_string());
        }
        let _ = fs::remove_dir_all(backup);
        Ok(manifest)
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(staging);
    }
    result
}

fn download_file(url: &str, destination: &Path) -> Result<(), String> {
    let mut reader = ureq::get(url)
        .set("User-Agent", ACP_REGISTRY_USER_AGENT)
        .call()
        .map_err(|error| error.to_string())?
        .into_reader();
    let mut file = fs::File::create(destination).map_err(|error| error.to_string())?;
    io::copy(&mut reader, &mut file).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

fn verify_download(path: &Path, expected: Option<&str>) -> Result<bool, String> {
    let Some(expected) = expected.filter(|value| !value.trim().is_empty()) else {
        return Ok(false);
    };
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual.eq_ignore_ascii_case(expected.trim()) {
        Ok(true)
    } else {
        Err(format!(
            "Agent download checksum mismatch: expected {}, got {actual}",
            expected.trim()
        ))
    }
}

fn extract_agent_archive(
    archive_path: &Path,
    archive_url: &str,
    destination: &Path,
    command: &Path,
) -> Result<(), String> {
    let name = archive_url.to_ascii_lowercase();
    if name.ends_with(".zip") {
        extract_zip(archive_path, destination)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let file = fs::File::open(archive_path).map_err(|error| error.to_string())?;
        extract_tar(flate2::read::GzDecoder::new(file), destination)
    } else if name.ends_with(".tar.bz2") || name.ends_with(".tbz2") {
        let file = fs::File::open(archive_path).map_err(|error| error.to_string())?;
        extract_tar(bzip2::read::BzDecoder::new(file), destination)
    } else if name.ends_with(".tar") {
        let file = fs::File::open(archive_path).map_err(|error| error.to_string())?;
        extract_tar(file, destination)
    } else {
        let output = destination.join(command);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::copy(archive_path, output).map_err(|error| error.to_string())?;
        Ok(())
    }
}

fn extract_zip(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file = fs::File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("Agent archive contains a symbolic link".to_string());
        }
        let relative = safe_archive_relative(
            entry
                .enclosed_name()
                .ok_or_else(|| "Agent archive contains an unsafe path".to_string())?,
        )?;
        let output = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(output).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut output_file = fs::File::create(output).map_err(|error| error.to_string())?;
        io::copy(&mut entry, &mut output_file).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn extract_tar(reader: impl Read, destination: &Path) -> Result<(), String> {
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries().map_err(|error| error.to_string())? {
        let mut entry = entry.map_err(|error| error.to_string())?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_symlink() || entry_type.is_hard_link() {
            return Err("Agent archive contains a link".to_string());
        }
        let path = entry.path().map_err(|error| error.to_string())?;
        let relative = safe_archive_relative(&path)?;
        let output = destination.join(relative);
        if entry_type.is_dir() {
            fs::create_dir_all(output).map_err(|error| error.to_string())?;
        } else if entry_type.is_file() {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let mut output_file = fs::File::create(output).map_err(|error| error.to_string())?;
            io::copy(&mut entry, &mut output_file).map_err(|error| error.to_string())?;
        } else {
            return Err("Agent archive contains an unsupported entry type".to_string());
        }
    }
    Ok(())
}

fn safe_archive_relative(path: &Path) -> Result<PathBuf, String> {
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => relative.push(segment),
            _ => return Err(format!("unsafe Agent archive path: {}", path.display())),
        }
    }
    if relative.as_os_str().is_empty() {
        return Err("empty Agent archive path".to_string());
    }
    Ok(relative)
}

fn safe_segment(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .map_err(|error| error.to_string())?
        .permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    fs::set_permissions(path, permissions).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

pub(crate) fn current_platform_key() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "darwin-aarch64",
        ("macos", "x86_64") => "darwin-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("linux", "x86_64") => "linux-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        ("windows", "x86_64") => "windows-x86_64",
        _ => "unsupported",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_sorts_registry_agents() {
        let index = parse_registry(
            r#"{
                "version":"1.0.0",
                "agents":[
                    {"id":"z","name":"Zulu","version":"1","distribution":{"npx":{"package":"z@1","args":["--acp"]}}},
                    {"id":"a","name":"Alpha","version":"2","distribution":{"uvx":{"package":"alpha==2"}}}
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(index.agents[0].id, "a");
        assert_eq!(index.agents[1].id, "z");
    }

    #[test]
    fn creates_version_pinned_npx_recipe() {
        let index = parse_registry(
            r#"{"version":"1","agents":[{"id":"copilot","name":"Copilot","version":"1.0.71","distribution":{"npx":{"package":"@github/copilot@1.0.71","args":["--acp"],"env":{"NO_UPDATE":"1"}}}}]}"#,
        )
        .unwrap();
        let recipe = index.agents[0].package_launch_recipe().unwrap();

        assert_eq!(recipe.command, "npx");
        assert_eq!(recipe.args, ["-y", "@github/copilot@1.0.71", "--acp"]);
        assert_eq!(recipe.env.get("NO_UPDATE").map(String::as_str), Some("1"));
    }

    #[test]
    fn preserves_unknown_distribution_kinds() {
        let index = parse_registry(
            r#"{"version":"1","agents":[{"id":"future","name":"Future","version":"1","distribution":{"container":{"image":"example/agent:1"}}}]}"#,
        )
        .unwrap();

        assert_eq!(index.agents[0].distribution_label(), "unsupported");
    }

    #[test]
    fn archive_paths_cannot_escape_install_directory() {
        assert_eq!(
            safe_archive_relative(Path::new("./bin/agent")).unwrap(),
            PathBuf::from("bin/agent")
        );
        assert!(safe_archive_relative(Path::new("../agent")).is_err());
        assert!(safe_archive_relative(Path::new("/tmp/agent")).is_err());
    }

    #[test]
    fn verifies_sha256_when_registry_provides_it() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("download");
        fs::write(&path, b"abc").unwrap();

        assert!(
            verify_download(
                &path,
                Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
            )
            .unwrap()
        );
        assert!(verify_download(&path, Some("deadbeef")).is_err());
        assert!(!verify_download(&path, None).unwrap());
    }

    #[test]
    fn raw_binary_uses_registry_command_path() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("download");
        let destination = temp.path().join("install");
        fs::create_dir(&destination).unwrap();
        fs::write(&archive, b"binary").unwrap();

        extract_agent_archive(
            &archive,
            "https://example.test/agent",
            &destination,
            Path::new("bin/agent"),
        )
        .unwrap();

        assert_eq!(fs::read(destination.join("bin/agent")).unwrap(), b"binary");
    }
}
