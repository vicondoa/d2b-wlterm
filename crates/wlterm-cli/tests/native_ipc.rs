use d2b_toolkit_core::{
    PublicRequest, PublicResponse, ShellListEntry, ShellListResult, ShellName, ShellOp,
    ShellOpResponse, ShellSessionState,
};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

fn read_frame(stream: &mut UnixStream) -> Vec<u8> {
    let mut prefix = [0_u8; 4];
    stream.read_exact(&mut prefix).expect("frame length");
    let len = u32::from_le_bytes(prefix) as usize;
    let mut payload = vec![0_u8; len];
    stream.read_exact(&mut payload).expect("frame payload");
    payload
}

fn write_json_frame<T: serde::Serialize>(stream: &mut UnixStream, value: &T) {
    let payload = serde_json::to_vec(value).expect("json frame");
    let len = u32::try_from(payload.len()).expect("frame length fits");
    stream.write_all(&len.to_le_bytes()).expect("write length");
    stream.write_all(&payload).expect("write payload");
    stream.flush().expect("flush frame");
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
    let listener = UnixListener::bind(&socket_path).expect("bind fake daemon");

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept cli");

        let request = read_frame(&mut stream);
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
            &mut stream,
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
        .env("XDG_RUNTIME_DIR", runtime_dir.path())
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
