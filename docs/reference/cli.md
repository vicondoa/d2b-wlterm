# CLI reference

Current commands:

- `d2b-wlterm name [seed]` prints a deterministic friendly name.
- `d2b-wlterm waybar` prints a minimal Waybar JSON payload.
- `d2b-wlterm open` exercises the already-attached Open model.
- `d2b-wlterm stop` exercises the Stop confirmation model.
- `d2b-wlterm config` prints the default config scaffold.

The CLI is intentionally small. Public-socket transport remains behind the
toolkit-backed adapter boundary.
