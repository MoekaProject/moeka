use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AgentNotification, AuthenticateRequest, CancelNotification, ClientCapabilities,
    CloseSessionRequest, ContentBlock, CreateTerminalRequest, EnvVariable, FileSystemCapabilities,
    Implementation, InitializeRequest, KillTerminalRequest, LoadSessionRequest, McpServer,
    McpServerStdio, NewSessionRequest, PromptRequest, ReadTextFileRequest, ReleaseTerminalRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse, ResourceLink,
    ResumeSessionRequest, SelectedPermissionOutcome, SessionConfigId, SessionConfigOption,
    SessionConfigOptionValue, SessionId, SessionModeId, SessionModeState, SessionUpdate,
    SetSessionConfigOptionRequest, SetSessionModeRequest, TerminalOutputRequest, TextContent,
    WaitForTerminalExitRequest, WriteTextFileRequest,
};
use agent_client_protocol::{AcpAgent, Agent, ConnectionTo};
use gpui::Context;

use super::acp_client_services::AcpClientServices;
use super::acp_registry::AgentLaunchRecipe;
use super::acp_transcript::AcpTranscript;
use super::{APP_VERSION, EditorView};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AcpConnectionStatus {
    Disconnected,
    Connecting,
    Authenticating,
    Ready,
    Prompting,
    Failed(String),
}

#[derive(Clone, Debug)]
pub(crate) struct AcpPanelState {
    pub status: AcpConnectionStatus,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    pub session_id: Option<SessionId>,
    pub transcript: AcpTranscript,
    pub updates: Vec<SessionUpdate>,
    pub stderr_lines: Vec<String>,
    pub pending_permission: Option<PendingAcpPermission>,
    pub pending_attachments: Vec<ContentBlock>,
    pub extensions: Vec<AcpExtensionEvent>,
    pub modes: Option<SessionModeState>,
    pub config_options: Vec<SessionConfigOption>,
    pub available_commands: Vec<agent_client_protocol::schema::v1::AvailableCommand>,
    pub usage: Option<agent_client_protocol::schema::v1::UsageUpdate>,
}

