//! d2b public-socket client boundary for wlterm.
//!
//! This crate is the only wlterm layer that owns d2b public daemon I/O. It
//! plans through `wlterm-core`, executes through `d2b-client`, and never talks
//! to the privileged broker or mutates host state directly.

use async_io::Async;
use d2b_client::{
    read_hello_response, send_hello, AttachedShell, ClientError, FrameBounds, PublicSocketClient,
};
use d2b_toolkit_core::{
    Capability, GraphicalLaunchPosture, Hello, HelloResponse, IsolationPosture as ToolkitIsolation,
    KnownFeatureFlag, LauncherItemKind, SessionPersistencePosture, ShellKillResult,
    ShellListResult, ShellName, ShellOp, ShellSessionState, SocketClass, TerminalSize,
    ToolkitError, WorkloadAvailability, WorkloadListResult, WorkloadProviderKind,
    WorkloadPublicSummary, WorkloadState,
};
use futures::executor::block_on;
use futures::future::{select, Either};
use futures::io::{AsyncRead, AsyncWrite};
use sha2::{Digest, Sha256};
use std::fmt;
use std::future::Future;
use std::io;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use wlterm_core::friendly_name::FriendlyName;
use wlterm_core::{
    DisabledReason, IsolationPosture, PlannedAction, ProviderKind, SafeCorrelation,
    SessionPersistence, ShellLauncherItem, ShellSession, ShellTarget, ShellVisualState,
    TargetAvailability, TargetId, VmPowerState, WorkloadSummary,
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

    fn block_on_operation<F, T>(
        &self,
        action: &'static str,
        trace: ActionTrace,
        operation: F,
    ) -> Result<T, D2bClientError>
    where
        F: Future<Output = Result<T, D2bClientError>>,
    {
        let timeout = Duration::from_millis(self.config.operation_timeout_ms);
        block_on(async {
            let operation = Box::pin(operation);
            let timer = Box::pin(async_io::Timer::after(timeout));
            match select(operation, timer).await {
                Either::Left((result, _)) => result,
                Either::Right(_) => Err(D2bClientError::timeout(action, trace)),
            }
        })
    }

    pub fn plan_to_shell_op(&self, action: &PlannedAction) -> Option<ShellOp> {
        match action {
            PlannedAction::ListSessions { target } => {
                Some(ShellOp::List(d2b_toolkit_core::ShellListArgs {
                    vm: target.id.as_str().to_string(),
                }))
            }
            PlannedAction::AttachShell {
                target,
                name,
                force,
            } => Some(ShellOp::Attach(d2b_toolkit_core::ShellAttachArgs {
                vm: target.id.as_str().to_string(),
                name: name.as_ref().map(to_toolkit_shell_name),
                force: *force,
                initial_terminal_size: self.config.initial_terminal_size,
            })),
            PlannedAction::KillShell { target, name } => {
                Some(ShellOp::Kill(d2b_toolkit_core::ShellKillArgs {
                    vm: target.id.as_str().to_string(),
                    name: to_toolkit_shell_name(name),
                }))
            }
            PlannedAction::DetachShell { target, name } => {
                Some(ShellOp::Detach(d2b_toolkit_core::ShellDetachArgs {
                    vm: target.id.as_str().to_string(),
                    name: Some(to_toolkit_shell_name(name)),
                }))
            }
            PlannedAction::RefreshTargets
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
        let response = self.block_on_operation("connect", trace.clone(), async {
            send_hello(
                &mut transport,
                &Hello::toolkit_client(vec![
                    KnownFeatureFlag::TypedErrors.wire_value(),
                    KnownFeatureFlag::ConfiguredLaunchV1.wire_value(),
                    KnownFeatureFlag::UnsafeLocalProviderV1.wire_value(),
                    KnownFeatureFlag::UnsafeLocalShellV1.wire_value(),
                ]),
                bounds,
            )
            .await
            .map_err(|error| D2bClientError::from_client_error("connect", trace.clone(), error))?;
            read_hello_response(&mut transport, bounds)
                .await
                .map_err(|error| D2bClientError::from_client_error("connect", trace, error))
        })?;
        let HelloResponse::HelloOk(hello) = response else {
            return Err(D2bClientError::protocol(
                "connect",
                ActionTrace::for_label("connect-public-socket"),
                "hello-rejected",
            ));
        };
        Ok(PublicSocketClient::with_bounds_and_negotiated_capabilities(
            transport,
            bounds,
            hello.negotiated_capabilities(),
        ))
    }

    pub fn inventory_blocking(&self) -> Result<Vec<WorkloadSummary>, D2bClientError> {
        let client = self.connect()?;
        let outcome = self.block_on_operation(
            "inventory",
            ActionTrace::for_label("workload-inventory"),
            self.inventory_with_client(client),
        )?;
        Ok(outcome.workloads)
    }

    pub fn discover_blocking(&self) -> Result<Vec<WorkloadSummary>, D2bClientError> {
        let client = self.connect()?;
        let outcome = self.block_on_operation(
            "inventory",
            ActionTrace::for_label("workload-inventory"),
            self.discover_with_client(client),
        )?;
        Ok(outcome.workloads)
    }

    pub async fn discover_with_client<T>(
        &self,
        mut client: PublicSocketClient<T>,
    ) -> Result<D2bInventoryOutcome<T>, D2bClientError>
    where
        T: AsyncRead + AsyncWrite + Unpin,
    {
        let trace = ActionTrace::for_label("workload-inventory");
        let supports_unsafe_shell = client
            .negotiated_capabilities()
            .is_some_and(|caps| caps.has(KnownFeatureFlag::UnsafeLocalShellV1));
        let inventory = client
            .workload_inventory()
            .await
            .map_err(|err| D2bClientError::from_client_error("inventory", trace.clone(), err))?;
        let workloads = shell_workloads_from_inventory(&inventory, supports_unsafe_shell)
            .map_err(|kind| D2bClientError::protocol("inventory", trace, kind))?;
        Ok(D2bInventoryOutcome { client, workloads })
    }

    pub async fn inventory_with_client<T>(
        &self,
        client: PublicSocketClient<T>,
    ) -> Result<D2bInventoryOutcome<T>, D2bClientError>
    where
        T: AsyncRead + AsyncWrite + Unpin,
    {
        let trace = ActionTrace::for_label("workload-inventory");
        let outcome = self.discover_with_client(client).await?;
        let mut client = outcome.client;
        let mut workloads = outcome.workloads;
        let supports_unsafe_shell = client
            .negotiated_capabilities()
            .is_some_and(|caps| caps.has(KnownFeatureFlag::UnsafeLocalShellV1));

        if supports_unsafe_shell
            && workloads
                .iter()
                .any(WorkloadSummary::requires_unsafe_local_shell)
        {
            client.require_unsafe_local_shell().map_err(|err| {
                D2bClientError::from_client_error("inventory", trace.clone(), err)
            })?;
        }

        for workload in &mut workloads {
            if !workload.actions_available() {
                continue;
            }
            match client
                .shell_list(workload.target.as_str().to_string())
                .await
            {
                Ok(list) => {
                    workload.sessions = shell_list_to_sessions(&list)
                        .map_err(|kind| D2bClientError::protocol("list", trace.clone(), kind))?;
                }
                Err(error) => match availability_from_shell_error(&error) {
                    Some(availability) => {
                        workload.availability = availability;
                        workload.sessions.clear();
                    }
                    None => {
                        return Err(D2bClientError::from_client_error(
                            "list",
                            trace.clone(),
                            error,
                        ));
                    }
                },
            }
        }

        Ok(D2bInventoryOutcome { client, workloads })
    }

    pub fn execute_blocking(
        &self,
        action: PlannedAction,
    ) -> Result<D2bActionOutcome<BlockingUnixTransport>, D2bClientError> {
        let client = self.connect()?;
        let action_label = action.metrics_label_value();
        self.block_on_operation(
            action_label,
            ActionTrace::for_label(action_label),
            self.execute_with_client(client, action),
        )
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
            PlannedAction::RefreshTargets => Ok(D2bActionOutcome::RefreshQueued { client }),
            PlannedAction::Disabled { reason } => Ok(D2bActionOutcome::Disabled { client, reason }),
            PlannedAction::FocusExistingShell { target, name } => {
                Ok(D2bActionOutcome::FocusExisting {
                    client,
                    target: target.id,
                    name,
                })
            }
            PlannedAction::PromptAlreadyAttached { target, name } => {
                Ok(D2bActionOutcome::PromptAlreadyAttached {
                    client,
                    target: target.id,
                    name,
                })
            }
            PlannedAction::PromptStop { target, name } => Ok(D2bActionOutcome::PromptStop {
                client,
                target: target.id,
                name,
                requires_confirmation: true,
            }),
            PlannedAction::ListSessions { target } => {
                let mut client = prepare_shell_client(client, &target, "list", &trace)?;
                let list = client
                    .shell_list(target.id.as_str().to_string())
                    .await
                    .map_err(|err| D2bClientError::from_client_error("list", trace.clone(), err))?;
                let sessions = shell_list_to_sessions(&list)
                    .map_err(|kind| D2bClientError::protocol("list", trace.clone(), kind))?;
                Ok(D2bActionOutcome::Listed {
                    client,
                    target: target.id,
                    sessions,
                })
            }
            PlannedAction::AttachShell {
                target,
                name,
                force,
            } => {
                let client = prepare_shell_client(client, &target, "open", &trace)?;
                let attached = client
                    .attach_shell(
                        target.id.as_str().to_string(),
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
                    target: target.id,
                    resolved_name,
                    force,
                    trace,
                })
            }
            PlannedAction::KillShell { target, name } => {
                let mut client = prepare_shell_client(client, &target, "stop", &trace)?;
                let result = client
                    .shell_kill(target.id.as_str().to_string(), to_toolkit_shell_name(&name))
                    .await
                    .map_err(|err| D2bClientError::from_client_error("stop", trace.clone(), err))?;
                Ok(D2bActionOutcome::Killed {
                    client,
                    target: target.id,
                    name,
                    result,
                    trace,
                })
            }
            PlannedAction::DetachShell { target, name } => {
                let mut client = prepare_shell_client(client, &target, "detach", &trace)?;
                let result = client
                    .shell_detach(
                        target.id.as_str().to_string(),
                        Some(to_toolkit_shell_name(&name)),
                    )
                    .await
                    .map_err(|err| {
                        D2bClientError::from_client_error("detach", trace.clone(), err)
                    })?;
                Ok(D2bActionOutcome::Detached {
                    client,
                    target: target.id,
                    name,
                    result,
                    trace,
                })
            }
        }
    }
}

