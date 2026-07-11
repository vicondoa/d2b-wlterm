# CLI reference

Shell commands accept a canonical workload target. A legacy local VM name is
resolved through public workload inventory for compatibility.

- `d2b-wlterm list <target>` lists persistent shell sessions.
- `d2b-wlterm create <target> [shell]` creates an attachment, disconnects the
  launcher view, and opens the configured proxied WezTerm window.
- `d2b-wlterm open <target> <shell> [--force]` opens an existing shell.
- `d2b-wlterm detach <target> <shell>` disconnects an attachment without
  killing the shell.
- `d2b-wlterm stop <target> <shell> --confirm` kills the named shell. Without
  `--confirm`, no daemon request is sent.
- `d2b-wlterm waybar` emits Waybar JSON.
- `d2b-wlterm state` or `status-json` emits control-center JSON.
- `d2b-wlterm control-center` or `quickshell` toggles the Quickshell panel.
- `d2b-wlterm prompt-name [shell]`, `already-attached`, `config`, and
  `async-error` expose frontend support state.

Discovery and shell dispatch use only d2bd's public socket. The CLI does not
invoke `d2b list`, inspect private artifacts, connect to the broker/helper, or
fall back to SSH or a host shell. Unsafe-local actions are disabled when
`unsafe-local-shell-v1` is not negotiated and report an update remediation.

Errors contain bounded kind and correlation values. Opaque handles, terminal
bytes, shell output, argv, environment, cwd, and private paths are never
rendered.
