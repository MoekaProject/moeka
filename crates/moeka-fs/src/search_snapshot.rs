use std::fs::{File, Metadata};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use memmap2::Mmap;
use sha2::{Digest, Sha256};

use crate::backend::{MAX_READ_BYTES, Revision, same_snapshot};
use crate::content::{classify_content, is_markdown_path};
use crate::local::LocalFilesystem;

const RESIDENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_SNAPSHOT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug)]
pub(crate) enum SearchText {
    Resident(String),
    Mapped {
        mapping: Mmap,
        _snapshot: File,
    },
    Lossy {
        mapping: Mmap,
        original: Mmap,
        _snapshots: (File, File),
    },
}

#[derive(Default)]
pub(crate) struct SourceOffsets {
    original: usize,
    decoded: usize,
    valid_end: usize,
}

#[derive(Debug)]
pub(crate) enum SearchSnapshotStep {
    Pending,
    Text(SearchText, Revision),
    Binary,
}

impl SearchText {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Resident(text) => text,
            Self::Mapped { mapping, .. } | Self::Lossy { mapping, .. } => {
                // Private snapshots contain only validated UTF-8 or decoded chunks.
                // No writable handle escapes or resizes it while mapped.
                unsafe { std::str::from_utf8_unchecked(mapping) }
            }
        }
    }

    pub(crate) fn source_offset(&self, decoded: usize, cursor: &mut SourceOffsets) -> usize {
        let Self::Lossy { original, .. } = self else {
            return decoded;
        };
        debug_assert!(decoded >= cursor.decoded);
        while cursor.decoded < decoded {
            if cursor.original < cursor.valid_end {
                let count = (decoded - cursor.decoded).min(cursor.valid_end - cursor.original);
                cursor.original += count;
                cursor.decoded += count;
            } else {
                match std::str::from_utf8(&original[cursor.original..]) {
                    Ok(_) => cursor.valid_end = original.len(),
                    Err(error) if error.valid_up_to() > 0 => {
                        cursor.valid_end = cursor.original + error.valid_up_to();
                    }
                    Err(error) => {
                        cursor.original += error
                            .error_len()
                            .unwrap_or(original.len() - cursor.original);
                        cursor.decoded += '\u{fffd}'.len_utf8();
                    }
                }
            }
        }
        cursor.original
    }
}

pub(crate) struct SearchSnapshot {
    source: File,
    path: PathBuf,
    metadata: Metadata,
    resident: Vec<u8>,
    spilled: Option<File>,
    hash: Sha256,
    size: u64,
}

