use std::collections::VecDeque;
use std::io;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use moeka_fs::backend::{
    DirectoryEntry, ExpectedRevision, Filesystem, FsError, FsResult, MAX_READ_BYTES,
    MAX_SAVE_BYTES, Metadata, ReadChunk, Revision, SaveOutcome, SaveRequest,
};
use moeka_fs::path::{FilesystemId, NativePath};
use moeka_fs::protocol::PROTOCOL_VERSION;
use moeka_fs::service::{BuildIdentity, Envelope, Reply, Request, Response, SearchItem};
use sha2::{Digest, Sha256};

use crate::transport::FramedTransport;
use crate::{ServerPayload, SshConnection, SshOptions};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_IN_FLIGHT: usize = 4;
const MAX_RPC_WAITERS: usize = 32;

#[derive(Default)]
struct AdmissionState {
    busy: bool,
    next_ticket: u64,
    waiters: VecDeque<(u64, bool)>,
}

#[derive(Default)]
struct Admission {
    state: Mutex<AdmissionState>,
    available: Condvar,
}

struct RpcPermit<'a>(&'a Admission);

impl Drop for RpcPermit<'_> {
    fn drop(&mut self) {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.busy = false;
        self.0.available.notify_all();
    }
}

impl Admission {
    fn acquire(&self, deadline: Instant, foreground: bool) -> FsResult<RpcPermit<'_>> {
        let error =
            |kind, cause| FsError::from_io("ssh_admission", None, io::Error::new(kind, cause));
        let mut state = self
            .state
            .lock()
            .map_err(|_| error(io::ErrorKind::Other, "SSH admission lock was poisoned"))?;
        if state.waiters.len() >= MAX_RPC_WAITERS
            || !foreground
                && state
                    .waiters
                    .iter()
                    .filter(|(_, foreground)| !foreground)
                    .count()
                    >= MAX_RPC_WAITERS - MAX_IN_FLIGHT
        {
            return Err(error(
                io::ErrorKind::WouldBlock,
                "SSH request queue is full",
            ));
        }
        let ticket = state.next_ticket;
        state.next_ticket = ticket
            .checked_add(1)
            .ok_or_else(|| error(io::ErrorKind::Other, "SSH admission IDs exhausted"))?;
        state.waiters.push_back((ticket, foreground));
        self.available.notify_all();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                state.waiters.retain(|(id, _)| *id != ticket);
                self.available.notify_all();
                return Err(error(
                    io::ErrorKind::TimedOut,
                    "SSH request expired before dispatch",
                ));
            }
            let next = state
                .waiters
                .iter()
                .find(|(_, foreground)| *foreground)
                .or_else(|| state.waiters.front());
            if !state.busy && next.is_some_and(|(id, _)| *id == ticket) {
                state.waiters.retain(|(id, _)| *id != ticket);
                state.busy = true;
                return Ok(RpcPermit(self));
            }
            state = self
                .available
                .wait_timeout(state, remaining)
                .map_err(|_| error(io::ErrorKind::Other, "SSH admission lock was poisoned"))?
                .0;
        }
    }
}

fn foreground_request(request: &Request) -> bool {
    !matches!(
        request,
        Request::StartSearch { .. }
            | Request::StartFilteredSearch { .. }
            | Request::NextSearch { .. }
            | Request::StartWatch { .. }
            | Request::PollWatch { .. }
            | Request::OpenDirectory { .. }
            | Request::NextDirectory { .. }
            | Request::ReadPreview { .. }
    )
}

struct RpcState {
    transport: FramedTransport,
    next_id: u64,
}

impl RpcState {
    fn request(&mut self, request: Request, timeout: Duration) -> FsResult<Response> {
        self.batch(vec![request], timeout)?
            .pop()
            .ok_or_else(|| FsError::invalid("ssh_request", "missing response"))?
    }

