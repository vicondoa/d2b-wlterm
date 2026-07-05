# Configuration reference

`d2b-wlterm` reads TOML configuration. The Home Manager module renders the same
shape under `$XDG_CONFIG_HOME/d2b-wlterm/config.toml`.

```toml
public_socket_path = "$XDG_RUNTIME_DIR/d2b/public.sock"
wezterm_command = ["wezterm", "start", "--"]
refresh_interval_seconds = 5

[ui]
default_open_behavior = "focus-existing"
stop_confirmation = true
async_error_display = "notification"

[waybar]
enable = true
module_name = "custom/d2b-wlterm"

[quickshell]
enable = false
control_center_state_path = "$XDG_RUNTIME_DIR/d2b-wlterm/control-center.json"
```

`ui.default_open_behavior` accepts:

- `focus-existing`: focus the existing attached terminal when possible.
- `prompt`: ask before opening another attachment.
- `force-open`: request a new attachment even when one is already attached.

`ui.async_error_display` accepts `inline`, `notification`, `waybar`, or
`silent`. Async d2b client failures include a bounded trace/correlation id and
must not display raw shell names, opaque daemon handles, or terminal bytes.

Offline VMs remain visible but shell list, create, and open actions are disabled
in the core planner.

Waybar output is a JSON custom-module payload with `text`, `tooltip`, and
`class`. The button text includes the active shell count and switches to an
error class whenever a renderable async error exists.

Control-center state is frontend-neutral JSON for Quickshell or similar shells.
It contains VM cards, sanitized shell labels, available actions, active-shell
counts, and safe async-error render data. Labels strip control and ANSI escape
characters, truncate long text, and use `unnamed-shell` for empty labels.

## Flake/Home Manager coverage

The Home Manager module renders this TOML shape directly. The flake exports a
`checks.<system>.home-manager-module` evaluation check that enables the module,
sets `defaultOpenBehavior = "force-open"`, enables Waybar output, and asserts
that the generated config still contains the `wezterm_command`, UI, and Waybar
keys documented here.
