# Design sketch

`d2b-wlterm` is a small user-session companion for d2b desktop workflows. It is
not a privileged control plane. It should talk to d2b through the public daemon
socket, then launch or focus a WezTerm window using user-session state.

The core model is a reducer over VM snapshots and UI events. It keeps offline
VMs visible but disables shell list, create, and open actions until the VM is
online. Planned effects stay typed so frontends can render prompts without
reaching into d2b state directly.

The model keeps three safety-sensitive concepts explicit:

1. **Stop confirmation**: destructive Stop actions require confirmation before a
   frontend sends a public-socket shell kill to d2b.
2. **Already-attached Open**: opening a VM/session that already has a terminal
   can focus the existing attachment, prompt the user, or force a new attach
   according to config.
3. **Async error display**: delayed d2b or compositor errors are captured as UI
   events with bounded correlation so a status bar, Waybar, or frontend can surface them
   after the initiating click without exposing shell names, handles, or terminal
   bytes.

The d2b adapter crate consumes shared toolkit DTOs and `d2b-client` to execute
planned shell list, attach, disconnect, and kill actions over the public daemon
socket. It refuses privileged broker paths, does not invoke subprocess bridges
for shell attach/open, and keeps offline VM actions disabled in the planner.

WeezTerm integration is deliberately a command boundary, not a privileged
control-plane dependency. `d2b-wlterm` chooses which terminal command to launch
from config, and WeezTerm's native d2b provider, when used, speaks the same
public daemon socket through the shared toolkit crates.
