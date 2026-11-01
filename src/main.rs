#![windows_subsystem = "windows"]

mod autostart;
mod catalog;
mod config;
mod custom;
mod detect;
mod engine;
mod i18n;
mod icons;
mod tray;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use i18n::{is_fr, t};
use windows_reactor::*;

const SETTINGS_TAG: &str = "settings";
const ADD_TAG: &str = "add";

struct SimAutoStart {
    state: engine::State,
    /// Tag de la page affichée : id d'un simulateur, ou `SETTINGS_TAG`.
    selected: String,
    /// Simulateurs installés (lecture du disque : coûteuse, refaite toutes les 10 s).
    installed_base: Vec<&'static str>,
    last_scan: Instant,
    /// `installed_base` plus les simulateurs en cours : la liste affichée.
    installed_sims: Vec<&'static str>,
    libraries: Vec<PathBuf>,
    timer: Option<ComponentTask>,
    /// Tâche du sélecteur de fichier en cours (le dialogue est bloquant).
    picker: Option<ComponentTask>,
    /// Sidebar dépliée (libellés) ou repliée (logos seuls).
    pane_open: bool,
}

/// Ce que l'utilisateur est en train d'ajouter avec le sélecteur de fichier.
#[derive(Clone)]
enum PickKind {
    Sim,
    /// Application ; si un simulateur est donné, elle y est liée tout de suite.
    App(Option<String>),
    /// Associe une application du catalogue à son exécutable (« Localiser… »).
    Locate(String),
}

#[derive(Clone)]
enum Message {
    Tick,
    AddExe(PickKind),
    Picked(PickKind, Option<String>),
    RemoveCustom(String),
    TogglePane,
    PaneOpenChanged(bool),
    SelectTag(Option<String>),
    Link(String, bool),
    AutoLaunch(bool),
    StartWithWindows(bool),
    CloseToTray(bool),
    StopOnExit(bool),
    ImportToLaunch,
}

fn keyed(views: Vec<View>) -> Vec<KeyedView> {
    views
        .into_iter()
        .enumerate()
        .map(|(i, view)| KeyedView::new(i.to_string(), view))
        .collect()
}

fn plural(n: usize) -> &'static str {
    if n > 1 {
        "s"
    } else {
        ""
    }
}

/// Logo de l'application, embarqué dans l'exécutable.
const ICON_BYTES: &[u8] = include_bytes!("../assets/icon.ico");

/// Chemin d'un fichier `.ico` pour l'icône de fenêtre : `windows-reactor` veut un chemin, alors
/// le logo embarqué est écrit une fois dans `%LOCALAPPDATA%\SimAutoStart\icon.ico`.
/// `None` si l'écriture échoue (la fenêtre garde alors l'icône de l'exécutable).
fn window_icon() -> Option<&'static str> {
    static PATH: std::sync::OnceLock<Option<&'static str>> = std::sync::OnceLock::new();
    *PATH.get_or_init(|| {
        let dir = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?.join("SimAutoStart");
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join("icon.ico");
        if std::fs::read(&path).ok().as_deref() != Some(ICON_BYTES) {
            std::fs::write(&path, ICON_BYTES).ok()?;
        }
        Some(Box::leak(path.to_string_lossy().into_owned().into_boxed_str()) as &'static str)
    })
}

/// Le « tick » de rafraîchissement : la version 0.100 n'a pas de minuteur,
/// on chaîne donc une tâche de fond qui dort 1 s puis renvoie `Tick`.
fn tick_after(context: &ComponentContext<SimAutoStart>) -> ComponentTask {
    context.spawn_background(|_| {
        std::thread::sleep(Duration::from_secs(1));
        Message::Tick
    })
}

// --- Petits composants visuels ------------------------------------------------------------

/// Logo du jeu s'il a été extrait, sinon un symbole générique.
fn sim_icon(sim_id: &str, fallback: Symbol) -> View {
    if let Some(path) = icons::icon_file(sim_id) {
        if let Ok(icon) = ImageIcon::new().source_file(path) {
            return icon.into();
        }
    }
    SymbolIcon::new().symbol(fallback).into()
}

