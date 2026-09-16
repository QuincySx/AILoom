//! 团队声明的软件包与插件依赖（AIL-033）：声明、检查、安装分离。
//! 安装：显式 `--yes` 门控 + 隔离前缀（<data>/ws/<id>/packages）；绝不随 sync 自动安装。

use crate::appctx::AppContext;
use crate::config::ProjectDeclaration;
use crate::error::{code, Error, Result};
use crate::resolver::resolve;
use crate::resource::ResourceKind;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

pub struct PackagesArgs {
    pub action: String, // check | install
    pub yes: bool,
    pub root: Option<PathBuf>,
}

struct PkgSpec {
    id: String,
    ecosystem: String,
    name: String,
    version: String,
}

fn collect_packages(
    data_root: Option<&std::path::Path>,
    explicit_root: Option<&std::path::Path>,
) -> Result<(AppContext, ProjectDeclaration, Vec<PkgSpec>)> {
    let cwd = std::env::current_dir()?;
    let ctx = AppContext::discover(data_root, &cwd, explicit_root)?;
    let declaration = ctx
        .declaration_path()
        .and_then(|p| ProjectDeclaration::load(&p).ok().flatten())
        .ok_or_else(|| {
            Error::new(code::WORKSPACE_INVALID, "工作区未绑定").fix("先运行 ailoom init")
        })?;
    let lock_path = ctx
        .workspace
        .workspace_root
        .join(crate::workspace::AILOOM_DIR)
        .join("machine")
        .join("sources.lock.json");
    let lock = crate::source::SourcesLock::load(&lock_path)?;
    let entry = lock
        .and_then(|l| l.sources.get(&declaration.source.name).cloned())
        .ok_or_else(|| Error::new(code::SOURCE_NOT_CACHED, "源未锁定"))?;
    let (snapshot, identity) = if declaration.source.kind == "git" {
        let src = crate::source::GitSource::new(
            entry.identity.trim_start_matches("git+"),
            entry.ref_.as_deref(),
        )?;
        (
            src.resolve(&ctx.source_cache(&src.identity), Some(&entry))?,
            src.identity.clone(),
        )
    } else {
        let p = declaration.source.path.clone().unwrap_or_default();
        let base = if PathBuf::from(&p).is_absolute() {
            PathBuf::from(&p)
        } else {
            ctx.workspace.workspace_root.join(&p)
        };
        let src = crate::source::LocalSource::new(&base)?;
        (src.resolve()?, src.identity.clone())
    };
    let manifest = crate::manifest::TeamManifest::load_from(&snapshot.root)?;
    let desired = resolve(crate::resolver::ResolveRequest {
        snapshot_root: &snapshot.root,
        manifest: &manifest,
        source: &declaration.source.name,
        identity: &identity,
        revision: snapshot.resolved_commit.clone(),
        content_digest: snapshot.content_digest.clone(),
        active_projects: &declaration.projects,
        active_roles: &declaration.roles,
    })?;
    let mut pkgs = Vec::new();
    for s in desired.deployable() {
        if s.entry.id.kind != ResourceKind::Package {
            continue;
        }
        let value: toml::Value = s
            .entry
            .raw
            .as_deref()
            .unwrap_or_default()
            .parse()
            .map_err(|e| Error::new(code::RENDER_FAILED, format!("package 解析失败: {e}")))?;
        let ecosystem = value
            .get("ecosystem")
            .and_then(|v| v.as_str())
            .unwrap_or("npm")
            .to_string();
        let name = value
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&s.entry.id.name)
            .to_string();
        let version = value
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        pkgs.push(PkgSpec {
            id: s.id.clone(),
            ecosystem,
            name,
            version,
        });
    }
    Ok((ctx, declaration, pkgs))
}

/// 隔离安装前缀：<data>/ws/<id>/packages（其它项目共享依赖不受卸载影响）。
fn packages_prefix(ctx: &AppContext) -> PathBuf {
    ctx.layout.ws_dir.join("packages")
}

