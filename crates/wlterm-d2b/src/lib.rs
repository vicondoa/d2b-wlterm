//! d2b public-socket client boundary for wlterm.
//!
//! This crate is the only wlterm layer that owns d2b public daemon I/O. It
//! plans through `wlterm-core`, executes through `d2b-client`, and never talks
//! to the privileged broker or mutates host state directly.

use d2b_client::{
    read_hello_response, send_hello, AttachedShell, ClientError, FrameBounds, PublicSocketClient,
};
use d2b_toolkit_core::{
    Hello, KnownFeatureFlag, ShellKillResult, ShellListResult, ShellName, ShellOp,
    ShellSessionState, SocketClass, TerminalSize, ToolkitError,
};
use futures::executor::block_on;
use futures::io::{AsyncRead, AsyncWrite};
use sha2::{Digest, Sha256};
use std::fmt;
use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use wlterm_core::friendly_name::FriendlyName;
use wlterm_core::{
    DisabledReason, PlannedAction, SafeCorrelation, ShellSession, ShellVisualState, VmId,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct D2bClientConfig {
    pub public_socket_path: String,
    pub initial_terminal_size: TerminalSize,
    pub operation_timeout_ms: u64,
}

impl Default for D2bClientConfig {
    fn default() -> Self {
        Self {
            public_socket_path: default_public_socket_path(),
            initial_terminal_size: TerminalSize { rows: 24, cols: 80 },
            operation_timeout_ms: 5_000,
        }
    }
}

fn default_public_socket_path() -> String {
    std::env::var("D2B_PUBLIC_SOCKET").unwrap_or_else(|_| "/run/d2b/public.sock".to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct D2bActionBoundary {
    config: D2bClientConfig,
}

impl D2bActionBoundary {
    pub fn new(config: D2bClientConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &D2bClientConfig {
        &self.config
    }

    pub fn plan_to_shell_op(&self, action: &PlannedAction) -> Option<ShellOp> {
        match action {
            PlannedAction::ListSessions { vm } => {
                Some(ShellOp::List(d2b_toolkit_core::ShellListArgs {
                    vm: vm.as_str().to_string(),
                }))
            }
            PlannedAction::AttachShell { vm, name, force } => {
                Some(ShellOp::Attach(d2b_toolkit_core::ShellAttachArgs {
                    vm: vm.as_str().to_string(),
                    name: name.as_ref().map(to_toolkit_shell_name),
                    force: *force,
                    initial_terminal_size: self.config.initial_terminal_size,
                }))
            }
            PlannedAction::KillShell { vm, name } => {
                Some(ShellOp::Kill(d2b_toolkit_core::ShellKillArgs {
                    vm: vm.as_str().to_string(),
                    name: to_toolkit_shell_name(name),
                }))
            }
            PlannedAction::RefreshVms
            | PlannedAction::FocusExistingShell { .. }
            | PlannedAction::PromptAlreadyAttached { .. }
            | PlannedAction::PromptStop { .. }
            | PlannedAction::Disabled { .. } => None,
        }
    }

    pub fn connect(&self) -> Result<PublicSocketClient<BlockingUnixTransport>, D2bClientError> {
        let trace = ActionTrace::for_label("connect-public-socket");
        d2b_client::ensure_allowed_socket(classify_socket_path(&self.config.public_socket_path))
            .map_err(|err| D2bClientError::from_client_error("connect", trace.clone(), err))?;
        let timeout = Duration::from_millis(self.config.operation_timeout_ms);
        let mut transport =
            BlockingUnixTransport::connect(&self.config.public_socket_path, timeout).map_err(
                |source| {
                    D2bClientError::from_client_error(
                        "connect",
                        trace.clone(),
                        ToolkitError::Io {
                            context: "connecting public socket",
                            source,
                        }
                        .into(),
                    )
                },
            )?;
        let bounds = FrameBounds::default();
        block_on(async {
            send_hello(
                &mut transport,
                &Hello::toolkit_client(vec![KnownFeatureFlag::TypedErrors.wire_value()]),
                bounds,
            )
            .await?;
            read_hello_response(&mut transport, bounds).await?;
            Ok::<(), ClientError>(())
        })
        .map_err(|err| D2bClientError::from_client_error("connect", trace, err))?;
        Ok(PublicSocketClient::with_bounds(transport, bounds))
    }

    pub fn execute_blocking(
        &self,
        action: PlannedAction,
    ) -> Result<D2bActionOutcome<BlockingUnixTransport>, D2bClientError> {
        let client = self.connect()?;
        block_on(self.execute_with_client(client, action))
    }

    pub async fn execute_with_client<T>(
        &self,
        client: PublicSocketClient<T>,
        action: PlannedAction,
    ) -> Result<D2bActionOutcome<T>, D2bClientError>
    where
        T: AsyncRead + AsyncWrite + Unpin,
    {
        let trace = ActionTrace::for_label(action.metrics_label_value());
        match action {
            PlannedAction::RefreshVms => Ok(D2bActionOutcome::RefreshQueued { client }),
            PlannedAction::Disabled { reason } => Ok(D2bActionOutcome::Disabled { client, reason }),
            PlannedAction::FocusExistingShell { vm, name } => {
                Ok(D2bActionOutcome::FocusExisting { client, vm, name })
            }
            PlannedAction::PromptAlreadyAttached { vm, name } => {
                Ok(D2bActionOutcome::PromptAlreadyAttached { client, vm, name })
            }
            PlannedAction::PromptStop { vm, name } => Ok(D2bActionOutcome::PromptStop {
                client,
                vm,
                name,
                requires_confirmation: true,
            }),
            PlannedAction::ListSessions { vm } => {
                let mut client = client;
                let list = client
                    .shell_list(vm.as_str().to_string())
                    .await
                    .map_err(|err| D2bClientError::from_client_error("list", trace.clone(), err))?;
                let sessions = shell_list_to_sessions(&list)
                    .map_err(|kind| D2bClientError::protocol("list", trace.clone(), kind))?;
                Ok(D2bActionOutcome::Listed {
                    client,
                    vm,
                    sessions,
                })
            }
            PlannedAction::AttachShell { vm, name, force } => {
                let attached = client
                    .attach_shell(
                        vm.as_str().to_string(),
                        name.as_ref().map(to_toolkit_shell_name),
                        force,
                        self.config.initial_terminal_size,
                    )
                    .await
                    .map_err(|err| D2bClientError::from_client_error("open", trace.clone(), err))?;
                let resolved_name = from_toolkit_shell_name(attached.resolved_name())
                    .map_err(|kind| D2bClientError::protocol("open", trace.clone(), kind))?;
                Ok(D2bActionOutcome::Attached {
                    attached,
                    vm,
                    resolved_name,
                    force,
                    trace,
                })
            }
            PlannedAction::KillShell { vm, name } => {
                let mut client = client;
                let result = client
                    .shell_kill(vm.as_str().to_string(), to_toolkit_shell_name(&name))
                    .await
                    .map_err(|err| D2bClientError::from_client_error("stop", trace.clone(), err))?;
                Ok(D2bActionOutcome::Killed {
                    client,
                    vm,
                    name,
                    result,
                    trace,
                })
            }
        }
    }
}

fn connect_seqpacket(path: &str) -> io::Result<OwnedFd> {
    use nix::sys::socket::{connect, socket, AddressFamily, SockFlag, SockType, UnixAddr};

    let fd = socket(
        AddressFamily::Unix,
        SockType::SeqPacket,
        SockFlag::SOCK_CLOEXEC,
        None,
    )
    .map_err(errno_to_io)?;
    let addr = UnixAddr::new(Path::new(path)).map_err(errno_to_io)?;
    connect(fd.as_raw_fd(), &addr).map_err(errno_to_io)?;
    Ok(fd)
}

fn errno_to_io(error: nix::errno::Errno) -> io::Error {
    io::Error::from_raw_os_error(error as i32)
}

const MAX_PUBLIC_PACKET: usize = 1024 * 1024 + 4;

pub struct BlockingUnixTransport {
    fd: OwnedFd,
    read_buf: Vec<u8>,
    read_pos: usize,
    write_buf: Vec<u8>,
}

impl BlockingUnixTransport {
    fn connect(path: &str, _timeout: Duration) -> io::Result<Self> {
        Ok(Self {
            fd: connect_seqpacket(path)?,
            read_buf: Vec::new(),
            read_pos: 0,
            write_buf: Vec::new(),
        })
    }
}

impl AsyncRead for BlockingUnixTransport {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if self.read_pos >= self.read_buf.len() {
            let mut packet = vec![0_u8; MAX_PUBLIC_PACKET];
            let len = nix::sys::socket::recv(
                self.fd.as_raw_fd(),
                &mut packet,
                nix::sys::socket::MsgFlags::empty(),
            )
            .map_err(errno_to_io)?;
            packet.truncate(len);
            self.read_buf = packet;
            self.read_pos = 0;
            if self.read_buf.is_empty() {
                return Poll::Ready(Ok(0));
            }
        }
        let available = &self.read_buf[self.read_pos..];
        let len = available.len().min(buf.len());
        buf[..len].copy_from_slice(&available[..len]);
        self.read_pos += len;
        Poll::Ready(Ok(len))
    }
}

impl AsyncWrite for BlockingUnixTransport {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.write_buf.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.write_buf.is_empty() {
            let sent = nix::sys::socket::send(
                self.fd.as_raw_fd(),
                &self.write_buf,
                nix::sys::socket::MsgFlags::empty(),
            )
            .map_err(errno_to_io)?;
            if sent != self.write_buf.len() {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "short write on public seqpacket socket",
                )));
            }
            self.write_buf.clear();
        }
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_flush(cx)
    }
}

