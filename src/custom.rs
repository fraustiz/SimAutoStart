//! Simulateurs et applications ajoutés par l'utilisateur à partir d'un exécutable.

use crate::catalog;
use crate::config::{Config, CustomEntry};
use std::path::Path;

/// Enregistre dans le catalogue en mémoire les entrées sauvegardées dans la config.
pub fn load(config: &Config) {
    for (id, process) in &config.process_overrides {
        catalog::set_process_override(id, process);
    }
    for sim in &config.custom_sims {
        catalog::register_sim(&sim.id, &sim.name, &sim.process);
    }
    for app in &config.custom_apps {
        catalog::register_companion(&app.id, &app.name, &app.process);
    }
}

fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Ajoute un simulateur (`is_sim`) ou une application à partir de son exécutable.
/// Renvoie l'identifiant. Si le catalogue connaît déjà ce programme (même nom de processus),
/// seul son chemin est mémorisé et l'identifiant du catalogue est renvoyé.
pub fn add(config: &mut Config, is_sim: bool, exe: &Path) -> Option<String> {
    let stem = exe.file_stem()?.to_string_lossy().to_string();
    let path = exe.to_string_lossy().to_string();

    let known = if is_sim {
        catalog::sims()
            .into_iter()
            .find(|s| s.processes.iter().any(|p| p.eq_ignore_ascii_case(&stem)))
            .map(|s| s.id)
    } else {
        catalog::companions()
            .into_iter()
            .find(|c| catalog::process_matches(catalog::process_of(c), &stem))
            .map(|c| c.id)
    };
    if let Some(id) = known {
        config.known_paths.insert(id.to_string(), path);
        return Some(id.to_string());
    }

    let id = format!("custom-{}-{}", if is_sim { "sim" } else { "app" }, slug(&stem));
    let list = if is_sim { &mut config.custom_sims } else { &mut config.custom_apps };
    if !list.iter().any(|e| e.id == id) {
        list.push(CustomEntry {
            id: id.clone(),
            name: stem.clone(),
            process: stem.clone(),
        });
    }
    if is_sim {
        catalog::register_sim(&id, &stem, &stem);
    } else {
        catalog::register_companion(&id, &stem, &stem);
    }
    config.known_paths.insert(id.clone(), path);
    Some(id)
}

/// « Localiser… » : associe une application du catalogue à son exécutable réel.
/// Si le nom du fichier diffère de celui du catalogue, il devient le nom de processus utilisé.
pub fn locate(config: &mut Config, id: &str, exe: &Path) {
    let Some(companion) = catalog::companion(id) else {
        return;
    };
    let Some(stem) = exe.file_stem().map(|s| s.to_string_lossy().to_string()) else {
        return;
    };
    config
        .known_paths
        .insert(id.to_string(), exe.to_string_lossy().to_string());
    if catalog::process_matches(companion.process, &stem) {
        config.process_overrides.remove(id);
        catalog::clear_process_override(id);
    } else {
        config.process_overrides.insert(id.to_string(), stem.clone());
        catalog::set_process_override(id, &stem);
    }
}

/// Retire une entrée personnalisée et toutes ses références (liens, chemin appris).
pub fn remove(config: &mut Config, id: &str) {
    config.custom_sims.retain(|e| e.id != id);
    config.custom_apps.retain(|e| e.id != id);
    config.known_paths.remove(id);
    config.links.remove(id);
    for set in config.links.values_mut() {
        set.remove(id);
    }
    catalog::unregister(id);
}

/// Ouvre la boîte de dialogue Windows « Ouvrir » filtrée sur les `.exe`.
/// Bloquant : à appeler depuis une tâche de fond. `None` si l'utilisateur annule.
pub fn pick_exe(title: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let script = "Add-Type -AssemblyName System.Windows.Forms; \
        [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
        $owner = New-Object System.Windows.Forms.Form -Property @{ TopMost = $true; ShowInTaskbar = $false }; \
        $d = New-Object System.Windows.Forms.OpenFileDialog; \
        $d.Title = $env:SAS_TITLE; \
        $d.Filter = 'Applications (*.exe)|*.exe'; \
        if ($d.ShowDialog($owner) -eq 'OK') { Write-Output $d.FileName }";
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-STA", "-Command", script])
        .env("SAS_TITLE", title)
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
}
