use std::{net::UdpSocket, path::Path, process::Command};

use crate::runtime::{self, Platform};

pub fn doctor(work: &Path) {
    println!("LAN preservation boundary (diagnostics only; play is not implemented):");
    match UdpSocket::bind("0.0.0.0:0") {
        Ok(socket) => println!(
            "  [ok] local UDP bind: {}",
            socket
                .local_addr()
                .map_or_else(|_| "available".to_owned(), |address| address.to_string())
        ),
        Err(error) => println!("  [missing] local UDP bind: {error}"),
    }
    match Command::new("ffmpeg").arg("-version").output() {
        Ok(output) if output.status.success() => println!("  [ok] ffmpeg available"),
        Ok(output) => println!("  [missing] ffmpeg failed with {}", output.status),
        Err(error) => println!("  [missing] ffmpeg unavailable: {error}"),
    }
    match Platform::current()
        .map_err(anyhow::Error::msg)
        .and_then(|platform| runtime::locate(work, platform))
    {
        Ok(path) => println!("  [ok] pinned host runtime: {}", path.display()),
        Err(error) => println!("  [missing] pinned host runtime: {error:#}"),
    }
    println!("  [not implemented] discovery, input transport, media stream, reconnect, security");
    println!("LAN readiness: unavailable by design in 0.1.0");
}
