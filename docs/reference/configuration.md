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

The schema is intentionally provisional until the d2b-toolkit adapter lands.
