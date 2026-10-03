//! Explicit real-filesystem regression. Roots must be owned fixtures on different devices.
//! No HOME override and no writes to the real agents target; run with --ignored and the two roots.
#![cfg(unix)]
use serde_json::Value;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "requires task-owned AILOOM_CROSS_DEVICE_SOURCE_ROOT and AILOOM_CROSS_DEVICE_DATA_ROOT"]
fn takeover_and_restore_cross_devices_preserve_directories_files_and_symlink_bodies() {
    let source_root = std::env::var("AILOOM_CROSS_DEVICE_SOURCE_ROOT").expect("owned source root");
    let data_root = std::env::var("AILOOM_CROSS_DEVICE_DATA_ROOT").expect("owned data root");
    let source = tempfile::Builder::new()
        .prefix("global-cross-source-")
        .tempdir_in(source_root)
        .unwrap();
    let data = tempfile::Builder::new()
        .prefix("global-cross-data-")
        .tempdir_in(data_root)
        .unwrap();
    assert_ne!(
        source.path().metadata().unwrap().dev(),
        data.path().metadata().unwrap().dev()
    );
    let claude = source.path().join("claude");
    let skills = claude.join("skills");
    std::fs::create_dir_all(&skills).unwrap();
    let run = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_ailoom"))
            .args(["--json", "--data-root", data.path().to_str().unwrap()])
            .args(args)
            .current_dir(source.path())
            .env("CLAUDE_CONFIG_DIR", &claude)
            .env("XDG_DATA_HOME", data.path().join("xdg-data"))
            .env("XDG_STATE_HOME", data.path().join("xdg-state"))
            .env("PI_CODING_AGENT_DIR", source.path().join("pi"))
            .output()
            .unwrap();
        let result: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
        let mut logs = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(data.path().join("commands.jsonl"))
            .unwrap();
        use std::io::Write;
        writeln!(logs,"{}",serde_json::json!({"args":args,"exit":output.status.code(),"stdout":result,"stderr":String::from_utf8_lossy(&output.stderr)})).unwrap();
        (output.status.success(), result)
    };
    let ok = |args: &[&str]| {
        let (success, result) = run(args);
        assert!(success, "{args:?}: {result}");
        result["result"].clone()
    };
    ok(&[
        "global", "--action", "select", "--target", "agents", "--state", "disable",
    ]);
    let directory = skills.join("directory");
    std::fs::create_dir_all(directory.join("nested")).unwrap();
    std::fs::write(directory.join("nested/file"), b"DIRECTORY\0CONTENT").unwrap();
    std::fs::set_permissions(
        directory.join("nested/file"),
        std::fs::Permissions::from_mode(0o751),
    )
    .unwrap();
    symlink("nested/file", directory.join("relative")).unwrap();
    symlink("missing", directory.join("broken")).unwrap();
    std::fs::set_permissions(
        directory.join("nested"),
        std::fs::Permissions::from_mode(0o500),
    )
    .unwrap();
    std::fs::write(skills.join("file"), b"RAW\0FILE").unwrap();
    let outside = source.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("preserved"), "external target unchanged").unwrap();
    symlink("../../outside", skills.join("link")).unwrap();
    symlink("missing-destination", skills.join("broken")).unwrap();
    for name in ["directory", "file", "link", "broken"] {
        let entry = skills.join(name);
        let expected_link = std::fs::read_link(&entry).ok();
        let result = ok(&[
            "global", "--action", "takeover", "--target", "claude", "--name", name,
        ]);
        let record = &result["archived"];
        let archive = Path::new(record["archived"].as_str().unwrap());
        assert!(entry.symlink_metadata().is_err());
        if let Some(target) = &expected_link {
            assert_eq!(std::fs::read_link(archive).unwrap(), *target);
        }
        if name == "directory" {
            assert_eq!(
                std::fs::read(archive.join("nested/file")).unwrap(),
                b"DIRECTORY\0CONTENT"
            );
            assert_eq!(
                archive
                    .join("nested/file")
                    .metadata()
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o751
            );
            assert_eq!(
                archive
                    .join("nested")
                    .metadata()
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o500
            );
            assert_eq!(
                std::fs::read_link(archive.join("relative")).unwrap(),
                Path::new("nested/file")
            );
            assert_eq!(
                std::fs::read_link(archive.join("broken")).unwrap(),
                Path::new("missing")
            );
        }
        if name == "file" {
            assert_eq!(std::fs::read(archive).unwrap(), b"RAW\0FILE");
        }
        // Occupied original path must leave both the occupant and archive untouched.
        std::fs::write(&entry, "occupant").unwrap();
        let id = record["id"].as_str().unwrap();
        assert!(!run(&["global", "--action", "restore", "--id", id]).0);
        assert_eq!(std::fs::read_to_string(&entry).unwrap(), "occupant");
        assert!(archive.symlink_metadata().is_ok());
        std::fs::remove_file(&entry).unwrap();
        ok(&["global", "--action", "restore", "--id", id]);
        assert!(archive.symlink_metadata().is_err());
        if let Some(target) = expected_link {
            assert_eq!(std::fs::read_link(&entry).unwrap(), target);
        }
        if name == "directory" {
            assert_eq!(
                std::fs::read(entry.join("nested/file")).unwrap(),
                b"DIRECTORY\0CONTENT"
            );
            assert_eq!(
                entry
                    .join("nested")
                    .metadata()
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o500
            );
            std::fs::set_permissions(entry.join("nested"), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        if name == "file" {
            assert_eq!(std::fs::read(entry).unwrap(), b"RAW\0FILE");
        }
    }
    // Special-file failure must keep the source and leave no empty archive directory.
    use std::os::unix::ffi::OsStrExt;
    let fifo = skills.join("fifo");
    let raw = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(raw.as_ptr(), 0o600) }, 0);
    let before = std::fs::read_dir(data.path().join("global/archive"))
        .unwrap()
        .count();
    assert!(!run(&["global", "--action", "takeover", "--target", "claude", "--name", "fifo"]).0);
    assert!(fifo.symlink_metadata().is_ok());
    assert_eq!(
        std::fs::read_dir(data.path().join("global/archive"))
            .unwrap()
            .count(),
        before
    );
    assert_eq!(
        std::fs::read_to_string(outside.join("preserved")).unwrap(),
        "external target unchanged"
    );
    assert!(std::fs::read_dir(&skills).unwrap().all(|e| !e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".ailoom-")));
    println!(
        "source fixture: {}\ndata fixture: {}",
        source.path().display(),
        data.path().display()
    );
    if std::env::var_os("AILOOM_KEEP_CROSS_DEVICE_FIXTURE").is_some() {
        let _ = source.keep();
        let _ = data.keep();
    }
}
