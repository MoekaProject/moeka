use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A native path encoded without applying the client's path parsing rules.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "style", content = "units", rename_all = "snake_case")]
pub enum NativePath {
    Unix(Vec<u8>),
    Windows(Vec<u16>),
}

impl std::fmt::Display for NativePath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unix(bytes) => String::from_utf8_lossy(bytes).fmt(formatter),
            Self::Windows(units) => String::from_utf16_lossy(units).fmt(formatter),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "backend", content = "id", rename_all = "snake_case")]
pub enum FilesystemId {
    Local,
    Ssh([u8; 32]),
}

/// Resource identity is independent of the currently selected workspace.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourcePath {
    pub filesystem: FilesystemId,
    pub path: NativePath,
}

impl ResourcePath {
    pub fn with_path(&self, path: NativePath) -> Self {
        Self {
            filesystem: self.filesystem.clone(),
            path,
        }
    }

    pub fn join_child(&self, name: &str) -> io::Result<Self> {
        self.path.join_child(name).map(|path| self.with_path(path))
    }

    pub fn join_native_child(&self, name: &NativePath) -> io::Result<Self> {
        self.path
            .join_native_child(name)
            .map(|path| self.with_path(path))
    }

    pub fn parent(&self) -> Option<Self> {
        self.path.parent().map(|path| self.with_path(path))
    }

    pub fn local(path: &Path) -> Self {
        Self {
            filesystem: FilesystemId::Local,
            path: NativePath::from_local(path),
        }
    }

    pub fn local_path(&self) -> io::Result<PathBuf> {
        match self.filesystem {
            FilesystemId::Local => self.path.to_local(),
            FilesystemId::Ssh(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "remote resources cannot be used as local filesystem paths",
            )),
        }
    }
}

impl NativePath {
    /// Number a duplicate filename without decoding native bytes or UTF-16 units.
    pub fn numbered_name(&self, suffix: usize, file: bool) -> Self {
        fn numbered<T: From<u8> + Copy + Eq>(name: &[T], suffix: usize, file: bool) -> Vec<T> {
            let dot = T::from(b'.');
            let split = file
                .then(|| name.iter().rposition(|unit| *unit == dot))
                .flatten()
                .filter(|at| *at > 0)
                .unwrap_or(name.len());
            let mut result = name[..split].to_vec();
            result.extend(format!("-{suffix}").bytes().map(T::from));
            if split + 1 < name.len() {
                result.extend_from_slice(&name[split..]);
            }
            result
        }
        match self {
            Self::Unix(name) => Self::Unix(numbered(name, suffix, file)),
            Self::Windows(name) => Self::Windows(numbered(name, suffix, file)),
        }
    }

