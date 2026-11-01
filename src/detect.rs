//! Détection : programmes installés, processus en cours, import des profils ToLaunch.

use crate::catalog::{self, Companion, Sim};
use crate::config::Config;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use sysinfo::{ProcessesToUpdate, System};

pub fn expand(template: &str) -> PathBuf {
    let mut out = template.to_string();
    for var in ["PROGRAMFILES(X86)", "PROGRAMFILES", "LOCALAPPDATA", "APPDATA", "USERPROFILE"] {
        if let Some(value) = std::env::var_os(var) {
            out = out.replace(&format!("%{var}%"), &value.to_string_lossy());
        }
    }
    PathBuf::from(out)
}

/// Bibliothèques Steam (dossiers `steamapps`).
pub fn steam_libraries() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    // Chemin Steam depuis le registre (via reg.exe, sans dépendance supplémentaire).
    if let Ok(output) = std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"])
        .output()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        if let Some(line) = text.lines().find(|l| l.contains("SteamPath")) {
            if let Some(value) = line.split("REG_SZ").nth(1) {
                roots.push(PathBuf::from(value.trim()));
            }
        }
    }
    roots.push(expand("%PROGRAMFILES(X86)%\\Steam"));

    let mut libraries = Vec::new();
    for root in roots {
        let steamapps = root.join("steamapps");
        let vdf = steamapps.join("libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(&vdf) {
            for line in text.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("\"path\"") {
                    let value = rest.trim().trim_matches('"').replace("\\\\", "\\");
                    libraries.push(PathBuf::from(value).join("steamapps"));
                }
            }
        }
        libraries.push(steamapps);
    }
    libraries.sort();
    libraries.dedup();
    libraries.retain(|p| p.exists());
    libraries
}

/// Photographie des processus en cours : nom en minuscules -> chemin de l'exécutable.
pub struct Snapshot {
    pub processes: HashMap<String, Option<PathBuf>>,
}

impl Snapshot {
    pub fn scan(system: &mut System) -> Self {
        system.refresh_processes(ProcessesToUpdate::All, true);
        let mut processes = HashMap::new();
        for process in system.processes().values() {
            let name = process.name().to_string_lossy().to_lowercase();
            let name = name.strip_suffix(".exe").unwrap_or(&name).to_string();
            processes
                .entry(name)
                .or_insert_with(|| process.exe().map(Path::to_path_buf));
        }
        Snapshot { processes }
    }

    /// Vrai si un processus correspond à `process` (un ou plusieurs noms séparés par `|`).
    pub fn is_running(&self, process: &str) -> bool {
        self.processes
            .keys()
            .any(|name| catalog::process_matches(process, name))
    }

    /// Chemin de l'exécutable d'un processus **exactement** nommé dans `process` (les alternatives
    /// à préfixe sont ignorées : le chemin d'un exécutable versionné serait vite périmé).
    pub fn exe_of(&self, process: &str) -> Option<&PathBuf> {
        self.processes
            .iter()
            .find(|(name, _)| catalog::process_matches_exact(process, name))?
            .1
            .as_ref()
    }

    /// Noms réels (minuscules, sans `.exe`) des processus qui correspondent à `process`.
    pub fn matching_names(&self, process: &str) -> Vec<String> {
        self.processes
            .keys()
            .filter(|name| catalog::process_matches(process, name))
            .cloned()
            .collect()
    }
}

pub fn sim_running(sim: &Sim, snapshot: &Snapshot) -> bool {
    sim.processes.iter().any(|p| snapshot.is_running(p))
}

pub fn sim_installed(sim: &Sim, config: &Config, libraries: &[PathBuf]) -> bool {
    if config.known_paths.contains_key(sim.id) {
        return true;
    }
    if sim.paths.iter().any(|p| expand(p).exists()) {
        return true;
    }
    if software_dir(sim).is_some() {
        return true;
    }
    match sim.steam_dir {
        Some(dir) => steam_installed(dir, libraries),
        None => false,
    }
}

/// Programmes installés d'après « Applications installées » de Windows (clés `Uninstall`) :
/// couples (nom affiché, dossier d'installation). La lecture complète passe par PowerShell
/// (≈ 0,7 s) : elle se fait dans un thread, et le résultat est gardé sur disque pour que la
/// session suivante démarre avec la liste déjà connue (voir `warm_up_software`).
static SOFTWARE: RwLock<Option<Vec<(String, PathBuf)>>> = RwLock::new(None);

