//! UI state concepts and Quickshell frontend for d2b-wlterm.

use std::{
    env, fs,
    io::Write as _,
    os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use wlterm_core::friendly_name::FriendlyName;
use wlterm_core::{
    realm_from_canonical_target, AsyncErrorDisplay, AsyncErrorEvent as CoreAsyncErrorEvent, Model,
    OpenBehavior, ProviderKind, SafeCorrelation, SessionId, ShellVisualState, TargetAvailability,
    TargetPowerState, TargetRemediation,
};

pub const DISPLAY_LABEL_MAX_CHARS: usize = 40;
pub const EMPTY_LABEL_PLACEHOLDER: &str = "unnamed-shell";

#[derive(Clone, PartialEq, Eq)]
pub enum OpenDecision {
    OpenNew { session: String },
    FocusExisting { session: String },
    Prompt { session: String },
    ForceOpen { session: String },
}

impl std::fmt::Debug for OpenDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenNew { .. } => f
                .debug_struct("OpenNew")
                .field("session", &"<redacted>")
                .finish(),
            Self::FocusExisting { .. } => f
                .debug_struct("FocusExisting")
                .field("session", &"<redacted>")
                .finish(),
            Self::Prompt { .. } => f
                .debug_struct("Prompt")
                .field("session", &"<redacted>")
                .finish(),
            Self::ForceOpen { .. } => f
                .debug_struct("ForceOpen")
                .field("session", &"<redacted>")
                .finish(),
        }
    }
}

pub fn decide_open(
    session: &SessionId,
    already_attached: bool,
    behavior: OpenBehavior,
) -> OpenDecision {
    if !already_attached {
        return OpenDecision::OpenNew {
            session: session.as_str().to_string(),
        };
    }

    match behavior {
        OpenBehavior::FocusExisting => OpenDecision::FocusExisting {
            session: session.as_str().to_string(),
        },
        OpenBehavior::ForceOpen => OpenDecision::ForceOpen {
            session: session.as_str().to_string(),
        },
        OpenBehavior::Prompt => OpenDecision::Prompt {
            session: session.as_str().to_string(),
        },
    }
}

const QML_FILE: &str = "shell.qml";
const PID_FILE: &str = "quickshell.pid";
const SIGTERM: i32 = 15;

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProcessIdentity {
    pid: u32,
    start_time_ticks: u64,
}

pub fn open(_config: &wlterm_core::Config) -> Result<(), String> {
    let dir = runtime_dir()?;
    fs::create_dir_all(&dir).map_err(|err| format!("failed to create runtime dir: {err}"))?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
        .map_err(|err| format!("failed to secure runtime dir: {err}"))?;

    let pid_path = dir.join(PID_FILE);
    if let Some(identity) = read_live_frontend(&pid_path, &dir) {
        // SAFETY: pid is validated against /proc start_time and cmdline before signaling.
        let _ = unsafe { kill(identity.pid as i32, SIGTERM) };
        let _ = fs::remove_file(&pid_path);
        return Ok(());
    }

    let qml_path = materialize_qml(&dir)?;
    let backend =
        env::current_exe().map_err(|err| format!("failed to locate d2b-wlterm backend: {err}"))?;
    let theme_json = fs::read_to_string("/etc/d2b/ui-colors.json").unwrap_or_else(|_| "{}".into());
    let quickshell = quickshell_program()
        .ok_or_else(|| "failed to find quickshell frontend binary".to_string())?;
    let mut child = Command::new(quickshell)
        .arg("--path")
        .arg(&qml_path)
        .arg("--no-duplicate")
        .env("D2B_WLTERM_BIN", backend)
        .env("D2B_WLTERM_THEME_JSON", theme_json)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("failed to launch quickshell: {err}"))?;
    let identity = process_identity(child.id())
        .ok_or_else(|| "failed to read quickshell process identity".to_string())?;
    write_pid_record(&pid_path, identity)?;
    std::thread::spawn(move || {
        let _ = child.wait();
        if read_pid_record(&pid_path).is_some_and(|current| current == identity) {
            let _ = fs::remove_file(&pid_path);
        }
    });
    Ok(())
}

fn runtime_dir() -> Result<PathBuf, String> {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "XDG_RUNTIME_DIR is required for the Quickshell frontend".to_string())
        .map(|path| path.join("d2b-wlterm").join("quickshell"))
}

fn quickshell_program() -> Option<PathBuf> {
    if let Some(path) = env::var_os("D2B_WLTERM_QUICKSHELL") {
        return Some(PathBuf::from(path));
    }
    if let Some(path) = find_in_path("quickshell") {
        return Some(path);
    }
    let system = PathBuf::from("/run/current-system/sw/bin/quickshell");
    if system.is_file() {
        return Some(system);
    }
    let entries = fs::read_dir("/nix/store").ok()?;
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("bin/quickshell"))
        .find(|path| path.is_file())
}

fn find_in_path(binary: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|dir| dir.join(binary))
            .find(|candidate| candidate.is_file())
    })
}

fn materialize_qml(dir: &Path) -> Result<PathBuf, String> {
    let path = dir.join(QML_FILE);
    write_private_file(&path, QML_SOURCE.as_bytes())?;
    Ok(path)
}

fn write_pid_record(path: &Path, identity: ProcessIdentity) -> Result<(), String> {
    write_private_file(
        path,
        format!("{} {}\n", identity.pid, identity.start_time_ticks).as_bytes(),
    )
}

fn write_private_file(path: &Path, content: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|err| format!("failed to open {}: {err}", tmp.display()))?;
    file.write_all(content)
        .map_err(|err| format!("failed to write {}: {err}", tmp.display()))?;
    file.sync_all()
        .map_err(|err| format!("failed to sync {}: {err}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|err| format!("failed to install {}: {err}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|err| format!("failed to secure {}: {err}", path.display()))?;
    Ok(())
}

fn read_pid_record(path: &Path) -> Option<ProcessIdentity> {
    let text = fs::read_to_string(path).ok()?;
    let mut parts = text.split_whitespace();
    Some(ProcessIdentity {
        pid: parts.next()?.parse().ok()?,
        start_time_ticks: parts.next()?.parse().ok()?,
    })
}

