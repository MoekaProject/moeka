use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use agent_client_protocol::schema::v1::SessionUpdate;
use gpui::Context;

use super::acp_client::AcpExtensionEvent;
use super::acp_transcript::{AcpTranscript, TranscriptEntry};
use super::{EditorView, config_paths, config_store, file_ops};

const ACP_SESSION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct StoredAcpSession {
    pub schema_version: u32,
    pub agent_id: String,
    pub agent_name: String,
    pub session_id: String,
    /// Legacy rendered markdown; superseded by `transcript_entries` but kept
    /// so sessions written by older builds still restore.
    #[serde(default)]
    pub transcript_markdown: String,
    #[serde(default)]
    pub transcript_entries: Vec<TranscriptEntry>,
    pub updates: Vec<SessionUpdate>,
    #[serde(default)]
    pub extensions: Vec<AcpExtensionEvent>,
    #[serde(default)]
    pub modes: Option<agent_client_protocol::schema::v1::SessionModeState>,
    #[serde(default)]
    pub config_options: Vec<agent_client_protocol::schema::v1::SessionConfigOption>,
    pub composer_draft: String,
    pub updated_at: String,
    /// Local-only history metadata; persisted in the sidecar index, not in
    /// the transcript JSON that is rewritten by active-session autosave.
    #[serde(skip)]
    pub custom_title: Option<String>,
    #[serde(skip)]
    pub archived: bool,
    #[serde(skip)]
    pub pinned: bool,
    #[serde(skip)]
    pub pinned_at: Option<String>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct AcpSessionMetadataIndex {
    #[serde(default)]
    sessions: Vec<AcpSessionMetadata>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct AcpSessionMetadata {
    agent_id: String,
    session_id: String,
    #[serde(default)]
    custom_title: Option<String>,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    pinned_at: Option<String>,
}

impl StoredAcpSession {
    /// Rebuilds the structured transcript, falling back to a single agent
    /// block for records saved before entries existed.
    pub(crate) fn restored_transcript(&self) -> AcpTranscript {
        if !self.transcript_entries.is_empty() {
            return AcpTranscript {
                entries: self.transcript_entries.clone(),
            };
        }
        let mut transcript = AcpTranscript::default();
        if !self.transcript_markdown.trim().is_empty() {
            transcript.entries.push(TranscriptEntry::Agent {
                text: self.transcript_markdown.clone(),
            });
        }
        transcript
    }

    /// A short human-readable label for history rows: the first line of the
    /// first user prompt, or the agent name as fallback.
    pub(crate) fn display_title(&self) -> String {
        if let Some(title) = self
            .custom_title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
        {
            return title.to_string();
        }
        self.transcript_entries
            .iter()
            .find_map(|entry| match entry {
                TranscriptEntry::User { text, .. } => {
                    let line = text.lines().next().unwrap_or_default().trim();
                    (!line.is_empty()).then(|| line.chars().take(64).collect::<String>())
                }
                _ => None,
            })
            .unwrap_or_else(|| self.agent_name.clone())
    }
}

impl EditorView {
    pub(crate) fn restore_active_acp_session(
        &mut self,
        active: Option<&config_store::ActiveAcpSessionRecord>,
    ) {
        let session = active
            .and_then(|active| {
                load_sessions(&active.agent_id)
                    .into_iter()
                    .find(|session| session.session_id == active.session_id)
            })
            .or_else(load_most_recent_session);
        let Some(session) = session else {
            return;
        };

        self.selected_agent_id = Some(session.agent_id.clone());
        self.acp_panel_state.agent_id = Some(session.agent_id.clone());
        self.acp_panel_state.agent_name = Some(session.agent_name.clone());
        self.acp_panel_state.session_id = Some(agent_client_protocol::schema::v1::SessionId::new(
            session.session_id.clone(),
        ));
        self.acp_panel_state.transcript = session.restored_transcript();
        self.acp_panel_state.updates = session.updates.clone();
        self.acp_panel_state.extensions = session.extensions.clone();
        self.acp_panel_state.modes = session.modes.clone();
        self.acp_panel_state.config_options = session.config_options.clone();
        self.pending_acp_open_session = Some(session);
        persist_active_session_reference(&self.acp_panel_state);
    }

    /// Reloads the session history (all agents) on a background thread;
    /// session files embed full transcripts, so parsing them on the UI
    /// thread would hitch with long chats.
    pub(crate) fn refresh_stored_acp_sessions(&mut self, cx: &mut Context<Self>) {
        self.stored_acp_sessions_epoch = self.stored_acp_sessions_epoch.wrapping_add(1);
        let epoch = self.stored_acp_sessions_epoch;
        let scroll_top = (-f32::from(self.agent_history_scroll_handle.offset().y)).max(0.0);
        self.stored_acp_sessions_task = Some(cx.spawn(async move |this, cx| {
            let sessions = cx
                .background_executor()
                .spawn(async move { load_all_sessions() })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.stored_acp_sessions_epoch == epoch {
                    this.stored_acp_sessions = sessions;
                    this.stored_acp_sessions_task = None;
                    this.pending_agent_history_scroll_top.set(Some(scroll_top));
                    cx.notify();
                }
            });
        }));
    }

    pub(crate) fn open_stored_acp_session(
        &mut self,
        session: StoredAcpSession,
        cx: &mut Context<Self>,
    ) {
        self.persist_active_acp_session(cx);
        // History spans every agent: opening a session that belongs to a
        // different agent must relaunch that agent instead of reusing the
        // current connection.
        let same_agent =
            self.acp_panel_state.agent_id.as_deref() == Some(session.agent_id.as_str());
        self.selected_agent_id = Some(session.agent_id.clone());
        if !same_agent {
            self.stop_acp_agent();
            self.acp_panel_state.status = super::acp_client::AcpConnectionStatus::Disconnected;
        }
        self.acp_panel_state.agent_id = Some(session.agent_id.clone());
        self.acp_panel_state.agent_name = Some(session.agent_name.clone());
        self.acp_panel_state.session_id = Some(agent_client_protocol::schema::v1::SessionId::new(
            session.session_id.clone(),
        ));
        self.acp_panel_state.transcript = session.restored_transcript();
        self.acp_panel_state.updates = session.updates.clone();
        self.acp_panel_state.extensions = session.extensions.clone();
        self.acp_panel_state.modes = session.modes.clone();
        self.acp_panel_state.config_options = session.config_options.clone();
        self.pending_acp_open_session = Some(session.clone());
        self.sync_agent_transcript_editor(cx);
        if let Some(composer) = self.agent_composer_editor.clone() {
            composer.update(cx, |editor, cx| {
                editor.set_text(&session.composer_draft, cx)
            });
        }

        if same_agent
            && matches!(
                self.acp_panel_state.status,
                super::acp_client::AcpConnectionStatus::Ready
                    | super::acp_client::AcpConnectionStatus::Prompting
                    | super::acp_client::AcpConnectionStatus::Failed(_)
            )
            && self.acp_command_tx.is_some()
        {
            // Reuse the live connection. A running turn is cancelled first;
            // the open command then queues behind it on the same channel.
            if self.acp_panel_state.status == super::acp_client::AcpConnectionStatus::Prompting {
                self.cancel_acp_prompt(cx);
                self.acp_panel_state.status = super::acp_client::AcpConnectionStatus::Ready;
            }
            self.send_pending_acp_open_session(cx);
        } else {
            self.start_selected_acp_agent(cx);
        }
    }

    pub(crate) fn send_pending_acp_open_session(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.pending_acp_open_session.as_ref() else {
            return;
        };
        let session_id =
            agent_client_protocol::schema::v1::SessionId::new(session.session_id.clone());
        if self.acp_command_tx.as_ref().is_none_or(|sender| {
            sender
                .try_send(super::acp_client::AcpCommand::OpenSession(session_id))
                .is_err()
        }) {
            self.acp_panel_state.status = super::acp_client::AcpConnectionStatus::Failed(
                "ACP command channel closed".to_string(),
            );
        }
        // The connection stays live while the session loads: keep the
        // current (Ready) status instead of flashing "Connecting…", which
        // reads as a full reconnect.
        cx.notify();
    }

    pub(crate) fn persist_active_acp_session(&self, cx: &Context<Self>) {
        let Some(record) = self.active_acp_session_record(cx) else {
            return;
        };
        if let Err(error) = save_session(&record) {
            eprintln!("failed to save ACP session: {error}");
        }
    }

    pub(crate) fn queue_active_acp_session_save(&mut self, cx: &mut Context<Self>) {
        self.acp_session_save_epoch = self.acp_session_save_epoch.wrapping_add(1);
        let epoch = self.acp_session_save_epoch;
        self.acp_session_save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(250))
                .await;
            let record = this
                .update(cx, |this, cx| {
                    (this.acp_session_save_epoch == epoch)
                        .then(|| this.active_acp_session_record(cx))
                        .flatten()
                })
                .ok()
                .flatten();
            if let Some(record) = record {
                let result = cx
                    .background_executor()
                    .spawn(async move { save_session(&record) })
                    .await;
                if let Err(error) = result {
                    eprintln!("failed to save ACP session: {error}");
                }
            }
            let _ = this.update(cx, |this, _| {
                if this.acp_session_save_epoch == epoch {
                    this.acp_session_save_task = None;
                }
            });
        }));
    }

    fn active_acp_session_record(&self, cx: &gpui::App) -> Option<StoredAcpSession> {
        let agent_id = self.acp_panel_state.agent_id.clone()?;
        let session_id = self.acp_panel_state.session_id.as_ref()?.0.to_string();
        let composer_draft = self
            .agent_composer_editor
            .as_ref()
            .map(|editor| editor.read(cx).text(cx).to_string())
            .unwrap_or_default();
        Some(StoredAcpSession {
            schema_version: ACP_SESSION_SCHEMA_VERSION,
            agent_id,
            agent_name: self
                .acp_panel_state
                .agent_name
                .clone()
                .unwrap_or_else(|| "Agent".to_string()),
            session_id,
            transcript_markdown: String::new(),
            transcript_entries: self.acp_panel_state.transcript.entries.clone(),
            updates: self.acp_panel_state.updates.clone(),
            extensions: self.acp_panel_state.extensions.clone(),
            modes: self.acp_panel_state.modes.clone(),
            config_options: self.acp_panel_state.config_options.clone(),
            composer_draft,
            updated_at: chrono::Utc::now().to_rfc3339(),
            custom_title: None,
            archived: false,
            pinned: false,
            pinned_at: None,
        })
    }

    pub(crate) fn set_acp_session_custom_title(
        &mut self,
        agent_id: &str,
        session_id: &str,
        title: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let title = title
            .map(|title| title.trim().to_string())
            .filter(|title| !title.is_empty());
        if let Err(error) = update_session_metadata(agent_id, session_id, |metadata| {
            metadata.custom_title = title.clone();
        }) {
            self.notify_error(error, cx);
            return;
        }
        self.refresh_stored_acp_sessions(cx);
    }

    pub(crate) fn set_acp_sessions_pinned(
        &mut self,
        targets: &[(String, String)],
        pinned: bool,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = update_sessions_metadata(targets, |metadata| {
            metadata.pinned = pinned;
            metadata.pinned_at = pinned.then(|| chrono::Utc::now().to_rfc3339());
        }) {
            self.notify_error(error, cx);
            return;
        }
        self.refresh_stored_acp_sessions(cx);
    }

    pub(crate) fn set_acp_sessions_archived(
        &mut self,
        targets: &[(String, String)],
        archived: bool,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = update_sessions_metadata(targets, |metadata| {
            metadata.archived = archived;
        }) {
            self.notify_error(error, cx);
            return;
        }
        self.selected_agent_session_ids.clear();
        self.agent_session_selection_anchor = None;
        self.refresh_stored_acp_sessions(cx);
    }

    pub(crate) fn delete_local_acp_sessions(
        &mut self,
        targets: &[(String, String)],
        cx: &mut Context<Self>,
    ) {
        let active_key = self.acp_panel_state.agent_id.clone().zip(
            self.acp_panel_state
                .session_id
                .as_ref()
                .map(|active| active.0.to_string()),
        );
        let includes_active = active_key
            .as_ref()
            .is_some_and(|active| targets.contains(active));
        if includes_active {
            if self.acp_panel_state.status == super::acp_client::AcpConnectionStatus::Prompting {
                self.cancel_acp_prompt(cx);
                self.acp_panel_state.status = super::acp_client::AcpConnectionStatus::Ready;
            }
            self.new_acp_session(cx);
        }
        let current_active = self.acp_panel_state.agent_id.clone().zip(
            self.acp_panel_state
                .session_id
                .as_ref()
                .map(|active| active.0.to_string()),
        );
        let deletable = targets
            .iter()
            .filter(|target| {
                current_active
                    .as_ref()
                    .is_none_or(|active| active != *target)
            })
            .cloned()
            .collect::<Vec<_>>();
        if deletable.is_empty() {
            return;
        }
        let target_set = deletable
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        let Ok(agent_directories) = fs::read_dir(sessions_dir()) else {
            return;
        };
        for agent_directory in agent_directories.flatten() {
            let Ok(entries) = fs::read_dir(agent_directory.path()) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(source) = fs::read(entry.path()) else {
                    continue;
                };
                let Ok(record) = serde_json::from_slice::<StoredAcpSession>(&source) else {
                    continue;
                };
                if !target_set.contains(&(record.agent_id, record.session_id)) {
                    continue;
                }
                if let Err(error) = fs::remove_file(entry.path())
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    self.notify_error(error.to_string(), cx);
                    return;
                }
            }
        }
        if let Err(error) = remove_sessions_metadata(&deletable) {
            self.notify_error(error, cx);
            return;
        }
        self.selected_agent_session_ids.clear();
        self.agent_session_selection_anchor = None;
        self.refresh_stored_acp_sessions(cx);
    }
}