pub fn to_toolkit_shell_name(name: &FriendlyName) -> ShellName {
    ShellName::new(name.as_str().to_string())
}

pub fn kill_shell_op(vm: &VmId, name: &FriendlyName) -> ShellOp {
    ShellOp::Kill(d2b_toolkit_core::ShellKillArgs {
        vm: vm.as_str().to_string(),
        name: to_toolkit_shell_name(name),
    })
}

pub fn classify_socket_path(path: &str) -> SocketClass {
    if path.contains("priv-broker") || path.contains("broker.sock") {
        SocketClass::PrivilegedBroker
    } else {
        SocketClass::PublicDaemon
    }
}

fn shell_list_to_sessions(result: &ShellListResult) -> Result<Vec<ShellSession>, &'static str> {
    let default_name = from_toolkit_shell_name(&result.default_name)?;
    result
        .sessions
        .iter()
        .map(|entry| {
            let name = from_toolkit_shell_name(&entry.name)?;
            let visual_state = match (entry.attached, entry.state) {
                (true, _) | (_, ShellSessionState::Attached) => ShellVisualState::Attached,
                (_, ShellSessionState::Detached) => ShellVisualState::Detached,
                _ => ShellVisualState::Unavailable,
            };
            Ok(ShellSession {
                is_default: entry.is_default || name.as_str() == default_name.as_str(),
                name,
                visual_state,
            })
        })
        .collect()
}

