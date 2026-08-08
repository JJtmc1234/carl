//! The one file format whisper reads, written by hand.
//!
//! A wav header is forty four bytes and no dependency is worth pulling in for it.

use std::io::Write;
use std::path::Path;

use super::{BYTES_PER_SAMPLE, RATE};
use crate::Result;

/// A 16 bit mono PCM wav, header written by hand.
pub fn write_wav(path: &Path, pcm: &[u8]) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    let data_len = pcm.len() as u32;

    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&RATE.to_le_bytes())?;
    f.write_all(&(RATE * BYTES_PER_SAMPLE as u32).to_le_bytes())?;
    f.write_all(&(BYTES_PER_SAMPLE as u16).to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    f.write_all(pcm)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wav_header_is_what_whisper_expects() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.wav");
        write_wav(&path, &vec![0u8; (RATE as usize / 10) * 2]).unwrap();

        let b = std::fs::read(&path).unwrap();
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(&b[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([b[22], b[23]]), 1, "must be mono");
        assert_eq!(
            u32::from_le_bytes([b[24], b[25], b[26], b[27]]),
            RATE,
            "whisper resamples anything else, so get it right at the source"
        );
        assert_eq!(u16::from_le_bytes([b[34], b[35]]), 16);
        assert_eq!(b.len(), 44 + (RATE as usize / 10) * 2);
    }

    #[test]
    fn an_empty_recording_still_writes_a_valid_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.wav");
        write_wav(&path, &[]).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 44);
    }
}
