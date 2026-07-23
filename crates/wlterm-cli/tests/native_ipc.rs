use d2b_toolkit_core::{
    HelloOk, HelloResponse, KnownFeatureFlag, PublicRequest, PublicResponse, ShellListEntry,
    ShellListResult, ShellName, ShellOp, ShellOpResponse, ShellSessionState, Version,
    WorkloadListResult, WorkloadOp, WorkloadOpResponse, WorkloadPublicSummary,
};
use nix::sys::socket::{
    accept4, bind, listen, recv, send, socket, AddressFamily, Backlog, MsgFlags, SockFlag,
    SockType, UnixAddr,
};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

fn read_frame(fd: &OwnedFd) -> Vec<u8> {
    let mut packet = vec![0_u8; 1024 * 1024 + 4];
    let packet_len = recv(fd.as_raw_fd(), &mut packet, MsgFlags::empty()).expect("recv packet");
    packet.truncate(packet_len);
    let prefix: [u8; 4] = packet[..4].try_into().expect("frame length");
    let len = u32::from_le_bytes(prefix) as usize;
    packet[4..][..len].to_vec()
}

fn write_json_frame<T: serde::Serialize>(fd: &OwnedFd, value: &T) {
    let mut packet = serde_json::to_vec(value).expect("json frame");
    let len = u32::try_from(packet.len()).expect("frame length fits");
    let mut framed = len.to_le_bytes().to_vec();
    framed.append(&mut packet);
    let sent = send(fd.as_raw_fd(), &framed, MsgFlags::empty()).expect("send packet");
    assert_eq!(sent, framed.len());
}

fn bind_seqpacket(path: &PathBuf) -> OwnedFd {
    let fd = socket(
        AddressFamily::Unix,
        SockType::SeqPacket,
        SockFlag::SOCK_CLOEXEC,
        None,
    )
    .expect("create seqpacket socket");
    let addr = UnixAddr::new(path).expect("socket path");
    bind(fd.as_raw_fd(), &addr).expect("bind fake daemon");
    listen(&fd, Backlog::new(1).expect("backlog")).expect("listen fake daemon");
    fd
}

fn accept_seqpacket(listener: &OwnedFd) -> OwnedFd {
    let raw = accept4(listener.as_raw_fd(), SockFlag::SOCK_CLOEXEC).expect("accept cli");
    // SAFETY: accept4 returns a fresh owned file descriptor on success.
    unsafe { OwnedFd::from_raw_fd(raw) }
}

fn unique_runtime_dir() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("d2b-wlterm-ipc-{suffix}-{}", std::process::id()))
}

struct RuntimeDir {
    path: PathBuf,
}

