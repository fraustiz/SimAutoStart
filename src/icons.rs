//! Logos des simulateurs : l'icône de l'exécutable est extraite une fois en PNG
//! dans `%LOCALAPPDATA%\SimAutoStart\icons\<id>.png`, puis réutilisée.

use crate::catalog::{self, Sim};
use crate::detect;
use crate::engine::State;
use std::collections::HashSet;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn icons_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("SimAutoStart").join("icons")
}

/// Chemin du PNG d'un simulateur, s'il a déjà été extrait.
pub fn icon_file(sim_id: &str) -> Option<PathBuf> {
    let path = icons_dir().join(format!("{sim_id}.png"));
    path.exists().then_some(path)
}

/// Cherche l'exécutable d'un simulateur : chemin appris, chemins par défaut, puis dossier Steam.
fn find_exe(sim: &Sim, known: Option<&String>, libraries: &[PathBuf]) -> Option<PathBuf> {
    if let Some(path) = known.map(PathBuf::from).filter(|p| p.exists()) {
        return Some(path);
    }
    if let Some(path) = sim.paths.iter().map(|p| detect::expand(p)).find(|p| p.exists()) {
        return Some(path);
    }
    if let Some(exe) = detect::software_dir(sim).and_then(|d| exe_in_dir(&d, sim)) {
        return Some(exe);
    }
    let dir = sim.steam_dir?;
    libraries
        .iter()
        .map(|lib| lib.join("common").join(dir))
        .filter(|d| d.exists())
        .find_map(|d| exe_in_dir(&d, sim))
}

/// Exécutable portant le nom d'un processus du jeu (jusqu'à 3 niveaux), sinon le plus gros `.exe`
/// de la racine qui ne ressemble pas à un installeur ou à un rapport de plantage.
fn exe_in_dir(root: &Path, sim: &Sim) -> Option<PathBuf> {
    let wanted: Vec<String> = sim
        .processes
        .iter()
        .map(|p| format!("{}.exe", p.to_lowercase()))
        .collect();
    let mut level = vec![root.to_path_buf()];
    for _ in 0..3 {
        let mut next = Vec::new();
        for dir in &level {
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    next.push(path);
                } else if path
                    .file_name()
                    .is_some_and(|n| wanted.contains(&n.to_string_lossy().to_lowercase()))
                {
                    return Some(path);
                }
            }
        }
        level = next;
    }
    let skip = ["unins", "crash", "setup", "redist", "vc_", "dxsetup", "installer", "report"];
    std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"))
                && !skip.iter().any(|s| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().to_lowercase().contains(s))
                })
        })
        .max_by_key(|p| std::fs::metadata(p).map_or(0, |m| m.len()))
}

/// Extrait l'icône associée à `exe` vers `out` (PNG) via System.Drawing, sans fenêtre visible.
fn extract(exe: &Path, out: &Path) -> bool {
    if let Some(dir) = out.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let script = "Add-Type -AssemblyName System.Drawing; \
        $i = [System.Drawing.Icon]::ExtractAssociatedIcon($env:SAS_EXE); \
        if ($i -eq $null) { exit 1 }; \
        $i.ToBitmap().Save($env:SAS_OUT, [System.Drawing.Imaging.ImageFormat]::Png)";
    Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("SAS_EXE", exe)
        .env("SAS_OUT", out)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .is_ok_and(|s| s.success())
        && out.exists()
}

/// Thread d'arrière-plan : extrait les logos manquants, puis réessaie quand un nouveau
/// chemin est appris (par exemple quand le jeu est lancé pour la première fois).
pub fn spawn(state: State, libraries: Vec<PathBuf>) {
    std::thread::spawn(move || {
        let mut attempted: HashSet<(String, PathBuf)> = HashSet::new();
        loop {
            for sim in catalog::sims() {
                let out = icons_dir().join(format!("{}.png", sim.id));
                if out.exists() {
                    continue;
                }
                let known = state.lock().unwrap().config.known_paths.get(sim.id).cloned();
                let Some(exe) = find_exe(sim, known.as_ref(), &libraries) else {
                    continue;
                };
                if attempted.insert((sim.id.to_string(), exe.clone())) {
                    extract(&exe, &out);
                }
            }
            std::thread::sleep(Duration::from_secs(10));
        }
    });
}
