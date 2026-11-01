//! Catalogue des simulateurs et des logiciels qui se branchent dessus.
//!
//! `process` = nom de l'exécutable sans `.exe`, comparé sans tenir compte de la casse.
//! `paths` = emplacements d'installation habituels (variables `%...%` développées au runtime).
//! Les chemins réellement utilisés sont aussi appris depuis les processus en cours
//! et depuis les profils ToLaunch (voir `detect.rs`).

use std::sync::RwLock;

pub struct Sim {
    pub id: &'static str,
    pub name: &'static str,
    /// Processus du jeu (le premier qui tourne déclenche les lancements).
    pub processes: &'static [&'static str],
    /// Dossier sous `steamapps\common` si le jeu vient de Steam.
    pub steam_dir: Option<&'static str>,
    /// Mot-clé du nom dans « Applications installées » (registre Uninstall) pour les jeux hors Steam.
    pub uninstall: Option<&'static str>,
    pub paths: &'static [&'static str],
}

pub struct Companion {
    pub id: &'static str,
    pub name: &'static str,
    pub process: &'static str,
    pub paths: &'static [&'static str],
    /// Simulateurs auxquels ce logiciel s'applique ; vide = tous.
    pub only_for: &'static [&'static str],
}

pub const SIMS: &[Sim] = &[
    Sim {
        id: "iracing",
        name: "iRacing",
        // iRacingUI = le lanceur : c'est lui que l'on ouvre en premier, bien avant que le simulateur
        // (iRacingSim64DX11) ne démarre ; sans lui l'appli semble en retard sur iRacing.
        processes: &["iRacingSim64DX11", "iRacingSim64", "iRacingUI"],
        steam_dir: None,
        uninstall: Some("iRacing.com"),
        paths: &["%PROGRAMFILES(X86)%\\iRacing\\iRacingUI.exe"],
    },
    Sim {
        id: "ac",
        name: "Assetto Corsa",
        processes: &["acs", "acs_x86"],
        steam_dir: Some("assettocorsa"),
        uninstall: None,
        paths: &[],
    },
    Sim {
        id: "acc",
        name: "Assetto Corsa Competizione",
        processes: &["AC2-Win64-Shipping"],
        steam_dir: Some("Assetto Corsa Competizione"),
        uninstall: None,
        paths: &[],
    },
    Sim {
        id: "lmu",
        name: "Le Mans Ultimate",
        processes: &["Le Mans Ultimate"],
        steam_dir: Some("Le Mans Ultimate"),
        uninstall: None,
        paths: &[],
    },
    Sim {
        id: "rf2",
        name: "rFactor 2",
        processes: &["rFactor2"],
        steam_dir: Some("rFactor 2"),
        uninstall: None,
        paths: &[],
    },
    Sim {
        id: "ams2",
        name: "Automobilista 2",
        processes: &["AMS2AVX", "AMS2"],
        steam_dir: Some("Automobilista 2"),
        uninstall: None,
        paths: &[],
    },
    Sim {
        id: "raceroom",
        name: "RaceRoom Racing Experience",
        processes: &["RRRE64", "RRRE"],
        steam_dir: Some("RaceRoom Racing Experience"),
        uninstall: None,
        paths: &[],
    },
    Sim {
        id: "wrc",
        name: "EA SPORTS WRC",
        processes: &["WRC"],
        steam_dir: Some("EA SPORTS WRC"),
        uninstall: None,
        paths: &[],
    },
    Sim {
        id: "dirtrally2",
        name: "DiRT Rally 2.0",
        processes: &["dirtrally2"],
        steam_dir: Some("DiRT Rally 2.0"),
        uninstall: None,
        paths: &[],
    },
    Sim {
        id: "f125",
        name: "F1 25",
        processes: &["F1_25"],
        steam_dir: Some("F1 25"),
        uninstall: None,
        paths: &[],
    },
    Sim {
        id: "beamng",
        name: "BeamNG.drive",
        processes: &["BeamNG.drive.x64", "BeamNG.drive"],
        steam_dir: Some("BeamNG.drive"),
        uninstall: None,
        paths: &[],
    },
];

