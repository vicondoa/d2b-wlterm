//! Authenticated d2b ComponentSession adapter.

use std::{
    fmt,
    os::fd::{AsFd, AsRawFd, OwnedFd},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use d2b_client_toolkit::contracts::{
    v2_identity::{RealmId, RealmPath},
    v2_services::{
        daemon,
        terminal::{
            self, terminal_selection::Selection, terminal_stream_frame::Frame, ShellAction,
            ShellManagementResult, ShellSelection, TerminalSelection,
        },
    },
};
use d2b_client_toolkit::{
    daemon_call_options, local_daemon_endpoint_identity, CancellationToken, Client, ClientError,
    DaemonClient, DaemonMethod, HandshakeCredentials, HostSocketConnector, RouteRecord, RouteTable,
    ServiceKind, ServiceOwner, TargetInput, TransportKind, TransportSelection,
};
use nix::{
    errno::Errno,
    poll::{poll, PollFd, PollFlags, PollTimeout},
    sys::socket::{
        connect, getsockopt, socket, sockopt, AddressFamily, SockFlag, SockType, UnixAddr,
    },
    unistd::{Gid, Uid, User},
};
use protobuf::EnumOrUnknown;
use wlterm_core::friendly_name::FriendlyName;
use wlterm_core::{
    IsolationPosture, ProviderKind, SessionPersistence, ShellSession, ShellVisualState,
    TargetAvailability, TargetId, TargetPowerState, WorkloadSummary,
};

pub use d2b_client_toolkit::{
    D2B_SOURCE_FINGERPRINT as CLIENT_SOURCE_FINGERPRINT,
    D2B_SOURCE_REVISION as CLIENT_SOURCE_REVISION,
};

const LIVE_ROUTING_UNAVAILABLE: &str =
    "authenticated shell stream routing is unavailable until the desktop route is frozen";
static OPERATION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Bound on the total time spent completing a non-blocking `connect(2)` to the
/// local d2b endpoint, covering both an in-progress handshake (`EINPROGRESS`)
/// and a transiently full listen backlog (`EAGAIN`).
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
/// Backoff between retrying `connect(2)` after `EAGAIN`; the accept backlog is
/// expected to drain within a handful of milliseconds under normal load.
const CONNECT_RETRY_BACKOFF: Duration = Duration::from_millis(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct D2bClientConfig {
    pub public_socket_path: String,
    pub operation_timeout_ms: u64,
}

impl Default for D2bClientConfig {
    fn default() -> Self {
        Self {
            public_socket_path: std::env::var("D2B_PUBLIC_SOCKET")
                .unwrap_or_else(|_| "/run/d2b/public.sock".to_owned()),
            operation_timeout_ms: 5_000,
        }
    }
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

    pub fn inventory_blocking(&self) -> Result<Vec<WorkloadSummary>, D2bClientError> {
        self.run(async |client, cancellation| {
            let response = client
                .list_workloads(None, 256, None, daemon_call_options(false)?, &cancellation)
                .await?;
            let mut summaries = Vec::new();
            for workload in response
                .workloads
                .iter()
                .filter(|workload| supports_shell(workload))
            {
                let mut summary = workload_summary(workload)?;
                if summary.actions_available() {
                    let result = manage_shell_request(
                        &client,
                        summary.target.as_str(),
                        ShellAction::SHELL_ACTION_LIST,
                        "",
                        &cancellation,
                    )
                    .await?;
                    let sessions: Result<Vec<_>, _> =
                        result.sessions.iter().map(shell_session).collect();
                    summary.sessions = sessions?;
                }
                summaries.push(summary);
            }
            Ok(summaries)
        })
    }

    pub fn discover_blocking(&self) -> Result<Vec<WorkloadSummary>, D2bClientError> {
        self.inventory_blocking()
    }

    pub fn list_shells_blocking(&self, target: &str) -> Result<Vec<ShellSession>, D2bClientError> {
        let result = self.manage_shell(target, ShellAction::SHELL_ACTION_LIST, "")?;
        result
            .sessions
            .iter()
            .map(shell_session)
            .collect::<Result<Vec<_>, _>>()
    }

    pub fn detach_shell_blocking(
        &self,
        target: &str,
        shell: &FriendlyName,
    ) -> Result<bool, D2bClientError> {
        self.manage_shell(target, ShellAction::SHELL_ACTION_DETACH, shell.as_str())
            .map(|result| result.applied)
    }

    pub fn kill_shell_blocking(
        &self,
        target: &str,
        shell: &FriendlyName,
    ) -> Result<bool, D2bClientError> {
        self.manage_shell(target, ShellAction::SHELL_ACTION_KILL, shell.as_str())
            .map(|result| result.applied)
    }

    pub fn open_shell_blocking(
        &self,
        _target: &str,
        _shell: Option<&FriendlyName>,
        _force: bool,
    ) -> Result<(), D2bClientError> {
        Err(D2bClientError::LiveRoutingUnavailable)
    }

    fn manage_shell(
        &self,
        target: &str,
        action: ShellAction,
        shell_handle: &str,
    ) -> Result<ShellManagementResult, D2bClientError> {
        let target = target.to_owned();
        let shell_handle = shell_handle.to_owned();
        self.run(async move |client, cancellation| {
            manage_shell_request(&client, &target, action, &shell_handle, &cancellation).await
        })
    }

    fn run<T, F, Fut>(&self, operation: F) -> Result<T, D2bClientError>
    where
        F: FnOnce(DaemonClient, CancellationToken) -> Fut,
        Fut: std::future::Future<Output = Result<T, D2bClientError>>,
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| D2bClientError::Runtime)?;
        runtime.block_on(async {
            let client = connect_daemon(&self.config.public_socket_path).await?;
            let cancellation = CancellationToken::default();
            tokio::time::timeout(
                std::time::Duration::from_millis(self.config.operation_timeout_ms),
                operation(client, cancellation),
            )
            .await
            .map_err(|_| D2bClientError::Timeout)?
        })
    }
}