fn read_live_frontend(path: &Path, runtime_dir: &Path) -> Option<ProcessIdentity> {
    let identity = read_pid_record(path)?;
    let live = process_identity(identity.pid)?;
    if live == identity && cmdline_matches_quickshell(identity.pid, runtime_dir) {
        Some(identity)
    } else {
        let _ = fs::remove_file(path);
        None
    }
}

fn process_identity(pid: u32) -> Option<ProcessIdentity> {
    let stat =
        fs::read_to_string(PathBuf::from("/proc").join(pid.to_string()).join("stat")).ok()?;
    let after_comm = stat.rsplit_once(") ")?.1;
    let start_time_ticks = after_comm.split_whitespace().nth(19)?.parse().ok()?;
    Some(ProcessIdentity {
        pid,
        start_time_ticks,
    })
}

fn cmdline_matches_quickshell(pid: u32, runtime_dir: &Path) -> bool {
    let bytes =
        fs::read(PathBuf::from("/proc").join(pid.to_string()).join("cmdline")).unwrap_or_default();
    let args: Vec<String> = bytes
        .split(|b| *b == 0)
        .filter(|part| !part.is_empty())
        .filter_map(|part| std::str::from_utf8(part).ok().map(ToOwned::to_owned))
        .collect();
    let qml_path = runtime_dir.join(QML_FILE).display().to_string();
    args.first()
        .and_then(|arg| Path::new(arg).file_name())
        .is_some_and(|name| name == "quickshell")
        && args
            .windows(2)
            .any(|pair| pair == ["--path", qml_path.as_str()])
        && args.iter().any(|arg| arg == "--no-duplicate")
}

