//! orthros-server binary: argument parsing around orthros_server::app.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use orthros_server::{app, Space};

struct Args {
    data: PathBuf,
    listen: String,
    token_file: PathBuf,
}

fn args() -> Result<Args, String> {
    let mut a = Args { data: "/var/lib/orthros".into(), listen: "127.0.0.1:8444".into(), token_file: PathBuf::new() };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut val = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--data" => a.data = val()?.into(),
            "--listen" => a.listen = val()?,
            "--token-file" => a.token_file = val()?.into(),
            "-h" | "--help" => {
                println!("usage: orthros-server --token-file FILE [--data DIR] [--listen ADDR:PORT]");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if a.token_file.as_os_str().is_empty() {
        return Err("--token-file is required".into());
    }
    Ok(a)
}

#[tokio::main]
async fn main() {
    let run = async {
        let a = args()?;
        let token = fs::read_to_string(&a.token_file).map_err(|e| format!("reading {}: {e}", a.token_file.display()))?;
        let token = token.trim().to_string();
        if token.len() < 16 {
            return Err("the token must be at least 16 characters".to_string());
        }
        let space = Arc::new(Space::open(a.data.clone()).map_err(|e| format!("opening {}: {e}", a.data.display()))?);
        let listener = tokio::net::TcpListener::bind(&a.listen).await.map_err(|e| format!("binding {}: {e}", a.listen))?;
        println!("orthros-server: serving {} on {}", a.data.display(), a.listen);
        axum::serve(listener, app(space, token))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
            .map_err(|e| e.to_string())
    };
    if let Err(e) = run.await {
        eprintln!("orthros-server: {e}");
        std::process::exit(1);
    }
}