fn from_toolkit_shell_name(name: &ShellName) -> Result<FriendlyName, &'static str> {
    FriendlyName::from_candidate(name.as_str()).map_err(|_| "invalid-shell-name")
}

#[derive(Clone, PartialEq, Eq)]
pub struct ActionTrace(String);

impl ActionTrace {
    pub fn for_label(label: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"d2b-wlterm-action");
        hasher.update((label.len() as u64).to_le_bytes());
        hasher.update(label.as_bytes());
        Self(format!("wlterm-{:x}", hasher.finalize()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn safe_correlation(&self) -> SafeCorrelation {
        SafeCorrelation::new(self.0.clone()).expect("trace format is safe")
    }

    pub const fn metrics_label_value(&self) -> &'static str {
        "trace"
    }
}

impl fmt::Debug for ActionTrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ActionTrace").field(&self.0).finish()
    }
}

impl fmt::Display for ActionTrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum D2bClientErrorKind {
    DaemonDown,
    AuthDenied,
    StaleSession,
    Timeout,
    Typed(String),
    Protocol(&'static str),
}

impl D2bClientErrorKind {
    pub fn metrics_label_value(&self) -> &str {
        match self {
            Self::DaemonDown => "daemon-down",
            Self::AuthDenied => "auth-denied",
            Self::StaleSession => "stale-session",
            Self::Timeout => "timeout",
            Self::Typed(kind) => kind.as_str(),
            Self::Protocol(kind) => kind,
        }
    }
}

#[derive(Debug)]
pub struct D2bClientError {
    action: &'static str,
    kind: D2bClientErrorKind,
    trace: ActionTrace,
    source: Option<ClientError>,
}

impl D2bClientError {
    fn from_client_error(action: &'static str, trace: ActionTrace, source: ClientError) -> Self {
        let kind = classify_client_error(&source);
        Self {
            action,
            kind,
            trace,
            source: Some(source),
        }
    }