pub struct D2bInventoryOutcome<T> {
    pub client: PublicSocketClient<T>,
    pub workloads: Vec<WorkloadSummary>,
}

impl<T> fmt::Debug for D2bInventoryOutcome<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("D2bInventoryOutcome")
            .field("workloads_len", &self.workloads.len())
            .finish()
    }
}

fn prepare_shell_client<T>(
    mut client: PublicSocketClient<T>,
    target: &ShellTarget,
    action: &'static str,
    trace: &ActionTrace,
) -> Result<PublicSocketClient<T>, D2bClientError>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    if target.provider_kind.is_unsafe_local() {
        client
            .require_unsafe_local_shell()
            .map_err(|err| D2bClientError::from_client_error(action, trace.clone(), err))?;
    }
    Ok(client)
}

fn shell_workloads_from_inventory(
    inventory: &WorkloadListResult,
    supports_unsafe_shell: bool,
) -> Result<Vec<WorkloadSummary>, &'static str> {
    inventory
        .workloads
        .iter()
        .filter(|workload| workload.capabilities().has(Capability::PersistentShell))
        .filter_map(|workload| {
            workload
                .launcher_items()
                .iter()
                .find(|item| {
                    item.kind() == LauncherItemKind::Shell
                        && item.capabilities().has(Capability::PersistentShell)
                })
                .map(|item| (workload, item))
        })
        .map(|(workload, shell_item)| {
            workload_to_summary(workload, shell_item, supports_unsafe_shell)
        })
        .collect()
}

