//! Moteur : surveille les processus et lance/ferme les logiciels liés aux simulateurs.

use crate::catalog;
use crate::i18n::is_fr;
use crate::config::Config;
use crate::detect::{self, Snapshot};
use crate::winproc;
use std::collections::BTreeSet;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use sysinfo::System;

pub struct Shared {
    pub config: Config,
    pub running_sims: BTreeSet<String>,
    pub running_companions: BTreeSet<String>,
    /// Logiciels lancés par SimAutoStart (candidats à la fermeture).
    pub launched: BTreeSet<String>,
    pub log: Vec<String>,
}

pub type State = Arc<Mutex<Shared>>;

pub fn new_state() -> State {
    Arc::new(Mutex::new(Shared {
        config: Config::load(),
        running_sims: BTreeSet::new(),
        running_companions: BTreeSet::new(),
        launched: BTreeSet::new(),
        log: Vec::new(),
    }))
}

fn push_log(shared: &mut Shared, line: String) {
    shared.log.push(line);
    let excess = shared.log.len().saturating_sub(50);
    shared.log.drain(..excess);
}

pub fn spawn(state: State) {
    std::thread::spawn(move || {
        let mut system = System::new().expect("sysinfo");
        let mut previous: BTreeSet<String> = BTreeSet::new();
        loop {
            let snapshot = Snapshot::scan(&mut system);
            tick(&state, &snapshot, &mut previous);
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

/// Demande une fermeture propre (`WM_CLOSE` aux fenêtres), attend jusqu'à 5 s, puis force
/// l'arrêt. Sans aucune fenêtre (agent Garage 61, appli de zone de notification sans fenêtre),
/// la demande propre est impossible : arrêt forcé tout de suite. Tout passe par l'API Windows
/// (`winproc`) : aucun outil en ligne de commande, donc aucune fenêtre de console.
/// Fait dans un thread pour ne pas bloquer la surveillance.
fn close_gracefully(process: &'static str) {
    std::thread::spawn(move || {
        let mut system = System::new().expect("sysinfo");
        // Le catalogue peut donner plusieurs noms possibles : on ferme ceux qui tournent vraiment.
        let pids = detect::pids_matching(&mut system, process);
        if pids.is_empty() {
            return;
        }
        if winproc::request_close(&pids) > 0 {
            for _ in 0..10 {
                std::thread::sleep(Duration::from_millis(500));
                if detect::pids_matching(&mut system, process).is_empty() {
                    return;
                }
            }
        }
        // Seulement ceux qu'on voulait fermer et qui tournent encore (pas un homonyme relancé depuis).
        let remaining: Vec<u32> = detect::pids_matching(&mut system, process)
            .into_iter()
            .filter(|pid| pids.contains(pid))
            .collect();
        winproc::terminate(&remaining);
    });
}

fn tick(state: &State, snapshot: &Snapshot, previous: &mut BTreeSet<String>) {
    let mut shared = state.lock().unwrap();

    if detect::learn_from_running(&mut shared.config, snapshot) {
        shared.config.save();
    }

    let current: BTreeSet<String> = catalog::sims()
        .into_iter()
        .filter(|s| detect::sim_running(s, snapshot))
        .map(|s| s.id.to_string())
        .collect();
    shared.running_companions = catalog::companions()
        .into_iter()
        .filter(|c| snapshot.is_running(catalog::process_of(c)))
        .map(|c| c.id.to_string())
        .collect();

    let started: Vec<String> = current.difference(previous).cloned().collect();
    let stopped: Vec<String> = previous.difference(&current).cloned().collect();

    if shared.config.auto_launch {
        for sim_id in &started {
            let name = crate::catalog::sim(sim_id).map_or(sim_id.as_str(), |s| s.name);
            push_log(
                &mut shared,
                if is_fr() { format!("{name} détecté") } else { format!("{name} detected") },
            );
            let linked: Vec<String> = shared
                .config
                .links
                .get(sim_id)
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default();
            for companion_id in linked {
                let Some(companion) = catalog::companion(&companion_id) else {
                    continue;
                };
                if snapshot.is_running(catalog::process_of(companion)) {
                    continue;
                }
                match detect::companion_exe(companion, &shared.config) {
                    Some(exe) => {
                        let mut command = Command::new(&exe);
                        if let Some(dir) = exe.parent() {
                            command.current_dir(dir);
                        }
                        match command.spawn() {
                            Ok(_) => {
                                shared.launched.insert(companion.id.to_string());
                                push_log(
                                    &mut shared,
                                    if is_fr() {
                                        format!("{} lancé", companion.name)
                                    } else {
                                        format!("{} launched", companion.name)
                                    },
                                );
                            }
                            Err(error) => push_log(
                                &mut shared,
                                if is_fr() {
                                    format!("{} : échec du lancement ({error})", companion.name)
                                } else {
                                    format!("{}: launch failed ({error})", companion.name)
                                },
                            ),
                        }
                    }
                    None => push_log(
                        &mut shared,
                        if is_fr() {
                            format!("{} : exécutable introuvable", companion.name)
                        } else {
                            format!("{}: executable not found", companion.name)
                        },
                    ),
                }
            }
        }
    }

    if shared.config.stop_on_exit && !stopped.is_empty() {
        let still_needed: BTreeSet<String> = current
            .iter()
            .filter_map(|sim| shared.config.links.get(sim))
            .flatten()
            .cloned()
            .collect();
        let to_close: Vec<String> = shared
            .launched
            .iter()
            .filter(|id| !still_needed.contains(*id))
            .cloned()
            .collect();
        for id in to_close {
            if let Some(companion) = catalog::companion(&id) {
                close_gracefully(catalog::process_of(companion));
                push_log(
                    &mut shared,
                    if is_fr() {
                        format!("{} : fermeture demandée", companion.name)
                    } else {
                        format!("{}: close requested", companion.name)
                    },
                );
            }
            shared.launched.remove(&id);
        }
    }

    shared.running_sims = current.clone();
    *previous = current;
}