const QML_SOURCE: &str = r##"
    //@ pragma StateDir $XDG_STATE_HOME/d2b-wlterm/quickshell
    //@ pragma IconTheme Adwaita

    import QtQuick
    import Quickshell
    import Quickshell.Io

    ShellRoot {
      id: root
      property string backend: Quickshell.env("D2B_WLTERM_BIN") || "d2b-wlterm"
      property var state: ({ workloads: [], vms: [], realmGroups: [], activeShells: 0, hasError: false, errors: [], remediation: null })
      property bool busy: false
      property string message: ""
      property string hoverHint: ""
      property bool failed: false
      property string confirmKey: ""
      property real panelTopMargin: 24
      property real panelRightMargin: 24
      property var theme: parseJsonObject(Quickshell.env("D2B_WLTERM_THEME_JSON"))

      function reload() { statusProc.exec([backend, "status-json"]) }
      function action(args) {
        busy = true
        failed = false
        message = runningMessage(args)
        actionProc.args = args
        actionProc.exec([backend].concat(args))
      }
      function runningMessage(args) {
        const verb = args[0] || "action"
        if (verb === "create") return "Creating shell in " + args[1] + "..."
        if (verb === "open") return "Attaching " + args[2] + "..."
        if (verb === "detach") return "Detaching " + args[2] + "..."
        if (verb === "stop") return "Stopping " + args[2] + "..."
        return "Working..."
      }
      function successMessage(args) {
        const verb = args[0] || "action"
        if (verb === "create") return "Created terminal"
        if (verb === "open") return "Attached terminal"
        if (verb === "detach") return "Detached terminal"
        if (verb === "stop") return "Stopped terminal"
        return "Done"
      }
      function statusText() {
        if (message.length > 0) return message
        if (hoverHint.length > 0) return hoverHint
        if (busy) return "working..."
        const groups = state.realmGroups || []
        if (groups.length === 0 && (state.workloads || state.vms || []).length === 0) return "no shell-capable workloads"
        return root.shellCountLabel(state.activeShells || 0, "active shell")
      }
      function shellCountLabel(count, singular) {
        return String(count) + " " + singular + (count === 1 ? "" : "s")
      }
      function parseJsonObject(text) {
        if (!text || text.length === 0) return ({})
        try {
          const parsed = JSON.parse(text)
          return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed : ({})
        } catch (e) {
          return ({})
        }
      }
      function isHexColor(value) {
        return typeof value === "string" && /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/.test(value)
      }
      function shellColor(name, fallback) { return fallback }
      function realmAccent(realm, fallbackVm) {
        const realms = theme.realms || ({})
        const envs = theme.envs || ({})
        if (realm && realms[realm] && isHexColor(realms[realm].accent)) return realms[realm].accent
        if (realm && envs[realm] && isHexColor(envs[realm].accent)) return envs[realm].accent
        return vmAccent(fallbackVm)
      }
      function vmAccent(vm) {
        const id = vm && (vm.legacyVmName || vm.id || vm.target || vm.label)
        const vms = theme.vms || ({})
        const envs = theme.envs || ({})
        if (id && vms[id] && vms[id].border && isHexColor(vms[id].border.active)) return vms[id].border.active
        if (vm && vm.env && envs[vm.env] && isHexColor(envs[vm.env].accent)) return envs[vm.env].accent
        if (theme.host && isHexColor(theme.host.accent)) return theme.host.accent
        return "#89b4fa"
      }
      function stateColor(name) {
        if (name === "running" || name === "detached") return "#a6e3a1"
        if (name === "attached") return "#89b4fa"
        if (name === "error") return "#f38ba8"
        return "#9399b2"
      }
      function screenWidth() { return panel.screen ? panel.screen.width : 1280 }
      function screenHeight() { return panel.screen && panel.screen.height > 0 ? panel.screen.height : 1080 }
      function clamp(value, min, max) { return Math.max(min, Math.min(max, value)) }
      function movePanel(dx, dy) {
        panelRightMargin = clamp(panelRightMargin - dx, 4, Math.max(4, screenWidth() - panel.width - 4))
        panelTopMargin = clamp(panelTopMargin + dy, 4, Math.max(4, screenHeight() - panel.height - 4))
      }
      function confirmStop(vm, shell) {
        const key = "stop:" + vm + ":" + shell
        if (confirmKey === key) {
          confirmKey = ""
          action(["stop", vm, shell, "--confirm"])
        } else {
          confirmKey = key
          message = "Click stop again to kill " + shell
          confirmTimer.restart()
        }
      }
      function maxPanelHeight() { return Math.max(720, Math.floor(root.screenHeight() * 0.92)) }
      function panelContentHeight() { return 360 + list.implicitHeight + (message.length > 0 ? 36 : 0) }

      Process {
        id: statusProc
        stdout: StdioCollector {
          onStreamFinished: {
            try { root.state = JSON.parse(this.text) }
            catch (e) { root.state = ({ workloads: [], vms: [], activeShells: 0, hasError: true, errors: [{ detail: String(e) }] }) }
          }
        }
        stderr: StdioCollector {}
        onExited: if (!actionProc.running) root.busy = false
      }

      Process {
        id: actionProc
        property string out: ""
        property string err: ""
        property var args: []
        stdout: StdioCollector { onStreamFinished: actionProc.out = this.text.trim() }
        stderr: StdioCollector { onStreamFinished: actionProc.err = this.text.trim() }
        onExited: (exitCode, exitStatus) => {
          const ok = exitCode === 0 && exitStatus === 0
          root.failed = !ok
          if (!ok) root.message = actionProc.err.length > 0 ? actionProc.err : (actionProc.out.length > 0 ? actionProc.out : "Action failed")
          else root.message = actionProc.out.length > 0 ? actionProc.out : root.successMessage(actionProc.args)
          actionProc.out = ""
          actionProc.err = ""
          actionProc.args = []
          root.busy = false
          clearMessage.restart()
          root.reload()
        }
      }

      Timer { id: clearMessage; interval: 2600; repeat: false; onTriggered: if (!root.busy) root.message = "" }
      Timer { id: confirmTimer; interval: 2400; repeat: false; onTriggered: { root.confirmKey = ""; if (!root.busy) root.message = "" } }
      Timer { interval: 2500; running: true; repeat: true; triggeredOnStart: true; onTriggered: if (!statusProc.running && !actionProc.running) root.reload() }

      PanelWindow {
        id: panel
        visible: true
        focusable: true
        aboveWindows: true
        exclusiveZone: 0
        implicitWidth: 420
        implicitHeight: Math.min(Math.max(620, root.panelContentHeight()), root.maxPanelHeight())
        color: "transparent"
        surfaceFormat { opaque: false }
        anchors { top: true; right: true }
        margins { top: root.panelTopMargin; right: root.panelRightMargin }

        Rectangle {
          anchors.fill: parent
          radius: 18
          color: "#0f1117"
          border.color: "#2a2d35"
          border.width: 1
          clip: true

          Column {
            x: 16
            y: 16
            width: parent.width - 32
            height: parent.height - 32
            spacing: 12

            Item {
              width: parent.width
              height: 32
              MouseArea {
                anchors.fill: parent
                acceptedButtons: Qt.LeftButton
                property real lastX: 0
                property real lastY: 0
                onPressed: (mouse) => { lastX = mouse.x; lastY = mouse.y }
                onPositionChanged: (mouse) => { if (pressed) root.movePanel(mouse.x - lastX, mouse.y - lastY) }
              }
              Text {
                anchors.centerIn: parent
                text: "d2b terminals"
                color: "#ffffff"
                font.pixelSize: 16
                font.bold: true
              }
              Row {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                spacing: 8
                IconButton { text: "refresh"; tooltip: "Refresh terminals"; enabled: !root.busy; onClicked: root.reload() }
              }
            }

            Rectangle { width: parent.width; height: 1; color: "#2a2d35" }

            Row {
              width: parent.width
              height: 24
              spacing: 10
              Text {
                text: {
                  const rg = root.state.realmGroups || []
                  const workloads = root.state.workloads || root.state.vms || []
                  return rg.length > 1
                    ? rg.length + " realms, " + workloads.length + " workload(s)"
                    : workloads.length + " workload(s)"
                }
                color: "#ffffff"; font.pixelSize: 13; font.bold: true
              }
              Text { text: root.statusText(); color: root.failed ? "#f38ba8" : "#9399b2"; font.pixelSize: 12; elide: Text.ElideRight; width: parent.width - 80 }
            }

            Rectangle {
              visible: root.message.length > 0 && !root.busy
              width: parent.width
              height: visible ? 28 : 0
              radius: 10
              color: root.failed ? "#2e1a1a" : "#1a2e1a"
              border.color: root.failed ? "#f38ba8" : "#a6e3a1"
              Text { anchors.fill: parent; anchors.margins: 7; text: root.message; color: root.failed ? "#f38ba8" : "#a6e3a1"; font.pixelSize: 11; elide: Text.ElideRight }
            }

            Text {
              visible: root.state.remediation !== null && root.state.remediation !== undefined
              width: parent.width
              text: visible ? root.state.remediation.message : ""
              color: "#f9e2af"
              font.pixelSize: 11
              wrapMode: Text.WordWrap
            }

            Flickable {
              width: parent.width
              height: parent.height - y
              contentWidth: width
              contentHeight: list.implicitHeight
              clip: true
              boundsBehavior: Flickable.StopAtBounds

              Column {
                id: list
                width: parent.width
                spacing: 8

                Repeater {
                  model: root.state.realmGroups || []
                  Rectangle {
                    width: list.width
                    height: realmGroupContent.implicitHeight + 18
                    radius: 13
                    color: "transparent"
                    clip: true
                    property var realmGroup: modelData

                    Rectangle {
                      x: 0
                      y: 0
                      width: 5
                      height: parent.height
                      radius: 0
                      color: root.realmAccent(realmGroup.realm, (realmGroup.workloads || [])[0])
                    }
                    Rectangle {
                      x: 5
                      y: 0
                      width: parent.width - 5
                      height: parent.height
                      radius: 10
                      color: "#10131a"
                      border.color: "#2a2d35"
                      border.width: 1
                    }

                    Column {
                      id: realmGroupContent
                      x: 13
                      y: 8
                      width: parent.width - 21
                      spacing: 6

                      Text {
                        visible: (root.state.realmGroups || []).length > 1
                        text: realmGroup.realm || "local"
                        color: "#6b7280"
                        font.pixelSize: 10
                        font.bold: true
                        leftPadding: 2
                        bottomPadding: 2
                      }

                      Repeater {
                        model: realmGroup.workloads || []
                        Rectangle {
                          id: vmCard
                          width: realmGroupContent.width
                          height: card.implicitHeight + 16
                          radius: 11
                          color: "#16181d"
                          border.color: "#313645"
                          border.width: 1
                          property var vm: modelData

                          Column {
                            id: card
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.top: parent.top
                            anchors.margins: 8
                            spacing: 6

                            Row {
                              width: parent.width
                              height: 28
                              spacing: 8
                                StatusIcon { icon: "circle"; accent: vm.disabled ? "#f38ba8" : "#9399b2"; tooltip: (vm.label || vm.target) + " · " + vm.providerKind + " · " + vm.availability; }
                                Text {
                                  width: parent.width - 104
                                  anchors.verticalCenter: parent.verticalCenter
                                  text: (vm.label || vm.target) + " · " + (vm.target || vm.canonicalTarget || "") + " · " + root.shellCountLabel(vm.activeShells || 0, "shell")
                                  color: "#ffffff"
                                  font.pixelSize: 12
                                  font.bold: true
                                  elide: Text.ElideRight
                                  wrapMode: Text.NoWrap
                                }
                                IconButton { text: "add"; tooltip: vm.disabled ? (vm.disabledReason || "unavailable") : "Create a named shell and open it"; enabled: !root.busy && !vm.disabled; onClicked: root.action(["create", vm.target]) }
                              }

                              Text {
                                width: parent.width
                                text: vm.providerKind + " · " + vm.isolationPosture + " · sessions: " + vm.sessionPersistence
                                color: vm.isolationPosture === "unsafe-local" ? "#f38ba8" : "#9399b2"
                                font.pixelSize: 10
                                font.bold: vm.isolationPosture === "unsafe-local"
                                elide: Text.ElideRight
                              }

                              Text {
                                visible: vm.isolationPosture === "unsafe-local"
                                width: parent.width
                                text: "UNSAFE LOCAL · NO ISOLATION"
                                color: "#f38ba8"
                                font.pixelSize: 11
                                font.bold: true
                              }

                              Text {
                                visible: vm.disabled
                                width: parent.width
                                text: (vm.availability || "unavailable") + (vm.remediation ? " · " + vm.remediation.message : "")
                                color: "#f9e2af"
                                font.pixelSize: 10
                                wrapMode: Text.WordWrap
                              }

                              Repeater {
                                model: vm.shells || []
                                Rectangle {
                                  width: card.width
                                  height: 32
                                  radius: 9
                                  color: "#0d0f14"
                                  border.color: "#313645"
                                  border.width: 1
                                  Row {
                                    anchors.fill: parent
                                    anchors.margins: 5
                                    spacing: 6
                                    StatusIcon { icon: modelData.attached ? "link" : "link_off"; accent: modelData.attached ? "#ffffff" : "#9399b2"; tooltip: modelData.attached ? "attached" : "detached"; }
                                    Text { text: modelData.name; color: "#ffffff"; font.pixelSize: 12; elide: Text.ElideRight; width: parent.width - 126; anchors.verticalCenter: parent.verticalCenter }
                                    IconButton { text: modelData.attached ? "link_off" : "terminal"; tooltip: modelData.attached ? ("Detach " + modelData.name) : ("Attach to " + modelData.name); enabled: !root.busy && !vm.disabled; onClicked: modelData.attached ? root.action(["detach", vm.target, modelData.name]) : root.action(["open", vm.target, modelData.name]) }
                                    IconButton { text: root.confirmKey === ("stop:" + vm.target + ":" + modelData.name) ? "priority_high" : "stop"; tooltip: "Stop " + modelData.name; accent: "#9399b2"; enabled: !root.busy && !vm.disabled; onClicked: root.confirmStop(vm.target, modelData.name) }
                                  }
                                }
                              }
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }

      component StatusIcon: Rectangle {
        property string icon: ""
        property string tooltip: ""
        property string accent: "#9399b2"
        width: 26
        height: 26
        radius: width / 2
        color: "transparent"
        Text {
          anchors.fill: parent
          text: parent.icon
          color: parent.accent
          font.pixelSize: 20
          font.family: "Material Symbols Rounded"
          horizontalAlignment: Text.AlignHCenter
          verticalAlignment: Text.AlignVCenter
        }
        MouseArea {
          anchors.fill: parent
          hoverEnabled: true
          onContainsMouseChanged: root.hoverHint = containsMouse ? parent.tooltip : ""
        }
      }

      component IconButton: Rectangle {
        property alias text: label.text
        property string tooltip: ""
        property color accent: "#9399b2"
        property bool prominent: false
        signal clicked()
        width: prominent ? 30 : 26
        height: prominent ? 30 : 26
        radius: width / 2
        opacity: enabled ? 1.0 : 0.45
        border.width: prominent ? 1 : 0
        border.color: prominent ? accent : "transparent"
        color: prominent
          ? Qt.rgba(accent.r, accent.g, accent.b, mouse.containsMouse ? 0.34 : 0.24)
          : (mouse.containsMouse ? Qt.rgba(accent.r, accent.g, accent.b, 0.12) : "transparent")

        Text {
          id: label
          anchors.fill: parent
          color: enabled ? parent.accent : "#9399b2"
          font.family: "Material Symbols Rounded"
          font.pixelSize: prominent ? 21 : 20
          font.bold: false
          horizontalAlignment: Text.AlignHCenter
          verticalAlignment: Text.AlignVCenter
        }
        MouseArea {
          id: mouse
          anchors.fill: parent
          hoverEnabled: true
          enabled: parent.enabled
          onContainsMouseChanged: root.hoverHint = containsMouse ? (parent.tooltip.length > 0 ? parent.tooltip : parent.text) : ""
          onClicked: parent.clicked()
          onEntered: parent.scale = 1.05
          onExited: parent.scale = 1.0
        }
      }
    }
    "##;

#[derive(Clone, PartialEq, Eq)]
pub struct StopRequest {
    pub session: String,
    pub requires_confirmation: bool,
}

impl std::fmt::Debug for StopRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StopRequest")
            .field("session", &"<redacted>")
            .field("requires_confirmation", &self.requires_confirmation)
            .finish()
    }
}

impl StopRequest {
    pub fn new(session: &SessionId, requires_confirmation: bool) -> Self {
        Self {
            session: session.as_str().to_string(),
            requires_confirmation,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsyncErrorEvent {
    pub message: String,
    pub display: AsyncErrorDisplay,
    pub correlation: Option<SafeCorrelation>,
}

impl AsyncErrorEvent {
    pub fn new(message: impl Into<String>, display: AsyncErrorDisplay) -> Self {
        Self {
            message: message.into(),
            display,
            correlation: None,
        }
    }

    pub fn with_correlation(
        message: impl Into<String>,
        display: AsyncErrorDisplay,
        correlation: SafeCorrelation,
    ) -> Self {
        Self {
            message: message.into(),
            display,
            correlation: Some(correlation),
        }
    }

    pub fn should_render(&self) -> bool {
        self.display != AsyncErrorDisplay::Silent
    }
}

/// A group of shell-capable workloads that share a realm.
///
/// Launchers and status displays consume this to present VMs organized by realm
/// rather than as a flat list. Shell operations still address the VM by its local
/// `id` over the public socket — the realm grouping is presentation metadata only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealmGroup {
    /// Realm label extracted from the canonical target, e.g. `"dev"` or `"local"`.
    pub realm: String,
    /// Workload cards belonging to this realm, in discovery order.
    pub workloads: Vec<WorkloadControlCard>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlCenterState {
    /// Flat list of all shell-capable VMs (kept for backward compatibility).
    pub vms: Vec<WorkloadControlCard>,
    /// Canonical workload list. New consumers should prefer this field.
    pub workloads: Vec<WorkloadControlCard>,
    /// Workloads grouped by realm, derived from each canonical target.
    /// Consumers that can use this should prefer it over the flat `vms` list.
    pub realm_groups: Vec<RealmGroup>,
    pub active_shells: usize,
    pub has_error: bool,
    pub errors: Vec<RenderedAsyncError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<TargetRemediation>,
}

impl ControlCenterState {
    pub fn from_model(model: &Model) -> Self {
        let errors: Vec<_> = model
            .async_errors()
            .iter()
            .filter_map(RenderedAsyncError::from_core)
            .collect();
        let workloads: Vec<_> = model
            .workloads()
            .map(WorkloadControlCard::from_summary)
            .collect();
        let active_shells = workloads
            .iter()
            .map(|workload| workload.active_shells)
            .sum();
        let realm_groups = build_realm_groups(&workloads);

        Self {
            vms: workloads.clone(),
            workloads,
            realm_groups,
            active_shells,
            has_error: !errors.is_empty(),
            errors,
            remediation: model.global_remediation(),
        }
    }

    pub fn empty() -> Self {
        Self {
            vms: Vec::new(),
            workloads: Vec::new(),
            realm_groups: Vec::new(),
            active_shells: 0,
            has_error: false,
            errors: Vec::new(),
            remediation: None,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("control center state serializes")
    }
}

/// Group VM control cards by the realm extracted from each card's canonical target.
///
/// VMs without a parseable realm (no canonical target, or target without a realm
/// segment) are placed in a synthetic `"local"` group. Realm insertion order
/// follows the order in which VMs appear in `vms`.
fn build_realm_groups(workloads: &[WorkloadControlCard]) -> Vec<RealmGroup> {
    let mut groups: Vec<RealmGroup> = Vec::new();
    for workload in workloads {
        let realm = realm_from_canonical_target(&workload.target)
            .unwrap_or("local")
            .to_owned();
        if let Some(group) = groups.iter_mut().find(|g| g.realm == realm) {
            group.workloads.push(workload.clone());
        } else {
            groups.push(RealmGroup {
                realm,
                workloads: vec![workload.clone()],
            });
        }
    }
    groups
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadControlCard {
    pub target: String,
    /// Legacy id retained for 0.1 frontend compatibility.
    pub id: String,
    pub canonical_target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legacy_vm_name: Option<String>,
    pub label: String,
    pub provider_kind: ProviderKind,
    pub isolation_posture: wlterm_core::IsolationPosture,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation_warning: Option<&'static str>,
    pub session_persistence: wlterm_core::SessionPersistence,
    pub availability: TargetAvailability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<TargetRemediation>,
    pub power_state: TargetPowerState,
    pub disabled: bool,
    pub disabled_reason: Option<String>,
    pub active_shells: usize,
    pub shells: Vec<ShellControlRow>,
}

impl WorkloadControlCard {
    fn from_summary(summary: &wlterm_core::WorkloadSummary) -> Self {
        let disabled = !summary.actions_available();
        let shells: Vec<_> = summary
            .sessions
            .iter()
            .map(ShellControlRow::from_session)
            .collect();
        let active_shells = shells
            .iter()
            .filter(|shell| shell.visual_state != ShellVisualState::Unavailable)
            .count();

        Self {
            target: summary.target.as_str().to_string(),
            id: summary.id.as_str().to_string(),
            canonical_target: summary.target.as_str().to_string(),
            legacy_vm_name: summary.legacy_vm_name.clone(),
            label: sanitize_display_label(
                summary
                    .workload_name
                    .as_deref()
                    .unwrap_or(summary.id.as_str()),
            ),
            provider_kind: summary.provider_kind,
            isolation_posture: summary.isolation_posture,
            isolation_warning: summary.isolation_posture.warning(),
            session_persistence: summary.session_persistence,
            availability: summary.availability,
            remediation: summary.remediation(),
            power_state: summary.power_state,
            disabled,
            disabled_reason: disabled.then(|| {
                if !summary.shell_feature_available {
                    "update-required".to_string()
                } else if !summary.availability.is_ready() {
                    summary.availability.metrics_label_value().to_string()
                } else {
                    match summary.power_state {
                        TargetPowerState::Offline => "target-offline".to_string(),
                        TargetPowerState::Unknown => "target-state-unknown".to_string(),
                        TargetPowerState::Online => "disabled".to_string(),
                    }
                }
            }),
            active_shells,
            shells,
        }
    }
}

pub type VmControlCard = WorkloadControlCard;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellControlRow {
    pub name: String,
    pub visual_state: ShellVisualState,
    pub is_default: bool,
    pub attached: bool,
    pub actions: Vec<&'static str>,
}

impl ShellControlRow {
    fn from_session(session: &wlterm_core::ShellSession) -> Self {
        let actions = match session.visual_state {
            ShellVisualState::Attached => vec!["focus-existing", "prompt-force-open", "stop"],
            ShellVisualState::Detached => vec!["open", "stop"],
            ShellVisualState::Unavailable => Vec::new(),
        };

        Self {
            name: sanitize_display_label(session.name.as_str()),
            visual_state: session.visual_state.clone(),
            is_default: session.is_default,
            attached: session.is_attached(),
            actions,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellNamePrompt {
    pub default_name: String,
    pub typed_text: String,
    pub resolved_name: Option<String>,
    pub error: Option<String>,
}

impl ShellNamePrompt {
    pub fn new(typed_text: &str) -> Self {
        let default_name = FriendlyName::generate()
            .map(|name| name.as_str().to_string())
            .unwrap_or_else(|_| "fresh-shell".to_string());
        Self::with_default(&default_name, typed_text)
    }

    pub fn with_default(default_name: &str, typed_text: &str) -> Self {
        let candidate = if typed_text.trim().is_empty() {
            default_name
        } else {
            typed_text.trim()
        };
        match FriendlyName::from_candidate(candidate) {
            Ok(name) => Self {
                default_name: sanitize_display_label(default_name),
                typed_text: sanitize_display_label(typed_text),
                resolved_name: Some(name.as_str().to_string()),
                error: None,
            },
            Err(_) => Self {
                default_name: sanitize_display_label(default_name),
                typed_text: sanitize_display_label(typed_text),
                resolved_name: None,
                error: Some("shell-name-invalid".to_string()),
            },
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("shell name prompt serializes")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlreadyAttachedNotice {
    pub mode: &'static str,
    pub shell: String,
    pub message: String,
    pub allow_force_open: bool,
}

impl AlreadyAttachedNotice {
    pub fn for_behavior(shell_name: &str, behavior: OpenBehavior) -> Self {
        let shell = sanitize_display_label(shell_name);
        match behavior {
            OpenBehavior::FocusExisting => Self {
                mode: "toast",
                shell: shell.clone(),
                message: format!("Focusing {shell}; use force-open if focus is unavailable."),
                allow_force_open: true,
            },
            OpenBehavior::Prompt => Self {
                mode: "prompt",
                shell: shell.clone(),
                message: format!("{shell} is already attached. Open another view?"),
                allow_force_open: true,
            },
            OpenBehavior::ForceOpen => Self {
                mode: "force-open",
                shell: shell.clone(),
                message: format!("Opening another view for {shell}."),
                allow_force_open: false,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCloseDecision {
    pub action: &'static str,
    pub shell: String,
}

pub fn disconnect_terminal_view(session: &SessionId) -> TerminalCloseDecision {
    TerminalCloseDecision {
        action: "disconnect",
        shell: sanitize_display_label(session.as_str()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedAsyncError {
    pub title: String,
    pub detail: String,
    pub display: AsyncErrorDisplay,
    pub correlation: Option<String>,
    pub digest: String,
}

impl RenderedAsyncError {
    pub fn from_event(event: &AsyncErrorEvent) -> Option<Self> {
        if !event.should_render() {
            return None;
        }
        Some(Self::from_parts(
            &event.message,
            event.display,
            event.correlation.as_ref(),
        ))
    }

    pub fn from_core(event: &CoreAsyncErrorEvent) -> Option<Self> {
        if !event.should_render() {
            return None;
        }
        Some(Self::from_parts(
            &event.message,
            event.display,
            event.correlation.as_ref(),
        ))
    }

    fn from_parts(
        message: &str,
        display: AsyncErrorDisplay,
        correlation: Option<&SafeCorrelation>,
    ) -> Self {
        let digest = digest_message(message, correlation);
        let correlation = correlation.map(|value| value.as_str().to_string());
        let detail = match &correlation {
            Some(correlation) => format!("correlation {correlation}; digest {digest}"),
            None => format!("digest {digest}"),
        };
        Self {
            title: "d2b-wlterm action failed".to_string(),
            detail,
            display,
            correlation,
            digest,
        }
    }
}

pub fn sanitize_display_label(value: &str) -> String {
    let mut sanitized = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                for next in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }
        if ch == '\u{9b}' {
            for next in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&next) {
                    break;
                }
            }
            continue;
        }
        if ch.is_control() {
            continue;
        }
        sanitized.push(ch);
    }

    let sanitized = sanitized.trim();
    if sanitized.is_empty() {
        return EMPTY_LABEL_PLACEHOLDER.to_string();
    }

    let mut truncated = String::new();
    for ch in sanitized.chars().take(DISPLAY_LABEL_MAX_CHARS) {
        truncated.push(ch);
    }
    if truncated.is_empty() {
        EMPTY_LABEL_PLACEHOLDER.to_string()
    } else {
        truncated
    }
}

fn digest_message(message: &str, correlation: Option<&SafeCorrelation>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"d2b-wlterm-ui-error");
    hasher.update((message.len() as u64).to_le_bytes());
    hasher.update(message.as_bytes());
    if let Some(correlation) = correlation {
        hasher.update(correlation.as_str().as_bytes());
    }
    let digest = hasher.finalize();
    let mut rendered = String::with_capacity(12);
    for byte in &digest[..6] {
        rendered.push_str(&format!("{byte:02x}"));
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;
    use wlterm_core::friendly_name::FriendlyName;
    use wlterm_core::{
        Config, ModelEvent, PlannedAction, ShellSession, UserIntent, VmId, VmPowerState, VmSummary,
    };

    fn vm(name: &str) -> VmId {
        VmId::new(name).unwrap()
    }

    fn shell(name: &str) -> FriendlyName {
        FriendlyName::from_candidate(name).unwrap()
    }

    fn workload(target: &str, legacy: &str, power: VmPowerState) -> VmSummary {
        let mut summary = VmSummary::discovered(vm(target), vm(legacy), power);
        summary.legacy_vm_name = Some(legacy.to_string());
        summary
    }

    #[test]
    fn already_attached_open_focuses_by_default() {
        let session = SessionId::new("work").unwrap();
        assert_eq!(
            decide_open(&session, true, OpenBehavior::FocusExisting),
            OpenDecision::FocusExisting {
                session: "work".into()
            }
        );
    }

    #[test]
    fn already_attached_open_can_force_new_attachment() {
        let session = SessionId::new("work").unwrap();
        assert_eq!(
            decide_open(&session, true, OpenBehavior::ForceOpen),
            OpenDecision::ForceOpen {
                session: "work".into()
            }
        );
    }

    #[test]
    fn stop_request_keeps_confirmation_explicit() {
        let session = SessionId::new("work").unwrap();
        assert!(StopRequest::new(&session, true).requires_confirmation);
    }

    #[test]
    fn disconnect_view_is_not_stop() {
        let session = SessionId::new("quiet-otter").unwrap();
        let decision = disconnect_terminal_view(&session);
        assert_eq!(decision.action, "disconnect");
        assert_ne!(decision.action, "stop");
    }

    #[test]
    fn qml_realm_groups_use_outer_border_and_neutral_workload_cards() {
        let realm_block = QML_SOURCE
            .find("height: realmGroupContent.implicitHeight + 18")
            .expect("realm group block exists");
        let rail_color = QML_SOURCE[realm_block..]
            .find("color: root.realmAccent(realmGroup.realm, (realmGroup.workloads || [])[0])")
            .expect("realm group left rail uses realm accent");
        let inset = QML_SOURCE[realm_block..]
            .find("x: 0")
            .expect("realm group includes a clean left rail inset");
        assert!(inset < 300);
        assert!(rail_color < 800);
        let neutral_shell = QML_SOURCE[realm_block..]
            .find("border.color: \"#2a2d35\"")
            .expect("realm group frame uses neutral border");
        assert!(neutral_shell < 800);
        let neutral_shell_abs = realm_block + neutral_shell;
        let surface = QML_SOURCE[realm_block..]
            .find("color: \"#10131a\"")
            .expect("realm group has neutral inset surface");
        assert!(surface < 800);
        let workload_card = QML_SOURCE[neutral_shell_abs..]
            .find("border.color: \"#313645\"")
            .expect("workload card keeps neutral border");
        assert!(workload_card < 3500);
    }

    #[test]
    fn manual_shell_name_prompt_defaults_or_overrides() {
        let defaulted = ShellNamePrompt::with_default("quiet-otter", "");
        assert_eq!(defaulted.resolved_name.as_deref(), Some("quiet-otter"));
        assert_eq!(defaulted.error, None);

        let override_name = ShellNamePrompt::with_default("quiet-otter", "brave-panda");
        assert_eq!(override_name.resolved_name.as_deref(), Some("brave-panda"));

        let invalid = ShellNamePrompt::with_default("quiet-otter", "bad/name");
        assert_eq!(invalid.resolved_name, None);
        assert_eq!(invalid.error.as_deref(), Some("shell-name-invalid"));
    }

    #[test]
    fn already_attached_notice_covers_focus_prompt_and_force_open() {
        let focus = AlreadyAttachedNotice::for_behavior("quiet-otter", OpenBehavior::FocusExisting);
        assert_eq!(focus.mode, "toast");
        assert!(focus.allow_force_open);

        let prompt = AlreadyAttachedNotice::for_behavior("quiet-otter", OpenBehavior::Prompt);
        assert_eq!(prompt.mode, "prompt");
        assert!(prompt.allow_force_open);

        let force = AlreadyAttachedNotice::for_behavior("quiet-otter", OpenBehavior::ForceOpen);
        assert_eq!(force.mode, "force-open");
        assert!(!force.allow_force_open);
    }

    #[test]
    fn control_center_state_marks_offline_vm_disabled() {
        let work = vm("work");
        let mut model = Model::new(Config::default());
        model.apply(ModelEvent::VmSnapshot {
            vms: vec![VmSummary::new(work.clone(), VmPowerState::Offline)],
        });

        assert_eq!(
            model.plan(UserIntent::ListSessions { target: work }),
            PlannedAction::Disabled {
                reason: wlterm_core::DisabledReason::TargetOffline
            }
        );

        let state = ControlCenterState::from_model(&model);
        assert!(state.vms[0].disabled);
        assert_eq!(
            state.vms[0].disabled_reason.as_deref(),
            Some("target-offline")
        );
    }

    #[test]
    fn control_center_counts_active_shells_and_renders_errors() {
        let mut summary = workload("work.example.d2b", "work", VmPowerState::Online);
        summary
            .sessions
            .push(ShellSession::attached(shell("quiet-otter")));
        summary
            .sessions
            .push(ShellSession::detached(shell("brave-panda")));

        let mut model = Model::new(Config::default());
        model.apply(ModelEvent::VmSnapshot { vms: vec![summary] });
        model.apply(ModelEvent::AsyncError {
            message: "contains \u{1b}[31mquiet-otter\u{1b}[0m and opaque handle".into(),
        });

        let state = ControlCenterState::from_model(&model);
        assert_eq!(state.active_shells, 2);
        assert_eq!(state.vms[0].canonical_target, "work.example.d2b");
        assert!(state.to_json().contains("\"canonicalTarget\""));
        assert!(state.has_error);
        assert_eq!(state.errors[0].title, "d2b-wlterm action failed");
        assert!(!state.to_json().contains("quiet-otter and opaque"));
    }

    #[test]
    fn unsafe_local_card_shows_provider_posture_persistence_and_remediation() {
        let mut unsafe_local = workload("tools.host.d2b", "tools", VmPowerState::Unknown);
        unsafe_local.provider_kind = ProviderKind::UnsafeLocal;
        unsafe_local.isolation_posture = wlterm_core::IsolationPosture::UnsafeLocal;
        unsafe_local.session_persistence = wlterm_core::SessionPersistence::UserManagerLifetime;
        unsafe_local.availability = TargetAvailability::HelperUnavailable;

        let mut model = Model::new(Config::default());
        model.apply(ModelEvent::WorkloadSnapshot {
            workloads: vec![unsafe_local],
        });
        let state = ControlCenterState::from_model(&model);
        let card = &state.workloads[0];

        assert_eq!(card.target, "tools.host.d2b");
        assert_eq!(card.provider_kind, ProviderKind::UnsafeLocal);
        assert_eq!(
            card.isolation_warning,
            Some("No isolation: runs in the host user session")
        );
        assert_eq!(
            card.session_persistence,
            wlterm_core::SessionPersistence::UserManagerLifetime
        );
        assert_eq!(
            card.remediation.unwrap().kind,
            wlterm_core::RemediationKind::RestartHelper
        );
        let json = state.to_json();
        assert!(json.contains("\"providerKind\":\"unsafe-local\""));
        assert!(json.contains("\"isolationPosture\":\"unsafe-local\""));
        assert!(json.contains("\"sessionPersistence\":\"user-manager-lifetime\""));
        assert!(QML_SOURCE.contains("UNSAFE LOCAL · NO ISOLATION"));
        assert!(QML_SOURCE.contains("[\"create\", vm.target]"));
    }

    #[test]
    fn feature_skew_remediation_is_visible_without_inventory_rows() {
        let mut model = Model::new(Config::default());
        model.apply(ModelEvent::GlobalRemediation {
            remediation: TargetRemediation {
                kind: wlterm_core::RemediationKind::UpdateD2b,
                message: "Update d2b components together",
            },
        });
        let state = ControlCenterState::from_model(&model);
        assert_eq!(
            state.remediation.unwrap().kind,
            wlterm_core::RemediationKind::UpdateD2b
        );
        assert!(state.to_json().contains("Update d2b components together"));
    }

    #[test]
    fn realm_groups_are_built_from_canonical_targets() {
        let dev_vm = workload("dev-general.dev.d2b", "dev-general", VmPowerState::Online);
        let work_vm = workload("work-aad.corp.d2b", "work-aad", VmPowerState::Online);
        let dev_vm2 = workload("dev-media.dev.d2b", "dev-media", VmPowerState::Online);

        let mut model = Model::new(Config::default());
        model.apply(ModelEvent::VmSnapshot {
            vms: vec![dev_vm, work_vm, dev_vm2],
        });

        let state = ControlCenterState::from_model(&model);

        // three VMs in flat list
        assert_eq!(state.vms.len(), 3);

        // two realm groups: dev (first) and corp (second)
        assert_eq!(state.realm_groups.len(), 2);
        assert_eq!(state.realm_groups[0].realm, "dev");
        assert_eq!(state.realm_groups[0].workloads.len(), 2);
        assert_eq!(state.realm_groups[0].workloads[0].id, "dev-general");
        assert_eq!(state.realm_groups[0].workloads[1].id, "dev-media");
        assert_eq!(state.realm_groups[1].realm, "corp");
        assert_eq!(state.realm_groups[1].workloads.len(), 1);
        assert_eq!(state.realm_groups[1].workloads[0].id, "work-aad");

        // realm groups are present in the serialized JSON
        let json = state.to_json();
        assert!(json.contains("\"realmGroups\""));
        assert!(json.contains("\"dev\""));
        assert!(json.contains("\"corp\""));
    }

    #[test]
    fn legacy_and_canonical_local_targets_share_local_realm() {
        let no_target = VmSummary::new(vm("home-general"), VmPowerState::Online);
        let local_vm = workload("home-media.local.d2b", "home-media", VmPowerState::Online);

        let mut model = Model::new(Config::default());
        model.apply(ModelEvent::VmSnapshot {
            vms: vec![no_target, local_vm],
        });

        let state = ControlCenterState::from_model(&model);

        // both VMs land in the "local" realm group
        assert_eq!(state.realm_groups.len(), 1);
        assert_eq!(state.realm_groups[0].realm, "local");
        assert_eq!(state.realm_groups[0].workloads.len(), 2);
    }

    #[test]
    fn realm_groups_preserve_discovery_order_across_realms() {
        // VMs are stored in a BTreeMap keyed by VmId, so they are returned in
        // lexicographic order: corp-b < dev-a < home-c → realm order: corp, dev, home.
        let dev = workload("dev-a.dev.d2b", "dev-a", VmPowerState::Online);
        let corp = workload("corp-b.corp.d2b", "corp-b", VmPowerState::Online);
        let home = workload("home-c.home.d2b", "home-c", VmPowerState::Online);

        let mut model = Model::new(Config::default());
        model.apply(ModelEvent::VmSnapshot {
            vms: vec![dev, corp, home],
        });

        let state = ControlCenterState::from_model(&model);
        // BTreeMap returns VMs in alphabetical order: corp-b, dev-a, home-c
        let realm_names: Vec<&str> = state
            .realm_groups
            .iter()
            .map(|g| g.realm.as_str())
            .collect();
        assert_eq!(realm_names, vec!["corp", "dev", "home"]);
    }

    #[test]
    fn async_errors_render_safe_correlation_and_digest() {
        let correlation = SafeCorrelation::new("wlterm-deadbeef").unwrap();
        let event = AsyncErrorEvent::with_correlation(
            "contains quiet-otter and opaque-session-handle",
            AsyncErrorDisplay::Inline,
            correlation,
        );

        let rendered = RenderedAsyncError::from_event(&event).unwrap();
        let json = serde_json::to_string(&rendered).unwrap();
        assert!(json.contains("wlterm-deadbeef"));
        assert!(json.contains("digest"));
        assert!(!json.contains("quiet-otter"));
        assert!(!json.contains("opaque-session-handle"));
    }

    #[test]
    fn silent_async_errors_do_not_render() {
        let event = AsyncErrorEvent::new("late failure", AsyncErrorDisplay::Silent);
        assert!(!event.should_render());
        assert!(RenderedAsyncError::from_event(&event).is_none());
    }

    #[test]
    fn labels_strip_ansi_controls_and_truncate() {
        let raw = "\u{1b}[31mquiet\u{1b}[0m\n-otter";
        assert_eq!(sanitize_display_label(raw), "quiet-otter");
        assert_eq!(
            sanitize_display_label("\u{1b}[31m\n\t"),
            EMPTY_LABEL_PLACEHOLDER
        );

        let long = "a".repeat(DISPLAY_LABEL_MAX_CHARS + 10);
        assert_eq!(
            sanitize_display_label(&long).chars().count(),
            DISPLAY_LABEL_MAX_CHARS
        );
    }

    #[test]
    fn debug_redacts_session_names() {
        let session = SessionId::new("quiet-otter").unwrap();
        let open = format!(
            "{:?}",
            decide_open(&session, true, OpenBehavior::FocusExisting)
        );
        let stop = format!("{:?}", StopRequest::new(&session, true));
        assert!(!open.contains("quiet-otter"));
        assert!(!stop.contains("quiet-otter"));
        assert!(open.contains("redacted"));
        assert!(stop.contains("redacted"));
    }
}
