# SimAutoStart

Détecte les jeux de simracing installés sur ton PC et **lance les applications compagnons que tu choisis dès qu'un jeu démarre** (SimHub, CrewChief, Garage 61, Trading Paints, Track Titan…), puis les ferme quand le jeu s'arrête.

Application Windows 11 native écrite en **Rust** avec **WinUI 3** ([`windows-reactor`](https://github.com/microsoft/windows-rs/pull/4479)), fond Mica. **Autonome** : le runtime Windows App SDK est livré à côté de l'exécutable, il n'y a rien à installer.

> 🇬🇧 [English version](README.md)

| Choisir les applis de chaque jeu | Réglages |
| --- | --- |
| ![Page d'un simulateur](docs/screenshots/sim-page.png) | ![Réglages](docs/screenshots/settings-page.png) |

L'interface s'affiche en français quand Windows est en français (en anglais sinon) ; les captures sont en anglais.

## Fonctionnalités

- **Détection automatique des jeux** : jeux Steam (lus dans les manifestes des bibliothèques Steam, donc les dossiers résiduels ne comptent pas), jeux hors Steam via la liste « Applications installées » de Windows (iRacing par exemple), et tes propres ajouts.
- **Une page par jeu** avec un interrupteur par application. Les applis absentes du PC sont regroupées dans une case **Apps connues**, avec un bouton *Localiser…*.
- **Lancement au démarrage du jeu, fermeture à son arrêt** (optionnelle) : demande de fermeture propre d'abord, arrêt forcé après 5 s.
- **Logos des jeux** extraits de leur exécutable.
- **Ajoute tes propres jeux et applis** depuis un `.exe` (page *Ajouts*, ou *Ajouter une application* sur la page d'un jeu).
- **Zone de notification** : fermer la fenêtre la cache (menu Ouvrir / Quitter), et une seule instance tourne à la fois.
- **Démarrage avec Windows** (démarre caché dans la zone de notification).
- **Mises à jour intégrées** : vérifie les releases GitHub (une fois par jour, désactivable), télécharge le nouveau zip, vérifie son SHA-256, remplace les fichiers de l'appli et redémarre.
- **Barre du bas** indiquant le jeu en cours.
- Importe les chemins de tes profils [ToLaunch](https://github.com/tolgayilmaz86/ToLaunch) (`%LOCALAPPDATA%\ToLaunch`).

### Jeux et applications du catalogue

| Jeux | Applications compagnons |
| --- | --- |
| iRacing, Assetto Corsa, Assetto Corsa Competizione, Le Mans Ultimate, rFactor 2, Automobilista 2, RaceRoom, EA SPORTS WRC, DiRT Rally 2.0, F1 25, BeamNG.drive | SimHub, CrewChief, MOZA Pit House, Garage 61 (iRacing seulement), Content Manager (Assetto Corsa seulement), Trading Paints (iRacing seulement), RaceLab, VoiceAttack, Fanatec FanaLab, OBS Studio, Kapps, Track Titan, Coach Dave Delta, Z1 Dashboard, Simucube True Drive, OpenKneeboard, HWiNFO, MSI Afterburner, Logitech G HUB, Sim Racing Studio |

> **À savoir :** les noms d'exécutables de plusieurs applications viennent de documentations publiques ou de mémoire et n'ont pas tous été confirmés sur une machine qui les a installées : Track Titan, Coach Dave Delta, Z1 Dashboard, Simucube True Drive et Sim Racing Studio surtout. Si l'un est faux, utilise **Localiser…** une fois : l'appli retient le vrai chemin et le vrai nom de processus. Les corrections sont les bienvenues (`src/catalog.rs`).

## Fonctionnement

1. Chaque seconde, les processus en cours sont comparés à la liste des jeux. Un jeu est « en cours » dès qu'un de ses processus existe (pour iRacing, cela inclut le lanceur `iRacingUI`, celui qu'on ouvre en premier).
2. Quand un jeu apparaît, les applis activées pour lui sont lancées (si elles ne tournent pas déjà). Quand il disparaît, celles que SimAutoStart a lancées sont fermées (si l'option est active).
3. Les chemins sont **appris** : depuis les processus en cours, depuis tes profils ToLaunch et depuis *Localiser…*. Le catalogue ne contient que des emplacements par défaut.

Les réglages et caches sont dans `%LOCALAPPDATA%\SimAutoStart` (`config.json`, `icons\`, `software.txt`). La seule requête réseau de l'appli est la vérification des mises à jour (un `GET` vers l'API GitHub, sans identifiant ni télémétrie), désactivable dans les réglages.

## Prérequis

- Windows 10 1809 ou plus récent (Windows 11 recommandé : fond Mica)
- Rien d'autre : le zip de la release (environ 21 Mo, 57 Mo décompressé) contient le runtime Windows App SDK. Garde les fichiers ensemble, dans le dossier où tu as décompressé.

## Compiler

Il faut la [chaîne d'outils Rust](https://rustup.rs) (cible MSVC) et les Build Tools de Visual Studio avec le **SDK Windows** (pour `rc.exe`, qui compile la ressource d'icône). Python n'est **pas** nécessaire.

```powershell
git clone https://github.com/fraustiz/SimAutoStart
cd SimAutoStart
cargo run --release
```

Le premier build télécharge le runtime Windows App SDK depuis NuGet (environ 165 Mo, puis gardé dans `%LOCALAPPDATA%\windows-reactor-setup`, environ 800 Mo une fois décompressé) et le copie à côté de l'exécutable : `target\release` devient un dossier autonome. Une connexion internet est nécessaire pour ce premier build. L'exécutable est `target\release\simautostart.exe` ; pour partager l'appli, livre-le avec les fichiers du runtime qui sont à côté (voir le zip de la release). `cargo test` lance le test d'aller-retour du registre pour *Démarrer avec Windows* (il écrit dans `HKCU\...\Run` puis remet l'état précédent).

## Limites connues

- **Les applis qui exigent les droits administrateur** (SimHub en fait partie) ne peuvent pas être fermées par SimAutoStart quand il tourne sans ces droits : Windows interdit à un processus normal de fermer un processus élevé. Elles sont bien lancées, mais peuvent rester ouvertes après l'arrêt du jeu. Ferme-les toi-même, ou lance SimAutoStart en administrateur (non testé).
- **Garage 61** est reconnu par son agent versionné (`garage61-agent-<date>-<hash>.exe`) : `garage61-launcher.exe` ne fait que le démarrer puis se termine.
- **Les mises à jour ne sont pas signées.** L'exécutable n'est pas signé, et le SHA-256 vient de la même release GitHub que le zip : il protège d'un téléchargement corrompu, pas d'un compte GitHub compromis. Les téléchargements sont limités aux fichiers de release de ce dépôt, en HTTPS. La mise à jour s'appuie sur `curl.exe` et `tar.exe`, fournis avec Windows 10 (1803 et plus) et Windows 11.
- L'appli refuse de mettre à jour un build de développement (lancé depuis `target\debug` ou `target\release`).
- `windows-reactor` est une préversion (0.100). L'icône de notification, l'interception de la fermeture et le sélecteur de fichier passent par du Win32 et PowerShell, faute de mieux dans le crate.
- Un jeu doit être détectable : Steam, liste des applications installées, chemin par défaut, ou ajouté à la main.
- Le premier lancement lit la liste des applications installées et extrait les logos (environ une seconde) ; tout est ensuite mis en cache.

## Crédits

- Construit sur [`windows-reactor`](https://github.com/microsoft/windows-rs) de Microsoft.
- Inspiré de [ToLaunch](https://github.com/tolgayilmaz86/ToLaunch) et d'iRacingManager, qui répondaient au même besoin avec des profils manuels.

## Licence

[MIT](LICENSE) © 2026 Fraustiz.
