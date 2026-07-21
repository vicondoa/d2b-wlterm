# Presentation model ownership

`d2b-wlterm` owns only terminal-launcher configuration and presentation state.
The structs in `wlterm-core`, `wlterm-ui`, and `wlterm-waybar` describe local
reducer state and the JSON rendered for the user-session UI. They are not d2b
request, response, session, identity, handle, or transport contracts.

Presentation state is output-only. Daemon data must enter through an adapter
that consumes canonical `d2b-client-toolkit` types and explicitly maps the
fields needed by the UI. The presentation types therefore do not implement
deserialization as an alternate wire format. Configuration types remain
deserializable because they own the repository-local TOML format.

The canonical d2b source owns target resolution, sessions, services, streams,
and descriptor handling. This repository must not copy their DTOs, generated
bindings, framing, handshake, fixtures, or redaction wrappers. The source pin
is recorded in the workspace and flake metadata; all canonical crates must
come from that one immutable source distribution.

Live target discovery, session setup, persistent-shell operations and streams,
and Wayland control require their owning service contracts. Their migration is
intentionally deferred rather than guessed in this presentation layer.