async fn manage_shell_request(
    client: &DaemonClient,
    target: &str,
    action: ShellAction,
    shell_handle: &str,
    cancellation: &CancellationToken,
) -> Result<ShellManagementResult, D2bClientError> {
    let selection = shell_selection(action, shell_handle, false);
    let terminal = client
        .open_terminal(
            DaemonMethod::Shell,
            target,
            &operation_id(),
            selection,
            daemon_call_options(action != ShellAction::SHELL_ACTION_LIST)?,
            cancellation,
        )
        .await?;
    loop {
        match terminal.receive().await?.frame {
            Some(Frame::ShellResult(result)) => return Ok(result),
            Some(Frame::Outcome(_)) => return Err(D2bClientError::Protocol),
            _ => {}
        }
    }
}

async fn connect_daemon(path: &str) -> Result<DaemonClient, D2bClientError> {
    let fd = connect_seqpacket(path)?;
    let daemon_uid = User::from_name("d2bd")
        .map_err(|_| D2bClientError::Connect)?
        .ok_or(D2bClientError::Connect)?
        .uid
        .as_raw();
    let uid = Uid::effective().as_raw();
    let gid = Gid::effective().as_raw();
    let identity = local_daemon_endpoint_identity(uid, gid)?;
    let connector =
        HostSocketConnector::from_seqpacket_fd(fd, daemon_uid, identity, HandshakeCredentials::Nn)?;
    let realm = RealmId::derive(&RealmPath::root());
    let connected = Client::new(
        RouteTable::new(vec![RouteRecord {
            owner: ServiceOwner::LocalRoot(realm.clone()),
            transport: TransportKind::LocalUnix,
        }]),
        connector,
    )
    .connect(
        TargetInput::LocalRoot(realm),
        ServiceKind::Daemon,
        TransportSelection::exact(TransportKind::LocalUnix),
    )
    .await?;
    DaemonClient::new(connected).map_err(Into::into)
}

fn connect_seqpacket(path: &str) -> Result<OwnedFd, D2bClientError> {
    let fd = socket(
        AddressFamily::Unix,
        SockType::SeqPacket,
        SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK,
        None,
    )
    .map_err(|_| D2bClientError::Connect)?;
    let address = UnixAddr::new(Path::new(path)).map_err(|_| D2bClientError::Connect)?;
    let deadline = Instant::now() + CONNECT_TIMEOUT;

    loop {
        match connect(fd.as_raw_fd(), &address) {
            Ok(()) => return Ok(fd),
            Err(Errno::EINTR) => continue,
            // A non-blocking connect to a local socket may report the
            // handshake as still in progress; wait for the fd to become
            // writable, then read back the real completion status.
            Err(Errno::EINPROGRESS) => {
                wait_connect_writable(&fd, deadline)?;
                return finish_connect(fd);
            }
            // AF_UNIX SOCK_SEQPACKET connect can return EAGAIN when the
            // listener's accept backlog is momentarily full rather than when
            // the peer is unreachable; retry with a bounded backoff instead
            // of failing the whole connection attempt immediately.
            Err(Errno::EAGAIN) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(D2bClientError::Connect);
                }
                std::thread::sleep(CONNECT_RETRY_BACKOFF.min(remaining));
                continue;
            }
            Err(_) => return Err(D2bClientError::Connect),
        }
    }
}

