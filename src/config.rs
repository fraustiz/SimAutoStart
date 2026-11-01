//! Configuration persistée dans `%LOCALAPPDATA%\SimAutoStart\config.json`.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Config {
    /// id du simulateur -> ids des logiciels à lancer avec lui.
    #[serde(default)]
    pub links: BTreeMap<String, BTreeSet<String>>,
    /// id du logiciel/simulateur -> chemin de l'exécutable appris.
    #[serde(default)]
    pub known_paths: BTreeMap<String, String>,
    /// Fermer les logiciels lancés par nous quand le simulateur se ferme.
    #[serde(default)]
    pub stop_on_exit: bool,
    /// Lancement automatique actif.
    #[serde(default = "yes")]
    pub auto_launch: bool,
    /// id d'une application du catalogue -> nom de processus réel (voir « Localiser… »).
    #[serde(default)]
    pub process_overrides: BTreeMap<String, String>,
    /// Fermer la fenêtre la cache dans la zone de notification au lieu de quitter.
    #[serde(default = "yes")]
    pub close_to_tray: bool,
    /// Simulateurs ajoutés par l'utilisateur (hors catalogue).
    #[serde(default)]
    pub custom_sims: Vec<CustomEntry>,
    /// Applications ajoutées par l'utilisateur (hors catalogue).
    #[serde(default)]
    pub custom_apps: Vec<CustomEntry>,
}

/// Entrée personnalisée ; le chemin de l'exécutable est dans `known_paths[id]`.
#[derive(Serialize, Deserialize, Clone)]
pub struct CustomEntry {
    pub id: String,
    pub name: String,
    /// Nom du processus (nom du fichier sans `.exe`).
    pub process: String,
}

fn yes() -> bool {
    true
}

fn path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("SimAutoStart").join("config.json")
}

impl Config {
    pub fn load() -> Self {
        let mut config: Config = std::fs::read_to_string(path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        if !path().exists() {
            config.auto_launch = true;
            config.close_to_tray = true;
        }
        config
    }

    pub fn save(&self) {
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, text);
        }
    }

    pub fn is_linked(&self, sim: &str, companion: &str) -> bool {
        self.links.get(sim).is_some_and(|set| set.contains(companion))
    }

    pub fn set_link(&mut self, sim: &str, companion: &str, on: bool) {
        let set = self.links.entry(sim.to_string()).or_default();
        if on {
            set.insert(companion.to_string());
        } else {
            set.remove(companion);
        }
    }
}
