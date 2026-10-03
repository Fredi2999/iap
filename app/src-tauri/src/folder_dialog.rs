//! Systemdialog „Ordner auswählen“.
//!
//! Warum ein eigener Dialog statt Pfade tippen: Wer ein Projekt oder einen
//! Skill wählt, soll nicht Laufwerksbuchstaben und Backslashes von Hand
//! schreiben. Der Dialog gibt nur einen Pfad zurück; gelesen oder geschrieben
//! wird dort nichts. Jede spätere Nutzung läuft wie bisher durch die Policy.

use std::path::PathBuf;

/// Öffnet den Ordnerdialog und liefert den gewählten Ordner, `None` bei Abbruch.
///
/// Blockiert den aufrufenden Thread, bis der Nutzer den Dialog schließt; deshalb
/// nur aus einem Hintergrundthread aufrufen.
///
/// # Errors
/// Textmeldung des Betriebssystems, wenn der Dialog nicht geöffnet werden kann.
#[cfg(windows)]
pub fn pick_folder(owner: Option<isize>, title: &str) -> Result<Option<PathBuf>, String> {
    use windows::{
        core::{HRESULT, HSTRING},
        Win32::{
            Foundation::HWND,
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
            },
            UI::Shell::{
                FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS,
                SIGDN_FILESYSPATH,
            },
        },
    };

    /// `HRESULT_FROM_WIN32(ERROR_CANCELLED)`: der Nutzer hat abgebrochen.
    const CANCELLED: HRESULT = HRESULT(0x8007_04C7_u32 as i32);

    // SAFETY: COM wird auf diesem Thread initialisiert und am Ende wieder freigegeben;
    // der Zeiger aus GetDisplayName wird genau einmal mit CoTaskMemFree freigegeben.
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let result = (|| -> windows::core::Result<Option<PathBuf>> {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            let options = dialog.GetOptions()?;
            dialog.SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)?;
            dialog.SetTitle(&HSTRING::from(title))?;
            let owner = owner.map_or(HWND::default(), |handle| HWND(handle as *mut _));
            if let Err(error) = dialog.Show(Some(owner)) {
                return if error.code() == CANCELLED {
                    Ok(None)
                } else {
                    Err(error)
                };
            }
            let item = dialog.GetResult()?;
            let name = item.GetDisplayName(SIGDN_FILESYSPATH)?;
            let path = name.to_string();
            CoTaskMemFree(Some(name.0.cast_const().cast()));
            Ok(Some(PathBuf::from(path.map_err(|_| {
                windows::core::Error::from(HRESULT(0x8007_000D_u32 as i32))
            })?)))
        })();
        if initialized {
            CoUninitialize();
        }
        result.map_err(|error| error.message())
    }
}

/// Auf nicht geprüften Plattformen gibt es keinen Dialog.
///
/// # Errors
/// Immer: die Plattform ist noch nicht geprüft.
#[cfg(not(windows))]
pub fn pick_folder(_owner: Option<isize>, _title: &str) -> Result<Option<PathBuf>, String> {
    Err("Der Ordnerdialog ist auf dieser Plattform noch nicht eingerichtet.".to_owned())
}

/// Befehl für die Oberfläche: Ordner wählen. `None` heißt abgebrochen.
#[tauri::command(async)]
pub async fn pick_folder_dialog(
    app: tauri::AppHandle,
    title: String,
) -> crate::AppResult<Option<String>> {
    #[cfg(windows)]
    let owner = {
        use tauri::Manager;
        app.get_webview_window("main")
            .and_then(|window| window.hwnd().ok())
            .map(|hwnd| hwnd.0 as isize)
    };
    // Nur Windows kennt ein Fenster-Handle als Besitzer des Dialogs.
    #[cfg(not(windows))]
    let owner = {
        let _ = &app;
        None
    };
    tauri::async_runtime::spawn_blocking(move || pick_folder(owner, &title))
        .await
        .map_err(|e| crate::AppError::Internal(e.to_string()))?
        .map(|path| path.map(|p| p.to_string_lossy().into_owned()))
        .map_err(crate::AppError::Invalid)
}
