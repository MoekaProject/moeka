//! Git operations layer for the Git panel: repo status queries and mutating
//! commands (stage/unstage/discard/commit/sync/branch), all executed through
//! the `git` CLI on bounded filesystem workers.
//!
//! Query results are plain data (`GitPanelSnapshot`) built off the main thread;
//! mutating commands go through [`GitOperation`] with a single in-flight
//! operation at a time, and completion funnels into one invalidation chain.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui::Context;

use super::*;

const GIT_PANEL_STATUS_REFRESH_MIN_INTERVAL_MS: u64 = 350;

// ── Data types ─────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GitPanelTab {
    Changes,
    History,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GitHistoryScope {
    Repository,
    CurrentFile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GitChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Conflicted,
}

impl GitChangeKind {
    pub(crate) fn letter(self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Added => "A",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Conflicted => "!",
        }
    }
}

/// One row in the staged or unstaged section. A file with both staged and
/// unstaged modifications yields one entry per section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GitChangeEntry {
    /// Repo-relative path (the new path for renames).
    pub rel_path: String,
    /// Original repo-relative path for renames.
    pub orig_path: Option<String>,
    pub kind: GitChangeKind,
    /// Which section this entry belongs to.
    pub staged: bool,
    /// True for `??` untracked files (discard means delete).
    pub untracked: bool,
}

impl GitChangeEntry {
    pub(crate) fn file_name(&self) -> &str {
        self.rel_path
            .rsplit('/')
            .next()
            .unwrap_or(self.rel_path.as_str())
    }

