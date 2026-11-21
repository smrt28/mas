use marsal_a_spion::ai::{AiConfig, run};
use std::{path::Path, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 {
        eprintln!("Usage: mas-ai-client config-ai.toml");
        return ExitCode::FAILURE;
    }
    match AiConfig::load(Path::new(&args[0])).and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
