use std::io::{self, Read, Write};

use flate2::{Compression, GzBuilder, read::GzDecoder};
use moeka_fs::protocol::PROTOCOL_VERSION;
use moeka_fs::service::BuildIdentity;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{MAX_PAYLOAD_BYTES, ServerPayload, ServerTarget};

const MAGIC: &[u8; 8] = b"MOEKAR01";
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const TARGETS: [ServerTarget; 4] = [
    ServerTarget::LinuxX86_64,
    ServerTarget::LinuxAarch64,
    ServerTarget::MacosX86_64,
    ServerTarget::MacosAarch64,
];

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub build: BuildIdentity,
    pub protocol: u32,
    pub payloads: Vec<PayloadManifest>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PayloadManifest {
    pub target: ServerTarget,
    pub offset: usize,
    pub compressed_bytes: usize,
    pub uncompressed_bytes: usize,
    pub sha256: String,
}

pub struct Bundle<'a> {
    manifest: Manifest,
    compressed: &'a [u8],
}

pub struct OwnedPayload {
    build: BuildIdentity,
    target: ServerTarget,
    sha256: String,
    bytes: Vec<u8>,
}

impl OwnedPayload {
    pub fn as_payload(&self) -> ServerPayload<'_> {
        ServerPayload {
            target: self.target,
            version: &self.build.app_version,
            sha256: &self.sha256,
            bytes: &self.bytes,
        }
    }

    pub fn build(&self) -> &BuildIdentity {
        &self.build
    }
}

impl<'a> Bundle<'a> {
    pub fn parse(bytes: &'a [u8]) -> io::Result<Self> {
        if bytes.len() < 12 || bytes.get(..8) != Some(MAGIC.as_slice()) {
            return Err(invalid("invalid remote bundle header"));
        }
        let length = u32::from_le_bytes(bytes[8..12].try_into().expect("checked prefix")) as usize;
        if length == 0 || length > MAX_MANIFEST_BYTES || length > bytes.len() - 12 {
            return Err(invalid("invalid remote bundle manifest length"));
        }
        let manifest: Manifest = serde_json::from_slice(&bytes[12..12 + length])
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if manifest.protocol != PROTOCOL_VERSION
            || manifest.payloads.is_empty()
            || manifest.payloads.len() > TARGETS.len()
        {
            return Err(invalid("empty or incompatible remote bundle"));
        }
        validate_build(&manifest.build)?;
        let compressed = &bytes[12 + length..];
        let mut offset = 0_usize;
        let mut next_target = 0_usize;
        for payload in &manifest.payloads {
            // Payloads follow `TARGETS` order, so duplicates are impossible.
            let Some(position) = TARGETS[next_target..]
                .iter()
                .position(|target| *target == payload.target)
            else {
                return Err(invalid("invalid remote payload manifest"));
            };
            next_target += position + 1;
            if payload.offset != offset
                || payload.compressed_bytes == 0
                || payload.uncompressed_bytes == 0
                || payload.uncompressed_bytes > MAX_PAYLOAD_BYTES
                || !valid_digest(&payload.sha256)
            {
                return Err(invalid("invalid remote payload manifest"));
            }
            offset = offset
                .checked_add(payload.compressed_bytes)
                .filter(|end| *end <= compressed.len())
                .ok_or_else(|| invalid("remote payload bounds exceeded"))?;
        }
        if offset != compressed.len() {
            return Err(invalid("unexpected trailing remote bundle data"));
        }
        Ok(Self {
            manifest,
            compressed,
        })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Official packages carry every target; local builds may carry fewer.
    pub fn is_complete(&self) -> bool {
        self.manifest.payloads.len() == TARGETS.len()
    }

    pub fn validate_identity(&self, app_version: &str, source_sha: &str) -> io::Result<()> {
        if self.manifest.build.app_version != app_version
            || self.manifest.build.source_sha != source_sha
        {
            return Err(invalid(
                "remote bundle does not match the application source",
            ));
        }
        Ok(())
    }

    /// Only the selected target is inflated; the other payloads remain compressed.
    pub fn payload(&self, target: ServerTarget) -> io::Result<OwnedPayload> {
        let payload = self
            .manifest
            .payloads
            .iter()
            .find(|payload| payload.target == target)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!(
                        "this build does not include an SSH server for {}",
                        target.name()
                    ),
                )
            })?;
        let compressed =
            &self.compressed[payload.offset..payload.offset + payload.compressed_bytes];
        let mut decoder = GzDecoder::new(compressed);
        let mut bytes = Vec::new();
        decoder
            .by_ref()
            .take(payload.uncompressed_bytes as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() != payload.uncompressed_bytes || !decoder.into_inner().is_empty() {
            return Err(invalid("remote payload decompression length mismatch"));
        }
        let owned = OwnedPayload {
            build: self.manifest.build.clone(),
            target,
            sha256: payload.sha256.clone(),
            bytes,
        };
        owned.as_payload().verify()?;
        validate_executable(target, &owned.bytes)?;
        Ok(owned)
    }
}

