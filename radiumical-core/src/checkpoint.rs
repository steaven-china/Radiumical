//! Agent checkpoint / time-machine.
//!
//! Before each batch of write/edit tool calls, the harness can create a
//! checkpoint.  If the workspace is a git repository, the checkpoint is a
//! lightweight commit on a session-private branch (`radi/{session_id}`).
//! Otherwise metadata is stored locally and a best-effort file snapshot is kept
//! under `~/.radi/sessions/{hash}/{session_id}/`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tokio::process::Command;

pub const CHECKPOINTS_FILE: &str = "checkpoints.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub message: String,
    pub created: DateTime<Utc>,
    /// Git commit hash, if this checkpoint was backed by git.
    pub commit: Option<String>,
    /// Git branch name, if this checkpoint was backed by git.
    pub branch: Option<String>,
}

/// Create a checkpoint before a batch of mutating tool calls.
///
/// Returns `Ok(None)` when there are no changes to snapshot.
pub async fn create_checkpoint(
    workspace: &Path,
    session_id: &str,
    summary: &str,
) -> Result<Option<Checkpoint>> {
    let summary = summary.trim();
    if summary.is_empty() {
        return Ok(None);
    }

    if !has_any_changes(workspace).await? {
        return Ok(None);
    }

    let id = format!("cp-{}", Utc::now().timestamp_millis());
    let message = format!("[radiumical] checkpoint: {}", summary);

    let cp = if is_git_repo(workspace).await {
        let commit = commit_all(workspace, &message).await?;
        let branch = ensure_checkpoint_branch(workspace, session_id, &commit).await?;
        Checkpoint {
            id,
            message,
            created: Utc::now(),
            commit: Some(commit),
            branch: Some(branch),
        }
    } else {
        let snapshot_dir = local_snapshot_dir(workspace, session_id, &id);
        create_local_snapshot(workspace, &snapshot_dir).await?;
        Checkpoint {
            id,
            message,
            created: Utc::now(),
            commit: None,
            branch: None,
        }
    };

    append_checkpoint_meta(workspace, session_id, &cp).await?;
    Ok(Some(cp))
}

/// List all checkpoints for a workspace/session, newest first.
pub async fn list_checkpoints(workspace: &Path, session_id: &str) -> Result<Vec<Checkpoint>> {
    list_checkpoints_sync(workspace, session_id)
}

/// Synchronous version of [`list_checkpoints`] for callers without an async runtime.
pub fn list_checkpoints_sync(workspace: &Path, session_id: &str) -> Result<Vec<Checkpoint>> {
    let path = checkpoints_file(workspace, session_id);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read_to_string(&path)
        .with_context(|| format!("read checkpoints {}", path.display()))?;
    let mut cps = Vec::new();
    for line in data.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(cp) = serde_json::from_str::<Checkpoint>(line) {
            cps.push(cp);
        }
    }
    cps.reverse();
    Ok(cps)
}

/// Diff between a checkpoint and the current working tree.
pub async fn diff_checkpoint(
    workspace: &Path,
    session_id: &str,
    checkpoint_id: &str,
) -> Result<String> {
    diff_checkpoint_sync(workspace, session_id, checkpoint_id)
}

/// Synchronous version of [`diff_checkpoint`].
pub fn diff_checkpoint_sync(
    workspace: &Path,
    session_id: &str,
    checkpoint_id: &str,
) -> Result<String> {
    let cps = list_checkpoints_sync(workspace, session_id)?;
    let cp = cps
        .iter()
        .find(|c| c.id == checkpoint_id)
        .context("checkpoint not found")?;
    if let Some(commit) = &cp.commit {
        run_git_sync(workspace, &["diff", commit])
    } else {
        local_diff_sync(workspace, session_id, checkpoint_id)
    }
}

/// Roll the working tree back to a checkpoint.
pub async fn rollback(workspace: &Path, session_id: &str, checkpoint_id: &str) -> Result<()> {
    rollback_sync(workspace, session_id, checkpoint_id)
}

/// Synchronous version of [`rollback`].
pub fn rollback_sync(workspace: &Path, session_id: &str, checkpoint_id: &str) -> Result<()> {
    let cps = list_checkpoints_sync(workspace, session_id)?;
    let cp = cps
        .iter()
        .find(|c| c.id == checkpoint_id)
        .context("checkpoint not found")?;
    if let Some(commit) = &cp.commit {
        run_git_sync(workspace, &["reset", "--hard", commit])?;
        Ok(())
    } else {
        restore_local_snapshot_sync(workspace, session_id, checkpoint_id)
    }
}