impl Default for AcpPanelState {
    fn default() -> Self {
        Self {
            status: AcpConnectionStatus::Disconnected,
            agent_id: None,
            agent_name: None,
            session_id: None,
            transcript: AcpTranscript::default(),
            updates: Vec::new(),
            stderr_lines: Vec::new(),
            pending_permission: None,
            pending_attachments: Vec::new(),
            extensions: Vec::new(),
            modes: None,
            config_options: Vec::new(),
            available_commands: Vec::new(),
            usage: None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum AcpCommand {
    Prompt(Vec<ContentBlock>),
    NewSession,
    OpenSession(SessionId),
    SetMode(SessionModeId),
    SetConfig(SessionConfigId, SessionConfigOptionValue),
    Cancel,
    Shutdown,
}

#[derive(Clone, Debug)]
pub(crate) enum AcpEvent {
    Authenticating,
    Ready {
        agent_name: Option<String>,
        session_id: SessionId,
        modes: Option<SessionModeState>,
        config_options: Vec<SessionConfigOption>,
    },
    SessionUpdate(SessionUpdate),
    Extension(AcpExtensionEvent),
    OpeningSession {
        session_id: SessionId,
        replaying: bool,
    },
    SessionArchived(SessionId),
    Permission(PendingAcpPermission),
    PromptFinished(String),
    ModeChanged(SessionModeId),
    ConfigChanged(Vec<SessionConfigOption>),
    FileWritten {
        path: PathBuf,
        content: String,
    },
    Stderr(String),
    Failed(String),
    Stopped,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpExtensionEvent {
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Clone, Debug)]
pub(crate) struct PendingAcpPermission {
    pub request: RequestPermissionRequest,
    response_tx: async_channel::Sender<RequestPermissionOutcome>,
}

impl EditorView {
    pub(crate) fn start_selected_acp_agent(&mut self, cx: &mut Context<Self>) {
        if !matches!(
            self.acp_panel_state.status,
            AcpConnectionStatus::Disconnected | AcpConnectionStatus::Failed(_)
        ) {
            return;
        }
        let Some(agent) = self.selected_registry_agent().cloned() else {
            self.notify_error("Select an ACP agent first.", cx);
            return;
        };
        let Some(recipe) = self.agent_launch_recipe(&agent) else {
            self.notify_error(
                format!("{} must be installed before it can start.", agent.name),
                cx,
            );
            return;
        };
        let Some(cwd) = self.chat_panel_working_directory() else {
            self.notify_error("Open a workspace before starting an ACP agent.", cx);
            return;
        };
        let additional_directories = self
            .workspaces
            .iter()
            .filter(|root| **root != cwd)
            .cloned()
            .collect::<Vec<_>>();
        self.persist_active_tab_state();
        let unsaved = self
            .all_tabs()
            .filter_map(|tab| {
                let path = std::fs::canonicalize(tab.path.as_ref()?).ok()?;
                tab.file_kind
                    .as_ref()
                    .is_some_and(super::FileKind::supports_text_buffer)
                    .then(|| (path, tab.snapshot.text.clone()))
            })
            .collect::<HashMap<_, _>>();

        self.stop_acp_agent();
        self.acp_panel_state = AcpPanelState {
            status: AcpConnectionStatus::Connecting,
            agent_id: Some(agent.id.clone()),
            agent_name: Some(agent.name.clone()),
            ..AcpPanelState::default()
        };
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        self.acp_command_tx = Some(command_tx);

        cx.background_executor()
            .spawn(async move {
                if let Err(error) = run_acp_connection(
                    recipe,
                    cwd,
                    additional_directories,
                    unsaved,
                    command_rx,
                    event_tx.clone(),
                )
                .await
                {
                    let _ = event_tx.send(AcpEvent::Failed(error)).await;
                }
            })
            .detach();

        self.acp_event_task = Some(cx.spawn(async move |this, cx| {
            while let Ok(event) = event_rx.recv().await {
                let stopped = matches!(event, AcpEvent::Stopped);
                if this
                    .update(cx, |this, cx| {
                        this.apply_acp_event(event, cx);
                        cx.notify();
                    })
                    .is_err()
                    || stopped
                {
                    break;
                }
            }
        }));
        cx.notify();
    }

    pub(crate) fn send_acp_prompt(&mut self, prompt: String, cx: &mut Context<Self>) {
        let prompt = prompt.trim().to_string();
        if (prompt.is_empty() && self.acp_panel_state.pending_attachments.is_empty())
            || self.acp_panel_state.status != AcpConnectionStatus::Ready
        {
            return;
        }
        let Some(sender) = self.acp_command_tx.as_ref() else {
            return;
        };
        let mut blocks = if prompt.is_empty() {
            Vec::new()
        } else {
            vec![ContentBlock::Text(TextContent::new(prompt.clone()))]
        };
        let mut attachment_links = Vec::new();
        for attachment in self.acp_panel_state.pending_attachments.drain(..) {
            if let ContentBlock::ResourceLink(resource) = &attachment {
                attachment_links.push((resource.name.clone(), resource.uri.clone()));
            }
            blocks.push(attachment);
        }
        self.acp_panel_state
            .transcript
            .push_user_prompt(&prompt, attachment_links);
        self.acp_panel_state.status = AcpConnectionStatus::Prompting;
        if sender.try_send(AcpCommand::Prompt(blocks)).is_err() {
            self.acp_panel_state.status =
                AcpConnectionStatus::Failed("ACP command channel closed".to_string());
        }
        self.sync_agent_transcript_editor(cx);
        self.queue_active_acp_session_save(cx);
        cx.notify();
    }

    pub(crate) fn new_acp_session(&mut self, cx: &mut Context<Self>) {
        if !matches!(
            self.acp_panel_state.status,
            AcpConnectionStatus::Ready | AcpConnectionStatus::Failed(_)
        ) {
            return;
        }
        self.persist_active_acp_session(cx);
        self.cancel_pending_acp_permission();
        self.acp_panel_state.session_id = None;
        self.acp_panel_state.transcript.clear();
        self.acp_panel_state.updates.clear();
        self.acp_panel_state.extensions.clear();
        self.acp_panel_state.pending_attachments.clear();
        self.acp_panel_state.status = AcpConnectionStatus::Connecting;
        self.sync_agent_transcript_editor(cx);
        if let Some(composer) = self.agent_composer_editor.clone() {
            composer.update(cx, |composer, cx| composer.clear(cx));
        }
        if self
            .acp_command_tx
            .as_ref()
            .is_none_or(|sender| sender.try_send(AcpCommand::NewSession).is_err())
        {
            self.acp_panel_state.status =
                AcpConnectionStatus::Failed("ACP command channel closed".to_string());
        }
        cx.notify();
    }

    pub(crate) fn add_acp_resource_link(
        &mut self,
        path: &std::path::Path,
        title: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let mut resource = ResourceLink::new(name.clone(), file_uri(path));
        if let Some(title) = title {
            resource = resource.title(title);
        }
        if let Ok(metadata) = std::fs::metadata(path) {
            resource = resource.size(i64::try_from(metadata.len()).ok());
        }
        self.acp_panel_state
            .pending_attachments
            .push(ContentBlock::ResourceLink(resource));
        self.toggle_agent_panel_open(cx);
        if let Some(composer) = self.agent_composer_editor.clone() {
            composer.update(cx, |composer, cx| {
                composer.append_text(&format!("[{name}] "), cx)
            });
        }
        cx.notify();
    }

    fn toggle_agent_panel_open(&mut self, cx: &mut Context<Self>) {
        if !self.is_utility_panel_active(super::RightUtilityPanel::Agent) {
            self.toggle_agent_panel(cx);
        }
    }

    pub(crate) fn cancel_acp_prompt(&mut self, cx: &mut Context<Self>) {
        self.cancel_pending_acp_permission();
        if let Some(sender) = &self.acp_command_tx {
            let _ = sender.try_send(AcpCommand::Cancel);
        }
        cx.notify();
    }

    pub(crate) fn set_acp_mode(&mut self, mode_id: SessionModeId, cx: &mut Context<Self>) {
        if let Some(sender) = &self.acp_command_tx {
            let _ = sender.try_send(AcpCommand::SetMode(mode_id));
        }
        cx.notify();
    }

    pub(crate) fn set_acp_config(
        &mut self,
        config_id: SessionConfigId,
        value: SessionConfigOptionValue,
        cx: &mut Context<Self>,
    ) {
        if let Some(sender) = &self.acp_command_tx {
            let _ = sender.try_send(AcpCommand::SetConfig(config_id, value));
        }
        cx.notify();
    }

    pub(crate) fn stop_acp_agent(&mut self) {
        self.cancel_pending_acp_permission();
        if let Some(sender) = self.acp_command_tx.take() {
            let _ = sender.try_send(AcpCommand::Shutdown);
        }
        self.acp_event_task = None;
    }

    pub(crate) fn select_acp_permission(
        &mut self,
        option_id: agent_client_protocol::schema::v1::PermissionOptionId,
        cx: &mut Context<Self>,
    ) {
        let Some(permission) = self.acp_panel_state.pending_permission.take() else {
            return;
        };
        let _ = permission
            .response_tx
            .try_send(RequestPermissionOutcome::Selected(
                SelectedPermissionOutcome::new(option_id),
            ));
        cx.notify();
    }

    fn cancel_pending_acp_permission(&mut self) {
        if let Some(permission) = self.acp_panel_state.pending_permission.take() {
            let _ = permission
                .response_tx
                .try_send(RequestPermissionOutcome::Cancelled);
        }
    }

    fn apply_acp_event(&mut self, event: AcpEvent, cx: &mut Context<Self>) {
        let should_persist = !matches!(&event, AcpEvent::Stderr(_));
        match event {
            AcpEvent::Authenticating => {
                self.acp_panel_state.status = AcpConnectionStatus::Authenticating;
            }
            AcpEvent::Ready {
                agent_name,
                session_id,
                modes,
                config_options,
            } => {
                self.acp_panel_state.status = AcpConnectionStatus::Ready;
                self.acp_panel_state.agent_name = agent_name.or_else(|| {
                    self.selected_registry_agent()
                        .map(|agent| agent.name.clone())
                });
                self.acp_panel_state.session_id = Some(session_id);
                self.acp_panel_state.modes = modes;
                self.acp_panel_state.config_options = config_options;
                if let Some(pending) = self.pending_acp_open_session.clone() {
                    self.acp_panel_state.transcript = pending.restored_transcript();
                    self.acp_panel_state.updates = pending.updates.clone();
                    self.acp_panel_state.extensions = pending.extensions.clone();
                    if let Some(composer) = self.agent_composer_editor.clone() {
                        composer.update(cx, |editor, cx| {
                            editor.set_text(&pending.composer_draft, cx)
                        });
                    }
                    self.send_pending_acp_open_session(cx);
                }
            }
            AcpEvent::SessionUpdate(update) => {
                match &update {
                    SessionUpdate::AvailableCommandsUpdate(commands) => {
                        self.acp_panel_state.available_commands =
                            commands.available_commands.clone();
                    }
                    SessionUpdate::CurrentModeUpdate(current) => {
                        if let Some(modes) = self.acp_panel_state.modes.as_mut() {
                            modes.current_mode_id = current.current_mode_id.clone();
                        }
                    }
                    SessionUpdate::ConfigOptionUpdate(options) => {
                        self.acp_panel_state.config_options = options.config_options.clone();
                    }
                    SessionUpdate::UsageUpdate(usage) => {
                        self.acp_panel_state.usage = Some(usage.clone());
                    }
                    _ => {}
                }
                self.acp_panel_state.transcript.apply_update(&update);
                self.acp_panel_state.updates.push(update);
            }
            AcpEvent::Extension(extension) => {
                self.acp_panel_state.extensions.push(extension);
            }
            AcpEvent::OpeningSession {
                session_id,
                replaying,
            } => {
                self.pending_acp_open_session = None;
                self.acp_panel_state.session_id = Some(session_id);
                if replaying {
                    self.acp_panel_state.transcript.clear();
                    self.acp_panel_state.updates.clear();
                }
            }
            AcpEvent::SessionArchived(session_id) => {
                self.pending_acp_open_session = None;
                self.acp_panel_state.session_id = Some(session_id);
                self.acp_panel_state.status = AcpConnectionStatus::Disconnected;
            }
            AcpEvent::Permission(permission) => {
                self.acp_panel_state.pending_permission = Some(permission);
            }
            AcpEvent::PromptFinished(stop_reason) => {
                self.acp_panel_state.status = AcpConnectionStatus::Ready;
                if stop_reason != "end_turn" {
                    self.acp_panel_state
                        .transcript
                        .push_turn_ended(&stop_reason);
                }
            }
            AcpEvent::ModeChanged(mode_id) => {
                if let Some(modes) = self.acp_panel_state.modes.as_mut() {
                    modes.current_mode_id = mode_id;
                }
            }
            AcpEvent::ConfigChanged(options) => {
                self.acp_panel_state.config_options = options;
            }
            AcpEvent::FileWritten { path, content } => {
                self.apply_acp_file_write(&path, &content, cx);
            }
            AcpEvent::Stderr(line) => {
                self.acp_panel_state.stderr_lines.push(line);
                if self.acp_panel_state.stderr_lines.len() > 200 {
                    self.acp_panel_state.stderr_lines.remove(0);
                }
            }
            AcpEvent::Failed(error) => {
                self.cancel_pending_acp_permission();
                self.acp_panel_state.transcript.push_error(&error);
                self.acp_panel_state.status = AcpConnectionStatus::Failed(error);
                self.acp_command_tx = None;
            }
            AcpEvent::Stopped => {
                self.cancel_pending_acp_permission();
                self.acp_panel_state.status = AcpConnectionStatus::Disconnected;
                self.acp_command_tx = None;
            }
        }
        self.sync_agent_transcript_editor(cx);
        if should_persist {
            self.queue_active_acp_session_save(cx);
        }
    }

    pub(crate) fn sync_agent_transcript_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.agent_transcript_editor.clone() else {
            return;
        };
        let labels = super::acp_transcript::transcript_labels(self.ui_language);
        let transcript = self.acp_panel_state.transcript.to_markdown(&labels);
        editor.update(cx, |editor, cx| editor.set_text(&transcript, cx));
    }

    fn apply_acp_file_write(
        &mut self,
        path: &std::path::Path,
        content: &str,
        cx: &mut Context<Self>,
    ) {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        for tab in self.all_tabs_mut() {
            let matches = tab.path.as_ref().is_some_and(|tab_path| {
                std::fs::canonicalize(tab_path).unwrap_or_else(|_| tab_path.clone()) == canonical
            });
            if matches {
                tab.snapshot.text = content.to_string();
                tab.is_dirty = false;
            }
        }
        let active_matches = self.current_file_path.as_ref().is_some_and(|active_path| {
            std::fs::canonicalize(active_path).unwrap_or_else(|_| active_path.clone()) == canonical
        });
        if active_matches {
            self.buffer = crate::buffer::Buffer::from_text(content);
            self.cursor.offset = self.cursor.offset.min(self.buffer.len());
            self.cursor.anchor = self.cursor.anchor.min(self.buffer.len());
            self.reparse();
            self.layout_dirty = true;
        }
        self.sidebar_git_statuses_dirty = true;
        cx.notify();
    }
}

async fn run_acp_connection(
    recipe: AgentLaunchRecipe,
    cwd: PathBuf,
    additional_directories: Vec<PathBuf>,
    unsaved: HashMap<PathBuf, String>,
    command_rx: async_channel::Receiver<AcpCommand>,
    event_tx: async_channel::Sender<AcpEvent>,
) -> Result<(), String> {
    let server = McpServer::Stdio(
        McpServerStdio::new("agent", recipe.command)
            .args(recipe.args)
            .env(environment_variables(recipe.env)),
    );
    let debug_tx = event_tx.clone();
    let agent = AcpAgent::new(server).with_debug(move |line, direction| {
        if direction == agent_client_protocol::LineDirection::Stderr {
            let _ = debug_tx.try_send(AcpEvent::Stderr(line.to_string()));
        }
    });
    let update_tx = event_tx.clone();
    let extension_tx = event_tx.clone();
    let permission_tx = event_tx.clone();
    let file_event_tx = event_tx.clone();
    let mut roots = vec![cwd.clone()];
    roots.extend(additional_directories.clone());
    let services = AcpClientServices::new(roots, unsaved);
    let fs_read = services.clone();
    let fs_write = services.clone();
    let terminal_create = services.clone();
    let terminal_output = services.clone();
    let terminal_wait = services.clone();
    let terminal_kill = services.clone();
    let terminal_release = services.clone();

    agent_client_protocol::Client
        .builder()
        .name("moeka-acp-client")
        .on_receive_notification(
            async move |notification: AgentNotification, _connection| {
                match notification {
                    AgentNotification::SessionNotification(notification) => {
                        let _ = update_tx
                            .send(AcpEvent::SessionUpdate(notification.update))
                            .await;
                    }
                    AgentNotification::ExtNotification(notification) => {
                        let params = serde_json::from_str(notification.params.get())
                            .unwrap_or(serde_json::Value::Null);
                        let _ = extension_tx
                            .send(AcpEvent::Extension(AcpExtensionEvent {
                                method: notification.method.to_string(),
                                params,
                            }))
                            .await;
                    }
                    _ => {}
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _connection| {
                let (response_tx, response_rx) = async_channel::bounded(1);
                if permission_tx
                    .send(AcpEvent::Permission(PendingAcpPermission {
                        request,
                        response_tx,
                    }))
                    .await
                    .is_err()
                {
                    return responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    ));
                }
                let outcome = response_rx
                    .recv()
                    .await
                    .unwrap_or(RequestPermissionOutcome::Cancelled);
                responder.respond(RequestPermissionResponse::new(outcome))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ReadTextFileRequest, responder, connection| {
                let services = fs_read.clone();
                connection.spawn(async move {
                    match blocking::unblock(move || services.read_text_file(request)).await {
                        Ok(response) => responder.respond(response),
                        Err(error) => responder.respond_with_error(error),
                    }
                })?;
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: WriteTextFileRequest, responder, connection| {
                let services = fs_write.clone();
                let events = file_event_tx.clone();
                let path = request.path.clone();
                let content = request.content.clone();
                connection.spawn(async move {
                    match blocking::unblock(move || services.write_text_file(request)).await {
                        Ok(response) => {
                            let _ = events.send(AcpEvent::FileWritten { path, content }).await;
                            responder.respond(response)
                        }
                        Err(error) => responder.respond_with_error(error),
                    }
                })?;
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: CreateTerminalRequest, responder, connection| {
                let services = terminal_create.clone();
                connection.spawn(async move {
                    match blocking::unblock(move || services.create_terminal(request)).await {
                        Ok(response) => responder.respond(response),
                        Err(error) => responder.respond_with_error(error),
                    }
                })?;
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: TerminalOutputRequest, responder, connection| {
                let services = terminal_output.clone();
                connection.spawn(async move {
                    match blocking::unblock(move || services.terminal_output(request)).await {
                        Ok(response) => responder.respond(response),
                        Err(error) => responder.respond_with_error(error),
                    }
                })?;
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: WaitForTerminalExitRequest, responder, connection| {
                let services = terminal_wait.clone();
                connection.spawn(async move {
                    match blocking::unblock(move || services.wait_for_terminal_exit(request)).await
                    {
                        Ok(response) => responder.respond(response),
                        Err(error) => responder.respond_with_error(error),
                    }
                })?;
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: KillTerminalRequest, responder, connection| {
                let services = terminal_kill.clone();
                connection.spawn(async move {
                    match blocking::unblock(move || services.kill_terminal(request)).await {
                        Ok(response) => responder.respond(response),
                        Err(error) => responder.respond_with_error(error),
                    }
                })?;
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ReleaseTerminalRequest, responder, connection| {
                let services = terminal_release.clone();
                connection.spawn(async move {
                    match blocking::unblock(move || services.release_terminal(request)).await {
                        Ok(response) => responder.respond(response),
                        Err(error) => responder.respond_with_error(error),
                    }
                })?;
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            let client_capabilities = ClientCapabilities::new()
                .fs(FileSystemCapabilities::new()
                    .read_text_file(true)
                    .write_text_file(true))
                .terminal(true);
            let initialize = InitializeRequest::new(ProtocolVersion::V1)
                .client_capabilities(client_capabilities)
                .client_info(Implementation::new("moeka", APP_VERSION).title("Moeka"));
            let initialized = connection.send_request(initialize).block_task().await?;
            if initialized.protocol_version != ProtocolVersion::V1 {
                return Err(agent_client_protocol::util::internal_error(format!(
                    "unsupported ACP protocol version: {:?}",
                    initialized.protocol_version
                )));
            }

            if let Some(auth_method) = initialized.auth_methods.first() {
                let _ = event_tx.send(AcpEvent::Authenticating).await;
                connection
                    .send_request(AuthenticateRequest::new(auth_method.id().clone()))
                    .block_task()
                    .await?;
            }

            let supports_additional = initialized
                .agent_capabilities
                .session_capabilities
                .additional_directories
                .is_some();
            let supports_load = initialized.agent_capabilities.load_session;
            let supports_resume = initialized
                .agent_capabilities
                .session_capabilities
                .resume
                .is_some();
            let supports_close = initialized
                .agent_capabilities
                .session_capabilities
                .close
                .is_some();
            let mut session_request = NewSessionRequest::new(cwd.clone());
            if supports_additional {
                session_request =
                    session_request.additional_directories(additional_directories.clone());
            }
            let session = connection
                .send_request(session_request)
                .block_task()
                .await?;
            let mut session_id = session.session_id;
            let mut session_modes = session.modes;
            let mut session_config_options = session.config_options.unwrap_or_default();
            let display_name = initialized
                .agent_info
                .map(|info| info.title.unwrap_or(info.name));
            let _ = event_tx
                .send(AcpEvent::Ready {
                    agent_name: display_name.clone(),
                    session_id: session_id.clone(),
                    modes: session_modes.clone(),
                    config_options: session_config_options.clone(),
                })
                .await;

            while let Ok(command) = command_rx.recv().await {
                match command {
                    AcpCommand::Prompt(prompt) => {
                        let prompt_tx = event_tx.clone();
                        connection.spawn({
                            let connection = connection.clone();
                            let session_id = session_id.clone();
                            async move {
                                let result = connection
                                    .send_request(PromptRequest::new(session_id, prompt))
                                    .block_task()
                                    .await;
                                match result {
                                    Ok(response) => {
                                        let _ = prompt_tx
                                            .send(AcpEvent::PromptFinished(stop_reason_name(
                                                &response.stop_reason,
                                            )))
                                            .await;
                                    }
                                    Err(error) => {
                                        let _ = prompt_tx
                                            .send(AcpEvent::Failed(error.to_string()))
                                            .await;
                                    }
                                }
                                Ok(())
                            }
                        })?;
                    }
                    AcpCommand::NewSession => {
                        if supports_close {
                            connection
                                .send_request(CloseSessionRequest::new(session_id.clone()))
                                .block_task()
                                .await?;
                        }
                        let mut request = NewSessionRequest::new(cwd.clone());
                        if supports_additional {
                            request =
                                request.additional_directories(additional_directories.clone());
                        }
                        let session = connection.send_request(request).block_task().await?;
                        session_id = session.session_id;
                        session_modes = session.modes;
                        session_config_options = session.config_options.unwrap_or_default();
                        let _ = event_tx
                            .send(AcpEvent::Ready {
                                agent_name: display_name.clone(),
                                session_id: session_id.clone(),
                                modes: session_modes.clone(),
                                config_options: session_config_options.clone(),
                            })
                            .await;
                    }
                    AcpCommand::OpenSession(target_session_id) => {
                        if supports_close {
                            connection
                                .send_request(CloseSessionRequest::new(session_id.clone()))
                                .block_task()
                                .await?;
                        }
                        if supports_load {
                            let _ = event_tx
                                .send(AcpEvent::OpeningSession {
                                    session_id: target_session_id.clone(),
                                    replaying: true,
                                })
                                .await;
                            let mut request =
                                LoadSessionRequest::new(target_session_id.clone(), cwd.clone());
                            if supports_additional {
                                request =
                                    request.additional_directories(additional_directories.clone());
                            }
                            let response = connection.send_request(request).block_task().await?;
                            session_id = target_session_id;
                            session_modes = response.modes;
                            session_config_options = response.config_options.unwrap_or_default();
                            let _ = event_tx
                                .send(AcpEvent::Ready {
                                    agent_name: display_name.clone(),
                                    session_id: session_id.clone(),
                                    modes: session_modes.clone(),
                                    config_options: session_config_options.clone(),
                                })
                                .await;
                        } else if supports_resume {
                            let _ = event_tx
                                .send(AcpEvent::OpeningSession {
                                    session_id: target_session_id.clone(),
                                    replaying: false,
                                })
                                .await;
                            let mut request =
                                ResumeSessionRequest::new(target_session_id.clone(), cwd.clone());
                            if supports_additional {
                                request =
                                    request.additional_directories(additional_directories.clone());
                            }
                            let response = connection.send_request(request).block_task().await?;
                            session_id = target_session_id;
                            session_modes = response.modes;
                            session_config_options = response.config_options.unwrap_or_default();
                            let _ = event_tx
                                .send(AcpEvent::Ready {
                                    agent_name: display_name.clone(),
                                    session_id: session_id.clone(),
                                    modes: session_modes.clone(),
                                    config_options: session_config_options.clone(),
                                })
                                .await;
                        } else {
                            let _ = event_tx
                                .send(AcpEvent::SessionArchived(target_session_id))
                                .await;
                        }
                    }
                    AcpCommand::SetMode(mode_id) => {
                        connection
                            .send_request(SetSessionModeRequest::new(
                                session_id.clone(),
                                mode_id.clone(),
                            ))
                            .block_task()
                            .await?;
                        let _ = event_tx.send(AcpEvent::ModeChanged(mode_id)).await;
                    }
                    AcpCommand::SetConfig(config_id, value) => {
                        let response = connection
                            .send_request(SetSessionConfigOptionRequest::new(
                                session_id.clone(),
                                config_id,
                                value,
                            ))
                            .block_task()
                            .await?;
                        let _ = event_tx
                            .send(AcpEvent::ConfigChanged(response.config_options))
                            .await;
                    }
                    AcpCommand::Cancel => {
                        connection
                            .send_notification(CancelNotification::new(session_id.clone()))?;
                    }
                    AcpCommand::Shutdown => {
                        if supports_close {
                            let _ = connection
                                .send_request(CloseSessionRequest::new(session_id.clone()))
                                .block_task()
                                .await;
                        }
                        break;
                    }
                }
            }
            let _ = event_tx.send(AcpEvent::Stopped).await;
            Ok(())
        })
        .await
        .map_err(|error| error.to_string())
}

fn environment_variables(env: BTreeMap<String, String>) -> Vec<EnvVariable> {
    env.into_iter()
        .map(|(name, value)| EnvVariable::new(name, value))
        .collect()
}

fn file_uri(path: &std::path::Path) -> String {
    const FILE_URI_ENCODE_SET: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
        .add(b' ')
        .add(b'#')
        .add(b'%')
        .add(b'?');
    format!(
        "file://{}",
        percent_encoding::utf8_percent_encode(&path.to_string_lossy(), FILE_URI_ENCODE_SET)
    )
}

fn stop_reason_name(reason: &agent_client_protocol::schema::v1::StopReason) -> String {
    serde_json::to_value(reason)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn stdio_actor_initializes_streams_prompt_and_stops() {
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("mock_acp.py");
        fs::write(
            &script,
            r#"import json
import sys

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    request_id = message.get("id")
    if method == "initialize":
        result = {
            "protocolVersion": 1,
            "agentCapabilities": {},
            "agentInfo": {"name": "mock", "title": "Mock ACP", "version": "1"},
            "authMethods": []
        }
        print(json.dumps({"jsonrpc": "2.0", "id": request_id, "result": result}), flush=True)
    elif method == "session/new":
        print(json.dumps({"jsonrpc": "2.0", "id": request_id, "result": {"sessionId": "mock-session"}}), flush=True)
    elif method == "session/prompt":
        update = {
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "mock-session",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": "Hello from mock"}
                }
            }
        }
        print(json.dumps(update), flush=True)
        print(json.dumps({"jsonrpc": "2.0", "id": request_id, "result": {"stopReason": "end_turn"}}), flush=True)
"#,
        )
        .unwrap();

        let recipe = AgentLaunchRecipe {
            command: "python3".to_string(),
            args: vec![script.to_string_lossy().into_owned()],
            env: BTreeMap::new(),
        };
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();

        let (result, (saw_ready, saw_update, saw_finished, saw_stopped)) =
            futures::executor::block_on(async {
                futures::join!(
                    run_acp_connection(
                        recipe,
                        temp.path().to_path_buf(),
                        Vec::new(),
                        HashMap::new(),
                        command_rx,
                        event_tx,
                    ),
                    async move {
                        let mut saw_ready = false;
                        let mut saw_update = false;
                        let mut saw_finished = false;
                        let mut saw_stopped = false;
                        while let Ok(event) = event_rx.recv().await {
                            match event {
                                AcpEvent::Ready { session_id, .. } => {
                                    assert_eq!(session_id.0.as_ref(), "mock-session");
                                    saw_ready = true;
                                    command_tx
                                        .send(AcpCommand::Prompt(vec![ContentBlock::from("Hello")]))
                                        .await
                                        .unwrap();
                                }
                                AcpEvent::SessionUpdate(SessionUpdate::AgentMessageChunk(
                                    chunk,
                                )) => {
                                    assert!(matches!(
                                        chunk.content,
                                        ContentBlock::Text(ref text) if text.text == "Hello from mock"
                                    ));
                                    saw_update = true;
                                }
                                AcpEvent::PromptFinished(reason) => {
                                    assert_eq!(reason, "end_turn");
                                    saw_finished = true;
                                    command_tx.send(AcpCommand::Shutdown).await.unwrap();
                                }
                                AcpEvent::Stopped => {
                                    saw_stopped = true;
                                    break;
                                }
                                AcpEvent::Failed(error) => panic!("ACP actor failed: {error}"),
                                _ => {}
                            }
                        }
                        (saw_ready, saw_update, saw_finished, saw_stopped)
                    }
                )
            });

        assert!(result.is_ok(), "{result:?}");
        assert!(saw_ready && saw_update && saw_finished && saw_stopped);
    }

    #[test]
    fn stdio_actor_round_trips_permission_selection() {
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("mock_permission_acp.py");
        fs::write(
            &script,
            r#"import json
import sys

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    request_id = message.get("id")
    if method == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":request_id,"result":{"protocolVersion":1,"agentCapabilities":{},"authMethods":[]}}), flush=True)
    elif method == "session/new":
        print(json.dumps({"jsonrpc":"2.0","id":request_id,"result":{"sessionId":"permission-session"}}), flush=True)
    elif method == "session/prompt":
        permission_id = "permission-1"
        request = {
            "jsonrpc":"2.0",
            "id":permission_id,
            "method":"session/request_permission",
            "params":{
                "sessionId":"permission-session",
                "toolCall":{"toolCallId":"tool-1","title":"Write file"},
                "options":[{"optionId":"allow-once","name":"Allow once","kind":"allow_once"}]
            }
        }
        print(json.dumps(request), flush=True)
        response = json.loads(sys.stdin.readline())
        selected = response.get("result", {}).get("outcome", {}).get("optionId")
        if selected != "allow-once":
            print(json.dumps({"jsonrpc":"2.0","id":request_id,"error":{"code":-32603,"message":"permission not selected"}}), flush=True)
        else:
            print(json.dumps({"jsonrpc":"2.0","id":request_id,"result":{"stopReason":"end_turn"}}), flush=True)
"#,
        )
        .unwrap();

        let recipe = AgentLaunchRecipe {
            command: "python3".to_string(),
            args: vec![script.to_string_lossy().into_owned()],
            env: BTreeMap::new(),
        };
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();

        let (result, saw_permission) = futures::executor::block_on(async {
            futures::join!(
                run_acp_connection(
                    recipe,
                    temp.path().to_path_buf(),
                    Vec::new(),
                    HashMap::new(),
                    command_rx,
                    event_tx,
                ),
                async move {
                    let mut saw_permission = false;
                    while let Ok(event) = event_rx.recv().await {
                        match event {
                            AcpEvent::Ready { .. } => {
                                command_tx
                                    .send(AcpCommand::Prompt(vec![ContentBlock::from("Write")]))
                                    .await
                                    .unwrap();
                            }
                            AcpEvent::Permission(permission) => {
                                saw_permission = true;
                                let option_id = permission.request.options[0].option_id.clone();
                                permission
                                    .response_tx
                                    .send(RequestPermissionOutcome::Selected(
                                        SelectedPermissionOutcome::new(option_id),
                                    ))
                                    .await
                                    .unwrap();
                            }
                            AcpEvent::PromptFinished(reason) => {
                                assert_eq!(reason, "end_turn");
                                command_tx.send(AcpCommand::Shutdown).await.unwrap();
                            }
                            AcpEvent::Stopped => break,
                            AcpEvent::Failed(error) => panic!("ACP actor failed: {error}"),
                            _ => {}
                        }
                    }
                    saw_permission
                }
            )
        });

        assert!(result.is_ok(), "{result:?}");
        assert!(saw_permission);
    }
}
