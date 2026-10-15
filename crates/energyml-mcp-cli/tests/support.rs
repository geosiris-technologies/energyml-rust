//! A package on disk, and the running program.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use energyml_storage::contract;
use serde_json::{Value, json};

/// How long the test waits for the program before it gives up. Generous: the
/// program starts a runtime and opens a zip, and a CI machine can be slow.
const PATIENCE: Duration = Duration::from_secs(20);

/// A temporary directory that is removed when it goes out of scope.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("energyml-mcp-cli-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("creates the temporary directory");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The contract's model (five objects) as an EPC in `dir`.
pub fn write_package(dir: &Path) -> PathBuf {
    let mut bytes = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut bytes));
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(
            b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
              <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
              <Default Extension=\"rels\" \
              ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
              </Types>",
        )
        .unwrap();
        for object in contract::model().objects {
            zip.start_file(object.part_name().as_str(), options).unwrap();
            zip.write_all(object.xml.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    let path = dir.join("model.epc");
    std::fs::write(&path, bytes).expect("writes the package");
    path
}

/// The program, with its stdout read line by line on a thread so that a test
/// never hangs on a program that stopped answering.
pub struct Program {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
}

impl Program {
    pub fn start(args: &[&str], env: &[(&str, &str)]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_energyml-mcp"));
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("starts the program");
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
            next_id: 0,
        }
    }

    /// Send one message (a line of JSON).
    pub fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().expect("stdin still open");
        writeln!(stdin, "{message}").expect("writes to the program");
        stdin.flush().expect("flushes");
    }

    /// Send a request and wait for its response. **Every line the program
    /// writes on stdout must be JSON**: that is what "stdout is the protocol"
    /// means, and a stray `println!` fails here.
    pub fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let deadline = Instant::now() + PATIENCE;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(remaining) {
                Ok(line) => {
                    let message: Value = serde_json::from_str(&line).unwrap_or_else(|error| {
                        panic!("stdout carries only protocol, got {line:?}: {error}")
                    });
                    if message["id"] == json!(id) {
                        return message;
                    }
                }
                Err(RecvTimeoutError::Timeout) => panic!("no answer to `{method}` within {PATIENCE:?}"),
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("the program closed stdout before answering `{method}`")
                }
            }
        }
    }

    /// The `initialize` exchange of the legacy lifecycle; returns the result.
    pub fn initialize(&mut self) -> Value {
        let answer = self.request(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "energyml-mcp-cli-test", "version": "0" }
            }),
        );
        self.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        answer["result"].clone()
    }

    /// `tools/call`; returns the `result` of the response.
    pub fn call(&mut self, name: &str, arguments: Value) -> Value {
        let answer = self.request("tools/call", json!({ "name": name, "arguments": arguments }));
        assert!(answer.get("error").is_none(), "a tool call is a result: {answer}");
        answer["result"].clone()
    }

    /// Close stdin, wait for the program to end, and return how it ended with
    /// what it wrote on stdout (after what was already read) and on stderr.
    pub fn finish(mut self) -> (ExitStatus, String, String) {
        drop(self.stdin.take());
        let deadline = Instant::now() + PATIENCE;
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("waits") {
                break status;
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                panic!("the program did not end after stdin closed");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let mut rest = String::new();
        while let Ok(line) = self.lines.recv_timeout(Duration::from_millis(200)) {
            rest.push_str(&line);
            rest.push('\n');
        }
        let mut stderr = String::new();
        if let Some(mut pipe) = self.child.stderr.take() {
            let _ = pipe.read_to_string(&mut stderr);
        }
        (status, rest, stderr)
    }
}

impl Drop for Program {
    fn drop(&mut self) {
        // A test that failed must not leave a server behind.
        let _ = self.child.kill();
    }
}

/// Run to the end with these arguments and no input; for the invocations that
/// must fail before serving.
pub fn run_to_end(args: &[&str], env: &[(&str, &str)]) -> (ExitStatus, String, String) {
    Program::start(args, env).finish()
}