    pub fn unix_absolute(path: &str) -> io::Result<Self> {
        if !path.starts_with('/') || path.as_bytes().contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "an absolute Unix path without NUL is required",
            ));
        }
        Ok(Self::Unix(path.as_bytes().to_vec()))
    }

    pub fn join_child(&self, name: &str) -> io::Result<Self> {
        let native_name = match self {
            Self::Unix(_) => Self::Unix(name.as_bytes().to_vec()),
            Self::Windows(_) => Self::Windows(name.encode_utf16().collect()),
        };
        self.join_native_child(&native_name)
    }

    /// Joins one native filename without converting its bytes or UTF-16 units.
    pub fn join_native_child(&self, name: &NativePath) -> io::Result<Self> {
        let invalid = || {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "a single native filename is required",
            )
        };
        Ok(match (self, name) {
            (Self::Unix(path), Self::Unix(name)) => {
                if path.contains(&0)
                    || name.is_empty()
                    || name == b"."
                    || name == b".."
                    || name.iter().any(|&byte| matches!(byte, 0 | b'/'))
                {
                    return Err(invalid());
                }
                let mut joined = path.clone();
                if !joined.is_empty() && joined.last() != Some(&b'/') {
                    joined.push(b'/');
                }
                joined.extend_from_slice(name);
                Self::Unix(joined)
            }
            (Self::Windows(path), Self::Windows(name)) => {
                if path.contains(&0)
                    || name.is_empty()
                    || name == &[46]
                    || name == &[46, 46]
                    || name.iter().any(|&unit| matches!(unit, 0 | 47 | 58 | 92))
                {
                    return Err(invalid());
                }
                let mut joined = path.clone();
                let drive_relative = joined.len() == 2 && joined[1] == b':' as u16;
                if !joined.is_empty()
                    && !drive_relative
                    && !joined.last().is_some_and(|unit| windows_separator(*unit))
                {
                    joined.push(b'\\' as u16);
                }
                joined.extend_from_slice(name);
                Self::Windows(joined)
            }
            _ => return Err(invalid()),
        })
    }

    pub fn parent(&self) -> Option<Self> {
        match self {
            Self::Unix(path) => {
                parent_units(path, usize::from(path.starts_with(b"/")), |b| b == b'/')
                    .map(Self::Unix)
            }
            Self::Windows(path) => {
                parent_units(path, windows_root_length(path), windows_separator).map(Self::Windows)
            }
        }
    }

    pub fn file_name(&self) -> Option<Self> {
        match self {
            Self::Unix(path) => {
                filename_units(path, usize::from(path.starts_with(b"/")), |b| b == b'/')
                    .map(Self::Unix)
            }
            Self::Windows(path) => {
                filename_units(path, windows_root_length(path), windows_separator)
                    .map(Self::Windows)
            }
        }
    }

    pub fn from_local(path: &Path) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Self::Unix(path.as_os_str().as_bytes().to_vec())
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            Self::Windows(path.as_os_str().encode_wide().collect())
        }
    }

    pub fn to_local(&self) -> io::Result<PathBuf> {
        match self {
            #[cfg(unix)]
            Self::Unix(bytes) if !bytes.contains(&0) => {
                use std::ffi::OsStr;
                use std::os::unix::ffi::OsStrExt;
                Ok(PathBuf::from(OsStr::from_bytes(bytes)))
            }
            #[cfg(windows)]
            Self::Windows(units) if !units.contains(&0) => {
                use std::ffi::OsString;
                use std::os::windows::ffi::OsStringExt;
                Ok(PathBuf::from(OsString::from_wide(units)))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path encoding is incompatible with this host or contains NUL",
            )),
        }
    }
}

fn windows_separator(unit: u16) -> bool {
    matches!(unit, 47 | 92)
}

#[test]
fn numbered_duplicates_preserve_extension_and_native_units() {
    assert_eq!(
        NativePath::Unix(b"note\xff.tar.gz".to_vec()).numbered_name(2, true),
        NativePath::Unix(b"note\xff.tar-2.gz".to_vec())
    );
    for (name, file, expected) in [
        ("foo.bar", false, "foo.bar-1"),
        (".env", true, ".env-1"),
        ("文稿.md", true, "文稿-1.md"),
        ("name.", true, "name-1"),
    ] {
        assert_eq!(
            NativePath::Windows(name.encode_utf16().collect()).numbered_name(1, file),
            NativePath::Windows(expected.encode_utf16().collect())
        );
    }
}

