//! Bounded isolation for mount-sensitive blocking work. A deadline stops waiting,
//! not a kernel syscall: the worker retains its admission slot until it exits.
use crate::i18n::runtime::{operation_text, rt};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    future::Future,
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Condvar, Mutex, OnceLock, Weak, mpsc},
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const QUEUE_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const REQUEST_TIMEOUT: Duration = QUEUE_TIMEOUT.saturating_add(DEFAULT_TIMEOUT);
const MAX_WORKERS: usize = 32;
const MAX_SCOPE_WORKERS: usize = 8;
const MAX_BACKGROUND_WORKERS: usize = 16;
const MAX_SCOPE_BACKGROUND_WORKERS: usize = 4;
const MAX_WAITERS: usize = 512;
static NEXT_WAITER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Priority {
    Foreground,
    Background,
}

#[derive(Debug)]
struct NotStarted(io::Error);

impl std::fmt::Display for NotStarted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for NotStarted {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

fn not_started(error: io::Error) -> io::Error {
    io::Error::new(error.kind(), NotStarted(error))
}

/// Only runner admission/setup failures prove that the closure never ran.
pub(crate) fn did_not_start(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<NotStarted>())
}

#[derive(Default)]
struct Admission {
    next_id: u64,
    active: usize,
    background_active: usize,
    background_scopes: HashMap<PathBuf, usize>,
    scopes: HashMap<PathBuf, usize>,
    stalled_scopes: HashMap<PathBuf, usize>,
    jobs: HashSet<(&'static str, PathBuf)>,
    stalled_jobs: HashSet<(&'static str, PathBuf)>,
    permits: HashMap<u64, ActivePermit>,
    waiters: HashMap<u64, Waker>,
    roots: Vec<PathBuf>,
}

struct ActivePermit {
    scope: PathBuf,
    key: (&'static str, PathBuf),
    stalled: bool,
    priority: Priority,
}

fn admission() -> &'static Mutex<Admission> {
    static STATE: OnceLock<Mutex<Admission>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}

/// Scope selection is lexical: never stat a mount to decide whether I/O is safe.
pub(crate) fn register_workspace_root(root: &Path) {
    if root.as_os_str().is_empty() {
        return;
    }
    let mut state = admission().lock().unwrap_or_else(|e| e.into_inner());
    if !state.roots.iter().any(|known| known == root) && state.roots.len() < 256 {
        state.roots.push(root.to_path_buf());
    }
}

pub(crate) fn format_error(operation: &str, path: &Path, error: impl std::fmt::Display) -> String {
    let text = error.to_string();
    let localized_prefix = format!("{} '{}'", operation_text(operation), path.display());
    let curly_prefix = format!("{operation} ‘{}’", path.display());
    let straight_prefix = format!("{operation} '{}'", path.display());
    let bare_prefix = format!("{operation} {}", path.display());
    if text.starts_with(&localized_prefix) {
        text
    } else if let Some(rest) = text
        .strip_prefix(&curly_prefix)
        .or_else(|| text.strip_prefix(&straight_prefix))
        .or_else(|| text.strip_prefix(&bare_prefix))
    {
        format!("{localized_prefix}{rest}")
    } else {
        format!("{localized_prefix}: {text}")
    }
}

fn scope_for(path: &Path, roots: &[PathBuf]) -> PathBuf {
    if let Some(root) = roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())
    {
        return root.clone();
    }
    // Includes a mount/share name for common macOS/Linux mount layouts and UNC
    // prefixes on Windows. Registered workspaces cover arbitrary sshfs locations.
    path.components().take(3).collect()
}

fn duplicate_operation_error(operation: &'static str, path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        format_error(operation, path, rt!(TaskBusy)),
    )
}

fn busy_workers_error(operation: &'static str, path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        format_error(operation, path, rt!(TaskNotStarted)),
    )
}

enum AcquirePoll {
    Acquired(Permit),
    Wait,
    Rejected(io::Error),
}