fn save_session(record: &StoredAcpSession) -> Result<(), String> {
    let path = session_path(&record.agent_id, &record.session_id);
    let source = serde_json::to_vec_pretty(record).map_err(|error| error.to_string())?;
    file_ops::atomic_write(&path, &source).map_err(|error| error.to_string())?;
    persist_active_session_reference_from_ids(&record.agent_id, &record.session_id);
    Ok(())
}

fn persist_active_session_reference(state: &super::acp_client::AcpPanelState) {
    let Some(agent_id) = state.agent_id.as_deref() else {
        return;
    };
    let Some(session_id) = state.session_id.as_ref() else {
        return;
    };
    persist_active_session_reference_from_ids(agent_id, &session_id.0);
}

fn persist_active_session_reference_from_ids(agent_id: &str, session_id: &str) {
    let mut config = config_store::load_config();
    config.session.active_acp_session = Some(config_store::ActiveAcpSessionRecord {
        agent_id: agent_id.to_string(),
        session_id: session_id.to_string(),
    });
    config_store::save_config(&config);
}

pub(crate) fn load_sessions(agent_id: &str) -> Vec<StoredAcpSession> {
    let directory = sessions_dir().join(safe_segment(agent_id));
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut sessions = entries
        .flatten()
        .filter_map(|entry| fs::read(entry.path()).ok())
        .filter_map(|source| serde_json::from_slice::<StoredAcpSession>(&source).ok())
        .filter(|record| record.schema_version == ACP_SESSION_SCHEMA_VERSION)
        .collect::<Vec<_>>();
    deduplicate_sessions(&mut sessions);
    apply_metadata_and_sort(&mut sessions, &load_metadata_index());
    sessions
}