fn windows_root_length(path: &[u16]) -> usize {
    if path.len() >= 2 && windows_separator(path[0]) && windows_separator(path[1]) {
        let namespaced =
            path.len() >= 4 && matches!(path[2], 46 | 63) && windows_separator(path[3]);
        let unc_start = if namespaced {
            if path.len() >= 8
                && path[4..7]
                    .iter()
                    .zip(b"UNC")
                    .all(|(unit, byte)| unit | 32 == u16::from(byte | 32))
                && windows_separator(path[7])
            {
                8
            } else if path.get(5) == Some(&(b':' as u16)) {
                return if path.get(6).is_some_and(|unit| windows_separator(*unit)) {
                    7
                } else {
                    6
                };
            } else {
                return path[4..]
                    .iter()
                    .position(|unit| windows_separator(*unit))
                    .map_or(path.len(), |index| index + 4);
            }
        } else {
            2
        };
        let server_end = path[unc_start..]
            .iter()
            .position(|unit| windows_separator(*unit))
            .map_or(path.len(), |index| index + unc_start);
        return path[server_end.saturating_add(1).min(path.len())..]
            .iter()
            .position(|unit| windows_separator(*unit))
            .map_or(path.len(), |index| server_end + 1 + index);
    }
    if path.get(1) == Some(&(b':' as u16)) {
        return if path.get(2).is_some_and(|unit| windows_separator(*unit)) {
            3
        } else {
            2
        };
    }
    usize::from(path.first().is_some_and(|unit| windows_separator(*unit)))
}

fn parent_units<T: Copy + PartialEq>(
    path: &[T],
    root: usize,
    separator: impl Fn(T) -> bool,
) -> Option<Vec<T>> {
    let end = path
        .iter()
        .rposition(|unit| !separator(*unit))
        .map_or(0, |index| index + 1)
        .max(root);
    if end <= root {
        return None;
    }
    let split = path[..end]
        .iter()
        .rposition(|unit| separator(*unit))
        .unwrap_or(root);
    Some(path[..split.max(root)].to_vec())
}