fn nav_item(tag: &str, label: String, icon: View, selected: bool) -> View {
    NavigationViewItem::new()
        .tag(tag)
        .is_selected(selected)
        .selects_on_invoked(true)
        .slots([
            SlotView::new(NavigationViewItemSlot::Content, TextBlock::new().text(label)),
            SlotView::new(NavigationViewItemSlot::Icon, icon),
        ])
}

fn card(content: impl Into<View>) -> View {
    Border::new()
        .background(ThemeBrush::CardBackground)
        .border_brush(ThemeBrush::CardStroke)
        .border_thickness(Thickness::uniform(1.0))
        .corner_radius(8.0)
        .padding(Thickness::new(20.0, 8.0, 20.0, 8.0))
        .content(content)
}

fn divider() -> View {
    Border::new()
        .height(1.0)
        .background(ThemeBrush::CardStroke)
        .into()
}

/// Ligne « titre + description à gauche, contrôle à droite ».
fn row(title: &str, description: String, control: impl Into<View>) -> View {
    Grid::new()
        .columns([GridLength::STAR, GridLength::Auto])
        .column_spacing(16.0)
        .children((
            StackPanel::new()
                .grid_column(0)
                .vertical_alignment(VerticalAlignment::Center)
                .children((
                    TextBlock::new().text(title.to_string()),
                    TextBlock::new()
                        .text(description)
                        .font_size(12.0)
                        .opacity(0.7),
                )),
            StackPanel::new()
                .grid_column(1)
                .vertical_alignment(VerticalAlignment::Center)
                .children((control,)),
        ))
        .into()
}

/// Insère un séparateur entre chaque ligne et enveloppe le tout dans une carte.
fn card_of_rows(rows: Vec<View>) -> View {
    let mut views = Vec::new();
    for (i, view) in rows.into_iter().enumerate() {
        if i > 0 {
            views.push(divider());
        }
        views.push(view);
    }
    card(StackPanel::new().spacing(12.0).keyed_children(keyed(views)))
}

fn page_title(title: String, subtitle: String) -> View {
    StackPanel::new()
        .spacing(2.0)
        .children((
            TextBlock::new()
                .text(title)
                .font_size(28.0)
                .font_weight(FontWeight::SEMI_BOLD),
            TextBlock::new().text(subtitle).opacity(0.7),
        ))
        .into()
}

fn section_title(text: &str) -> View {
    TextBlock::new()
        .text(text.to_string())
        .font_size(16.0)
        .font_weight(FontWeight::SEMI_BOLD)
        .into()
}

impl SimAutoStart {
    /// Recalcule la liste complète des simulateurs installés (manifestes Steam, chemins…).
    fn rescan(&mut self) {
        let shared = self.state.lock().unwrap();
        self.installed_base = catalog::sims()
            .into_iter()
            .filter(|s| detect::sim_installed(s, &shared.config, &self.libraries))
            .map(|s| s.id)
            .collect();
        self.last_scan = Instant::now();
        drop(shared);
        self.merge_running();
    }

    /// Ajoute à la liste de base les simulateurs en cours (rapide : appelé chaque seconde).
    fn merge_running(&mut self) {
        let shared = self.state.lock().unwrap();
        self.installed_sims = catalog::sims()
            .into_iter()
            .filter(|s| self.installed_base.contains(&s.id) || shared.running_sims.contains(s.id))
            .map(|s| s.id)
            .collect();
    }

