# Design sketch

`d2b-wlterm` is a small user-session companion for d2b desktop workflows. It is
not a privileged control plane. It should talk to d2b through the public daemon
socket, then launch or focus a WezTerm window using user-session state.

The model keeps three safety-sensitive concepts explicit:

1. **Stop confirmation**: destructive Stop actions require confirmation before a
   frontend sends them to d2b.
2. **Already-attached Open**: opening a VM/session that already has a terminal
   should focus or report the existing attachment rather than silently spawning
   duplicates.
3. **Async error display**: delayed d2b or compositor errors are captured as UI
   events so a panel/Waybar/frontend can surface them after the initiating click.
