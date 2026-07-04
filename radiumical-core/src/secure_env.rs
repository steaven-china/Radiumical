//! Secure env storage — device-bound XOR obfuscation for API keys.
//!
//! Keys are stored in `~/.radi/.env.bin` as obfuscated binary. The obfuscation
//! key is derived from the machine's hardware identifier, so the file is not
//! portable across devices.
//!
//! NOT real encryption — this is obfuscation to prevent casual reading.
//! For real security use OS keychain (keyring crate) or env vars.

use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

// ── Paths ──

fn env_bin_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".radi")
        .join(".env.bin")
}

// ── Device identifier ──

/// Get a stable machine identifier for key derivation.
fn machine_id() -> String {
    #[cfg(target_os = "windows")]
    {
        // Windows: MachineGuid from registry
        win_machine_guid().unwrap_or_else(fallback_id)
    }
    #[cfg(target_os = "linux")]
    {
        fs::read_to_string("/etc/machine-id")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| fallback_id())
    }
    #[cfg(target_os = "macos")]
    {
        // macOS: IOPlatformUUID via ioreg
        std::process::Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
            .ok()
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout);
                s.split("IOPlatformUUID")
                    .nth(1)?
                    .split('"')
                    .nth(1)
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| fallback_id())
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        fallback_id()
    }
}

#[cfg(target_os = "windows")]
fn win_machine_guid() -> Option<String> {
    use std::process::Command;
    let output = Command::new("reg")
        .args([
            "query",
            r#"HKLM\SOFTWARE\Microsoft\Cryptography"#,
            "/v",
            "MachineGuid",
        ])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout);
    // Parse: "MachineGuid    REG_SZ    <guid>"
    for line in s.lines() {
        if line.contains("MachineGuid") {
            if let Some(guid) = line.split_whitespace().last() {
                return Some(guid.to_string());
            }
        }
    }
    None
}

fn fallback_id() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".into());
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".into());
    format!("{host}:{user}")
}

// ── XOR obfuscation ──

fn derive_key(machine_id: &str) -> Vec<u8> {
    // Simple key derivation: hash machine_id into a 32-byte key.
    let mut key = Vec::with_capacity(32);
    let bytes = machine_id.as_bytes();
    for i in 0..32 {
        let mut b = 0u8;
        for (j, &byte) in bytes.iter().enumerate() {
            b = b.wrapping_add(byte.wrapping_mul((i as u8).wrapping_add(j as u8)));
        }
        key.push(b);
    }
    key
}

fn xor_obfuscate(data: &[u8], key: &[u8]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, &b)| b ^ key[i % key.len()])
        .collect()
}

// ── File format ──

/// The .env.bin file format:
/// - 4 bytes: magic "RENV"
/// - 1 byte: version (1)
/// - Rest: XOR-obfuscated JSON (HashMap<String, String>)
///
/// # Security Note
///
/// This is **obfuscation, not encryption**. The XOR key is derived from the
/// machine ID (hostname + username hash), so secrets are not readable on a
/// different machine, but a determined attacker with local access can reverse
/// it. This is equivalent to how tools like `git-credential-store` work.
///
/// For production secrets, use a proper secret manager (Vault, AWS SSM, etc.).
/// This module is for developer convenience — storing API keys on a dev machine
/// so they don't appear in shell history or env vars.
const MAGIC: &[u8; 4] = b"RENV";
const VERSION: u8 = 1;

fn serialize_env(map: &HashMap<String, String>, key: &[u8]) -> Vec<u8> {
    let json = serde_json::to_vec(map).unwrap_or_default();
    let obfuscated = xor_obfuscate(&json, key);
    let mut out = Vec::with_capacity(5 + obfuscated.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&obfuscated);
    out
}

fn deserialize_env(data: &[u8], key: &[u8]) -> Option<HashMap<String, String>> {
    if data.len() < 5 {
        return None;
    }
    if &data[..4] != MAGIC {
        // Legacy: try raw JSON
        return serde_json::from_slice(data).ok();
    }
    if data[4] != VERSION {
        return None;
    }
    let obfuscated = &data[5..];
    let json = xor_obfuscate(obfuscated, key);
    serde_json::from_slice(&json).ok()
}

// ── Public API ──

/// Load all stored env vars.
pub fn load_env() -> HashMap<String, String> {
    let path = env_bin_path();
    let data = match fs::read(&path) {
        Ok(d) => d,
        Err(_) => return HashMap::new(),
    };
    let key = derive_key(&machine_id());
    deserialize_env(&data, &key).unwrap_or_default()
}

/// Save all env vars (overwrites file).
pub fn save_env(map: &HashMap<String, String>) -> Result<()> {
    let dir = env_bin_path().parent().unwrap().to_path_buf();
    fs::create_dir_all(&dir)?;
    let key = derive_key(&machine_id());
    let data = serialize_env(map, &key);
    fs::write(env_bin_path(), &data)?;
    Ok(())
}

/// Get a single env var. Checks stored first, then falls back to process env.
pub fn get(key: &str) -> Option<String> {
    // Check stored
    let stored = load_env();
    if let Some(v) = stored.get(key) {
        if !v.is_empty() {
            return Some(v.clone());
        }
    }
    // Fall back to process env
    std::env::var(key).ok()
}

/// Set a single env var and persist.
pub fn set(key: &str, value: &str) -> Result<()> {
    let mut map = load_env();
    map.insert(key.to_string(), value.to_string());
    save_env(&map)
}

/// Delete a single env var and persist.
pub fn remove(key: &str) -> Result<()> {
    let mut map = load_env();
    map.remove(key);
    save_env(&map)
}

/// List all stored keys (values hidden).
pub fn list_keys() -> Vec<String> {
    load_env().keys().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xor_roundtrip() {
        let key = derive_key("test-machine-id");
        let data = b"sk-1234567890abcdef";
        let obfuscated = xor_obfuscate(data, &key);
        assert_ne!(obfuscated, data);
        let restored = xor_obfuscate(&obfuscated, &key);
        assert_eq!(restored, data);
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let key = derive_key("test-id");
        let mut map = HashMap::new();
        map.insert("OPENAI_API_KEY".to_string(), "sk-abc123".to_string());
        map.insert("DEEPSEEK_API_KEY".to_string(), "sk-def456".to_string());
        let serialized = serialize_env(&map, &key);
        let deserialized = deserialize_env(&serialized, &key).unwrap();
        assert_eq!(deserialized.get("OPENAI_API_KEY").unwrap(), "sk-abc123");
        assert_eq!(deserialized.get("DEEPSEEK_API_KEY").unwrap(), "sk-def456");
    }

    #[test]
    fn test_different_keys_fail() {
        let key1 = derive_key("machine-a");
        let key2 = derive_key("machine-b");
        let mut map = HashMap::new();
        map.insert("SECRET".to_string(), "value".to_string());
        let serialized = serialize_env(&map, &key1);
        let result = deserialize_env(&serialized, &key2);
        // Should NOT decrypt correctly with different key
        assert!(result.is_none() || result.unwrap().get("SECRET") != Some(&"value".to_string()));
    }

    #[test]
    fn test_machine_id_deterministic() {
        let a = machine_id();
        let b = machine_id();
        assert_eq!(a, b);
    }
}