    // --- Barre d'état (bas de fenêtre) : quel jeu est lancé -------------------------------
    fn status_bar(&self) -> View {
        let shared = self.state.lock().unwrap();
        let running: Vec<&catalog::Sim> = catalog::sims()
            .into_iter()
            .filter(|s| shared.running_sims.contains(s.id))
            .collect();

        let left: View = match running.first() {
            None => StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(10.0)
                .vertical_alignment(VerticalAlignment::Center)
                .children((
                    TextBlock::new().text("○").opacity(0.5),
                    TextBlock::new().text(t("No game running", "Aucun jeu lancé")).opacity(0.7),
                ))
                .into(),
            Some(first) => {
                let names = running.iter().map(|s| s.name).collect::<Vec<_>>().join(", ");
                let logo: View = match icons::icon_file(first.id)
                    .and_then(|p| Image::new().source_file(p).ok())
                {
                    Some(image) => image.width(20.0).height(20.0).into(),
                    None => TextBlock::new().text("●").foreground(ThemeBrush::Accent).into(),
                };
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(10.0)
                    .vertical_alignment(VerticalAlignment::Center)
                    .children((
                        logo,
                        TextBlock::new()
                            .text(if is_fr() {
                                format!("En cours : {names}")
                            } else {
                                format!("Running: {names}")
                            })
                            .font_weight(FontWeight::SEMI_BOLD),
                    ))
                    .into()
            }
        };

        let companions_running = shared
            .running_companions
            .iter()
            .filter(|id| {
                running
                    .iter()
                    .any(|sim| shared.config.is_linked(sim.id, id))
            })
            .count();
        let right = if !shared.config.auto_launch {
            t("Auto-launch is off", "Lancement automatique désactivé").to_string()
        } else if running.is_empty() {
            t("Monitoring", "Surveillance active").to_string()
        } else {
            if is_fr() {
                format!(
                    "{companions_running} logiciel{} associé{} en cours",
                    plural(companions_running),
                    plural(companions_running)
                )
            } else {
                format!(
                    "{companions_running} linked app{} running",
                    plural(companions_running)
                )
            }
        };

        Border::new()
            .grid_row(2)
            .background(ThemeBrush::CardBackground)
            .border_brush(ThemeBrush::CardStroke)
            .border_thickness(Thickness::new(0.0, 1.0, 0.0, 0.0))
            .padding(Thickness::new(20.0, 10.0, 20.0, 10.0))
            .content(
                Grid::new()
                    .columns([GridLength::STAR, GridLength::Auto])
                    .children((
                        StackPanel::new().grid_column(0).children((left,)),
                        TextBlock::new()
                            .grid_column(1)
                            .text(right)
                            .font_size(12.0)
                            .opacity(0.7)
                            .vertical_alignment(VerticalAlignment::Center),
                    )),
            )
    }

