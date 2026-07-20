use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use wlterm_core::friendly_name::FriendlyName;
use wlterm_core::{AsyncErrorDisplay, Config, Model, ModelEvent, OpenBehavior, SessionId};
use wlterm_d2b::{D2bActionBoundary, D2bClientConfig};
use wlterm_ui::{
    decide_open, AlreadyAttachedNotice, AsyncErrorEvent, ControlCenterState, OpenDecision,
    RenderedAsyncError, ShellNamePrompt, StopRequest,
};
use wlterm_waybar::WaybarStatus;

const SERVICES_UNAVAILABLE: &str =
    "canonical terminal and desktop services are not available in this source cut";

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("d2b-wlterm: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(args: Vec<String>) -> Result<String, String> {
    match args.first().map(String::as_str) {
        None | Some("--help") | Some("-h") => Ok(help()),
        Some("--version") | Some("-V") => Ok(env!("CARGO_PKG_VERSION").to_owned()),
        Some("name") => friendly_name(args.get(1)),
        Some("waybar") => {
            let config = load_config();
            Ok(WaybarStatus::from_model(&live_model(&config)).to_json())
        }
        Some("state") | Some("status-json") => {
            let config = load_config();
            Ok(ControlCenterState::from_model(&live_model(&config)).to_json())
        }
        Some("control-center") | Some("quickshell") => {
            let config = load_config();
            wlterm_ui::open(&config).map_err(|error| error.to_string())?;
            Ok(String::new())
        }
        Some("render-sample") => {
            let output = args
                .get(1)
                .ok_or_else(|| "render-sample requires an explicit PNG output path".to_owned())?;
            if args.len() != 2 {
                return Err("render-sample accepts exactly one PNG output path".to_owned());
            }
            let artifact = wlterm_ui::render_sample(Path::new(output))?;
            Ok(format!(
                "rendered={} dimensions={}x{} bytes={}",
                artifact.path.display(),
                artifact.width,
                artifact.height,
                artifact.bytes
            ))
        }
        Some("prompt-name") => {
            Ok(ShellNamePrompt::new(args.get(1).map_or("", String::as_str)).to_json())
        }
        Some("already-attached") => render_already_attached(args.get(1)),
        Some("open") if args.get(1).is_none() => Ok(render_open_decision(decide_open(
            &SessionId::new("default").map_err(|_| "session id must not be empty".to_owned())?,
            true,
            OpenBehavior::FocusExisting,
        ))),
        Some("stop") if args.get(1).is_none() => Ok(render_stop_request(&StopRequest::new(
            &SessionId::new("default").map_err(|_| "session id must not be empty".to_owned())?,
            true,
        ))),
        Some("list" | "create" | "open" | "detach" | "stop") => {
            Err(SERVICES_UNAVAILABLE.to_owned())
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
    "d2b-wlterm

Commands:
  name [seed]
  waybar
  state|status-json
  control-center|quickshell
  render-sample <output.png>
  prompt-name [shell]
  already-attached [focus-existing|prompt|force-open]
  list <target>
  create <target> [shell]
  open <target> <shell> [--force]
  detach <target> <shell>
  stop <target> <shell> --confirm
  config
  async-error"
        .to_owned()
}

fn friendly_name(candidate: Option<&String>) -> Result<String, String> {
    match candidate {
        Some(candidate) => FriendlyName::from_candidate(candidate)
            .map(|name| name.as_str().to_owned())
            .map_err(|_| "friendly name must satisfy d2b shell-name grammar".to_owned()),
        None => FriendlyName::generate()
            .map(|name| name.as_str().to_owned())
            .map_err(|_| "unable to allocate a friendly name".to_owned()),
    }
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
            model.apply(ModelEvent::AsyncError {
                message: error.to_string(),
            });
        }
    }
    model
}

fn default_config_toml() -> String {
    let config = Config::default();
    format!(
        "public_socket_path = \"{}\"\nwezterm_command = [{}]\nwayland_proxy_command = [{}]\nrefresh_interval_seconds = {}\n\n[ui]\ndefault_open_behavior = \"focus-existing\"\nstop_confirmation = {}\nasync_error_display = \"notification\"\n\n[waybar]\nenable = {}\nmodule_name = \"{}\"\n\n[quickshell]\nenable = false\ncontrol_center_state_path = \"$XDG_RUNTIME_DIR/d2b-wlterm/control-center.json\"",
        config.public_socket_path,
        quoted_list(&config.wezterm_command),
        quoted_list(&config.wayland_proxy_command),
        config.refresh_interval_seconds,
        config.ui.stop_confirmation,
        config.waybar.enable,
        config.waybar.module_name.replace('"', "\\\""),
    )
}

fn quoted_list(parts: &[String]) -> String {
    parts
        .iter()
        .map(|part| format!("\"{}\"", part.replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_open_decision(decision: OpenDecision) -> String {
    match decision {
        OpenDecision::OpenNew { .. } => "open-new".to_owned(),
        OpenDecision::FocusExisting { .. } => "focus-existing".to_owned(),
        OpenDecision::Prompt { .. } => "prompt".to_owned(),
        OpenDecision::ForceOpen { .. } => "force-open".to_owned(),
    }
}

fn render_stop_request(request: &StopRequest) -> String {
    format!(
        "stop requires_confirmation={}",
        request.requires_confirmation
    )
}

fn render_async_error(event: &AsyncErrorEvent) -> String {
    RenderedAsyncError::from_event(event).map_or_else(
        || "async-error render=false".to_owned(),
        |value| value.detail,
    )
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
    fn blocked_services_fail_closed() {
        assert_eq!(
            run(vec!["list".into(), "work".into()]).unwrap_err(),
            SERVICES_UNAVAILABLE
        );
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
    fn render_sample_requires_one_explicit_output_path() {
        assert_eq!(
            run(vec!["render-sample".into()]).unwrap_err(),
            "render-sample requires an explicit PNG output path"
        );
        assert_eq!(
            run(vec![
                "render-sample".into(),
                "sample.png".into(),
                "extra.png".into()
            ])
            .unwrap_err(),
            "render-sample accepts exactly one PNG output path"
        );
    }
}