// ═══ Git helpers ═══

fn run_git_sync(workspace: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(args)
        .output()
        .with_context(|| format!("run git {:?} in {}", args, workspace.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {:?} failed: {}", args, stderr);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn run_git(workspace: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(args)
        .output()
        .await
        .with_context(|| format!("run git {:?} in {}", args, workspace.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {:?} failed: {}", args, stderr);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn is_git_repo(workspace: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["rev-parse", "--git-dir"])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn has_any_changes(workspace: &Path) -> Result<bool> {
    if is_git_repo(workspace).await {
        let out = run_git(workspace, &["status", "--porcelain"]).await?;
        Ok(!out.trim().is_empty())
    } else {
        // For non-git workspaces, assume there is something to snapshot if the
        // directory exists and is not empty.
        Ok(workspace.exists() && workspace.read_dir()?.next().is_some())
    }
}

async fn ensure_checkpoint_branch(
    workspace: &Path,
    session_id: &str,
    commit: &str,
) -> Result<String> {
    let branch = format!("radi/{session_id}");
    // Point the session branch at this checkpoint commit.
    run_git(workspace, &["branch", "-f", &branch, commit]).await?;
    Ok(branch)
}

async fn commit_all(workspace: &Path, message: &str) -> Result<String> {
    run_git(workspace, &["add", "-A"]).await?;
    run_git(workspace, &["commit", "-m", message, "--no-verify"]).await?;
    let hash = run_git(workspace, &["rev-parse", "HEAD"]).await?;
    // Move current branch HEAD back one step while keeping changes staged.
    // This leaves the checkpoint commit only on the radi/{session} branch.
    if run_git(workspace, &["rev-parse", "HEAD~1"]).await.is_ok() {
        let _ = run_git(workspace, &["reset", "--soft", "HEAD~1"]).await;
    }
    Ok(hash)
}

// ═══ Local snapshot helpers (non-git fallback) ═══

fn local_snapshot_dir(workspace: &Path, session_id: &str, checkpoint_id: &str) -> PathBuf {
    session_checkpoints_dir(workspace, session_id)
        .join("snapshots")
        .join(checkpoint_id)
}

async fn create_local_snapshot(workspace: &Path, snapshot_dir: &Path) -> Result<()> {
    let _ = fs::remove_dir_all(snapshot_dir);
    fs::create_dir_all(snapshot_dir)
        .with_context(|| format!("create snapshot dir {}", snapshot_dir.display()))?;
    copy_dir_contents(workspace, snapshot_dir).await?;
    Ok(())
}

fn local_diff_sync(workspace: &Path, session_id: &str, checkpoint_id: &str) -> Result<String> {
    let snapshot_dir = local_snapshot_dir(workspace, session_id, checkpoint_id);
    if !snapshot_dir.exists() {
        anyhow::bail!("snapshot not found");
    }
    let mut lines = Vec::new();
    diff_dirs_sync(&snapshot_dir, workspace, &mut lines).with_context(|| "compute local diff")?;
    Ok(lines.join("\n"))
}

fn restore_local_snapshot_sync(
    workspace: &Path,
    session_id: &str,
    checkpoint_id: &str,
) -> Result<()> {
    let snapshot_dir = local_snapshot_dir(workspace, session_id, checkpoint_id);
    if !snapshot_dir.exists() {
        anyhow::bail!("snapshot not found");
    }
    clean_dir_contents_sync(workspace)?;
    copy_dir_contents_sync(&snapshot_dir, workspace)?;
    Ok(())
}

// ═══ Filesystem helpers ═══

async fn copy_dir_contents(src: &Path, dst: &Path) -> Result<()> {
    let entries = collect_files_sync(src)?;
    for rel in entries {
        let from = src.join(&rel);
        let to = dst.join(&rel);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
        }
        fs::copy(&from, &to)
            .with_context(|| format!("copy {} to {}", from.display(), to.display()))?;
    }
    Ok(())
}

fn copy_dir_contents_sync(src: &Path, dst: &Path) -> Result<()> {
    let entries = collect_files_sync(src)?;
    for rel in entries {
        let from = src.join(&rel);
        let to = dst.join(&rel);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
        }
        fs::copy(&from, &to)
            .with_context(|| format!("copy {} to {}", from.display(), to.display()))?;
    }
    Ok(())
}

fn clean_dir_contents_sync(dir: &Path) -> Result<()> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("read dir {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        let s = name.to_string_lossy();
        if s == ".git" || s == "target" || s == "node_modules" || s.starts_with('.') {
            continue;
        }
        entries.push(entry.path());
    }
    for path in entries {
        if path.is_dir() {
            fs::remove_dir_all(&path).with_context(|| format!("remove dir {}", path.display()))?;
        } else {
            fs::remove_file(&path).with_context(|| format!("remove file {}", path.display()))?;
        }
    }
    Ok(())
}

fn collect_files_sync(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut v = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| !is_hidden_or_build_artifact(e))
    {
        let entry = entry?;
        if entry.file_type().is_file() {
            let rel = entry.path().strip_prefix(dir).unwrap_or(entry.path());
            v.push(rel.to_path_buf());
        }
    }
    Ok(v)
}