    // --- Page d'un simulateur ---------------------------------------------------------
    fn sim_page(&self, sim_id: &str, context: &mut ViewContext<Self>) -> View {
        let shared = self.state.lock().unwrap();
        let Some(sim) = catalog::sim(sim_id) else {
            return page_title("SimAutoStart".into(), t("Pick a simulator", "Choisis un simulateur").into());
        };
        let running = shared.running_sims.contains(sim.id);
        let linked = shared.config.links.get(sim.id).map_or(0, |set| set.len());

        let state_line = if running {
            t("● Running — the selected apps are launched", "● En cours — les logiciels cochés sont lancés").to_string()
        } else {
            t("Waiting for the simulator to start", "En attente du lancement du simulateur").to_string()
        };

        let mut rows: Vec<View> = Vec::new();
        let mut known_rows: Vec<View> = Vec::new();
        for companion in detect::companions_for(sim.id) {
            let companion_running = shared.running_companions.contains(companion.id);
            let found =
                companion_running || detect::companion_exe(companion, &shared.config).is_some();
            let status = if companion_running {
                t("Running", "En cours")
            } else if found {
                t("Installed", "Installé")
            } else {
                t(
                    "Not found — run it once so SimAutoStart learns where it is",
                    "Introuvable — lance-le une fois pour que SimAutoStart l'apprenne",
                )
            };
            if !found && !catalog::is_custom(companion.id) {
                known_rows.push(row(
                    companion.name,
                    t(
                        "Not detected on this PC",
                        "Non détectée sur ce PC",
                    )
                    .to_string(),
                    Button::new()
                        .on_click(context.message(Message::AddExe(PickKind::Locate(
                            companion.id.to_string(),
                        ))))
                        .content(t("Locate…", "Localiser…")),
                ));
                continue;
            }
            let id = companion.id.to_string();
            let status = if catalog::is_custom(companion.id) {
                format!("{} · {status}", t("Custom", "Personnalisé"))
            } else {
                status.to_string()
            };
            rows.push(row(
                companion.name,
                status,
                ToggleSwitch::new()
                    .is_on(shared.config.is_linked(sim.id, companion.id))
                    .is_enabled(found)
                    .on_toggled(context.callback(move |on: bool| Message::Link(id.clone(), on))),
            ));
        }

        let summary = match linked {
            0 => t("No app selected", "Aucun logiciel sélectionné").to_string(),
            1 => t("1 app selected", "1 logiciel sélectionné").to_string(),
            n if is_fr() => format!("{n} logiciels sélectionnés"),
            n => format!("{n} apps selected"),
        };

        rows.push(row(
            t("Add an app", "Ajouter une application"),
            if is_fr() {
                format!("Choisis un .exe à lancer avec {}", sim.name)
            } else {
                format!("Pick an .exe to launch with {}", sim.name)
            },
            Button::new()
                .on_click(context.message(Message::AddExe(PickKind::App(Some(sim.id.to_string())))))
                .content(t("Browse…", "Parcourir…")),
        ));

        let mut blocks: Vec<View> = vec![
            page_title(sim.name.to_string(), state_line),
            section_title(&if is_fr() {
                format!("Lancer avec {}  ·  {summary}", sim.name)
            } else {
                format!("Launch with {}  ·  {summary}", sim.name)
            }),
            card_of_rows(rows),
        ];
        if !known_rows.is_empty() {
            blocks.push(section_title(&if is_fr() {
                format!("Apps connues  ·  {}", known_rows.len())
            } else {
                format!("Known apps  ·  {}", known_rows.len())
            }));
            blocks.push(
                TextBlock::new()
                    .text(t(
                        "Not found on this PC. Run one once, or locate its executable, to use it.",
                        "Introuvables sur ce PC. Lance-en une fois, ou localise son exécutable, pour l'utiliser.",
                    ))
                    .font_size(12.0)
                    .opacity(0.7)
                    .into(),
            );
            blocks.push(card_of_rows(known_rows));
        }
        if catalog::is_custom(sim.id) {
            blocks.push(card_of_rows(vec![row(
                t("Custom simulator", "Simulateur personnalisé"),
                shared
                    .config
                    .known_paths
                    .get(sim.id)
                    .cloned()
                    .unwrap_or_default(),
                Button::new()
                    .on_click(context.message(Message::RemoveCustom(sim.id.to_string())))
                    .content(t("Remove", "Retirer")),
            )]));
        }

        ScrollViewer::new()
            .content(StackPanel::new().spacing(20.0).keyed_children(keyed(blocks)))
            .into()
    }

    // --- Page « Ajouts » : simulateurs et applications personnalisés -------------------
    fn add_page(&self, context: &mut ViewContext<Self>) -> View {
        let shared = self.state.lock().unwrap();

        let list = |entries: &[config::CustomEntry], kind: PickKind, title: &str, hint: &str| {
            let mut rows: Vec<View> = entries
                .iter()
                .map(|e| {
                    row(
                        &e.name,
                        shared.config.known_paths.get(&e.id).cloned().unwrap_or_default(),
                        Button::new()
                            .on_click(context.message(Message::RemoveCustom(e.id.clone())))
                            .content(t("Remove", "Retirer")),
                    )
                })
                .collect();
            rows.push(row(
                title,
                hint.to_string(),
                Button::new()
                    .on_click(context.message(Message::AddExe(kind)))
                    .content(t("Browse…", "Parcourir…")),
            ));
            card_of_rows(rows)
        };

        let sims = list(
            &shared.config.custom_sims,
            PickKind::Sim,
            t("Add a simulator", "Ajouter un simulateur"),
            t(
                "Pick the game's executable: it shows up in the sidebar with its logo",
                "Choisis l'exécutable du jeu : il apparaît dans la sidebar avec son logo",
            ),
        );
        let apps = list(
            &shared.config.custom_apps,
            PickKind::App(None),
            t("Add an app", "Ajouter une application"),
            t(
                "Available for every simulator; turn it on in each simulator's page",
                "Disponible pour tous les simulateurs ; coche-la dans la page de chacun",
            ),
        );

        ScrollViewer::new()
            .content(
                StackPanel::new()
                    .spacing(20.0)
                    .keyed_children(keyed(vec![
                        page_title(
                            t("Additions", "Ajouts").into(),
                            t(
                                "Simulators and apps missing from the catalog",
                                "Simulateurs et applications absents du catalogue",
                            )
                            .into(),
                        ),
                        section_title(t("Simulators", "Simulateurs")),
                        sims,
                        section_title(t("Apps", "Applications")),
                        apps,
                    ])),
            )
            .into()
    }

