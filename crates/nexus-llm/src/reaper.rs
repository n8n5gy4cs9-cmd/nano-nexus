//! `server.json` bookkeeping and stale-process reaping (INFERENCE-CONTRACT §3).
//!
//! If the app died without stopping its server, `server.json` records the orphan. On the next launch
//! we kill it, but only if the pid is alive *and* its executable is our `llama-server` (a reused pid
//! belonging to an unrelated process must never be killed).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Contents of `<data>/runtime/server.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub pid: u32,
    pub port: u16,
    /// Unix milliseconds (UTC).
    pub started_at: i64,
    pub exe: PathBuf,
}

impl ServerInfo {
    pub fn new(pid: u32, port: u16, exe: PathBuf) -> Self {
        let started_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Self {
            pid,
            port,
            started_at,
            exe,
        }
    }

    /// Write the file (creating parent directories).
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    /// Read the file; `None` if it is missing or unparsable.
    pub fn read(path: &Path) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    /// Delete the file (ignores "not found").
    pub fn remove(path: &Path) {
        let _ = std::fs::remove_file(path);
    }
}

/// What [`reap_stale`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaped {
    /// No `server.json`.
    NoRecord,
    /// The recorded process is gone; the stale file was removed.
    AlreadyGone,
    /// The pid is alive but is not our server; nothing was killed, the file was removed.
    NotOurs,
    /// An orphaned server was killed.
    Killed(u32),
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Kill an orphaned server recorded in `server_json`, if (and only if) it is `our_exe`.
pub fn reap_stale(server_json: &Path, our_exe: &Path) -> Reaped {
    let Some(info) = ServerInfo::read(server_json) else {
        let existed = server_json.exists();
        ServerInfo::remove(server_json);
        return if existed {
            Reaped::AlreadyGone
        } else {
            Reaped::NoRecord
        };
    };
    let pid = Pid::from_u32(info.pid);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );
    let outcome = match sys.process(pid) {
        None => Reaped::AlreadyGone,
        Some(p) => match p.exe() {
            Some(exe) if same_file(exe, our_exe) => {
                tracing::warn!(
                    pid = info.pid,
                    "killing orphaned llama-server from a previous run"
                );
                p.kill();
                Reaped::Killed(info.pid)
            }
            _ => Reaped::NotOurs,
        },
    };
    ServerInfo::remove(server_json);
    outcome
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn sleeper() -> std::process::Child {
        Command::new("sleep")
            .arg("300")
            .stdin(Stdio::null())
            .spawn()
            .unwrap()
    }

    fn exe_of(pid: u32) -> PathBuf {
        std::fs::read_link(format!("/proc/{pid}/exe")).unwrap_or_else(|_| PathBuf::from("/bin/sleep"))
    }

    #[test]
    fn info_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("runtime/server.json");
        let info = ServerInfo::new(42, 5555, PathBuf::from("/x/llama-server"));
        info.write(&p).unwrap();
        assert_eq!(ServerInfo::read(&p), Some(info));
        ServerInfo::remove(&p);
        assert!(ServerInfo::read(&p).is_none());
    }

    #[test]
    fn no_record() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            reap_stale(&dir.path().join("server.json"), Path::new("/x")),
            Reaped::NoRecord
        );
    }

    #[test]
    fn kills_orphan_with_matching_exe() {
        let dir = tempfile::tempdir().unwrap();
        let json = dir.path().join("server.json");
        let mut child = sleeper();
        let exe = exe_of(child.id());
        ServerInfo::new(child.id(), 1, exe.clone()).write(&json).unwrap();
        assert_eq!(reap_stale(&json, &exe), Reaped::Killed(child.id()));
        assert!(!json.exists());
        let status = child.wait().unwrap();
        assert!(!status.success());
    }

    #[test]
    fn never_kills_an_unrelated_process_with_the_recorded_pid() {
        let dir = tempfile::tempdir().unwrap();
        let json = dir.path().join("server.json");
        let mut child = sleeper();
        ServerInfo::new(child.id(), 1, PathBuf::from("/opt/nanonexus/llama-server"))
            .write(&json)
            .unwrap();
        assert_eq!(
            reap_stale(&json, Path::new("/opt/nanonexus/llama-server")),
            Reaped::NotOurs
        );
        assert!(!json.exists());
        assert!(
            child.try_wait().unwrap().is_none(),
            "unrelated process was killed!"
        );
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn dead_pid_just_clears_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let json = dir.path().join("server.json");
        let mut child = sleeper();
        let (pid, exe) = (child.id(), exe_of(child.id()));
        child.kill().unwrap();
        child.wait().unwrap();
        ServerInfo::new(pid, 1, exe.clone()).write(&json).unwrap();
        assert_eq!(reap_stale(&json, &exe), Reaped::AlreadyGone);
        assert!(!json.exists());
    }

    #[test]
    fn corrupt_file_is_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let json = dir.path().join("server.json");
        std::fs::write(&json, "not json").unwrap();
        assert_eq!(reap_stale(&json, Path::new("/x")), Reaped::AlreadyGone);
        assert!(!json.exists());
    }
}
