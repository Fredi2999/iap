//! Einzelne Bildschirmaufnahme und ihre Aufbereitung („Bildschirm ansehen“).
//!
//! Ein Bild lebt nur im Arbeitsspeicher: Es wird aufgenommen, verkleinert,
//! als PNG-Bytes kodiert, an den lokalen `llama-server` geschickt und danach
//! überschrieben. Es gibt keine Bilddatei und keinen Protokolleintrag mit
//! Bildinhalt. Die Aufnahme selbst ist plattformabhängig (`windows_capture`).

use thiserror::Error;
use zeroize::Zeroize;

pub mod base64;
#[cfg(windows)]
pub mod windows_capture;

/// Fehler bei Aufnahme und Aufbereitung, mit verständlichem Text.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ScreenError {
    #[error("Die gewählte Quelle gibt es nicht mehr.")]
    NoSource,
    #[error(
        "Das Bild ist leer. Der Inhalt ist vermutlich geschützt und lässt sich nicht aufnehmen."
    )]
    Protected,
    #[error("Die Bildschirmaufnahme ist auf dieser Plattform noch nicht geprüft.")]
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
    #[error("Aufnahmefehler: {0}")]
    Os(String),
    #[error("Bildaufbereitung fehlgeschlagen: {0}")]
    Encode(String),
}

/// Ein aufgenommenes Bild (RGBA, 8 Bit je Kanal). Der Inhalt wird beim
/// Fallenlassen überschrieben.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Drop for Frame {
    fn drop(&mut self) {
        self.rgba.zeroize();
    }
}

impl Frame {
    /// Ob das Bild vollständig schwarz ist (geschützter Inhalt, Aufnahmefehler).
    pub fn is_blank(&self) -> bool {
        self.rgba
            .chunks_exact(4)
            .all(|px| px[0] == 0 && px[1] == 0 && px[2] == 0)
    }

    /// Schneidet ein Rechteck aus. Ränder werden auf das Bild begrenzt.
    pub fn crop(&self, x: i32, y: i32, width: u32, height: u32) -> Result<Frame, ScreenError> {
        let x0 = u32::try_from(x.max(0)).unwrap_or(0).min(self.width);
        let y0 = u32::try_from(y.max(0)).unwrap_or(0).min(self.height);
        let w = width.min(self.width - x0);
        let h = height.min(self.height - y0);
        if w == 0 || h == 0 {
            return Err(ScreenError::NoSource);
        }
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for row in y0..y0 + h {
            let start = ((row * self.width + x0) * 4) as usize;
            rgba.extend_from_slice(&self.rgba[start..start + (w * 4) as usize]);
        }
        Ok(Frame {
            width: w,
            height: h,
            rgba,
        })
    }

    /// Verkleinert durch Flächenmittelung, sodass die längere Seite höchstens
    /// `max_side` Pixel hat. Kleinere Bilder bleiben unverändert.
    pub fn downscaled(&self, max_side: u32) -> Frame {
        let longest = self.width.max(self.height);
        if longest <= max_side || max_side == 0 {
            return Frame {
                width: self.width,
                height: self.height,
                rgba: self.rgba.clone(),
            };
        }
        let scale = f64::from(max_side) / f64::from(longest);
        let out_w = ((f64::from(self.width) * scale).round() as u32).max(1);
        let out_h = ((f64::from(self.height) * scale).round() as u32).max(1);
        let mut rgba = vec![0_u8; (out_w * out_h * 4) as usize];
        for oy in 0..out_h {
            let sy0 = (u64::from(oy) * u64::from(self.height) / u64::from(out_h)) as u32;
            let sy1 = (((u64::from(oy) + 1) * u64::from(self.height) / u64::from(out_h)) as u32)
                .max(sy0 + 1);
            for ox in 0..out_w {
                let sx0 = (u64::from(ox) * u64::from(self.width) / u64::from(out_w)) as u32;
                let sx1 = (((u64::from(ox) + 1) * u64::from(self.width) / u64::from(out_w)) as u32)
                    .max(sx0 + 1);
                let mut sum = [0_u32; 3];
                let mut count = 0_u32;
                for sy in sy0..sy1.min(self.height) {
                    for sx in sx0..sx1.min(self.width) {
                        let i = ((sy * self.width + sx) * 4) as usize;
                        sum[0] += u32::from(self.rgba[i]);
                        sum[1] += u32::from(self.rgba[i + 1]);
                        sum[2] += u32::from(self.rgba[i + 2]);
                        count += 1;
                    }
                }
                let o = ((oy * out_w + ox) * 4) as usize;
                let count = count.max(1);
                rgba[o] = (sum[0] / count) as u8;
                rgba[o + 1] = (sum[1] / count) as u8;
                rgba[o + 2] = (sum[2] / count) as u8;
                rgba[o + 3] = 255;
            }
        }
        Frame {
            width: out_w,
            height: out_h,
            rgba,
        }
    }