/// Every stored session across all agents, most recent first.
pub(crate) fn load_all_sessions() -> Vec<StoredAcpSession> {
    let Ok(entries) = fs::read_dir(sessions_dir()) else {
        return Vec::new();
    };
    let mut sessions = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .flat_map(|entry| {
            fs::read_dir(entry.path())
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|entry| fs::read(entry.path()).ok())
                .filter_map(|source| serde_json::from_slice::<StoredAcpSession>(&source).ok())
                .filter(|record| record.schema_version == ACP_SESSION_SCHEMA_VERSION)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    deduplicate_sessions(&mut sessions);
    apply_metadata_and_sort(&mut sessions, &load_metadata_index());
    sessions
}

fn load_most_recent_session() -> Option<StoredAcpSession> {
    fs::read_dir(sessions_dir())
        .ok()?
        .flatten()
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry)
        })
        .flat_map(|entry| {
            fs::read_dir(entry.path())
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|entry| fs::read(entry.path()).ok())
                .filter_map(|source| serde_json::from_slice::<StoredAcpSession>(&source).ok())
                .filter(|record| record.schema_version == ACP_SESSION_SCHEMA_VERSION)
                .collect::<Vec<_>>()
        })
        .max_by(|left, right| left.updated_at.cmp(&right.updated_at))
}

