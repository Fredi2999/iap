//! Bildschirm-, Fenster- und Bereichsaufnahme über GDI (Windows).
//!
//! GDI liefert die sichtbare Desktop-Fläche und über `PrintWindow` auch
//! einzelne Fenster. Inhalte mit Kopierschutz (DRM-Video, manche
//! Sicherheitsfenster) kommen als schwarzes Bild an; das erkennt
//! [`super::Frame::is_blank`], und IAP meldet es verständlich.
//! Die Koordinaten sind physische Pixel, weil Tauri per-Monitor-DPI-bewusst läuft.

use std::{ffi::c_void, mem::size_of, ptr};

use pa_types::screen::{MonitorInfo, ScreenSources, ScreenTarget, WindowInfo};
use windows::{
    core::BOOL,
    Win32::{
        Foundation::{HWND, LPARAM, RECT},
        Graphics::Gdi::{
            BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
            EnumDisplayMonitors, GetDC, GetDIBits, GetMonitorInfoW, ReleaseDC, SelectObject,
            BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HBITMAP, HDC,
            HGDIOBJ, HMONITOR, MONITORINFO, MONITORINFOEXW, SRCCOPY,
        },
        Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS},
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowLongW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
            IsIconic, IsWindowVisible, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
        },
    },
};

use super::{Frame, ScreenError};

/// `PW_RENDERFULLCONTENT`: nimmt auch Fenster mit Hardwarebeschleunigung auf.
const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(2);

struct MonitorRect {
    info: MonitorInfo,
    rect: RECT,
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _dc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let list = &mut *(data.0 as *mut Vec<MonitorRect>);
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(monitor, ptr::addr_of_mut!(info).cast::<MONITORINFO>()).as_bool() {
        let rc = info.monitorInfo.rcMonitor;
        let name_len = info
            .szDevice
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(info.szDevice.len());
        let index = list.len() as u32;
        list.push(MonitorRect {
            info: MonitorInfo {
                index,
                name: String::from_utf16_lossy(&info.szDevice[..name_len]),
                x: rc.left,
                y: rc.top,
                width: u32::try_from(rc.right - rc.left).unwrap_or(0),
                height: u32::try_from(rc.bottom - rc.top).unwrap_or(0),
                primary: info.monitorInfo.dwFlags & 1 != 0,
            },
            rect: rc,
        });
    }
    BOOL(1)
}

fn monitors() -> Vec<MonitorRect> {
    let mut list: Vec<MonitorRect> = Vec::new();
    // SAFETY: `list` lebt während des synchronen Aufrufs; der Rückruf läuft im selben Thread.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect_monitor),
            LPARAM(ptr::addr_of_mut!(list) as isize),
        );
    }
    list
}

struct WindowEntry {
    hwnd: HWND,
    title: String,
}

unsafe extern "system" fn collect_window(hwnd: HWND, data: LPARAM) -> BOOL {
    let list = &mut *(data.0 as *mut Vec<WindowEntry>);
    if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return BOOL(1);
    }
    if (GetWindowLongW(hwnd, GWL_EXSTYLE) as u32) & WS_EX_TOOLWINDOW.0 != 0 {
        return BOOL(1);
    }
    let mut process = 0_u32;
    GetWindowThreadProcessId(hwnd, Some(&mut process));
    if process == std::process::id() {
        // IAP' eigene Fenster (Hauptfenster, Pet, Auswahlrahmen) nie aufnehmen.
        return BOOL(1);
    }
    let mut buffer = [0_u16; 256];
    let length = GetWindowTextW(hwnd, &mut buffer);
    if length <= 0 {
        return BOOL(1);
    }
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_err()
        || rect.right - rect.left < 80
        || rect.bottom - rect.top < 60
    {
        return BOOL(1);
    }
    list.push(WindowEntry {
        hwnd,
        title: String::from_utf16_lossy(&buffer[..length as usize]),
    });
    BOOL(1)
}

fn windows() -> Vec<WindowEntry> {
    let mut list: Vec<WindowEntry> = Vec::new();
    // SAFETY: wie bei den Monitoren; synchroner Rückruf im selben Thread.
    unsafe {
        let _ = EnumWindows(
            Some(collect_window),
            LPARAM(ptr::addr_of_mut!(list) as isize),
        );
    }
    list
}

/// Wählbare Monitore und Fenster.
pub fn list_sources() -> ScreenSources {
    ScreenSources {
        monitors: monitors().into_iter().map(|m| m.info).collect(),
        windows: windows()
            .into_iter()
            .map(|w| WindowInfo {
                handle: w.hwnd.0 as i64,
                title: w.title,
            })
            .collect(),
    }
}

