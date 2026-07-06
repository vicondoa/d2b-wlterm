use std::env;
use std::fs;
use std::process::{Command, ExitCode, Stdio};

use wlterm_core::friendly_name::FriendlyName;
use wlterm_core::{
    AsyncErrorDisplay, Config, Model, ModelEvent, OpenBehavior, PlannedAction, SessionId, VmId,
    VmPowerState, VmSummary,
};
use wlterm_d2b::{D2bActionBoundary, D2bActionOutcome};
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
    "d2b-wlterm\n\nCommands:\n  name [seed]\n  waybar\n  state|status-json\n  control-center|quickshell\n  prompt-name [shell]\n  already-attached [focus-existing|prompt|force-open]\n  list <vm>\n  create <vm> [shell]\n  open <vm> <shell> [--force]\n  detach <vm> <shell>\n  stop <vm> <shell> --confirm\n  config\n  async-error".to_string()
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
    let boundary = D2bActionBoundary::new(wlterm_d2b::D2bClientConfig {
        public_socket_path: config.public_socket_path.clone(),
        ..Default::default()
    });
    let mut vms = Vec::new();
    for candidate in list_running_vms() {
        let vm = match VmId::new(candidate) {
            Ok(vm) => vm,
            Err(_) => continue,
        };
        let outcome = boundary.execute_blocking(PlannedAction::ListSessions { vm: vm.clone() });
        let Ok(D2bActionOutcome::Listed { sessions, .. }) = outcome else {
            continue;
        };
        let mut summary = VmSummary::new(vm, VmPowerState::Online);
        summary.sessions = sessions;
        vms.push(summary);
    }
    model.apply(ModelEvent::VmSnapshot { vms });
    model
}

