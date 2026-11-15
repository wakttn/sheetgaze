use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{
    Config as NotifyConfig, Event as NotifyEvent, PollWatcher, RecommendedWatcher, RecursiveMode,
    Watcher,
};
use thiserror::Error;

use crate::domain::reload::{WatchConfig, WatchMode};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchEvent {
    Changed,
    BackendError(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchNotice {
    Fallback { from: WatchMode, reason: String },
    Unavailable(String),
}

#[derive(Debug, Error)]
pub enum WatchError {
    #[error("could not register file watch: {0}")]
    Registration(String),
}

pub struct WatchStartup {
    pub session: WatchSession,
    pub effective: WatchMode,
    pub notice: Option<WatchNotice>,
}

pub struct WatchSession {
    _backend: Backend,
}

enum Backend {
    Poll { _watcher: PollWatcher },
    Native { _watcher: RecommendedWatcher },
    None,
}

impl WatchSession {
    pub fn start<F>(
        path: &Path,
        config: &WatchConfig,
        handler: F,
    ) -> Result<WatchStartup, WatchError>
    where
        F: Fn(WatchEvent) + Send + Sync + 'static,
    {
        if config.mode == WatchMode::Off {
            return Ok(WatchStartup {
                session: Self {
                    _backend: Backend::None,
                },
                effective: WatchMode::Off,
                notice: None,
            });
        }
        let watch_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let path = watch_path.as_path();
        let handler = Arc::new(handler);
        let requested = config.mode;
        let effective = match requested {
            WatchMode::Auto => resolve_auto_mode(path),
            mode => mode,
        };
        if effective == WatchMode::Poll {
            match create_poll(path, config, handler) {
                Ok(watcher) => {
                    return Ok(WatchStartup {
                        session: Self {
                            _backend: Backend::Poll { _watcher: watcher },
                        },
                        effective,
                        notice: None,
                    });
                }
                Err(error) => {
                    return Err(WatchError::Registration(error.to_string()));
                }
            }
        }
        match create_native(path, handler.clone()) {
            Ok(watcher) => Ok(WatchStartup {
                session: Self {
                    _backend: Backend::Native { _watcher: watcher },
                },
                effective: WatchMode::Native,
                notice: None,
            }),
            Err(error) => match create_poll(path, config, handler) {
                Ok(watcher) => Ok(WatchStartup {
                    session: Self {
                        _backend: Backend::Poll { _watcher: watcher },
                    },
                    effective: WatchMode::Poll,
                    notice: Some(WatchNotice::Fallback {
                        from: requested,
                        reason: error.to_string(),
                    }),
                }),
                Err(poll_error) => Err(WatchError::Registration(format!(
                    "native: {error}; poll: {poll_error}"
                ))),
            },
        }
    }

    pub fn stop(self) {
        drop(self);
    }
}

fn create_poll<F>(path: &Path, config: &WatchConfig, handler: Arc<F>) -> notify::Result<PollWatcher>
where
    F: Fn(WatchEvent) + Send + Sync + 'static,
{
    let notify_config = NotifyConfig::default()
        .with_poll_interval(config.poll_interval)
        .with_compare_contents(false);
    let mut watcher = PollWatcher::new(
        move |result| forward(result, handler.as_ref()),
        notify_config,
    )?;
    watch_paths(&mut watcher, path)?;
    Ok(watcher)
}

fn create_native<F>(path: &Path, handler: Arc<F>) -> notify::Result<RecommendedWatcher>
where
    F: Fn(WatchEvent) + Send + Sync + 'static,
{
    let mut watcher = RecommendedWatcher::new(
        move |result| forward(result, handler.as_ref()),
        NotifyConfig::default(),
    )?;
    watch_paths(&mut watcher, path)?;
    Ok(watcher)
}

fn watch_paths(watcher: &mut impl Watcher, path: &Path) -> notify::Result<()> {
    watcher.watch(path, RecursiveMode::NonRecursive)?;
    if let Some(parent) = path.parent()
        && parent != path
    {
        watcher.watch(parent, RecursiveMode::NonRecursive)?;
    }
    Ok(())
}

fn forward<F>(result: notify::Result<NotifyEvent>, handler: &F)
where
    F: Fn(WatchEvent),
{
    match result {
        Ok(_) => handler(WatchEvent::Changed),
        Err(error) => handler(WatchEvent::BackendError(error.to_string())),
    }
}

#[cfg(target_os = "linux")]
pub fn resolve_auto_mode(path: &Path) -> WatchMode {
    let probe = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let Some(fs_type) = linux_mount_fs_type(&probe) else {
        return WatchMode::Poll;
    };
    if is_remote_or_virtual_fs(&fs_type) {
        WatchMode::Poll
    } else {
        WatchMode::Native
    }
}

#[cfg(target_os = "windows")]
pub fn resolve_auto_mode(_path: &Path) -> WatchMode {
    WatchMode::Poll
}

#[cfg(target_os = "macos")]
pub fn resolve_auto_mode(_path: &Path) -> WatchMode {
    WatchMode::Native
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub fn resolve_auto_mode(_path: &Path) -> WatchMode {
    WatchMode::Poll
}

#[cfg(target_os = "linux")]
fn linux_mount_fs_type(path: &Path) -> Option<String> {
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
    mount_fs_type(&mountinfo, path)
}

pub fn is_remote_or_virtual_fs(fs_type: &str) -> bool {
    fs_type == "9p"
        || fs_type == "virtiofs"
        || fs_type == "drvfs"
        || fs_type.starts_with("fuse")
        || fs_type == "cifs"
        || fs_type == "smb3"
        || fs_type == "nfs"
        || fs_type == "nfs4"
        || fs_type == "sshfs"
        || fs_type == "davfs"
        || fs_type == "webdav"
        || fs_type == "afs"
        || fs_type == "ceph"
        || fs_type == "glusterfs"
}

pub fn mount_fs_type(mountinfo: &str, path: &Path) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for line in mountinfo.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let Some(mount_point) = left.split_whitespace().nth(4) else {
            continue;
        };
        let Some(fs_type) = right.split_whitespace().next() else {
            continue;
        };
        let mount_point = unescape_mount_point(mount_point);
        let mount_path = PathBuf::from(&mount_point);
        if !path.starts_with(&mount_path) {
            continue;
        }
        let length = mount_point.len();
        if best
            .as_ref()
            .is_none_or(|(best_len, _)| length >= *best_len)
        {
            best = Some((length, fs_type.to_string()));
        }
    }
    best.map(|(_, fs_type)| fs_type)
}

fn unescape_mount_point(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let digits: String = chars.by_ref().take(3).collect();
            if digits.len() == 3
                && let Ok(byte) = u8::from_str_radix(&digits, 8)
            {
                result.push(byte as char);
                continue;
            }
            result.push('\\');
            result.push_str(&digits);
        } else {
            result.push(ch);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounts_select_longest_matching_point() {
        let mountinfo = "\
1 0 0:1 / / rw - ext4 /dev/root rw
2 0 0:2 / /mnt/c rw - 9p C:\\ rw
3 0 0:3 / /mnt/c/Users rw - drvfs C:\\Users rw
";
        assert_eq!(
            mount_fs_type(mountinfo, Path::new("/home/user/book.xlsx")).as_deref(),
            Some("ext4")
        );
        assert_eq!(
            mount_fs_type(mountinfo, Path::new("/mnt/c/Users/me/book.xlsx")).as_deref(),
            Some("drvfs")
        );
    }

    #[test]
    fn virtual_filesystems_are_detected() {
        for fs_type in ["9p", "drvfs", "fuse.sshfs", "cifs", "nfs4", "virtiofs"] {
            assert!(is_remote_or_virtual_fs(fs_type), "{fs_type}");
        }
        assert!(!is_remote_or_virtual_fs("ext4"));
        assert!(!is_remote_or_virtual_fs("overlay"));
    }

    #[test]
    fn mount_points_are_unescaped() {
        assert_eq!(unescape_mount_point("/mnt/Box\\040Drive"), "/mnt/Box Drive");
    }

    #[test]
    fn off_mode_starts_without_backend() {
        let config = WatchConfig {
            mode: WatchMode::Off,
            ..WatchConfig::default()
        };
        let startup = WatchSession::start(Path::new("/tmp/book.csv"), &config, |_| {}).unwrap();
        assert_eq!(startup.effective, WatchMode::Off);
        assert!(startup.notice.is_none());
    }
}
