//! A separate process owns POSIX locks, so reader descriptor closes cannot release them.
use super::*;
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Write},
    os::unix::{
        fs::MetadataExt,
        io::{AsRawFd, IntoRawFd},
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
const HELPER: &str = "__store-writer-exclusion";
const TIMEOUT: Duration = Duration::from_secs(10);
fn failure(message: impl std::fmt::Display) -> Error {
    Error::Storage(format!("Writer exclusion: {message}"))
}
fn identity(path: &Path) -> Result<(u64, u64)> {
    let lock_path = path.join("LOCK");
    let m = std::fs::metadata(&lock_path)
        .map_err(|error| failure(format!("{} is unavailable: {error}", lock_path.display())))?;
    if !m.is_file() {
        return Err(failure("LOCK must be a regular file"));
    }
    Ok((m.dev(), m.ino()))
}
/// Retains all requested existing LOCK files in an independent helper process.
/// Call `finish` after all reads and comparisons, before publishing their result.
pub struct WriterExclusion {
    child: Child,
    paths: Vec<(PathBuf, (u64, u64))>,
    finished: bool,
}
impl WriterExclusion {
    /// The executable must dispatch `writer_exclusion_helper_command` before normal startup.
    pub fn acquire(paths: &[PathBuf]) -> Result<Self> {
        Self::acquire_with_executable(paths, &std::env::current_exe().map_err(failure)?)
    }
    /// Explicit executable selection supports independently hosted operator applications.
    pub fn acquire_with_executable(paths: &[PathBuf], executable: &Path) -> Result<Self> {
        if paths.is_empty() || paths.len() > 6 {
            return Err(failure("expected one to six stores"));
        }
        let mut seen = BTreeSet::new();
        let mut checked = Vec::new();
        for path in paths {
            let path = path.canonicalize().map_err(|error| failure(format!("Store {} is unavailable: {error}", path.display())))?;
            let id = identity(&path)?;
            if !seen.insert(id) {
                return Err(failure("aliased LOCK files"));
            }
            checked.push((path, id));
        }
        let child = Command::new(executable)
            .arg(HELPER)
            .args(checked.iter().map(|(p, _)| p))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(failure)?;
        let mut guard = Self {
            child,
            paths: checked,
            finished: false,
        };
        let output = guard
            .child
            .stdout
            .as_mut()
            .ok_or_else(|| failure("missing readiness pipe"))?;
        let mut poll = libc::pollfd {
            fd: output.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll points to one initialized descriptor for this synchronous call.
        let ready = unsafe { libc::poll(&mut poll, 1, TIMEOUT.as_millis() as i32) };
        if ready <= 0 {
            return Err(failure("helper readiness failed or timed out"));
        }
        let mut byte = [0];
        output.read_exact(&mut byte).map_err(|error| failure(format!(
            "could not acquire every store lock; ensure all source and candidate stores are stopped: {error}"
        )))?;
        if byte != [b'R'] {
            return Err(failure("helper could not acquire every lock"));
        }
        guard.check()?;
        Ok(guard)
    }
    /// PID for operator diagnostics and controlled isolated recovery exercises.
    pub fn helper_pid(&self) -> u32 {
        self.child.id()
    }
    /// Reject a dead helper or a replaced lock inode before accepting any result.
    pub fn check(&mut self) -> Result<()> {
        if self.finished || self.child.try_wait().map_err(failure)?.is_some() {
            return Err(failure("lock helper is no longer alive"));
        }
        for (path, id) in &self.paths {
            if identity(path)? != *id {
                return Err(failure("LOCK identity changed"));
            }
        }
        Ok(())
    }
    pub(super) fn check_path(&mut self, path: &Path) -> Result<()> {
        self.check()?;
        let path = path.canonicalize().map_err(|error| failure(format!("Store {} is unavailable: {error}", path.display())))?;
        if !self.paths.iter().any(|(p, _)| *p == path) {
            return Err(failure("store is outside retained exclusion"));
        }
        Ok(())
    }
    /// Confirm the helper remained alive through the operation, then release all locks.
    pub fn finish(mut self) -> Result<()> {
        self.check()?;
        self.child
            .stdin
            .as_mut()
            .ok_or_else(|| failure("missing control pipe"))?
            .write_all(b"F")
            .map_err(failure)?;
        self.child.stdin.take();
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().map_err(failure)? {
                self.finished = true;
                return if status.success() {
                    Ok(())
                } else {
                    Err(failure("helper failed before confirmed release"))
                };
            }
            if Instant::now() >= deadline {
                return Err(failure("helper release timed out"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for WriterExclusion {
    fn drop(&mut self) {
        if !self.finished {
            self.child.stdin.take();
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
/// Internal subprocess dispatch; returns false for normal command lines.
/// A missing parent pipe or any incomplete acquisition exits unsuccessfully.
pub fn writer_exclusion_helper_command(args: &[String]) -> Result<bool> {
    if args.first().map(String::as_str) != Some(HELPER) {
        return Ok(false);
    }
    if !(2..=7).contains(&args.len()) {
        return Err(failure("invalid helper arguments"));
    }
    let mut files: Vec<File> = Vec::new();
    let mut seen = BTreeSet::new();
    for path in &args[1..] {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(Path::new(path).join("LOCK"))
            .map_err(failure)?;
        let meta = file.metadata().map_err(failure)?;
        if !meta.is_file() || !seen.insert((meta.dev(), meta.ino())) {
            return Err(failure("invalid or aliased LOCK file"));
        }
        // SAFETY: zeroed flock is initialized below and the file remains owned until release.
        let mut lock: libc::flock = unsafe { std::mem::zeroed() };
        lock.l_type = libc::F_WRLCK as libc::c_short;
        lock.l_whence = libc::SEEK_SET as libc::c_short;
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLK, &lock) } == -1 {
            return Err(failure(std::io::Error::last_os_error()));
        }
        files.push(file);
    }
    std::io::stdout().write_all(b"R").map_err(failure)?;
    std::io::stdout().flush().map_err(failure)?;
    let mut command = [0];
    std::io::stdin().read_exact(&mut command).map_err(failure)?;
    if command != [b'F'] {
        return Err(failure("parent did not confirm completion"));
    }
    let mut release_error = None;
    for file in files {
        // SAFETY: into_raw_fd transfers the sole owned descriptor; close is called once.
        if unsafe { libc::close(file.into_raw_fd()) } != 0 {
            release_error = Some(std::io::Error::last_os_error());
        }
    }
    if let Some(error) = release_error {
        return Err(failure(error));
    }
    Ok(true)
}
