mod doctor;

use std::{ffi::OsString, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    match args.next().as_deref().and_then(|arg| arg.to_str()) {
        Some("doctor") if args.next().is_none() => match doctor::run().await {
            Ok(report) => {
                println!("{report}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("CAIdex doctor failed: {error}");
                ExitCode::FAILURE
            }
        },
        Some("--version") if args.next().is_none() => {
            println!(
                "CAIdex {} (Codex {})",
                env!("CARGO_PKG_VERSION"),
                caidex_runtime::CODEX_VERSION
            );
            ExitCode::SUCCESS
        }
        Some("--help") if args.next().is_none() => {
            println!(
                "CAIdex development CLI\n\nCommands:\n  doctor      Offline check of the pinned Codex app-server\n  --version   Show integration version\n\nFull CLI/Host attach is not implemented yet."
            );
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "Use 'caidex doctor' or 'caidex --help'. Full CLI/Host attach is not implemented yet."
            );
            ExitCode::FAILURE
        }
    }
}

fn codex_binary() -> OsString {
    std::env::var_os("CAIDEX_CODEX_BIN").unwrap_or_else(|| "codex".into())
}