    fn batch(
        &mut self,
        requests: Vec<Request>,
        timeout: Duration,
    ) -> FsResult<Vec<FsResult<Response>>> {
        if requests.is_empty() || requests.len() > MAX_IN_FLIGHT {
            return Err(FsError::invalid(
                "ssh_request",
                "invalid in-flight request count",
            ));
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| FsError::invalid("ssh_request", "invalid request deadline"))?;
        let first = self.next_id;
        let end = first
            .checked_add(requests.len() as u64)
            .ok_or_else(|| FsError::invalid("ssh_request", "request IDs exhausted"))?;
        self.next_id = end;
        for (id, request) in (first..end).zip(requests) {
            if let Err(error) = self.transport.send(
                &Envelope { id, request },
                deadline.saturating_duration_since(Instant::now()),
            ) {
                self.transport.close();
                return Err(FsError::from_io("ssh_send", None, error));
            }
        }
        let mut responses = Vec::with_capacity((end - first) as usize);
        for id in first..end {
            let reply: Reply = self
                .transport
                .receive(deadline.saturating_duration_since(Instant::now()))
                .map_err(|error| FsError::from_io("ssh_receive", None, error))?;
            if reply.id != id {
                self.transport.close();
                return Err(FsError::from_io(
                    "ssh_receive",
                    None,
                    io::Error::new(io::ErrorKind::InvalidData, "SSH reply identity mismatch"),
                ));
            }
            responses.push(reply.result);
        }
        Ok(responses)
    }
}

pub struct SshFilesystem {
    identity: FilesystemId,
    state: Mutex<RpcState>,
    admission: Admission,
    build: BuildIdentity,
    capabilities: Vec<String>,
}

impl SshFilesystem {
    /// Connect only from a bounded background task, never from a UI callback.
    pub fn connect(
        options: &SshOptions,
        payload: &ServerPayload<'_>,
        expected_build: Option<BuildIdentity>,
    ) -> FsResult<Self> {
        let connection = SshConnection::start(options, payload)
            .map_err(|error| FsError::from_io("ssh_connect", None, error))?;
        let transport = FramedTransport::new(connection)
            .map_err(|error| FsError::from_io("ssh_transport", None, error))?;
        let mut state = RpcState {
            transport,
            next_id: 1,
        };
        let response = state.request(
            Request::Hello {
                protocol: PROTOCOL_VERSION,
                expected_build,
            },
            REQUEST_TIMEOUT,
        )?;
        let Response::Hello {
            protocol,
            build,
            capabilities,
        } = response
        else {
            return Err(FsError::invalid(
                "ssh_hello",
                "unexpected handshake response",
            ));
        };
        if protocol != PROTOCOL_VERSION {
            return Err(FsError::unsupported(
                "ssh_hello",
                "filesystem protocol mismatch",
            ));
        }
        let config_path = options.config_file.as_deref().map(NativePath::from_local);
        let encoded = serde_json::to_vec(&(&options.destination, options.port, config_path))
            .map_err(|error| FsError::invalid("ssh_identity", &error.to_string()))?;
        let identity = FilesystemId::Ssh(Sha256::digest(encoded).into());
        Ok(Self {
            identity,
            state: Mutex::new(state),
            admission: Admission::default(),
            build,
            capabilities,
        })
    }

    pub fn identity(&self) -> &FilesystemId {
        &self.identity
    }

    pub fn build(&self) -> &BuildIdentity {
        &self.build
    }

    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    /// Ephemeral host/kernel, namespace, root and user scope; not host authentication.
    pub fn peer_identity(&self) -> FsResult<[u8; 32]> {
        if !self
            .capabilities
            .iter()
            .any(|capability| capability == "peer_instance_binding")
        {
            return Err(FsError::unsupported(
                "ssh_peer_identity",
                "execution-scope binding is unavailable",
            ));
        }
        match self.request(Request::PeerIdentity)? {
            Response::PeerIdentity(identity) => Ok(identity),
            _ => Err(FsError::invalid(
                "ssh_peer_identity",
                "unexpected execution-scope response",
            )),
        }
    }

