//! Process spawning utilities.
//!
//! Provides wrappers around `std::process::Command` and `tokio::process::Command`
//! that apply `CREATE_NO_WINDOW` on Windows so no console windows flash when
//! spawning subprocesses from a GUI (Tauri) application.

/// Create a `std::process::Command` (sync) with `CREATE_NO_WINDOW` on Windows.
pub fn std_command(program: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

/// Create a `tokio::process::Command` (async) with `CREATE_NO_WINDOW` on Windows.
pub fn tokio_command(program: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn std_command_runs_successfully() {
        let output = if cfg!(target_os = "windows") {
            std_command("cmd").args(["/c", "echo", "hello"]).output()
        } else {
            std_command("echo").arg("hello").output()
        };
        let output = output.expect("failed to run std_command");
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.trim().contains("hello"));
    }

    #[tokio::test]
    async fn tokio_command_runs_successfully() {
        let output = if cfg!(target_os = "windows") {
            tokio_command("cmd")
                .args(["/c", "echo", "hello"])
                .output()
                .await
        } else {
            tokio_command("echo").arg("hello").output().await
        };
        let output = output.expect("failed to run tokio_command");
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.trim().contains("hello"));
    }
}