pub const COMPANIONS: &[Companion] = &[
    Companion {
        id: "simhub",
        name: "SimHub",
        process: "SimHubWPF",
        paths: &["%PROGRAMFILES(X86)%\\SimHub\\SimHubWPF.exe"],
        only_for: &[],
    },
    Companion {
        id: "crewchief",
        name: "CrewChief",
        process: "CrewChiefV4",
        paths: &["%PROGRAMFILES(X86)%\\Britton IT Ltd\\CrewChiefV4\\CrewChiefV4.exe"],
        only_for: &[],
    },
    Companion {
        id: "moza",
        name: "MOZA Pit House",
        process: "MOZA Pit House",
        paths: &["%PROGRAMFILES(X86)%\\MOZA Pit House\\MOZA Pit House.exe"],
        only_for: &[],
    },
    Companion {
        id: "garage61",
        name: "Garage 61",
        process: "garage61-launcher",
        paths: &["%APPDATA%\\garage61-install\\garage61-launcher.exe"],
        only_for: &["iracing"],
    },
    Companion {
        id: "contentmanager",
        name: "Content Manager",
        process: "Content Manager",
        paths: &["%USERPROFILE%\\Documents\\Content Manager\\Content Manager.exe"],
        only_for: &["ac"],
    },
    Companion {
        id: "tradingpaints",
        name: "Trading Paints",
        process: "Trading Paints",
        paths: &["%LOCALAPPDATA%\\Programs\\Trading Paints\\Trading Paints.exe"],
        only_for: &["iracing"],
    },
    Companion {
        id: "racelab",
        name: "RaceLab",
        process: "RaceLabApps",
        paths: &["%LOCALAPPDATA%\\Programs\\RaceLab\\RaceLabApps.exe"],
        only_for: &["iracing", "acc", "lmu", "rf2", "ams2"],
    },
    Companion {
        id: "voiceattack",
        name: "VoiceAttack",
        process: "VoiceAttack",
        paths: &["%PROGRAMFILES(X86)%\\VoiceAttack\\VoiceAttack.exe"],
        only_for: &[],
    },
    Companion {
        id: "fanalab",
        name: "Fanatec FanaLab",
        process: "FanaLab",
        paths: &["%PROGRAMFILES%\\Fanatec\\FanaLab\\FanaLab.exe"],
        only_for: &[],
    },
    Companion {
        id: "obs",
        name: "OBS Studio",
        process: "obs64",
        paths: &["%PROGRAMFILES%\\obs-studio\\bin\\64bit\\obs64.exe"],
        only_for: &[],
    },
    Companion {
        id: "kapps",
        name: "Kapps",
        process: "Kapps",
        paths: &["%LOCALAPPDATA%\\kapps\\Kapps.exe"],
        only_for: &[],
    },
    Companion {
        id: "tracktitan",
        name: "Track Titan",
        // Nom d'exécutable non confirmé : « Track Titan » (nom du produit) ou le nom interne de
        // l'appli (dossier de journaux `track-titan-ghost-application`, d'après sa FAQ).
        process: "Track Titan|track-titan-ghost-application",
        paths: &[
            "%LOCALAPPDATA%\\Programs\\Track Titan\\Track Titan.exe",
            "%LOCALAPPDATA%\\Programs\\track-titan-ghost-application\\track-titan-ghost-application.exe",
        ],
        only_for: &[],
    },
    Companion {
        id: "coachdavedelta",
        name: "Coach Dave Delta",
        process: "Coach Dave Delta",
        paths: &[],
        only_for: &[],
    },
    Companion {
        id: "z1dashboard",
        name: "Z1 Dashboard",
        process: "Z1 Dashboard",
        paths: &[],
        only_for: &[],
    },
    Companion {
        id: "truedrive",
        name: "Simucube True Drive",
        process: "TrueDrive",
        paths: &[],
        only_for: &[],
    },
    Companion {
        id: "openkneeboard",
        name: "OpenKneeboard",
        process: "OpenKneeboardApp",
        paths: &["%PROGRAMFILES%\\OpenKneeboard\\bin\\OpenKneeboardApp.exe"],
        only_for: &[],
    },
    Companion {
        id: "hwinfo",
        name: "HWiNFO",
        process: "HWiNFO64",
        paths: &["%PROGRAMFILES%\\HWiNFO64\\HWiNFO64.exe"],
        only_for: &[],
    },
    Companion {
        id: "afterburner",
        name: "MSI Afterburner",
        process: "MSIAfterburner",
        paths: &["%PROGRAMFILES(X86)%\\MSI Afterburner\\MSIAfterburner.exe"],
        only_for: &[],
    },
    Companion {
        id: "ghub",
        name: "Logitech G HUB",
        process: "lghub",
        paths: &["%PROGRAMFILES%\\LGHUB\\lghub.exe"],
        only_for: &[],
    },
    Companion {
        id: "srs",
        name: "Sim Racing Studio",
        process: "SimRacingStudio",
        paths: &[],
        only_for: &[],
    },
];

