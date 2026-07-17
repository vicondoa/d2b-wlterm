# Design

`d2b-wlterm` is an unprivileged user-session companion. Its repository-local
model is presentation state, not a second d2b protocol. Canonical client,
contract, and session source comes from the exactly pinned
`d2b-client-toolkit` distribution. See
[Presentation model ownership](../reference/presentation-model.md).

## Presentation model

The reducer accepts bounded display identifiers and normalized presentation
rows from an adapter. It does not parse canonical v2 identities or deserialize
daemon records. The future service adapter will consume canonical toolkit types
and map only the fields required by the UI.

Realm grouping is presentation metadata derived from the canonical target.
Cards expose provider kind, typed isolation posture, session persistence,
availability, and remediation.

Realm surfaces use one rounded accent frame with a neutral inset, keeping both
ends of the accent rail on the same contour.

## Control-center lifecycle

The Quickshell surface requests on-demand keyboard focus. Startup inactive
events are ignored until genuine first focus; later focus loss exits an
unpinned process. Pinning suppresses only that dismissal. No presentation action closes the panel
unconditionally.

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

1. Blocked target, shell, stream, and Wayland operations fail closed.
2. There is no legacy public-JSON, SSH, host-shell, or direct-compositor
   fallback.
3. Presentation serialization is local UI output, never a daemon input format.
4. The source pin and fingerprint bind all future canonical client types to one
   immutable d2b release.

The client does not access the privileged broker, private unsafe-local helper,
root-owned bundle artifacts, or host process state.