fn filename_units<T: Copy + PartialEq>(
    path: &[T],
    root: usize,
    separator: impl Fn(T) -> bool,
) -> Option<Vec<T>> {
    let end = path
        .iter()
        .rposition(|unit| !separator(*unit))
        .map_or(0, |index| index + 1);
    if end <= root {
        return None;
    }
    let start = path[..end]
        .iter()
        .rposition(|unit| separator(*unit))
        .map_or(root, |index| index + 1);
    Some(path[start..end].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_path_operations_keep_authority_and_server_separator_rules() {
        let root = ResourcePath {
            filesystem: FilesystemId::Ssh([7; 32]),
            path: NativePath::unix_absolute("/notes").unwrap(),
        };
        let child = root.join_child("draft\\原稿.md").unwrap();
        assert_eq!(child.parent(), Some(root.clone()));
        assert_eq!(
            child.path.file_name().unwrap().to_string(),
            "draft\\原稿.md"
        );
        assert!(child.local_path().is_err());
        for invalid in ["", ".", "..", "/tmp/file", "a/b", "a\0b"] {
            assert!(root.join_child(invalid).is_err());
        }
        assert!(NativePath::unix_absolute("relative").is_err());
        assert!(NativePath::unix_absolute("/a\0b").is_err());
        assert_eq!(NativePath::Unix(b"/".to_vec()).parent(), None);
        let raw = NativePath::Unix(b"/notes/\xff".to_vec());
        assert_eq!(raw.parent(), Some(root.path));
        assert_eq!(raw.file_name(), Some(NativePath::Unix(vec![255])));
    }

    #[test]
    fn native_child_join_preserves_unix_bytes_and_windows_units() {
        let root = ResourcePath {
            filesystem: FilesystemId::Ssh([42; 32]),
            path: NativePath::Unix(b"/notes".to_vec()),
        };
        let original = NativePath::Unix(b"/old/\xff\\draft:1.md".to_vec());
        let filename = original.file_name().unwrap();
        let joined = root.join_native_child(&filename).unwrap();
        assert_eq!(joined.filesystem, root.filesystem);
        assert_eq!(
            joined.path,
            NativePath::Unix(b"/notes/\xff\\draft:1.md".to_vec())
        );

        for invalid in [b"".as_slice(), b".", b"..", b"a/b", b"a\0b"] {
            assert!(
                root.join_native_child(&NativePath::Unix(invalid.to_vec()))
                    .is_err()
            );
        }
        assert!(
            root.join_native_child(&NativePath::Windows(vec![97]))
                .is_err()
        );
        assert!(
            NativePath::Unix(vec![0])
                .join_native_child(&filename)
                .is_err()
        );

        let drive = NativePath::Windows(vec![67, 58]);
        let name = NativePath::Windows(vec![0xd800, 0x0061]);
        assert_eq!(
            drive.join_native_child(&name).unwrap(),
            NativePath::Windows(vec![67, 58, 0xd800, 0x0061])
        );
        let root = NativePath::Windows(vec![67, 58, 92]);
        assert_eq!(
            root.join_native_child(&name).unwrap(),
            NativePath::Windows(vec![67, 58, 92, 0xd800, 0x0061])
        );
        for invalid in [
            vec![],
            vec![46],
            vec![46, 46],
            vec![47],
            vec![92],
            vec![58],
            vec![0],
        ] {
            assert!(
                root.join_native_child(&NativePath::Windows(invalid))
                    .is_err()
            );
        }
        assert!(root.join_native_child(&NativePath::Unix(vec![97])).is_err());
    }

    #[test]
    fn windows_path_operations_do_not_use_the_clients_unix_parser() {
        let native = |path: &str| NativePath::Windows(path.encode_utf16().collect());
        for (child, parent) in [
            (r"C:\notes\draft.md", r"C:\notes"),
            (r"C:\notes", "C:\\"),
            (r"\\host\share\notes", r"\\host\share"),
            (r"\\host\share\notes\", r"\\host\share"),
            (r"\\?\UNC\host\share\notes", r"\\?\UNC\host\share"),
            (r"\\?\C:\notes", "\\\\?\\C:\\"),
        ] {
            assert_eq!(native(child).parent(), Some(native(parent)));
        }
        assert_eq!(native("C:\\").parent(), None);
        assert_eq!(native(r"\\host\share").parent(), None);
        assert_eq!(native(r"\\?\UNC\host\share").parent(), None);
        assert_eq!(native("\\\\?\\C:\\").parent(), None);
        assert!(native("C:\\").join_child(r"a\b").is_err());
        assert!(native("C:\\").join_child("C:relative").is_err());
        assert_eq!(native("C:\\").join_child("x").unwrap(), native(r"C:\x"));
        assert_eq!(native("C:").join_child("x").unwrap(), native("C:x"));
    }

    #[cfg(unix)]
    #[test]
    fn wire_path_preserves_non_utf8_and_unix_separators() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let path = Path::new(OsStr::from_bytes(b"/notes/\xff\\draft.md"));
        let encoded = serde_json::to_vec(&NativePath::from_local(path)).unwrap();
        let decoded: NativePath = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.to_local().unwrap(), path);
        assert!(NativePath::Windows(vec![67, 58, 92]).to_local().is_err());
        assert!(NativePath::Unix(vec![b'/', 0]).to_local().is_err());
        let local = ResourcePath::local(path);
        assert_eq!(local.local_path().unwrap(), path);
        let remote = ResourcePath {
            filesystem: FilesystemId::Ssh([9; 32]),
            path: local.path.clone(),
        };
        assert_ne!(local, remote);
        assert_eq!(
            remote.local_path().unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        assert_eq!(
            serde_json::from_slice::<ResourcePath>(&serde_json::to_vec(&remote).unwrap()).unwrap(),
            remote
        );
    }

    #[cfg(windows)]
    #[test]
    fn wire_path_preserves_unpaired_surrogates() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        let path = PathBuf::from(OsString::from_wide(&[67, 58, 92, 0xd800]));
        let encoded = serde_json::to_vec(&NativePath::from_local(&path)).unwrap();
        let decoded: NativePath = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.to_local().unwrap(), path);
        assert!(NativePath::Unix(b"/notes".to_vec()).to_local().is_err());
        assert!(NativePath::Windows(vec![0]).to_local().is_err());
    }
}
