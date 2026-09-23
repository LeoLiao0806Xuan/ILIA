use std::{env, fs, path::PathBuf, process::ExitCode};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ilia_updater::{DetachedSignature, TrustedPublicKey};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct PrivateKeyFile {
    key_id: String,
    algorithm: String,
    private_key_base64: String,
}

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
    match args.next().as_deref() {
        Some("keygen") => {
            let private_path = PathBuf::from(args.next().ok_or("missing private key path")?);
            let public_path = PathBuf::from(args.next().ok_or("missing public key path")?);
            let key_id = args.next().unwrap_or_else(|| "ilia-release-1".into());
            let random = SystemRandom::new();
            let private_key = Ed25519KeyPair::generate_pkcs8(&random)
                .map_err(|_| "failed to generate Ed25519 key")?;
            let signing = Ed25519KeyPair::from_pkcs8(private_key.as_ref())
                .map_err(|_| "generated Ed25519 key was rejected")?;
            let private = PrivateKeyFile {
                key_id: key_id.clone(),
                algorithm: "ed25519".into(),
                private_key_base64: STANDARD.encode(private_key.as_ref()),
            };
            let public = TrustedPublicKey {
                key_id,
                algorithm: "ed25519".into(),
                public_key_base64: STANDARD.encode(signing.public_key().as_ref()),
            };
            write_private(&private_path, &serde_json::to_vec_pretty(&private)?)?;
            write_public(&public_path, &serde_json::to_vec_pretty(&public)?)?;
        }
        Some("sign") => {
            let private_path = PathBuf::from(args.next().ok_or("missing private key path")?);
            let manifest_path = PathBuf::from(args.next().ok_or("missing manifest path")?);
            let signature_path = PathBuf::from(args.next().ok_or("missing signature path")?);
            let private: PrivateKeyFile = serde_json::from_slice(&fs::read(private_path)?)?;
            if private.algorithm != "ed25519" {
                return Err("unsupported key algorithm".into());
            }
            let private_key = STANDARD.decode(private.private_key_base64)?;
            let signing = Ed25519KeyPair::from_pkcs8(&private_key)
                .map_err(|_| "private Ed25519 key was rejected")?;
            let manifest = fs::read(manifest_path)?;
            let signature = DetachedSignature {
                key_id: private.key_id,
                algorithm: "ed25519".into(),
                signature_base64: STANDARD.encode(signing.sign(&manifest).as_ref()),
            };
            write_public(&signature_path, &serde_json::to_vec_pretty(&signature)?)?;
        }
        _ => return Err("usage: ilia-update-sign keygen <private> <public> [key-id] | sign <private> <manifest> <signature>".into()),
    }
    Ok(())
}

fn write_private(path: &PathBuf, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        return Err(format!("refusing to overwrite private key: {}", path.display()).into());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

fn write_public(path: &PathBuf, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}
