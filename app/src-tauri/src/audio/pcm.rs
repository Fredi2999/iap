//! Flüchtiger Audiopuffer und WAV-Kodierung im Arbeitsspeicher.
//!
//! Rohes Audio darf nie in einer Datei landen. Der Puffer überschreibt seinen
//! Inhalt beim Verwerfen und beim Fallenlassen (`zeroize`), damit auch nach
//! Abbruch, Fehler oder vollständigem Beenden nichts im Speicher zurückbleibt.

use zeroize::Zeroize;

/// Abtastrate, mit der aufgenommen und erkannt wird (Whisper erwartet 16 kHz).
pub const CAPTURE_RATE: u32 = 16_000;

/// Obergrenze einer einzelnen Aufnahme; verhindert unbegrenztes Wachsen.
pub const MAX_RECORDING_SECONDS: u32 = 120;

/// Aufnahmepuffer mit fester Obergrenze.
#[derive(Default)]
pub struct AudioBuffer {
    samples: Vec<i16>,
    overflowed: bool,
}

impl AudioBuffer {
    /// Neuer, leerer Puffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Hängt Samples an; über der Obergrenze werden weitere verworfen und der
    /// Überlauf gemerkt, damit die Oberfläche die Aufnahme beenden kann.
    pub fn push(&mut self, chunk: &[i16]) {
        let limit = (CAPTURE_RATE * MAX_RECORDING_SECONDS) as usize;
        let room = limit.saturating_sub(self.samples.len());
        if chunk.len() > room {
            self.overflowed = true;
        }
        self.samples
            .extend_from_slice(&chunk[..chunk.len().min(room)]);
    }

    /// Ob die Obergrenze erreicht wurde.
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Länge in Millisekunden.
    pub fn duration_ms(&self) -> u64 {
        (self.samples.len() as u64 * 1000) / u64::from(CAPTURE_RATE)
    }

    /// Anzahl Samples.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Ob nichts aufgenommen wurde.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Übergibt den Inhalt und lässt den Puffer leer zurück.
    pub fn take(&mut self) -> Vec<i16> {
        self.overflowed = false;
        std::mem::take(&mut self.samples)
    }

    /// Überschreibt und leert den Puffer.
    pub fn wipe(&mut self) {
        self.samples.zeroize();
        self.samples.clear();
        self.overflowed = false;
    }
}

impl Drop for AudioBuffer {
    fn drop(&mut self) {
        self.wipe();
    }
}

/// Pegel 0..1 (Effektivwert) für die Aufnahmeanzeige.
pub fn level(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    let rms = (sum / samples.len() as f64).sqrt() / f64::from(i16::MAX);
    // Sprache liegt meist weit unter Vollaussteuerung; sanft anheben.
    (rms * 4.0).min(1.0) as f32
}

/// Kodiert 16-Bit-Mono-PCM als WAV im Speicher.
pub fn encode_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + samples.len() * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1_u16.to_le_bytes()); // Mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

/// Wandelt mono-PCM per linearer Interpolation auf eine andere Abtastrate.
///
/// Genügt für Sprache (Piper 22,05 kHz auf die 16 kHz von Whisper); ein
/// aufwendigerer Resampler wäre nur eine weitere Abhängigkeit.
pub fn resample_linear(samples: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = f64::from(from) / f64::from(to);
    let out_len = (samples.len() as f64 / ratio).floor() as usize;
    (0..out_len)
        .map(|i| {
            let position = i as f64 * ratio;
            let index = position.floor() as usize;
            let fraction = position - index as f64;
            let a = f64::from(samples[index]);
            let b = f64::from(samples[(index + 1).min(samples.len() - 1)]);
            (a + (b - a) * fraction).round() as i16
        })
        .collect()
}

/// Wandelt little-endian Bytes (Ausgabe von Piper) in Samples; ein
/// übrig gebliebenes einzelnes Byte wird zurückgegeben, damit es zum
/// nächsten Block gehört.
pub fn bytes_to_samples(bytes: &[u8], carry: &mut Option<u8>) -> Vec<i16> {
    let mut out = Vec::with_capacity(bytes.len() / 2 + 1);
    let mut iter = bytes.iter().copied();
    if let Some(first) = carry.take() {
        match iter.next() {
            Some(second) => out.push(i16::from_le_bytes([first, second])),
            None => {
                *carry = Some(first);
                return out;
            }
        }
    }
    let rest: Vec<u8> = iter.collect();
    let mut chunks = rest.chunks_exact(2);
    for pair in &mut chunks {
        out.push(i16::from_le_bytes([pair[0], pair[1]]));
    }
    *carry = chunks.remainder().first().copied();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_wipes_on_demand_and_reports_duration() {
        let mut buffer = AudioBuffer::new();
        buffer.push(&[100; 16_000]);
        assert_eq!(buffer.duration_ms(), 1000);
        buffer.wipe();
        assert!(buffer.is_empty());
        assert_eq!(buffer.duration_ms(), 0);
    }

    #[test]
    fn buffer_stops_at_limit_and_flags_overflow() {
        let mut buffer = AudioBuffer::new();
        let limit = (CAPTURE_RATE * MAX_RECORDING_SECONDS) as usize;
        buffer.push(&vec![1; limit - 10]);
        assert!(!buffer.overflowed());
        buffer.push(&[1; 100]);
        assert!(buffer.overflowed());
        assert_eq!(buffer.len(), limit);
    }

    #[test]
    fn take_empties_the_buffer() {
        let mut buffer = AudioBuffer::new();
        buffer.push(&[1, 2, 3]);
        assert_eq!(buffer.take(), vec![1, 2, 3]);
        assert!(buffer.is_empty());
    }

    #[test]
    fn wav_header_is_valid_and_sized() {
        let wav = encode_wav(&[0, 1, -1, 32767], 16_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 8);
        assert_eq!(wav.len(), 44 + 8);
        assert_eq!(
            u32::from_le_bytes(wav[4..8].try_into().unwrap()) as usize,
            wav.len() - 8
        );
    }

    #[test]
    fn level_is_zero_for_silence_and_capped_for_loud_audio() {
        assert_eq!(level(&[0; 100]), 0.0);
        assert_eq!(level(&[i16::MAX; 100]), 1.0);
        assert_eq!(level(&[]), 0.0);
    }

    #[test]
    fn resampling_changes_length_by_the_rate_ratio_and_keeps_constant_signals() {
        let out = resample_linear(&[1000; 22_050], 22_050, 16_000);
        assert_eq!(out.len(), 16_000);
        assert!(out.iter().all(|&s| s == 1000));
        assert_eq!(resample_linear(&[1, 2, 3], 16_000, 16_000), vec![1, 2, 3]);
        assert!(resample_linear(&[], 22_050, 16_000).is_empty());
    }

    #[test]
    fn byte_stream_conversion_keeps_odd_bytes_between_blocks() {
        let mut carry = None;
        let mut samples = bytes_to_samples(&[1, 0, 2], &mut carry);
        assert_eq!(samples, vec![1]);
        assert_eq!(carry, Some(2));
        samples = bytes_to_samples(&[0, 3, 0], &mut carry);
        assert_eq!(samples, vec![2, 3]);
        assert_eq!(carry, None);
    }
}