fn workload_to_summary(
    workload: &WorkloadPublicSummary,
    shell_item: &d2b_toolkit_core::LauncherItemSummary,
    supports_unsafe_shell: bool,
) -> Result<WorkloadSummary, &'static str> {
    let target = TargetId::new(workload.identity().target().as_str().to_string())
        .map_err(|_| "invalid-workload-target")?;
    let compatibility_id = workload
        .identity()
        .legacy_vm_name()
        .map(|legacy| legacy.as_str())
        .unwrap_or_else(|| workload.identity().workload_id().as_str());
    let compatibility_id =
        TargetId::new(compatibility_id.to_string()).map_err(|_| "invalid-workload-id")?;
    let provider_kind = map_provider_kind(workload.provider_kind());
    let availability = map_availability(workload.availability(), workload.graphical_posture());
    let power_state = map_power_state(
        workload.state(),
        workload.provider_kind(),
        workload.availability(),
    );
    let mut summary = WorkloadSummary::discovered(target, compatibility_id, power_state);
    summary.legacy_vm_name = workload
        .identity()
        .legacy_vm_name()
        .map(|legacy| legacy.as_str().to_string());
    summary.workload_name = workload.identity().workload_name().map(str::to_string);
    summary.provider_kind = provider_kind;
    summary.isolation_posture = match workload.execution_posture().isolation() {
        ToolkitIsolation::VirtualMachine => IsolationPosture::VirtualMachine,
        ToolkitIsolation::ProviderManaged => IsolationPosture::ProviderManaged,
        ToolkitIsolation::UnsafeLocal => IsolationPosture::UnsafeLocal,
    };
    summary.session_persistence = match workload.execution_posture().session_persistence() {
        SessionPersistencePosture::RuntimeManaged => SessionPersistence::RuntimeManaged,
        SessionPersistencePosture::UserManagerLifetime => SessionPersistence::UserManagerLifetime,
    };
    summary.availability = availability;
    summary.shell_feature_available = !provider_kind.is_unsafe_local() || supports_unsafe_shell;
    summary.shell_launcher_item = Some(ShellLauncherItem {
        id: shell_item.id().as_str().to_string(),
        name: shell_item.name().to_string(),
    });
    Ok(summary)
}

