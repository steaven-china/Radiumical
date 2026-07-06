//! Build script: captures git version info as compile-time environment variables.
//!
//! Sets:
//! - `GIT_VERSION` — `git describe --tags --always --dirty` (e.g. `v0.1.0-5-g7a742e9c` or `7a742e9c-dirty`)
//! - `GIT_HASH`    — short commit hash only (e.g. `7a742e9c`)
//! - `GIT_TAG`     — latest tag, if any (e.g. `v0.1.0`)
//! - `GIT_DIRTY`   — `true` if the working tree is dirty, else `false`

use std::process::Command;

fn run_git(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn main() {
    let describe =
        run_git(&["describe", "--tags", "--always", "--dirty"]).unwrap_or_else(|| "unknown".into());

    let hash = run_git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into());

    let tag = run_git(&["describe", "--tags", "--abbrev=0"]).filter(|t| !t.is_empty());

    let dirty = run_git(&["diff-index", "--quiet", "HEAD", "--"])
        .map(|_| "false")
        .unwrap_or("true");

    println!("cargo:rustc-env=GIT_VERSION={describe}");
    println!("cargo:rustc-env=GIT_HASH={hash}");
    if let Some(t) = tag {
        println!("cargo:rustc-env=GIT_TAG={t}");
    }
    println!("cargo:rustc-env=GIT_DIRTY={dirty}");

    // Re-run if any git ref changes
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
}