fn session_path(agent_id: &str, session_id: &str) -> PathBuf {
    let hash = file_ops::content_hash(session_id.as_bytes());
    sessions_dir()
        .join(safe_segment(agent_id))
        .join(format!("{hash}.json"))
}

fn session_metadata_path() -> PathBuf {
    config_paths::config_dir()
        .join("acp")
        .join("session-metadata.json")
}

fn load_metadata_index() -> AcpSessionMetadataIndex {
    fs::read(session_metadata_path())
        .ok()
        .and_then(|source| serde_json::from_slice(&source).ok())
        .unwrap_or_default()
}

fn save_metadata_index(index: &AcpSessionMetadataIndex) -> Result<(), String> {
    let source = serde_json::to_vec_pretty(index).map_err(|error| error.to_string())?;
    file_ops::atomic_write(&session_metadata_path(), &source).map_err(|error| error.to_string())
}

fn update_session_metadata(
    agent_id: &str,
    session_id: &str,
    update: impl FnOnce(&mut AcpSessionMetadata),
) -> Result<(), String> {
    let mut index = load_metadata_index();
    let position = index
        .sessions
        .iter()
        .position(|metadata| metadata.agent_id == agent_id && metadata.session_id == session_id);
    let metadata = if let Some(position) = position {
        &mut index.sessions[position]
    } else {
        index.sessions.push(AcpSessionMetadata {
            agent_id: agent_id.to_string(),
            session_id: session_id.to_string(),
            custom_title: None,
            archived: false,
            pinned: false,
            pinned_at: None,
        });
        index
            .sessions
            .last_mut()
            .expect("metadata was just inserted")
    };
    update(metadata);
    save_metadata_index(&index)
}

