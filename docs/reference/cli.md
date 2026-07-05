# CLI reference

Current commands:

- `d2b-wlterm name [seed]` prints a deterministic friendly name.
- `d2b-wlterm waybar` prints a minimal Waybar JSON payload.
- `d2b-wlterm list <vm>` lists shell sessions through the d2b public socket.
- `d2b-wlterm create <vm> [shell]` creates a shell attachment and disconnects
  the launcher view without killing the shell.
- `d2b-wlterm open <vm> <shell> [--force]` opens a shell attachment and
  disconnects the launcher view without killing the shell.
- `d2b-wlterm stop <vm> <shell> --confirm` maps Stop to a public-socket shell
  kill; without `--confirm`, the command only reports that confirmation is
  required.
- `d2b-wlterm config` prints the default config scaffold.

The CLI is intentionally small. Public-socket transport remains behind the
toolkit-backed adapter boundary, which refuses broker sockets and redacts daemon
error details to bounded kind plus trace/correlation values.
