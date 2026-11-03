//! Encoding-tolerant source reading: UTF-8, UTF-8 BOM, UTF-16 LE/BE BOM.

use std::io;
use std::path::Path;

/// Read a source file, transcoding UTF-16 (BOM-detected) to UTF-8.
pub fn read_source(path: &Path) -> io::Result<String> {
    let bytes = std::fs::read(path)?;
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return Ok(String::from_utf16_lossy(&u16s));
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return Ok(String::from_utf16_lossy(&u16s));
    }
    let bytes = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        &bytes[..]
    };
    String::from_utf8(bytes.to_vec())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