fn update_sessions_metadata(
    targets: &[(String, String)],
    mut update: impl FnMut(&mut AcpSessionMetadata),
) -> Result<(), String> {
    let mut index = load_metadata_index();
    for (agent_id, session_id) in targets {
        let position = index.sessions.iter().position(|metadata| {
            metadata.agent_id == *agent_id && metadata.session_id == *session_id
        });
        let metadata = if let Some(position) = position {
            &mut index.sessions[position]
        } else {
            index.sessions.push(AcpSessionMetadata {
                agent_id: agent_id.clone(),
                session_id: session_id.clone(),
                custom_title: None,
                archived: false,
                pinned: false,
                pinned_at: None,
            });
            index
                .sessions
                .last_mut()
                .expect("metadata was just inserted")
        };
        update(metadata);
    }
    save_metadata_index(&index)
}

fn remove_sessions_metadata(targets: &[(String, String)]) -> Result<(), String> {
    let mut index = load_metadata_index();
    index.sessions.retain(|metadata| {
        !targets.iter().any(|(agent_id, session_id)| {
            metadata.agent_id == *agent_id && metadata.session_id == *session_id
        })
    });
    save_metadata_index(&index)
}

fn apply_metadata_and_sort(sessions: &mut [StoredAcpSession], index: &AcpSessionMetadataIndex) {
    for session in sessions.iter_mut() {
        if let Some(metadata) = index.sessions.iter().find(|metadata| {
            metadata.agent_id == session.agent_id && metadata.session_id == session.session_id
        }) {
            session.custom_title = metadata.custom_title.clone();
            session.archived = metadata.archived;
            session.pinned = metadata.pinned;
            session.pinned_at = metadata.pinned_at.clone();
        }
    }
    sessions.sort_by(|left, right| {
        right
            .pinned
            .cmp(&left.pinned)
            .then_with(|| right.pinned_at.cmp(&left.pinned_at))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
    });
}

