use std::{env, path::PathBuf, process::ExitCode};

use ilia_inference::{RuntimeManager, RuntimePreference};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime_root = PathBuf::from("runtime");
    let mut backend = RuntimePreference::Auto;
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--runtime-root" => {
                runtime_root = PathBuf::from(args.next().ok_or("missing --runtime-root value")?)
            }
            "--backend" => {
                backend = args
                    .next()
                    .ok_or("missing --backend value")?
                    .parse()
                    .map_err(|error: String| error)?
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    let report = RuntimeManager::probe(runtime_root, backend);
    println!("{}", serde_json::to_string_pretty(&report)?);
    if report.recommended_backend.is_none() {
        return Err("no compatible runtime detected".into());
    }
    Ok(())
}