impl RuntimeDir {
    fn create() -> Self {
        let path = unique_runtime_dir();
        std::fs::create_dir_all(path.join("d2b")).expect("socket dir");
        Self { path }
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for RuntimeDir {
    fn drop(&mut self) {
        if self.path.exists() {
            if let Err(err) = std::fs::remove_dir_all(&self.path) {
                if std::thread::panicking() {
                    eprintln!(
                        "failed to clean up runtime dir {} during unwind: {err}",
                        self.path.display()
                    );
                } else {
                    panic!("cleanup runtime dir {}: {err}", self.path.display());
                }
            }
        }
    }
}

#[test]
fn cli_list_uses_real_public_socket_frames() {
    let runtime_dir = RuntimeDir::create();
    let socket_path = runtime_dir.path().join("d2b").join("public.sock");
    let listener = bind_seqpacket(&socket_path);

    let server = thread::spawn(move || {
        let inventory = accept_seqpacket(&listener);
        serve_hello(&inventory);
        let request: PublicRequest =
            serde_json::from_slice(&read_frame(&inventory)).expect("inventory request");
        let op_id = match request {
            PublicRequest::Workload {
                op_id,
                op: WorkloadOp::List(_),
            } => op_id,
            other => panic!("unexpected inventory request: {other:?}"),
        };
        write_json_frame(
            &inventory,
            &PublicResponse::Workload {
                op_id,
                response: WorkloadOpResponse::List(WorkloadListResult {
                    workloads: vec![shell_workload()],
                }),
            },
        );

        let list = accept_seqpacket(&listener);
        serve_hello(&list);
        serve_shell_list(&list);
    });

    let output = Command::new(env!("CARGO_BIN_EXE_d2b-wlterm"))
        .env("D2B_PUBLIC_SOCKET", &socket_path)
        .env(
            "D2B_WLTERM_CONFIG",
            runtime_dir.path().join("missing-config.toml"),
        )
        .arg("list")
        .arg("corp-vm")
        .output()
        .expect("run d2b-wlterm");

    server.join().expect("fake daemon thread");

    assert!(
        output.status.success(),
        "status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("default\tdetached\tdefault"), "{stdout}");
    assert!(stdout.contains("build\tattached"), "{stdout}");
}

fn serve_hello(stream: &OwnedFd) {
    let hello: serde_json::Value = serde_json::from_slice(&read_frame(stream)).expect("hello json");
    assert_eq!(
        hello.get("type").and_then(serde_json::Value::as_str),
        Some("hello")
    );
    write_json_frame(
        stream,
        &HelloResponse::HelloOk(HelloOk {
            server_version: Version::new("0.4.0"),
            selected_version: Version::new("0.4.0"),
            capabilities: vec![
                KnownFeatureFlag::TypedErrors.wire_value(),
                KnownFeatureFlag::ConfiguredLaunchV1.wire_value(),
                KnownFeatureFlag::UnsafeLocalProviderV1.wire_value(),
                KnownFeatureFlag::UnsafeLocalShellV1.wire_value(),
            ],
        }),
    );
}

fn serve_shell_list(stream: &OwnedFd) {
    let request: PublicRequest =
        serde_json::from_slice(&read_frame(stream)).expect("shell request");
    let op_id = match request {
        PublicRequest::Shell {
            op_id,
            op: ShellOp::List(args),
        } => {
            assert_eq!(args.vm, "corp-vm.work.d2b");
            op_id
        }
        other => panic!("unexpected request: {other:?}"),
    };
    write_json_frame(
        stream,
        &PublicResponse::Shell {
            op_id,
            response: ShellOpResponse::List(ShellListResult {
                default_name: ShellName::new("default").unwrap(),
                sessions: vec![
                    ShellListEntry {
                        name: ShellName::new("default").unwrap(),
                        state: ShellSessionState::Detached,
                        attached: false,
                        is_default: true,
                    },
                    ShellListEntry {
                        name: ShellName::new("build").unwrap(),
                        state: ShellSessionState::Attached,
                        attached: true,
                        is_default: false,
                    },
                ],
            }),
        },
    );
}

fn shell_workload() -> WorkloadPublicSummary {
    serde_json::from_value(serde_json::json!({
        "identity": {
            "workloadId": "corp-vm",
            "workloadName": "Corporate VM",
            "realmId": "work",
            "realmPath": ["work"],
            "canonicalTarget": "corp-vm.work.d2b",
            "legacyVmName": "corp-vm",
            "runtimeKind": "nixos",
            "providerId": "local-cloud-hypervisor"
        },
        "providerKind": "local-vm",
        "state": "running",
        "executionPosture": {
            "isolation": "virtual-machine",
            "environment": "runtime-managed",
            "displayEnvironment": "runtime-managed",
            "executionIdentity": "workload-user",
            "sessionPersistence": "runtime-managed"
        },
        "availability": "ready",
        "graphicalPosture": "proxied",
        "capabilities": ["configured-launch", "persistent-shell", "pty", "window-forwarding"],
        "launcherItems": [{
            "id": "terminal",
            "name": "Terminal",
            "icon": {"name": "terminal"},
            "type": "shell",
            "graphical": false,
            "capabilities": ["persistent-shell", "pty"]
        }]
    }))
    .expect("valid toolkit fixture")
}