    /// Pair live channels only after both authenticated peers prove the same scope.
    pub fn verify_peer(&self, other: &Self) -> FsResult<()> {
        if self.identity != other.identity || self.peer_identity()? != other.peer_identity()? {
            return Err(FsError::invalid(
                "ssh_peer_identity",
                "SSH channels refer to different execution scopes",
            ));
        }
        Ok(())
    }

    pub fn request(&self, request: Request) -> FsResult<Response> {
        self.request_with_timeout(request, REQUEST_TIMEOUT)
    }

    pub fn request_with_timeout(&self, request: Request, timeout: Duration) -> FsResult<Response> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| FsError::invalid("ssh_request", "invalid request deadline"))?;
        let _permit = self
            .admission
            .acquire(deadline, foreground_request(&request))?;
        self.state
            .lock()
            .map_err(|_| {
                FsError::from_io(
                    "ssh_request",
                    None,
                    io::Error::other("SSH state lock was poisoned"),
                )
            })?
            .request(request, deadline.saturating_duration_since(Instant::now()))
    }

    fn batch(&self, requests: Vec<Request>) -> FsResult<Vec<FsResult<Response>>> {
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let _permit = self
            .admission
            .acquire(deadline, requests.iter().any(foreground_request))?;
        self.state
            .lock()
            .map_err(|_| {
                FsError::from_io(
                    "ssh_request",
                    None,
                    io::Error::other("SSH state lock was poisoned"),
                )
            })?
            .batch(requests, deadline.saturating_duration_since(Instant::now()))
    }

    pub fn next_directory_pages(
        &self,
        session: u64,
    ) -> FsResult<(Vec<FsResult<DirectoryEntry>>, bool)> {
        let count = self.page_credit_window();
        let mut entries = Vec::new();
        let mut completed = false;
        for reply in self.batch(vec![Request::NextDirectory { session }; count])? {
            let Response::Directory {
                entries: page,
                done,
            } = reply?
            else {
                return Err(FsError::invalid(
                    "ssh_directory",
                    "unexpected directory response",
                ));
            };
            if completed && (!done || !page.is_empty()) {
                return Err(FsError::invalid(
                    "ssh_directory",
                    "directory continued after completion",
                ));
            }
            entries.extend(page);
            completed = done;
        }
        Ok((entries, completed))
    }

    pub fn next_search_pages(&self, session: u64) -> FsResult<(Vec<SearchItem>, bool)> {
        let count = self.page_credit_window();
        let mut items = Vec::new();
        let mut completed = false;
        for reply in self.batch(vec![Request::NextSearch { session }; count])? {
            let Response::Search { items: page, done } = reply? else {
                return Err(FsError::invalid("ssh_search", "unexpected search response"));
            };
            if completed && (!done || !page.is_empty()) {
                return Err(FsError::invalid(
                    "ssh_search",
                    "search continued after completion",
                ));
            }
            items.extend(page);
            completed = done;
        }
        Ok((items, completed))
    }

    fn page_credit_window(&self) -> usize {
        if self
            .capabilities
            .iter()
            .any(|capability| capability == "page_credit_window")
        {
            MAX_IN_FLIGHT
        } else {
            1
        }
    }

    pub fn read_file(&self, path: &NativePath, max_bytes: usize) -> FsResult<(Vec<u8>, Revision)> {
        let Response::ReadOpened { session, revision } =
            self.request(Request::OpenRead { path: path.clone() })?
        else {
            return Err(FsError::invalid("ssh_open_read", "unexpected response"));
        };
        let result = (|| {
            if revision.size > max_bytes as u64 {
                return Err(FsError::invalid(
                    "ssh_read",
                    "file exceeds the caller's memory budget",
                ));
            }
            let mut bytes = Vec::new();
            loop {
                let remaining = revision.size.saturating_sub(bytes.len() as u64);
                let count = remaining
                    .div_ceil(MAX_READ_BYTES as u64)
                    .clamp(1, MAX_IN_FLIGHT as u64) as usize;
                let requests = (0..count)
                    .map(|_| Request::NextRead {
                        session,
                        length: MAX_READ_BYTES,
                    })
                    .collect();
                for (index, response) in self.batch(requests)?.into_iter().enumerate() {
                    let Response::Read(chunk) = response? else {
                        return Err(FsError::invalid("ssh_read", "unexpected read response"));
                    };
                    if chunk.offset != bytes.len() as u64
                        || chunk.revision != revision
                        || chunk.bytes.len() > MAX_READ_BYTES
                        || chunk.bytes.len() > max_bytes.saturating_sub(bytes.len())
                        || (!chunk.eof && chunk.bytes.is_empty())
                        || (chunk.eof && index + 1 != count)
                    {
                        return Err(FsError::invalid("ssh_read", "inconsistent read stream"));
                    }
                    bytes.extend_from_slice(&chunk.bytes);
                    if chunk.eof {
                        let digest: [u8; 32] = Sha256::digest(&bytes).into();
                        if bytes.len() as u64 != revision.size || digest != revision.sha256 {
                            return Err(FsError::invalid(
                                "ssh_read",
                                "read stream integrity mismatch",
                            ));
                        }
                        return Ok((bytes, revision));
                    }
                }
            }
        })();
        result.map_err(|error| self.cancel_failed_session(session, error))
    }

    pub fn save_bytes(
        &self,
        operation_id: String,
        path: NativePath,
        expected: ExpectedRevision,
        bytes: &[u8],
    ) -> FsResult<SaveOutcome> {
        if bytes.len() <= MAX_SAVE_BYTES {
            return self.save(SaveRequest {
                operation_id,
                path,
                expected,
                bytes: bytes.to_vec(),
            });
        }
        let Response::Session(session) = self.request(Request::BeginSave {
            operation_id,
            path,
            expected,
            total_bytes: bytes.len() as u64,
        })?
        else {
            return Err(FsError::invalid("ssh_begin_save", "unexpected response"));
        };
        let result = (|| {
            for (window, bytes) in bytes.chunks(MAX_READ_BYTES * MAX_IN_FLIGHT).enumerate() {
                let requests = bytes
                    .chunks(MAX_READ_BYTES)
                    .enumerate()
                    .map(|(index, chunk)| Request::AppendSave {
                        session,
                        offset: ((window * MAX_IN_FLIGHT + index) * MAX_READ_BYTES) as u64,
                        bytes: chunk.to_vec(),
                    })
                    .collect();
                for response in self.batch(requests)? {
                    if !matches!(response?, Response::Unit) {
                        return Err(FsError::invalid("ssh_append_save", "unexpected response"));
                    }
                }
            }
            match self.request(Request::CommitSave { session })? {
                Response::Save(outcome) => Ok(outcome),
                _ => Err(FsError::invalid("ssh_commit_save", "unexpected response")),
            }
        })();
        result.map_err(|error| self.cancel_failed_session(session, error))
    }

    fn cancel_failed_session(&self, session: u64, mut error: FsError) -> FsError {
        if let Err(cleanup) = self.unit(Request::Cancel { session }) {
            error.message.push_str(&format!(
                "; session cleanup: {}: {}",
                cleanup.operation, cleanup.message
            ));
        }
        error
    }

    fn unit(&self, request: Request) -> FsResult<()> {
        match self.request(request)? {
            Response::Unit => Ok(()),
            _ => Err(FsError::invalid("ssh_response", "expected unit response")),
        }
    }
}

