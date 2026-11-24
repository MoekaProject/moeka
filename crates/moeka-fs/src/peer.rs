//! Ephemeral execution-scope identity for pairing already authenticated channels.
//! Never use this as a host-key check or persist it as a resource authority.

use std::io;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn identity() -> io::Result<[u8; 32]> {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::MetadataExt;

    let root = std::fs::File::open("/")?;
    let metadata = root.metadata()?;
    let boot = boot_identity()?;
    let mut digest = Sha256::new();
    digest.update(b"moeka-execution-scope-v1");
    for part in [
        std::env::consts::OS.as_bytes(),
        boot.as_slice(),
        &metadata.dev().to_le_bytes(),
        &metadata.ino().to_le_bytes(),
        // SAFETY: geteuid has no preconditions or pointer arguments.
        &unsafe { libc::geteuid() }.to_le_bytes(),
    ] {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::ffi::OsStrExt;
        for namespace in ["/proc/self/ns/mnt", "/proc/self/ns/user"] {
            let namespace = std::fs::read_link(namespace)?;
            let bytes = namespace.as_os_str().as_bytes();
            digest.update((bytes.len() as u64).to_le_bytes());
            digest.update(bytes);
        }
        let info = read_small(&format!("/proc/self/fdinfo/{}", root.as_raw_fd()))?;
        let mount = info
            .split(|byte| *byte == b'\n')
            .find_map(|line| line.strip_prefix(b"mnt_id:"))
            .map(<[u8]>::trim_ascii)
            .filter(|value| !value.is_empty() && value.iter().all(u8::is_ascii_digit))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "root mount ID is unavailable")
            })?;
        digest.update((mount.len() as u64).to_le_bytes());
        digest.update(mount);
    }
    Ok(digest.finalize().into())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn identity() -> io::Result<[u8; 32]> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "execution-scope identity is unsupported",
    ))
}

#[cfg(target_os = "linux")]
fn read_small(path: &str) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(4097)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "kernel identity exceeds its bound",
        ));
    }
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn boot_identity() -> io::Result<Vec<u8>> {
    checked_boot_identity(&read_small("/proc/sys/kernel/random/boot_id")?)
}

#[cfg(target_os = "macos")]
fn boot_identity() -> io::Result<Vec<u8>> {
    let mut bytes = [0_u8; 128];
    let mut length = bytes.len();
    // SAFETY: the name is NUL-terminated, the writable buffer has `length`
    // bytes, and the null new-value pointer requests a read only.
    let result = unsafe {
        libc::sysctlbyname(
            c"kern.bootsessionuuid".as_ptr(),
            bytes.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length > bytes.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "kernel identity exceeds its bound",
        ));
    }
    checked_boot_identity(&bytes[..length])
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn checked_boot_identity(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes).trim_ascii();
    if bytes.len() != 36
        || !bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid kernel boot-session identity",
        ));
    }
    Ok(bytes.to_vec())
}