impl Admission {
    fn allocate_permit(
        &mut self,
        scope: PathBuf,
        key: (&'static str, PathBuf),
        priority: Priority,
    ) -> Permit {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.active += 1;
        *self.scopes.entry(scope.clone()).or_default() += 1;
        if priority == Priority::Background {
            self.background_active += 1;
            *self.background_scopes.entry(scope.clone()).or_default() += 1;
        }
        self.jobs.insert(key.clone());
        self.permits.insert(
            id,
            ActivePermit {
                scope,
                key,
                stalled: false,
                priority,
            },
        );
        Permit { id }
    }

    fn poll_acquire(
        &mut self,
        operation: &'static str,
        path: &Path,
        deadline: Instant,
        waker: &Waker,
        priority: Priority,
        waiter_id: u64,
    ) -> AcquirePoll {
        let scope = scope_for(path, &self.roots);
        let key = (operation, path.to_path_buf());
        let expired = Instant::now() >= deadline;
        if expired {
            return AcquirePoll::Rejected(busy_workers_error(operation, path));
        }
        if self.jobs.contains(&key) {
            if self.stalled_jobs.contains(&key) || expired {
                return AcquirePoll::Rejected(duplicate_operation_error(operation, path));
            }
            return self.wait(operation, path, waiter_id, waker);
        }
        let scope_active = self.scopes.get(&scope).copied().unwrap_or(0);
        let background_full = priority == Priority::Background
            && (self.background_active >= MAX_BACKGROUND_WORKERS
                || self.background_scopes.get(&scope).copied().unwrap_or(0)
                    >= MAX_SCOPE_BACKGROUND_WORKERS);
        if self.active >= MAX_WORKERS || scope_active >= MAX_SCOPE_WORKERS || background_full {
            return self.wait(operation, path, waiter_id, waker);
        }
        AcquirePoll::Acquired(self.allocate_permit(scope, key, priority))
    }

    fn wait(
        &mut self,
        operation: &'static str,
        path: &Path,
        id: u64,
        waker: &Waker,
    ) -> AcquirePoll {
        if self.waiters.len() >= MAX_WAITERS && !self.waiters.contains_key(&id) {
            return AcquirePoll::Rejected(io::Error::new(
                io::ErrorKind::WouldBlock,
                format_error(operation, path, rt!(TaskNotStarted)),
            ));
        }
        self.waiters.insert(id, waker.clone());
        AcquirePoll::Wait
    }
}

fn mark_permit_stalled(id: u64) {
    let wakers = {
        let mut state = admission().lock().unwrap_or_else(|e| e.into_inner());
        let Some((scope, key)) = state.permits.get_mut(&id).and_then(|permit| {
            (!permit.stalled).then(|| {
                permit.stalled = true;
                (permit.scope.clone(), permit.key.clone())
            })
        }) else {
            return;
        };
        *state.stalled_scopes.entry(scope).or_default() += 1;
        state.stalled_jobs.insert(key);
        state.waiters.values().cloned().collect::<Vec<_>>()
    };
    for waker in wakers {
        waker.wake();
    }
}

struct Permit {
    id: u64,
}

impl Permit {
    #[cfg(test)]
    fn acquire(operation: &'static str, path: &Path) -> io::Result<Self> {
        let mut state = admission().lock().unwrap_or_else(|e| e.into_inner());
        let scope = scope_for(path, &state.roots);
        let key = (operation, path.to_path_buf());
        if state.jobs.contains(&key) {
            return Err(duplicate_operation_error(operation, path));
        }
        if state.active >= MAX_WORKERS
            || state.scopes.get(&scope).copied().unwrap_or(0) >= MAX_SCOPE_WORKERS
        {
            return Err(busy_workers_error(operation, path));
        }
        Ok(state.allocate_permit(scope, key, Priority::Foreground))
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let wakers = {
            let mut state = admission().lock().unwrap_or_else(|e| e.into_inner());
            if let Some(permit) = state.permits.remove(&self.id) {
                state.active -= 1;
                if permit.priority == Priority::Background {
                    state.background_active -= 1;
                    if let Some(count) = state.background_scopes.get_mut(&permit.scope) {
                        *count -= 1;
                        if *count == 0 {
                            state.background_scopes.remove(&permit.scope);
                        }
                    }
                }
                if let Some(count) = state.scopes.get_mut(&permit.scope) {
                    *count -= 1;
                    if *count == 0 {
                        state.scopes.remove(&permit.scope);
                    }
                }
                state.jobs.remove(&permit.key);
                if permit.stalled {
                    if let Some(count) = state.stalled_scopes.get_mut(&permit.scope) {
                        *count -= 1;
                        if *count == 0 {
                            state.stalled_scopes.remove(&permit.scope);
                        }
                    }
                    state.stalled_jobs.remove(&permit.key);
                }
            }
            state.waiters.values().cloned().collect::<Vec<_>>()
        };
        for waker in wakers {
            waker.wake();
        }
    }
}

#[derive(Clone)]
struct JobContext {
    operation: &'static str,
    path: PathBuf,
    deadline: Instant,
    timeout: Duration,
}

impl JobContext {
    fn timeout_error(&self) -> io::Error {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format_error(
                self.operation,
                &self.path,
                rt!(TaskTimedOut, format!("{:.1}s", self.timeout.as_secs_f64())),
            ),
        )
    }
}