fn wait_connect_writable(fd: &OwnedFd, deadline: Instant) -> Result<(), D2bClientError> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(D2bClientError::Connect);
        }
        let timeout = PollTimeout::try_from(remaining).unwrap_or(PollTimeout::MAX);
        let mut fds = [PollFd::new(fd.as_fd(), PollFlags::POLLOUT)];
        match poll(&mut fds, timeout) {
            Ok(0) => return Err(D2bClientError::Connect),
            Ok(_) => return Ok(()),
            Err(Errno::EINTR) => continue,
            Err(_) => return Err(D2bClientError::Connect),
        }
    }
}

fn finish_connect(fd: OwnedFd) -> Result<OwnedFd, D2bClientError> {
    let pending_error =
        getsockopt(&fd, sockopt::SocketError).map_err(|_| D2bClientError::Connect)?;
    if pending_error == 0 {
        Ok(fd)
    } else {
        Err(D2bClientError::Connect)
    }
}

pub fn shell_selection(action: ShellAction, shell_handle: &str, force: bool) -> TerminalSelection {
    TerminalSelection {
        selection: Some(Selection::Shell(ShellSelection {
            action: EnumOrUnknown::new(action),
            shell_handle: shell_handle.to_owned(),
            force,
            ..Default::default()
        })),
        ..Default::default()
    }
}

fn supports_shell(workload: &daemon::WorkloadProjection) -> bool {
    workload.runtime.as_ref().is_some_and(|runtime| {
        runtime.supported_capabilities.iter().any(|capability| {
            capability.enum_value().ok()
                == Some(daemon::RuntimeCapability::RUNTIME_CAPABILITY_SHELL)
        })
    })
}

fn workload_summary(
    workload: &daemon::WorkloadProjection,
) -> Result<WorkloadSummary, D2bClientError> {
    let identity = workload.identity.as_ref().ok_or(D2bClientError::Protocol)?;
    let target =
        TargetId::new(identity.canonical_target.clone()).map_err(|_| D2bClientError::Protocol)?;
    let compatibility_id =
        TargetId::new(identity.workload_name.clone()).map_err(|_| D2bClientError::Protocol)?;
    let mut summary =
        WorkloadSummary::discovered(target, compatibility_id, map_power_state(workload));
    summary.workload_name = Some(identity.workload_name.clone());
    summary.provider_kind = map_provider(workload);
    summary.isolation_posture = if summary.provider_kind.is_unsafe_local() {
        IsolationPosture::UnsafeLocal
    } else {
        IsolationPosture::VirtualMachine
    };
    summary.session_persistence = if summary.provider_kind.is_unsafe_local() {
        SessionPersistence::UserManagerLifetime
    } else {
        SessionPersistence::RuntimeManaged
    };
    summary.availability = if workload
        .lifecycle
        .as_ref()
        .is_some_and(|value| value.degraded)
    {
        TargetAvailability::Degraded
    } else {
        TargetAvailability::Ready
    };
    Ok(summary)
}

fn map_provider(workload: &daemon::WorkloadProjection) -> ProviderKind {
    match workload
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.kind.enum_value().ok())
    {
        Some(daemon::RuntimeKind::RUNTIME_KIND_UNSAFE_LOCAL) => ProviderKind::UnsafeLocal,
        Some(daemon::RuntimeKind::RUNTIME_KIND_QEMU_MEDIA) => ProviderKind::QemuMedia,
        Some(daemon::RuntimeKind::RUNTIME_KIND_ACA_SANDBOX)
        | Some(daemon::RuntimeKind::RUNTIME_KIND_REMOTE) => ProviderKind::ProviderManaged,
        _ => ProviderKind::LocalVm,
    }
}