fn diff_dirs_sync(old: &Path, new: &Path, out: &mut Vec<String>) -> Result<()> {
    let mut old_files: Vec<String> = collect_files_sync(old)?
        .into_iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let mut new_files: Vec<String> = collect_files_sync(new)?
        .into_iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    old_files.sort();
    new_files.sort();

    let mut i = 0;
    let mut j = 0;
    while i < old_files.len() || j < new_files.len() {
        match (old_files.get(i), new_files.get(j)) {
            (Some(a), Some(b)) if a == b => {
                let old_text = fs::read_to_string(old.join(a)).unwrap_or_default();
                let new_text = fs::read_to_string(new.join(b)).unwrap_or_default();
                if old_text != new_text {
                    out.push(format!("diff --git a/{a} b/{a}"));
                    out.push(format!("--- a/{a}"));
                    out.push(format!("+++ b/{a}"));
                    let diff = similar::TextDiff::from_lines(&old_text, &new_text);
                    for change in diff.iter_all_changes() {
                        let sign = match change.tag() {
                            similar::ChangeTag::Delete => "-",
                            similar::ChangeTag::Insert => "+",
                            similar::ChangeTag::Equal => " ",
                        };
                        for line in change.value().lines() {
                            out.push(format!("{sign}{line}"));
                        }
                    }
                }
                i += 1;
                j += 1;
            }
            (Some(a), Some(b)) if a < b => {
                out.push(format!("- {a}"));
                i += 1;
            }
            (Some(_), Some(b)) => {
                out.push(format!("+ {b}"));
                j += 1;
            }
            (Some(a), None) => {
                out.push(format!("- {a}"));
                i += 1;
            }
            (None, Some(b)) => {
                out.push(format!("+ {b}"));
                j += 1;
            }
            (None, None) => break,
        }
    }
    Ok(())
}

fn is_hidden_or_build_artifact(entry: &walkdir::DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy();
    name.starts_with('.') || name == "target" || name == "node_modules" || name == ".git"
}

// ═══ Metadata persistence ═══

fn session_checkpoints_dir(workspace: &Path, session_id: &str) -> PathBuf {
    crate::config::Config::dir()
        .join("sessions")
        .join(crate::session::workspace_hash(&workspace.to_string_lossy()))
        .join(session_id)
}

fn checkpoints_file(workspace: &Path, session_id: &str) -> PathBuf {
    session_checkpoints_dir(workspace, session_id).join(CHECKPOINTS_FILE)
}

async fn append_checkpoint_meta(workspace: &Path, session_id: &str, cp: &Checkpoint) -> Result<()> {
    append_checkpoint_meta_sync(workspace, session_id, cp)
}

fn append_checkpoint_meta_sync(workspace: &Path, session_id: &str, cp: &Checkpoint) -> Result<()> {
    let dir = session_checkpoints_dir(workspace, session_id);
    fs::create_dir_all(&dir)
        .with_context(|| format!("create checkpoints dir {}", dir.display()))?;
    let path = dir.join(CHECKPOINTS_FILE);
    let line = serde_json::to_string(cp)?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open checkpoints file {}", path.display()))?;
    use std::io::Write;
    writeln!(file, "{line}").with_context(|| format!("write checkpoint {}", path.display()))?;
    Ok(())
}