    // --- Page des réglages --------------------------------------------------------------
    fn settings_page(&self, context: &mut ViewContext<Self>) -> View {
        let shared = self.state.lock().unwrap();

        let options = card_of_rows(vec![
            row(
                t("Auto-launch", "Lancement automatique"),
                t(
                    "Starts the chosen apps as soon as a simulator is detected",
                    "Démarre les logiciels choisis dès qu'un simulateur est détecté",
                )
                .to_string(),
                ToggleSwitch::new()
                    .is_on(shared.config.auto_launch)
                    .on_toggled(context.callback(Message::AutoLaunch)),
            ),
            row(
                t("Close when the simulator stops", "Fermer à l'arrêt du simulateur"),
                t(
                    "Asks for a clean close, then forces it after 5 s",
                    "Demande la fermeture propre, puis force après 5 s",
                )
                .to_string(),
                ToggleSwitch::new()
                    .is_on(shared.config.stop_on_exit)
                    .on_toggled(context.callback(Message::StopOnExit)),
            ),
            row(
                t("Start with Windows", "Démarrer avec Windows"),
                t(
                    "Opens SimAutoStart in the tray when you sign in",
                    "Ouvre SimAutoStart dans la zone de notification à l'ouverture de session",
                )
                .to_string(),
                ToggleSwitch::new()
                    .is_on(autostart::is_enabled())
                    .on_toggled(context.callback(Message::StartWithWindows)),
            ),
            row(
                t("Keep running in the tray", "Rester dans la zone de notification"),
                t(
                    "Closing the window hides it; reopen or quit from the tray icon",
                    "Fermer la fenêtre la cache ; la rouvrir ou quitter depuis la zone de notification",
                )
                .to_string(),
                ToggleSwitch::new()
                    .is_on(shared.config.close_to_tray)
                    .on_toggled(context.callback(Message::CloseToTray)),
            ),
            row(
                t("Import from ToLaunch", "Importer depuis ToLaunch"),
                t(
                    "Picks up the program paths from your ToLaunch profiles",
                    "Récupère les chemins des programmes de tes profils ToLaunch",
                )
                .to_string(),
                Button::new()
                    .on_click(context.message(Message::ImportToLaunch))
                    .content(t("Import", "Importer")),
            ),
        ]);

        let mut events: Vec<View> = shared
            .log
            .iter()
            .rev()
            .take(8)
            .map(|line| TextBlock::new().text(line.clone()).opacity(0.85).into())
            .collect();
        if events.is_empty() {
            events.push(
                TextBlock::new()
                    .text(t("No events yet", "Aucun événement pour l'instant"))
                    .opacity(0.7)
                    .into(),
            );
        }
        let activity = card(
            StackPanel::new()
                .spacing(6.0)
                .keyed_children(keyed(events)),
        );

        ScrollViewer::new()
            .content(
                StackPanel::new()
                    .spacing(20.0)
                    .keyed_children(keyed(vec![
                        page_title(
                            t("Settings", "Réglages").into(),
                            t("How monitoring behaves", "Comportement de la surveillance").into(),
                        ),
                        options.into(),
                        section_title(t("Recent activity", "Activité récente")),
                        activity.into(),
                    ])),
            )
            .into()
    }
}

impl Component for SimAutoStart {
    type Input = ();
    type Message = Message;

