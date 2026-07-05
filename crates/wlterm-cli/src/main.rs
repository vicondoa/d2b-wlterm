use std::env;
use std::process::ExitCode;

use wlterm_core::friendly_name::FriendlyName;
use wlterm_core::{AsyncErrorDisplay, Config, OpenBehavior, PlannedAction, SessionId, VmId};
use wlterm_d2b::{D2bActionBoundary, D2bActionOutcome};
use wlterm_ui::{decide_open, AsyncErrorEvent, OpenDecision, StopRequest};
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
        Some("waybar") => Ok(WaybarStatus::idle().to_json()),
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
    "d2b-wlterm\n\nCommands:\n  name [seed]\n  waybar\n  list <vm>\n  create <vm> [shell]\n  open <vm> <shell> [--force]\n  stop <vm> <shell> --confirm\n  config\n  async-error".to_string()
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
    attach_then_disconnect(PlannedAction::AttachShell {
        vm,
        name: Some(name),
        force: false,
    })
}

fn run_open(vm: Option<&String>, name: Option<&String>, force: bool) -> Result<String, String> {
    let vm = parse_vm(vm)?;
    let name = parse_shell_name(name)?;
    attach_then_disconnect(PlannedAction::AttachShell {
        vm,
        name: Some(name),
        force,
    })
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

fn attach_then_disconnect(action: PlannedAction) -> Result<String, String> {
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
    Ok(format!("opened={}", resolved_name.as_str()))
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
        "public_socket_path = \"{}\"\nwezterm_command = [{}]\nrefresh_interval_seconds = {}\n\n[ui]\ndefault_open_behavior = \"focus-existing\"\nstop_confirmation = {}\nasync_error_display = \"notification\"\n\n[waybar]\nenable = {}\nmodule_name = \"{}\"",
        cfg.public_socket_path,
        cfg.wezterm_command
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
    let display = match event.display {
        AsyncErrorDisplay::Inline => "inline",
        AsyncErrorDisplay::Notification => "notification",
        AsyncErrorDisplay::Waybar => "waybar",
        AsyncErrorDisplay::Silent => "silent",
    };
    format!(
        "async-error render={} display={display}",
        event.should_render()
    )
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
        assert_eq!(
            run(vec!["waybar".into()]).unwrap(),
            WaybarStatus::idle().to_json()
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
