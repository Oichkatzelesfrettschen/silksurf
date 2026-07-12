/*
 * profile -- on-disk browser profile state (origin-keyed localStorage).
 *
 * Storage lives at $XDG_DATA_HOME/silksurf/storage/<origin-hash>.json
 * (fallback ~/.local/share). Writes are atomic: a temp file in the same
 * directory is renamed over the target, so a crash mid-write never leaves a
 * truncated store. SILKSURF_EPHEMERAL=1 disables persistence entirely (the
 * private-browsing escape hatch).
 *
 * The origin key hashes scheme+host+port with FNV-1a; the JSON payload is a
 * flat string map matching the Storage API surface.
 */

use std::collections::HashMap;
use std::path::PathBuf;

/// True when persistence is disabled for this process.
pub(crate) fn ephemeral_mode() -> bool {
    std::env::var_os("SILKSURF_EPHEMERAL").is_some_and(|value| value == "1")
}

/// FNV-1a over the origin string: stable, dependency-free, collision-safe
/// enough for a per-user handful of origins.
fn origin_hash(origin: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in origin.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// scheme://host:port normalized origin for a page URL; None for URLs
/// without a host (about:, data:).
pub(crate) fn storage_origin(page_url: &str) -> Option<String> {
    let parsed = url::Url::parse(page_url).ok()?;
    let host = parsed.host_str()?;
    let port = parsed.port_or_known_default().unwrap_or(0);
    Some(format!("{}://{host}:{port}", parsed.scheme()))
}

fn storage_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
        })?;
    Some(base.join("silksurf/storage"))
}

fn storage_path(origin: &str) -> Option<PathBuf> {
    Some(storage_dir()?.join(format!("{:016x}.json", origin_hash(origin))))
}

/// Load the persisted localStorage map for a page URL. Missing files and
/// unreadable JSON yield an empty map (a corrupt store must not break
/// navigation; the next flush rewrites it).
pub(crate) fn load_local_storage(page_url: &str) -> HashMap<String, String> {
    if ephemeral_mode() {
        return HashMap::new();
    }
    let Some(path) = storage_origin(page_url).and_then(|origin| storage_path(&origin)) else {
        return HashMap::new();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return HashMap::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Persist the localStorage map for a page URL with an atomic
/// temp-file-then-rename write. Errors are reported, never fatal.
pub(crate) fn flush_local_storage(page_url: &str, entries: &HashMap<String, String>) {
    if ephemeral_mode() {
        return;
    }
    let Some(origin) = storage_origin(page_url) else {
        return;
    };
    let Some(path) = storage_path(&origin) else {
        return;
    };
    let Some(dir) = path.parent() else {
        return;
    };
    if let Err(err) = std::fs::create_dir_all(dir) {
        eprintln!("[SilkSurf] Storage dir create failed: {err}");
        return;
    }
    let payload = match serde_json::to_vec_pretty(entries) {
        Ok(payload) => payload,
        Err(err) => {
            eprintln!("[SilkSurf] Storage serialize failed: {err}");
            return;
        }
    };
    let temp = path.with_extension("json.tmp");
    if let Err(err) = std::fs::write(&temp, &payload) {
        eprintln!("[SilkSurf] Storage write failed: {err}");
        return;
    }
    if let Err(err) = std::fs::rename(&temp, &path) {
        eprintln!("[SilkSurf] Storage rename failed: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_normalizes_scheme_host_port() {
        assert_eq!(
            storage_origin("https://example.com/a/b?q=1").as_deref(),
            Some("https://example.com:443")
        );
        assert_eq!(
            storage_origin("http://example.com:8080/").as_deref(),
            Some("http://example.com:8080")
        );
        assert_eq!(storage_origin("not a url"), None);
    }

    #[test]
    fn storage_roundtrips_through_temp_xdg_dir() {
        let scratch =
            std::env::temp_dir().join(format!("silksurf-profile-test-{}", std::process::id()));
        // Serialize test isolation through env: this test owns the var for
        // its duration; the suite has no other XDG_DATA_HOME consumers.
        // SAFETY: test-only env mutation on a single thread.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &scratch);
        }
        let url = "https://roundtrip.example/";
        let mut entries = HashMap::new();
        entries.insert("theme".to_string(), "dark".to_string());
        flush_local_storage(url, &entries);
        let loaded = load_local_storage(url);
        assert_eq!(loaded.get("theme").map(String::as_str), Some("dark"));
        // Overwrite is atomic and replaces content.
        entries.insert("theme".to_string(), "light".to_string());
        flush_local_storage(url, &entries);
        assert_eq!(
            load_local_storage(url).get("theme").map(String::as_str),
            Some("light")
        );
        let _ = std::fs::remove_dir_all(&scratch);
        // SAFETY: test-only env mutation on a single thread.
        unsafe {
            std::env::remove_var("XDG_DATA_HOME");
        }
    }
}
