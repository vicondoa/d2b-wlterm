# Configuration reference

`d2b-wlterm` reads TOML configuration. The Home Manager module renders the same
shape under `$XDG_CONFIG_HOME/d2b-wlterm/config.toml`.

```toml
public_socket_path = "/run/d2b/public.sock"
wezterm_command = ["weezterm", "start", "--"]
wayland_proxy_command = ["d2b-wayland-proxy"]
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

`wezterm_command` is the only supported terminal backend.
`wayland_proxy_command` must name a d2b-wayland-proxy build with typed readiness
support. `create` and `open` wait for first-client readiness; failure terminates
the proxy and never starts WezTerm directly.

`ui.default_open_behavior` accepts:

- `focus-existing`: focus the existing attached terminal when possible.
- `prompt`: ask before opening another attachment.
- `force-open`: request a new attachment even when one is already attached.

`ui.async_error_display` accepts `inline`, `notification`, `waybar`, or
`silent`. Async d2b client failures include a bounded trace/correlation id and
must not display raw shell names, opaque daemon handles, or terminal bytes.

Unavailable workloads remain visible. Cards include provider kind, isolation
posture, session persistence, availability, and typed remediation. Unsafe-local
cards warn that they provide no isolation and remain disabled unless
`unsafe-local-shell-v1` was negotiated.

Waybar output is a JSON custom-module payload with `text`, `tooltip`, and
`class`. The tooltip includes an explicit no-isolation warning when unsafe-local
workloads are present.

Control-center state is frontend-neutral JSON. It contains canonical workload
cards plus legacy `vms`/`id` compatibility fields, sanitized shell labels,
available actions, active-shell counts, and safe async-error data.

## Flake/Home Manager coverage

The flake's package and Home Manager checks use release version 0.2.0. The
module check evaluates generated TOML, Waybar, and Quickshell output without
starting d2b.