impl SearchSnapshot {
    pub(crate) fn open(local: &LocalFilesystem, path: &Path) -> io::Result<Self> {
        let source = local.open(path)?;
        let metadata = source.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a regular file",
            ));
        }
        if metadata.len() > MAX_SNAPSHOT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "file exceeds the search snapshot budget",
            ));
        }
        Ok(Self {
            source,
            path: path.to_owned(),
            metadata,
            resident: Vec::new(),
            spilled: None,
            hash: Sha256::new(),
            size: 0,
        })
    }

    pub(crate) fn step(&mut self, local: &LocalFilesystem) -> io::Result<SearchSnapshotStep> {
        let mut bytes = [0; MAX_READ_BYTES];
        let length = self.source.read(&mut bytes)?;
        if length == 0 {
            if !same_snapshot(&self.metadata, &self.source.metadata()?)
                || !same_snapshot(&self.metadata, &local.metadata(&self.path)?)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "file changed while preparing search",
                ));
            }
            if self.spilled.is_none()
                && !classify_content(&self.path, &self.resident).supports_text_search()
            {
                return Ok(SearchSnapshotStep::Binary);
            }
            let text = if let Some(snapshot) = self.spilled.take() {
                // Only this module owns the anonymous snapshot and it is never
                // modified after this read-only mapping is created.
                let mapping = unsafe { Mmap::map(&snapshot)? };
                if !classify_content(&self.path, &mapping).supports_text_search() {
                    return Ok(SearchSnapshotStep::Binary);
                }
                if std::str::from_utf8(&mapping).is_ok() {
                    SearchText::Mapped {
                        mapping,
                        _snapshot: snapshot,
                    }
                } else {
                    Self::lossy_snapshot(snapshot, mapping)?
                }
            } else {
                match String::from_utf8(std::mem::take(&mut self.resident)) {
                    Ok(text) => SearchText::Resident(text),
                    Err(error) => {
                        let mut snapshot = tempfile::tempfile()?;
                        snapshot.write_all(error.as_bytes())?;
                        drop(error);
                        let mapping = unsafe { Mmap::map(&snapshot)? };
                        Self::lossy_snapshot(snapshot, mapping)?
                    }
                }
            };
            return Ok(SearchSnapshotStep::Text(
                text,
                Revision {
                    size: self.size,
                    sha256: self.hash.clone().finalize().into(),
                },
            ));
        }
        if self.size.saturating_add(length as u64) > MAX_SNAPSHOT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "file exceeds the search snapshot budget",
            ));
        }
        if self.size == 0
            && length >= 4096
            && !classify_content(&self.path, &bytes[..length]).supports_text_search()
            || !is_markdown_path(&self.path) && bytes[..length].contains(&0)
        {
            return Ok(SearchSnapshotStep::Binary);
        }
        if self.spilled.is_none() && self.resident.len() + length > RESIDENT_BYTES {
            let mut snapshot = tempfile::tempfile()?;
            snapshot.write_all(&self.resident)?;
            self.resident = Vec::new();
            self.spilled = Some(snapshot);
        }
        if let Some(snapshot) = &mut self.spilled {
            snapshot.write_all(&bytes[..length])?;
        } else {
            self.resident.extend_from_slice(&bytes[..length]);
        }
        self.hash.update(&bytes[..length]);
        self.size += length as u64;
        Ok(SearchSnapshotStep::Pending)
    }

    fn lossy_snapshot(snapshot: File, original: Mmap) -> io::Result<SearchText> {
        let mut decoded = tempfile::tempfile()?;
        let mut writer = BufWriter::with_capacity(MAX_READ_BYTES, &mut decoded);
        let mut size = 0u64;
        for chunk in original.utf8_chunks() {
            let replacement = if chunk.invalid().is_empty() {
                ""
            } else {
                "\u{fffd}"
            };
            size = size.saturating_add((chunk.valid().len() + replacement.len()) as u64);
            if size > MAX_SNAPSHOT_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "decoded text exceeds the search snapshot budget",
                ));
            }
            writer.write_all(chunk.valid().as_bytes())?;
            writer.write_all(replacement.as_bytes())?;
        }
        writer.flush()?;
        drop(writer);
        let mapping = unsafe { Mmap::map(&decoded)? };
        Ok(SearchText::Lossy {
            mapping,
            original,
            _snapshots: (decoded, snapshot),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossy_search_preserves_raw_offsets_and_revision_for_resident_and_spilled_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notes.md");
        for prefix in [0, RESIDENT_BYTES + 1] {
            let mut original = vec![b'x'; prefix];
            original.extend_from_slice(b"\xffneedle\n\xef\xbfneedle\xf0\x9fneedle\0needle");
            std::fs::write(&path, &original).unwrap();
            let mut reader = SearchSnapshot::open(&LocalFilesystem, &path).unwrap();
            let (text, revision) = loop {
                match reader.step(&LocalFilesystem).unwrap() {
                    SearchSnapshotStep::Pending => {}
                    SearchSnapshotStep::Text(text, revision) => break (text, revision),
                    SearchSnapshotStep::Binary => panic!("markdown was classified as binary"),
                }
            };
            assert_eq!(text.as_str(), String::from_utf8_lossy(&original));
            assert_eq!(revision.size, original.len() as u64);
            assert_eq!(revision.sha256, <[u8; 32]>::from(Sha256::digest(&original)));
            let expected = original
                .windows(6)
                .enumerate()
                .filter_map(|(start, bytes)| (bytes == b"needle").then_some(start..start + 6))
                .collect::<Vec<_>>();
            let mut offsets = SourceOffsets::default();
            let actual = text
                .as_str()
                .match_indices("needle")
                .map(|(start, _)| {
                    text.source_offset(start, &mut offsets)
                        ..text.source_offset(start + 6, &mut offsets)
                })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn large_snapshots_spill_and_reject_changes_before_publishing_results() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes");
        let original = "abcd\n".repeat(RESIDENT_BYTES / 5 + MAX_READ_BYTES);
        std::fs::write(&path, &original).unwrap();
        let mut reader = SearchSnapshot::open(&LocalFilesystem, &path).unwrap();
        let (text, revision) = loop {
            if let SearchSnapshotStep::Text(text, revision) = reader.step(&LocalFilesystem).unwrap()
            {
                break (text, revision);
            }
        };
        assert!(matches!(text, SearchText::Mapped { .. }));
        assert_eq!(text.as_str(), original);
        assert_eq!(revision.size, original.len() as u64);
        let mut changed = SearchSnapshot::open(&LocalFilesystem, &path).unwrap();
        assert!(matches!(
            changed.step(&LocalFilesystem).unwrap(),
            SearchSnapshotStep::Pending
        ));
        std::fs::write(&path, "replaced").unwrap();
        assert_eq!(
            changed.step(&LocalFilesystem).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
