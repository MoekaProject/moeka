use std::io::{self, Read, Write};

use serde::{Serialize, de::DeserializeOwned};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Bounded framing shared by both ends of a stdio connection.
pub fn read_frame<T: DeserializeOwned>(input: &mut impl Read) -> io::Result<Option<T>> {
    read_raw_frame(input)?
        .map(|bytes| {
            serde_json::from_slice(&bytes)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
        })
        .transpose()
}

pub fn read_raw_frame(input: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut prefix = [0; 4];
    loop {
        match input.read(&mut prefix[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    input.read_exact(&mut prefix[1..])?;
    let len = u32::from_le_bytes(prefix) as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid filesystem protocol frame length",
        ));
    }
    let mut bytes = vec![0; len];
    input.read_exact(&mut bytes)?;
    Ok(Some(bytes))
}

pub fn write_frame<T: Serialize>(output: &mut impl Write, value: &T) -> io::Result<()> {
    let mut bytes = LimitedBuffer(Vec::new());
    serde_json::to_writer(&mut bytes, value).map_err(|error| {
        let kind = error.io_error_kind().unwrap_or(io::ErrorKind::InvalidData);
        io::Error::new(kind, error)
    })?;
    output.write_all(&(bytes.0.len() as u32).to_le_bytes())?;
    output.write_all(&bytes.0)?;
    output.flush()
}

struct LimitedBuffer(Vec<u8>);

impl Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_FRAME_BYTES.saturating_sub(self.0.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "filesystem protocol frame exceeds its byte limit",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct Fragmented<'a>(&'a [u8]);

    impl Read for Fragmented<'_> {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let count = output.len().min(self.0.len()).min(1);
            output[..count].copy_from_slice(&self.0[..count]);
            self.0 = &self.0[count..];
            Ok(count)
        }
    }

    #[test]
    fn frames_survive_fragmentation_and_distinguish_clean_eof_from_truncation() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &vec!["one", "two"]).unwrap();
        let first_frame_len = bytes.len();
        write_frame(&mut bytes, &vec!["three"]).unwrap();
        let mut input = Fragmented(&bytes);
        assert_eq!(
            read_frame::<Vec<String>>(&mut input).unwrap().unwrap(),
            ["one", "two"]
        );
        assert_eq!(
            read_frame::<Vec<String>>(&mut input).unwrap().unwrap(),
            ["three"]
        );
        assert!(read_frame::<Vec<String>>(&mut input).unwrap().is_none());
        for end in 1..first_frame_len {
            let mut input = Cursor::new(&bytes[..end]);
            assert_eq!(
                read_frame::<Vec<String>>(&mut input).unwrap_err().kind(),
                io::ErrorKind::UnexpectedEof
            );
        }
    }

    #[test]
    fn corrupt_frames_are_rejected_before_body_allocation_or_output() {
        for len in [0_u32, MAX_FRAME_BYTES as u32 + 1, u32::MAX] {
            let error = read_frame::<String>(&mut Cursor::new(len.to_le_bytes())).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        }
        let mut output = Vec::new();
        let error = write_frame(&mut output, &"x".repeat(MAX_FRAME_BYTES)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(output.is_empty());
        let mut invalid = Vec::new();
        invalid.extend_from_slice(&1_u32.to_le_bytes());
        invalid.push(b'{');
        assert!(read_frame::<String>(&mut Cursor::new(invalid)).is_err());
    }
}
