//! Lists every configured host once and prints what the sidebar would show:
//! `cargo run -p client-core --example list [HOST…]` (hosts given here replace `~/.config/tmls/hosts`).
//! Session names only: no titles, nothing from inside a session.

use std::path::PathBuf;
use std::time::Duration;

use client_core::config::Config;
use client_core::listing::listing_command;
use client_core::poll::list_once;

fn main() {
    let home = std::env::var_os("HOME").unwrap_or_default();
    let dir = PathBuf::from(home).join(".config/tmls");
    let cli_hosts: Vec<String> = std::env::args().skip(1).collect();
    let cfg = Config::load(&dir, &cli_hosts);
    for host in &cfg.hosts {
        let snap = list_once(
            host,
            listing_command(host, &cfg.local_name),
            Duration::from_secs(8),
        );
        if snap.online {
            println!("{host}: online, {} sessions", snap.sessions.len());
        } else {
            println!(
                "{host}: offline ({})",
                snap.error.as_deref().unwrap_or("no reason given")
            );
        }
        for s in &snap.sessions {
            println!(
                "  {} {} {}",
                s.status.mark(),
                s.tool.letter().unwrap_or('-'),
                s.key.name
            );
        }
    }
}