    pub(crate) fn parent_dir(&self) -> &str {
        match self.rel_path.rsplit_once('/') {
            Some((parent, _)) => parent,
            None => "",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GitPanelStatus {
    pub repo_root: PathBuf,
    /// Current branch name; `None` on a detached HEAD.
    pub branch: Option<String>,
    /// Short HEAD hash used as the label on a detached HEAD.
    pub detached_label: Option<String>,
    pub has_remote: bool,
    pub has_upstream: bool,
    pub ahead: usize,
    pub behind: usize,
    /// False for a freshly-initialized repo without any commit.
    pub has_head: bool,
    pub staged: Vec<GitChangeEntry>,
    pub unstaged: Vec<GitChangeEntry>,
    pub conflicted: Vec<GitChangeEntry>,
}

impl GitPanelStatus {
    pub(crate) fn is_clean(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty() && self.conflicted.is_empty()
    }

    pub(crate) fn branch_label(&self) -> String {
        self.branch
            .clone()
            .or_else(|| self.detached_label.clone())
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
pub(crate) enum GitPanelSnapshot {
    /// The base directory is not inside a git repository.
    NoRepo {
        base_dir: PathBuf,
    },
    Repo(Box<GitPanelStatus>),
}

/// A mutating git command triggered from the UI. Only one runs at a time.
#[derive(Clone, Debug)]
pub(crate) enum GitOperation {
    Init {
        base_dir: PathBuf,
    },
    StagePaths(Vec<String>),
    UnstagePaths(Vec<String>),
    StageAll,
    UnstageAll,
    /// Restore tracked worktree files from the index and trash untracked ones.
    DiscardPaths {
        tracked: Vec<String>,
        untracked: Vec<String>,
    },
    Commit {
        message: String,
    },
    Sync,
    Checkout {
        branch: String,
    },
    CreateBranch {
        name: String,
    },
    AddRemote {
        url: String,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum GitOpOutcome {
    Done,
    Initialized,
    Committed,
    SyncedUpToDate,
    Synced {
        pushed: usize,
        pulled: usize,
    },
    PublishedBranch {
        branch: String,
    },
    /// Sync was requested but the repo has no remote configured; the UI opens
    /// the "set remote" input flow.
    SyncNoRemote,
    CheckedOut {
        branch: String,
    },
    BranchCreated {
        branch: String,
    },
    RemoteAdded,
}

#[derive(Clone, Debug)]
pub(crate) enum GitOpError {
    /// Pull hit conflicts; the rebase was aborted and the worktree restored.
    Conflict,
    /// Authentication / access failure talking to the remote.
    Auth,
    /// `user.name` / `user.email` are not configured.
    IdentityMissing,
    Other(String),
}

// ── CLI helpers ────────────────────────────────────────────────────

/// Runs `git -C <repo_root> <args>` capturing stdout and stderr. Network
/// prompts are disabled. Callers must use the filesystem runner: a process
/// blocked on filesystem I/O cannot be cancelled by dropping its async task.
pub(super) fn run_git(repo_root: &Path, args: &[&str]) -> Result<String, String> {
    run_git_raw(repo_root, args).map(|output| output.trim_end().to_string())
}

pub(super) fn run_git_raw(repo_root: &Path, args: &[&str]) -> Result<String, String> {
    crate::platform::fs_task::check_deadline().map_err(|error| error.to_string())?;
    let output = crate::platform::process::headless_command("git")
        .map_err(|error| error.to_string())?
        // Without this, `git status` refreshes and rewrites `.git/index`, which
        // the workspace watcher reports back as a git change and re-triggers
        // the very status poll that wrote it.
        .arg("--no-pager")
        .arg("--no-optional-locks")
        .arg("-c")
        .arg("core.quotepath=false")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .env("GIT_EDITOR", "true")
        .output()
        .map_err(|error| error.to_string())?;
    crate::platform::fs_task::check_deadline().map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(stderr)
        }
    }
}

pub(super) fn git_repo_root_result(dir: &Path) -> Result<Option<PathBuf>, String> {
    match run_git(dir, &["rev-parse", "--show-toplevel"]) {
        Ok(output) => Ok((!output.is_empty()).then(|| PathBuf::from(output))),
        Err(error) if error.to_ascii_lowercase().contains("not a git repository") => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) fn git_repo_root(dir: &Path) -> Option<PathBuf> {
    git_repo_root_result(dir).ok().flatten()
}

pub(super) struct GitRepoContext {
    pub(super) repo_root: PathBuf,
    pub(super) file_path: PathBuf,
    pub(super) relative_path: String,
}

pub(super) fn git_repo_context_for_file(file_path: &Path) -> Option<GitRepoContext> {
    let file_dir = file_path.parent()?;
    let repo_root = git_repo_root(file_dir)?;
    let relative_path = file_path
        .strip_prefix(&repo_root)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    Some(GitRepoContext {
        repo_root,
        file_path: file_path.to_path_buf(),
        relative_path,
    })
}

// ── History ────────────────────────────────────────────────────────

#[derive(Clone)]
pub(crate) struct GitHistoryItem {
    pub commit_hash: Option<String>,
    pub short_hash: String,
    pub message: String,
    pub author: String,
    pub time_label: String,
    pub is_working_tree: bool,
}

pub(super) const GIT_HISTORY_PAGE_SIZE: usize = 100;

/// Collects one page of history items. In [`GitHistoryScope::CurrentFile`] the
/// path is the file (with `--follow` and a WORKING row on the first page); in
/// [`GitHistoryScope::Repository`] the path is any directory inside the repo.
pub(super) fn collect_git_history_items(
    language: crate::i18n::UiLanguage,
    path: &Path,
    scope: GitHistoryScope,
    skip: usize,
) -> Option<(Vec<GitHistoryItem>, bool)> {
    let (repo_root, relative_path) = match scope {
        GitHistoryScope::CurrentFile => {
            let context = git_repo_context_for_file(path)?;
            (context.repo_root, Some(context.relative_path))
        }
        GitHistoryScope::Repository => (git_repo_root(path)?, None),
    };

    let mut items = Vec::new();
    if skip == 0
        && let Some(relative_path) = relative_path.as_deref()
    {
        let status_output = run_git(
            &repo_root,
            &["status", "--porcelain=v1", "--", relative_path],
        )
        .unwrap_or_default();
        if !status_output.trim().is_empty() {
            items.push(GitHistoryItem {
                commit_hash: None,
                short_hash: crate::i18n::translate(language, crate::i18n::TextKey::WorkingTree)
                    .to_string(),
                message: crate::i18n::translate(language, crate::i18n::TextKey::UncommittedChanges)
                    .to_string(),
                author: String::new(),
                time_label: crate::i18n::translate(language, crate::i18n::TextKey::Current)
                    .to_string(),
                is_working_tree: true,
            });
        }
    }

    let limit = GIT_HISTORY_PAGE_SIZE;
    let max_count = format!("--max-count={}", limit.saturating_add(1));
    let skip_arg = format!("--skip={skip}");
    let mut args = vec![
        "log",
        "--format=%H%x1f%s%x1f%ct%x1f%an",
        max_count.as_str(),
        skip_arg.as_str(),
    ];
    if let Some(relative_path) = relative_path.as_deref() {
        args.insert(1, "--follow");
        args.push("--");
        args.push(relative_path);
    }
    let log_output = run_git(&repo_root, &args).unwrap_or_default();

    let mut commit_count = 0usize;
    for line in log_output.lines() {
        let mut parts = line.split('\x1f');
        let Some(commit_hash) = parts.next() else {
            continue;
        };
        let Some(message) = parts.next() else {
            continue;
        };
        let Some(timestamp) = parts.next() else {
            continue;
        };
        let author = parts.next().unwrap_or_default();
        let short_hash = commit_hash.chars().take(8).collect::<String>();
        let time_label = timestamp
            .parse::<i64>()
            .map(|value| crate::i18n::format_unix_timestamp_relative(language, value))
            .unwrap_or_else(|_| crate::i18n::format_relative_time(language, SystemTime::now()));
        items.push(GitHistoryItem {
            commit_hash: Some(commit_hash.to_string()),
            short_hash,
            message: message.to_string(),
            author: author.to_string(),
            time_label,
            is_working_tree: false,
        });
        commit_count += 1;
    }

    let has_more = commit_count > limit;
    if has_more && let Some(index) = items.iter().rposition(|item| item.commit_hash.is_some()) {
        items.remove(index);
    }
    Some((items, has_more))
}

/// File entry of one commit, for the expanded row in repository history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GitCommitFileEntry {
    pub rel_path: String,
    pub kind: GitChangeKind,
}

#[cfg(test)]
pub(super) fn collect_git_commit_files(
    repo_root: &Path,
    commit_hash: &str,
) -> Vec<GitCommitFileEntry> {
    let Ok(output) = run_git(
        repo_root,
        &["show", "--format=", "--name-status", commit_hash],
    ) else {
        return Vec::new();
    };
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let code = parts.next()?.trim();
            let first_path = parts.next()?.trim();
            let second_path = parts.next().map(str::trim);
            let kind = match code.as_bytes().first()? {
                b'M' | b'T' => GitChangeKind::Modified,
                b'A' => GitChangeKind::Added,
                b'D' => GitChangeKind::Deleted,
                b'R' | b'C' => GitChangeKind::Renamed,
                _ => GitChangeKind::Modified,
            };
            let rel_path = second_path.unwrap_or(first_path);
            if rel_path.is_empty() {
                return None;
            }
            Some(GitCommitFileEntry {
                rel_path: rel_path.to_string(),
                kind,
            })
        })
        .collect()
}

fn error_is_auth(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    [
        "authentication failed",
        "could not read username",
        "could not read password",
        "permission denied",
        "publickey",
        "terminal prompts disabled",
        "access denied",
        "403",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

fn error_is_conflict(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    ["conflict", "could not apply", "needs merge"]
        .iter()
        .any(|needle| lowered.contains(needle))
}

fn classify_remote_error(message: String) -> GitOpError {
    if error_is_auth(&message) {
        GitOpError::Auth
    } else {
        GitOpError::Other(message)
    }
}

// ── Status snapshot ────────────────────────────────────────────────

fn kind_for_status_byte(byte: u8) -> Option<GitChangeKind> {
    match byte {
        b'M' | b'T' => Some(GitChangeKind::Modified),
        b'A' => Some(GitChangeKind::Added),
        b'D' => Some(GitChangeKind::Deleted),
        b'R' | b'C' => Some(GitChangeKind::Renamed),
        _ => None,
    }
}

fn is_conflict_code(index: u8, worktree: u8) -> bool {
    matches!(
        (index, worktree),
        (b'D', b'D') | (b'A', b'A') | (b'U', _) | (_, b'U')
    )
}

fn split_rename_paths(raw: &str) -> (String, Option<String>) {
    if let Some((orig, new)) = raw.split_once(" -> ") {
        (
            new.trim().trim_matches('"').to_string(),
            Some(orig.trim().trim_matches('"').to_string()),
        )
    } else {
        (raw.trim().trim_matches('"').to_string(), None)
    }
}

pub(super) fn parse_git_status_entries(
    output: &str,
) -> (
    Vec<GitChangeEntry>,
    Vec<GitChangeEntry>,
    Vec<GitChangeEntry>,
) {
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut conflicted = Vec::new();

    for line in output.lines() {
        let bytes = line.as_bytes();
        if bytes.len() < 4 {
            continue;
        }
        let index_code = bytes[0];
        let worktree_code = bytes[1];
        let (rel_path, orig_path) = split_rename_paths(&line[3..]);
        if rel_path.is_empty() {
            continue;
        }

        if index_code == b'?' && worktree_code == b'?' {
            unstaged.push(GitChangeEntry {
                rel_path,
                orig_path: None,
                kind: GitChangeKind::Added,
                staged: false,
                untracked: true,
            });
            continue;
        }

        if is_conflict_code(index_code, worktree_code) {
            conflicted.push(GitChangeEntry {
                rel_path,
                orig_path,
                kind: GitChangeKind::Conflicted,
                staged: false,
                untracked: false,
            });
            continue;
        }

        if let Some(kind) = kind_for_status_byte(index_code) {
            staged.push(GitChangeEntry {
                rel_path: rel_path.clone(),
                orig_path: orig_path.clone(),
                kind,
                staged: true,
                untracked: false,
            });
        }
        if let Some(kind) = kind_for_status_byte(worktree_code) {
            unstaged.push(GitChangeEntry {
                rel_path,
                orig_path,
                kind,
                staged: false,
                untracked: false,
            });
        }
    }

    (staged, unstaged, conflicted)
}

/// Builds the full panel snapshot inside the bounded filesystem runner.
pub(super) fn collect_git_panel_snapshot(base_dir: &Path) -> Result<GitPanelSnapshot, String> {
    let Some(repo_root) = git_repo_root_result(base_dir)? else {
        return Ok(GitPanelSnapshot::NoRepo {
            base_dir: base_dir.to_path_buf(),
        });
    };

    let branch =
        Some(run_git(&repo_root, &["branch", "--show-current"])?).filter(|name| !name.is_empty());
    let has_head = match run_git(&repo_root, &["rev-parse", "--verify", "HEAD"]) {
        Ok(_) => true,
        Err(error) if error.contains("Needed a single revision") => false,
        Err(error) => return Err(error),
    };
    let detached_label = if branch.is_none() && has_head {
        Some(run_git(&repo_root, &["rev-parse", "--short", "HEAD"])?)
    } else {
        None
    };
    let has_remote = !run_git(&repo_root, &["remote"])?.trim().is_empty();

    let (has_upstream, ahead, behind) = if !has_head {
        (false, 0, 0)
    } else {
        match run_git(
            &repo_root,
            &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
        ) {
            Ok(counts) => {
                let mut parts = counts.split_whitespace();
                let ahead = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                let behind = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                (true, ahead, behind)
            }
            Err(error)
                if error.contains("no upstream configured")
                    || error.contains("HEAD does not point to a branch") =>
            {
                (false, 0, 0)
            }
            Err(error) => return Err(error),
        }
    };

    let status_output = run_git(
        &repo_root,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    let (staged, unstaged, conflicted) = parse_git_status_entries(&status_output);

    Ok(GitPanelSnapshot::Repo(Box::new(GitPanelStatus {
        repo_root,
        branch,
        detached_label,
        has_remote,
        has_upstream,
        ahead,
        behind,
        has_head,
        staged,
        unstaged,
        conflicted,
    })))
}

/// Lists local branches, current one first, inside the filesystem runner.
pub(super) fn collect_local_branches(repo_root: &Path) -> Result<Vec<String>, String> {
    let output = run_git(
        repo_root,
        &[
            "branch",
            "--list",
            "--format=%(refname:short)",
            "--sort=-committerdate",
        ],
    )?;
    Ok(output
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect())
}

// ── Mutating operations ────────────────────────────────────────────

fn stage_paths(repo_root: &Path, paths: &[String]) -> Result<GitOpOutcome, GitOpError> {
    let mut args = vec!["add", "--"];
    args.extend(paths.iter().map(String::as_str));
    run_git(repo_root, &args).map_err(GitOpError::Other)?;
    Ok(GitOpOutcome::Done)
}

fn unstage_paths(repo_root: &Path, paths: &[String]) -> Result<GitOpOutcome, GitOpError> {
    let has_head = run_git(repo_root, &["rev-parse", "--verify", "HEAD"]).is_ok();
    let mut args = if has_head {
        vec!["restore", "--staged", "--"]
    } else {
        vec!["rm", "-q", "-r", "--cached", "--ignore-unmatch", "--"]
    };
    args.extend(paths.iter().map(String::as_str));
    run_git(repo_root, &args).map_err(GitOpError::Other)?;
    Ok(GitOpOutcome::Done)
}

fn unstage_all(repo_root: &Path) -> Result<GitOpOutcome, GitOpError> {
    unstage_paths(repo_root, &[".".to_string()])
}

fn discard_paths(
    repo_root: &Path,
    tracked: &[String],
    untracked: &[String],
) -> Result<GitOpOutcome, GitOpError> {
    if !tracked.is_empty() {
        let mut args = vec!["restore", "--"];
        args.extend(tracked.iter().map(String::as_str));
        run_git(repo_root, &args).map_err(GitOpError::Other)?;
    }
    for rel_path in untracked {
        let absolute = repo_root.join(rel_path);
        if absolute.exists() {
            crate::platform::trash::move_to_trash(&absolute)
                .map_err(|error| GitOpError::Other(error.to_string()))?;
        }
    }
    Ok(GitOpOutcome::Done)
}

fn commit_staged(repo_root: &Path, message: &str) -> Result<GitOpOutcome, GitOpError> {
    match run_git(repo_root, &["commit", "-m", message]) {
        Ok(_) => Ok(GitOpOutcome::Committed),
        Err(error) => {
            if error.contains("Please tell me who you are") {
                Err(GitOpError::IdentityMissing)
            } else {
                Err(GitOpError::Other(error))
            }
        }
    }
}

fn preferred_remote(repo_root: &Path) -> Option<String> {
    let output = run_git(repo_root, &["remote"]).ok()?;
    let remotes: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    if remotes.is_empty() {
        return None;
    }
    if remotes.contains(&"origin") {
        Some("origin".to_string())
    } else {
        Some(remotes[0].to_string())
    }
}

fn count_commits(repo_root: &Path, range: &str) -> usize {
    run_git(repo_root, &["rev-list", "--count", range])
        .ok()
        .and_then(|output| output.trim().parse().ok())
        .unwrap_or(0)
}

fn sync_repository(repo_root: &Path) -> Result<GitOpOutcome, GitOpError> {
    let Some(remote) = preferred_remote(repo_root) else {
        return Ok(GitOpOutcome::SyncNoRemote);
    };
    let branch = run_git(repo_root, &["branch", "--show-current"])
        .map_err(GitOpError::Other)?
        .trim()
        .to_string();
    if branch.is_empty() {
        return Err(GitOpError::Other(
            "detached HEAD: checkout a branch before syncing".to_string(),
        ));
    }

    let has_upstream = run_git(
        repo_root,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
    .is_ok();

    if !has_upstream {
        run_git(repo_root, &["push", "-u", &remote, &branch]).map_err(classify_remote_error)?;
        return Ok(GitOpOutcome::PublishedBranch { branch });
    }

    run_git(repo_root, &["fetch", &remote]).map_err(classify_remote_error)?;
    let behind_before = count_commits(repo_root, "HEAD..@{upstream}");

    if behind_before > 0 {
        if let Err(error) = run_git(repo_root, &["pull", "--rebase", "--autostash"]) {
            if error_is_conflict(&error) || rebase_in_progress(repo_root) {
                let _ = run_git(repo_root, &["rebase", "--abort"]);
                return Err(GitOpError::Conflict);
            }
            return Err(classify_remote_error(error));
        }
    }

    let ahead_after = count_commits(repo_root, "@{upstream}..HEAD");
    if ahead_after > 0 {
        run_git(repo_root, &["push"]).map_err(classify_remote_error)?;
    }

    if ahead_after == 0 && behind_before == 0 {
        Ok(GitOpOutcome::SyncedUpToDate)
    } else {
        Ok(GitOpOutcome::Synced {
            pushed: ahead_after,
            pulled: behind_before,
        })
    }
}

fn rebase_in_progress(repo_root: &Path) -> bool {
    repo_root.join(".git/rebase-merge").exists() || repo_root.join(".git/rebase-apply").exists()
}

/// Executes one operation synchronously. Runs on the background executor.
pub(super) fn execute_git_operation(
    repo_root: &Path,
    operation: &GitOperation,
) -> Result<GitOpOutcome, GitOpError> {
    match operation {
        GitOperation::Init { base_dir } => {
            run_git(base_dir, &["init"]).map_err(GitOpError::Other)?;
            Ok(GitOpOutcome::Initialized)
        }
        GitOperation::StagePaths(paths) => stage_paths(repo_root, paths),
        GitOperation::UnstagePaths(paths) => unstage_paths(repo_root, paths),
        GitOperation::StageAll => {
            run_git(repo_root, &["add", "--all"]).map_err(GitOpError::Other)?;
            Ok(GitOpOutcome::Done)
        }
        GitOperation::UnstageAll => unstage_all(repo_root),
        GitOperation::DiscardPaths { tracked, untracked } => {
            discard_paths(repo_root, tracked, untracked)
        }
        GitOperation::Commit { message } => commit_staged(repo_root, message),
        GitOperation::Sync => sync_repository(repo_root),
        GitOperation::Checkout { branch } => {
            run_git(repo_root, &["checkout", branch]).map_err(GitOpError::Other)?;
            Ok(GitOpOutcome::CheckedOut {
                branch: branch.clone(),
            })
        }
        GitOperation::CreateBranch { name } => {
            run_git(repo_root, &["checkout", "-b", name]).map_err(GitOpError::Other)?;
            Ok(GitOpOutcome::BranchCreated {
                branch: name.clone(),
            })
        }
        GitOperation::AddRemote { url } => {
            run_git(repo_root, &["remote", "add", "origin", url]).map_err(GitOpError::Other)?;
            Ok(GitOpOutcome::RemoteAdded)
        }
    }
}

// ── EditorView integration ─────────────────────────────────────────

impl EditorView {
    /// Base directory the Git panel operates on: the workspace root, falling
    /// back to the active file's directory when no workspace folder matches.
    pub(crate) fn git_panel_base_dir(&self) -> Option<PathBuf> {
        if !self.sidebar_root.as_os_str().is_empty() {
            return Some(self.sidebar_root.clone());
        }
        self.current_file_path
            .as_ref()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
    }

    pub(crate) fn git_panel_repo_status(&self) -> Option<&GitPanelStatus> {
        match self.git_panel_snapshot.as_ref() {
            Some(GitPanelSnapshot::Repo(status)) => Some(status),
            _ => None,
        }
    }

    pub(crate) fn invalidate_git_panel_status(&mut self) {
        self.git_panel_status_dirty = true;
        self.git_panel_status_epoch = self.git_panel_status_epoch.wrapping_add(1);
    }

    /// Called from the Git panel render path; only schedules work when dirty.
    pub(crate) fn refresh_git_panel_status_if_needed(&mut self, cx: &mut Context<Self>) {
        if !self.git_panel_status_dirty {
            return;
        }
        self.schedule_git_panel_status_refresh(cx);
    }

    fn schedule_git_panel_status_refresh(&mut self, cx: &mut Context<Self>) {
        if self.git_panel_status_task.is_some() {
            return;
        }
        let Some(base_dir) = self.git_panel_base_dir() else {
            self.git_panel_snapshot = None;
            self.git_panel_status_dirty = false;
            return;
        };

        if file_watch::operation_suppressed("Refresh Git status", &base_dir) {
            return;
        }
        let epoch = self.git_panel_status_epoch;
        let delay = remaining_refresh_delay(
            self.git_panel_status_scanned_at,
            GIT_PANEL_STATUS_REFRESH_MIN_INTERVAL_MS,
        );
        let background_base_dir = base_dir.clone();
        let task = cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let snapshot =
                crate::platform::fs_task::run("Refresh Git status", base_dir.clone(), move || {
                    collect_git_panel_snapshot(&background_base_dir).map_err(std::io::Error::other)
                })
                .await;
            this.update(cx, |this, cx| {
                this.git_panel_status_task = None;
                let same_root = this.git_panel_base_dir().as_ref() == Some(&base_dir);
                if this.git_panel_status_epoch != epoch || !same_root {
                    if same_root && let Err(error) = snapshot {
                        this.notify_fs_error("Refresh Git status", &base_dir, error, cx);
                    }
                    if this.git_panel_status_dirty {
                        this.schedule_git_panel_status_refresh(cx);
                    }
                    return;
                }
                this.git_panel_status_dirty = false;
                this.git_panel_status_scanned_at = Some(SystemTime::now());
                match snapshot {
                    Ok(snapshot) => this.git_panel_snapshot = Some(snapshot),
                    Err(error) => {
                        this.git_panel_status_dirty = true;
                        this.notify_fs_error("Refresh Git status", &base_dir, error, cx);
                    }
                };
                cx.notify();
            })
            .ok();
        });
        self.git_panel_status_task = Some(task);
    }

    pub(crate) fn git_operation_running(&self) -> bool {
        self.git_operation_in_flight.is_some()
    }

    /// Runs one mutating git operation on bounded filesystem workers. Only one
    /// operation may be in flight; extra requests are dropped with a notify so
    /// buttons appear inert while busy.
    pub(crate) fn run_git_operation(&mut self, operation: GitOperation, cx: &mut Context<Self>) {
        if self.git_operation_in_flight.is_some() {
            return;
        }
        let Some(base_dir) = (match &operation {
            GitOperation::Init { base_dir } => Some(base_dir.clone()),
            _ => self.git_panel_base_dir(),
        }) else {
            return;
        };
        if file_watch::operation_suppressed("Git operation", &base_dir) {
            return;
        }
        let cached_root = self
            .git_panel_repo_status()
            .filter(|status| base_dir.starts_with(&status.repo_root))
            .map(|status| status.repo_root.clone());

        self.git_operation_in_flight = Some(operation.clone());
        cx.notify();

        cx.spawn(async move |this, cx| {
            let background_operation = operation.clone();
            let read_dir = base_dir.clone();
            let result = crate::platform::fs_task::run_value("Git operation", base_dir.clone(), move || {
                let repo_root = match &background_operation {
                    GitOperation::Init { base_dir } => base_dir.clone(),
                    _ => match cached_root {
                        Some(root) => root,
                        None => git_repo_root_result(&read_dir).map_err(GitOpError::Other)?
                            .ok_or_else(|| GitOpError::Other("Not a Git repository".into()))?,
                    }
                };
                execute_git_operation(&repo_root, &background_operation)
            }).await;
            this.update(cx, |this, cx| {
                this.git_operation_in_flight = None;
                if this.git_panel_base_dir().as_ref() != Some(&base_dir) { cx.notify(); return; }
                match result {
                    Ok(result) => this.handle_git_operation_result(&operation, result, cx),
                    Err(error) => this.notify_fs_error("Git operation", &base_dir, format!("{error}. If the operation started, completion is unknown; do not retry until the filesystem responds."), cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn handle_git_operation_result(
        &mut self,
        operation: &GitOperation,
        result: Result<GitOpOutcome, GitOpError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(outcome) => {
                match &outcome {
                    GitOpOutcome::Done => {}
                    GitOpOutcome::Initialized => {
                        self.notify_info(self.t(crate::i18n::TextKey::GitInitDone), cx);
                    }
                    GitOpOutcome::Committed => {
                        self.git_commit_message.clear();
                        if let Some(editor) = &self.git_commit_message_editor {
                            editor.update(cx, |input, cx| input.set_text("", cx));
                        }
                        self.notify_info(self.t(crate::i18n::TextKey::GitCommitDone), cx);
                    }
                    GitOpOutcome::SyncedUpToDate => {
                        self.notify_info(self.t(crate::i18n::TextKey::GitSyncUpToDate), cx);
                    }
                    GitOpOutcome::Synced { pushed, pulled } => {
                        let message =
                            crate::i18n::git_synced_message(self.ui_language, *pushed, *pulled);
                        self.notify_info(message, cx);
                    }
                    GitOpOutcome::PublishedBranch { branch } => {
                        let message =
                            crate::i18n::git_published_branch_message(self.ui_language, branch);
                        self.notify_info(message, cx);
                    }
                    GitOpOutcome::SyncNoRemote => {
                        self.open_git_remote_input(cx);
                    }
                    GitOpOutcome::CheckedOut { branch } => {
                        let message =
                            crate::i18n::git_checked_out_message(self.ui_language, branch);
                        self.notify_info(message, cx);
                    }
                    GitOpOutcome::BranchCreated { branch } => {
                        let message =
                            crate::i18n::git_branch_created_message(self.ui_language, branch);
                        self.notify_info(message, cx);
                    }
                    GitOpOutcome::RemoteAdded => {
                        self.notify_info(self.t(crate::i18n::TextKey::GitRemoteAdded), cx);
                        // Setting the remote was triggered from a sync attempt;
                        // continue with the sync right away.
                        self.run_git_operation(GitOperation::Sync, cx);
                    }
                }
                self.invalidate_after_git_mutation(operation, cx);
            }
            Err(error) => {
                let message = match error {
                    GitOpError::Conflict => {
                        self.t(crate::i18n::TextKey::GitSyncConflict).to_string()
                    }
                    GitOpError::Auth => self.t(crate::i18n::TextKey::GitAuthFailed).to_string(),
                    GitOpError::IdentityMissing => {
                        self.t(crate::i18n::TextKey::GitIdentityMissing).to_string()
                    }
                    GitOpError::Other(detail) => {
                        let mut detail = detail.trim().to_string();
                        const MAX_DETAIL: usize = 220;
                        if detail.len() > MAX_DETAIL {
                            let mut end = MAX_DETAIL;
                            while !detail.is_char_boundary(end) {
                                end -= 1;
                            }
                            detail.truncate(end);
                            detail.push('…');
                        }
                        format!(
                            "{}: {detail}",
                            self.t(crate::i18n::TextKey::GitOperationFailed)
                        )
                    }
                };
                self.notify_error(message, cx);
                // Even failures can leave partial effects (e.g. sync pulled but
                // failed to push), so refresh everything anyway.
                self.invalidate_after_git_mutation(operation, cx);
            }
        }
    }

    /// One invalidation chain after any mutating git operation: sidebar badges,
    /// panel status, history list, diff tabs, and the inline diff baseline.
    pub(super) fn invalidate_after_git_mutation(
        &mut self,
        operation: &GitOperation,
        cx: &mut Context<Self>,
    ) {
        self.invalidate_sidebar_git_statuses();
        self.invalidate_git_panel_status();
        self.invalidate_git_history_cache();
        self.mark_inline_diff_dirty(true);
        let diff_tab_indexes: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| tab.kind == TabKind::Diff)
            .map(|(index, _)| index)
            .collect();
        for tab_index in diff_tab_indexes {
            self.refresh_git_diff_tab(tab_index, cx);
        }
        // Diff refreshes install their snapshots asynchronously.
        // Discard/checkout/sync can rewrite files that are open in tabs; reload
        // the active buffer from disk when it was not touched in the editor.
        if matches!(
            operation,
            GitOperation::DiscardPaths { .. } | GitOperation::Checkout { .. } | GitOperation::Sync
        ) {
            self.reload_clean_tabs_from_disk(cx);
        }
        cx.notify();
    }

    /// Re-reads non-dirty text tabs from disk after git rewrote worktree files
    /// (discard/checkout/pull), without waiting for the debounced fs watcher.
    fn reload_clean_tabs_from_disk(&mut self, cx: &mut Context<Self>) {
        let paths = self
            .all_tabs()
            .filter(|tab| {
                tab.kind == TabKind::Editor
                    && !tab.is_dirty
                    && tab
                        .file_kind
                        .as_ref()
                        .is_some_and(FileKind::supports_text_buffer)
            })
            .filter_map(|tab| tab.path.clone())
            .collect::<Vec<_>>();
        self.handle_fs_events(&paths, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_status_sections() {
        let output = "M  staged.md\nMM both.md\n?? new.md\nR  old.md -> new-name.md\nUU conflict.md\nD  gone.md";
        let (staged, unstaged, conflicted) = parse_git_status_entries(output);

        assert_eq!(
            staged
                .iter()
                .map(|entry| entry.rel_path.as_str())
                .collect::<Vec<_>>(),
            vec!["staged.md", "both.md", "new-name.md", "gone.md"]
        );
        assert_eq!(
            unstaged
                .iter()
                .map(|entry| entry.rel_path.as_str())
                .collect::<Vec<_>>(),
            vec!["both.md", "new.md"]
        );
        assert!(unstaged[1].untracked);
        assert_eq!(conflicted.len(), 1);
        assert_eq!(conflicted[0].rel_path, "conflict.md");
        assert_eq!(staged[2].orig_path.as_deref(), Some("old.md"));
        assert_eq!(staged[2].kind, GitChangeKind::Renamed);
    }

    #[test]
    fn change_entry_splits_name_and_parent() {
        let entry = GitChangeEntry {
            rel_path: "docs/notes/a.md".to_string(),
            orig_path: None,
            kind: GitChangeKind::Modified,
            staged: false,
            untracked: false,
        };
        assert_eq!(entry.file_name(), "a.md");
        assert_eq!(entry.parent_dir(), "docs/notes");

        let top_level = GitChangeEntry {
            rel_path: "a.md".to_string(),
            orig_path: None,
            kind: GitChangeKind::Modified,
            staged: false,
            untracked: false,
        };
        assert_eq!(top_level.file_name(), "a.md");
        assert_eq!(top_level.parent_dir(), "");
    }

    #[cfg(not(feature = "mas"))]
    #[test]
    fn unstages_files_before_the_first_commit() {
        let directory = tempfile::tempdir().expect("tempdir");
        run_git(directory.path(), &["init", "-q"]).expect("init repository");
        std::fs::write(directory.path().join("note.md"), "draft\n").expect("write note");
        run_git(directory.path(), &["add", "note.md"]).expect("stage note");

        unstage_paths(directory.path(), &["note.md".to_string()]).expect("unstage note");

        let status = run_git(directory.path(), &["status", "--short"]).expect("status");
        assert_eq!(status, "?? note.md");
        assert!(directory.path().join("note.md").exists());
    }

    #[test]
    fn auth_and_conflict_errors_are_classified() {
        assert!(error_is_auth("fatal: Authentication failed for host"));
        assert!(error_is_auth(
            "git@github.com: Permission denied (publickey)."
        ));
        assert!(!error_is_auth("fatal: not a git repository"));
        assert!(error_is_conflict(
            "CONFLICT (content): Merge conflict in a.md"
        ));
        assert!(error_is_conflict("error: could not apply 1234abc"));
    }

    #[cfg(not(feature = "mas"))]
    #[test]
    fn inaccessible_repository_is_not_reported_as_no_repository() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing");
        assert!(git_repo_root_result(&missing).is_err());
        assert!(collect_git_panel_snapshot(&missing).is_err());
        assert!(matches!(
            collect_git_panel_snapshot(directory.path()).unwrap(),
            GitPanelSnapshot::NoRepo { .. }
        ));
    }

    #[cfg(not(feature = "mas"))]
    #[test]
    fn raw_git_output_preserves_document_newlines() {
        let directory = tempfile::tempdir().unwrap();
        run_git(directory.path(), &["init"]).unwrap();
        std::fs::write(directory.path().join("note.md"), "line\n\n").unwrap();
        run_git(directory.path(), &["add", "note.md"]).unwrap();
        assert_eq!(
            run_git_raw(directory.path(), &["show", ":0:note.md"]).unwrap(),
            "line\n\n"
        );
    }

    #[cfg(not(feature = "mas"))]
    #[test]
    fn execute_git_operations_full_cycle() {
        let root = std::env::temp_dir().join(format!(
            "moeka-git-ops-cycle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();

        // init
        let outcome = execute_git_operation(
            &root,
            &GitOperation::Init {
                base_dir: root.clone(),
            },
        )
        .unwrap();
        assert!(matches!(outcome, GitOpOutcome::Initialized));
        let repo_root = git_repo_root(&root).unwrap();
        run_git(&repo_root, &["config", "user.email", "test@example.com"]).unwrap();
        run_git(&repo_root, &["config", "user.name", "Moeka Test"]).unwrap();

        // stage + commit
        std::fs::write(repo_root.join("a.md"), "hello\n").unwrap();
        execute_git_operation(
            &repo_root,
            &GitOperation::StagePaths(vec!["a.md".to_string()]),
        )
        .unwrap();
        match collect_git_panel_snapshot(&repo_root).unwrap() {
            GitPanelSnapshot::Repo(status) => {
                assert_eq!(status.staged.len(), 1);
                assert_eq!(status.staged[0].rel_path, "a.md");
                assert!(!status.has_head);
            }
            other => panic!("expected repo snapshot, got {other:?}"),
        }
        let outcome = execute_git_operation(
            &repo_root,
            &GitOperation::Commit {
                message: "first".to_string(),
            },
        )
        .unwrap();
        assert!(matches!(outcome, GitOpOutcome::Committed));
        match collect_git_panel_snapshot(&repo_root).unwrap() {
            GitPanelSnapshot::Repo(status) => {
                assert!(status.is_clean());
                assert!(status.has_head);
                assert!(!status.branch_label().is_empty());
            }
            other => panic!("expected repo snapshot, got {other:?}"),
        }

        // modify + discard tracked
        std::fs::write(repo_root.join("a.md"), "changed\n").unwrap();
        match collect_git_panel_snapshot(&repo_root).unwrap() {
            GitPanelSnapshot::Repo(status) => {
                assert_eq!(status.unstaged.len(), 1);
                assert_eq!(status.unstaged[0].kind, GitChangeKind::Modified);
            }
            other => panic!("expected repo snapshot, got {other:?}"),
        }
        execute_git_operation(
            &repo_root,
            &GitOperation::DiscardPaths {
                tracked: vec!["a.md".to_string()],
                untracked: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(repo_root.join("a.md")).unwrap(),
            "hello\n"
        );

        // branch create + checkout back
        let original = run_git(&repo_root, &["branch", "--show-current"]).unwrap();
        let outcome = execute_git_operation(
            &repo_root,
            &GitOperation::CreateBranch {
                name: "feature/test".to_string(),
            },
        )
        .unwrap();
        assert!(matches!(outcome, GitOpOutcome::BranchCreated { .. }));
        let branches = collect_local_branches(&repo_root).unwrap();
        assert!(branches.iter().any(|name| name == "feature/test"));
        let outcome = execute_git_operation(
            &repo_root,
            &GitOperation::Checkout {
                branch: original.clone(),
            },
        )
        .unwrap();
        assert!(matches!(outcome, GitOpOutcome::CheckedOut { .. }));

        // sync without a remote reports SyncNoRemote
        let outcome = execute_git_operation(&repo_root, &GitOperation::Sync).unwrap();
        assert!(matches!(outcome, GitOpOutcome::SyncNoRemote));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(not(feature = "mas"))]
    #[test]
    fn history_collector_pages_and_reports_working_tree() {
        let root = std::env::temp_dir().join(format!(
            "moeka-git-ops-history-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            let status = crate::platform::process::headless_command("git")
                .unwrap()
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git command failed: {args:?}");
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Moeka Test"]);
        for index in 0..3 {
            std::fs::write(root.join("note.md"), format!("version {index}")).unwrap();
            git(&["add", "note.md"]);
            git(&["commit", "-q", "-m", &format!("commit {index}")]);
        }
        let canonical_root = root.canonicalize().unwrap();

        let (repo_items, _) = collect_git_history_items(
            crate::i18n::UiLanguage::English,
            &canonical_root,
            GitHistoryScope::Repository,
            0,
        )
        .unwrap();
        assert_eq!(repo_items.len(), 3);
        assert_eq!(repo_items[0].author, "Moeka Test");

        // Dirty the worktree: per-file scope should prepend a WORKING row.
        std::fs::write(root.join("note.md"), "dirty").unwrap();
        let (file_items, _) = collect_git_history_items(
            crate::i18n::UiLanguage::English,
            &canonical_root.join("note.md"),
            GitHistoryScope::CurrentFile,
            0,
        )
        .unwrap();
        assert!(file_items[0].is_working_tree);
        assert_eq!(file_items.len(), 4);

        let files = collect_git_commit_files(
            &canonical_root,
            repo_items[0].commit_hash.as_deref().unwrap(),
        );
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].rel_path, "note.md");
        assert_eq!(files[0].kind, GitChangeKind::Modified);

        std::fs::remove_dir_all(root).unwrap();
    }
}
