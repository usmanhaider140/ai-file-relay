//! Chrome native messaging framing.
//!
//! Each message is a 4-byte length (native byte order) followed by that many
//! bytes of UTF-8 JSON. Chrome limits messages from the host to 1 MB and
//! messages to the host to 64 MiB.

use std::io::{self, Read, Write};

use serde::{Serialize, de::DeserializeOwned};

/// Largest message Chrome accepts from a native host.
pub const MAX_OUTGOING: usize = 1024 * 1024;
/// Largest message Chrome sends to a native host.
pub const MAX_INCOMING: usize = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
    #[error("bad JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("message is {0} bytes, over the {1} byte limit")]
    TooLarge(usize, usize),
}

/// Read one message. Returns `Ok(None)` when Chrome closes the pipe.
pub fn read_message<T: DeserializeOwned>(reader: &mut impl Read) -> Result<Option<T>, FrameError> {
    let mut len_bytes = [0u8; 4];
    match reader.read_exact(&mut len_bytes) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_ne_bytes(len_bytes) as usize;
    if len > MAX_INCOMING {
        return Err(FrameError::TooLarge(len, MAX_INCOMING));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    Ok(Some(serde_json::from_slice(&buf)?))
}

/// Write one message and flush it.
pub fn write_message<T: Serialize>(writer: &mut impl Write, msg: &T) -> Result<(), FrameError> {
    let body = serde_json::to_vec(msg)?;
    if body.len() > MAX_OUTGOING {
        return Err(FrameError::TooLarge(body.len(), MAX_OUTGOING));
    }
    // Safe cast: MAX_OUTGOING fits in u32.
    let len = body.len() as u32;
    writer.write_all(&len.to_ne_bytes())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use relay_protocol::{ExtensionToHub, HubToExtension};
    use std::io::Cursor;

    #[test]
    fn round_trip() {
        let mut buf = Vec::new();
        write_message(&mut buf, &ExtensionToHub::Ping).unwrap();
        let msg: Option<ExtensionToHub> = read_message(&mut Cursor::new(buf)).unwrap();
        assert_eq!(msg, Some(ExtensionToHub::Ping));
    }

    #[test]
    fn closed_pipe_is_none() {
        let msg: Option<ExtensionToHub> = read_message(&mut Cursor::new(Vec::new())).unwrap();
        assert_eq!(msg, None);
    }

    #[test]
    fn oversized_outgoing_is_refused() {
        let big = HubToExtension::Error {
            message: "x".repeat(MAX_OUTGOING + 1),
        };
        let err = write_message(&mut Vec::new(), &big).unwrap_err();
        assert!(matches!(err, FrameError::TooLarge(..)));
    }

    #[test]
    fn oversized_incoming_is_refused() {
        let len = (MAX_INCOMING as u32 + 1).to_ne_bytes();
        let err = read_message::<ExtensionToHub>(&mut Cursor::new(len.to_vec())).unwrap_err();
        assert!(matches!(err, FrameError::TooLarge(..)));
    }
}