thread_local! {
    static CURRENT_JOB: RefCell<Option<JobContext>> = const { RefCell::new(None) };
}

/// Check between phases, especially before a copy fallback or destructive step.
/// This cannot interrupt a syscall already in progress.
pub(crate) fn check_deadline() -> io::Result<()> {
    CURRENT_JOB.with(|current| {
        if let Some(job) = current.borrow().as_ref()
            && Instant::now() >= job.deadline
        {
            return Err(job.timeout_error());
        }
        Ok(())
    })
}

struct ResultState<T> {
    result: Option<io::Result<T>>,
    done: bool,
    claimed: bool,
    abandoned: bool,
    waker: Option<Waker>,
}

struct Shared<T> {
    state: Mutex<ResultState<T>>,
    received: Condvar,
    context: JobContext,
    permit_id: u64,
}

impl<T> Shared<T> {
    // Keep a worker reference until delivery or cancellation, so neither the UI
    // nor the deadline service can become the last owner of an abandoned T.
    fn reclaim_unreceived_result(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        while !state.claimed && !state.abandoned && Instant::now() < self.context.deadline {
            let remaining = self
                .context
                .deadline
                .saturating_duration_since(Instant::now());
            state = self
                .received
                .wait_timeout(state, remaining)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        let (discarded, waker) = if state.abandoned {
            (state.result.take(), None)
        } else if !state.claimed && state.result.as_ref().is_some_and(Result::is_ok) {
            (
                state.result.replace(Err(self.context.timeout_error())),
                state.waker.take(),
            )
        } else {
            (None, None)
        };
        drop(state);
        drop(discarded);
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn complete(&self, result: io::Result<T>) {
        let mut result = Some(result);
        let waker = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.done {
                None
            } else {
                state.done = true;
                state.result = result.take();
                state.waker.take()
            }
        };
        // Late results (and their destructors) remain on the blocking worker.
        drop(result);
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

trait Expirable: Send + Sync {
    fn expire(&self);
    fn done(&self) -> bool;
}

impl<T: Send> Expirable for Shared<T> {
    fn expire(&self) {
        mark_permit_stalled(self.permit_id);
        self.complete(Err(self.context.timeout_error()));
    }
    fn done(&self) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).done
    }
}

#[derive(Default)]
struct WaitNotice {
    done: bool,
    waker: Option<Waker>,
}

struct WaitRegistration {
    state: Mutex<WaitNotice>,
}

impl Expirable for WaitRegistration {
    fn expire(&self) {
        let waker = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.done {
                None
            } else {
                state.done = true;
                state.waker.take()
            }
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn done(&self) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).done
    }
}

struct WaitPermit<'a> {
    id: u64,
    priority: Priority,
    operation: &'static str,
    path: &'a Path,
    deadline: Instant,
    watchdog: &'static mpsc::Sender<Deadline>,
    registration: Option<Arc<WaitRegistration>>,
}

