# Configuration reference

`d2b-wlterm` reads TOML configuration. The Home Manager module renders the same
shape under `$XDG_CONFIG_HOME/d2b-wlterm/config.toml`.

```toml
public_socket_path = "$XDG_RUNTIME_DIR/d2b/public.sock"
wezterm_command = ["weezterm", "start", "--"]
refresh_interval_seconds = 5

[ui]
default_open_behavior = "focus-existing"
stop_confirmation = true
async_error_display = "notification"

[waybar]
enable = true
module_name = "custom/d2b-wlterm"
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
