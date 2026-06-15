use gls_server::{ServerOptions, run_server};
use std::path::PathBuf;

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", help());
        return Ok(());
    }
    let mut host = "127.0.0.1".to_string();
    let mut port = 19090u16;
    let mut artifact_dir = PathBuf::from("outputs/webui_runtime");
    let mut workspace_root = std::env::current_dir().map_err(|err| err.to_string())?;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--host" => {
                host = args
                    .get(index + 1)
                    .ok_or_else(|| "missing_value:--host".to_string())?
                    .to_string();
                index += 2;
            }
            "--port" => {
                port = args
                    .get(index + 1)
                    .ok_or_else(|| "missing_value:--port".to_string())?
                    .parse()
                    .map_err(|_| "invalid_port".to_string())?;
                index += 2;
            }
            "--artifact-dir" => {
                artifact_dir = PathBuf::from(
                    args.get(index + 1)
                        .ok_or_else(|| "missing_value:--artifact-dir".to_string())?,
                );
                index += 2;
            }
            "--workspace-root" => {
                workspace_root = PathBuf::from(
                    args.get(index + 1)
                        .ok_or_else(|| "missing_value:--workspace-root".to_string())?,
                );
                index += 2;
            }
            other => return Err(format!("unknown_gls_server_arg:{other}")),
        }
    }
    let options = ServerOptions {
        host,
        port,
        artifact_dir,
        workspace_root,
    };
    run_server(&options)?;
    Ok(())
}

fn help() -> &'static str {
    "gls-server [--host 127.0.0.1] [--port 19090] [--artifact-dir DIR] [--workspace-root DIR]"
}