impl Future for WaitPermit<'_> {
    type Output = io::Result<Permit>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let poll = {
            let mut state = admission().lock().unwrap_or_else(|e| e.into_inner());
            let poll = state.poll_acquire(
                self.operation,
                self.path,
                self.deadline,
                cx.waker(),
                self.priority,
                self.id,
            );
            if !matches!(poll, AcquirePoll::Wait) {
                state.waiters.remove(&self.id);
            }
            poll
        };
        match poll {
            AcquirePoll::Acquired(permit) => {
                if let Some(registration) = self.registration.take() {
                    registration
                        .state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .done = true;
                }
                Poll::Ready(Ok(permit))
            }
            AcquirePoll::Rejected(error) => {
                if let Some(registration) = self.registration.take() {
                    registration
                        .state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .done = true;
                }
                Poll::Ready(Err(error))
            }
            AcquirePoll::Wait => {
                if let Some(registration) = &self.registration {
                    registration
                        .state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .waker = Some(cx.waker().clone());
                } else {
                    let registration = Arc::new(WaitRegistration {
                        state: Mutex::new(WaitNotice {
                            done: false,
                            waker: Some(cx.waker().clone()),
                        }),
                    });
                    let expirable: Arc<dyn Expirable> = registration.clone();
                    if self
                        .watchdog
                        .send(Deadline {
                            at: self.deadline,
                            result: Arc::downgrade(&expirable),
                        })
                        .is_err()
                    {
                        return Poll::Ready(Err(io::Error::other(
                            crate::i18n::runtime::RuntimeText::TaskNotStarted.to_string(),
                        )));
                    }
                    self.registration = Some(registration);
                }
                Poll::Pending
            }
        }
    }
}

impl Drop for WaitPermit<'_> {
    fn drop(&mut self) {
        admission()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .waiters
            .remove(&self.id);
        if let Some(registration) = self.registration.take() {
            let mut state = registration.state.lock().unwrap_or_else(|e| e.into_inner());
            state.done = true;
            state.waker = None;
        }
    }
}

struct Deadline {
    at: Instant,
    result: Weak<dyn Expirable>,
}

fn watchdog() -> io::Result<&'static mpsc::Sender<Deadline>> {
    static WATCHDOG: OnceLock<Result<mpsc::Sender<Deadline>, String>> = OnceLock::new();
    WATCHDOG
        .get_or_init(|| {
            let (sender, receiver) = mpsc::channel::<Deadline>();
            std::thread::Builder::new()
                .name("moeka-fs-deadlines".into())
                .spawn(move || {
                    let mut pending = Vec::<Deadline>::new();
                    loop {
                        let now = Instant::now();
                        pending.retain(|deadline| {
                            let Some(result) = deadline.result.upgrade() else {
                                return false;
                            };
                            if result.done() {
                                return false;
                            }
                            if now >= deadline.at {
                                result.expire();
                                return false;
                            }
                            true
                        });
                        let delay = pending
                            .iter()
                            .map(|deadline| deadline.at.saturating_duration_since(now))
                            .min()
                            .unwrap_or(Duration::from_secs(1))
                            .min(Duration::from_millis(100));
                        match receiver.recv_timeout(delay) {
                            Ok(deadline) => pending.push(deadline),
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                    }
                })
                .map_err(|error| error.to_string())?;
            Ok(sender)
        })
        .as_ref()
        .map_err(|error| io::Error::other(error.clone()))
}

struct Job<T>(Arc<Shared<T>>);

impl<T> Future for Job<T> {
    type Output = io::Result<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(result) = state.result.take() {
            state.claimed = true;
            self.0.received.notify_one();
            Poll::Ready(result)
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        state.abandoned = true;
        state.waker = None;
        self.0.received.notify_one();
    }
}

pub(crate) async fn run<T: Send + 'static>(
    operation: &'static str,
    path: PathBuf,
    work: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    run_with_timeout(operation, path, DEFAULT_TIMEOUT, work).await
}

pub(crate) async fn run_value<T: Send + 'static>(
    operation: &'static str,
    path: PathBuf,
    work: impl FnOnce() -> T + Send + 'static,
) -> io::Result<T> {
    run(operation, path, move || Ok(work())).await
}

/// Optional previews/indexing cannot consume the slots reserved for user I/O.
pub(crate) async fn run_background<T: Send + 'static>(
    operation: &'static str,
    path: PathBuf,
    work: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    run_queued(
        operation,
        path,
        QUEUE_TIMEOUT,
        DEFAULT_TIMEOUT,
        Priority::Background,
        work,
    )
    .await
}

pub(crate) async fn run_write<T: Send + 'static>(
    operation: &'static str,
    path: PathBuf,
    work: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    run_with_timeout(operation, path, Duration::from_secs(30), work).await
}

pub(crate) async fn run_background_anyhow<T: Send + 'static>(
    operation: &'static str,
    path: PathBuf,
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    run_background(operation, path, move || Ok(work())).await?
}

pub(crate) async fn run_anyhow<T: Send + 'static>(
    operation: &'static str,
    path: PathBuf,
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    Ok(run(operation, path, move || {
        work().map_err(|error| io::Error::other(format!("{error:#}")))
    })
    .await?)
}

