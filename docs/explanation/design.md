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
   frontend sends them to d2b.
2. **Already-attached Open**: opening a VM/session that already has a terminal
   can focus the existing attachment, prompt the user, or force a new attach
   according to config.
3. **Async error display**: delayed d2b or compositor errors are captured as UI
   events so a panel/Waybar/frontend can surface them after the initiating click.

The d2b adapter crate consumes shared toolkit DTOs and maps planned shell
actions to those DTOs. It does not own public-socket networking; that remains a
separate async boundary.