fn map_power_state(workload: &daemon::WorkloadProjection) -> TargetPowerState {
    match workload
        .lifecycle
        .as_ref()
        .and_then(|lifecycle| lifecycle.state.enum_value().ok())
    {
        Some(daemon::WorkloadLifecycleState::WORKLOAD_LIFECYCLE_STATE_RUNNING)
        | Some(daemon::WorkloadLifecycleState::WORKLOAD_LIFECYCLE_STATE_BOOTED) => {
            TargetPowerState::Online
        }
        Some(daemon::WorkloadLifecycleState::WORKLOAD_LIFECYCLE_STATE_STOPPED)
        | Some(daemon::WorkloadLifecycleState::WORKLOAD_LIFECYCLE_STATE_STOPPING) => {
            TargetPowerState::Offline
        }
        _ => TargetPowerState::Unknown,
    }
}

fn shell_session(value: &terminal::ShellSession) -> Result<ShellSession, D2bClientError> {
    let name =
        FriendlyName::from_candidate(&value.shell_handle).map_err(|_| D2bClientError::Protocol)?;
    let state = match value
        .state
        .enum_value()
        .map_err(|_| D2bClientError::Protocol)?
    {
        terminal::ShellSessionState::SHELL_SESSION_STATE_ATTACHED => ShellVisualState::Attached,
        terminal::ShellSessionState::SHELL_SESSION_STATE_DETACHED => ShellVisualState::Detached,
        _ => ShellVisualState::Unavailable,
    };
    Ok(ShellSession {
        name,
        visual_state: state,
        is_default: value.is_default,
    })
}

fn operation_id() -> String {
    format!(
        "wlterm-{}-{}",
        std::process::id(),
        OPERATION_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

#[derive(Debug)]
pub enum D2bClientError {
    Canonical(ClientError),
    Connect,
    Runtime,
    Timeout,
    Protocol,
    LiveRoutingUnavailable,
}

impl From<ClientError> for D2bClientError {
    fn from(value: ClientError) -> Self {
        Self::Canonical(value)
    }
}

impl fmt::Display for D2bClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Canonical(_) => "authenticated d2b client operation failed",
            Self::Connect => "unable to connect to the authenticated d2b endpoint",
            Self::Runtime => "unable to start the authenticated client runtime",
            Self::Timeout => "authenticated d2b client operation timed out",
            Self::Protocol => "authenticated d2b service response was invalid",
            Self::LiveRoutingUnavailable => LIVE_ROUTING_UNAVAILABLE,
        })
    }
}

impl std::error::Error for D2bClientError {}

#[cfg(test)]
mod tests {
    use super::*;
    use protobuf::MessageField;

    #[test]
    fn binds_the_frozen_w6_canonical_source() {
        assert_eq!(
            CLIENT_SOURCE_REVISION,
            "9dc902243cdd7aba7ef269988b96f0aae6e037da"
        );
        assert_eq!(
            CLIENT_SOURCE_FINGERPRINT,
            "5a20cef3a64281df819eeb76bdfe385999755479b467b559653011582fb9c043"
        );
        assert_eq!(ServiceKind::Daemon, d2b_client_toolkit::ServiceKind::Daemon);
    }

    #[test]
    fn shell_management_uses_canonical_terminal_selection() {
        let selection = shell_selection(ShellAction::SHELL_ACTION_KILL, "quiet-otter", false);
        let Some(Selection::Shell(shell)) = selection.selection else {
            panic!("shell selection");
        };
        assert_eq!(
            shell.action.enum_value().unwrap(),
            ShellAction::SHELL_ACTION_KILL
        );
        assert_eq!(shell.shell_handle, "quiet-otter");
        assert!(!shell.force);
    }

