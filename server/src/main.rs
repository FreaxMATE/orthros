//! orthros-server binary: configuration around orthros_server::app.
//!
//! Everything has a default, so `orthros-server` alone is a working server:
//! data in the platform data dir, 127.0.0.1:8787, and a token generated on
//! first start (printed once, kept in <data>/token).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use orthros_server::{app, Space};

const USAGE: &str = "\
orthros-server: sync server for Orthros

usage:
  orthros-server [--data DIR] [--listen ADDR:PORT] [--token-file FILE]
  orthros-server token [--data DIR]      print the token

defaults (or environment variables):
  --data        ORTHROS_DATA        ~/.local/share/orthros-server (macOS: ~/Library/Application Support/orthros-server)
  --listen      ORTHROS_LISTEN      127.0.0.1:8787
  --token-file  ORTHROS_TOKEN_FILE  <data>/token, generated on first start
                ORTHROS_TOKEN       the token itself, instead of a file
";

struct Args {
    data: PathBuf,
    listen: String,
    token_file: Option<PathBuf>,
    print_token: bool,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn args() -> Result<Args, String> {
    let mut a = Args {
        data: env("ORTHROS_DATA").map(PathBuf::from).unwrap_or_else(|| {
            dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("orthros-server")
        }),
        listen: env("ORTHROS_LISTEN").unwrap_or_else(|| "127.0.0.1:8787".into()),
        token_file: env("ORTHROS_TOKEN_FILE").map(PathBuf::from),
        print_token: false,
    };
    let mut it = std::env::args().skip(1).peekable();
    if it.peek().map(String::as_str) == Some("token") {
        it.next();
        a.print_token = true;
    }
    while let Some(flag) = it.next() {
        let mut val = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--data" => a.data = val()?.into(),
            "--listen" => a.listen = val()?,
            "--token-file" => a.token_file = Some(val()?.into()),
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("orthros-server {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}\n\n{USAGE}")),
        }
    }
    Ok(a)
}

fn new_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("no randomness for a token: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Write a secret readable by its owner only.
fn write_private(path: &Path, content: &str) -> std::io::Result<()> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    opts.open(path)?.write_all(format!("{content}\n").as_bytes())
}

/// The token, from (in order) --token-file, ORTHROS_TOKEN, or <data>/token,
/// which is created on first start. Returns it and whether it is new.
fn token(a: &Args) -> Result<(String, bool), String> {
    let read = |p: &Path| fs::read_to_string(p).map(|t| t.trim().to_string()).map_err(|e| format!("reading {}: {e}", p.display()));
    let (token, new) = if let Some(file) = &a.token_file {
        (read(file)?, false)
    } else if let Some(t) = env("ORTHROS_TOKEN") {
        (t, false)
    } else {
        let file = a.data.join("token");
        if file.exists() {
            (read(&file)?, false)
        } else {
            fs::create_dir_all(&a.data).map_err(|e| format!("creating {}: {e}", a.data.display()))?;
            let t = new_token()?;
            write_private(&file, &t).map_err(|e| format!("writing {}: {e}", file.display()))?;
            (t, true)
        }
    };
    if token.len() < 16 {
        return Err("the token must be at least 16 characters".into());
    }
    Ok((token, new))
}

async fn shutdown() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => {},
            _ = term.recv() => {},
        }
    }
    #[cfg(not(unix))]
    let _ = ctrl_c.await;
}

#[tokio::main]
async fn main() {
    let run = async {
        let a = args()?;
        let (token, new) = token(&a)?;
        if a.print_token {
            println!("{token}");
            return Ok(());
        }
        let space = Arc::new(Space::open(a.data.clone()).map_err(|e| format!("opening {}: {e}", a.data.display()))?);
        let listener = tokio::net::TcpListener::bind(&a.listen).await.map_err(|e| format!("binding {}: {e}", a.listen))?;
        println!("orthros-server {}: serving {} on {}", env!("CARGO_PKG_VERSION"), a.data.display(), a.listen);
        if new {
            println!(
                "\nNew sync token (kept in {}):\n\n  {token}\n\n\
                 Connect each device (with this server's HTTPS address):\n\n  \
                 orthros --connect https://<this server> {token}\n",
                a.data.join("token").display()
            );
        }
        axum::serve(listener, app(space, token)).with_graceful_shutdown(shutdown()).await.map_err(|e| e.to_string())
    };
    if let Err(e) = run.await {
        eprintln!("orthros-server: {e}");
        std::process::exit(1);
    }
}