impl Filesystem for SshFilesystem {
    fn create_symlink(
        &self,
        target: &NativePath,
        link: &NativePath,
        directory: Option<bool>,
    ) -> FsResult<()> {
        if !self
            .capabilities
            .iter()
            .any(|capability| capability == "create_symlink")
        {
            return Err(FsError::unsupported(
                "ssh_symlink",
                "server cannot create symbolic links",
            ));
        }
        self.unit(Request::CreateSymlink {
            target: target.clone(),
            link: link.clone(),
            directory,
        })
    }

    fn read_previews(
        &self,
        paths: &[NativePath],
        length: usize,
    ) -> FsResult<Vec<FsResult<moeka_fs::backend::ReadPreview>>> {
        if paths.len() > moeka_fs::backend::MAX_BATCH
            || length == 0
            || length > moeka_fs::backend::MAX_READ_BYTES
        {
            return Err(FsError::invalid("ssh_previews", "invalid preview batch"));
        }
        let mut results = Vec::with_capacity(paths.len());
        for paths in paths.chunks(MAX_IN_FLIGHT) {
            let requests = paths
                .iter()
                .map(|path| Request::ReadPreview {
                    path: path.clone(),
                    length,
                })
                .collect();
            for response in self.batch(requests)? {
                results.push(response.and_then(|response| match response {
                    Response::Preview(preview) if preview.bytes.len() <= length => Ok(preview),
                    _ => Err(FsError::invalid(
                        "ssh_preview",
                        "unexpected preview response",
                    )),
                }));
            }
        }
        Ok(results)
    }