pub fn create(build: BuildIdentity, payloads: &[(ServerTarget, &[u8])]) -> io::Result<Vec<u8>> {
    validate_build(&build)?;
    if payloads.is_empty() {
        return Err(invalid("at least one remote target is required"));
    }
    let mut compressed = Vec::new();
    let mut entries = Vec::new();
    let mut next_target = 0_usize;
    for (target, bytes) in payloads {
        let Some(position) = TARGETS
            .get(next_target..)
            .and_then(|rest| rest.iter().position(|candidate| candidate == target))
        else {
            return Err(invalid(
                "remote payload targets are duplicated or out of order",
            ));
        };
        next_target += position + 1;
        if bytes.is_empty() || bytes.len() > MAX_PAYLOAD_BYTES {
            return Err(invalid("remote executable exceeds its size budget"));
        }
        validate_executable(*target, bytes)?;
        let offset = compressed.len();
        let mut writer = GzBuilder::new()
            .mtime(0)
            .write(&mut compressed, Compression::best());
        writer.write_all(bytes)?;
        writer.finish()?;
        entries.push(PayloadManifest {
            target: *target,
            offset,
            compressed_bytes: compressed.len() - offset,
            uncompressed_bytes: bytes.len(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
        });
    }
    let manifest = serde_json::to_vec(&Manifest {
        build,
        protocol: PROTOCOL_VERSION,
        payloads: entries,
    })
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if manifest.len() > MAX_MANIFEST_BYTES {
        return Err(invalid("remote bundle manifest exceeds its size budget"));
    }
    let mut bytes = Vec::with_capacity(12 + manifest.len() + compressed.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(manifest.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&manifest);
    bytes.extend_from_slice(&compressed);
    Ok(bytes)
}

fn validate_build(build: &BuildIdentity) -> io::Result<()> {
    let version = |version: &str| {
        !version.is_empty()
            && version.len() <= 64
            && version
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-_".contains(&byte))
    };
    if !version(&build.app_version)
        || !version(&build.server_version)
        || build.source_sha.len() != 40
        || !build
            .source_sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid("invalid remote bundle build identity"));
    }
    Ok(())
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn validate_executable(target: ServerTarget, bytes: &[u8]) -> io::Result<()> {
    let valid = match target {
        ServerTarget::LinuxX86_64 | ServerTarget::LinuxAarch64 => {
            let machine = if target == ServerTarget::LinuxX86_64 {
                62_u16
            } else {
                183
            };
            bytes.len() >= 64
                && bytes[..4] == *b"\x7fELF"
                && bytes[4] == 2
                && bytes[5] == 1
                && bytes[18..20] == machine.to_le_bytes()
        }
        ServerTarget::MacosX86_64 | ServerTarget::MacosAarch64 => {
            let cpu = if target == ServerTarget::MacosX86_64 {
                0x0100_0007_u32
            } else {
                0x0100_000c
            };
            bytes.len() >= 32
                && bytes[..4] == [0xcf, 0xfa, 0xed, 0xfe]
                && bytes[4..8] == cpu.to_le_bytes()
        }
    };
    if valid {
        Ok(())
    } else {
        Err(invalid(
            "remote executable platform or architecture mismatch",
        ))
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> Vec<(ServerTarget, Vec<u8>)> {
        TARGETS
            .iter()
            .map(|target| {
                let mut bytes = vec![0; 64];
                match target {
                    ServerTarget::LinuxX86_64 | ServerTarget::LinuxAarch64 => {
                        bytes[..6].copy_from_slice(b"\x7fELF\x02\x01");
                        let machine = if *target == ServerTarget::LinuxX86_64 {
                            62_u16
                        } else {
                            183
                        };
                        bytes[18..20].copy_from_slice(&machine.to_le_bytes());
                    }
                    ServerTarget::MacosX86_64 | ServerTarget::MacosAarch64 => {
                        bytes[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
                        let cpu = if *target == ServerTarget::MacosX86_64 {
                            0x0100_0007_u32
                        } else {
                            0x0100_000c
                        };
                        bytes[4..8].copy_from_slice(&cpu.to_le_bytes());
                    }
                }
                (*target, bytes)
            })
            .collect()
    }

    fn build() -> BuildIdentity {
        BuildIdentity {
            app_version: "0.1.38".into(),
            server_version: "0.1.0".into(),
            source_sha: "1".repeat(40),
        }
    }

    #[test]
    fn bundle_binds_payloads_to_source_and_architecture() {
        let fixtures = fixtures();
        let inputs: Vec<_> = fixtures
            .iter()
            .map(|(target, bytes)| (*target, bytes.as_slice()))
            .collect();
        assert!(create(build(), &[]).is_err());
        let bytes = create(build(), &inputs).unwrap();
        let bundle = Bundle::parse(&bytes).unwrap();
        assert!(bundle.is_complete());
        bundle.validate_identity("0.1.38", &"1".repeat(40)).unwrap();
        assert!(bundle.validate_identity("0.1.39", &"1".repeat(40)).is_err());
        for (target, bytes) in &fixtures {
            assert_eq!(bundle.payload(*target).unwrap().as_payload().bytes, bytes);
        }
        let mut swapped = inputs;
        swapped.swap(0, 1);
        assert!(create(build(), &swapped).is_err());
        assert!(validate_executable(ServerTarget::MacosX86_64, &fixtures[3].1).is_err());
    }

    #[test]
    fn partial_bundles_serve_only_their_targets() {
        let fixtures = fixtures();
        let inputs: Vec<_> = fixtures
            .iter()
            .map(|(target, bytes)| (*target, bytes.as_slice()))
            .collect();
        let bytes = create(build(), &[inputs[1], inputs[3]]).unwrap();
        let bundle = Bundle::parse(&bytes).unwrap();
        assert!(!bundle.is_complete());
        assert!(bundle.payload(ServerTarget::LinuxAarch64).is_ok());
        assert!(bundle.payload(ServerTarget::MacosAarch64).is_ok());
        let error = bundle.payload(ServerTarget::LinuxX86_64).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert!(create(build(), &[inputs[1], inputs[1]]).is_err());
    }

    #[test]
    fn corrupt_or_truncated_payloads_are_never_returned_for_execution() {
        let fixtures = fixtures();
        let inputs: Vec<_> = fixtures
            .iter()
            .map(|(target, bytes)| (*target, bytes.as_slice()))
            .collect();
        let mut bytes = create(build(), &inputs).unwrap();
        assert!(Bundle::parse(&bytes[..bytes.len() - 1]).is_err());
        *bytes.last_mut().unwrap() ^= 1;
        let bundle = Bundle::parse(&bytes).unwrap();
        assert!(bundle.payload(ServerTarget::MacosAarch64).is_err());
        bytes.extend_from_slice(b"trailing");
        assert!(Bundle::parse(&bytes).is_err());
    }
}
