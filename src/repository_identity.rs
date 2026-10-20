//! Repository identity is separate from the Git transport/cache identity.
use crate::error::{code, Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostMapping {
    pub alias: String,
    pub hostname: String,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GitConnection {
    /// Machine-specific transport. Never rewrite this before clone/fetch.
    pub url: String,
    pub repository_url: String,
    #[serde(rename = "ref", default)]
    pub ref_: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_mapping: Option<HostMapping>,
    /// Changes even when the URL is unchanged, invalidating prior candidates.
    pub revision: String,
}

pub fn validate_url(url: &str) -> Result<()> {
    if url.is_empty()
        || url.contains(['\n', '\r', '?', '#'])
        || !(url.starts_with("https://")
            || url.starts_with("http://")
            || url.starts_with("ssh://")
            || url.starts_with("git://")
            || url.starts_with("file://")
            || Path::new(url).is_absolute()
            || (url.contains('@') && url.contains(':') && !url.contains("://")))
    {
        return Err(Error::new(
            code::USAGE,
            "请输入 HTTPS、SSH 或本地 Git 仓库地址",
        ));
    }
    if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        if rest
            .split('/')
            .next()
            .is_some_and(|authority| authority.contains('@'))
        {
            return Err(Error::new(
                code::SOURCE_URL_CREDENTIAL,
                "HTTPS 地址不能携带认证信息",
            ));
        }
    }
    if !Path::new(url).is_absolute() && url.contains("::") {
        return Err(Error::new(code::USAGE, "不支持外部 Git helper 协议"));
    }
    crate::source::GitSource::new(url, None)?;
    Ok(())
}

/// Preserve nonstandard SSH users/ports and unknown server semantics.
pub fn key(raw: &str) -> String {
    let raw = raw.trim().trim_end_matches('/');
    let owned;
    let url = if !raw.contains("://") && raw.contains('@') {
        if let Some((authority, path)) = raw.split_once(':') {
            owned = format!("ssh://{authority}/{path}");
            owned.as_str()
        } else {
            raw
        }
    } else {
        raw
    };
    let Some((scheme, rest)) = url.split_once("://") else {
        return raw.to_string();
    };
    let Some((authority, path)) = rest.split_once('/') else {
        return raw.to_string();
    };
    let path = path
        .trim_end_matches('/')
        .strip_suffix(".git")
        .unwrap_or(path.trim_end_matches('/'));
    let (user, host) = authority
        .rsplit_once('@')
        .map_or(("", authority), |(u, h)| (u, h));
    let host = host.to_ascii_lowercase();
    // GitHub's standard Git endpoint uses the single git user across accounts.
    if host == "github.com" && ((scheme == "ssh" && user == "git") || scheme == "https") {
        return format!("github.com/{path}");
    }
    format!(
        "{}://{}/{path}",
        scheme.to_ascii_lowercase(),
        if scheme == "ssh" && !user.is_empty() {
            format!("{user}@{host}")
        } else {
            host
        }
    )
}

/// Explicit, sanitized mapping only; never reads authentication directories or executes Match.
pub fn mapped_url(url: &str, mapping: &HostMapping) -> Result<String> {
    let valid = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c))
    };
    if !valid(&mapping.alias)
        || !valid(&mapping.hostname)
        || mapping.port == Some(0)
        || mapping.user.as_ref().is_some_and(|u| !valid(u))
    {
        return Err(Error::new(
            code::USAGE,
            "SSH 映射只接受 Host、HostName、User 和 Port 的明确值",
        ));
    }
    let owned;
    let url = if !url.contains("://") {
        let (authority, path) = url
            .split_once(':')
            .ok_or_else(|| Error::new(code::USAGE, "SSH 地址无效"))?;
        owned = format!("ssh://{authority}/{path}");
        owned.as_str()
    } else {
        url
    };
    let rest = url
        .strip_prefix("ssh://")
        .ok_or_else(|| Error::new(code::USAGE, "主机映射仅适用于 SSH 地址"))?;
    let (authority, path) = rest
        .split_once('/')
        .ok_or_else(|| Error::new(code::USAGE, "SSH 地址缺少仓库路径"))?;
    let (explicit_user, host) = authority
        .rsplit_once('@')
        .map_or((None, authority), |(u, h)| (Some(u), h));
    let (host, explicit_port) = host
        .split_once(':')
        .map_or((host, None), |(h, p)| (h, Some(p)));
    if !host.eq_ignore_ascii_case(&mapping.alias) {
        return Err(Error::new(code::USAGE, "Host 映射与连接地址中的别名不一致"));
    }
    let user = explicit_user.or(mapping.user.as_deref()).ok_or_else(|| {
        Error::new(
            code::USAGE,
            "请提供明确的 SSH User，或在连接地址中填写 user@",
        )
    })?;
    let port = explicit_port
        .map(str::to_string)
        .or_else(|| mapping.port.filter(|p| *p != 22).map(|p| p.to_string()));
    let mapped = format!(
        "ssh://{user}@{}{}/{path}",
        mapping.hostname.to_ascii_lowercase(),
        port.map(|p| format!(":{p}")).unwrap_or_default()
    );
    if mapping.hostname.eq_ignore_ascii_case("github.com")
        && user == "git"
        && explicit_port.is_none()
        && mapping.port.unwrap_or(22) == 22
    {
        Ok(format!("https://github.com/{path}"))
    } else {
        Ok(mapped)
    }
}

