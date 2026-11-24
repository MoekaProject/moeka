use std::io::{self, Read, Write};
use std::sync::Arc;

use moeka_fs::backend::{FsError, LocalBackend};
use moeka_fs::protocol::{PROTOCOL_VERSION, read_frame, write_frame};
use moeka_fs::service::{BuildIdentity, Envelope, Reply, ServerArtifact, Service};

fn build_identity() -> BuildIdentity {
    BuildIdentity {
        server_version: env!("CARGO_PKG_VERSION").into(),
        app_version: option_env!("MOEKA_REMOTE_APP_VERSION")
            .unwrap_or("dev-unknown")
            .into(),
        source_sha: option_env!("MOEKA_REMOTE_SOURCE_SHA")
            .unwrap_or("dev-unknown")
            .into(),
    }
}

fn serve(input: &mut impl Read, output: &mut impl Write) -> io::Result<()> {
    let mut service = Service::new(Arc::new(LocalBackend::new()), build_identity());
    while let Some(value) = read_frame::<serde_json::Value>(input)? {
        let id = value
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let reply = match serde_json::from_value::<Envelope>(value) {
            Ok(request) => service.dispatch(request),
            Err(error) => Reply {
                id,
                result: Err(FsError::invalid("request", &error.to_string())),
            },
        };
        match write_frame(output, &reply) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
                write_frame(
                    output,
                    &Reply {
                        id,
                        result: Err(FsError::invalid("response", "response exceeds frame limit")),
                    },
                )?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args == ["--artifact-info"] {
        let result = (|| -> Result<String, Box<dyn std::error::Error>> {
            let path = std::env::current_exe()?;
            let revision = LocalBackend::new().revision(&path)?;
            let info = ServerArtifact {
                build: build_identity(),
                target: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
                revision,
            };
            Ok(serde_json::to_string(&info)?)
        })();
        match result {
            Ok(info) => println!("{info}"),
            Err(error) => {
                eprintln!("remote artifact identity error: {error}");
                std::process::exit(1);
            }
        }
    } else if args == ["--version"] {
        let build = build_identity();
        println!(
            "moeka-remote-server {} protocol {} app {} source {}",
            build.server_version, PROTOCOL_VERSION, build.app_version, build.source_sha
        );
    } else if args == ["--stdio"] {
        let stdin = io::stdin();
        let stdout = io::stdout();
        if let Err(error) = serve(&mut stdin.lock(), &mut stdout.lock()) {
            eprintln!("remote protocol error: {error}");
            std::process::exit(1);
        }
    } else {
        eprintln!("usage: moeka-remote-server --stdio | --version | --artifact-info");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moeka_fs::protocol::PROTOCOL_VERSION;
    use moeka_fs::service::{Request, Response};

    #[test]
    fn handshake_and_invalid_requests_use_framed_replies() {
        let mut input = Vec::new();
        write_frame(
            &mut input,
            &Envelope {
                id: 1,
                request: Request::Hello {
                    protocol: PROTOCOL_VERSION + 1,
                    expected_build: None,
                },
            },
        )
        .unwrap();
        write_frame(
            &mut input,
            &Envelope {
                id: 2,
                request: Request::Hello {
                    protocol: PROTOCOL_VERSION,
                    expected_build: Some(build_identity()),
                },
            },
        )
        .unwrap();
        write_frame(
            &mut input,
            &serde_json::json!({"id": 3, "request": {"op": "not_supported"}}),
        )
        .unwrap();
        let mut output = Vec::new();
        serve(&mut input.as_slice(), &mut output).unwrap();
        let mut replies = output.as_slice();
        assert!(
            read_frame::<Reply>(&mut replies)
                .unwrap()
                .unwrap()
                .result
                .is_err()
        );
        assert!(matches!(
            read_frame::<Reply>(&mut replies).unwrap().unwrap().result,
            Ok(Response::Hello { .. })
        ));
        let invalid = read_frame::<Reply>(&mut replies).unwrap().unwrap();
        assert_eq!(invalid.id, 3);
        assert!(invalid.result.is_err());
        assert!(read_frame::<Reply>(&mut replies).unwrap().is_none());
    }
}