    fn protocol(action: &'static str, trace: ActionTrace, kind: &'static str) -> Self {
        Self {
            action,
            kind: D2bClientErrorKind::Protocol(kind),
            trace,
            source: None,
        }
    }

    pub fn action(&self) -> &'static str {
        self.action
    }

    pub fn kind(&self) -> &D2bClientErrorKind {
        &self.kind
    }

    pub fn trace(&self) -> &ActionTrace {
        &self.trace
    }

    pub fn safe_correlation(&self) -> SafeCorrelation {
        self.trace.safe_correlation()
    }

    pub fn metrics_label_value(&self) -> &str {
        self.kind.metrics_label_value()
    }
}

impl fmt::Display for D2bClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "d2b {action} failed: {kind} (trace {trace})",
            action = self.action,
            kind = self.kind.metrics_label_value(),
            trace = self.trace,
        )
    }
}

impl std::error::Error for D2bClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

fn classify_client_error(error: &ClientError) -> D2bClientErrorKind {
    match error {
        ClientError::Daemon { kind } => classify_typed_error(kind),
        ClientError::Core(ToolkitError::PrivilegedBrokerRefused) => D2bClientErrorKind::AuthDenied,
        ClientError::Core(ToolkitError::Io { source, .. }) => match source.kind() {
            std::io::ErrorKind::PermissionDenied => D2bClientErrorKind::AuthDenied,
            std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::NotFound
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::UnexpectedEof => D2bClientErrorKind::DaemonDown,
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
                D2bClientErrorKind::Timeout
            }
            _ => D2bClientErrorKind::Protocol("io"),
        },
        ClientError::Core(ToolkitError::Protocol { kind }) => D2bClientErrorKind::Protocol(kind),
        ClientError::Core(ToolkitError::FrameTooLarge { .. }) => {
            D2bClientErrorKind::Protocol("frame-too-large")
        }
        ClientError::Codec { .. } => D2bClientErrorKind::Protocol("codec"),
        ClientError::Hello { .. } => D2bClientErrorKind::Protocol("hello"),
        ClientError::UnexpectedResponse { .. } => {
            D2bClientErrorKind::Protocol("unexpected-response")
        }
        ClientError::CorrelationMismatch => D2bClientErrorKind::Protocol("correlation-mismatch"),
    }
}

fn classify_typed_error(kind: &str) -> D2bClientErrorKind {
    if kind.contains("auth") || kind.contains("permission") || kind.contains("denied") {
        D2bClientErrorKind::AuthDenied
    } else if kind.contains("stale-session") {
        D2bClientErrorKind::StaleSession
    } else if kind.contains("timeout") || kind.contains("timed-out") {
        D2bClientErrorKind::Timeout
    } else {
        D2bClientErrorKind::Typed(kind.to_string())
    }
}