    /// Kodiert als PNG im Speicher (ohne Alphakanal, kleiner).
    pub fn to_png(&self) -> Result<Vec<u8>, ScreenError> {
        let mut rgb = Vec::with_capacity((self.width * self.height * 3) as usize);
        for px in self.rgba.chunks_exact(4) {
            rgb.extend_from_slice(&px[..3]);
        }
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder
                .write_header()
                .map_err(|e| ScreenError::Encode(e.to_string()))?;
            writer
                .write_image_data(&rgb)
                .map_err(|e| ScreenError::Encode(e.to_string()))?;
        }
        rgb.zeroize();
        Ok(out)
    }
}

/// `data:image/png;base64,…` für die Anfrage an den lokalen Server.
pub fn png_data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64::encode(png))
}

/// Lokale Uhrzeit als Text (`TT.MM.JJJJ HH:MM:SS`) für die Beschriftung einer Aufnahme.
#[cfg(windows)]
pub fn local_time_string() -> String {
    // SAFETY: `GetLocalTime` hat keine Vorbedingungen und liefert einen Wert.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!(
        "{:02}.{:02}.{} {:02}:{:02}:{:02}",
        t.wDay, t.wMonth, t.wYear, t.wHour, t.wMinute, t.wSecond
    )
}

/// Ohne Windows-Uhr: UTC-Sekunden seit 1970 (die Plattform ist ohnehin noch nicht geprüft).
#[cfg(not(windows))]
pub fn local_time_string() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("UTC {secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> Frame {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, 128, 255]);
            }
        }
        Frame {
            width,
            height,
            rgba,
        }
    }

    #[test]
    fn blank_frames_are_detected() {
        let black = Frame {
            width: 2,
            height: 2,
            rgba: [0, 0, 0, 255].repeat(4),
        };
        assert!(black.is_blank());
        assert!(!gradient(4, 4).is_blank());
    }

    #[test]
    fn crop_is_clamped_and_empty_crop_is_an_error() {
        let frame = gradient(10, 8);
        let cropped = frame.crop(8, 6, 100, 100).expect("Ausschnitt");
        assert_eq!((cropped.width, cropped.height), (2, 2));
        assert_eq!(&cropped.rgba[..3], &[8, 6, 128]);
        assert_eq!(frame.crop(20, 0, 5, 5).err(), Some(ScreenError::NoSource));
    }

    #[test]
    fn downscale_keeps_aspect_ratio_and_averages() {
        let flat = Frame {
            width: 400,
            height: 200,
            rgba: [100, 150, 200, 255].repeat(400 * 200),
        };
        let small = flat.downscaled(100);
        assert_eq!((small.width, small.height), (100, 50));
        assert_eq!(&small.rgba[..4], &[100, 150, 200, 255]);
        let same = flat.downscaled(1000);
        assert_eq!((same.width, same.height), (400, 200));
    }

    #[test]
    fn png_has_signature_and_data_url_prefix() {
        let png = gradient(16, 16).to_png().expect("PNG");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert!(png_data_url(&png).starts_with("data:image/png;base64,iVBOR"));
    }
}
