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

    pub(crate) fn refresh_stored_acp_sessions(&mut self) {
        self.stored_acp_sessions = self
            .selected_agent_id
            .as_deref()
            .map(load_sessions)
            .unwrap_or_default();
    }

    pub(crate) fn open_stored_acp_session(
        &mut self,
        session: StoredAcpSession,
        cx: &mut Context<Self>,
    ) {
        self.persist_active_acp_session(cx);
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

        if matches!(
            self.acp_panel_state.status,
            super::acp_client::AcpConnectionStatus::Ready
                | super::acp_client::AcpConnectionStatus::Failed(_)
        ) && self.acp_command_tx.is_some()
        {
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
        } else {
            self.acp_panel_state.status = super::acp_client::AcpConnectionStatus::Connecting;
        }
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
        })
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
    sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
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

    #[test]
    fn session_file_name_does_not_expose_remote_id() {
        let path = session_path("agent", "../../private/session");
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(!name.contains("private"));
        assert!(name.ends_with(".json"));
    }
}
