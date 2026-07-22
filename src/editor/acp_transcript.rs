//! Structured ACP transcript model.
//!
//! Streaming `SessionUpdate`s are folded into a list of [`TranscriptEntry`]
//! values instead of an append-only string, so tool calls can update their
//! status in place (Zed-style). The entries are rendered per type by the
//! agent timeline (`agent_timeline.rs`): text-bearing entries reuse the
//! read-only embedded Markdown editor, structured entries use dedicated
//! icon-based rows.

use agent_client_protocol::schema::v1::{
    ContentBlock, PlanEntryStatus, SessionUpdate, ToolCallStatus, ToolKind,
};

/// One logical block in the conversation.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum TranscriptEntry {
    /// A user prompt, plus any attachment links (`name`, `uri`).
    User {
        text: String,
        attachments: Vec<(String, String)>,
    },
    /// Streamed agent response text (Markdown).
    Agent { text: String },
    /// Streamed agent reasoning.
    Thought { text: String },
    /// A tool call, updated in place as `ToolCallUpdate`s arrive.
    ToolCall {
        id: String,
        title: String,
        kind: ToolKind,
        status: ToolCallStatus,
        /// Short human-readable summaries of produced content
        /// (diff paths, terminal ids, text snippets).
        details: Vec<String>,
    },
    /// The agent's latest execution plan (replaces the previous one).
    Plan {
        entries: Vec<(PlanEntryStatus, String)>,
    },
    /// Turn ended for a non-standard reason (`max_tokens`, `refusal`, …).
    TurnEnded { stop_reason: String },
    /// Fatal error reported by the connection.
    Error { message: String },
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpTranscript {
    pub entries: Vec<TranscriptEntry>,
    /// Identity stamp used by the timeline view to detect that the whole
    /// transcript was replaced (session switch, new session, clear) rather
    /// than mutated in place. Never persisted; restored transcripts always
    /// get a fresh stamp.
    #[serde(skip, default = "next_transcript_epoch")]
    pub epoch: u64,
}

impl Default for AcpTranscript {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            epoch: next_transcript_epoch(),
        }
    }
}

/// Monotonic, process-unique transcript identity stamps.
fn next_transcript_epoch() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl AcpTranscript {
    pub fn with_entries(entries: Vec<TranscriptEntry>) -> Self {
        Self {
            entries,
            epoch: next_transcript_epoch(),
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.epoch = next_transcript_epoch();
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn push_user_prompt(&mut self, text: &str, attachments: Vec<(String, String)>) {
        self.entries.push(TranscriptEntry::User {
            text: text.to_string(),
            attachments,
        });
    }

    pub fn push_error(&mut self, message: &str) {
        self.entries.push(TranscriptEntry::Error {
            message: message.to_string(),
        });
    }

    pub fn push_turn_ended(&mut self, stop_reason: &str) {
        self.entries.push(TranscriptEntry::TurnEnded {
            stop_reason: stop_reason.to_string(),
        });
    }

    /// Folds a streaming update into the entry list. Returns `true` when the
    /// transcript changed and the timeline must re-sync.
    pub fn apply_update(&mut self, update: &SessionUpdate) -> bool {
        match update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                let text = content_block_markdown(&chunk.content);
                if text.is_empty() {
                    return false;
                }
                if let Some(TranscriptEntry::Agent { text: existing }) = self.entries.last_mut() {
                    existing.push_str(&text);
                } else {
                    self.entries.push(TranscriptEntry::Agent { text });
                }
                true
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                let text = content_block_markdown(&chunk.content);
                if text.is_empty() {
                    return false;
                }
                if let Some(TranscriptEntry::Thought { text: existing }) = self.entries.last_mut() {
                    existing.push_str(&text);
                } else {
                    self.entries.push(TranscriptEntry::Thought { text });
                }
                true
            }
            SessionUpdate::ToolCall(call) => {
                let entry = TranscriptEntry::ToolCall {
                    id: call.tool_call_id.to_string(),
                    title: call.title.clone(),
                    kind: call.kind,
                    status: call.status,
                    details: tool_content_details(&call.content),
                };
                // Replaying sessions can re-announce a call; update in place.
                if let Some(existing) = self.tool_call_mut(&call.tool_call_id.to_string()) {
                    *existing = entry;
                } else {
                    self.entries.push(entry);
                }
                true
            }
            SessionUpdate::ToolCallUpdate(update) => {
                let id = update.tool_call_id.to_string();
                let fields = &update.fields;
                let new_details =
                    tool_content_details(fields.content.as_deref().unwrap_or_default());
                if let Some(TranscriptEntry::ToolCall {
                    title,
                    kind,
                    status,
                    details,
                    ..
                }) = self.tool_call_mut(&id)
                {
                    if let Some(new_title) = &fields.title {
                        *title = new_title.clone();
                    }
                    if let Some(new_kind) = fields.kind {
                        *kind = new_kind;
                    }
                    if let Some(new_status) = fields.status {
                        *status = new_status;
                    }
                    if !new_details.is_empty() {
                        *details = new_details;
                    }
                } else {
                    self.entries.push(TranscriptEntry::ToolCall {
                        id,
                        title: fields.title.clone().unwrap_or_default(),
                        kind: fields.kind.unwrap_or_default(),
                        status: fields.status.unwrap_or_default(),
                        details: new_details,
                    });
                }
                true
            }
            SessionUpdate::Plan(plan) => {
                let entries = plan
                    .entries
                    .iter()
                    .map(|entry| (entry.status.clone(), entry.content.clone()))
                    .collect::<Vec<_>>();
                if let Some(TranscriptEntry::Plan { entries: existing }) = self
                    .entries
                    .iter_mut()
                    .rev()
                    .find(|entry| matches!(entry, TranscriptEntry::Plan { .. }))
                {
                    *existing = entries;
                } else {
                    self.entries.push(TranscriptEntry::Plan { entries });
                }
                true
            }
            SessionUpdate::UserMessageChunk(chunk) => {
                // Only seen during history replay; fold into the last user entry.
                let text = content_block_markdown(&chunk.content);
                if text.is_empty() {
                    return false;
                }
                if let Some(TranscriptEntry::User { text: existing, .. }) = self.entries.last_mut()
                {
                    existing.push_str(&text);
                } else {
                    self.entries.push(TranscriptEntry::User {
                        text,
                        attachments: Vec::new(),
                    });
                }
                true
            }
            // Status-style updates carry no transcript content.
            _ => false,
        }
    }

    fn tool_call_mut(&mut self, id: &str) -> Option<&mut TranscriptEntry> {
        self.entries.iter_mut().rev().find(
            |entry| matches!(entry, TranscriptEntry::ToolCall { id: call_id, .. } if call_id == id),
        )
    }
}

