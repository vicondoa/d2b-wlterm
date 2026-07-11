use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Deserialize;
use wlterm_core::friendly_name::FriendlyName;
use wlterm_core::{
    AsyncErrorDisplay, Config, Model, ModelEvent, OpenBehavior, PlannedAction, ProviderKind,
    SessionId, ShellTarget, TargetId, WorkloadSummary,
};
use wlterm_d2b::{D2bActionBoundary, D2bActionOutcome, D2bClientConfig, D2bClientErrorKind};
use wlterm_ui::{
    decide_open, AlreadyAttachedNotice, AsyncErrorEvent, ControlCenterState, OpenDecision,
    RenderedAsyncError, ShellNamePrompt, StopRequest,
};
use wlterm_waybar::WaybarStatus;

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("d2b-wlterm: {err}");
            ExitCode::from(2)
        }
    }
}

fn terminal_proxy_provider(workload: &WorkloadSummary) -> ProviderKind {
    if workload.provider_kind.is_unsafe_local() {
        // The proxied child is the trusted terminal client, not the unsafe-local
        // workload process. The shell itself remains daemon/helper-routed.
        ProviderKind::LocalVm
    } else {
        workload.provider_kind
    }
}

fn run(args: Vec<String>) -> Result<String, String> {
    match args.first().map(String::as_str) {
        None | Some("--help") | Some("-h") => Ok(help()),
        Some("--version") | Some("-V") => Ok(env!("CARGO_PKG_VERSION").to_string()),
        Some("name") => {
            if let Some(candidate) = args.get(1) {
                Ok(FriendlyName::from_candidate(candidate)
                    .map_err(|_| "friendly name must satisfy d2b shell-name grammar".to_string())?
                    .as_str()
                    .to_string())
            } else {
                Ok(FriendlyName::generate()
                    .map_err(|_| "unable to allocate a friendly name".to_string())?
                    .as_str()
                    .to_string())
            }
        }
        Some("waybar") => {
            let cfg = load_config();
            Ok(WaybarStatus::from_model(&live_model(&cfg)).to_json())
        }
        Some("state") | Some("status-json") => {
            let cfg = load_config();
            Ok(ControlCenterState::from_model(&live_model(&cfg)).to_json())
        }
        Some("control-center") | Some("quickshell") => {
            let cfg = load_config();
            wlterm_ui::open(&cfg).map_err(|err| err.to_string())?;
            Ok(String::new())
        }
        Some("prompt-name") => {
            Ok(ShellNamePrompt::new(args.get(1).map_or("", String::as_str)).to_json())
        }
        Some("already-attached") => render_already_attached(args.get(1)),
        Some("list") => run_list(args.get(1)),
        Some("create") => run_create(args.get(1), args.get(2)),
        Some("open") if args.get(1).is_some() => run_open(
            args.get(1),
            args.get(2),
            args.iter().any(|arg| arg == "--force"),
        ),
        Some("open") => Ok(render_open_decision(decide_open(
            &SessionId::new("default").map_err(|_| "session id must not be empty".to_string())?,
            true,
            OpenBehavior::FocusExisting,
        ))),
        Some("detach") if args.get(1).is_some() => run_detach(args.get(1), args.get(2)),
        Some("stop") if args.get(1).is_some() => run_stop(
            args.get(1),
            args.get(2),
            args.iter().any(|arg| arg == "--confirm"),
        ),
        Some("stop") => Ok(render_stop_request(&StopRequest::new(
            &SessionId::new("default").map_err(|_| "session id must not be empty".to_string())?,
            true,
        ))),
        Some("config") => Ok(default_config_toml()),
        Some("async-error") => Ok(render_async_error(&AsyncErrorEvent::new(
            "example async error",
            AsyncErrorDisplay::Notification,
        ))),
        Some(other) => Err(format!("unknown command '{other}'\n\n{}", help())),
    }
}

fn help() -> String {
    "d2b-wlterm\n\nCommands:\n  name [seed]\n  waybar\n  state|status-json\n  control-center|quickshell\n  prompt-name [shell]\n  already-attached [focus-existing|prompt|force-open]\n  list <target>\n  create <target> [shell]\n  open <target> <shell> [--force]\n  detach <target> <shell>\n  stop <target> <shell> --confirm\n  config\n  async-error".to_string()
}

