//! Zone de notification (icône + menu), fermeture vers la zone de notification et instance unique.
//!
//! `windows-reactor` 0.100 n'a pas d'icône de notification ni de moyen d'intercepter la
//! fermeture : tout passe ici par l'API Windows. L'icône est celle de l'exécutable (ressource 1).
//! La fenêtre principale est retrouvée par son titre ; sa procédure est remplacée pour que
//! `WM_CLOSE` la cache au lieu de la détruire (si l'option est active).

use crate::i18n::t;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WINDOW_TITLE: &str = "SimAutoStart";
const WM_TRAY: u32 = WM_APP + 1;
const ID_OPEN: usize = 1;
const ID_QUIT: usize = 2;

static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
static ORIGINAL_PROC: AtomicIsize = AtomicIsize::new(0);
static TRAY_HWND: AtomicIsize = AtomicIsize::new(0);
static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(true);

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Active ou non la mise en zone de notification à la fermeture.
pub fn set_close_to_tray(on: bool) {
    CLOSE_TO_TRAY.store(on, Ordering::Relaxed);
}

/// Retire l'icône de la zone de notification puis quitte (sinon une icône fantôme reste jusqu'au
/// prochain survol). Utilisé avant de laisser une mise à jour remplacer les fichiers.
pub fn shutdown() -> ! {
    let hwnd = TRAY_HWND.load(Ordering::Relaxed) as HWND;
    if !hwnd.is_null() {
        remove_icon(hwnd);
    }
    std::process::exit(0)
}

/// Affiche (et met au premier plan) la fenêtre principale, cachée ou réduite.
pub fn show_main_window() {
    let hwnd = MAIN_HWND.load(Ordering::Relaxed) as HWND;
    if hwnd.is_null() {
        return;
    }
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        ShowWindow(hwnd, SW_RESTORE);
        SetForegroundWindow(hwnd);
    }
}

/// Garantit une seule instance. Si une autre tourne déjà (souvent cachée dans la zone de
/// notification), on la réaffiche et on renvoie `false` : l'appelant doit quitter.
pub fn ensure_single_instance() -> bool {
    let name = wide("Local\\SimAutoStart.SingleInstance");
    unsafe {
        let mutex = CreateMutexW(null(), 0, name.as_ptr());
        if mutex.is_null() || GetLastError() != ERROR_ALREADY_EXISTS {
            // On garde le mutex ouvert pour toute la durée du processus.
            return true;
        }
        let title = wide(WINDOW_TITLE);
        let other = FindWindowW(null(), title.as_ptr());
        if !other.is_null() {
            ShowWindow(other, SW_SHOW);
            ShowWindow(other, SW_RESTORE);
            SetForegroundWindow(other);
        }
        false
    }
}

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_CLOSE && CLOSE_TO_TRAY.load(Ordering::Relaxed) {
        ShowWindow(hwnd, SW_HIDE);
        return 0;
    }
    let original = ORIGINAL_PROC.load(Ordering::Relaxed);
    // SAFETY : `original` est la procédure précédente renvoyée par SetWindowLongPtrW.
    let original: WNDPROC = std::mem::transmute::<isize, WNDPROC>(original);
    CallWindowProcW(original, hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn find_main_window(hwnd: HWND, out: LPARAM) -> i32 {
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == GetCurrentProcessId() && IsWindowVisible(hwnd) != 0 {
        let mut buffer = [0u16; 128];
        let len = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        if len > 0 && String::from_utf16_lossy(&buffer[..len as usize]) == WINDOW_TITLE {
            *(out as *mut isize) = hwnd as isize;
            return 0; // trouvé : on arrête l'énumération
        }
    }
    1
}

/// Retrouve la fenêtre principale (elle est créée un peu après le démarrage), remplace sa
/// procédure, et la cache d'emblée si l'appli a été lancée par Windows (`--startup`).
fn hook_main_window(start_hidden: bool) {
    for _ in 0..100 {
        let mut found: isize = 0;
        unsafe {
            EnumWindows(Some(find_main_window), &mut found as *mut isize as LPARAM);
        }
        if found != 0 {
            let hwnd = found as HWND;
            MAIN_HWND.store(found, Ordering::Relaxed);
            unsafe {
                let previous = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, main_proc as *const () as usize as isize);
                ORIGINAL_PROC.store(previous, Ordering::Relaxed);
                if start_hidden {
                    ShowWindow(hwnd, SW_HIDE);
                }
            }
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn add_icon(hwnd: HWND) {
    unsafe {
        let mut data: NOTIFYICONDATAW = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = 1;
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        // Ressource d'icône n° 1 de l'exécutable (voir `assets/app.rc`).
        data.hIcon = LoadIconW(GetModuleHandleW(null()), 1 as *const u16);
        for (slot, unit) in data.szTip.iter_mut().zip(wide(WINDOW_TITLE)) {
            *slot = unit;
        }
        Shell_NotifyIconW(NIM_ADD, &data);
    }
}

fn remove_icon(hwnd: HWND) {
    unsafe {
        let mut data: NOTIFYICONDATAW = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = 1;
        Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

fn show_menu(hwnd: HWND) {
    unsafe {
        let menu = CreatePopupMenu();
        AppendMenuW(menu, MF_STRING, ID_OPEN, wide(t("Open", "Ouvrir")).as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, ID_QUIT, wide(t("Quit", "Quitter")).as_ptr());
        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point);
        // Sans ça le menu ne se ferme pas quand on clique ailleurs.
        SetForegroundWindow(hwnd);
        TrackPopupMenu(menu, TPM_RIGHTBUTTON, point.x, point.y, 0, hwnd, null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
    }
}

static TASKBAR_CREATED: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn tray_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            match (lparam as u32) & 0xFFFF {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => show_main_window(),
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            match wparam & 0xFFFF {
                ID_OPEN => show_main_window(),
                ID_QUIT => {
                    remove_icon(hwnd);
                    std::process::exit(0);
                }
                _ => {}
            }
            0
        }
        _ if msg == TASKBAR_CREATED.load(Ordering::Relaxed) as u32 && msg != 0 => {
            // L'Explorateur a redémarré : l'icône a disparu, on la remet.
            add_icon(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Démarre l'icône de notification (thread dédié avec sa boucle de messages) et accroche la
/// fenêtre principale pour intercepter sa fermeture.
pub fn start(start_hidden: bool) {
    std::thread::spawn(move || hook_main_window(start_hidden));
    std::thread::spawn(|| unsafe {
        let instance = GetModuleHandleW(null());
        let class_name = wide("SimAutoStartTray");
        let mut class: WNDCLASSW = std::mem::zeroed();
        class.lpfnWndProc = Some(tray_proc);
        class.hInstance = instance;
        class.lpszClassName = class_name.as_ptr();
        RegisterClassW(&class);
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) as isize,
            Ordering::Relaxed,
        );
        // Fenêtre cachée de premier niveau (et non « message-only ») pour recevoir
        // aussi le message de diffusion TaskbarCreated.
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            wide("SimAutoStartTray").as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        TRAY_HWND.store(hwnd as isize, Ordering::Relaxed);
        add_icon(hwnd);
        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    });
}