/// Localized labels for the structured timeline blocks.
pub(crate) struct TranscriptLabels {
    pub thinking: &'static str,
    pub plan: &'static str,
    pub tool_pending: &'static str,
    pub tool_running: &'static str,
    pub tool_done: &'static str,
    pub tool_failed: &'static str,
    pub turn_stopped: &'static str,
}

fn content_block_markdown(content: &ContentBlock) -> String {
    match content {
        ContentBlock::Text(text) => text.text.clone(),
        ContentBlock::ResourceLink(link) => format!("[{}]({})", link.name, link.uri),
        ContentBlock::Image(_) => "*[image]*".to_string(),
        ContentBlock::Audio(_) => "*[audio]*".to_string(),
        other => {
            let mut block = String::from("\n\n```json\n");
            match serde_json::to_string_pretty(other) {
                Ok(json) => block.push_str(&json),
                Err(error) => block.push_str(&error.to_string()),
            }
            block.push_str("\n```\n");
            block
        }
    }
}

fn tool_content_details(
    content: &[agent_client_protocol::schema::v1::ToolCallContent],
) -> Vec<String> {
    use agent_client_protocol::schema::v1::ToolCallContent;
    content
        .iter()
        .filter_map(|item| match item {
            ToolCallContent::Diff(diff) => Some(format!("diff {}", diff.path.display())),
            ToolCallContent::Terminal(terminal) => {
                Some(format!("terminal {}", terminal.terminal_id))
            }
            ToolCallContent::Content(block) => match &block.content {
                ContentBlock::Text(text) => {
                    let summary: String = text.text.chars().take(120).collect();
                    let trimmed = summary.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_string())
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

pub(crate) fn transcript_labels(language: crate::i18n::UiLanguage) -> TranscriptLabels {
    use crate::i18n::UiLanguage;
    match language {
        UiLanguage::ChineseSimplified => TranscriptLabels {
            thinking: "思考中",
            plan: "计划",
            tool_pending: "等待中",
            tool_running: "运行中",
            tool_done: "完成",
            tool_failed: "失败",
            turn_stopped: "回合中止：",
        },
        UiLanguage::ChineseTraditional => TranscriptLabels {
            thinking: "思考中",
            plan: "計劃",
            tool_pending: "等待中",
            tool_running: "執行中",
            tool_done: "完成",
            tool_failed: "失敗",
            turn_stopped: "回合中止：",
        },
        UiLanguage::Japanese => TranscriptLabels {
            thinking: "思考中",
            plan: "プラン",
            tool_pending: "待機中",
            tool_running: "実行中",
            tool_done: "完了",
            tool_failed: "失敗",
            turn_stopped: "ターン中断:",
        },
        UiLanguage::Korean => TranscriptLabels {
            thinking: "생각 중",
            plan: "계획",
            tool_pending: "대기 중",
            tool_running: "실행 중",
            tool_done: "완료",
            tool_failed: "실패",
            turn_stopped: "턴 중단:",
        },
        _ => TranscriptLabels {
            thinking: "Thinking",
            plan: "Plan",
            tool_pending: "pending",
            tool_running: "running",
            tool_done: "done",
            tool_failed: "failed",
            turn_stopped: "Turn stopped:",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{
        ContentChunk, Plan, PlanEntry, PlanEntryPriority, TextContent, ToolCall, ToolCallUpdate,
        ToolCallUpdateFields,
    };

    #[test]
    fn streams_agent_chunks_into_one_entry() {
        let mut transcript = AcpTranscript::default();
        transcript.apply_update(&SessionUpdate::AgentMessageChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new("Hello ")),
        )));
        transcript.apply_update(&SessionUpdate::AgentMessageChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new("**world**")),
        )));
        assert_eq!(transcript.entries.len(), 1);
        assert_eq!(
            transcript.entries[0],
            TranscriptEntry::Agent {
                text: "Hello **world**".to_string()
            }
        );
    }

    #[test]
    fn tool_call_updates_in_place() {
        let mut transcript = AcpTranscript::default();
        transcript.apply_update(&SessionUpdate::ToolCall(
            ToolCall::new("call-1", "Read file").kind(ToolKind::Read),
        ));
        transcript.apply_update(&SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            "call-1",
            ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
        )));
        assert_eq!(transcript.entries.len(), 1);
        assert!(matches!(
            &transcript.entries[0],
            TranscriptEntry::ToolCall {
                title,
                status: ToolCallStatus::Completed,
                ..
            } if title == "Read file"
        ));
    }

    #[test]
    fn user_prompt_keeps_text_and_attachments() {
        let mut transcript = AcpTranscript::default();
        transcript.push_user_prompt(
            "line one\nline two",
            vec![("notes.md".into(), "file:///notes.md".into())],
        );
        assert_eq!(
            transcript.entries[0],
            TranscriptEntry::User {
                text: "line one\nline two".to_string(),
                attachments: vec![("notes.md".into(), "file:///notes.md".into())],
            }
        );
    }

    #[test]
    fn plan_replaces_previous_plan() {
        let mut transcript = AcpTranscript::default();
        let plan = |status| {
            SessionUpdate::Plan(Plan::new(vec![PlanEntry::new(
                "step",
                PlanEntryPriority::Medium,
                status,
            )]))
        };
        transcript.apply_update(&plan(PlanEntryStatus::Pending));
        transcript.apply_update(&plan(PlanEntryStatus::Completed));
        assert_eq!(
            transcript
                .entries
                .iter()
                .filter(|entry| matches!(entry, TranscriptEntry::Plan { .. }))
                .count(),
            1
        );
        assert!(matches!(
            &transcript.entries[0],
            TranscriptEntry::Plan { entries }
                if entries == &[(PlanEntryStatus::Completed, "step".to_string())]
        ));
    }

    #[test]
    fn thought_chunks_fold_into_one_entry() {
        let mut transcript = AcpTranscript::default();
        transcript.apply_update(&SessionUpdate::AgentThoughtChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new("consider ")),
        )));
        transcript.apply_update(&SessionUpdate::AgentThoughtChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new("the plan")),
        )));
        assert_eq!(transcript.entries.len(), 1);
        assert_eq!(
            transcript.entries[0],
            TranscriptEntry::Thought {
                text: "consider the plan".to_string()
            }
        );
    }

    #[test]
    fn transcripts_get_distinct_epochs() {
        let first = AcpTranscript::default();
        let second = AcpTranscript::with_entries(Vec::new());
        assert_ne!(first.epoch, second.epoch);

        let mut third = AcpTranscript::default();
        let before = third.epoch;
        third.clear();
        assert_ne!(before, third.epoch);
    }
}