pub enum D2bActionOutcome<T> {
    RefreshQueued {
        client: PublicSocketClient<T>,
    },
    Disabled {
        client: PublicSocketClient<T>,
        reason: DisabledReason,
    },
    FocusExisting {
        client: PublicSocketClient<T>,
        vm: VmId,
        name: FriendlyName,
    },
    PromptAlreadyAttached {
        client: PublicSocketClient<T>,
        vm: VmId,
        name: FriendlyName,
    },
    PromptStop {
        client: PublicSocketClient<T>,
        vm: VmId,
        name: FriendlyName,
        requires_confirmation: bool,
    },
    Listed {
        client: PublicSocketClient<T>,
        vm: VmId,
        sessions: Vec<ShellSession>,
    },
    Attached {
        attached: AttachedShell<T>,
        vm: VmId,
        resolved_name: FriendlyName,
        force: bool,
        trace: ActionTrace,
    },
    Killed {
        client: PublicSocketClient<T>,
        vm: VmId,
        name: FriendlyName,
        result: ShellKillResult,
        trace: ActionTrace,
    },
}

impl<T> fmt::Debug for D2bActionOutcome<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RefreshQueued { .. } => f.write_str("RefreshQueued"),
            Self::Disabled { reason, .. } => {
                f.debug_struct("Disabled").field("reason", reason).finish()
            }
            Self::FocusExisting { vm, .. } => f
                .debug_struct("FocusExisting")
                .field("vm", vm)
                .field("name", &"<redacted>")
                .finish(),
            Self::PromptAlreadyAttached { vm, .. } => f
                .debug_struct("PromptAlreadyAttached")
                .field("vm", vm)
                .field("name", &"<redacted>")
                .finish(),
            Self::PromptStop {
                vm,
                requires_confirmation,
                ..
            } => f
                .debug_struct("PromptStop")
                .field("vm", vm)
                .field("name", &"<redacted>")
                .field("requires_confirmation", requires_confirmation)
                .finish(),
            Self::Listed { vm, sessions, .. } => f
                .debug_struct("Listed")
                .field("vm", vm)
                .field("sessions_len", &sessions.len())
                .finish(),
            Self::Attached {
                vm, force, trace, ..
            } => f
                .debug_struct("Attached")
                .field("vm", vm)
                .field("resolved_name", &"<redacted>")
                .field("force", force)
                .field("trace", trace)
                .finish(),
            Self::Killed {
                vm, result, trace, ..
            } => f
                .debug_struct("Killed")
                .field("vm", vm)
                .field("name", &"<redacted>")
                .field("killed", &result.killed)
                .field("state", &result.state)
                .field("trace", trace)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use d2b_toolkit_core::{
        ErrorEnvelope, OpaqueHandle, PublicResponse, ShellAttachResult, ShellListEntry,
        ShellOpResponse,
    };
    use futures::executor::block_on;
    use futures::io::{AsyncRead, AsyncWrite};
    use serde::Serialize;
    use serde_json::Value;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    #[derive(Default)]
    struct FakePublicSocket {
        reads: Vec<u8>,
        read_pos: usize,
        writes: Vec<u8>,
    }

    impl FakePublicSocket {
        fn with_responses(responses: Vec<PublicResponse>) -> Self {
            let mut reads = Vec::new();
            for response in responses {
                reads.extend(frame(&response));
            }
            Self {
                reads,
                read_pos: 0,
                writes: Vec::new(),
            }
        }

        fn written_json_frames(&self) -> Vec<Value> {
            let mut frames = Vec::new();
            let mut pos = 0;
            while pos < self.writes.len() {
                let len =
                    u32::from_le_bytes(self.writes[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                frames.push(serde_json::from_slice(&self.writes[pos..pos + len]).unwrap());
                pos += len;
            }
            frames
        }
    }

    impl AsyncRead for FakePublicSocket {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<std::io::Result<usize>> {
            if self.read_pos >= self.reads.len() {
                return Poll::Ready(Ok(0));
            }
            let available = self.reads.len() - self.read_pos;
            let n = available.min(buf.len());
            let start = self.read_pos;
            let end = start + n;
            buf[..n].copy_from_slice(&self.reads[start..end]);
            self.read_pos = end;
            Poll::Ready(Ok(n))
        }
    }

    impl AsyncWrite for FakePublicSocket {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            self.writes.extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    fn frame<T: Serialize>(value: &T) -> Vec<u8> {
        let payload = serde_json::to_vec(value).unwrap();
        let mut frame = Vec::with_capacity(payload.len() + 4);
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        frame.extend_from_slice(&payload);
        frame
    }

    fn shell(name: &str) -> FriendlyName {
        FriendlyName::from_candidate(name).unwrap()
    }

    fn vm(name: &str) -> VmId {
        VmId::new(name).unwrap()
    }

    fn boundary() -> D2bActionBoundary {
        D2bActionBoundary::new(D2bClientConfig::default())
    }

    fn list_response(op_id: u64) -> PublicResponse {
        PublicResponse::Shell {
            op_id: Some(op_id),
            response: ShellOpResponse::List(ShellListResult {
                default_name: ShellName::new("quiet-otter"),
                sessions: vec![ShellListEntry {
                    name: ShellName::new("quiet-otter"),
                    state: ShellSessionState::Detached,
                    attached: false,
                    is_default: true,
                }],
            }),
        }
    }

    fn attach_response(op_id: u64, name: &str) -> PublicResponse {
        PublicResponse::Shell {
            op_id: Some(op_id),
            response: ShellOpResponse::Attach(ShellAttachResult {
                session: OpaqueHandle::new("opaque-session-handle"),
                resolved_name: ShellName::new(name),
                state: ShellSessionState::Attached,
                force_evicted: false,
            }),
        }
    }

    fn kill_response(op_id: u64, name: &str) -> PublicResponse {
        PublicResponse::Shell {
            op_id: Some(op_id),
            response: ShellOpResponse::Kill(ShellKillResult {
                name: ShellName::new(name),
                killed: true,
                state: ShellSessionState::Killed,
            }),
        }
    }

    fn shell_error(op_id: u64, kind: &str) -> PublicResponse {
        PublicResponse::Error {
            op_id: Some(op_id),
            error: ErrorEnvelope {
                kind: kind.into(),
                exit_code: 69,
                message: "contains quiet-otter and opaque-session-handle".into(),
                remediation: "retry without leaking terminal bytes".into(),
            },
        }
    }

    #[test]
    fn list_executes_public_socket_shell_list() {
        block_on(async {
            let client =
                PublicSocketClient::new(FakePublicSocket::with_responses(vec![list_response(1)]));
            let outcome = boundary()
                .execute_with_client(client, PlannedAction::ListSessions { vm: vm("work") })
                .await
                .unwrap();
            let D2bActionOutcome::Listed {
                client, sessions, ..
            } = outcome
            else {
                panic!("expected list outcome");
            };
            assert_eq!(sessions.len(), 1);
            assert!(sessions[0].is_default);
            assert_eq!(sessions[0].visual_state, ShellVisualState::Detached);
            let frames = client.into_inner().written_json_frames();
            assert_eq!(frames[0]["kind"], "shell");
            assert_eq!(frames[0]["payload"]["op"], "list");
            assert_eq!(frames[0]["payload"]["args"]["vm"], "work");
        });
    }

    #[test]
    fn open_executes_public_socket_attach() {
        block_on(async {
            let client =
                PublicSocketClient::new(FakePublicSocket::with_responses(vec![attach_response(
                    1,
                    "quiet-otter",
                )]));
            let outcome = boundary()
                .execute_with_client(
                    client,
                    PlannedAction::AttachShell {
                        vm: vm("work"),
                        name: Some(shell("quiet-otter")),
                        force: true,
                    },
                )
                .await
                .unwrap();
            let D2bActionOutcome::Attached {
                attached,
                resolved_name,
                force,
                ..
            } = outcome
            else {
                panic!("expected attached outcome");
            };
            assert_eq!(resolved_name.as_str(), "quiet-otter");
            assert!(force);
            let client = attached.into_inner();
            let frames = client.into_inner().written_json_frames();
            assert_eq!(frames[0]["payload"]["op"], "attach");
            assert_eq!(frames[0]["payload"]["args"]["force"], true);
            assert_eq!(frames[0]["payload"]["args"]["name"], "quiet-otter");
        });
    }

    #[test]
    fn create_executes_attach_without_force() {
        block_on(async {
            let client =
                PublicSocketClient::new(FakePublicSocket::with_responses(vec![attach_response(
                    1,
                    "fresh-panda",
                )]));
            let outcome = boundary()
                .execute_with_client(
                    client,
                    PlannedAction::AttachShell {
                        vm: vm("work"),
                        name: Some(shell("fresh-panda")),
                        force: false,
                    },
                )
                .await
                .unwrap();
            let D2bActionOutcome::Attached { attached, .. } = outcome else {
                panic!("expected attached outcome");
            };
            let frames = attached.into_inner().into_inner().written_json_frames();
            assert_eq!(frames[0]["payload"]["op"], "attach");
            assert_eq!(frames[0]["payload"]["args"]["force"], false);
        });
    }

    #[test]
    fn stop_requires_prompt_before_shell_kill() {
        block_on(async {
            let client = PublicSocketClient::new(FakePublicSocket::default());
            let outcome = boundary()
                .execute_with_client(
                    client,
                    PlannedAction::PromptStop {
                        vm: vm("work"),
                        name: shell("quiet-otter"),
                    },
                )
                .await
                .unwrap();
            let D2bActionOutcome::PromptStop {
                client,
                requires_confirmation,
                ..
            } = outcome
            else {
                panic!("expected prompt");
            };
            assert!(requires_confirmation);
            assert!(client.into_inner().written_json_frames().is_empty());

            let client =
                PublicSocketClient::new(FakePublicSocket::with_responses(vec![kill_response(
                    1,
                    "quiet-otter",
                )]));
            let outcome = boundary()
                .execute_with_client(
                    client,
                    PlannedAction::KillShell {
                        vm: vm("work"),
                        name: shell("quiet-otter"),
                    },
                )
                .await
                .unwrap();
            let D2bActionOutcome::Killed { client, result, .. } = outcome else {
                panic!("expected kill");
            };
            assert!(result.killed);
            let frames = client.into_inner().written_json_frames();
            assert_eq!(frames[0]["payload"]["op"], "kill");
        });
    }

    #[test]
    fn closing_terminal_view_is_disconnect_only() {
        block_on(async {
            let responses = vec![
                attach_response(1, "quiet-otter"),
                PublicResponse::Shell {
                    op_id: Some(2),
                    response: ShellOpResponse::CloseAttach(d2b_toolkit_core::ShellDetachResult {
                        resolved_name: ShellName::new("quiet-otter"),
                        detached: true,
                        cause: None,
                    }),
                },
            ];
            let client = PublicSocketClient::new(FakePublicSocket::with_responses(responses));
            let outcome = boundary()
                .execute_with_client(
                    client,
                    PlannedAction::AttachShell {
                        vm: vm("work"),
                        name: Some(shell("quiet-otter")),
                        force: false,
                    },
                )
                .await
                .unwrap();
            let D2bActionOutcome::Attached { attached, .. } = outcome else {
                panic!("expected attached");
            };
            let (client, detach) = attached.close_attach().await.unwrap();
            assert!(detach.detached);
            let frames = client.into_inner().written_json_frames();
            assert_eq!(frames[0]["payload"]["op"], "attach");
            assert_eq!(frames[1]["payload"]["op"], "closeAttach");
            assert_ne!(frames[1]["payload"]["op"], "kill");
        });
    }

    #[test]
    fn offline_disabled_action_does_not_contact_daemon() {
        block_on(async {
            let client = PublicSocketClient::new(FakePublicSocket::default());
            let outcome = boundary()
                .execute_with_client(
                    client,
                    PlannedAction::Disabled {
                        reason: DisabledReason::VmOffline,
                    },
                )
                .await
                .unwrap();
            let D2bActionOutcome::Disabled { client, reason } = outcome else {
                panic!("expected disabled");
            };
            assert_eq!(reason, DisabledReason::VmOffline);
            assert!(client.into_inner().written_json_frames().is_empty());
        });
    }

    #[test]
    fn daemon_down_is_sanitized() {
        block_on(async {
            let client = PublicSocketClient::new(FakePublicSocket::default());
            let err = boundary()
                .execute_with_client(client, PlannedAction::ListSessions { vm: vm("work") })
                .await
                .unwrap_err();
            assert_eq!(err.kind(), &D2bClientErrorKind::DaemonDown);
            let rendered = err.to_string();
            assert!(rendered.contains("trace wlterm-"));
            assert!(!rendered.contains("quiet-otter"));
            assert!(!rendered.contains("opaque-session-handle"));
        });
    }

    #[test]
    fn typed_errors_are_classified_without_sensitive_payloads() {
        for (typed, expected) in [
            ("public-socket-auth-denied", D2bClientErrorKind::AuthDenied),
            (
                "guest-control-shell-stale-session",
                D2bClientErrorKind::StaleSession,
            ),
            ("guest-control-shell-timeout", D2bClientErrorKind::Timeout),
            (
                "guest-control-shell-feature-disabled",
                D2bClientErrorKind::Typed("guest-control-shell-feature-disabled".into()),
            ),
        ] {
            block_on(async {
                let client =
                    PublicSocketClient::new(FakePublicSocket::with_responses(vec![shell_error(
                        1, typed,
                    )]));
                let err = boundary()
                    .execute_with_client(client, PlannedAction::ListSessions { vm: vm("work") })
                    .await
                    .unwrap_err();
                assert_eq!(err.kind(), &expected);
                let debug = format!("{err:?}");
                let display = err.to_string();
                for rendered in [debug, display] {
                    assert!(!rendered.contains("quiet-otter"));
                    assert!(!rendered.contains("opaque-session-handle"));
                    assert!(!rendered.contains("terminal bytes"));
                }
                assert_ne!(err.metrics_label_value(), "quiet-otter");
                assert_eq!(err.trace().metrics_label_value(), "trace");
                assert_eq!(err.safe_correlation().metrics_label_value(), "correlation");
            });
        }
    }

    #[test]
    fn broker_socket_paths_are_refused_before_connect() {
        let boundary = D2bActionBoundary::new(D2bClientConfig {
            public_socket_path: "/run/d2b/priv-broker.sock".into(),
            ..D2bClientConfig::default()
        });
        let err = match boundary.connect() {
            Ok(_) => panic!("broker socket should be refused"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), &D2bClientErrorKind::AuthDenied);
        assert!(!err.to_string().contains("priv-broker.sock"));
    }

    #[test]
    fn default_client_uses_public_daemon_socket() {
        assert_eq!(
            D2bClientConfig::default().public_socket_path,
            "/run/d2b/public.sock"
        );
    }

    #[test]
    fn shell_name_does_not_become_metric_label() {
        let op = kill_shell_op(&vm("work"), &shell("customer-project-shell"));

        assert_eq!(op.metrics_label_value(), "kill");
        if let ShellOp::Kill(args) = op {
            assert_eq!(args.name.metrics_label_value(), "shell");
        }
    }
}
