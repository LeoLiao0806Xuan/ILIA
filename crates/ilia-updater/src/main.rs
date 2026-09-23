use std::{env, fs, path::PathBuf, process::ExitCode, thread, time::Duration};

use ilia_updater::{TrustedPublicKey, UpdateEngine, fetch_verified_release};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let command = args
        .next()
        .ok_or("missing command: use apply or rollback")?;
    let mut root = None;
    let mut manifest_url = None;
    let mut signature_url = None;
    let mut public_key = None;
    let mut wait_pid = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--root" => root = args.next().map(PathBuf::from),
            "--manifest-url" => manifest_url = args.next(),
            "--signature-url" => signature_url = args.next(),
            "--public-key" => public_key = args.next().map(PathBuf::from),
            "--wait-pid" => wait_pid = args.next().and_then(|value| value.parse::<u32>().ok()),
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    let root = root.ok_or("missing --root")?;
    let manifest_url = manifest_url.ok_or("missing --manifest-url")?;
    let signature_url = signature_url.ok_or("missing --signature-url")?;
    let public_key = public_key.ok_or("missing --public-key")?;
    let trusted: TrustedPublicKey = serde_json::from_slice(&fs::read(public_key)?)?;
    let release = fetch_verified_release(&manifest_url, &signature_url, &trusted)?;
    let engine = UpdateEngine::new(root)?;

    match command.as_str() {
        "apply" => {
            let stage = engine.stage(&release)?;
            if let Some(pid) = wait_pid {
                wait_for_process_exit(pid, Duration::from_secs(120))?;
            }
            let outcome = engine.apply(&release, &stage)?;
            println!("{}", serde_json::to_string_pretty(&outcome)?);
        }
        "rollback" => engine.rollback(&release.manifest)?,
        _ => return Err(format!("unknown command: {command}").into()),
    }
    Ok(())
}

fn wait_for_process_exit(pid: u32, timeout: Duration) -> Result<(), String> {
    let started = std::time::Instant::now();
    while started.elapsed() < timeout {
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .map_err(|error| error.to_string())?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains("INFO:") || !stdout.contains(&pid.to_string()) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(format!("timed out waiting for process {pid} to exit"))
}