fn deduplicate_sessions(sessions: &mut Vec<StoredAcpSession>) {
    let mut latest = HashMap::<(String, String), StoredAcpSession>::new();
    for session in sessions.drain(..) {
        let key = (session.agent_id.clone(), session.session_id.clone());
        match latest.get_mut(&key) {
            Some(existing) if existing.updated_at < session.updated_at => *existing = session,
            None => {
                latest.insert(key, session);
            }
            _ => {}
        }
    }
    sessions.extend(latest.into_values());
}

fn sessions_dir() -> PathBuf {
    config_paths::config_dir().join("acp").join("sessions")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, updated_at: &str) -> StoredAcpSession {
        StoredAcpSession {
            schema_version: ACP_SESSION_SCHEMA_VERSION,
            agent_id: "agent".to_string(),
            agent_name: "Agent".to_string(),
            session_id: id.to_string(),
            transcript_markdown: String::new(),
            transcript_entries: vec![TranscriptEntry::User {
                text: format!("Prompt {id}"),
                attachments: Vec::new(),
            }],
            updates: Vec::new(),
            extensions: Vec::new(),
            modes: None,
            config_options: Vec::new(),
            composer_draft: String::new(),
            updated_at: updated_at.to_string(),
            custom_title: None,
            archived: false,
            pinned: false,
            pinned_at: None,
        }
    }

    #[test]
    fn session_file_name_does_not_expose_remote_id() {
        let path = session_path("agent", "../../private/session");
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(!name.contains("private"));
        assert!(name.ends_with(".json"));
    }

    #[test]
    fn metadata_overrides_title_and_sorts_pinned_first() {
        let mut sessions = vec![
            session("newer", "2026-07-22T12:00:00Z"),
            session("pinned", "2026-07-20T12:00:00Z"),
        ];
        let index = AcpSessionMetadataIndex {
            sessions: vec![AcpSessionMetadata {
                agent_id: "agent".to_string(),
                session_id: "pinned".to_string(),
                custom_title: Some("Custom title".to_string()),
                archived: true,
                pinned: true,
                pinned_at: Some("2026-07-22T13:00:00Z".to_string()),
            }],
        };

        apply_metadata_and_sort(&mut sessions, &index);

        assert_eq!(sessions[0].session_id, "pinned");
        assert_eq!(sessions[0].display_title(), "Custom title");
        assert!(sessions[0].pinned);
        assert!(sessions[0].archived);
        assert_eq!(sessions[1].display_title(), "Prompt newer");
    }

    #[test]
    fn duplicate_session_keys_keep_the_newest_record() {
        let mut sessions = vec![
            session("same", "2026-07-20T12:00:00Z"),
            session("same", "2026-07-22T12:00:00Z"),
        ];

        deduplicate_sessions(&mut sessions);

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].updated_at, "2026-07-22T12:00:00Z");
    }
}