    fn read_preview(
        &self,
        path: &NativePath,
        length: usize,
    ) -> FsResult<moeka_fs::backend::ReadPreview> {
        match self.request(Request::ReadPreview {
            path: path.clone(),
            length,
        })? {
            Response::Preview(preview) if preview.bytes.len() <= length => Ok(preview),
            _ => Err(FsError::invalid(
                "ssh_preview",
                "unexpected preview response",
            )),
        }
    }

    fn canonicalize(&self, path: &NativePath) -> FsResult<NativePath> {
        match self.request(Request::Canonicalize { path: path.clone() })? {
            Response::Path(path) => Ok(path),
            _ => Err(FsError::invalid(
                "ssh_canonicalize",
                "unexpected path response",
            )),
        }
    }

    fn read_link(&self, path: &NativePath) -> FsResult<NativePath> {
        match self.request(Request::ReadLink { path: path.clone() })? {
            Response::Path(path) => Ok(path),
            _ => Err(FsError::invalid(
                "ssh_read_link",
                "unexpected path response",
            )),
        }
    }

    fn metadata(&self, path: &NativePath, follow: bool) -> FsResult<Metadata> {
        match self.request(Request::Metadata {
            path: path.clone(),
            follow,
        })? {
            Response::Metadata(metadata) => Ok(metadata),
            _ => Err(FsError::invalid("ssh_metadata", "unexpected response")),
        }
    }

    fn metadata_batch(
        &self,
        paths: &[NativePath],
        follow: bool,
    ) -> FsResult<Vec<FsResult<Metadata>>> {
        match self.request(Request::MetadataBatch {
            paths: paths.to_vec(),
            follow,
        })? {
            Response::MetadataBatch(metadata) if metadata.len() == paths.len() => Ok(metadata),
            _ => Err(FsError::invalid(
                "ssh_metadata_batch",
                "unexpected response",
            )),
        }
    }

    fn read_chunk(&self, path: &NativePath, offset: u64, length: usize) -> FsResult<ReadChunk> {
        match self.request(Request::Read {
            path: path.clone(),
            offset,
            length,
        })? {
            Response::Read(chunk) if chunk.offset == offset && chunk.bytes.len() <= length => {
                Ok(chunk)
            }
            _ => Err(FsError::invalid("ssh_read", "unexpected response")),
        }
    }

    fn create_file(&self, path: &NativePath) -> FsResult<()> {
        self.unit(Request::CreateFile { path: path.clone() })
    }

    fn create_dir(&self, path: &NativePath, recursive: bool) -> FsResult<()> {
        self.unit(Request::CreateDirectory {
            path: path.clone(),
            recursive,
        })
    }

    fn copy_file(&self, source: &NativePath, target: &NativePath) -> FsResult<u64> {
        match self.request(Request::CopyFile {
            source: source.clone(),
            target: target.clone(),
        })? {
            Response::Copied(bytes) => Ok(bytes),
            _ => Err(FsError::invalid("ssh_copy", "unexpected response")),
        }
    }

    fn rename(&self, source: &NativePath, target: &NativePath) -> FsResult<()> {
        self.unit(Request::Rename {
            source: source.clone(),
            target: target.clone(),
        })
    }