fn software_cache_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("SimAutoStart").join("software.txt")
}

/// Une ligne par programme : `nom|dossier`.
fn parse_software(text: &str) -> Vec<(String, PathBuf)> {
    text.lines()
        .filter_map(|line| {
            let (name, dir) = line.split_once('|')?;
            let dir = dir.trim().trim_matches('"');
            (!dir.is_empty()).then(|| (name.trim().to_string(), PathBuf::from(dir)))
        })
        .collect()
}

fn read_installed_software() -> Vec<(String, PathBuf)> {
    use std::os::windows::process::CommandExt;
    let script = "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
        Get-ItemProperty \
        'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*', \
        'HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*', \
        'HKCU:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*' \
        -ErrorAction SilentlyContinue | \
        Where-Object { $_.DisplayName -and $_.InstallLocation } | \
        ForEach-Object { $_.DisplayName + '|' + $_.InstallLocation }";
    let Ok(output) = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x0800_0000)
        .output()
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let software = parse_software(&text);
    if !software.is_empty() {
        // Garde le résultat pour le prochain démarrage.
        let path = software_cache_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
    software
}

/// Charge tout de suite la liste gardée sur disque (lecture de fichier : instantané, les jeux hors
/// Steam apparaissent dès l'ouverture), puis la relit dans un thread pour la tenir à jour.
pub fn warm_up_software() {
    if let Ok(text) = std::fs::read_to_string(software_cache_path()) {
        let cached = parse_software(&text);
        if !cached.is_empty() {
            *SOFTWARE.write().unwrap() = Some(cached);
        }
    }
    std::thread::spawn(|| {
        let fresh = read_installed_software();
        if !fresh.is_empty() {
            *SOFTWARE.write().unwrap() = Some(fresh);
        }
    });
}

/// Dossier d'installation d'un jeu hors Steam, retrouvé par le mot-clé de son nom dans la liste
/// des programmes installés. `None` tant que la liste n'est pas chargée.
pub fn software_dir(sim: &Sim) -> Option<PathBuf> {
    let keyword = sim.uninstall?.to_lowercase();
    let software = SOFTWARE.read().unwrap();
    software
        .as_ref()?
        .iter()
        .find(|(name, dir)| name.to_lowercase().contains(&keyword) && dir.exists())
        .map(|(_, dir)| dir.clone())
}

/// Un jeu Steam est installé s'il a un manifeste `appmanifest_*.acf` dont `installdir` correspond
/// et dont le dossier existe. Le simple dossier ne suffit pas : Steam laisse des résidus
/// (sauvegardes, `input`, `Bin`…) après une désinstallation.
pub fn steam_installed(install_dir: &str, libraries: &[PathBuf]) -> bool {
    libraries.iter().any(|lib| {
        let Ok(entries) = std::fs::read_dir(lib) else {
            return false;
        };
        entries.flatten().any(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                return false;
            }
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                return false;
            };
            text.lines().any(|line| {
                let mut parts = line.split('"').filter(|p| !p.trim().is_empty());
                parts.next() == Some("installdir")
                    && parts.next().is_some_and(|v| v.eq_ignore_ascii_case(install_dir))
            }) && lib.join("common").join(install_dir).exists()
        })
    })
}

/// Chemin exécutable utilisable pour lancer un logiciel compagnon.
pub fn companion_exe(companion: &Companion, config: &Config) -> Option<PathBuf> {
    if let Some(known) = config.known_paths.get(companion.id) {
        let path = PathBuf::from(known);
        if path.exists() {
            return Some(path);
        }
    }
    companion.paths.iter().map(|p| expand(p)).find(|p| p.exists())
}

/// Retient les chemins des programmes actuellement lancés. Renvoie vrai si la config a changé.
pub fn learn_from_running(config: &mut Config, snapshot: &Snapshot) -> bool {
    let mut changed = false;
    let mut remember = |id: &str, exe: Option<&PathBuf>| {
        if let Some(exe) = exe {
            let value = exe.to_string_lossy().to_string();
            if config.known_paths.get(id) != Some(&value) {
                config.known_paths.insert(id.to_string(), value);
                changed = true;
            }
        }
    };
    for companion in catalog::companions() {
        remember(companion.id, snapshot.exe_of(catalog::process_of(companion)));
    }
    for sim in catalog::sims() {
        if let Some(exe) = sim.processes.iter().find_map(|p| snapshot.exe_of(p)) {
            remember(sim.id, Some(exe));
        }
    }
    changed
}

