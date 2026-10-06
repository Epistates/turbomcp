//! A stdio upstream's process: its stderr into `tracing`, and the spec's
//! shutdown sequence at the end.

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;

/// A running stdio upstream. Dropping it kills the process group; prefer
/// [`shutdown`](Self::shutdown), which gives the server the spec's chance to
/// exit on its own.
pub(crate) struct ChildProcess {
    child: Child,
    grace: Duration,
}

impl ChildProcess {
    /// Take over `child`, logging what it writes to stderr under `label`
    /// ("The server MAY write UTF-8 strings to its standard error (stderr)
    /// for any logging purposes ... Clients MAY capture, forward, or ignore
    /// this logging").
    pub(crate) fn new(mut child: Child, label: String, grace: Duration) -> Self {
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::info!(target: "turbomcp_proxy::upstream", upstream = %label, "{line}");
                }
            });
        }
        Self { child, grace }
    }

    /// The spec's stdio shutdown: "the client SHOULD initiate shutdown by
    /// first closing the input stream to the child process (the server),
    /// waiting for the server to exit, or sending SIGTERM if the server does
    /// not exit within a reasonable time; and sending SIGKILL if the server
    /// does not exit within a reasonable time after SIGTERM." Stdin is
    /// already closed by the client; this waits, then signals.
    pub(crate) async fn shutdown(mut self) {
        if tokio::time::timeout(self.grace, self.child.wait())
            .await
            .is_ok()
        {
            return;
        }
        self.signal(Signal::Term);
        if tokio::time::timeout(self.grace, self.child.wait())
            .await
            .is_ok()
        {
            return;
        }
        self.signal(Signal::Kill);
        let _ = self.child.wait().await;
    }

    /// Signal the whole process group: a wrapper (`npx`, `uv run`) starts the
    /// real server as its own child, which signalling the wrapper alone would
    /// leave running.
    fn signal(&mut self, signal: Signal) {
        #[cfg(unix)]
        if let Some(pid) = self
            .child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .and_then(rustix::process::Pid::from_raw)
        {
            let signal = match signal {
                Signal::Term => rustix::process::Signal::TERM,
                Signal::Kill => rustix::process::Signal::KILL,
            };
            let _ = rustix::process::kill_process_group(pid, signal);
            return;
        }
        let _ = signal;
        let _ = self.child.start_kill();
    }
}

impl Drop for ChildProcess {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            self.signal(Signal::Kill);
        }
    }
}

#[derive(Clone, Copy)]
enum Signal {
    Term,
    Kill,
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Stdio;

    /// A server that ignores `SIGTERM` and starts a grandchild (as `npx` does)
    /// is still gone after shutdown, grandchild included.
    #[tokio::test]
    async fn shutdown_escalates_and_reaches_the_whole_group() {
        let dir = std::env::temp_dir().join(format!("turbomcp-proxy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pid_file = dir.join("grandchild.pid");
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c")
            .arg(format!(
                "trap '' TERM; sleep 60 & echo $! > {}; while true; do sleep 1; done",
                pid_file.display()
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .process_group(0);
        let child = cmd.spawn().unwrap();
        let mut grandchild = None;
        for _ in 0..200 {
            if let Some(pid) = std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|s| s.trim().parse::<i32>().ok())
            {
                grandchild = rustix::process::Pid::from_raw(pid);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let grandchild = grandchild.expect("the grandchild started");
        let started = std::time::Instant::now();
        ChildProcess::new(child, "test".into(), Duration::from_millis(100))
            .shutdown()
            .await;
        assert!(started.elapsed() < Duration::from_secs(5));
        let mut gone = false;
        for _ in 0..200 {
            if rustix::process::test_kill_process(grandchild).is_err() {
                gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(gone, "the grandchild outlived shutdown");
    }
}
