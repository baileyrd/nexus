//! BL-140 Phase 2b — remote-forge runtime factory + SSH transport.
//!
//! Two factories:
//!
//! - [`build_remote_runtime_over_pipes`] — takes a paired reader /
//!   writer + transport guard. Used by tests and by callers that want
//!   to plug in a non-SSH transport (e.g. WebSocket in Phase 3+).
//! - [`build_remote_runtime_ssh`] — spawns `ssh user@host -- nexus
//!   serve --forge-path /path --stdio` and wires its stdio into a
//!   [`RemoteClient`]. The production path for `--forge-path ssh://`.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use nexus_remote::{ForgeUri, RemoteClient, SshForgeUri};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::process::{Child, Command};

use crate::invoker::{IpcInvoker, IpcInvokerError};

/// Hard ceiling on the spawn-and-handshake handshake. SSH's own
/// connection setup adds latency; this bound is generous to tolerate
/// slow networks while still failing fast on a hung child.
pub const SSH_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Errors raised by the remote-runtime factories.
#[derive(Debug, thiserror::Error)]
pub enum RemoteRuntimeError {
    /// The forge URI was not an `ssh://` URI.
    #[error("expected ssh:// URI, got: {0}")]
    NotSshUri(String),
    /// Failed to spawn the `ssh` binary. Most commonly: `ssh` isn't on
    /// `$PATH`. The OS error is included verbatim.
    #[error("failed to spawn ssh: {0}")]
    Spawn(#[source] std::io::Error),
    /// The spawned `ssh` child closed its stdio handles before we
    /// could capture them. Indicates a fundamentally broken spawn —
    /// e.g. the OS denied piped stdio.
    #[error("ssh child stdio handles missing after spawn")]
    MissingStdio,
}

/// A handle that keeps the underlying transport alive for the lifetime
/// of a [`RemoteRuntime`]. Drop tears it down (for SSH: kills the
/// child process).
///
/// Trait object so non-SSH transports (test duplexes, future
/// WebSocket) can plug in without changing `RemoteRuntime`.
pub trait TransportGuard: Send + Sync {}

/// SSH-backed transport guard owning the spawned child process.
///
/// Drop kills the child. The `Child` is wrapped in an `Option` so the
/// `Drop` impl can `take()` and call `start_kill` synchronously even
/// though we're outside a tokio runtime context.
pub struct SshTransportGuard {
    child: Option<Child>,
}

impl TransportGuard for SshTransportGuard {}

impl Drop for SshTransportGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Best-effort kill. If the child already exited we just
            // get an error here that we don't surface.
            let _ = child.start_kill();
        }
    }
}

/// No-op transport guard for tests / duplex pairs where the caller
/// owns the transport lifetime out of band.
pub struct NoopTransportGuard;
impl TransportGuard for NoopTransportGuard {}

/// Assembled remote runtime — the remote-forge counterpart to
/// [`crate::Runtime`].
///
/// Holds an [`Arc<RemoteClient>`] for the actual IPC work and a
/// [`TransportGuard`] that keeps the transport alive. Drop the
/// runtime to tear down the connection.
pub struct RemoteRuntime {
    /// JSON-RPC client over the transport. Cheap to clone the
    /// inner `Arc` if the caller needs multiple invokers.
    pub client: Arc<RemoteClient>,
    transport: Box<dyn TransportGuard>,
}

impl RemoteRuntime {
    /// Return an [`IpcInvoker`] trait object backed by this runtime's
    /// remote client. Mirrors [`crate::Runtime::invoker`].
    #[must_use]
    pub fn invoker(&self) -> Arc<dyn IpcInvoker + Send + Sync> {
        Arc::new(RemoteIpcInvoker {
            client: Arc::clone(&self.client),
        })
    }

    /// Stop the inbound router task + drop the transport guard. After
    /// this returns, every subsequent invoker call fails fast.
    pub async fn shutdown(self) {
        self.client.shutdown().await;
        // self.transport drops here, killing the SSH child if there
        // is one.
        drop(self.transport);
    }
}

/// `IpcInvoker` implementation over a JSON-RPC client. Lives here
/// rather than in `nexus-remote` so the crate stays kernel-agnostic;
/// this is the bridge between the wire protocol and the kernel's
/// trait surface.
struct RemoteIpcInvoker {
    client: Arc<RemoteClient>,
}

#[async_trait::async_trait]
impl IpcInvoker for RemoteIpcInvoker {
    async fn ipc_call(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, IpcInvokerError> {
        match self
            .client
            .ipc_call(target_plugin_id, command_id, args, Some(timeout))
            .await
        {
            Ok(v) => Ok(v),
            Err(nexus_remote::RemoteClientError::Server { code, message }) => {
                Err(IpcInvokerError::Remote { code, message })
            }
            Err(nexus_remote::RemoteClientError::Timeout(_)) => Err(IpcInvokerError::Timeout {
                plugin_id: target_plugin_id.to_string(),
                command: command_id.to_string(),
                timeout_ms: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
            }),
            Err(other) => Err(IpcInvokerError::Transport(other.to_string())),
        }
    }
}

/// Build a remote runtime over a caller-supplied paired
/// reader / writer + transport guard.
///
/// Used by tests (`tokio::io::duplex`) and by callers that want to
/// plug a non-SSH transport. The transport guard is dropped together
/// with the runtime.
pub fn build_remote_runtime_over_pipes<R>(
    reader: R,
    writer: Box<dyn AsyncWrite + Unpin + Send>,
    transport: Box<dyn TransportGuard>,
) -> RemoteRuntime
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let client = Arc::new(RemoteClient::new(reader, writer));
    RemoteRuntime { client, transport }
}

