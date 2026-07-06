//! Compile-time version information.
//!
//! Build info is captured at compile time via `build.rs` and embedded
//! as environment variables. The formatted string looks like:
//! `[1a2b3c4d/v0.1.0+release+x86_64]`
//!
//! Where:
//! - `1a2b3c4d` = short git hash (or tag if exact match)
//! - `v0.1.0`   = latest tag (omitted if unknown)
//! - `release`  = build profile (release / debug / release-small)
//! - `x86_64`   = target architecture

/// Returns the full version string, e.g. `[7a742e9c/v0.1.0+release-small+x86_64]`.
pub fn version_string() -> String {
    let git = option_env!("GIT_TAG")
        .map(|t| t.to_string())
        .unwrap_or_else(|| {
            option_env!("GIT_HASH")
                .map(|h| h.to_string())
                .unwrap_or_else(|| option_env!("GIT_VERSION").unwrap_or("unknown").into())
        });

    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        option_env!("PROFILE").unwrap_or("release")
    };

    let arch = std::env::consts::ARCH;

    format!("[{git}/{profile}+{arch}]")
}

/// Returns just the git describe string (e.g. `v0.1.0-5-g7a742e9c-dirty`).
pub fn git_version() -> &'static str {
    option_env!("GIT_VERSION").unwrap_or("unknown")
}

/// Returns the short git hash.
pub fn git_hash() -> &'static str {
    option_env!("GIT_HASH").unwrap_or("unknown")
}

/// Returns the latest git tag, if any.
pub fn git_tag() -> Option<&'static str> {
    option_env!("GIT_TAG")
}

/// Returns true if the working tree was dirty at build time.
pub fn git_dirty() -> bool {
    option_env!("GIT_DIRTY")
        .map(|d| d == "true")
        .unwrap_or(false)
}