pub fn run(args: &PackagesArgs, json: bool, data_root: Option<&std::path::Path>) -> Result<Value> {
    let (ctx, _declaration, pkgs) = collect_packages(data_root, args.root.as_deref())?;
    // 锁定契约（AIL-033）：版本必须为精确 semver；同包不同版本是可解释冲突，
    // 不允许静默覆盖
    for p in &pkgs {
        if p.ecosystem == "npm" {
            validate_exact_version(&p.name, &p.version)?;
        }
    }
    check_version_conflicts(&pkgs)?;

    let prefix = packages_prefix(&ctx);
    let mut rows: Vec<Value> = Vec::new();
    for p in &pkgs {
        if p.ecosystem != "npm" {
            rows.push(json!({ "id": p.id, "ecosystem": p.ecosystem, "state": "unsupported", "reason": "首批仅支持 npm" }));
            continue;
        }
        let installed = check_npm(&prefix, &p.name, &p.version);
        rows.push(json!({
            "id": p.id,
            "ecosystem": "npm",
            "package": p.name,
            "version": p.version,
            "state": if installed { "satisfied" } else { "missing" },
        }));
    }

    match args.action.as_str() {
        "check" => {
            let value = json!({ "prefix": prefix.display().to_string(), "packages": rows });
            if !json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
            Ok(value)
        }
        "install" => {
            if !args.yes {
                return Err(Error::new(
                    code::USAGE,
                    "安装是显式动作：追加 --yes 确认（普通 sync 不擅自安装）",
                ));
            }
            // 依赖已满足 → 不执行 npm install（幂等）
            let npm_pkgs: Vec<&PkgSpec> = pkgs.iter().filter(|p| p.ecosystem == "npm").collect();
            let missing: Vec<&&PkgSpec> = npm_pkgs
                .iter()
                .filter(|p| !check_npm(&prefix, &p.name, &p.version))
                .collect();
            let mut npm_ran = false;
            if !missing.is_empty() {
                // 生成隔离 package.json + npm install（精确锁定版本）
                let dir = prefix.clone();
                std::fs::create_dir_all(&dir)?;
                let manifest_path = dir.join("package.json");
                let mut manifest: serde_json::Value = if manifest_path.is_file() {
                    serde_json::from_str(&std::fs::read_to_string(&manifest_path)?).map_err(
                        |e| {
                            Error::new(
                                code::USER_CONTENT_CONFLICT,
                                format!("package.json 损坏: {e}"),
                            )
                        },
                    )?
                } else {
                    json!({ "name": "ailoom-workspace-packages", "private": true })
                };
                let deps = manifest
                    .as_object_mut()
                    .unwrap()
                    .entry("dependencies")
                    .or_insert(json!({}));
                for p in &npm_pkgs {
                    deps.as_object_mut()
                        .unwrap()
                        .insert(p.name.clone(), json!(p.version));
                }
                crate::sync_common::atomic_write(
                    manifest_path.as_path(),
                    serde_json::to_vec_pretty(&manifest)?.as_slice(),
                )?;
                let output = Command::new("npm")
                    .args(["install", "--no-audit", "--no-fund", "--prefix"])
                    .arg(&dir)
                    .output()
                    .map_err(|e| Error::new(code::PR_CREATE_FAILED, format!("npm 不可用: {e}")))?;
                if !output.status.success() {
                    // 安装失败不标资源可用
                    return Err(Error::new(
                        code::PR_CREATE_FAILED,
                        format!(
                            "npm install 失败：{}",
                            String::from_utf8_lossy(&output.stderr).trim()
                        ),
                    )
                    .fix("检查网络/registry；失败不标记依赖可用"));
                }
                npm_ran = true;
            }
            // 重查状态：返回安装后状态（rows 为安装前快照，重算）
            let mut installed_count = 0;
            let mut after_rows: Vec<Value> = Vec::new();
            for p in &pkgs {
                if p.ecosystem != "npm" {
                    after_rows.push(json!({ "id": p.id, "ecosystem": p.ecosystem, "state": "unsupported", "reason": "首批仅支持 npm" }));
                    continue;
                }
                let ok = check_npm(&prefix, &p.name, &p.version);
                if ok {
                    installed_count += 1;
                }
                after_rows.push(json!({
                    "id": p.id,
                    "ecosystem": "npm",
                    "package": p.name,
                    "version": p.version,
                    "state": if ok { "satisfied" } else { "missing" },
                }));
            }
            let value = json!({
                "prefix": prefix.display().to_string(),
                "installed_ok": installed_count,
                "npm_ran": npm_ran,
                "packages": after_rows,
            });
            if !json {
                crate::logging::info(format!(
                    "依赖安装完成：{installed_count} 项满足{}",
                    if npm_ran {
                        ""
                    } else {
                        "（此前已满足，未执行 npm install）"
                    }
                ));
            }
            Ok(value)
        }
        other => Err(Error::new(
            code::USAGE,
            format!("未知 packages 动作: {other}（check/install）"),
        )),
    }
}

