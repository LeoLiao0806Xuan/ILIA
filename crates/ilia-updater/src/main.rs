use std::{env, fs, path::PathBuf, process::ExitCode, thread, time::Duration};

use ilia_updater::{
    ProxyConfig, TrustedPublicKey, UpdateEngine, UpdateProgress, create_local_package,
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
    let mut restart = false;
    let mut progress_file = None;
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
            "--restart" => restart = true,
            "--progress-file" => progress_file = args.next().map(PathBuf::from),
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
    let restart_executable = root.join("ilia-desktop.exe");
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
            write_progress(
                progress_file.as_deref(),
                &progress("checking", "", None, 0, 0, "正在验证更新清单和签名"),
            )?;
            let manifest_url = manifest_url.ok_or("missing --manifest-url")?;
            let signature_url = signature_url.ok_or("missing --signature-url")?;
            let release = fetch_verified_release_with_proxy(
                &manifest_url,
                &signature_url,
                &trusted,
                proxy.as_ref(),
            )?;
            let stage =
                engine.stage_with_proxy_and_progress(&release, proxy.as_ref(), |value| {
                    let _ = write_progress(progress_file.as_deref(), value);
                })?;
            write_progress(
                progress_file.as_deref(),
                &progress(
                    "ready_to_apply",
                    &release.manifest.release_id,
                    None,
                    1,
                    1,
                    "更新已下载并验证，正在准备安装",
                ),
            )?;
            if let Some(pid) = wait_pid {
                wait_for_process_exit(pid, Duration::from_secs(120))?;
            }
            write_progress(
                progress_file.as_deref(),
                &progress(
                    "applying",
                    &release.manifest.release_id,
                    None,
                    1,
                    1,
                    "正在安装更新",
                ),
            )?;
            let outcome = engine.apply(&release, &stage)?;
            println!("{}", serde_json::to_string_pretty(&outcome)?);
            write_progress(
                progress_file.as_deref(),
                &progress(
                    "applied",
                    &release.manifest.release_id,
                    None,
                    1,
                    1,
                    "更新安装完成",
                ),
            )?;
            if restart {
                restart_desktop(&restart_executable)?;
            }
        }
        "apply-package" => {
            write_progress(
                progress_file.as_deref(),
                &progress("checking", "", None, 0, 0, "正在读取并验证本地签名更新包"),
            )?;
            let package = package.ok_or("missing --package")?;
            let extraction = package.with_extension("ilia-stage");
            let (release, stage) = load_local_package(&package, &trusted, &extraction)?;
            write_progress(
                progress_file.as_deref(),
                &progress(
                    "ready_to_apply",
                    &release.manifest.release_id,
                    None,
                    1,
                    1,
                    "本地更新包已验证，正在准备安装",
                ),
            )?;
            if let Some(pid) = wait_pid {
                wait_for_process_exit(pid, Duration::from_secs(120))?;
            }
            write_progress(
                progress_file.as_deref(),
                &progress(
                    "applying",
                    &release.manifest.release_id,
                    None,
                    1,
                    1,
                    "正在安装更新",
                ),
            )?;
            let outcome = engine.apply(&release, &stage)?;
            println!("{}", serde_json::to_string_pretty(&outcome)?);
            write_progress(
                progress_file.as_deref(),
                &progress(
                    "applied",
                    &release.manifest.release_id,
                    None,
                    1,
                    1,
                    "更新安装完成",
                ),
            )?;
            if restart {
                restart_desktop(&restart_executable)?;
            }
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

fn progress(
    phase: &str,
    release_id: &str,
    component_id: Option<String>,
    downloaded_bytes: u64,
    total_bytes: u64,
    message_zh: &str,
) -> UpdateProgress {
    UpdateProgress {
        phase: phase.into(),
        release_id: release_id.into(),
        component_id,
        downloaded_bytes,
        total_bytes,
        message_zh: message_zh.into(),
    }
}

fn write_progress(
    path: Option<&std::path::Path>,
    progress: &UpdateProgress,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(path) = path else { return Ok(()) };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.new");
    fs::write(&temporary, serde_json::to_vec(progress)?)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn restart_desktop(executable: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    if !executable.is_file() {
        return Err(format!(
            "updated desktop executable is missing: {}",
            executable.display()
        )
        .into());
    }
    std::process::Command::new(executable).spawn()?;
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
