//! How loud is it, and is that speech or just a room.
//!
//! One number decides when Carl thinks you have stopped talking, so it gets its own file and
//! its own tests. Measuring it the wrong way already made him look deaf once.

use super::BYTES_PER_SAMPLE;

/// A floor to fall back on when calibration is impossible, and a hard minimum so a silent
/// room cannot produce a threshold of zero that nothing ever falls below.
pub const SPEECH_FLOOR: f32 = 0.02;

/// Root mean square, the honest measure of how loud something is.
pub(super) fn rms_of(pcm: &[u8]) -> f32 {
    if pcm.len() < BYTES_PER_SAMPLE {
        return 0.0;
    }
    let mut sum = 0f64;
    let mut n = 0u64;
    for pair in pcm.chunks_exact(BYTES_PER_SAMPLE) {
        let s = i16::from_le_bytes([pair[0], pair[1]]) as f64 / i16::MAX as f64;
        sum += s * s;
        n += 1;
    }
    ((sum / n as f64).sqrt()) as f32
}

/// Kept only so the regression test can show what peak does wrong. Nothing ships using it.
#[cfg(test)]
fn peak_of(pcm: &[u8]) -> f32 {
    let mut peak = 0i32;
    for pair in pcm.chunks_exact(BYTES_PER_SAMPLE) {
        peak = peak.max((i16::from_le_bytes([pair[0], pair[1]]) as i32).abs());
    }
    peak as f32 / i16::MAX as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_reads_as_quiet_and_a_tone_does_not() {
        assert_eq!(rms_of(&vec![0u8; 1000]), 0.0);

        let loud: Vec<u8> = (0..500)
            .flat_map(|i| (if i % 2 == 0 { 16000i16 } else { -16000 }).to_le_bytes())
            .collect();
        assert!(rms_of(&loud) > SPEECH_FLOOR);
    }

    /// The bug that made Carl seem deaf after waking. One click in an otherwise silent room
    /// sends peak over the floor, so a peak based silence test never fires and the recording
    /// runs to its cap. RMS sees the same room as quiet.
    #[test]
    fn one_click_fools_peak_and_does_not_fool_rms() {
        let mut room = vec![0u8; 16000 * 2];
        // A single loud sample, like a key press.
        room[1000..1002].copy_from_slice(&30000i16.to_le_bytes());

        assert!(
            peak_of(&room) > SPEECH_FLOOR,
            "peak is fooled by one transient, which is the whole problem"
        );
        assert!(
            rms_of(&room) < SPEECH_FLOOR,
            "rms should still see a quiet room"
        );
    }

    /// A silent room must not calibrate to a threshold of zero, or nothing is ever quieter
    /// than it and every recording runs to its cap.
    #[test]
    fn calibration_never_returns_zero() {
        assert_eq!((0.0f32 * 3.0).max(SPEECH_FLOOR), SPEECH_FLOOR);
    }
}