/// Importe les chemins des profils ToLaunch (`%LOCALAPPDATA%\ToLaunch\Profiles`).
/// Les programmes reconnus dans le catalogue voient leur chemin mémorisé ;
/// les liaisons simulateur -> logiciel ne sont pas déduites (à choisir dans l'interface).
pub fn import_tolaunch(config: &mut Config) -> usize {
    let Some(base) = std::env::var_os("LOCALAPPDATA") else {
        return 0;
    };
    let dir = PathBuf::from(base).join("ToLaunch").join("Profiles");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut imported = 0;
    for entry in entries.flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let mut paths: Vec<String> = Vec::new();
        if let Some(main) = value.get("mainProgramPath").and_then(|v| v.as_str()) {
            paths.push(main.to_string());
        }
        if let Some(programs) = value.get("programs").and_then(|v| v.as_array()) {
            paths.extend(
                programs
                    .iter()
                    .filter_map(|p| p.get("path").and_then(|v| v.as_str()))
                    .map(str::to_string),
            );
        }
        for path in paths {
            let Some(stem) = Path::new(&path).file_stem().map(|s| s.to_string_lossy().to_lowercase())
            else {
                continue;
            };
            let id = catalog::companions()
                .into_iter()
                .find(|c| catalog::process_matches_exact(catalog::process_of(c), &stem))
                .map(|c| c.id)
                .or_else(|| {
                    catalog::sims()
                        .into_iter()
                        .find(|s| s.processes.iter().any(|p| p.to_lowercase() == stem))
                        .map(|s| s.id)
                });
            if let Some(id) = id {
                if !config.known_paths.contains_key(id) && Path::new(&path).exists() {
                    config.known_paths.insert(id.to_string(), path);
                    imported += 1;
                }
            }
        }
    }
    imported
}

pub fn companions_for(sim_id: &str) -> impl Iterator<Item = &'static Companion> {
    let sim_id = sim_id.to_string();
    catalog::companions()
        .into_iter()
        .filter(move |c| c.applies_to(&sim_id))
}

#[allow(dead_code)]
pub fn sim_by_id(id: &str) -> Option<&'static Sim> {
    catalog::sim(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(entries: &[(&str, &str)]) -> Snapshot {
        Snapshot {
            processes: entries
                .iter()
                .map(|(name, exe)| (name.to_string(), Some(PathBuf::from(exe))))
                .collect(),
        }
    }

    const SPEC: &str = "garage61-launcher|garage61-agent*";

    #[test]
    fn versioned_agent_counts_as_running_and_is_the_one_to_close() {
        let snap = snapshot(&[
            ("garage61-agent-20261008125121-34ed9ae", "C:\\x\\garage61-agent-20261008125121-34ed9ae.exe"),
            ("explorer", "C:\\Windows\\explorer.exe"),
        ]);
        assert!(snap.is_running(SPEC), "l'agent versionné doit compter comme « en cours »");
        assert_eq!(snap.matching_names(SPEC), vec!["garage61-agent-20261008125121-34ed9ae".to_string()]);
        assert!(!snapshot(&[("explorer", "C:\\Windows\\explorer.exe")]).is_running(SPEC));
    }

    #[test]
    fn the_path_of_a_versioned_agent_is_never_learned() {
        let only_agent = snapshot(&[("garage61-agent-20261008125121-34ed9ae", "C:\\a\\agent.exe")]);
        assert_eq!(only_agent.exe_of(SPEC), None, "chemin versionné : à ne pas retenir");

        let with_launcher = snapshot(&[
            ("garage61-agent-20261008125121-34ed9ae", "C:\\a\\agent.exe"),
            ("garage61-launcher", "C:\\install\\garage61-launcher.exe"),
        ]);
        assert_eq!(with_launcher.exe_of(SPEC), Some(&PathBuf::from("C:\\install\\garage61-launcher.exe")));
    }
}