/// Build a remote runtime by spawning `ssh user@host -- nexus serve
/// --forge-path /path --stdio` and wiring its stdio into a
/// [`RemoteClient`].
///
/// The spawned `ssh` process inherits stderr from the parent so
/// authentication prompts / banner / error messages reach the user's
/// terminal directly.
///
/// # Errors
/// - [`RemoteRuntimeError::NotSshUri`] if the URI isn't `ssh://`.
/// - [`RemoteRuntimeError::Spawn`] if `ssh` couldn't be exec'd
///   (usually because the binary isn't on `$PATH`).
/// - [`RemoteRuntimeError::MissingStdio`] if the OS refused to give
///   us piped stdio.
pub fn build_remote_runtime_ssh(uri: &ForgeUri) -> Result<RemoteRuntime> {
    let ForgeUri::Ssh(ssh) = uri;
    let mut cmd = build_ssh_command(ssh);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped());
    // Stderr inherits so SSH errors / banner / password prompts reach
    // the user.

    let mut child = cmd
        .spawn()
        .map_err(RemoteRuntimeError::Spawn)
        .context("spawning ssh subprocess")?;

    let stdin = child
        .stdin
        .take()
        .ok_or(RemoteRuntimeError::MissingStdio)
        .context("ssh stdin handle")?;
    let stdout = child
        .stdout
        .take()
        .ok_or(RemoteRuntimeError::MissingStdio)
        .context("ssh stdout handle")?;

    let guard = SshTransportGuard { child: Some(child) };
    let writer_boxed: Box<dyn AsyncWrite + Unpin + Send> = Box::new(stdin);
    Ok(build_remote_runtime_over_pipes(
        stdout,
        writer_boxed,
        Box::new(guard),
    ))
}

/// Build the `tokio::process::Command` that spawns `ssh` against the
/// given URI. Exposed for unit tests so we can pin the argv shape
/// without actually spawning a process.
pub(crate) fn build_ssh_command(ssh: &SshForgeUri) -> Command {
    let mut cmd = Command::new("ssh");
    // -T disables pseudo-tty allocation. We're piping binary JSON
    // frames; TTY layer would mangle them.
    cmd.arg("-T");
    if let Some(port) = ssh.port {
        cmd.arg("-p").arg(port.to_string());
    }
    // Authority — `user@host` or just `host`.
    let authority = match &ssh.user {
        Some(u) => format!("{u}@{}", ssh.host),
        None => ssh.host.clone(),
    };
    cmd.arg(authority);
    // `--` separator so any `-` characters in the remote path can't
    // be reinterpreted as ssh options.
    cmd.arg("--");
    // The command to run on the remote: `nexus serve --forge-path
    // /path --stdio`.
    cmd.arg("nexus")
        .arg("serve")
        .arg("--forge-path")
        .arg(&ssh.path)
        .arg("--stdio");
    cmd.kill_on_drop(true);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_remote::SshForgeUri;

    fn ssh(user: Option<&str>, host: &str, port: Option<u16>, path: &str) -> SshForgeUri {
        SshForgeUri {
            user: user.map(str::to_string),
            host: host.to_string(),
            port,
            path: path.to_string(),
        }
    }

    fn argv(cmd: &Command) -> Vec<String> {
        let std = cmd.as_std();
        std::iter::once(std.get_program().to_string_lossy().into_owned())
            .chain(std.get_args().map(|a| a.to_string_lossy().into_owned()))
            .collect()
    }

    #[test]
    fn ssh_command_with_user_host_port_path() {
        let c = build_ssh_command(&ssh(Some("alice"), "host", Some(2222), "/srv/forge"));
        assert_eq!(
            argv(&c),
            vec![
                "ssh",
                "-T",
                "-p",
                "2222",
                "alice@host",
                "--",
                "nexus",
                "serve",
                "--forge-path",
                "/srv/forge",
                "--stdio",
            ]
        );
    }

    #[test]
    fn ssh_command_omits_port_when_none() {
        let c = build_ssh_command(&ssh(None, "host", None, "/srv/forge"));
        let a = argv(&c);
        assert!(!a.contains(&"-p".to_string()), "argv: {a:?}");
        // argv: ["ssh", "-T", "host", "--", "nexus", ...]
        assert_eq!(a[2], "host");
    }

    #[test]
    fn ssh_command_omits_user_at_when_none() {
        let c = build_ssh_command(&ssh(None, "host.example.com", Some(22), "/srv/forge"));
        let a = argv(&c);
        let auth = a
            .iter()
            .find(|s| s.contains("host.example.com"))
            .expect("authority arg");
        assert_eq!(auth, "host.example.com", "should not have user@");
    }

    #[test]
    fn ssh_command_separator_protects_path_with_leading_dash_chars() {
        let c = build_ssh_command(&ssh(None, "host", None, "/srv/-strange/path"));
        let a = argv(&c);
        let dash_idx = a.iter().position(|s| s == "--").expect("-- separator");
        let path_idx = a
            .iter()
            .position(|s| s == "/srv/-strange/path")
            .expect("path");
        assert!(dash_idx < path_idx, "-- must precede the path: {a:?}");
    }

    #[test]
    fn ssh_command_uses_t_flag_to_disable_tty() {
        let c = build_ssh_command(&ssh(None, "host", None, "/srv/forge"));
        let a = argv(&c);
        assert!(a.contains(&"-T".to_string()), "argv: {a:?}");
    }
}