/// Liest eine Bitmap als RGBA (aus BGRA, von oben nach unten).
///
/// # Safety
/// `dc` und `bitmap` müssen gültig und `bitmap` in keinen anderen DC gewählt sein.
unsafe fn read_pixels(
    dc: HDC,
    bitmap: HBITMAP,
    width: i32,
    height: i32,
) -> Result<Frame, ScreenError> {
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bgra = vec![0_u8; (width as usize) * (height as usize) * 4];
    let lines = GetDIBits(
        dc,
        bitmap,
        0,
        height as u32,
        Some(bgra.as_mut_ptr().cast::<c_void>()),
        &mut info,
        DIB_RGB_COLORS,
    );
    if lines == 0 {
        return Err(ScreenError::Os(
            "Bilddaten konnten nicht gelesen werden".to_owned(),
        ));
    }
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
        px[3] = 255;
    }
    Ok(Frame {
        width: width as u32,
        height: height as u32,
        rgba: bgra,
    })
}

/// Nimmt ein Rechteck der Desktop-Fläche auf.
fn capture_desktop_rect(rect: RECT) -> Result<Frame, ScreenError> {
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return Err(ScreenError::NoSource);
    }
    // SAFETY: alle GDI-Objekte werden in jedem Pfad wieder freigegeben.
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return Err(ScreenError::Os(
                "Kein Zugriff auf die Bildschirmfläche".to_owned(),
            ));
        }
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
        let copied = BitBlt(
            memory,
            0,
            0,
            width,
            height,
            Some(screen),
            rect.left,
            rect.top,
            SRCCOPY | CAPTUREBLT,
        );
        SelectObject(memory, previous);
        let result = match copied {
            Ok(()) => read_pixels(memory, bitmap, width, height),
            Err(error) => Err(ScreenError::Os(error.message())),
        };
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        result
    }
}

/// Nimmt ein einzelnes Fenster auf.
fn capture_window(hwnd: HWND) -> Result<Frame, ScreenError> {
    // SAFETY: wie oben; das Fenster kann jederzeit verschwinden, Fehler werden gemeldet.
    unsafe {
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).map_err(|_| ScreenError::NoSource)?;
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 {
            return Err(ScreenError::NoSource);
        }
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
        let printed = PrintWindow(hwnd, memory, PW_RENDERFULLCONTENT).as_bool();
        SelectObject(memory, previous);
        let result = if printed {
            read_pixels(memory, bitmap, width, height)
        } else {
            Err(ScreenError::Protected)
        };
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        result
    }
}

/// Nimmt genau das Gewählte auf und liefert Bild und Beschriftung.
pub fn capture(target: &ScreenTarget) -> Result<(Frame, String), ScreenError> {
    let (frame, label) = match target {
        ScreenTarget::Monitor { index } => {
            let list = monitors();
            let monitor = list
                .iter()
                .find(|m| m.info.index == *index)
                .ok_or(ScreenError::NoSource)?;
            (
                capture_desktop_rect(monitor.rect)?,
                format!("Bildschirm {}", index + 1),
            )
        }
        ScreenTarget::Window { handle } => {
            let entry = windows()
                .into_iter()
                .find(|w| w.hwnd.0 as i64 == *handle)
                .ok_or(ScreenError::NoSource)?;
            (
                capture_window(entry.hwnd)?,
                format!("Fenster „{}“", entry.title),
            )
        }
        ScreenTarget::Region {
            monitor,
            x,
            y,
            width,
            height,
        } => {
            let list = monitors();
            let m = list
                .iter()
                .find(|m| m.info.index == *monitor)
                .ok_or(ScreenError::NoSource)?;
            let full = capture_desktop_rect(m.rect)?;
            (
                full.crop(*x, *y, *width, *height)?,
                format!("Bereich auf Bildschirm {}", monitor + 1),
            )
        }
    };
    if frame.is_blank() {
        return Err(ScreenError::Protected);
    }
    Ok((frame, label))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_least_one_monitor_is_listed_on_a_desktop_session() {
        // In einer Sitzung ohne Desktop (z. B. Dienstkonto) kann die Liste leer sein.
        let sources = list_sources();
        if sources.monitors.is_empty() {
            return;
        }
        assert!(sources.monitors.iter().any(|m| m.primary));
        assert!(sources.monitors.iter().all(|m| m.width > 0 && m.height > 0));
    }
}
