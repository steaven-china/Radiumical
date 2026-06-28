//! System tools — sysinfo, list_dir, tree, time, cron.
use std::path::PathBuf;
use std::process::Command;

/// System information (OS, CPU, memory, uptime).
pub fn sysinfo() -> String {
    let mut out = String::new();

    // OS info
    if let Ok(output) = Command::new("uname").arg("-a").output() {
        out.push_str(&format!(
            "OS: {}",
            String::from_utf8_lossy(&output.stdout).trim()
        ));
        out.push('\n');
    }
    // CPU
    #[cfg(target_os = "linux")]
    if let Ok(cpu) = std::fs::read_to_string("/proc/cpuinfo") {
        for line in cpu.lines() {
            if line.starts_with("model name") {
                out.push_str(&format!(
                    "CPU: {}\n",
                    line.split(':').nth(1).unwrap_or("?").trim()
                ));
                break;
            }
        }
    }
    // Memory
    #[cfg(target_os = "linux")]
    if let Ok(mem) = std::fs::read_to_string("/proc/meminfo") {
        for line in mem.lines() {
            if line.starts_with("MemTotal") {
                out.push_str(&format!(
                    "RAM: {}\n",
                    line.split(':').nth(1).unwrap_or("?").trim()
                ));
                break;
            }
        }
    }
    // Uptime
    #[cfg(target_os = "linux")]
    if let Ok(up) = std::fs::read_to_string("/proc/uptime") {
        let secs: f64 = up
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        let hours = secs as u64 / 3600;
        let mins = (secs as u64 % 3600) / 60;
        out.push_str(&format!("Uptime: {hours}h {mins}m\n"));
    }
    // Disk usage
    if let Ok(output) = Command::new("df").args(["-h", "."]).output() {
        let df = String::from_utf8_lossy(&output.stdout);
        if let Some(line) = df.lines().nth(1) {
            out.push_str(&format!("Disk: {line}"));
        }
    }

    if out.is_empty() {
        "No system info available.".into()
    } else {
        out
    }
}

/// List directory contents.
pub fn list_dir(path: &PathBuf) -> String {
    let dir = if path.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        path.clone()
    };
    let mut out = String::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let prefix = if is_dir { "📁" } else { "📄" };
            // File size
            let size = if !is_dir {
                entry.metadata().map(|m| m.len()).ok().unwrap_or(0)
            } else {
                0
            };
            let size_str = if size > 0 {
                format!(" {:>8}", size)
            } else {
                "        ".into()
            };
            out.push_str(&format!("{prefix}{size_str}  {name}\n"));
        }
    } else {
        out = format!("Cannot read: {}", dir.display());
    }
    out
}

/// Directory tree view (max depth 3).
pub fn tree(path: &PathBuf, depth: usize) -> String {
    let dir = if path.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        path.clone()
    };
    let mut out = format!("{}\n", dir.display());
    tree_recurse(&dir, "", depth.min(3), &mut out);
    out
}

fn tree_recurse(dir: &std::path::Path, prefix: &str, depth: usize, out: &mut String) {
    if depth == 0 {
        return;
    }
    let entries: Vec<_> = match std::fs::read_dir(dir) {
        Ok(e) => e.flatten().collect(),
        Err(_) => return,
    };
    for (i, entry) in entries.iter().enumerate() {
        let is_last = i == entries.len() - 1;
        let branch = if is_last { "└── " } else { "├── " };
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let hidden = name.starts_with('.');
        if hidden && depth < 3 {
            continue;
        } // skip hidden unless deep
        out.push_str(&format!("{prefix}{branch}{name}\n"));
        if is_dir {
            let next_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
            tree_recurse(&entry.path(), &next_prefix, depth - 1, out);
        }
    }
}

/// Current time.
pub fn time_now() -> String {
    let now = chrono::Local::now();
    format!("{}", now.format("%Y-%m-%d %H:%M:%S %A"))
}

/// Simple cron-like: parse a crontab entry and show next run times.
pub fn cron_info() -> String {
    // Just show the system crontab if available
    if let Ok(output) = Command::new("crontab").args(["-l"]).output() {
        let content = String::from_utf8_lossy(&output.stdout);
        if content.trim().is_empty() {
            "No crontab entries.".into()
        } else {
            format!("Crontab:\n{content}")
        }
    } else {
        "crontab not available".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_sysinfo_returns_string() {
        let info = sysinfo();
        assert!(!info.is_empty());
    }

    #[test]
    fn test_time_now_returns_date() {
        let t = time_now();
        assert!(t.contains("202")); // any year in 202x
    }

    #[test]
    fn test_list_dir_current() {
        let result = list_dir(&PathBuf::from("."));
        assert!(result.contains("Cargo.toml") || result.contains("src"));
    }

    #[test]
    fn test_tree_shallow() {
        let result = tree(&PathBuf::from("src"), 1);
        assert!(result.contains("main.rs") || result.contains("tui"));
    }
}
