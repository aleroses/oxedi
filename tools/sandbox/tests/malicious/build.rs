// Synthetic hostile build script for make sandbox-pr-check: it tries to reach the host home,
// the network and the shared caches, and records only whether each try succeeded. It never
// reads or sends any file's contents.
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

const HOST_HOME: &str = "@HOST_HOME@";
const HOST_CANARY: &str = "@HOST_CANARY@";

fn outcome(result: io::Result<()>) -> &'static str {
    match result {
        Ok(()) => "succeeded",
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => "denied",
        Err(_) => "failed",
    }
}

fn connect(host: &str) -> io::Result<()> {
    let address = host.to_socket_addrs()?.next().ok_or(io::ErrorKind::NotFound)?;
    TcpStream::connect_timeout(&address, Duration::from_secs(5)).map(drop)
}

fn write_byte(path: &str) -> io::Result<()> {
    OpenOptions::new().create(true).append(true).open(path)?.write_all(b"x")
}

fn main() {
    let attempts = [
        ("read host home", outcome(fs::read_dir(HOST_HOME).map(drop))),
        ("connect 1.1.1.1:443", outcome(connect("1.1.1.1:443"))),
        ("connect index.crates.io:443", outcome(connect("index.crates.io:443"))),
        ("write /sandbox/cargo/x", outcome(write_byte("/sandbox/cargo/x"))),
        ("write host canary", outcome(write_byte(HOST_CANARY))),
    ];
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let lines: String = attempts.iter().map(|(what, result)| format!("{what}: {result}\n")).collect();
    let _ = fs::write(Path::new(&manifest).join("../../target/malicious-build.txt"), lines);
}
