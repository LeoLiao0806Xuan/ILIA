use std::{env, fs, path::PathBuf, process::ExitCode, thread, time::Duration};

use ilia_updater::{
    ProxyConfig, TrustedPublicKey, UpdateEngine, create_local_package,
    fetch_verified_release_with_proxy, load_local_package,
};

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
        .ok_or("missing command: use apply, apply-package, rollback, or pack")?;
    let mut root = None;
    let mut manifest_url = None;
    let mut signature_url = None;
    let mut public_key = None;
    let mut wait_pid = None;
    let mut package = None;
    let mut output = None;
    let mut manifest = None;
    let mut signature = None;
    let mut proxy_config = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--root" => root = args.next().map(PathBuf::from),
            "--manifest-url" => manifest_url = args.next(),
            "--signature-url" => signature_url = args.next(),
            "--public-key" => public_key = args.next().map(PathBuf::from),
            "--wait-pid" => wait_pid = args.next().and_then(|value| value.parse::<u32>().ok()),
            "--package" => package = args.next().map(PathBuf::from),
            "--output" => output = args.next().map(PathBuf::from),
            "--manifest" => manifest = args.next().map(PathBuf::from),
            "--signature" => signature = args.next().map(PathBuf::from),
            "--proxy-config" => proxy_config = args.next().map(PathBuf::from),
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    if command == "pack" {
        create_local_package(
            &output.ok_or("missing --output")?,
            &manifest.ok_or("missing --manifest")?,
            &signature.ok_or("missing --signature")?,
        )?;
        return Ok(());
    }
    let root = root.ok_or("missing --root")?;
    let public_key = public_key.ok_or("missing --public-key")?;
    let trusted: TrustedPublicKey = serde_json::from_slice(&fs::read(public_key)?)?;
    let proxy: Option<ProxyConfig> = proxy_config
        .map(fs::read)
        .transpose()?
        .map(|bytes| serde_json::from_slice(&bytes))
        .transpose()?;
    let engine = UpdateEngine::new(root)?;

    match command.as_str() {
        "apply" => {
            let manifest_url = manifest_url.ok_or("missing --manifest-url")?;
            let signature_url = signature_url.ok_or("missing --signature-url")?;
            let release = fetch_verified_release_with_proxy(
                &manifest_url,
                &signature_url,
                &trusted,
                proxy.as_ref(),
            )?;
            let stage = engine.stage(&release)?;
            if let Some(pid) = wait_pid {
                wait_for_process_exit(pid, Duration::from_secs(120))?;
            }
            let outcome = engine.apply(&release, &stage)?;
            println!("{}", serde_json::to_string_pretty(&outcome)?);
        }
        "apply-package" => {
            let package = package.ok_or("missing --package")?;
            let extraction = package.with_extension("ilia-stage");
            let (release, stage) = load_local_package(&package, &trusted, &extraction)?;
            if let Some(pid) = wait_pid {
                wait_for_process_exit(pid, Duration::from_secs(120))?;
            }
            let outcome = engine.apply(&release, &stage)?;
            println!("{}", serde_json::to_string_pretty(&outcome)?);
        }
        "rollback" => {
            let manifest_url = manifest_url.ok_or("missing --manifest-url")?;
            let signature_url = signature_url.ok_or("missing --signature-url")?;
            let release = fetch_verified_release_with_proxy(
                &manifest_url,
                &signature_url,
                &trusted,
                proxy.as_ref(),
            )?;
            engine.rollback(&release.manifest)?;
        }
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