// --- Entrées personnalisées -------------------------------------------------------------
// Les simulateurs et applications ajoutés par l'utilisateur sont fusionnés avec le catalogue.
// Leurs chaînes sont « fuitées » (`Box::leak`) pour avoir la même forme `'static` que le
// catalogue : quelques octets par ajout, jamais à chaque rafraîchissement.

type Custom = (Vec<&'static Sim>, Vec<&'static Companion>);
static CUSTOM: RwLock<Custom> = RwLock::new((Vec::new(), Vec::new()));

fn leak(text: &str) -> &'static str {
    Box::leak(text.to_string().into_boxed_str())
}


// --- Surcharge du nom de processus --------------------------------------------------------
// Quand le nom d'exécutable du catalogue est faux (ou a changé), « Localiser… » mémorise le vrai.

static OVERRIDES: RwLock<Vec<(&'static str, &'static str)>> = RwLock::new(Vec::new());

pub fn set_process_override(id: &str, process: &str) {
    let mut all = OVERRIDES.write().unwrap();
    all.retain(|(known, _)| *known != id);
    all.push((leak(id), leak(process)));
}

pub fn clear_process_override(id: &str) {
    OVERRIDES.write().unwrap().retain(|(known, _)| *known != id);
}

/// Nom de processus effectivement utilisé pour une application (catalogue ou surcharge).
pub fn process_of(companion: &Companion) -> &'static str {
    OVERRIDES
        .read()
        .unwrap()
        .iter()
        .find(|(id, _)| *id == companion.id)
        .map_or(companion.process, |(_, process)| *process)
}

/// Ne garde que lettres et chiffres, en minuscules : « Track Titan », « track-titan » et
/// `TrackTitan` sont alors le même nom.
pub fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// `spec` est un nom de processus ou plusieurs séparés par `|` ; la comparaison ignore la casse,
/// les espaces, les tirets et l'extension déjà retirée.
pub fn process_matches(spec: &str, name: &str) -> bool {
    let name = normalize(name);
    spec.split('|').any(|alt| normalize(alt) == name)
}

/// Les identifiants des entrées personnalisées commencent par `custom-`.
pub fn is_custom(id: &str) -> bool {
    id.starts_with("custom-")
}

pub fn sims() -> Vec<&'static Sim> {
    let mut all: Vec<&'static Sim> = SIMS.iter().collect();
    all.extend(CUSTOM.read().unwrap().0.iter().copied());
    all
}

pub fn companions() -> Vec<&'static Companion> {
    let mut all: Vec<&'static Companion> = COMPANIONS.iter().collect();
    all.extend(CUSTOM.read().unwrap().1.iter().copied());
    all
}

pub fn sim(id: &str) -> Option<&'static Sim> {
    sims().into_iter().find(|s| s.id == id)
}

pub fn companion(id: &str) -> Option<&'static Companion> {
    companions().into_iter().find(|c| c.id == id)
}

pub fn register_sim(id: &str, name: &str, process: &str) {
    if sim(id).is_some() {
        return;
    }
    let processes: &'static [&'static str] = Box::leak(vec![leak(process)].into_boxed_slice());
    let entry: &'static Sim = Box::leak(Box::new(Sim {
        id: leak(id),
        name: leak(name),
        processes,
        steam_dir: None,
        uninstall: None,
        paths: &[],
    }));
    CUSTOM.write().unwrap().0.push(entry);
}

pub fn register_companion(id: &str, name: &str, process: &str) {
    if companion(id).is_some() {
        return;
    }
    let entry: &'static Companion = Box::leak(Box::new(Companion {
        id: leak(id),
        name: leak(name),
        process: leak(process),
        paths: &[],
        only_for: &[],
    }));
    CUSTOM.write().unwrap().1.push(entry);
}

pub fn unregister(id: &str) {
    let mut custom = CUSTOM.write().unwrap();
    custom.0.retain(|s| s.id != id);
    custom.1.retain(|c| c.id != id);
}

impl Companion {
    pub fn applies_to(&self, sim_id: &str) -> bool {
        self.only_for.is_empty() || self.only_for.contains(&sim_id)
    }
}
