use spicetify_guard_lib::{get_versions_from_config, Cache};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

fn main() {
    if let Err(e) = run_guard() {
        let _ = log_line(&format!("fatal: {e}"));
        std::process::exit(1);
    }
}

#[derive(Default, Debug)]
struct StatusInfo {
    applied: bool,
    mismatch_or_stock: bool,
    spotify_ver: Option<String>,
    backup_ver: Option<String>,
}

fn run_guard() -> Result<(), String> {
    log_line("boot guard start")?;

    // 1) Installation check
    let has_config = has_config_xpui();
    let spicetify_ok = cmd_ok(&spicetify_bin(), &["--version"], Some(Duration::from_secs(timeout_secs())));
    log_line(&format!("has_config={has_config}, spicetify_ok={spicetify_ok}"))?;
    let installed = has_config && spicetify_ok;
    log_line(&format!("installed={installed}"))?;

    if !installed {
        log_line("not installed → skipping heavy work (optional: run installer here)")?;
        return Ok(());
    }

    // 2) Load cache
    let cache_path = cache_file();
    let mut cache = load_cache(&cache_path).unwrap_or_default();

    // 3) Parse status
    let (spotify_ver, backup_ver) = get_versions_from_config();
    let info = StatusInfo {
        applied: spotify_ver.is_some(),
        mismatch_or_stock: spotify_ver != backup_ver,
        spotify_ver,
        backup_ver,
    };
    log_line(&format!(
        "status: applied={}, mismatch_or_stock={}, spotify_ver={:?}, backup_ver={:?}",
        info.applied, info.mismatch_or_stock, info.spotify_ver, info.backup_ver
    ))?;

    // 4) Early exit: applied + versions match + recently OK
    let versions_match = info.spotify_ver.is_some() && info.spotify_ver == info.backup_ver;
    let recently_ok = cache
        .last_ok
        .map(|ts| {
            SystemTime::now()
                .duration_since(ts)
                .unwrap_or_default()
                .as_secs() < 12 * 3600
        })
        .unwrap_or(false);
    if info.applied && versions_match && recently_ok {
        log_line("skip: already applied, versions match, recently ok")?;
        return Ok(());
    }

    // 5) If nothing needs to be changed at all: Applied + versions match now
    if info.applied && versions_match {
        log_line("skip: already applied and versions match (no commands needed)")?;
        cache.last_ok = Some(SystemTime::now());
        cache.spotify_ver = info.spotify_ver.clone();
        cache.backup_ver = info.backup_ver.clone();
        save_cache(&cache_path, &cache)?;
        return Ok(());
    }

    // 6) Need modification → stop Spotify once before changes
    let need_mod = info.mismatch_or_stock || !info.applied || !versions_match;
    if need_mod {
        let _ = must_ok_cmd("taskkill", &["/F", "/IM", "Spotify.exe"], timeout_secs(), "kill spotify");
    }

    // 7) Do ops based on specific case: primary install, upgrade, or match
    if !info.applied {
        // Primary application: backup and apply
        must_ok(&spicetify_bin(), &["-n", "backup", "--bypass-admin"], timeout_secs(), "backup")?;  // create backup
        must_ok(&spicetify_bin(), &["-n", "apply", "--bypass-admin"], timeout_secs(), "apply")?;   // apply modifications
        must_ok(&spicetify_bin(), &["restart", "--bypass-admin"], timeout_secs(), "restart")?;    // single restart
    } else if info.spotify_ver != info.backup_ver {
        // Spotify version upgrade → restore backup and re-apply modifications
        must_ok(&spicetify_bin(), &["-n", "restore", "backup", "--bypass-admin"], timeout_secs(), "restore backup")?; // restore→backup
        must_ok(&spicetify_bin(), &["-n", "apply", "--bypass-admin"], timeout_secs(), "apply")?;                      // re-apply
        must_ok(&spicetify_bin(), &["restart", "--bypass-admin"], timeout_secs(), "restart")?;                       // single restart
    } else {
        // Versions match → skip without update/restart
        log_line("skip: applied and versions match (no commands)")?;
    }

    // 8) Update cache
    cache.last_ok = Some(SystemTime::now());
    cache.spotify_ver = info.spotify_ver;
    cache.backup_ver = info.backup_ver;
    save_cache(&cache_path, &cache)?;
    log_line("boot guard done")?;
    Ok(())
}

fn timeout_secs() -> u64 {
    std::env::var("SPICE_GUARD_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(600)
}

fn has_config_xpui() -> bool {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    Path::new(&appdata)
        .join("spicetify")
        .join("config-xpui.ini")
        .exists()
}

fn spicetify_bin() -> String {
    // Prefer explicit local exe; for .bat rely on PATH to avoid CreateProcess quirks
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let exe = Path::new(&local).join("spicetify").join("spicetify.exe");
        if exe.exists() {
            return exe.to_string_lossy().to_string();
        }
        let bat = Path::new(&local).join("spicetify").join("spicetify.bat");
        if bat.exists() {
            return "spicetify".to_string();
        }
    }
    "spicetify".to_string()
}

fn cmd_ok(bin: &str, args: &[&str], timeout: Option<Duration>) -> bool {
    let start = Instant::now();
    let mut child = match Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {
                if let Some(t) = timeout {
                    if start.elapsed() > t {
                        let _ = child.kill();
                        return false;
                    }
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            Err(_) => return false,
        }
    }
}

fn must_ok(bin: &str, args: &[&str], timeout_s: u64, label: &str) -> Result<(), String> {
    let start = Instant::now();
    log_line(&format!("executing: {} {} ({})", bin, args.join(" "), label))?;
    if !cmd_ok(bin, args, Some(Duration::from_secs(timeout_s))) {
        return Err(format!("command failed: {} {}", bin, args.join(" ")));
    }
    let elapsed_str = format!("{:?}", start.elapsed());
    log_line(&format!("finished in {} ({})", elapsed_str, label))?;
    Ok(())
}

fn must_ok_cmd(bin: &str, args: &[&str], timeout_s: u64, label: &str) -> Result<(), String> {
    let start = Instant::now();
    log_line(&format!("executing: {} {} ({})", bin, args.join(" "), label))?;
    if !cmd_ok(bin, args, Some(Duration::from_secs(timeout_s))) {
        // Log failure but do not bubble up
        log_line(&format!("command failed: {} {}", bin, args.join(" ")))?;
    }
    let elapsed_str = format!("{:?}", start.elapsed());
    log_line(&format!("finished in {} ({})", elapsed_str, label))?;
    Ok(())
}

// ---------- Cache ----------
fn cache_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    Path::new(&appdata).join("Spotify")
}

fn cache_file() -> PathBuf {
    cache_dir().join("spicetify_boot_guard_cache.json")
}

fn load_cache(p: &Path) -> Option<Cache> {
    let data = fs::read(p).ok()?;
    serde_json::from_slice(&data).ok()
}

fn save_cache(p: &Path, c: &Cache) -> Result<(), String> {
    let dir = cache_dir();
    let _ = fs::create_dir_all(&dir);
    let data = serde_json::to_vec_pretty(c).map_err(|e| e.to_string())?;
    fs::write(p, data).map_err(|e| e.to_string())
}

// ---------- Log ----------
fn log_line(s: &str) -> Result<(), String> {
    let dir = cache_dir();
    let _ = fs::create_dir_all(&dir);
    let log_path = dir.join("spicetify_boot_guard.log");
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| e.to_string())?;
    let now = chrono_like_now();
    let line = format!("[{}] {}\r\n", now, s);
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())
}

fn chrono_like_now() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", now)
}