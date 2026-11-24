use std::io;
use std::path::PathBuf;

use moeka_fs::local::native as fs;
use moeka_fs::service::{BuildIdentity, ServerArtifact};
use moeka_ssh::bundle::{TARGETS, create};
use sha2::{Digest, Sha256};

fn run() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 5 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: moeka-remote-bundle OUTPUT APP_VERSION SERVER_VERSION SOURCE_SHA PAYLOAD_DIRECTORY",
        ));
    }
    let text = |index: usize| {
        args[index].to_str().map(str::to_owned).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "build identities must be UTF-8",
            )
        })
    };
    let build = BuildIdentity {
        app_version: text(1)?,
        server_version: text(2)?,
        source_sha: text(3)?,
    };
    let directory = PathBuf::from(&args[4]);
    let mut payloads = Vec::new();
    for target in TARGETS {
        let path = directory.join(format!("{}.bin", target.name()));
        let bytes = fs::read(&path).map_err(|error| {
            io::Error::new(error.kind(), format!("read {}: {error}", path.display()))
        })?;
        let provenance = directory.join(format!("{}.json", target.name()));
        let info: ServerArtifact = serde_json::from_slice(&fs::read(&provenance)?)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        if info.build != build
            || info.target != target.name()
            || info.revision.size != bytes.len() as u64
            || info.revision.sha256 != digest
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("remote artifact identity mismatch for {}", target.name()),
            ));
        }
        payloads.push((target, bytes));
    }
    let inputs: Vec<_> = payloads
        .iter()
        .map(|(target, bytes)| (*target, bytes.as_slice()))
        .collect();
    let bundle = create(build, &inputs)?;
    let output = PathBuf::from(&args[0]);
    if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, bundle)?;
    println!("Created verified four-target bundle {}", output.display());
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