fn map_provider_kind(kind: WorkloadProviderKind) -> ProviderKind {
    match kind {
        WorkloadProviderKind::LocalVm => ProviderKind::LocalVm,
        WorkloadProviderKind::QemuMedia => ProviderKind::QemuMedia,
        WorkloadProviderKind::ProviderManaged => ProviderKind::ProviderManaged,
        WorkloadProviderKind::UnsafeLocal => ProviderKind::UnsafeLocal,
    }
}

fn map_power_state(
    state: WorkloadState,
    provider: WorkloadProviderKind,
    availability: WorkloadAvailability,
) -> VmPowerState {
    if provider == WorkloadProviderKind::UnsafeLocal {
        return if availability == WorkloadAvailability::Ready {
            VmPowerState::Online
        } else {
            VmPowerState::Unknown
        };
    }
    match state {
        WorkloadState::Running => VmPowerState::Online,
        WorkloadState::Stopped | WorkloadState::Stopping => VmPowerState::Offline,
        WorkloadState::Starting | WorkloadState::Failed => VmPowerState::Unknown,
    }
}

fn map_availability(
    availability: WorkloadAvailability,
    graphical: GraphicalLaunchPosture,
) -> TargetAvailability {
    match availability {
        WorkloadAvailability::Ready => match graphical {
            GraphicalLaunchPosture::GraphicalSessionInactive => {
                TargetAvailability::GraphicalSessionInactive
            }
            GraphicalLaunchPosture::WaylandUnavailable => TargetAvailability::WaylandUnavailable,
            GraphicalLaunchPosture::ProxyUnavailable => TargetAvailability::ProxyUnavailable,
            GraphicalLaunchPosture::Proxied | GraphicalLaunchPosture::NotApplicable => {
                TargetAvailability::Ready
            }
        },
        WorkloadAvailability::HelperUnavailable => TargetAvailability::HelperUnavailable,
        WorkloadAvailability::HelperStale => TargetAvailability::HelperStale,
        WorkloadAvailability::UserManagerUnavailable => TargetAvailability::UserManagerUnavailable,
        WorkloadAvailability::GraphicalSessionInactive => {
            TargetAvailability::GraphicalSessionInactive
        }
        WorkloadAvailability::WaylandUnavailable => TargetAvailability::WaylandUnavailable,
        WorkloadAvailability::ProxyUnavailable => TargetAvailability::ProxyUnavailable,
        WorkloadAvailability::Degraded => TargetAvailability::Degraded,
    }
}

fn availability_from_shell_error(error: &ClientError) -> Option<TargetAvailability> {
    let ClientError::Daemon { kind } = error else {
        return None;
    };
    Some(if kind.contains("helper-unavailable") {
        TargetAvailability::HelperUnavailable
    } else if kind.contains("helper-stale") {
        TargetAvailability::HelperStale
    } else if kind.contains("user-manager-unavailable") {
        TargetAvailability::UserManagerUnavailable
    } else if kind.contains("graphical-session-inactive") {
        TargetAvailability::GraphicalSessionInactive
    } else if kind.contains("wayland-unavailable") {
        TargetAvailability::WaylandUnavailable
    } else if kind.contains("proxy-unavailable") {
        TargetAvailability::ProxyUnavailable
    } else {
        TargetAvailability::Degraded
    })
}

fn connect_seqpacket(path: &str, connect_timeout: Duration) -> io::Result<OwnedFd> {
    use nix::errno::Errno;
    use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
    use nix::sys::socket::{
        connect, getsockopt, socket, sockopt, AddressFamily, SockFlag, SockType, UnixAddr,
    };

    let fd = socket(
        AddressFamily::Unix,
        SockType::SeqPacket,
        SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK,
        None,
    )
    .map_err(errno_to_io)?;
    let addr = UnixAddr::new(Path::new(path)).map_err(errno_to_io)?;
    match connect(fd.as_raw_fd(), &addr) {
        Ok(()) | Err(Errno::EISCONN) => {}
        Err(Errno::EINPROGRESS) | Err(Errno::EALREADY) | Err(Errno::EAGAIN) => {
            let mut pollfd = [PollFd::new(fd.as_fd(), PollFlags::POLLOUT)];
            let timeout = PollTimeout::try_from(connect_timeout)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
            if poll(&mut pollfd, timeout).map_err(errno_to_io)? == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "timed out connecting to d2bd public socket",
                ));
            }
            let socket_error = getsockopt(&fd, sockopt::SocketError).map_err(errno_to_io)?;
            if socket_error != 0 {
                return Err(io::Error::from_raw_os_error(socket_error));
            }
        }
        Err(error) => return Err(errno_to_io(error)),
    }
    Ok(fd)
}

