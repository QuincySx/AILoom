//! User-owned login startup. Enabling/disabling never starts/stops a current process.
use super::{atomic_write, failure};
use crate::error::Result;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const MARKER: &str = "AILoom managed web service";

fn label(root: &Path) -> String {
    let digest = format!("{:x}", Sha256::digest(root.to_string_lossy().as_bytes()));
    format!("dev.ailoom.web.{}", &digest[..16])
}

fn config_path(root: &Path) -> Result<PathBuf> {
    let home = crate::paths::user_home().ok_or_else(|| failure("无法确定用户目录"))?;
    #[cfg(target_os = "macos")]
    {
        Ok(home
            .join("Library/LaunchAgents")
            .join(format!("{}.plist", label(root))))
    }
    #[cfg(target_os = "linux")]
    {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        Ok(config
            .join("systemd/user")
            .join(format!("{}.service", label(root))))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (root, home);
        Err(failure("当前系统暂不支持登录自启动；可使用 service start"))
    }
}

pub(super) fn status(root: &Path) -> Result<Value> {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        return Ok(json!({"supported":false,"enabled":false}));
    }
    let path = config_path(root)?;
    Ok(json!({"supported":true,"enabled":path.is_file(),"path":path}))
}

fn checked(program: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(failure(format!(
            "{program} 失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

pub(super) fn set(root: &Path, enabled: bool, port: u16) -> Result<()> {
    let path = config_path(root)?;
    let content = if enabled {
        let exe = std::env::current_exe()?;
        let store = super::root_path(&crate::paths::resolve_store_root()?, false)?;
        let search_path =
            std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into());
        if cfg!(target_os = "macos") {
            plist(root, &exe, &store, &search_path, port)
        } else {
            unit(root, &exe, &store, &search_path, port)
        }
    } else {
        String::new()
    };
    update(&path, enabled, &content, |enabled| {
        register(root, &path, enabled)
    })?;
    #[cfg(target_os = "linux")]
    if !enabled {
        checked("systemctl", &["--user", "daemon-reload"])?;
    }
    Ok(())
}

fn update(
    path: &Path,
    enabled: bool,
    content: &str,
    mut register: impl FnMut(bool) -> Result<()>,
) -> Result<()> {
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(failure("自启动配置不能是符号链接"));
    }
    let old = match fs::read_to_string(path) {
        Ok(s) if s.contains(MARKER) => Some(s),
        Ok(_) => return Err(failure("同名自启动配置不是由 AILoom 生成，未覆盖")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    if !enabled && old.is_none() {
        return Ok(());
    }
    fs::create_dir_all(path.parent().unwrap())?;
    if enabled {
        atomic_write(path, content.as_bytes())?;
        let result = register(true);
        if result.is_err() {
            if let Some(old) = old {
                atomic_write(path, old.as_bytes())?;
            } else {
                fs::remove_file(path)?;
            }
        }
        result
    } else {
        register(false)?;
        // Keep generated settings outside the autoload extension.
        if let Err(e) = fs::rename(path, path.with_extension("disabled")) {
            register(true)?;
            return Err(e.into());
        }
        Ok(())
    }
}

fn register(root: &Path, path: &Path, enabled: bool) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let _ = path;
        let target = format!("gui/{}/{}", unsafe { libc::getuid() }, label(root));
        // Do not bootstrap here: RunAtLoad takes effect at the next login.
        checked(
            "/bin/launchctl",
            &[if enabled { "enable" } else { "disable" }, &target],
        )
    }
    #[cfg(target_os = "linux")]
    {
        let _ = root;
        checked("systemctl", &["--user", "daemon-reload"])?;
        checked(
            "systemctl",
            &[
                "--user",
                if enabled { "enable" } else { "disable" },
                path.file_name()
                    .unwrap()
                    .to_str()
                    .ok_or_else(|| failure("自启动文件名不是 UTF-8"))?,
            ],
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (root, path, enabled);
        Err(failure("当前系统暂不支持登录自启动"))
    }
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn plist(root: &Path, exe: &Path, store: &Path, search_path: &str, port: u16) -> String {
    let args = [
        exe.to_string_lossy().into_owned(),
        "--data-root".into(),
        root.to_string_lossy().into_owned(),
        "service".into(),
        "run".into(),
        "--port".into(),
        port.to_string(),
    ];
    let args = args
        .iter()
        .map(|s| format!("<string>{}</string>", xml(s)))
        .collect::<String>();
    let log = xml(&root.join("service/service.log").to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- {MARKER} -->
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{label}</string>
<key>ProgramArguments</key><array>{args}</array>
<key>RunAtLoad</key><true/>
<key>WorkingDirectory</key><string>{root}</string>
<key>EnvironmentVariables</key><dict><key>PATH</key><string>{search_path}</string><key>AILOOM_STORE_ROOT</key><string>{store}</string><key>XDG_DATA_HOME</key><string></string></dict>
<key>StandardOutPath</key><string>{log}</string>
<key>StandardErrorPath</key><string>{log}</string>
<key>Umask</key><integer>63</integer>
</dict></plist>
"#,
        label = label(root),
        root = xml(&root.to_string_lossy()),
        store = xml(&store.to_string_lossy()),
        search_path = xml(search_path)
    )
}

// systemd does not invoke a shell. Quote literal arguments and escape its own expansions.
fn quote(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('$', "$$")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}
fn unit(root: &Path, exe: &Path, store: &Path, search_path: &str, port: u16) -> String {
    let args = [
        exe.to_string_lossy().into_owned(),
        "--data-root".into(),
        root.to_string_lossy().into_owned(),
        "service".into(),
        "run".into(),
        "--port".into(),
        port.to_string(),
    ];
    // Environment= expands specifiers, but not dollar signs.
    let env = |s: String| quote(&s).replace("$$", "$");
    format!("# {MARKER}\n[Unit]\nDescription=AILoom web service\n\n[Service]\nType=simple\nExecStart={}\nEnvironment={}\nEnvironment={}\nEnvironment=\"XDG_DATA_HOME=\"\nUMask=0077\nTimeoutStopSec=120\n\n[Install]\nWantedBy=default.target\n",args.iter().map(|s|quote(s)).collect::<Vec<_>>().join(" "),env(format!("PATH={search_path}")),env(format!("AILOOM_STORE_ROOT={}",store.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toggles_are_idempotent_and_failed_registration_rolls_back() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("user/service.plist");
        let content = format!("# {MARKER}\nfirst");
        assert!(update(&path, true, &content, |_| Err(failure(
            "registration failed"
        )))
        .is_err());
        assert!(!path.exists());
        update(&path, true, &content, |enabled| {
            assert!(enabled);
            Ok(())
        })
        .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
        assert!(update(&path, true, &format!("{content} changed"), |_| Err(
            failure("failed")
        ))
        .is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
        assert!(update(&path, false, "", |_| Err(failure("failed"))).is_err());
        assert!(path.exists());
        update(&path, false, "", |enabled| {
            assert!(!enabled);
            Ok(())
        })
        .unwrap();
        assert!(!path.exists());
        assert_eq!(
            fs::read_to_string(path.with_extension("disabled")).unwrap(),
            content
        );
        update(&path, false, "", |_| panic!("already disabled")).unwrap();
        fs::write(&path, "owned by someone else").unwrap();
        assert!(update(&path, true, &content, |_| panic!("must not register")).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "owned by someone else");
    }
    #[test]
    fn startup_files_preserve_paths_and_do_not_open_a_browser() {
        let root = Path::new("/tmp/data & space/$test%one");
        let exe = Path::new("/tmp/A & B/ailoom");
        let store = Path::new("/tmp/store");
        let p = plist(root, exe, store, "/opt/homebrew/bin:/usr/bin", 7811);
        assert!(p.contains("/tmp/A &amp; B/ailoom"));
        assert!(p.contains("<string>service</string><string>run</string>"));
        assert!(!p.contains("KeepAlive")); // explicit stop stays stopped
        let u = unit(root, exe, store, "/usr/bin", 7811);
        assert!(u.contains("$$test%%one"));
        assert!(u.contains("WantedBy=default.target"));
        assert!(!u.contains("Restart=always"));
        assert_ne!(label(root), label(Path::new("/tmp/other")));
        #[cfg(target_os = "macos")]
        {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("service.plist");
            fs::write(&path, p).unwrap();
            assert!(Command::new("/usr/bin/plutil")
                .arg("-lint")
                .arg(path)
                .output()
                .unwrap()
                .status
                .success());
        }
    }
}
