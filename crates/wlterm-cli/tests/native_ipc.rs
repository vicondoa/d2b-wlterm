use d2b_toolkit_core::{
    FeatureFlag, HelloOk, HelloResponse, PublicRequest, PublicResponse, ShellListEntry,
    ShellListResult, ShellName, ShellOp, ShellOpResponse, ShellSessionState, Version,
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
    std::env::temp_dir().join(format!("d2b-wlterm-ipc-{suffix}-{}", std::process::id()))
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
        let stream = accept_seqpacket(&listener);

        let hello = read_frame(&stream);
        let hello: serde_json::Value = serde_json::from_slice(&hello).expect("hello json");
        assert_eq!(
            hello.get("type").and_then(serde_json::Value::as_str),
            Some("hello")
        );
        write_json_frame(
            &stream,
            &HelloResponse::HelloOk(HelloOk {
                server_version: Version::new("0.4.0"),
                selected_version: Version::new("0.4.0"),
                capabilities: vec![FeatureFlag::new("typed-errors")],
            }),
        );

        let request = read_frame(&stream);
        let request: PublicRequest = serde_json::from_slice(&request).expect("public request");
        let (op_id, vm) = match request {
            PublicRequest::Shell {
                op_id,
                op: ShellOp::List(args),
            } => (op_id, args.vm),
            other => panic!("unexpected request: {other:?}"),
        };
        assert_eq!(vm, "work");

        write_json_frame(
            &stream,
            &PublicResponse::Shell {
                op_id,
                response: ShellOpResponse::List(ShellListResult {
                    default_name: ShellName::new("default"),
                    sessions: vec![
                        ShellListEntry {
                            name: ShellName::new("default"),
                            state: ShellSessionState::Detached,
                            attached: false,
                            is_default: true,
                        },
                        ShellListEntry {
                            name: ShellName::new("build"),
                            state: ShellSessionState::Attached,
                            attached: true,
                            is_default: false,
                        },
                    ],
                }),
            },
        );
    });

    let output = Command::new(env!("CARGO_BIN_EXE_d2b-wlterm"))
        .env("D2B_PUBLIC_SOCKET", &socket_path)
        .arg("list")
        .arg("work")
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