fn errno_to_io(error: nix::errno::Errno) -> io::Error {
    io::Error::from_raw_os_error(error as i32)
}

const MAX_PUBLIC_PACKET: usize = 1024 * 1024 + 4;

pub struct BlockingUnixTransport {
    fd: Async<OwnedFd>,
    read_buf: Vec<u8>,
    read_pos: usize,
    write_buf: Vec<u8>,
}

impl BlockingUnixTransport {
    fn connect(path: &str, timeout: Duration) -> io::Result<Self> {
        Ok(Self {
            fd: Async::new(connect_seqpacket(path, timeout)?)?,
            read_buf: Vec::new(),
            read_pos: 0,
            write_buf: Vec::new(),
        })
    }
}

impl AsyncRead for BlockingUnixTransport {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if self.read_pos >= self.read_buf.len() {
            loop {
                let mut packet = vec![0_u8; MAX_PUBLIC_PACKET];
                match nix::sys::socket::recv(
                    self.fd.get_ref().as_raw_fd(),
                    &mut packet,
                    nix::sys::socket::MsgFlags::MSG_DONTWAIT
                        | nix::sys::socket::MsgFlags::MSG_TRUNC,
                ) {
                    Ok(len) if len > packet.len() => {
                        return Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "d2bd public socket packet exceeded the frame bound",
                        )));
                    }
                    Ok(len) => {
                        packet.truncate(len);
                        self.read_buf = packet;
                        self.read_pos = 0;
                        if self.read_buf.is_empty() {
                            return Poll::Ready(Ok(0));
                        }
                        break;
                    }
                    Err(nix::errno::Errno::EAGAIN) => match self.fd.poll_readable(cx) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Ok(())) => continue,
                        Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                    },
                    Err(error) => return Poll::Ready(Err(errno_to_io(error))),
                }
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
        if self.write_buf.len().saturating_add(buf.len()) > MAX_PUBLIC_PACKET {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "d2bd public socket packet exceeded the frame bound",
            )));
        }
        self.write_buf.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while !self.write_buf.is_empty() {
            match nix::sys::socket::send(
                self.fd.get_ref().as_raw_fd(),
                &self.write_buf,
                nix::sys::socket::MsgFlags::MSG_DONTWAIT,
            ) {
                Ok(sent) if sent == self.write_buf.len() => self.write_buf.clear(),
                Ok(_) => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "short write on public seqpacket socket",
                    )));
                }
                Err(nix::errno::Errno::EAGAIN) => match self.fd.poll_writable(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(())) => continue,
                    Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                },
                Err(error) => return Poll::Ready(Err(errno_to_io(error))),
            }
        }
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_flush(cx)
    }
}

pub fn to_toolkit_shell_name(name: &FriendlyName) -> ShellName {
    ShellName::new(name.as_str().to_string()).expect("validated friendly name is a toolkit shell")
}