    fn create(_input: &(), context: &ComponentContext<Self>) -> Self {
        let state = engine::new_state();
        custom::load(&state.lock().unwrap().config);
        tray::set_close_to_tray(state.lock().unwrap().config.close_to_tray);
        autostart::refresh_if_enabled();
        // Premier import des chemins depuis ToLaunch si on n'a encore rien appris.
        {
            let mut shared = state.lock().unwrap();
            if shared.config.known_paths.is_empty() {
                detect::import_tolaunch(&mut shared.config);
                shared.config.save();
            }
        }
        engine::spawn(state.clone());
        let libraries = detect::steam_libraries();
        detect::warm_up_software();
        icons::spawn(state.clone(), libraries.clone());
        let mut app = Self {
            state,
            selected: String::new(),
            installed_base: Vec::new(),
            last_scan: Instant::now(),
            installed_sims: Vec::new(),
            libraries,
            timer: None,
            picker: None,
            pane_open: true,
        };
        app.rescan();
        app.selected = app
            .installed_sims
            .first()
            .map(|s| s.to_string())
            .unwrap_or_else(|| SETTINGS_TAG.to_string());
        app.timer = Some(tick_after(context));
        app
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::Tick => {
                if self.last_scan.elapsed() > Duration::from_secs(10) {
                    self.rescan();
                } else {
                    self.merge_running();
                }
                self.timer = Some(tick_after(context));
            }
            Message::AddExe(kind) => {
                let title = match kind {
                    PickKind::Sim => t("Choose the simulator's executable", "Choisir l'exécutable du simulateur"),
                    PickKind::App(_) => t("Choose the app to launch", "Choisir l'application à lancer"),
                    PickKind::Locate(_) => t("Locate the app's executable", "Localiser l'exécutable de l'application"),
                };
                self.picker = Some(context.spawn_background(move |_| {
                    Message::Picked(kind, custom::pick_exe(title))
                }));
            }
            Message::Picked(kind, path) => {
                self.picker = None;
                if let Some(path) = path {
                    if let PickKind::Locate(id) = &kind {
                        let mut shared = self.state.lock().unwrap();
                        custom::locate(&mut shared.config, id, Path::new(&path));
                        shared.config.save();
                        drop(shared);
                        self.rescan();
                        return;
                    }
                    let mut shared = self.state.lock().unwrap();
                    let is_sim = matches!(kind, PickKind::Sim);
                    if let Some(id) = custom::add(&mut shared.config, is_sim, Path::new(&path)) {
                        if let PickKind::App(Some(sim)) = &kind {
                            shared.config.set_link(sim, &id, true);
                        }
                        if is_sim {
                            self.selected = id;
                        }
                    }
                    shared.config.save();
                    drop(shared);
                    self.rescan();
                }
            }
            Message::RemoveCustom(id) => {
                let mut shared = self.state.lock().unwrap();
                custom::remove(&mut shared.config, &id);
                shared.config.save();
                drop(shared);
                if self.selected == id {
                    self.selected = ADD_TAG.to_string();
                }
                self.rescan();
            }
            Message::TogglePane => self.pane_open = !self.pane_open,
            Message::PaneOpenChanged(open) => self.pane_open = open,
            Message::SelectTag(Some(tag)) => self.selected = tag,
            Message::SelectTag(None) => {}
            Message::Link(companion, on) => {
                let mut shared = self.state.lock().unwrap();
                let sim = self.selected.clone();
                shared.config.set_link(&sim, &companion, on);
                shared.config.save();
            }
            Message::StartWithWindows(on) => {
                autostart::set_enabled(on);
            }
            Message::CloseToTray(on) => {
                let mut shared = self.state.lock().unwrap();
                shared.config.close_to_tray = on;
                shared.config.save();
                tray::set_close_to_tray(on);
            }
            Message::AutoLaunch(on) => {
                let mut shared = self.state.lock().unwrap();
                shared.config.auto_launch = on;
                shared.config.save();
            }
            Message::StopOnExit(on) => {
                let mut shared = self.state.lock().unwrap();
                shared.config.stop_on_exit = on;
                shared.config.save();
            }
            Message::ImportToLaunch => {
                let mut shared = self.state.lock().unwrap();
                detect::import_tolaunch(&mut shared.config);
                shared.config.save();
                drop(shared);
                self.rescan();
            }
        }
    }

    fn view(&self, _input: &(), context: &mut ViewContext<Self>) -> View {
        context.window_title("SimAutoStart");
        let visuals = WindowVisuals::new()
            .backdrop(WindowBackdrop::Mica)
            .client_size(1040.0, 700.0)
            .constraints(WindowConstraints {
                min_width: Some(820.0),
                min_height: Some(560.0),
                ..Default::default()
            });
        let visuals = match window_icon() {
            Some(path) => visuals.icon(path),
            None => visuals,
        };
        context.window_visuals(visuals);


        // --- Sidebar : un item par simulateur détecté, puis les réglages ----------------
        let mut menu: Vec<KeyedView> = Vec::new();
        {
            let shared = self.state.lock().unwrap();
            for id in &self.installed_sims {
                let Some(sim) = catalog::sim(id) else { continue };
                let label = if shared.running_sims.contains(sim.id) {
                    format!("{}   ●", sim.name)
                } else {
                    sim.name.to_string()
                };
                menu.push(KeyedView::new(
                    sim.id,
                    nav_item(
                        sim.id,
                        label,
                        sim_icon(sim.id, Symbol::Play),
                        self.selected == sim.id,
                    ),
                ));
            }
        }
        menu.push(KeyedView::new(
            ADD_TAG,
            nav_item(
                ADD_TAG,
                t("Additions", "Ajouts").to_string(),
                SymbolIcon::new().symbol(Symbol::Add).into(),
                self.selected == ADD_TAG,
            ),
        ));
        menu.push(KeyedView::new(
            SETTINGS_TAG,
            nav_item(
                SETTINGS_TAG,
                t("Settings", "Réglages").to_string(),
                SymbolIcon::new().symbol(Symbol::Setting).into(),
                self.selected == SETTINGS_TAG,
            ),
        ));

        let page = if self.selected == SETTINGS_TAG {
            self.settings_page(context)
        } else if self.selected == ADD_TAG {
            self.add_page(context)
        } else {
            self.sim_page(&self.selected.clone(), context)
        };

        let navigation = NavigationView::new()
            .pane_display_mode(NavigationViewPaneDisplayMode::Left)
            .is_pane_toggle_button_visible(false)
            .is_back_button_visible(NavigationViewBackButtonVisible::Collapsed)
            .is_settings_visible(false)
            .always_show_header(false)
            .open_pane_length(280.0)
            .is_pane_open(self.pane_open)
            .on_is_pane_open_changed(context.callback(Message::PaneOpenChanged))
            .on_selected_tag_changed(context.callback(Message::SelectTag))
            .grid_row(1)
            .slots([
                SlotView::collection(NavigationViewSlot::MenuItems, menu),
                SlotView::new(
                    NavigationViewSlot::Content,
                    Border::new()
                        .padding(Thickness::new(36.0, 12.0, 36.0, 24.0))
                        .content(page),
                ),
            ]);

        let count = self.installed_sims.len();
        let title_bar = TitleBar::new()
            .preferred_height(WindowTitleBarHeight::Tall)
            .title("SimAutoStart")
            .subtitle(if is_fr() {
                format!("{count} simulateur{} détecté{}", plural(count), plural(count))
            } else {
                format!("{count} simulator{} detected", plural(count))
            })
            .is_pane_toggle_button_visible(true)
            .on_pane_toggle_requested(context.message(Message::TogglePane))
            .grid_row(0);

        Grid::new()
            .rows([GridLength::Auto, GridLength::STAR, GridLength::Auto])
            .children((title_bar, navigation, self.status_bar()))
            .into()
    }
}

fn main() {
    // Une seule instance : si l'appli tourne déjà (cachée dans la zone de notification),
    // on la réaffiche et on quitte.
    if !tray::ensure_single_instance() {
        return;
    }
    // Lancée par Windows (`--startup`) : on démarre directement dans la zone de notification.
    let start_hidden = std::env::args().any(|arg| arg == "--startup");
    tray::start(start_hidden);
    App::run_component::<SimAutoStart>(()).unwrap();
}