    fn trash(&self, path: &NativePath) -> FsResult<()> {
        self.unit(Request::Trash { path: path.clone() })
    }

    fn save(&self, request: SaveRequest) -> FsResult<SaveOutcome> {
        if request.bytes.len() > MAX_SAVE_BYTES {
            return self.save_bytes(
                request.operation_id,
                request.path,
                request.expected,
                &request.bytes,
            );
        }
        match self.request(Request::Save { save: request })? {
            Response::Save(outcome) => Ok(outcome),
            _ => Err(FsError::invalid("ssh_save", "unexpected response")),
        }
    }

    fn save_outcome(&self, operation_id: &str) -> SaveOutcome {
        match self.request(Request::SaveOutcome {
            operation_id: operation_id.into(),
        }) {
            Ok(Response::Save(outcome)) => outcome,
            Ok(_) => {
                SaveOutcome::Failed(FsError::invalid("ssh_save_outcome", "unexpected response"))
            }
            Err(error) => SaveOutcome::Uncertain(error),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use moeka_fs::protocol::write_frame;
    use std::process::Command;

    fn wait_for_waiters(admission: &Admission, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut state = admission.state.lock().unwrap();
        while state.waiters.len() != count {
            let result = admission
                .available
                .wait_timeout(state, deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            state = result.0;
            assert!(!result.1.timed_out(), "request was not queued");
        }
    }

    #[test]
    fn foreground_requests_precede_queued_scans_without_wall_clock_races() {
        let admission = Admission::default();
        let first = admission
            .acquire(Instant::now() + Duration::from_secs(5), true)
            .unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let sender = &sender;
            let admission = &admission;
            scope.spawn(move || {
                let _permit = admission
                    .acquire(Instant::now() + Duration::from_secs(5), false)
                    .unwrap();
                sender.send("background").unwrap();
            });
            wait_for_waiters(admission, 1);
            scope.spawn(move || {
                let _permit = admission
                    .acquire(Instant::now() + Duration::from_secs(5), true)
                    .unwrap();
                sender.send("foreground").unwrap();
            });
            wait_for_waiters(admission, 2);
            drop(first);
            assert_eq!(
                receiver.recv_timeout(Duration::from_secs(5)).unwrap(),
                "foreground"
            );
            assert_eq!(
                receiver.recv_timeout(Duration::from_secs(5)).unwrap(),
                "background"
            );
        });
    }

    #[test]
    fn expired_queued_requests_do_not_send_or_invalidate_a_healthy_connection() {
        let request = Request::Metadata {
            path: NativePath::Unix(b"x".to_vec()),
            follow: false,
        };
        let remote = SshFilesystem {
            identity: FilesystemId::Ssh([0; 32]),
            state: Mutex::new(state(std::slice::from_ref(&request), false)),
            admission: Admission::default(),
            build: BuildIdentity {
                server_version: "test".into(),
                app_version: "test".into(),
                source_sha: "test".into(),
            },
            capabilities: Vec::new(),
        };
        let permit = remote
            .admission
            .acquire(Instant::now() + Duration::from_secs(5), true)
            .unwrap();
        assert_eq!(
            remote
                .request_with_timeout(request.clone(), Duration::ZERO)
                .unwrap_err()
                .kind,
            "TimedOut"
        );
        drop(permit);
        assert!(matches!(remote.request(request).unwrap(), Response::Unit));
    }

    fn state(requests: &[Request], wrong_identity: bool) -> RpcState {
        state_with_response(requests, wrong_identity, Response::Unit)
    }

    fn state_with_response(
        requests: &[Request],
        wrong_identity: bool,
        response: Response,
    ) -> RpcState {
        let mut input = Vec::new();
        let mut replies = Vec::new();
        for (index, request) in requests.iter().enumerate() {
            let id = index as u64 + 1;
            write_frame(
                &mut input,
                &Envelope {
                    id,
                    request: request.clone(),
                },
            )
            .unwrap();
            write_frame(
                &mut replies,
                &Reply {
                    id: if wrong_identity && index == 1 {
                        999
                    } else {
                        id
                    },
                    result: Ok(response.clone()),
                },
            )
            .unwrap();
        }
        let escaped: String = replies.iter().map(|byte| format!("\\{byte:03o}")).collect();
        // The peer withholds every reply until the entire credit window arrives.
        let script = format!(
            "dd bs=1 count={} of=/dev/null 2>/dev/null; printf '{escaped}'",
            input.len(),
        );
        let mut command = Command::new("sh");
        command.args(["-c", &script]);
        RpcState {
            transport: FramedTransport::new(SshConnection::spawn(command).unwrap()).unwrap(),
            next_id: 1,
        }
    }

    #[test]
    fn channel_pairing_rejects_different_scopes_and_missing_proof() {
        let peer = |identity| SshFilesystem {
            identity: FilesystemId::Ssh([0; 32]),
            state: Mutex::new(state_with_response(
                &[Request::PeerIdentity],
                false,
                Response::PeerIdentity(identity),
            )),
            admission: Admission::default(),
            build: BuildIdentity {
                server_version: "test".into(),
                app_version: "test".into(),
                source_sha: "test".into(),
            },
            capabilities: vec!["peer_instance_binding".into()],
        };
        {
            let first = peer([1; 32]);
            let second = peer([1; 32]);
            first.verify_peer(&second).unwrap();
        }
        {
            let first = peer([1; 32]);
            let second = peer([2; 32]);
            assert_eq!(first.verify_peer(&second).unwrap_err().kind, "InvalidInput");
        }
        let mut unsupported = peer([1; 32]);
        unsupported.capabilities.clear();
        assert_eq!(unsupported.peer_identity().unwrap_err().kind, "Unsupported");
    }

    #[test]
    fn symlink_requests_require_server_capability_before_dispatch() {
        let target = NativePath::Unix(b"../missing\xff".to_vec());
        let link = NativePath::Unix(b"/workspace/link".to_vec());
        let request = Request::CreateSymlink {
            target: target.clone(),
            link: link.clone(),
            directory: None,
        };
        let mut remote = SshFilesystem {
            identity: FilesystemId::Ssh([0; 32]),
            state: Mutex::new(state(&[request], false)),
            admission: Admission::default(),
            build: BuildIdentity {
                server_version: "test".into(),
                app_version: "test".into(),
                source_sha: "test".into(),
            },
            capabilities: Vec::new(),
        };
        assert_eq!(
            remote
                .create_symlink(&target, &link, None)
                .unwrap_err()
                .kind,
            "Unsupported"
        );
        remote.capabilities.push("create_symlink".into());
        remote.create_symlink(&target, &link, None).unwrap();
    }

    #[test]
    fn batches_fill_a_bounded_credit_window_before_waiting_for_responses() {
        let request = Request::Metadata {
            path: NativePath::Unix(b"x".to_vec()),
            follow: false,
        };
        let requests = vec![request.clone(); MAX_IN_FLIGHT];
        let mut state = state(&requests, false);
        assert!(
            state
                .batch(vec![request; MAX_IN_FLIGHT + 1], Duration::from_secs(5))
                .is_err()
        );
        let responses = state.batch(requests, Duration::from_secs(5)).unwrap();
        assert_eq!(responses.len(), MAX_IN_FLIGHT);
        assert!(
            responses
                .into_iter()
                .all(|response| matches!(response, Ok(Response::Unit)))
        );
    }

    #[test]
    fn a_mismatched_response_invalidates_all_pending_requests_without_replay() {
        let request = Request::Metadata {
            path: NativePath::Unix(b"x".to_vec()),
            follow: true,
        };
        let requests = vec![request.clone(); 2];
        let mut state = state(&requests, true);
        assert_eq!(
            state
                .batch(requests, Duration::from_secs(5))
                .unwrap_err()
                .kind,
            "InvalidData"
        );
        assert_eq!(
            state
                .request(request, Duration::from_secs(5))
                .unwrap_err()
                .kind,
            "NotConnected"
        );
    }
}
