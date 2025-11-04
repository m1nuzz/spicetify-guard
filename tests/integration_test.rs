use assert_cmd::prelude::*;
use assert_fs::prelude::*;
use predicates::prelude::*;
use std::process::Command;

#[test]
fn test_not_installed() {
    let temp = assert_fs::TempDir::new().unwrap();
    let log_file = temp.child("Spotify/spicetify_boot_guard.log");
    let mut cmd = Command::cargo_bin("spicetify_boot_guard_smart").unwrap();
    cmd.env("APPDATA", temp.path());
    cmd.assert().success();
    log_file.assert(predicate::str::contains("not installed"));
}

#[test]
fn test_up_to_date() {
    let temp = assert_fs::TempDir::new().unwrap();
    // Fake spicetify in PATH as .bat
    let spicetify_dir = temp.child("spicetify");
    spicetify_dir.create_dir_all().unwrap();
    let spicetify_bat = spicetify_dir.child("spicetify.bat");
    spicetify_bat.write_str("@echo off\r\necho spicetify --version>nul\r\nexit /b 0\r\n").unwrap();

    // Config with matching versions
    let config_file = spicetify_dir.child("config-xpui.ini");
    config_file.write_str("[Backup]\nversion = 1.2.3\nwith = 1.2.3\n").unwrap();

    // Cache marks recent OK
    let spotify_dir = temp.child("Spotify");
    spotify_dir.create_dir_all().unwrap();
    let cache_file = spotify_dir.child("spicetify_boot_guard_cache.json");
    let cache = spicetify_boot_guard_smart_lib::Cache {
        last_ok: Some(std::time::SystemTime::now()),
        spotify_ver: Some("1.2.3".to_string()),
        backup_ver: Some("1.2.3".to_string()),
    };
    let cache_json = serde_json::to_string(&cache).unwrap();
    cache_file.write_str(&cache_json).unwrap();

    let log_file = spotify_dir.child("spicetify_boot_guard.log");
    let mut cmd = Command::cargo_bin("spicetify_boot_guard_smart").unwrap();
    cmd.env("APPDATA", temp.path());
    cmd.env("LOCALAPPDATA", temp.path()); // so spicetify_bin() sees local structure
    cmd.env("PATH", spicetify_dir.path());
    cmd.assert().success();
    log_file.assert(predicate::str::contains(
        "skip: already applied, versions match, recently ok",
    ));
}

#[test]
fn test_mismatch_with_no_restart_chain() {
    let temp = assert_fs::TempDir::new().unwrap();
    // Fake spicetify in PATH writes args into %APPDATA%\spicetify_args.log
    let spicetify_dir = temp.child("spicetify");
    spicetify_dir.create_dir_all().unwrap();
    let spicetify_bat = spicetify_dir.child("spicetify.bat");
    spicetify_bat.write_str("@echo off\r\necho %*>> %APPDATA%\\spicetify_args.log\r\nexit /b 0\r\n").unwrap();

    // Config with version mismatch
    let config_file = spicetify_dir.child("config-xpui.ini");
    config_file.write_str("[Backup]\nversion = 1.2.3\nwith = 4.5.6\n").unwrap();

    // Prepare dirs for cache/logs
    let spotify_dir = temp.child("Spotify");
    spotify_dir.create_dir_all().unwrap();
    let log_file = spotify_dir.child("spicetify_boot_guard.log");
    let spicetify_args_log = temp.child("spicetify_args.log");

    // Run
    let mut cmd = Command::cargo_bin("spicetify_boot_guard_smart").unwrap();
    let path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{};{}", spicetify_dir.path().to_str().unwrap(), path);
    cmd.env("APPDATA", temp.path());
    cmd.env("LOCALAPPDATA", temp.path());
    cmd.env("PATH", new_path);
    cmd.env("SPICE_GUARD_TIMEOUT", "600");
    cmd.assert().success();

    // Debug print (helpful when test fails)
    println!("--- log_file ---\n{}", std::fs::read_to_string(log_file.path()).unwrap());
    println!("--- spicetify_args_log ---\n{}", std::fs::read_to_string(spicetify_args_log.path()).unwrap());

    // Assertions
    log_file.assert(predicate::str::contains("status: applied=true, mismatch_or_stock=true"));
    let args_content = std::fs::read_to_string(spicetify_args_log.path()).unwrap();
    assert!(args_content.contains("-n restore backup"));
    assert!(args_content.contains("-n backup"));
    assert!(args_content.contains("-n apply"));
    assert!(args_content.contains("-n update"));
    let restart_count = args_content.matches("restart").count();
    assert_eq!(restart_count, 1, "expected single 'restart' at the end");
    // Verify that --bypass-admin is included with the restart command
    assert!(args_content.contains("restart --bypass-admin"), "restart should include --bypass-admin flag");
}