fn load_config() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str::<Config>(&text).ok())
        .unwrap_or_default()
}

fn config_path() -> Option<std::path::PathBuf> {
    if let Some(path) = env::var_os("D2B_WLTERM_CONFIG") {
        return Some(path.into());
    }
    if let Some(base) = env::var_os("XDG_CONFIG_HOME") {
        return Some(std::path::PathBuf::from(base).join("d2b-wlterm/config.toml"));
    }
    env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .map(|home| home.join(".config/d2b-wlterm/config.toml"))
}

fn live_model(config: &Config) -> Model {
    let mut model = Model::new(config.clone());
    if env::var_os("D2B_WLTERM_TEST_IDLE").is_some() {
        return model;
    }
    let boundary = D2bActionBoundary::new(D2bClientConfig {
        public_socket_path: config.public_socket_path.clone(),
        ..Default::default()
    });
    match boundary.inventory_blocking() {
        Ok(workloads) => model.apply(ModelEvent::WorkloadSnapshot { workloads }),
        Err(error) => {
            if matches!(error.kind(), D2bClientErrorKind::FeatureUnavailable(_)) {
                model.apply(ModelEvent::GlobalRemediation {
                    remediation: wlterm_core::TargetRemediation {
                        kind: wlterm_core::RemediationKind::UpdateD2b,
                        message:
                            "Update d2b, d2bd, d2b-toolkit, and d2b-unsafe-local-helper together",
                    },
                });
            }
            model.apply(ModelEvent::AsyncError {
                message: error.to_string(),
            });
        }
    }
    model
}