fn list_running_vms() -> Vec<String> {
    let output = Command::new("d2b")
        .args(["vm", "list"])
        .stdin(Stdio::null())
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let vm = parts.next()?;
            let state = parts.next().unwrap_or("unknown");
            if state == "running" && !vm.starts_with("sys-") {
                Some(vm.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn run_list(vm: Option<&String>) -> Result<String, String> {
    let vm = parse_vm(vm)?;
    let outcome = D2bActionBoundary::new(Default::default())
        .execute_blocking(PlannedAction::ListSessions { vm })
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

fn run_create(vm: Option<&String>, name: Option<&String>) -> Result<String, String> {
    let vm = parse_vm(vm)?;
    let name = match name {
        Some(name) => parse_shell_name(Some(name))?,
        None => FriendlyName::generate().map_err(|_| "unable to allocate a friendly name")?,
    };
    let resolved = ensure_shell(&vm, &name, false)?;
    spawn_terminal(&load_config(), &vm, &resolved)?;
    Ok(format!("opened={}", resolved.as_str()))
}

fn run_open(vm: Option<&String>, name: Option<&String>, force: bool) -> Result<String, String> {
    let vm = parse_vm(vm)?;
    let name = parse_shell_name(name)?;
    let resolved = ensure_shell(&vm, &name, force)?;
    spawn_terminal(&load_config(), &vm, &resolved)?;
    Ok(format!("opened={}", resolved.as_str()))
}

fn run_stop(vm: Option<&String>, name: Option<&String>, confirmed: bool) -> Result<String, String> {
    let vm = parse_vm(vm)?;
    let name = parse_shell_name(name)?;
    if !confirmed {
        return Err("stop requires --confirm".to_string());
    }

    let action = PlannedAction::KillShell { vm, name };
    let outcome = D2bActionBoundary::new(Default::default())
        .execute_blocking(action)
        .map_err(|err| err.to_string())?;
    match outcome {
        D2bActionOutcome::Killed { result, .. } => Ok(format!("killed={}", result.killed)),
        _ => Err("unexpected stop result".to_string()),
    }
}

fn run_detach(vm: Option<&String>, name: Option<&String>) -> Result<String, String> {
    let vm = parse_vm(vm)?;
    let name = parse_shell_name(name)?;
    let outcome = D2bActionBoundary::new(Default::default())
        .execute_blocking(PlannedAction::DetachShell { vm, name })
        .map_err(|err| err.to_string())?;
    match outcome {
        D2bActionOutcome::Detached { result, .. } => Ok(format!("detached={}", result.detached)),
        _ => Err("unexpected detach result".to_string()),
    }
}

fn ensure_shell(vm: &VmId, name: &FriendlyName, force: bool) -> Result<FriendlyName, String> {
    let action = PlannedAction::AttachShell {
        vm: vm.clone(),
        name: Some(name.clone()),
        force,
    };
    let outcome = D2bActionBoundary::new(Default::default())
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

fn spawn_terminal(config: &Config, vm: &VmId, shell: &FriendlyName) -> Result<(), String> {
    let terminal_command = render_terminal_command(config, vm, shell);
    let Some((terminal_program, terminal_args)) = terminal_command.split_first() else {
        return Err("wezterm_command must not be empty".to_string());
    };
    let proxy_command = render_proxy_command(config, vm, terminal_program, terminal_args);
    let Some((program, args)) = proxy_command.split_first() else {
        return Err("wayland_proxy_command must not be empty".to_string());
    };
    Command::new(program)
        .args(args)
        .env("WEEZTERM_D2B_SHELL_NAME", shell.as_str())
        .env("WEEZTERM_D2B_BOUND_VM", vm.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("failed to launch terminal: {err}"))?;
    Ok(())
}

fn render_proxy_command(
    config: &Config,
    vm: &VmId,
    terminal_program: &str,
    terminal_args: &[String],
) -> Vec<String> {
    let mut command = config.wayland_proxy_command.clone();
    command.extend([
        "--host-terminal".to_string(),
        "--vm-name".to_string(),
        vm.as_str().to_string(),
        "--border-enable".to_string(),
        "--border-label".to_string(),
        vm.as_str().to_string(),
    ]);
    if let Some(colors) = vm_border_colors(vm) {
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

fn vm_border_colors(vm: &VmId) -> Option<BorderColors> {
    let text = fs::read_to_string("/etc/d2b/ui-colors.json").ok()?;
    let root: serde_json::Value = serde_json::from_str(&text).ok()?;
    let border = root.get("vms")?.get(vm.as_str())?.get("border")?;
    let active = color_string(border.get("active")?)?;
    let inactive = border
        .get("inactive")
        .and_then(color_string)
        .unwrap_or_else(|| active.clone());
    let urgent = border
        .get("urgent")
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

fn render_terminal_command(config: &Config, vm: &VmId, shell: &FriendlyName) -> Vec<String> {
    let domain = format!("d2b-{}", vm.as_str());
    let mut command: Vec<String> = config
        .wezterm_command
        .iter()
        .map(|part| {
            part.replace("{vm}", vm.as_str())
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

fn parse_vm(value: Option<&String>) -> Result<VmId, String> {
    VmId::new(value.ok_or_else(|| "vm is required".to_string())?)
        .map_err(|_| "vm id must not be empty".to_string())
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
    fn terminal_command_inserts_domain_before_separator() {
        let mut cfg = Config::default();
        cfg.wezterm_command = vec!["weezterm".into(), "start".into(), "--".into()];
        let vm = VmId::new("dev-general").unwrap();
        let shell = FriendlyName::from_candidate("quiet-otter").unwrap();

        assert_eq!(
            render_terminal_command(&cfg, &vm, &shell),
            vec![
                "weezterm",
                "--config",
                "window_close_confirmation=\"NeverPrompt\"",
                "start",
                "--domain",
                "d2b-dev-general",
                "--",
            ]
        );
    }

    #[test]
    fn terminal_command_keeps_explicit_domain() {
        let mut cfg = Config::default();
        cfg.wezterm_command = vec![
            "weezterm".into(),
            "start".into(),
            "--domain".into(),
            "{domain}".into(),
            "--".into(),
        ];
        let vm = VmId::new("dev-general").unwrap();
        let shell = FriendlyName::from_candidate("quiet-otter").unwrap();

        assert_eq!(
            render_terminal_command(&cfg, &vm, &shell),
            vec![
                "weezterm",
                "--config",
                "window_close_confirmation=\"NeverPrompt\"",
                "start",
                "--domain",
                "d2b-dev-general",
                "--",
            ]
        );
    }

    #[test]
    fn terminal_command_keeps_explicit_close_confirmation() {
        let mut cfg = Config::default();
        cfg.wezterm_command = vec![
            "weezterm".into(),
            "--config".into(),
            "window_close_confirmation=\"NeverPrompt\"".into(),
            "start".into(),
            "--".into(),
        ];
        let vm = VmId::new("dev-general").unwrap();
        let shell = FriendlyName::from_candidate("quiet-otter").unwrap();

        assert_eq!(
            render_terminal_command(&cfg, &vm, &shell),
            vec![
                "weezterm",
                "--config",
                "window_close_confirmation=\"NeverPrompt\"",
                "start",
                "--domain",
                "d2b-dev-general",
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