pub(crate) async fn run_with_timeout<T: Send + 'static>(
    operation: &'static str,
    path: PathBuf,
    timeout: Duration,
    work: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    run_queued(
        operation,
        path,
        QUEUE_TIMEOUT,
        timeout,
        Priority::Foreground,
        work,
    )
    .await
}

async fn run_queued<T: Send + 'static>(
    operation: &'static str,
    path: PathBuf,
    queue_timeout: Duration,
    timeout: Duration,
    priority: Priority,
    work: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    let watchdog = watchdog().map_err(not_started)?;
    let permit = WaitPermit {
        id: NEXT_WAITER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        priority,
        operation,
        path: &path,
        deadline: Instant::now() + queue_timeout,
        watchdog,
        registration: None,
    }
    .await
    .map_err(not_started)?;
    let deadline = Instant::now() + timeout;
    let permit_id = permit.id;
    let context = JobContext {
        operation,
        path,
        deadline,
        timeout,
    };
    let shared = Arc::new(Shared {
        state: Mutex::new(ResultState {
            result: None,
            done: false,
            claimed: false,
            abandoned: false,
            waker: None,
        }),
        received: Condvar::new(),
        context: context.clone(),
        permit_id,
    });
    let expirable: Arc<dyn Expirable> = shared.clone();
    watchdog
        .send(Deadline {
            at: context.deadline,
            result: Arc::downgrade(&expirable),
        })
        .map_err(|_| {
            not_started(io::Error::other(
                crate::i18n::runtime::RuntimeText::TaskNotStarted.to_string(),
            ))
        })?;
    let worker_result = shared.clone();
    std::thread::Builder::new()
        .name("moeka-filesystem".into())
        .spawn(move || {
            let _permit = permit;
            CURRENT_JOB.with(|current| *current.borrow_mut() = Some(context.clone()));
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                check_deadline()?;
                work()
            }));
            let result = match outcome {
                Ok(Ok(value)) if Instant::now() >= context.deadline => {
                    mark_permit_stalled(permit_id);
                    drop(value);
                    Err(context.timeout_error())
                }
                Ok(Ok(value)) => Ok(value),
                Ok(Err(error)) => {
                    if Instant::now() >= context.deadline {
                        mark_permit_stalled(permit_id);
                    }
                    Err(io::Error::new(
                        error.kind(),
                        format_error(context.operation, &context.path, error),
                    ))
                }
                Err(_) => Err(io::Error::other(format_error(
                    context.operation,
                    &context.path,
                    rt!(WorkerFailed),
                ))),
            };
            worker_result.complete(result);
            worker_result.reclaim_unreceived_result();
        })
        .map_err(not_started)?;
    Job(shared).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        task::Wake,
    };

    // Capacity fixtures intentionally hold whole scopes; keep their reservations
    // separate while ordinary worker tests still exercise concurrent admission.
    static CAPACITY_TESTS: Mutex<()> = Mutex::new(());

    fn wait_for_permit(operation: &'static str, path: &Path) -> Permit {
        block_on(WaitPermit {
            id: NEXT_WAITER.fetch_add(1, Ordering::Relaxed),
            priority: Priority::Foreground,
            operation,
            path,
            deadline: Instant::now() + QUEUE_TIMEOUT,
            watchdog: watchdog().unwrap(),
            registration: None,
        })
        .unwrap()
    }

    struct TestWaker(std::thread::Thread);
    impl Wake for TestWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    fn block_on<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::from(Arc::new(TestWaker(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            if let Poll::Ready(result) = future.as_mut().poll(&mut cx) {
                return result;
            }
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }

    #[test]
    fn timeout_is_independent_of_a_blocked_worker_and_retains_its_permit() {
        let dir = tempfile::tempdir().unwrap();
        register_workspace_root(dir.path());
        let path = dir.path().join("hung.md");
        let (release, wait) = mpsc::channel();
        let started = Arc::new(AtomicBool::new(false));
        let worker_started = started.clone();
        let before = Instant::now();
        let result = block_on(run_with_timeout(
            "test blocked read",
            path.clone(),
            Duration::from_millis(40),
            move || {
                worker_started.store(true, Ordering::SeqCst);
                wait.recv().unwrap();
                Ok(1)
            },
        ));
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(started.load(Ordering::SeqCst));
        assert!(before.elapsed() < Duration::from_secs(2));
        let duplicate = block_on(run("test blocked read", path.clone(), || Ok(2)));
        assert_eq!(duplicate.unwrap_err().kind(), io::ErrorKind::WouldBlock);
        assert_eq!(
            block_on(run("other local read", dir.path().join("local.md"), || Ok(
                3
            )))
            .unwrap(),
            3
        );
        release.send(()).unwrap();
        for _ in 0..100 {
            if !admission()
                .lock()
                .unwrap()
                .jobs
                .contains(&("test blocked read", path.clone()))
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("worker slot not released after syscall completion");
    }

    #[test]
    fn per_workspace_admission_is_bounded_and_does_not_block_other_workspaces() {
        let _capacity = CAPACITY_TESTS.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        register_workspace_root(dir.path());
        let permits = (0..MAX_SCOPE_WORKERS)
            .map(|index| wait_for_permit("test capacity", &dir.path().join(index.to_string())))
            .collect::<Vec<_>>();
        assert_eq!(
            Permit::acquire("test excess", &dir.path().join("extra"))
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        let other = tempfile::tempdir().unwrap();
        register_workspace_root(other.path());
        assert_eq!(
            block_on(run("test other root", other.path().join("file"), || Ok(7))).unwrap(),
            7
        );
        drop(permits);
    }

    #[test]
    fn errors_include_operation_path_and_original_cause() {
        let dir = tempfile::tempdir().unwrap();
        register_workspace_root(dir.path());
        let path = dir.path().join("missing.md");
        let error = block_on(run("read document", path.clone(), move || {
            std::fs::read(path)
        }))
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("Read file"));
        assert!(error.to_string().contains("missing.md"));
    }

    #[test]
    fn localized_worker_errors_preserve_io_kind_path_and_admission_identity() {
        use crate::i18n::{UiLanguage, runtime};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.md");
        let error = block_on(run("read document", path.clone(), || {
            runtime::set_language(UiLanguage::ChineseSimplified);
            Err::<(), _>(io::Error::new(
                io::ErrorKind::PermissionDenied,
                rt!(ReadFile, "EACCES"),
            ))
        }))
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(error.to_string().starts_with("读取文件"));
        assert!(error.to_string().ends_with("EACCES"));
        assert!(!did_not_start(&error));
        runtime::set_language(UiLanguage::ChineseSimplified);
        let formatted = format_error("read document", &path, &error);
        assert_eq!(formatted.matches(path.to_str().unwrap()).count(), 1);
        let rejected = not_started(busy_workers_error("read document", &path));
        assert_eq!(rejected.kind(), io::ErrorKind::WouldBlock);
        assert!(did_not_start(&rejected));
        assert!(rejected.to_string().contains("未能启动"));
        runtime::set_language(UiLanguage::English);
    }

    #[test]
    fn deadline_check_stops_later_destructive_phases_and_panics_become_errors() {
        let dir = tempfile::tempdir().unwrap();
        register_workspace_root(dir.path());
        let destructive = Arc::new(AtomicBool::new(false));
        let flag = destructive.clone();
        let result = block_on(run_with_timeout(
            "test copy",
            dir.path().join("source"),
            Duration::from_millis(20),
            move || {
                std::thread::sleep(Duration::from_millis(50));
                check_deadline()?;
                flag.store(true, Ordering::SeqCst);
                Ok(())
            },
        ));
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        std::thread::sleep(Duration::from_millis(60));
        assert!(!destructive.load(Ordering::SeqCst));
        let panic = block_on(run::<()>("test panic", dir.path().join("panic"), || {
            panic!("injected")
        }));
        assert!(
            panic
                .unwrap_err()
                .to_string()
                .contains("Background worker failed")
        );
    }

    #[test]
    fn abandoning_a_completed_result_does_not_run_its_destructor_on_ui_or_watchdog() {
        struct BlockingDrop(mpsc::Receiver<()>);
        impl Drop for BlockingDrop {
            fn drop(&mut self) {
                let _ = self.0.recv();
            }
        }
        let dir = tempfile::tempdir().unwrap();
        register_workspace_root(dir.path());
        let path = dir.path().join("resource");
        let (release, wait) = mpsc::channel();
        let (ready, ready_rx) = mpsc::channel();
        let (cancelled, cancelled_rx) = mpsc::channel();
        let job = std::thread::spawn(move || {
            let mut future = Box::pin(run("test abandoned resource", path, move || {
                ready.send(()).unwrap();
                Ok(BlockingDrop(wait))
            }));
            let waker = Waker::from(Arc::new(TestWaker(std::thread::current())));
            assert!(
                future
                    .as_mut()
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending()
            );
            ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            std::thread::sleep(Duration::from_millis(20));
            drop(future);
            cancelled.send(()).unwrap();
        });
        let cancellation = cancelled_rx.recv_timeout(Duration::from_secs(2));
        // Keep cleanup bounded even if this regression fails.
        if cancellation.is_err() {
            let _ = release.send(());
        }
        assert!(
            cancellation.is_ok(),
            "cancellation ran a blocking destructor on its caller"
        );
        let error = block_on(run_with_timeout(
            "test independent deadline",
            dir.path().join("other"),
            Duration::from_millis(20),
            || {
                std::thread::sleep(Duration::from_millis(60));
                Ok(())
            },
        ))
        .unwrap_err();
        release.send(()).unwrap();
        job.join().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn healthy_workspace_bursts_queue_even_when_another_worker_stalls() {
        let _capacity = CAPACITY_TESTS.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        register_workspace_root(dir.path());
        let shared_path = dir.path().join("shared.md");
        let threads = (0..(MAX_SCOPE_WORKERS * 2 + 2))
            .map(|index| {
                let path = if index % 3 == 0 {
                    shared_path.clone()
                } else {
                    dir.path().join(format!("item-{index}.md"))
                };
                std::thread::spawn(move || {
                    block_on(run("Read sidebar directory", path, move || {
                        std::thread::sleep(Duration::from_millis(5));
                        Ok(index)
                    }))
                })
            })
            .collect::<Vec<_>>();
        for (expected, handle) in threads.into_iter().enumerate() {
            assert_eq!(handle.join().unwrap().unwrap(), expected);
        }

        let permits = (0..MAX_SCOPE_WORKERS)
            .map(|index| {
                wait_for_permit(
                    "Read sidebar directory",
                    &dir.path().join(format!("stalled-{index}")),
                )
            })
            .collect::<Vec<_>>();
        mark_permit_stalled(permits[0].id);
        let overflow_path = dir.path().join("overflow");
        let before = Instant::now();
        let error = block_on(run_queued(
            "Read sidebar directory",
            overflow_path.clone(),
            Duration::from_millis(30),
            DEFAULT_TIMEOUT,
            Priority::Foreground,
            || Ok(()),
        ))
        .unwrap_err();
        assert!(before.elapsed() >= Duration::from_millis(30));
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert!(did_not_start(&error));
        let formatted = format_error("Read sidebar directory", &overflow_path, &error);
        assert_eq!(
            formatted
                .matches(&format!("Read directory '{}'", overflow_path.display()))
                .count(),
            1
        );
        assert!(!formatted.contains('‘'));
        drop(permits);
    }

    #[test]
    fn queue_wait_does_not_consume_execution_budget() {
        let _capacity = CAPACITY_TESTS.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        register_workspace_root(dir.path());
        let permits = (0..MAX_SCOPE_WORKERS)
            .map(|index| {
                wait_for_permit("queue budget blockers", &dir.path().join(index.to_string()))
            })
            .collect::<Vec<_>>();
        let execution_budget = DEFAULT_TIMEOUT;
        let mut task = std::pin::pin!(run_queued(
            "independent execution budget",
            dir.path().join("read"),
            QUEUE_TIMEOUT,
            execution_budget,
            Priority::Foreground,
            || {
                check_deadline()?;
                Ok(CURRENT_JOB.with(|job| job.borrow().as_ref().unwrap().deadline))
            },
        ));
        let waker = Waker::from(Arc::new(TestWaker(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        assert!(task.as_mut().poll(&mut cx).is_pending());
        let released_at = Instant::now();
        drop(permits);
        let deadline = block_on(task).unwrap();
        assert!(
            deadline >= released_at + execution_budget,
            "execution budget must start after queue admission, not before waiting"
        );
    }

    #[test]
    fn background_saturation_reserves_foreground_capacity() {
        let _capacity = CAPACITY_TESTS.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        register_workspace_root(dir.path());
        let permits = {
            let mut state = admission().lock().unwrap();
            (0..MAX_SCOPE_BACKGROUND_WORKERS)
                .map(|index| {
                    state.allocate_permit(
                        dir.path().to_path_buf(),
                        ("background saturation", dir.path().join(index.to_string())),
                        Priority::Background,
                    )
                })
                .collect::<Vec<_>>()
        };
        for permit in &permits {
            mark_permit_stalled(permit.id);
        }
        assert_eq!(
            block_on(run(
                "foreground during slow previews",
                dir.path().join("read"),
                || Ok(7)
            ))
            .unwrap(),
            7
        );
        let error = block_on(run_queued(
            "background overflow",
            dir.path().join("preview"),
            Duration::from_millis(20),
            DEFAULT_TIMEOUT,
            Priority::Background,
            || -> io::Result<()> { panic!("exceeded background cap") },
        ))
        .unwrap_err();
        assert!(did_not_start(&error));
        drop(permits);
    }

    #[test]
    fn expired_admission_never_executes_and_cancellation_unregisters_waiter() {
        let _capacity = CAPACITY_TESTS.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        register_workspace_root(dir.path());
        let result: io::Result<()> = block_on(run_queued(
            "expired queue",
            dir.path().join("read"),
            Duration::ZERO,
            DEFAULT_TIMEOUT,
            Priority::Foreground,
            || panic!("expired job ran"),
        ));
        assert!(did_not_start(&result.unwrap_err()));
        let permits = (0..MAX_SCOPE_WORKERS)
            .map(|index| {
                wait_for_permit("cancel queue blockers", &dir.path().join(index.to_string()))
            })
            .collect::<Vec<_>>();
        let path = dir.path().join("pending");
        let id = u64::MAX;
        let mut pending = Box::pin(WaitPermit {
            id,
            priority: Priority::Foreground,
            operation: "cancel queued read",
            path: &path,
            deadline: Instant::now() + QUEUE_TIMEOUT,
            watchdog: watchdog().unwrap(),
            registration: None,
        });
        let waker = Waker::from(Arc::new(TestWaker(std::thread::current())));
        assert!(
            pending
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        assert!(admission().lock().unwrap().waiters.contains_key(&id));
        drop(pending);
        assert!(!admission().lock().unwrap().waiters.contains_key(&id));
        drop(permits);
    }

    #[test]
    fn error_kind_alone_never_authorizes_retrying_a_write() {
        assert!(!did_not_start(&io::ErrorKind::WouldBlock.into()));
        assert!(!did_not_start(&io::ErrorKind::TimedOut.into()));
        assert!(did_not_start(&not_started(
            io::ErrorKind::WouldBlock.into()
        )));
        let context = JobContext {
            operation: "Read file list thumbnail",
            path: PathBuf::from("note.md"),
            deadline: Instant::now(),
            timeout: DEFAULT_TIMEOUT,
        };
        let text = context.timeout_error().to_string();
        assert!(!text.contains("drive"));
        assert!(!text.contains("write"));
        assert!(text.contains("may still be running"));
    }

    #[test]
    fn pending_admission_queue_is_bounded_and_existing_waiters_can_repoll() {
        let waker = Waker::from(Arc::new(TestWaker(std::thread::current())));
        let mut state = Admission::default();
        let path = Path::new("queued.md");
        for id in 0..MAX_WAITERS as u64 {
            assert!(matches!(
                state.wait("queue test", path, id, &waker),
                AcquirePoll::Wait
            ));
        }
        assert!(matches!(
            state.wait("queue test", path, 0, &waker),
            AcquirePoll::Wait
        ));
        assert!(matches!(
            state.wait("queue test", path, MAX_WAITERS as u64, &waker),
            AcquirePoll::Rejected(_)
        ));
        assert_eq!(state.waiters.len(), MAX_WAITERS);
    }

    #[test]
    fn scope_selection_does_not_require_existing_paths() {
        let roots = vec![PathBuf::from("/Volumes/team/project")];
        assert_eq!(
            scope_for(Path::new("/Volumes/team/project/missing/file"), &roots),
            roots[0]
        );
        assert_eq!(
            scope_for(Path::new("/Volumes/offline/file"), &[]),
            PathBuf::from("/Volumes/offline")
        );
        assert!(check_deadline().is_ok());
    }
}