/// 精确 semver 校验：`[v]主.次.补[-预发布][+构建]`。
/// RW-03/S03：核心必须恰好三段数字（`1.2` 这类不完整版本在启动安装器前拒绝，
/// 避免 npm 解析为范围后安装-检查永远不一致）；范围/占位字符（`^ ~ * x X < > |`
/// 与空格）只在核心段检查——`1.2.3-next.1` 等 prerelease 合法（其标识符里的
/// `x` 不是占位符）；预发布/构建段为点分隔的 `[0-9A-Za-z-]` 标识符。
fn validate_exact_version(name: &str, version: &str) -> Result<()> {
    let reject = |why: String| {
        Err(Error::new(
            code::MANIFEST_MISSING_FIELD,
            format!(
                "包 {name} 的 version 必须是精确版本（如 1.2.3 或 1.2.3-next.1），{why}: `{version}`"
            ),
        ))
    };
    let v = version.trim();
    let body = v.strip_prefix('v').unwrap_or(v);
    if body.is_empty() {
        return reject("为空".into());
    }
    let (body, build) = match body.split_once('+') {
        Some((b, build)) => (b, Some(build)),
        None => (body, None),
    };
    let (core, pre) = match body.split_once('-') {
        Some((c, pre)) => (c, Some(pre)),
        None => (body, None),
    };
    let numeric: Vec<&str> = core.split('.').collect();
    let core_ok = numeric.len() == 3
        && !core.contains(['*', '^', '~', '<', '>', '|', ' ', 'x', 'X'])
        && numeric
            .iter()
            .all(|seg| !seg.is_empty() && seg.chars().all(|c| c.is_ascii_digit()));
    if !core_ok {
        return reject("核心必须是三段数字（拒绝范围/tag/不完整版本）".into());
    }
    let ident_ok = |s: &str| {
        !s.is_empty()
            && s.split('.').all(|seg| {
                !seg.is_empty() && seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            })
    };
    if let Some(p) = pre {
        if !ident_ok(p) {
            return reject("预发布段非法".into());
        }
    }
    if let Some(b) = build {
        if !ident_ok(b) {
            return reject("构建段非法".into());
        }
    }
    Ok(())
}

/// 同名包多资源声明：版本一致 → 合并；不一致 → 可解释冲突。
fn check_version_conflicts(pkgs: &[PkgSpec]) -> Result<()> {
    let mut seen: std::collections::BTreeMap<&str, (&str, &str)> = Default::default();
    for p in pkgs {
        if p.ecosystem != "npm" {
            continue;
        }
        match seen.get(p.name.as_str()) {
            Some((prev_ver, prev_id)) if *prev_ver != p.version => {
                return Err(Error::new(
                    code::USER_CONTENT_CONFLICT,
                    format!(
                        "包 {} 版本冲突：{} 声明 {}，{} 声明 {}（同一工作区不能同时锁定两个版本）",
                        p.name, prev_id, prev_ver, p.id, p.version
                    ),
                ));
            }
            Some(_) => {}
            None => {
                seen.insert(p.name.as_str(), (p.version.as_str(), p.id.as_str()));
            }
        }
    }
    Ok(())
}

/// 检查隔离前缀内 node_modules/<name> 是否存在且 package.json version 匹配。
fn check_npm(prefix: &std::path::Path, name: &str, version: &str) -> bool {
    let scope_split = name.split_once('/');
    let module_dir = match scope_split {
        Some((scope, base)) => prefix.join("node_modules").join(scope).join(base),
        None => prefix.join("node_modules").join(name),
    };
    let pj = module_dir.join("package.json");
    if !pj.is_file() {
        return false;
    }
    let Ok(text) = std::fs::read_to_string(&pj) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    v.get("version")
        .and_then(|x| x.as_str())
        .map(|x| x == version)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err_of(v: &str) -> String {
        let spec = PkgSpec {
            id: "test/pkg/common/p".into(),
            name: "p".into(),
            ecosystem: "npm".into(),
            version: v.into(),
        };
        validate_exact_version(&spec.name, &spec.version)
            .map_err(|e| format!("{e}"))
            .err()
            .unwrap_or_default()
    }

    /// RW-03/S03：不完整版本在启动安装器前拒绝，prerelease 合法。
    #[test]
    fn exact_version_validation_matrix() {
        // 合法：完整精确版本 + prerelease + build + v 前缀
        for v in [
            "1.2.3",
            "v1.2.3",
            "0.0.1",
            "1.2.3-next.1",
            "1.2.3-rc.1+build.5",
            "1.0.0-x.7.z.92",
        ] {
            assert!(validate_exact_version("p", v).is_ok(), "{v} 应合法");
        }
        // 非法：不完整版本（旧实现放行 1.2 并交给 npm 造成反复安装）
        for v in ["1.2", "1", "1.2.", ".1.2", "v1.2"] {
            assert!(validate_exact_version("p", v).is_err(), "{v} 应拒绝");
        }
        // 非法：范围 / tag / 占位 / 空格 / 空
        for v in [
            "^1.2.3",
            "~1.2.3",
            "*",
            "1.x",
            "1.2.x",
            "latest",
            "next",
            ">=1.0.0",
            "1.0.0 || 2.0.0",
            "1 2",
            "",
        ] {
            assert!(validate_exact_version("p", v).is_err(), "{v} 应拒绝");
        }
        // 非法：prerelease/build 段畸形
        for v in ["1.2.3-", "1.2.3-next..1", "1.2.3+", "1.2.3+b@d"] {
            assert!(validate_exact_version("p", v).is_err(), "{v} 应拒绝");
        }
        assert!(err_of("1.2").contains("精确版本"));
    }
}