pub fn connections(data: &Path) -> Result<Vec<GitConnection>> {
    let mut items: Vec<_> = crate::collections::load(data)?
        .sources
        .into_values()
        .filter_map(|s| s.connection)
        .collect();
    let skills = crate::personal_library::library_root(data).join("resources/skills");
    if skills.is_dir() {
        for entry in std::fs::read_dir(skills)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            if let Some(connection) =
                crate::skill_source::read_meta(&entry.path()).and_then(|m| m.connection)
            {
                items.push(connection);
            }
        }
    }
    items.sort_by(|a, b| {
        a.revision
            .cmp(&b.revision)
            .then(a.url.cmp(&b.url))
            .then(a.ref_.cmp(&b.ref_))
    });
    Ok(items)
}

/// Use only consistent, explicitly confirmed mappings already saved on this machine.
pub fn key_for(data: &Path, url: &str) -> Result<String> {
    let mut resolved = std::collections::BTreeSet::new();
    for c in connections(data)? {
        if c.url == url {
            resolved.insert(key(&c.repository_url));
        } else if let Some(mapping) = c.host_mapping {
            if let Ok(mapped) = mapped_url(url, &mapping) {
                resolved.insert(key(&mapped));
            }
        }
    }
    Ok(if resolved.len() == 1 {
        resolved.into_iter().next().unwrap()
    } else {
        key(url)
    })
}

pub fn binding_stamp(data: &Path) -> Result<String> {
    let value = serde_json::json!({"registry":crate::collections::load(data)?.revision,"connections":connections(data)?});
    Ok(crate::ids::sha256_prefix(&serde_json::to_vec(&value)?, 64))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_unverified_users_ports_hosts_and_case() {
        assert_eq!(
            key("git@github.com:Owner/repo.git"),
            key("https://github.com/Owner/repo")
        );
        for other in [
            "other@github.com:Owner/repo.git",
            "ssh://git@github.com:2222/Owner/repo",
            "git@looks-github:Owner/repo",
            "https://gitlab.com/Owner/repo",
            "https://github.com/owner/repo",
        ] {
            assert_ne!(key(other), key("https://github.com/Owner/repo"));
        }
    }
    #[test]
    fn explicit_alias_keeps_transport_and_respects_overrides() {
        let m = HostMapping {
            alias: "quincy-github".into(),
            hostname: "github.com".into(),
            user: Some("git".into()),
            port: Some(22),
        };
        let url = "git@quincy-github:Owner/repo.git";
        assert_eq!(
            key(&mapped_url(url, &m).unwrap()),
            key("https://github.com/Owner/repo")
        );
        assert!(mapped_url("git@another:Owner/repo", &m).is_err());
        assert_ne!(
            key(&mapped_url("other@quincy-github:Owner/repo", &m).unwrap()),
            key("https://github.com/Owner/repo")
        );
        assert_ne!(
            key(&mapped_url("ssh://git@quincy-github:2222/Owner/repo", &m).unwrap()),
            key("https://github.com/Owner/repo")
        );
        assert_eq!(url, "git@quincy-github:Owner/repo.git");
    }
}
