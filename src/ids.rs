//! 哈希与 ID 工具。

use sha2::{Digest, Sha256};

pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex(&h.finalize())
}

pub fn sha256_prefix(data: &[u8], n: usize) -> String {
    let full = sha256_hex(data);
    full[..n.min(full.len())].to_string()
}

pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 契约 §3：workspace_id = sha256(workspace_root)[0..16]。
pub fn workspace_id_from_root(root: &std::path::Path) -> String {
    sha256_prefix(root.to_string_lossy().as_bytes(), 16)
}

/// 缓存 key：sha256(source_identity)[0..16]。
pub fn cache_key_from_identity(identity: &str) -> String {
    sha256_prefix(identity.as_bytes(), 16)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
