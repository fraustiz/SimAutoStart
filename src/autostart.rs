//! Démarrage avec Windows : valeur `SimAutoStart` dans
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, qui lance l'exe avec `--startup`
//! (l'appli démarre alors cachée dans la zone de notification).
//!
//! Les opérations de bas niveau prennent un `Slot` (clé + nom de valeur) : les tests utilisent
//! une clé jetable et ne touchent jamais à la vraie clé `Run` de l'utilisateur.

use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};

/// Emplacement d'une valeur du registre (sous `HKEY_CURRENT_USER`).
struct Slot {
    key: &'static str,
    name: &'static str,
}

/// L'entrée réelle : celle que Windows lit à l'ouverture de session.
const REAL: Slot = Slot {
    key: "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
    name: "SimAutoStart",
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Ouvre la clé ; `create` la crée si elle n'existe pas (utile aux tests seulement).
fn open(slot: &Slot, create: bool) -> Option<HKEY> {
    let mut key: HKEY = null_mut();
    let path = wide(slot.key);
    let status = unsafe {
        if create {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | KEY_QUERY_VALUE,
                null(),
                &mut key,
                null_mut(),
            )
        } else {
            RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_SET_VALUE | KEY_QUERY_VALUE, &mut key)
        }
    };
    (status == ERROR_SUCCESS).then_some(key)
}

/// Lit la commande enregistrée, ou `None` si la valeur n'existe pas.
fn read(slot: &Slot) -> Option<String> {
    let key = open(slot, false)?;
    let name = wide(slot.name);
    let mut size = 0u32;
    let mut value_type = 0u32;
    let mut text = None;
    unsafe {
        let probe =
            RegQueryValueExW(key, name.as_ptr(), null(), &mut value_type, null_mut(), &mut size);
        if probe == ERROR_SUCCESS && size > 0 {
            let mut buffer = vec![0u16; size as usize / 2 + 1];
            let status = RegQueryValueExW(
                key,
                name.as_ptr(),
                null(),
                &mut value_type,
                buffer.as_mut_ptr().cast::<u8>(),
                &mut size,
            );
            if status == ERROR_SUCCESS {
                let units = &buffer[..(size as usize / 2).min(buffer.len())];
                text = Some(String::from_utf16_lossy(units).trim_end_matches('\0').to_string());
            }
        }
        RegCloseKey(key);
    }
    text
}

fn write(slot: &Slot, create: bool, command: &str) -> bool {
    let Some(key) = open(slot, create) else {
        return false;
    };
    let data = wide(command);
    let status = unsafe {
        let status = RegSetValueExW(
            key,
            wide(slot.name).as_ptr(),
            0,
            REG_SZ,
            data.as_ptr().cast::<u8>(),
            (data.len() * 2) as u32,
        );
        RegCloseKey(key);
        status
    };
    status == ERROR_SUCCESS
}

fn remove(slot: &Slot) {
    if let Some(key) = open(slot, false) {
        // Supprimer une valeur absente n'est pas une erreur pour nous.
        unsafe {
            RegDeleteValueW(key, wide(slot.name).as_ptr());
            RegCloseKey(key);
        }
    }
}

/// Chemin de l'exécutable dans une commande : `"C:\chemin\app.exe" --startup` ou `C:\app.exe -x`.
fn executable_of(command: &str) -> String {
    let command = command.trim();
    match command.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or("").to_string(),
        None => command.split_whitespace().next().unwrap_or("").to_string(),
    }
}

/// Ne réécrit l'entrée que si l'exécutable enregistré n'existe plus (appli déplacée ou
/// supprimée). Si il existe encore, on n'y touche pas : lancer une autre copie (version de
/// développement, archive décompressée…) ne doit pas détourner le démarrage de l'utilisateur.
fn repair(slot: &Slot, current_command: &str) {
    if let Some(registered) = read(slot) {
        if !Path::new(&executable_of(&registered)).exists() {
            write(slot, false, current_command);
        }
    }
}

/// Commande à enregistrer : chemin de l'exécutable entre guillemets, puis `--startup`.
fn command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\" --startup", exe.display()))
}

/// Vrai si l'entrée de démarrage existe (lecture du registre : instantanée).
pub fn is_enabled() -> bool {
    read(&REAL).is_some()
}

/// Crée ou supprime l'entrée. Renvoie `false` en cas d'échec.
pub fn set_enabled(enabled: bool) -> bool {
    if enabled {
        command().is_some_and(|command| write(&REAL, false, &command))
    } else {
        remove(&REAL);
        true
    }
}

/// Répare l'entrée si elle existe mais que son exécutable a disparu.
pub fn refresh_if_enabled() {
    if let Some(command) = command() {
        repair(&REAL, &command);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Registry::RegDeleteKeyW;

    // Clés jetables, une par test (cargo les exécute en parallèle) : ces tests n'écrivent
    // jamais dans la vraie clé `Run`.
    const ROUND_TRIP: Slot = Slot { key: "Software\\SimAutoStartTestRoundTrip", name: "Entry" };
    const REPAIR: Slot = Slot { key: "Software\\SimAutoStartTestRepair", name: "Entry" };

    fn cleanup(slot: &Slot) {
        remove(slot);
        unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, wide(slot.key).as_ptr()) };
    }

    #[test]
    fn executable_is_extracted_from_commands() {
        assert_eq!(executable_of("\"C:\\a b\\app.exe\" --startup"), "C:\\a b\\app.exe");
        assert_eq!(executable_of("C:\\app.exe --startup"), "C:\\app.exe");
        assert_eq!(executable_of("  \"C:\\x.exe\"  "), "C:\\x.exe");
        assert_eq!(executable_of(""), "");
    }

    #[test]
    fn write_read_remove_round_trip() {
        cleanup(&ROUND_TRIP);
        assert_eq!(read(&ROUND_TRIP), None, "la valeur ne doit pas exister au départ");

        assert!(write(&ROUND_TRIP, true, "\"C:\\Chemin avec espaces\\app.exe\" --startup"));
        assert_eq!(
            read(&ROUND_TRIP).as_deref(),
            Some("\"C:\\Chemin avec espaces\\app.exe\" --startup"),
            "la commande relue doit être identique (guillemets et espaces compris)"
        );

        remove(&ROUND_TRIP);
        assert_eq!(read(&ROUND_TRIP), None);
        remove(&ROUND_TRIP); // supprimer une valeur absente ne doit pas échouer
        cleanup(&ROUND_TRIP);
    }

    #[test]
    fn repair_only_touches_entries_whose_exe_is_gone() {
        cleanup(&REPAIR);
        let notepad = "\"C:\\Windows\\System32\\notepad.exe\" --startup";
        let new_command = "\"C:\\Windows\\System32\\cmd.exe\" --startup";

        // L'exécutable enregistré existe : on ne doit PAS le détourner vers une autre copie.
        assert!(write(&REPAIR, true, notepad));
        repair(&REPAIR, new_command);
        assert_eq!(read(&REPAIR).as_deref(), Some(notepad));

        // L'exécutable enregistré a disparu : on répare avec la commande actuelle.
        assert!(write(&REPAIR, false, "\"C:\\dossier-inexistant\\app.exe\" --startup"));
        repair(&REPAIR, new_command);
        assert_eq!(read(&REPAIR).as_deref(), Some(new_command));

        // Sans entrée, la réparation ne doit rien créer (pas d'activation en douce).
        remove(&REPAIR);
        repair(&REPAIR, new_command);
        assert_eq!(read(&REPAIR), None);
        cleanup(&REPAIR);
    }
}
