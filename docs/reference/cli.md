# CLI reference

Presentation commands remain available:

- `d2b-wlterm waybar`
- `d2b-wlterm state` or `status-json`
- `d2b-wlterm control-center` or `quickshell`
- `d2b-wlterm render-sample <output.png>`
- `d2b-wlterm prompt-name`, `already-attached`, `config`, and `async-error`

The client adapter uses authenticated ComponentSession discovery and canonical
terminal selections for list, detach, and kill. Interactive `create` and `open`
remain fail-closed until the canonical desktop route can carry the terminal
stream. No command uses the removed public-JSON protocol, copied fixtures,
SSH, a host shell, or direct Wayland control.
