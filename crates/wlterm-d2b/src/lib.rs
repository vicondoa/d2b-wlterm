//! d2b toolkit adapter boundary for wlterm.
//!
//! The real public-socket transport belongs behind this crate boundary. For now
//! wlterm-core plans actions and this crate maps those plans to toolkit DTOs.

use d2b_toolkit_core::{
    ShellAttachArgs, ShellKillArgs, ShellListArgs, ShellName, ShellOp, TerminalSize,
};
use wlterm_core::friendly_name::FriendlyName;
use wlterm_core::{PlannedAction, VmId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct D2bClientConfig {
    pub public_socket_path: String,
    pub initial_terminal_size: TerminalSize,
}

impl Default for D2bClientConfig {
    fn default() -> Self {
        Self {
            public_socket_path: default_public_socket_path(),
            initial_terminal_size: TerminalSize { rows: 24, cols: 80 },
        }
    }
}

fn default_public_socket_path() -> String {
    std::env::var("XDG_RUNTIME_DIR")
        .map(|dir| format!("{dir}/d2b/public.sock"))
        .unwrap_or_else(|_| "/run/d2b/public.sock".to_string())
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
            PlannedAction::ListSessions { vm } => Some(ShellOp::List(ShellListArgs {
                vm: vm.as_str().to_string(),
            })),
            PlannedAction::AttachShell { vm, name, force } => {
                Some(ShellOp::Attach(ShellAttachArgs {
                    vm: vm.as_str().to_string(),
                    name: name.as_ref().map(to_toolkit_shell_name),
                    force: *force,
                    initial_terminal_size: self.config.initial_terminal_size,
                }))
            }
            PlannedAction::StopVm { .. }
            | PlannedAction::RefreshVms
            | PlannedAction::FocusExistingShell { .. }
            | PlannedAction::PromptAlreadyAttached { .. }
            | PlannedAction::PromptStop { .. }
            | PlannedAction::Disabled { .. } => None,
        }
    }
}

pub fn to_toolkit_shell_name(name: &FriendlyName) -> ShellName {
    ShellName::new(name.as_str().to_string())
}

pub fn kill_shell_op(vm: &VmId, name: &FriendlyName) -> ShellOp {
    ShellOp::Kill(ShellKillArgs {
        vm: vm.as_str().to_string(),
        name: to_toolkit_shell_name(name),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wlterm_core::friendly_name::FriendlyName;
    use wlterm_core::{PlannedAction, VmId};

    fn shell(name: &str) -> FriendlyName {
        FriendlyName::from_candidate(name).unwrap()
    }

    #[test]
    fn attach_plan_maps_to_toolkit_shell_op_without_transport() {
        let boundary = D2bActionBoundary::new(D2bClientConfig::default());
        let action = PlannedAction::AttachShell {
            vm: VmId::new("work").unwrap(),
            name: Some(shell("quiet-otter")),
            force: true,
        };

        let Some(ShellOp::Attach(args)) = boundary.plan_to_shell_op(&action) else {
            panic!("expected attach op");
        };

        assert_eq!(args.vm, "work");
        assert_eq!(args.name.expect("name").metrics_label_value(), "shell");
        assert!(args.force);
        assert_eq!(args.initial_terminal_size.rows, 24);
    }

    #[test]
    fn offline_disabled_plan_has_no_d2b_shell_op() {
        let boundary = D2bActionBoundary::new(D2bClientConfig::default());
        let action = PlannedAction::Disabled {
            reason: wlterm_core::DisabledReason::VmOffline,
        };

        assert!(boundary.plan_to_shell_op(&action).is_none());
    }

    #[test]
    fn shell_name_does_not_become_metric_label() {
        let op = kill_shell_op(
            &VmId::new("work").unwrap(),
            &shell("customer-project-shell"),
        );

        assert_eq!(op.metrics_label_value(), "kill");
        if let ShellOp::Kill(args) = op {
            assert_eq!(args.name.metrics_label_value(), "shell");
        }
    }
}
