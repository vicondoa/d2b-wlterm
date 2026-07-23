# Design

`d2b-wlterm` is an unprivileged user-session companion. It discovers
shell-capable workloads and performs shell operations through d2bd's public
socket using d2b-toolkit's runtime-agnostic transport.

## Workload model

The canonical identifier is `<workload>.<realm>[.<ancestor>...].d2b`. Inventory
rows must advertise `persistent-shell` and contain a shell launcher item.
`legacyVmName` and the legacy flat `id`/`vms` JSON fields remain compatibility
aliases; dispatch always preserves the canonical target. This permits
first-class local VMs with no legacy name and unsafe-local workloads without
coercing either into a VM-only model.

Realm grouping is presentation metadata derived from the canonical target.
Cards expose provider kind, typed isolation posture, session persistence,
availability, and remediation.

Realm surfaces use one rounded accent frame with a neutral inset, keeping both
ends of the accent rail on the same contour.

## Control-center lifecycle

The Quickshell surface requests on-demand keyboard focus. Startup inactive
events are ignored until genuine first focus; later focus loss exits an
unpinned process. Pinning suppresses only that dismissal, so create/open
actions can naturally move focus to a terminal while a pinned panel remains
open. No action closes the panel unconditionally.

Layer Shell supplies the usable output bounds after exclusive zones such as
Waybar. Header dragging changes only in-process margins inside those bounds;
it uses no compositor IPC or hard-coded bar geometry. Output, scale, and
content-size changes reclamp the card, and a new process starts at the original
24 px top-right margins.

`unsafe-local` means no isolation boundary. Its shells run in the authenticated
host user's session and may access that user's files, network, agents, D-Bus,
and other ambient resources. The UI therefore displays an explicit
`UNSAFE LOCAL · NO ISOLATION` warning.

## Safety boundaries

1. Stop requires confirmation before a shell kill is sent.
2. Opening an attached shell focuses, prompts, or force-opens according to
   configuration; there is still one active attachment unless force is explicit.
3. Unsafe-local shell calls require negotiated `unsafe-local-shell-v1`. Version
   skew disables those actions with an update remediation.
4. Public requests contain canonical targets and semantic shell operations, not
   argv, environment, cwd, paths, terminal bytes, or private helper messages.
5. GUI windows are launched only as children of `d2b-wayland-proxy`. The
   launcher waits for typed first-client readiness and terminates the proxy on
   failure. It never starts WezTerm directly as a fallback.

The client does not access the privileged broker, private unsafe-local helper,
root-owned bundle artifacts, or host process state. WezTerm remains the sole
terminal backend and speaks the same public shell protocol for its byte stream.
