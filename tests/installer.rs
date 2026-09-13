//! 安装器验收（AIL-029）：本地 HTTP fixture + 临时安装目录，覆盖
//! 安装/升级/失败保留旧安装/sha256 工具探测/错误平台/卸载/npm 包装器。
//! 真实发布（npm publish、公开 release、发布时包名复核）不在本卡验收。

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// v1/v2 假二进制：可执行 shell 脚本（下载→校验→替换后可运行）
const V1: &str = "#!/bin/sh\necho installed-v1\n";
const V2: &str = "#!/bin/sh\necho installed-v2\n";

/// fixture 使用真实支持矩阵内的三元组；install.sh/cli.js 拒绝矩阵外取值本身由
/// installer_rejects_unsupported_platform / wrapper 测试覆盖。
const TRIPLE: &str = "aarch64-apple-darwin";
const TRIPLE_ENV: (&str, &str) = ("AILOOM_TRIPLE", TRIPLE);

struct Fixture {
    _dir: tempfile::TempDir,
    dir: PathBuf,
    child: std::process::Child,
    port: u16,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_http_server() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let child = Command::new("python3")
        .args([
            "-m",
            "http.server",
            &port.to_string(),
            "--bind",
            "127.0.0.1",
            "--directory",
            dir.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("启动本地 HTTP fixture");
    // 等待端口就绪
    for _ in 0..50 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Fixture {
        _dir: tmp,
        dir,
        child,
        port,
    }
}

impl Fixture {
    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

fn sha256_of(path: &Path) -> String {
    let out = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

struct InstallerEnv {
    tmp: tempfile::TempDir,
    script: PathBuf,
}

impl InstallerEnv {
    fn new() -> InstallerEnv {
        let tmp = tempfile::tempdir().unwrap();
        let script = std::env::current_dir()
            .unwrap()
            .join("scripts/install.sh")
            .canonicalize()
            .unwrap();
        InstallerEnv { tmp, script }
    }

    fn bin_dir(&self) -> PathBuf {
        self.tmp.path().join("bin")
    }

    /// 受控 PATH：prepend 工具目录（可注入只有 sha256sum / 只有 shasum 的环境）
    fn run(
        &self,
        base: &str,
        tool_dir: Option<&Path>,
        extra_env: &[(&str, &str)],
        args: &[&str],
    ) -> (i32, String, String) {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg(&self.script).args(args);
        let mut path = std::env::var("PATH").unwrap_or_default();
        if let Some(td) = tool_dir {
            path = format!("{}:{}", td.display(), path);
        }
        cmd.env("PATH", path)
            .env("HOME", self.tmp.path().join("home"))
            .env("AILOOM_INSTALL_MODE", "download")
            .env("AILOOM_BIN_DIR", self.bin_dir())
            .env("AILOOM_DOWNLOAD_BASE", base)
            .env("AILOOM_LOG", "error");
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

fn publish(fix: &Fixture, name: &str, content: &str) -> PathBuf {
    let p = fix.dir.join(name);
    std::fs::write(&p, content).unwrap();
    // 写 .sha256（标准 "<hex>  <file>" 格式）
    let hash = sha256_of(&p);
    std::fs::write(
        fix.dir.join(format!("{name}.sha256")),
        format!("{hash}  {name}\n"),
    )
    .unwrap();
    p
}

fn installed_bytes(env: &InstallerEnv) -> Option<Vec<u8>> {
    std::fs::read(env.bin_dir().join(format!("ailoom-{TRIPLE}"))).ok()
}

fn run_installed(env: &InstallerEnv) -> String {
    let out = Command::new(env.bin_dir().join("ailoom"))
        .output()
        .expect("软链应可运行");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write_tool(tool_dir: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(tool_dir).unwrap();
    let p = tool_dir.join(name);
    std::fs::write(&p, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// 构造一个不含任何 sha256 工具的受控 PATH 目录（软链安装脚本所需的最小命令集）
fn restricted_path_dir(env: &InstallerEnv, name: &str) -> PathBuf {
    let dir = env.tmp.path().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    for (tool, src) in [
        ("dirname", "/usr/bin/dirname"),
        ("uname", "/usr/bin/uname"),
        ("mkdir", "/bin/mkdir"),
        ("chmod", "/bin/chmod"),
        ("mv", "/bin/mv"),
        ("ln", "/bin/ln"),
        ("rm", "/bin/rm"),
        ("cut", "/usr/bin/cut"),
        ("curl", "/usr/bin/curl"),
    ] {
        std::os::unix::fs::symlink(src, dir.join(tool)).unwrap();
    }
    dir
}

/// 合法 v1 安装 → v2 升级成功；下载与替换不执行未校验文件（先落临时名）。
#[test]
fn installer_installs_and_upgrades_verified_artifacts() {
    let fix = start_http_server();
    let env = InstallerEnv::new();
    publish(&fix, &format!("ailoom-{TRIPLE}"), V1);

    let (code, out, err) = env.run(&fix.base(), None, &[TRIPLE_ENV], &[]);
    assert_eq!(code, 0, "安装失败: {err}");
    assert!(out.contains("安装完成"), "{out}");
    assert_eq!(run_installed(&env), "installed-v1");
    // 安装目录不应残留临时文件
    let leftovers: Vec<_> = std::fs::read_dir(env.bin_dir())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
        .collect();
    assert!(leftovers.is_empty(), "临时文件残留: {leftovers:?}");

    // 升级 v2
    publish(&fix, &format!("ailoom-{TRIPLE}"), V2);
    let (code, _, err) = env.run(&fix.base(), None, &[TRIPLE_ENV], &[]);
    assert_eq!(code, 0, "升级失败: {err}");
    assert_eq!(run_installed(&env), "installed-v2");
}

/// 升级时 404 / 缺少摘要 / 摘要不匹配：v1 逐字节不变且仍可运行。
#[test]
fn installer_failed_upgrade_preserves_old_install() {
    let fix = start_http_server();
    let env = InstallerEnv::new();
    let v1_path = publish(&fix, &format!("ailoom-{TRIPLE}"), V1);

    let (code, _, _) = env.run(&fix.base(), None, &[TRIPLE_ENV], &[]);
    assert_eq!(code, 0, "首次安装应成功");
    let v1_bytes = installed_bytes(&env).unwrap();

    // 1) 404：移除制品
    std::fs::remove_file(&v1_path).unwrap();
    let (code, _, err) = env.run(&fix.base(), None, &[TRIPLE_ENV], &[]);
    assert_ne!(code, 0, "404 必须失败");
    assert!(err.contains("下载失败"), "404 应给出下载失败错误: {err}");
    assert_eq!(
        installed_bytes(&env),
        Some(v1_bytes.clone()),
        "404 后 v1 逐字节不变"
    );
    assert_eq!(run_installed(&env), "installed-v1", "v1 仍可运行");

    // 2) 缺少摘要文件
    publish(&fix, &format!("ailoom-{TRIPLE}"), V2);
    std::fs::remove_file(fix.dir.join(format!("ailoom-{TRIPLE}.sha256"))).unwrap();
    let (code, _, err) = env.run(&fix.base(), None, &[TRIPLE_ENV], &[]);
    assert_ne!(code, 0, "缺少 sha256 必须失败");
    assert!(err.contains("拒绝安装"), "{err}");
    assert_eq!(
        installed_bytes(&env),
        Some(v1_bytes.clone()),
        "缺摘要后 v1 不变"
    );
    assert_eq!(run_installed(&env), "installed-v1");

    // 3) 摘要不匹配
    std::fs::write(
        fix.dir.join(format!("ailoom-{TRIPLE}.sha256")),
        format!("{}  x\n", "0".repeat(64)),
    )
    .unwrap();
    let (code, _, err) = env.run(&fix.base(), None, &[TRIPLE_ENV], &[]);
    assert_ne!(code, 0, "摘要不匹配必须失败");
    assert!(err.contains("sha256 校验失败"), "{err}");
    assert_eq!(
        installed_bytes(&env),
        Some(v1_bytes.clone()),
        "摘要不匹配后 v1 不变"
    );
    assert_eq!(run_installed(&env), "installed-v1");

    // 恢复正确 v2 后升级成功
    publish(&fix, &format!("ailoom-{TRIPLE}"), V2);
    let (code, _, err) = env.run(&fix.base(), None, &[TRIPLE_ENV], &[]);
    assert_eq!(code, 0, "恢复后升级应成功: {err}");
    assert_eq!(run_installed(&env), "installed-v2");
}

/// 只有 sha256sum 或只有 shasum 的受控环境都能校验；都缺失时明确失败。
#[test]
fn installer_sha256_tool_fallback_and_fail_closed() {
    let fix = start_http_server();
    let env = InstallerEnv::new();
    publish(&fix, &format!("ailoom-{TRIPLE}"), V1);

    // 只有 sha256sum（自制包装脚本走受控路径；包装目标用本机 shasum）
    let only_sum = env.tmp.path().join("tools-only-sum");
    write_tool(
        &only_sum,
        "sha256sum",
        "#!/bin/sh\nexec /usr/bin/shasum -a 256 \"$@\"\n",
    );
    let (code, _, err) = env.run(&fix.base(), Some(&only_sum), &[TRIPLE_ENV], &[]);
    assert_eq!(code, 0, "只有 sha256sum 时应成功: {err}");

    // 只有 shasum：受控 PATH = shasum 包装目录 + 无 sha256 工具的基础目录
    let restricted = env.tmp.path().join("tools-restricted");
    write_tool(
        &restricted,
        "shasum",
        "#!/bin/sh\nexec /usr/bin/shasum -a 256 \"$@\"\n",
    );
    let base = restricted_path_dir(&env, "tools-restricted-base");
    let bin2 = env.tmp.path().join("bin2");
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(&env.script)
        .env(
            "PATH",
            format!("{}:{}", restricted.display(), base.display()),
        )
        .env("HOME", env.tmp.path().join("home"))
        .env("AILOOM_INSTALL_MODE", "download")
        .env("AILOOM_BIN_DIR", &bin2)
        .env("AILOOM_DOWNLOAD_BASE", fix.base())
        .env("AILOOM_TRIPLE", TRIPLE);
    let out = cmd.output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "只有 shasum 的环境应成功: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ran = Command::new(bin2.join("ailoom")).output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&ran.stdout).trim(),
        "installed-v1",
        "只有 shasum 的环境安装后可运行"
    );

    // 都缺失：受控 PATH 无 sha256sum/shasum → fail closed
    let base_none = restricted_path_dir(&env, "tools-none-base");
    let bin3 = env.tmp.path().join("bin3");
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(&env.script)
        .env("PATH", base_none.display().to_string())
        .env("HOME", env.tmp.path().join("home"))
        .env("AILOOM_INSTALL_MODE", "download")
        .env("AILOOM_BIN_DIR", &bin3)
        .env("AILOOM_DOWNLOAD_BASE", fix.base())
        .env("AILOOM_TRIPLE", TRIPLE);
    let out = cmd.output().unwrap();
    assert_ne!(out.status.code(), Some(0), "无 sha256 工具必须失败");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("sha256sum 或 shasum"), "{err}");
    assert!(
        !bin3.join("ailoom").exists() && !bin3.join(format!("ailoom-{TRIPLE}")).exists(),
        "校验不可用时不应产出任何安装物"
    );
}

/// 错误平台：非支持矩阵三元组失败且不写任何文件。
#[test]
fn installer_rejects_unsupported_platform() {
    let fix = start_http_server();
    let env = InstallerEnv::new();
    publish(&fix, &format!("ailoom-{TRIPLE}"), V1);
    let (code, _, err) = env.run(
        &fix.base(),
        None,
        &[("AILOOM_TRIPLE", "sparc-sun-solaris")],
        &[],
    );
    assert_ne!(code, 0, "未知平台必须失败");
    assert!(err.contains("不支持"), "{err}");
    assert!(!env.bin_dir().join("ailoom").exists(), "不应产出安装物");
}

/// 卸载：只删除 AILoom 安装文件，用户文件与配置不动。
#[test]
fn installer_uninstall_keeps_user_files() {
    let fix = start_http_server();
    let env = InstallerEnv::new();
    publish(&fix, &format!("ailoom-{TRIPLE}"), V1);
    let (code, _, err) = env.run(&fix.base(), None, &[TRIPLE_ENV], &[]);
    assert_eq!(code, 0, "{err}");

    // 用户自己的文件/配置放同一目录（不应被删）
    let marker = env.bin_dir().join("user-settings.json");
    std::fs::write(&marker, b"{\"keep\":true}").unwrap();

    let (code, out, err) = env.run(&fix.base(), None, &[TRIPLE_ENV], &["uninstall"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("已卸载"), "{out}");
    assert!(!env.bin_dir().join("ailoom").exists(), "软链应删除");
    assert!(
        !env.bin_dir().join(format!("ailoom-{TRIPLE}")).exists(),
        "binary 应删除"
    );
    assert!(
        !env.bin_dir()
            .join(format!("ailoom-{TRIPLE}.sha256"))
            .exists(),
        "校验文件应删除"
    );
    assert!(
        marker.is_file(),
        "用户文件必须保留: {}",
        std::fs::read_to_string(&marker).unwrap()
    );

    // 幂等：重复卸载成功
    let (code, _, _) = env.run(&fix.base(), None, &[TRIPLE_ENV], &["uninstall"]);
    assert_eq!(code, 0);
}

/// npm 包装器：本地制品定位正确 binary、sha256 校验通过后透传；包内容无开发机绝对路径。
#[test]
fn npm_wrapper_locates_local_artifact_and_passes_through() {
    let env = InstallerEnv::new();
    // 本地放置制品 + 校验文件（模拟 npm 安装后用户放置/下载完成的 binary）
    let bin_dir = env.tmp.path().join("npm-bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    std::fs::write(bin_dir.join(format!("ailoom-{TRIPLE}")), V1).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            bin_dir.join(format!("ailoom-{TRIPLE}")),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let hash = sha256_of(&bin_dir.join(format!("ailoom-{TRIPLE}")));
    std::fs::write(
        bin_dir.join(format!("ailoom-{TRIPLE}.sha256")),
        format!("{hash}  x\n"),
    )
    .unwrap();

    let repo = std::env::current_dir().unwrap();
    let out = Command::new("node")
        .arg(repo.join("packaging/npm/cli.js"))
        .arg("version-fake-arg")
        .env("HOME", env.tmp.path().join("home"))
        .env("AILOOM_BIN_DIR", &bin_dir)
        .env("AILOOM_TRIPLE", TRIPLE)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(out.status.code(), Some(0), "wrapper 应透传成功");
    assert_eq!(
        stdout, "installed-v1",
        "wrapper 执行了校验过的本地制品: {stdout}"
    );

    // 校验文件缺失 → 拒绝运行
    std::fs::remove_file(bin_dir.join(format!("ailoom-{TRIPLE}.sha256"))).unwrap();
    let out = Command::new("node")
        .arg(repo.join("packaging/npm/cli.js"))
        .env("HOME", env.tmp.path().join("home"))
        .env("AILOOM_BIN_DIR", &bin_dir)
        .env("AILOOM_TRIPLE", TRIPLE)
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0), "缺校验文件必须拒绝");

    // 包内容不含开发机绝对路径或秘密（扫描已发布文件集合）
    for f in [
        "packaging/npm/cli.js",
        "packaging/npm/package.json",
        "packaging/npm/PLATFORMS.md",
    ] {
        let text = std::fs::read_to_string(repo.join(f)).unwrap();
        assert!(!text.contains("/Users/"), "{f} 含绝对路径");
        assert!(
            !text.to_lowercase().contains("secret"),
            "{f} 含疑似秘密字面量"
        );
    }
    // package.json files 白名单不含额外文件
    let pkg: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(repo.join("packaging/npm/package.json")).unwrap(),
    )
    .unwrap();
    let files: Vec<String> = pkg["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        files,
        vec!["cli.js", "PLATFORMS.md"],
        "发布白名单: {files:?}"
    );
}
