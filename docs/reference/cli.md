# CLI reference

Presentation commands remain available:

- `d2b-wlterm waybar`
- `d2b-wlterm state` or `status-json`
- `d2b-wlterm control-center` or `quickshell`
- `d2b-wlterm render-sample <output.png>`
- `d2b-wlterm prompt-name`, `already-attached`, `config`, and `async-error`

The `list`, `create`, `open`, `detach`, and `stop` names are reserved for the
canonical terminal service adapter. They fail closed in this source cut.
No command uses the removed public-JSON protocol, copied fixtures, SSH, a host
shell, or direct Wayland control.
