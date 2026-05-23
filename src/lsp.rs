//! Language detection + diagnostics via linters/LSP wrappers.
use std::path::Path;
use std::process::Command;

/// Detect the primary language of a workspace.
pub fn detect_language(workspace: &Path) -> Vec<&'static str> {
    let mut langs = Vec::new();
    if workspace.join("Cargo.toml").exists() { langs.push("rust"); }
    if workspace.join("package.json").exists() { langs.push("javascript"); }
    if workspace.join("tsconfig.json").exists() { langs.push("typescript"); }
    if workspace.join("go.mod").exists() { langs.push("go"); }
    if workspace.join("requirements.txt").exists() || workspace.join("pyproject.toml").exists() || workspace.join("setup.py").exists() {
        langs.push("python");
    }
    if workspace.join("CMakeLists.txt").exists() { langs.push("cpp"); }
    if workspace.join("pom.xml").exists() || workspace.join("build.gradle").exists() {
        langs.push("java");
    }
    if !workspace.join("*.rs").exists() && workspace.join("src").is_dir() {
        // Check file extensions
        if let Ok(entries) = std::fs::read_dir(workspace.join("src")) {
            for e in entries.flatten() {
                match e.path().extension().and_then(|s| s.to_str()) {
                    Some("rs") => { langs.push("rust"); break; }
                    Some("ts") => { langs.push("typescript"); break; }
                    Some("js") => { langs.push("javascript"); break; }
                    Some("py") => { langs.push("python"); break; }
                    Some("go") => { langs.push("go"); break; }
                    _ => {}
                }
            }
        }
    }
    langs.dedup();
    langs
}

/// Run diagnostics for the detected language.
pub fn run_diagnostics(workspace: &Path, lang: &str) -> Result<String, String> {
    match lang {
        "rust" => run_cargo_check(workspace),
        "python" => run_python_lint(workspace),
        "javascript" | "typescript" => run_eslint(workspace),
        "go" => run_go_vet(workspace),
        "cpp" => run_clang_tidy(workspace),
        _ => Err(format!("No diagnostics available for {lang}")),
    }
}

fn run_cargo_check(workspace: &Path) -> Result<String, String> {
    let output = Command::new("cargo")
        .args(["check", "--message-format=short"])
        .current_dir(workspace)
        .output()
        .map_err(|e| format!("cargo not found: {e}"))?;
    Ok(String::from_utf8_lossy(&output.stderr).to_string())
}

fn run_python_lint(workspace: &Path) -> Result<String, String> {
    // Try ruff first, then pylint
    for cmd in &["ruff", "pylint", "flake8"] {
        if let Ok(output) = Command::new(cmd)
            .args(["."])
            .current_dir(workspace)
            .output()
        {
            let out = String::from_utf8_lossy(&output.stdout);
            let err = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{out}{err}");
            if !combined.trim().is_empty() {
                return Ok(combined);
            }
        }
    }
    Err("No Python linter found (try: pip install ruff)".into())
}

fn run_eslint(workspace: &Path) -> Result<String, String> {
    for cmd in &["npx eslint", "eslint"] {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if let Ok(output) = Command::new(parts[0])
            .args(&parts[1..])
            .arg(".")
            .current_dir(workspace)
            .output()
        {
            return Ok(String::from_utf8_lossy(&output.stdout).to_string());
        }
    }
    Err("eslint not found".into())
}

fn run_go_vet(workspace: &Path) -> Result<String, String> {
    let output = Command::new("go")
        .args(["vet", "./..."])
        .current_dir(workspace)
        .output()
        .map_err(|e| format!("go not found: {e}"))?;
    Ok(String::from_utf8_lossy(&output.stderr).to_string())
}

fn run_clang_tidy(workspace: &Path) -> Result<String, String> {
    Err("clang-tidy integration not yet implemented".into())
}