fn run_list(target: Option<&String>) -> Result<String, String> {
    let config = load_config();
    let boundary = boundary_for(&config);
    let workload = resolve_shell_workload(&boundary, target)?;
    ensure_workload_available(&workload)?;
    let outcome = boundary
        .execute_blocking(PlannedAction::ListSessions {
            target: workload.shell_target(),
        })
        .map_err(|err| err.to_string())?;
    let D2bActionOutcome::Listed { sessions, .. } = outcome else {
        return Err("unexpected list result".to_string());
    };
    Ok(sessions
        .iter()
        .map(|session| {
            format!(
                "{}\t{}{}",
                session.name.as_str(),
                session.visual_state.metrics_label_value(),
                if session.is_default { "\tdefault" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

fn run_create(target: Option<&String>, name: Option<&String>) -> Result<String, String> {
    let config = load_config();
    let boundary = boundary_for(&config);
    let workload = resolve_shell_workload(&boundary, target)?;
    ensure_workload_available(&workload)?;
    let name = match name {
        Some(name) => parse_shell_name(Some(name))?,
        None => FriendlyName::generate().map_err(|_| "unable to allocate a friendly name")?,
    };
    let resolved = ensure_shell(&boundary, workload.shell_target(), &name, false)?;
    spawn_terminal(&config, &workload, &resolved)?;
    Ok(format!("opened={}", resolved.as_str()))
}

fn run_open(target: Option<&String>, name: Option<&String>, force: bool) -> Result<String, String> {
    let config = load_config();
    let boundary = boundary_for(&config);
    let workload = resolve_shell_workload(&boundary, target)?;
    ensure_workload_available(&workload)?;
    let name = parse_shell_name(name)?;
    let resolved = ensure_shell(&boundary, workload.shell_target(), &name, force)?;
    spawn_terminal(&config, &workload, &resolved)?;
    Ok(format!("opened={}", resolved.as_str()))
}

fn run_stop(
    target: Option<&String>,
    name: Option<&String>,
    confirmed: bool,
) -> Result<String, String> {
    if !confirmed {
        return Err("stop requires --confirm".to_string());
    }
    let config = load_config();
    let boundary = boundary_for(&config);
    let workload = resolve_shell_workload(&boundary, target)?;
    ensure_workload_available(&workload)?;
    let name = parse_shell_name(name)?;

    let action = PlannedAction::KillShell {
        target: workload.shell_target(),
        name,
    };
    let outcome = boundary
        .execute_blocking(action)
        .map_err(|err| err.to_string())?;
    match outcome {
        D2bActionOutcome::Killed { result, .. } => Ok(format!("killed={}", result.killed)),
        _ => Err("unexpected stop result".to_string()),
    }
}

fn run_detach(target: Option<&String>, name: Option<&String>) -> Result<String, String> {
    let config = load_config();
    let boundary = boundary_for(&config);
    let workload = resolve_shell_workload(&boundary, target)?;
    ensure_workload_available(&workload)?;
    let name = parse_shell_name(name)?;
    let outcome = boundary
        .execute_blocking(PlannedAction::DetachShell {
            target: workload.shell_target(),
            name,
        })
        .map_err(|err| err.to_string())?;
    match outcome {
        D2bActionOutcome::Detached { result, .. } => Ok(format!("detached={}", result.detached)),
        _ => Err("unexpected detach result".to_string()),
    }
}

fn ensure_shell(
    boundary: &D2bActionBoundary,
    target: ShellTarget,
    name: &FriendlyName,
    force: bool,
) -> Result<FriendlyName, String> {
    let action = PlannedAction::AttachShell {
        target,
        name: Some(name.clone()),
        force,
    };
    let outcome = boundary
        .execute_blocking(action)
        .map_err(|err| err.to_string())?;
    let D2bActionOutcome::Attached {
        attached,
        resolved_name,
        ..
    } = outcome
    else {
        return Err("unexpected open result".to_string());
    };
    futures::executor::block_on(attached.close_attach()).map_err(|err| err.to_string())?;
    Ok(resolved_name)
}

const PROXY_READY_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_READINESS_EVENT_BYTES: usize = 4096;
static READINESS_SOCKET_SEQUENCE: AtomicU64 = AtomicU64::new(1);

struct ProxyReadinessSocket {
    listener: UnixListener,
    path: PathBuf,
}

impl ProxyReadinessSocket {
    fn bind() -> Result<Self, String> {
        let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| {
                "XDG_RUNTIME_DIR is required for proxied terminal windows".to_string()
            })?;
        let directory = runtime_dir.join("d2b-wlterm").join("proxy-readiness");
        ensure_private_directory(&directory)?;
        let sequence = READINESS_SOCKET_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!("{}-{sequence}.sock", std::process::id()));
        let listener = UnixListener::bind(&path)
            .map_err(|_| "failed to create d2b-wayland-proxy readiness socket".to_string())?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|_| "failed to secure d2b-wayland-proxy readiness socket".to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "failed to configure d2b-wayland-proxy readiness socket".to_string())?;
        Ok(Self { listener, path })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn wait_until_ready(
        &self,
        child: &mut Child,
        workload: &WorkloadSummary,
    ) -> Result<(), String> {
        let deadline = Instant::now() + PROXY_READY_TIMEOUT;
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    return wait_for_proxy_events(stream, deadline, workload);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if child
                        .try_wait()
                        .map_err(|_| "failed to inspect d2b-wayland-proxy".to_string())?
                        .is_some()
                    {
                        return Err(
                            "d2b-wayland-proxy exited before terminal readiness; no direct fallback"
                                .to_string(),
                        );
                    }
                    if Instant::now() >= deadline {
                        return Err(
                            "d2b-wayland-proxy readiness timed out; no direct fallback".to_string()
                        );
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => {
                    return Err(
                        "d2b-wayland-proxy readiness channel failed; no direct fallback"
                            .to_string(),
                    );
                }
            }
        }
    }
}

impl Drop for ProxyReadinessSocket {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn ensure_private_directory(path: &Path) -> Result<(), String> {
    if path.exists() {
        let metadata = fs::symlink_metadata(path)
            .map_err(|_| "failed to inspect proxy readiness directory".to_string())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("proxy readiness directory is not a private directory".to_string());
        }
    } else {
        fs::create_dir_all(path)
            .map_err(|_| "failed to create proxy readiness directory".to_string())?;
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|_| "failed to secure proxy readiness directory".to_string())
}

fn wait_for_proxy_events(
    stream: UnixStream,
    deadline: Instant,
    workload: &WorkloadSummary,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(deadline.saturating_duration_since(Instant::now())))
        .map_err(|_| "failed to configure proxy readiness deadline".to_string())?;
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).map_err(|_| {
            "d2b-wayland-proxy readiness channel failed; no direct fallback".to_string()
        })?;
        if read == 0 {
            return Err(
                "d2b-wayland-proxy closed before terminal readiness; no direct fallback"
                    .to_string(),
            );
        }
        if read > MAX_READINESS_EVENT_BYTES {
            return Err("d2b-wayland-proxy readiness event exceeded its bound".to_string());
        }
        let event: ProxyReadinessEvent = serde_json::from_str(line.trim_end())
            .map_err(|_| "d2b-wayland-proxy sent an invalid readiness event".to_string())?;
        event.validate_for(workload)?;
        match (event.state, event.stage) {
            (ProxyReadinessState::Ready, ProxyReadinessStage::FirstClient) => return Ok(()),
            (ProxyReadinessState::Failed, stage) => {
                return Err(format!(
                    "d2b-wayland-proxy failed at {} ({}); no direct fallback",
                    stage.label(),
                    event
                        .failure
                        .map(ProxyReadinessFailure::label)
                        .unwrap_or("unknown")
                ));
            }
            (ProxyReadinessState::Ready, _) => {}
        }
        if Instant::now() >= deadline {
            return Err("d2b-wayland-proxy readiness timed out; no direct fallback".to_string());
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum ProxyReadinessStage {
    Upstream,
    Listener,
    FirstClient,
}

impl ProxyReadinessStage {
    const fn label(self) -> &'static str {
        match self {
            Self::Upstream => "upstream",
            Self::Listener => "listener",
            Self::FirstClient => "first-client",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum ProxyReadinessState {
    Ready,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum ProxyReadinessFailure {
    UpstreamUnavailable,
    ListenerUnavailable,
    FirstClientTimeout,
    ClientRejected,
    ChannelUnavailable,
}

impl ProxyReadinessFailure {
    const fn label(self) -> &'static str {
        match self {
            Self::UpstreamUnavailable => "upstream-unavailable",
            Self::ListenerUnavailable => "listener-unavailable",
            Self::FirstClientTimeout => "first-client-timeout",
            Self::ClientRejected => "client-rejected",
            Self::ChannelUnavailable => "channel-unavailable",
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProxyReadinessEvent {
    protocol_version: u16,
    target: String,
    provider_kind: ProviderKind,
    stage: ProxyReadinessStage,
    state: ProxyReadinessState,
    #[serde(default)]
    failure: Option<ProxyReadinessFailure>,
}

impl ProxyReadinessEvent {
    fn validate_for(&self, workload: &WorkloadSummary) -> Result<(), String> {
        if self.protocol_version != 1
            || self.target != workload.target.as_str()
            || self.provider_kind != terminal_proxy_provider(workload)
            || (self.state == ProxyReadinessState::Ready && self.failure.is_some())
            || (self.state == ProxyReadinessState::Failed && self.failure.is_none())
        {
            return Err("d2b-wayland-proxy readiness identity mismatch".to_string());
        }
        Ok(())
    }
}

fn boundary_for(config: &Config) -> D2bActionBoundary {
    D2bActionBoundary::new(D2bClientConfig {
        public_socket_path: config.public_socket_path.clone(),
        ..D2bClientConfig::default()
    })
}

fn resolve_shell_workload(
    boundary: &D2bActionBoundary,
    raw_target: Option<&String>,
) -> Result<WorkloadSummary, String> {
    let requested = parse_target(raw_target)?;
    let workloads = boundary
        .discover_blocking()
        .map_err(|err| err.to_string())?;
    if requested.is_canonical() {
        return workloads
            .into_iter()
            .find(|workload| workload.target == requested)
            .ok_or_else(|| "target is not an advertised persistent-shell workload".to_string());
    }
    let mut matches = workloads.into_iter().filter(|workload| {
        workload.id == requested
            || workload
                .legacy_vm_name
                .as_deref()
                .is_some_and(|legacy| legacy == requested.as_str())
    });
    let Some(workload) = matches.next() else {
        return Err("target is not an advertised persistent-shell workload".to_string());
    };
    if matches.next().is_some() {
        return Err("target alias is ambiguous; use a canonical workload target".to_string());
    }
    Ok(workload)
}

fn ensure_workload_available(workload: &WorkloadSummary) -> Result<(), String> {
    if workload.actions_available() {
        return Ok(());
    }
    let reason = if !workload.shell_feature_available {
        "unsafe-local-shell-v1 is unavailable".to_string()
    } else if !workload.availability.is_ready() {
        format!(
            "provider is {}",
            workload.availability.metrics_label_value()
        )
    } else {
        "target is not running".to_string()
    };
    let remediation = workload
        .remediation()
        .map(|value| format!("; remediation: {}", value.message))
        .unwrap_or_default();
    Err(format!("{reason}{remediation}"))
}

fn spawn_terminal(
    config: &Config,
    workload: &WorkloadSummary,
    shell: &FriendlyName,
) -> Result<(), String> {
    let readiness = ProxyReadinessSocket::bind()?;
    let terminal_command = render_terminal_command(config, workload, shell);
    let Some((terminal_program, terminal_args)) = terminal_command.split_first() else {
        return Err("wezterm_command must not be empty".to_string());
    };
    let proxy_command = render_proxy_command(
        config,
        workload,
        terminal_program,
        terminal_args,
        readiness.path(),
    );
    let Some((program, args)) = proxy_command.split_first() else {
        return Err("wayland_proxy_command must not be empty".to_string());
    };
    let mut proxy = Command::new(program)
        .args(args)
        .env("WEEZTERM_D2B_SHELL_NAME", shell.as_str())
        .env("WEEZTERM_D2B_BOUND_TARGET", workload.target.as_str())
        .env(
            "WEEZTERM_D2B_BOUND_VM",
            workload
                .legacy_vm_name
                .as_deref()
                .unwrap_or(workload.target.as_str()),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "failed to launch d2b-wayland-proxy".to_string())?;
    if let Err(error) = readiness.wait_until_ready(&mut proxy, workload) {
        let _ = proxy.kill();
        let _ = proxy.wait();
        return Err(error);
    }
    Ok(())
}

fn render_proxy_command(
    config: &Config,
    workload: &WorkloadSummary,
    terminal_program: &str,
    terminal_args: &[String],
    readiness_path: &Path,
) -> Vec<String> {
    let mut command = config.wayland_proxy_command.clone();
    command.extend([
        "--host-terminal".to_string(),
        "--target".to_string(),
        workload.target.as_str().to_string(),
        "--provider-kind".to_string(),
        terminal_proxy_provider(workload)
            .metrics_label_value()
            .to_string(),
        "--border-enable".to_string(),
        "--border-label".to_string(),
        proxy_label(workload),
        "--readiness-socket".to_string(),
        readiness_path.display().to_string(),
        "--first-client-timeout-ms".to_string(),
        PROXY_READY_TIMEOUT.as_millis().to_string(),
    ]);
    if let Some(legacy) = &workload.legacy_vm_name {
        command.extend(["--vm-name".to_string(), legacy.clone()]);
    }
    if workload.provider_kind.is_unsafe_local() {
        command.extend([
            "--app-id-prefix".to_string(),
            format!("d2b.unsafe-local.{}.", workload.target.as_str()),
            "--title-prefix".to_string(),
            format!("[unsafe-local {}] ", workload.target.as_str()),
        ]);
    }
    if let Some(colors) = target_border_colors(workload) {
        command.extend([
            "--border-color-active".to_string(),
            colors.active,
            "--border-color-inactive".to_string(),
            colors.inactive,
            "--border-color-urgent".to_string(),
            colors.urgent,
        ]);
    }
    command.extend([
        "--terminal-program".to_string(),
        terminal_program.to_string(),
        "--".to_string(),
    ]);
    command.extend(terminal_args.iter().cloned());
    command
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BorderColors {
    active: String,
    inactive: String,
    urgent: String,
}

fn target_border_colors(workload: &WorkloadSummary) -> Option<BorderColors> {
    let text = fs::read_to_string("/etc/d2b/ui-colors.json").ok()?;
    let root: serde_json::Value = serde_json::from_str(&text).ok()?;
    let legacy = workload
        .legacy_vm_name
        .as_deref()
        .unwrap_or(workload.id.as_str());
    let border = root
        .get("vms")
        .and_then(|vms| vms.get(legacy))
        .and_then(|vm| vm.get("border"));
    let realm = wlterm_core::realm_from_canonical_target(workload.target.as_str());
    let realm_accent = realm.and_then(|realm| {
        root.get("realms")
            .and_then(|realms| realms.get(realm))
            .or_else(|| root.get("envs").and_then(|envs| envs.get(realm)))
            .and_then(|value| value.get("accent"))
            .and_then(color_string)
    });
    let active = border
        .and_then(|border| border.get("active"))
        .and_then(color_string)
        .or(realm_accent)?;
    let inactive = border
        .and_then(|border| border.get("inactive"))
        .and_then(color_string)
        .unwrap_or_else(|| active.clone());
    let urgent = border
        .and_then(|border| border.get("urgent"))
        .and_then(color_string)
        .unwrap_or_else(|| active.clone());
    Some(BorderColors {
        active,
        inactive,
        urgent,
    })
}

fn color_string(value: &serde_json::Value) -> Option<String> {
    let color = value.as_str()?;
    let bytes = color.as_bytes();
    let valid = bytes.len() == 7
        && bytes[0] == b'#'
        && bytes[1..].iter().all(|byte| byte.is_ascii_hexdigit());
    valid.then(|| color.to_string())
}

fn proxy_label(workload: &WorkloadSummary) -> String {
    if workload.provider_kind.is_unsafe_local() {
        format!("{} · unsafe-local · NO ISOLATION", workload.target.as_str())
    } else {
        workload.target.as_str().to_string()
    }
}

fn render_terminal_command(
    config: &Config,
    workload: &WorkloadSummary,
    shell: &FriendlyName,
) -> Vec<String> {
    let domain = format!("d2b-{}", workload.target.as_str());
    let legacy = workload
        .legacy_vm_name
        .as_deref()
        .unwrap_or(workload.target.as_str());
    let mut command: Vec<String> = config
        .wezterm_command
        .iter()
        .map(|part| {
            part.replace("{target}", workload.target.as_str())
                .replace("{vm}", legacy)
                .replace("{shell}", shell.as_str())
                .replace("{domain}", &domain)
        })
        .collect();
    ensure_close_confirmation(&mut command);
    ensure_weezterm_domain(&mut command, &domain);
    command
}

fn ensure_close_confirmation(command: &mut Vec<String>) {
    if command
        .iter()
        .any(|part| part.contains("window_close_confirmation"))
    {
        return;
    }
    let insert_at = command
        .iter()
        .position(|part| part == "start" || part == "-e")
        .unwrap_or(command.len());
    command.splice(
        insert_at..insert_at,
        [
            "--config".to_string(),
            "window_close_confirmation=\"NeverPrompt\"".to_string(),
        ],
    );
}

fn ensure_weezterm_domain(command: &mut Vec<String>, domain: &str) {
    if command.iter().any(|part| part == "--domain") {
        return;
    }
    if let Some(index) = command.iter().position(|part| part == "--") {
        command.splice(index..index, ["--domain".to_string(), domain.to_string()]);
    } else {
        command.extend(["--domain".to_string(), domain.to_string()]);
    }
}

fn parse_target(value: Option<&String>) -> Result<TargetId, String> {
    TargetId::new(value.ok_or_else(|| "target is required".to_string())?)
        .map_err(|_| "target must be a canonical d2b target or legacy VM name".to_string())
}

fn parse_shell_name(value: Option<&String>) -> Result<FriendlyName, String> {
    FriendlyName::from_candidate(value.ok_or_else(|| "shell name is required".to_string())?)
        .map_err(|_| "friendly name must satisfy d2b shell-name grammar".to_string())
}

fn default_config_toml() -> String {
    let cfg = Config::default();
    format!(
        "public_socket_path = \"{}\"\nwezterm_command = [{}]\nwayland_proxy_command = [{}]\nrefresh_interval_seconds = {}\n\n[ui]\ndefault_open_behavior = \"focus-existing\"\nstop_confirmation = {}\nasync_error_display = \"notification\"\n\n[waybar]\nenable = {}\nmodule_name = \"{}\"\n\n[quickshell]\nenable = false\ncontrol_center_state_path = \"$XDG_RUNTIME_DIR/d2b-wlterm/control-center.json\"",
        cfg.public_socket_path,
        cfg.wezterm_command
            .iter()
            .map(|part| format!("\"{}\"", part.replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(", "),
        cfg.wayland_proxy_command
            .iter()
            .map(|part| format!("\"{}\"", part.replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(", "),
        cfg.refresh_interval_seconds,
        cfg.ui.stop_confirmation,
        cfg.waybar.enable,
        cfg.waybar.module_name.replace('"', "\\\""),
    )
}

fn render_open_decision(decision: OpenDecision) -> String {
    match decision {
        OpenDecision::OpenNew { .. } => "open-new".to_string(),
        OpenDecision::FocusExisting { .. } => "focus-existing".to_string(),
        OpenDecision::Prompt { .. } => "prompt".to_string(),
        OpenDecision::ForceOpen { .. } => "force-open".to_string(),
    }
}

fn render_stop_request(request: &StopRequest) -> String {
    format!(
        "stop requires_confirmation={}",
        request.requires_confirmation
    )
}

fn render_async_error(event: &AsyncErrorEvent) -> String {
    match RenderedAsyncError::from_event(event) {
        Some(rendered) => rendered.detail,
        None => "async-error render=false".to_string(),
    }
}

fn render_already_attached(value: Option<&String>) -> Result<String, String> {
    let behavior = match value.map(String::as_str).unwrap_or("focus-existing") {
        "focus-existing" => OpenBehavior::FocusExisting,
        "prompt" => OpenBehavior::Prompt,
        "force-open" => OpenBehavior::ForceOpen,
        other => return Err(format!("unknown already-attached behavior '{other}'")),
    };
    let notice = AlreadyAttachedNotice::for_behavior("default", behavior);
    Ok(format!(
        "{} allow_force_open={}",
        notice.mode, notice.allow_force_open
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn name_command_validates_candidate() {
        assert_eq!(
            run(vec!["name".into(), "Quiet-Otter".into()]).unwrap(),
            "quiet-otter"
        );
        assert!(run(vec!["name".into(), "bad/name".into()]).is_err());
    }

    #[test]
    fn waybar_command_outputs_json() {
        env::set_var("D2B_WLTERM_TEST_IDLE", "1");
        assert_eq!(
            run(vec!["waybar".into()]).unwrap(),
            WaybarStatus::idle().to_json()
        );
        env::remove_var("D2B_WLTERM_TEST_IDLE");
    }

    #[test]
    fn discovery_has_no_cli_subprocess_fallback() {
        let source = include_str!("main.rs");
        assert!(!source.contains("Command::new(\"d2b\")"));
        assert!(!source.contains("[\"list\", \"--json\"]"));
        assert!(!source.contains("[\"vm\", \"list\"]"));
    }

    fn workload() -> WorkloadSummary {
        let mut workload = WorkloadSummary::discovered(
            TargetId::new("dev-general.dev.d2b").unwrap(),
            TargetId::new("dev-general").unwrap(),
            wlterm_core::TargetPowerState::Online,
        );
        workload.legacy_vm_name = Some("dev-general".to_string());
        workload
    }

    fn unsafe_workload() -> WorkloadSummary {
        let mut workload = WorkloadSummary::discovered(
            TargetId::new("tools.host.d2b").unwrap(),
            TargetId::new("tools").unwrap(),
            wlterm_core::TargetPowerState::Online,
        );
        workload.provider_kind = ProviderKind::UnsafeLocal;
        workload.isolation_posture = wlterm_core::IsolationPosture::UnsafeLocal;
        workload
    }

    #[test]
    fn proxy_command_requires_typed_readiness_and_canonical_identity() {
        let cfg = Config::default();
        let command = render_proxy_command(
            &cfg,
            &unsafe_workload(),
            "wezterm",
            &["start".to_string()],
            Path::new("/run/user/1000/d2b-wlterm/proxy-readiness/test.sock"),
        );
        assert_eq!(command[0], "d2b-wayland-proxy");
        assert!(command
            .windows(2)
            .any(|pair| pair == ["--target", "tools.host.d2b"]));
        assert!(command
            .windows(2)
            .any(|pair| pair == ["--provider-kind", "local-vm"]));
        assert!(command
            .windows(2)
            .any(|pair| pair[0] == "--readiness-socket"));
        assert!(command.iter().any(|arg| arg == "--first-client-timeout-ms"));
        assert!(command.iter().any(|arg| arg.contains("NO ISOLATION")));
        assert!(command
            .iter()
            .any(|arg| arg == "[unsafe-local tools.host.d2b] "));
    }

    #[test]
    fn proxy_failure_is_typed_and_has_no_direct_terminal_fallback() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let producer = std::thread::spawn(move || {
            writer
                .write_all(
                    br#"{"protocolVersion":1,"target":"tools.host.d2b","providerKind":"local-vm","stage":"upstream","state":"failed","failure":"upstream-unavailable"}
"#,
                )
                .unwrap();
        });
        let error = wait_for_proxy_events(
            reader,
            Instant::now() + Duration::from_secs(1),
            &unsafe_workload(),
        )
        .unwrap_err();
        producer.join().unwrap();

        assert!(error.contains("upstream-unavailable"));
        assert!(error.contains("no direct fallback"));
        assert!(!error.contains("wezterm"));
        assert!(!error.contains("terminal bytes"));
    }

    #[test]
    fn proxy_first_client_readiness_allows_window_open() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let producer = std::thread::spawn(move || {
            for event in [
                r#"{"protocolVersion":1,"target":"dev-general.dev.d2b","providerKind":"local-vm","stage":"upstream","state":"ready"}"#,
                r#"{"protocolVersion":1,"target":"dev-general.dev.d2b","providerKind":"local-vm","stage":"listener","state":"ready"}"#,
                r#"{"protocolVersion":1,"target":"dev-general.dev.d2b","providerKind":"local-vm","stage":"first-client","state":"ready"}"#,
            ] {
                writer.write_all(event.as_bytes()).unwrap();
                writer.write_all(b"\n").unwrap();
            }
        });
        wait_for_proxy_events(reader, Instant::now() + Duration::from_secs(1), &workload())
            .unwrap();
        producer.join().unwrap();
    }

    #[test]
    fn terminal_command_inserts_domain_before_separator() {
        let cfg = Config {
            wezterm_command: vec!["weezterm".into(), "start".into(), "--".into()],
            ..Default::default()
        };
        let shell = FriendlyName::from_candidate("quiet-otter").unwrap();

        assert_eq!(
            render_terminal_command(&cfg, &workload(), &shell),
            vec![
                "weezterm",
                "--config",
                "window_close_confirmation=\"NeverPrompt\"",
                "start",
                "--domain",
                "d2b-dev-general.dev.d2b",
                "--",
            ]
        );
    }

    #[test]
    fn terminal_command_keeps_explicit_domain() {
        let cfg = Config {
            wezterm_command: vec![
                "weezterm".into(),
                "start".into(),
                "--domain".into(),
                "{domain}".into(),
                "--".into(),
            ],
            ..Default::default()
        };
        let shell = FriendlyName::from_candidate("quiet-otter").unwrap();

        assert_eq!(
            render_terminal_command(&cfg, &workload(), &shell),
            vec![
                "weezterm",
                "--config",
                "window_close_confirmation=\"NeverPrompt\"",
                "start",
                "--domain",
                "d2b-dev-general.dev.d2b",
                "--",
            ]
        );
    }

    #[test]
    fn terminal_command_keeps_explicit_close_confirmation() {
        let cfg = Config {
            wezterm_command: vec![
                "weezterm".into(),
                "--config".into(),
                "window_close_confirmation=\"NeverPrompt\"".into(),
                "start".into(),
                "--".into(),
            ],
            ..Default::default()
        };
        let shell = FriendlyName::from_candidate("quiet-otter").unwrap();

        assert_eq!(
            render_terminal_command(&cfg, &workload(), &shell),
            vec![
                "weezterm",
                "--config",
                "window_close_confirmation=\"NeverPrompt\"",
                "start",
                "--domain",
                "d2b-dev-general.dev.d2b",
                "--",
            ]
        );
    }

    #[test]
    fn stop_without_confirmation_does_not_dispatch() {
        assert_eq!(
            run(vec!["stop".into(), "work".into(), "quiet-otter".into()]).unwrap_err(),
            "stop requires --confirm"
        );
    }
}
