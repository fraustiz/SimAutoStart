//! Fermeture de processus par l'API Windows, sans lancer `taskkill.exe`.
//!
//! SimAutoStart n'a pas de console : chaque outil en ligne de commande qu'il lance ouvre une
//! fenêtre de console, et `taskkill` pouvait même échouer au démarrage (0xc0000142). On fait
//! donc directement ce que fait `taskkill` : `WM_CLOSE` aux fenêtres (fermeture propre), puis
//! `TerminateProcess` (arrêt forcé).

use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
};

struct Search<'a> {
    pids: &'a [u32],
    windows: usize,
}

unsafe extern "system" fn post_close(hwnd: HWND, data: LPARAM) -> i32 {
    // SAFETY : `data` pointe sur le `Search` de `request_close`, vivant pendant l'énumération.
    let search = &mut *(data as *mut Search);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if search.pids.contains(&pid) {
        PostMessageW(hwnd, WM_CLOSE, 0, 0);
        search.windows += 1;
    }
    1 // continuer l'énumération
}

/// Demande la fermeture propre : `WM_CLOSE` à chaque fenêtre de premier niveau (visible ou non,
/// comme `taskkill` sans `/F`). Renvoie le nombre de fenêtres prévenues ; 0 signifie que les
/// processus n'ont aucune fenêtre et ne peuvent pas être fermés proprement.
pub fn request_close(pids: &[u32]) -> usize {
    if pids.is_empty() {
        return 0;
    }
    let mut search = Search { pids, windows: 0 };
    unsafe {
        EnumWindows(Some(post_close), &mut search as *mut Search as LPARAM);
    }
    search.windows
}

/// Arrêt forcé. Renvoie le nombre de processus effectivement arrêtés (un processus lancé en
/// administrateur ne peut pas être arrêté par un processus normal : il est alors ignoré).
pub fn terminate(pids: &[u32]) -> usize {
    let mut stopped = 0;
    for &pid in pids {
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if handle.is_null() {
                continue;
            }
            if TerminateProcess(handle, 1) != 0 {
                stopped += 1;
            }
            CloseHandle(handle);
        }
    }
    stopped
}
