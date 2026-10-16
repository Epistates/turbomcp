//! A server running as this process's child: the spec's stdio shutdown
//! sequence, and its stderr into `tracing`.

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;

/// A server process launched for a stdio connection ([`connect_child`]).
///
/// [`shutdown`](Self::shutdown) is the stdio transport's sequence: "the
/// client SHOULD initiate shutdown by first closing the input stream to the
/// child process (the server), waiting for the server to exit, or sending
/// SIGTERM if the server does not exit within a reasonable time; and sending
/// SIGKILL if the server does not exit within a reasonable time after
/// SIGTERM". Dropping it without one kills the server at once, so a host
/// that exits or panics leaves no orphan behind.
///
/// On Unix the signals go to the server's whole process group, which
/// [`connect_child`] gives it: a wrapper (`npx`, `uv run`) starts the real
/// server as its own child, and signalling the wrapper alone leaves that one
/// running.
///
/// [`connect_child`]: crate::connect_child
#[derive(Debug)]
pub struct ServerProcess {
    child: Child,
    grace: Duration,
}

impl ServerProcess {
    /// How long [`shutdown`](Self::shutdown) waits at each step by default.
    pub const DEFAULT_GRACE: Duration = Duration::from_secs(5);

    /// Take over `child`. Give it its own process group when spawning
    /// (`Command::process_group(0)` on Unix) for signals to reach whatever it
    /// starts.
    #[must_use]
    pub fn new(child: Child) -> Self {
        Self {
            child,
            grace: Self::DEFAULT_GRACE,
        }
    }

    /// Wait `grace` for the server to exit after its input closes, and again
    /// after `SIGTERM`, before escalating.
    #[must_use]
    pub fn grace(mut self, grace: Duration) -> Self {
        self.grace = grace;
        self
    }

    /// Log what the server writes to stderr (when it was piped) through
    /// `tracing`, line by line under `label`: "The server MAY write UTF-8
    /// strings to its standard error (stderr) for any logging purposes ...
    /// Clients MAY capture, forward, or ignore this logging". Lines are
    /// logged at `info` on target `turbomcp::server_stderr`, truncated to
    /// 4 KiB and with control characters escaped, since the server's output
    /// is not this process's to trust.
    #[must_use]
    pub fn log_stderr(mut self, label: impl Into<String>) -> Self {
        if let Some(stderr) = self.child.stderr.take() {
            let label = label.into();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::info!(
                        target: "turbomcp::server_stderr",
                        server = %label,
                        "{}",
                        sanitize_line(&line)
                    );
                }
            });
        }
        self
    }

    /// The server's process id, while it runs.
    #[must_use]
    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }

    /// Shut the server down: wait for it to exit (its input is closed when
    /// the [`Client`](crate::Client) closes or drops), then `SIGTERM`, then
    /// `SIGKILL`, each after the [`grace`](Self::grace). Close the client
    /// first; this waits for the exit either way.
    pub async fn shutdown(mut self) {
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

    /// Signal the process group on Unix (see the type docs); elsewhere there
    /// is only killing the process.
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
            if rustix::process::kill_process_group(pid, signal).is_ok() {
                return;
            }
        }
        let _ = signal;
        let _ = self.child.start_kill();
    }
}

impl Drop for ServerProcess {
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

/// The most of one stderr line logged.
const MAX_STDERR_LINE: usize = 4096;

/// `line` safe to log: control characters escaped (a terminal escape in a
/// server's output must not reach the operator's terminal as one), and cut
/// at [`MAX_STDERR_LINE`] bytes.
fn sanitize_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len().min(MAX_STDERR_LINE));
    for c in line.chars() {
        if out.len() >= MAX_STDERR_LINE {
            out.push('…');
            break;
        }
        if c.is_control() {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stderr_lines_are_escaped_and_bounded() {
        assert_eq!(sanitize_line("ok \u{1b}[31mred"), "ok \\u{1b}[31mred");
        let long = "x".repeat(10_000);
        assert!(sanitize_line(&long).len() <= MAX_STDERR_LINE + '…'.len_utf8());
    }

    /// A server that ignores `SIGTERM` and starts a grandchild (as `npx` does)
    /// is still gone after shutdown, grandchild included.
    #[cfg(unix)]
    #[tokio::test]
    async fn shutdown_escalates_and_reaches_the_whole_group() {
        use std::process::Stdio;
        let dir = std::env::temp_dir().join(format!("turbomcp-client-{}", std::process::id()));
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
        ServerProcess::new(child)
            .grace(Duration::from_millis(100))
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