pub fn kill_shell_op(target: &TargetId, name: &FriendlyName) -> ShellOp {
    ShellOp::Kill(d2b_toolkit_core::ShellKillArgs {
        vm: target.as_str().to_string(),
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
    FeatureUnavailable(KnownFeatureFlag),
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
            Self::FeatureUnavailable(_) => "feature-unavailable",
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

    fn timeout(action: &'static str, trace: ActionTrace) -> Self {
        Self {
            action,
            kind: D2bClientErrorKind::Timeout,
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

    pub fn remediation(&self) -> Option<&'static str> {
        matches!(self.kind, D2bClientErrorKind::FeatureUnavailable(_))
            .then_some("update d2b, d2bd, d2b-toolkit, and d2b-unsafe-local-helper together")
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
        )?;
        if let Some(remediation) = self.remediation() {
            write!(f, "; remediation: {remediation}")?;
        }
        Ok(())
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
        ClientError::Core(ToolkitError::FeatureUnavailable { feature }) => {
            D2bClientErrorKind::FeatureUnavailable(*feature)
        }
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
        ClientError::Core(
            ToolkitError::InvalidTarget { .. }
            | ToolkitError::InvalidToken { .. }
            | ToolkitError::InvalidIdentity { .. }
            | ToolkitError::InconsistentWorkloadIdentity
            | ToolkitError::LauncherItemUnavailable
            | ToolkitError::AmbiguousLauncherItems { .. },
        ) => D2bClientErrorKind::Protocol("invalid-public-contract"),
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
        target: TargetId,
        name: FriendlyName,
    },
    PromptAlreadyAttached {
        client: PublicSocketClient<T>,
        target: TargetId,
        name: FriendlyName,
    },
    PromptStop {
        client: PublicSocketClient<T>,
        target: TargetId,
        name: FriendlyName,
        requires_confirmation: bool,
    },
    Listed {
        client: PublicSocketClient<T>,
        target: TargetId,
        sessions: Vec<ShellSession>,
    },
    Attached {
        attached: AttachedShell<T>,
        target: TargetId,
        resolved_name: FriendlyName,
        force: bool,
        trace: ActionTrace,
    },
    Killed {
        client: PublicSocketClient<T>,
        target: TargetId,
        name: FriendlyName,
        result: ShellKillResult,
        trace: ActionTrace,
    },
    Detached {
        client: PublicSocketClient<T>,
        target: TargetId,
        name: FriendlyName,
        result: d2b_toolkit_core::ShellDetachResult,
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
            Self::FocusExisting { target, .. } => f
                .debug_struct("FocusExisting")
                .field("target", target)
                .field("name", &"<redacted>")
                .finish(),
            Self::PromptAlreadyAttached { target, .. } => f
                .debug_struct("PromptAlreadyAttached")
                .field("target", target)
                .field("name", &"<redacted>")
                .finish(),
            Self::PromptStop {
                target,
                requires_confirmation,
                ..
            } => f
                .debug_struct("PromptStop")
                .field("target", target)
                .field("name", &"<redacted>")
                .field("requires_confirmation", requires_confirmation)
                .finish(),
            Self::Listed {
                target, sessions, ..
            } => f
                .debug_struct("Listed")
                .field("target", target)
                .field("sessions_len", &sessions.len())
                .finish(),
            Self::Attached {
                target,
                force,
                trace,
                ..
            } => f
                .debug_struct("Attached")
                .field("target", target)
                .field("resolved_name", &"<redacted>")
                .field("force", force)
                .field("trace", trace)
                .finish(),
            Self::Killed {
                target,
                result,
                trace,
                ..
            } => f
                .debug_struct("Killed")
                .field("target", target)
                .field("name", &"<redacted>")
                .field("killed", &result.killed)
                .field("state", &result.state)
                .field("trace", trace)
                .finish(),
            Self::Detached {
                target,
                result,
                trace,
                ..
            } => f
                .debug_struct("Detached")
                .field("target", target)
                .field("name", &"<redacted>")
                .field("detached", &result.detached)
                .field("trace", trace)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use d2b_toolkit_core::{
        ErrorEnvelope, NegotiatedCapabilities, OpaqueHandle, PublicResponse, ShellAttachResult,
        ShellListEntry, ShellOpResponse, WorkloadOpResponse,
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

    fn target_id(name: &str) -> TargetId {
        TargetId::new(name).unwrap()
    }

    fn target(name: &str) -> ShellTarget {
        ShellTarget {
            id: target_id(name),
            provider_kind: ProviderKind::LocalVm,
        }
    }

    fn unsafe_target(name: &str) -> ShellTarget {
        ShellTarget {
            id: target_id(name),
            provider_kind: ProviderKind::UnsafeLocal,
        }
    }

    fn boundary() -> D2bActionBoundary {
        D2bActionBoundary::new(D2bClientConfig::default())
    }

    fn list_response(op_id: u64) -> PublicResponse {
        PublicResponse::Shell {
            op_id: Some(op_id),
            response: ShellOpResponse::List(ShellListResult {
                default_name: ShellName::new("quiet-otter").unwrap(),
                sessions: vec![ShellListEntry {
                    name: ShellName::new("quiet-otter").unwrap(),
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
                resolved_name: ShellName::new(name).unwrap(),
                state: ShellSessionState::Attached,
                force_evicted: false,
            }),
        }
    }

    fn kill_response(op_id: u64, name: &str) -> PublicResponse {
        PublicResponse::Shell {
            op_id: Some(op_id),
            response: ShellOpResponse::Kill(ShellKillResult {
                name: ShellName::new(name).unwrap(),
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

    fn fixture_inventory(json: &str) -> WorkloadListResult {
        match serde_json::from_str::<PublicResponse>(json).unwrap() {
            PublicResponse::Workload {
                response: WorkloadOpResponse::List(result),
                ..
            } => result,
            other => panic!("unexpected fixture response: {other:?}"),
        }
    }

    fn unsafe_shell_capabilities() -> NegotiatedCapabilities {
        NegotiatedCapabilities::from_features([KnownFeatureFlag::UnsafeLocalShellV1.wire_value()])
    }

    #[test]
    fn toolkit_v020_workload_fixtures_conform_and_filter_shell_items() {
        let local = fixture_inventory(include_str!(
            "../tests/fixtures/toolkit-v0.2.0/local-vm-list-response.json"
        ));
        let unsafe_local = fixture_inventory(include_str!(
            "../tests/fixtures/toolkit-v0.2.0/unsafe-local-list-response.json"
        ));
        let first_class = fixture_inventory(include_str!(
            "../tests/fixtures/toolkit-v0.2.0/first-class-local-vm-list-response.json"
        ));

        let local = shell_workloads_from_inventory(&local, true).unwrap();
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].target.as_str(), "corp-vm.work.d2b");
        assert_eq!(local[0].legacy_vm_name.as_deref(), Some("corp-vm"));

        let unsafe_local = shell_workloads_from_inventory(&unsafe_local, true).unwrap();
        assert_eq!(unsafe_local.len(), 1);
        assert_eq!(unsafe_local[0].provider_kind, ProviderKind::UnsafeLocal);
        assert_eq!(
            unsafe_local[0].shell_launcher_item.as_ref().unwrap().id,
            "terminal"
        );

        assert!(
            shell_workloads_from_inventory(&first_class, true)
                .unwrap()
                .is_empty(),
            "configured-launch-only workloads are not terminal targets"
        );
    }

    #[test]
    fn unsafe_local_discovery_exposes_posture_and_typed_remediation() {
        let inventory = fixture_inventory(include_str!(
            "../tests/fixtures/toolkit-v0.2.0/unsafe-local-list-response.json"
        ));

        let skewed = shell_workloads_from_inventory(&inventory, false).unwrap();
        let target = &skewed[0];
        assert_eq!(target.target.as_str(), "tools.host.d2b");
        assert_eq!(target.isolation_posture, IsolationPosture::UnsafeLocal);
        assert_eq!(
            target.session_persistence,
            SessionPersistence::UserManagerLifetime
        );
        assert!(!target.shell_feature_available);
        assert_eq!(
            target.remediation().unwrap().kind,
            wlterm_core::RemediationKind::UpdateD2b
        );

        let negotiated = shell_workloads_from_inventory(&inventory, true).unwrap();
        assert_eq!(
            negotiated[0].remediation().unwrap().kind,
            wlterm_core::RemediationKind::RestartHelper
        );
    }

    #[test]
    fn first_class_local_vm_without_legacy_name_uses_canonical_target() {
        let mut value: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/toolkit-v0.2.0/first-class-local-vm-list-response.json"
        ))
        .unwrap();
        let workload = &mut value["result"]["workloads"][0];
        workload["state"] = serde_json::json!("running");
        workload["capabilities"] =
            serde_json::json!(["configured-launch", "persistent-shell", "pty"]);
        workload["launcherItems"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "id": "terminal",
                "name": "Terminal",
                "icon": {"name": "terminal"},
                "type": "shell",
                "graphical": false,
                "capabilities": ["persistent-shell", "pty"]
            }));
        let inventory = fixture_inventory(&serde_json::to_string(&value).unwrap());
        let summaries = shell_workloads_from_inventory(&inventory, true).unwrap();

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].target.as_str(), "builder.dev.d2b");
        assert_eq!(summaries[0].id.as_str(), "builder");
        assert!(summaries[0].legacy_vm_name.is_none());
        assert!(summaries[0].actions_available());
    }

    #[test]
    fn list_executes_public_socket_shell_list() {
        block_on(async {
            let client =
                PublicSocketClient::new(FakePublicSocket::with_responses(vec![list_response(1)]));
            let outcome = boundary()
                .execute_with_client(
                    client,
                    PlannedAction::ListSessions {
                        target: target("work"),
                    },
                )
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
            assert_eq!(frames[0]["type"], "shell");
            assert_eq!(frames[0]["op"], "list");
            assert_eq!(frames[0]["args"]["vm"], "work");
        });
    }

    #[test]
    fn unsafe_local_feature_skew_fails_before_writing_with_update_remediation() {
        block_on(async {
            let client = PublicSocketClient::with_negotiated_capabilities(
                FakePublicSocket::default(),
                NegotiatedCapabilities::default(),
            );
            let error = boundary()
                .execute_with_client(
                    client,
                    PlannedAction::ListSessions {
                        target: unsafe_target("tools.host.d2b"),
                    },
                )
                .await
                .unwrap_err();

            assert_eq!(
                error.kind(),
                &D2bClientErrorKind::FeatureUnavailable(KnownFeatureFlag::UnsafeLocalShellV1)
            );
            assert!(error.remediation().unwrap().contains("update d2b"));
            let source = error.source.as_ref().unwrap();
            assert!(
                matches!(
                    source,
                    ClientError::Core(ToolkitError::FeatureUnavailable {
                        feature: KnownFeatureFlag::UnsafeLocalShellV1
                    })
                ),
                "{source:?}"
            );
        });
    }

    #[test]
    fn canonical_target_dispatch_covers_list_create_open_detach_and_stop() {
        block_on(async {
            let canonical = "tools.host.d2b";

            let client = PublicSocketClient::with_negotiated_capabilities(
                FakePublicSocket::with_responses(vec![list_response(1)]),
                unsafe_shell_capabilities(),
            );
            let listed = boundary()
                .execute_with_client(
                    client,
                    PlannedAction::ListSessions {
                        target: unsafe_target(canonical),
                    },
                )
                .await
                .unwrap();
            let D2bActionOutcome::Listed { client, .. } = listed else {
                panic!("expected list");
            };
            assert_eq!(
                client.into_inner().written_json_frames()[0]["args"]["vm"],
                canonical
            );

            for (name, force) in [("fresh-panda", false), ("quiet-otter", true)] {
                let client = PublicSocketClient::with_negotiated_capabilities(
                    FakePublicSocket::with_responses(vec![attach_response(1, name)]),
                    unsafe_shell_capabilities(),
                );
                let attached = boundary()
                    .execute_with_client(
                        client,
                        PlannedAction::AttachShell {
                            target: unsafe_target(canonical),
                            name: Some(shell(name)),
                            force,
                        },
                    )
                    .await
                    .unwrap();
                let D2bActionOutcome::Attached { attached, .. } = attached else {
                    panic!("expected attach");
                };
                let frame = attached.into_inner().into_inner().written_json_frames();
                assert_eq!(frame[0]["args"]["vm"], canonical);
                assert_eq!(frame[0]["args"]["force"], force);
            }

            let detach_response = PublicResponse::Shell {
                op_id: Some(1),
                response: ShellOpResponse::Detach(d2b_toolkit_core::ShellDetachResult {
                    resolved_name: ShellName::new("quiet-otter").unwrap(),
                    detached: true,
                    cause: None,
                }),
            };
            let client = PublicSocketClient::with_negotiated_capabilities(
                FakePublicSocket::with_responses(vec![detach_response]),
                unsafe_shell_capabilities(),
            );
            let detached = boundary()
                .execute_with_client(
                    client,
                    PlannedAction::DetachShell {
                        target: unsafe_target(canonical),
                        name: shell("quiet-otter"),
                    },
                )
                .await
                .unwrap();
            let D2bActionOutcome::Detached { client, .. } = detached else {
                panic!("expected detach");
            };
            assert_eq!(
                client.into_inner().written_json_frames()[0]["args"]["vm"],
                canonical
            );

            let client = PublicSocketClient::with_negotiated_capabilities(
                FakePublicSocket::with_responses(vec![kill_response(1, "quiet-otter")]),
                unsafe_shell_capabilities(),
            );
            let killed = boundary()
                .execute_with_client(
                    client,
                    PlannedAction::KillShell {
                        target: unsafe_target(canonical),
                        name: shell("quiet-otter"),
                    },
                )
                .await
                .unwrap();
            let D2bActionOutcome::Killed { client, .. } = killed else {
                panic!("expected kill");
            };
            assert_eq!(
                client.into_inner().written_json_frames()[0]["args"]["vm"],
                canonical
            );
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
                        target: target("work"),
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
            assert_eq!(frames[0]["op"], "attach");
            assert_eq!(frames[0]["args"]["force"], true);
            assert_eq!(frames[0]["args"]["name"], "quiet-otter");
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
                        target: target("work"),
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
            assert_eq!(frames[0]["op"], "attach");
            assert_eq!(frames[0]["args"]["force"], false);
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
                        target: target("work"),
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
                        target: target("work"),
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
            assert_eq!(frames[0]["op"], "kill");
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
                        resolved_name: ShellName::new("quiet-otter").unwrap(),
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
                        target: target("work"),
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
            assert_eq!(frames[0]["op"], "attach");
            assert_eq!(frames[1]["op"], "closeAttach");
            assert_ne!(frames[1]["op"], "kill");
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
                .execute_with_client(
                    client,
                    PlannedAction::ListSessions {
                        target: target("work"),
                    },
                )
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
                    .execute_with_client(
                        client,
                        PlannedAction::ListSessions {
                            target: target("work"),
                        },
                    )
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
    fn operation_timeout_bounds_a_stalled_async_transport() {
        let boundary = D2bActionBoundary::new(D2bClientConfig {
            operation_timeout_ms: 20,
            ..D2bClientConfig::default()
        });
        let started = std::time::Instant::now();
        let result = boundary.block_on_operation(
            "test",
            ActionTrace::for_label("timeout-test"),
            futures::future::pending::<Result<(), D2bClientError>>(),
        );
        assert!(matches!(
            result.unwrap_err().kind(),
            D2bClientErrorKind::Timeout
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn shell_name_does_not_become_metric_label() {
        let op = kill_shell_op(&target_id("work"), &shell("customer-project-shell"));

        assert_eq!(op.metrics_label_value(), "kill");
        if let ShellOp::Kill(args) = op {
            assert_eq!(args.name.metrics_label_value(), "shell");
        }
    }
}