    #[test]
    fn canonical_projection_preserves_same_user_shell_posture() {
        let workload = daemon::WorkloadProjection {
            identity: MessageField::some(daemon::WorkloadIdentityProjection {
                workload_name: "tools".to_owned(),
                canonical_target: "tools.local-root.d2b".to_owned(),
                ..Default::default()
            }),
            lifecycle: MessageField::some(daemon::WorkloadLifecycleProjection {
                state: EnumOrUnknown::new(
                    daemon::WorkloadLifecycleState::WORKLOAD_LIFECYCLE_STATE_RUNNING,
                ),
                ..Default::default()
            }),
            runtime: MessageField::some(daemon::RuntimeProjection {
                kind: EnumOrUnknown::new(daemon::RuntimeKind::RUNTIME_KIND_UNSAFE_LOCAL),
                supported_capabilities: vec![EnumOrUnknown::new(
                    daemon::RuntimeCapability::RUNTIME_CAPABILITY_SHELL,
                )],
                ..Default::default()
            }),
            ..Default::default()
        };

        assert!(supports_shell(&workload));
        let summary = workload_summary(&workload).unwrap();
        assert_eq!(summary.target.as_str(), "tools.local-root.d2b");
        assert_eq!(summary.id.as_str(), "tools");
        assert_eq!(summary.provider_kind, ProviderKind::UnsafeLocal);
        assert_eq!(summary.isolation_posture, IsolationPosture::UnsafeLocal);
        assert_eq!(
            summary.session_persistence,
            SessionPersistence::UserManagerLifetime
        );
        assert_eq!(summary.power_state, TargetPowerState::Online);
        assert!(summary.shell_launcher_item.is_none());
    }

    #[test]
    fn live_stream_routing_remains_fail_closed() {
        let boundary = D2bActionBoundary::new(D2bClientConfig::default());
        assert!(matches!(
            boundary.open_shell_blocking("work.local-root.d2b", None, false),
            Err(D2bClientError::LiveRoutingUnavailable)
        ));
    }

    /// Regression coverage for the `connect_seqpacket` EAGAIN/EINPROGRESS
    /// handling: bind a real `SOCK_SEQPACKET` listener with a backlog far
    /// smaller than the number of concurrent connect attempts, and assert
    /// every attempt still completes once the acceptor drains the backlog,
    /// instead of failing on the first transient `EAGAIN`.
    #[test]
    fn connect_seqpacket_retries_past_backlog_contention() {
        use nix::sys::socket::{accept, bind, listen, Backlog};
        use std::os::fd::FromRawFd;
        use std::sync::atomic::AtomicUsize;

        static PATH_COUNTER: AtomicUsize = AtomicUsize::new(0);

        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("crates/<name> manifest dir has a workspace root")
            .to_path_buf();
        let socket_path = workspace_root.join("target").join(format!(
            "wt-{}-{}.sock",
            std::process::id(),
            PATH_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&socket_path);

        let listener = socket(
            AddressFamily::Unix,
            SockType::SeqPacket,
            SockFlag::SOCK_CLOEXEC,
            None,
        )
        .expect("create listener socket");
        let address = UnixAddr::new(socket_path.as_path()).expect("listener path fits sun_path");
        bind(listener.as_raw_fd(), &address).expect("bind listener");
        listen(&listener, Backlog::new(1).expect("valid backlog")).expect("listen");

        const CLIENTS: usize = 8;
        let acceptor = std::thread::spawn({
            let listener_fd = listener.as_raw_fd();
            move || {
                let mut accepted = Vec::with_capacity(CLIENTS);
                for _ in 0..CLIENTS {
                    loop {
                        match accept(listener_fd) {
                            Ok(fd) => {
                                // SAFETY: `accept` returns a freshly opened,
                                // uniquely owned descriptor.
                                accepted.push(unsafe { OwnedFd::from_raw_fd(fd) });
                                break;
                            }
                            Err(Errno::EINTR) => continue,
                            Err(Errno::EAGAIN) => {
                                std::thread::sleep(Duration::from_millis(1));
                                continue;
                            }
                            Err(error) => panic!("accept failed: {error}"),
                        }
                    }
                }
                accepted
            }
        });

        let path_string = socket_path.to_str().expect("utf8 socket path").to_owned();
        let clients: Vec<_> = (0..CLIENTS)
            .map(|_| {
                let path = path_string.clone();
                std::thread::spawn(move || connect_seqpacket(&path))
            })
            .collect();

        let results: Vec<_> = clients
            .into_iter()
            .map(|handle| handle.join().expect("client thread"))
            .collect();
        let accepted = acceptor.join().expect("acceptor thread");

        let _ = std::fs::remove_file(&socket_path);

        assert_eq!(accepted.len(), CLIENTS);
        assert!(
            results.iter().all(Result::is_ok),
            "every connect_seqpacket call should succeed once the backlog drains: {results:?}"
        );
    }
}
