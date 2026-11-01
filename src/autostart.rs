//! Démarrage avec Windows : valeur `SimAutoStart` dans
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, qui lance l'exe avec `--startup`
//! (l'appli démarre alors cachée dans la zone de notification).

use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "SimAutoStart";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn open_run_key() -> Option<HKEY> {
    let mut key: HKEY = null_mut();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            0,
            KEY_SET_VALUE | KEY_QUERY_VALUE,
            &mut key,
        )
    };
    (status == ERROR_SUCCESS).then_some(key)
}

/// Commande enregistrée : chemin de l'exécutable entre guillemets, puis `--startup`.
fn command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\" --startup", exe.display()))
}

/// Vrai si l'entrée de démarrage existe (lecture du registre : instantanée).
pub fn is_enabled() -> bool {
    let Some(key) = open_run_key() else {
        return false;
    };
    let status = unsafe {
        RegQueryValueExW(key, wide(VALUE_NAME).as_ptr(), null(), null_mut(), null_mut(), null_mut())
    };
    unsafe { RegCloseKey(key) };
    status == ERROR_SUCCESS
}

/// Crée ou supprime l'entrée. Renvoie `false` en cas d'échec.
pub fn set_enabled(enabled: bool) -> bool {
    let Some(key) = open_run_key() else {
        return false;
    };
    let name = wide(VALUE_NAME);
    let ok = if enabled {
        match command() {
            Some(command) => {
                let data = wide(&command);
                let status = unsafe {
                    RegSetValueExW(
                        key,
                        name.as_ptr(),
                        0,
                        REG_SZ,
                        data.as_ptr().cast::<u8>(),
                        (data.len() * 2) as u32,
                    )
                };
                status == ERROR_SUCCESS
            }
            None => false,
        }
    } else {
        // Supprimer une valeur absente n'est pas une erreur pour nous.
        unsafe { RegDeleteValueW(key, name.as_ptr()) };
        true
    };
    unsafe { RegCloseKey(key) };
    ok
}

/// Si l'entrée existe, la réécrit avec le chemin actuel (au cas où l'exe a été déplacé).
pub fn refresh_if_enabled() {
    if is_enabled() {
        set_enabled(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Écrit réellement dans HKCU\...\Run, puis remet l'état d'origine.
    #[test]
    fn enable_then_disable_round_trip() {
        let initially = is_enabled();

        assert!(set_enabled(true));
        assert!(is_enabled(), "l'entrée doit exister après activation");
        let output = std::process::Command::new("reg")
            .args(["query", &format!("HKCU\\{RUN_KEY}"), "/v", VALUE_NAME])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        assert!(text.contains("--startup"), "commande enregistrée : {text}");
        assert!(text.contains(".exe\" --startup"), "chemin entre guillemets : {text}");

        assert!(set_enabled(false));
        assert!(!is_enabled(), "l'entrée doit disparaître après désactivation");

        // Supprimer une entrée absente ne doit pas échouer.
        assert!(set_enabled(false));

        if initially {
            set_enabled(true);
        }
    }
}
