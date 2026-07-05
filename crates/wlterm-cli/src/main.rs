use std::env;
use std::process::ExitCode;

use wlterm_core::friendly_name::FriendlyName;
use wlterm_core::{AsyncErrorDisplay, Config, OpenBehavior, SessionId};
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
        Some("open") => {
            let session = SessionId::new(args.get(1).map(String::as_str).unwrap_or("default"))
                .map_err(|_| "session id must not be empty".to_string())?;
            Ok(render_open_decision(decide_open(
                &session,
                true,
                OpenBehavior::FocusExisting,
            )))
        }
        Some("stop") => {
            let session = SessionId::new(args.get(1).map(String::as_str).unwrap_or("default"))
                .map_err(|_| "session id must not be empty".to_string())?;
            Ok(render_stop_request(&StopRequest::new(&session, true)))
        }
        Some("config") => Ok(default_config_toml()),
        Some("async-error") => Ok(render_async_error(&AsyncErrorEvent::new(
            "example async error",
            AsyncErrorDisplay::Notification,
        ))),
        Some(other) => Err(format!("unknown command '{other}'\n\n{}", help())),
    }
}

fn help() -> String {
    "d2b-wlterm skeleton\n\nCommands:\n  name [seed]\n  waybar\n  open [session]\n  stop [session]\n  config\n  async-error".to_string()
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
    format!("async-error render={}", event.should_render())
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
}
