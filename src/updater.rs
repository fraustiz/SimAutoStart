//! Mises à jour intégrées, depuis les releases GitHub du dépôt.
//!
//! 1. `check` demande la liste des releases (une seule requête GET, rien d'autre n'est envoyé)
//!    et garde la plus récente qui est plus récente que la version en cours.
//! 2. `install` télécharge le zip, **vérifie son SHA-256** contre le fichier `.sha256` publié avec
//!    lui, le décompresse dans un dossier temporaire. Sans empreinte publiée, rien n'est installé.
//! 3. `apply_and_exit` lance l'exécutable *décompressé* en mode `--apply-update` : une fois cette
//!    appli fermée, il copie ses fichiers par-dessus ceux de l'appli puis la relance. (Un exécutable
//!    en cours d'exécution ne peut pas se remplacer lui-même, d'où ce processus d'aide.)
//!
//! Limite assumée : l'exécutable n'est pas signé, et l'empreinte vient du même endroit que le zip ;
//! elle protège d'un téléchargement corrompu, pas d'un compte GitHub compromis. Le téléchargement
//! est limité aux fichiers du dépôt (`DOWNLOAD_PREFIX`) et passe par HTTPS.
//!
//! `curl.exe` et `tar.exe` sont fournis avec Windows 10 (1803 et plus) et Windows 11.

use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::fmt;
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};

const API: &str = "https://api.github.com/repos/fraustiz/SimAutoStart/releases?per_page=15";
/// Seuls les fichiers publiés par le dépôt peuvent être téléchargés.
const DOWNLOAD_PREFIX: &str = "https://github.com/fraustiz/SimAutoStart/releases/download/";
const ARCHIVE_SUFFIX: &str = "windows-x64.zip";
const APP_EXE: &str = "SimAutoStart.exe";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

// --- Versions (SemVer, avec pré-versions : 0.1.0-beta.2 < 0.1.0-beta.10 < 0.1.0) ------------

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ident {
    // L'ordre des variantes compte : un identifiant numérique précède un identifiant texte.
    Num(u64),
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    core: [u64; 3],
    pre: Vec<Ident>,
}

impl Version {
    /// Accepte `v0.1.0-beta.2`, `0.1.0`, ignore les métadonnées de build (`+...`).
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        let text = text.split('+').next()?;
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };
        let mut parts = core.split('.');
        let core = [
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        ];
        if parts.next().is_some() {
            return None;
        }
        let pre = pre
            .map(|pre| {
                pre.split('.')
                    .map(|id| {
                        id.parse::<u64>()
                            .map(Ident::Num)
                            .unwrap_or_else(|_| Ident::Text(id.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Version { core, pre })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| {
            // Une version finale l'emporte sur ses pré-versions.
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.core[0], self.core[1], self.core[2])?;
        for (i, id) in self.pre.iter().enumerate() {
            f.write_str(if i == 0 { "-" } else { "." })?;
            match id {
                Ident::Num(n) => write!(f, "{n}")?,
                Ident::Text(t) => f.write_str(t)?,
            }
        }
        Ok(())
    }
}

/// Version de l'appli. En build de débogage, `SIMAUTOSTART_FAKE_VERSION` permet de simuler une
/// version plus ancienne pour tester la mise à jour (sans effet dans un build release).
pub fn current_version() -> Version {
    #[cfg(debug_assertions)]
    if let Some(fake) = std::env::var("SIMAUTOSTART_FAKE_VERSION")
        .ok()
        .and_then(|v| Version::parse(&v))
    {
        return fake;
    }
    Version::parse(env!("CARGO_PKG_VERSION")).expect("version de Cargo.toml invalide")
}

// --- Releases GitHub ------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub tag: String,
    pub version: Version,
    pub zip_url: String,
    /// Fichier `.sha256` publié avec le zip ; sans lui, l'installation est refusée.
    pub sha_url: Option<String>,
    pub size: u64,
    pub page_url: String,
}

/// Choisit, parmi les releases, la plus récente qui dépasse `current` et porte une archive du
/// dépôt. Les brouillons, les releases sans archive et les URL hors du dépôt sont ignorés.
pub fn parse_releases(json: &str, current: &Version) -> Result<Option<Update>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("réponse de GitHub illisible ({e})"))?;
    let releases = value
        .as_array()
        .ok_or_else(|| "réponse de GitHub inattendue".to_string())?;

    let mut best: Option<Update> = None;
    for release in releases {
        if release["draft"].as_bool() == Some(true) {
            continue;
        }
        let Some(tag) = release["tag_name"].as_str() else { continue };
        let Some(version) = Version::parse(tag) else { continue };
        if version <= *current || best.as_ref().is_some_and(|b| version <= b.version) {
            continue;
        }
        let assets = release["assets"].as_array().cloned().unwrap_or_default();
        let asset_url = |name: &str| -> Option<String> {
            assets
                .iter()
                .find(|a| a["name"].as_str() == Some(name))
                .and_then(|a| a["browser_download_url"].as_str())
                .filter(|url| url.starts_with(DOWNLOAD_PREFIX))
                .map(str::to_string)
        };
        let Some(zip) = assets.iter().find(|a| {
            a["name"].as_str().is_some_and(|n| n.ends_with(ARCHIVE_SUFFIX))
                && a["browser_download_url"]
                    .as_str()
                    .is_some_and(|u| u.starts_with(DOWNLOAD_PREFIX))
        }) else {
            continue;
        };
        let zip_name = zip["name"].as_str().unwrap_or_default().to_string();
        best = Some(Update {
            tag: tag.to_string(),
            version,
            zip_url: zip["browser_download_url"].as_str().unwrap_or_default().to_string(),
            sha_url: asset_url(&format!("{zip_name}.sha256")),
            size: zip["size"].as_u64().unwrap_or(0),
            page_url: release["html_url"].as_str().unwrap_or_default().to_string(),
        });
    }
    Ok(best)
}

fn curl() -> Command {
    let mut command = Command::new("curl.exe");
    command
        .args(["-sS", "-L", "--fail", "--connect-timeout", "15"])
        .args(["-A", &format!("SimAutoStart/{}", current_version())])
        .creation_flags(CREATE_NO_WINDOW);
    command
}

fn curl_text(url: &str) -> Result<String, String> {
    let output = curl()
        .args(["--max-time", "30", "-H", "Accept: application/vnd.github+json", url])
        .output()
        .map_err(|e| format!("curl.exe introuvable ({e})"))?;
    if !output.status.success() {
        return Err(format!(
            "échec de la requête : {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Interroge GitHub : `Ok(Some(..))` si une version plus récente existe.
pub fn check(current: &Version) -> Result<Option<Update>, String> {
    parse_releases(&curl_text(API)?, current)
}

// --- État partagé avec l'interface ----------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Available(Update),
    Downloading { done: u64, total: u64 },
    /// Archive vérifiée et décompressée, prête à être appliquée.
    Ready { dir: PathBuf, tag: String },
    Error(String),
}

pub type Shared = Arc<Mutex<Status>>;

pub fn new_status() -> Shared {
    Arc::new(Mutex::new(Status::Idle))
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Un exécutable lancé depuis `target\debug` ou `target\release` est un build de développement :
/// l'installer par-dessus écraserait le travail en cours, donc on ne le fait pas.
pub fn is_dev_build() -> bool {
    #[cfg(debug_assertions)]
    if std::env::var_os("SIMAUTOSTART_ALLOW_DEV_UPDATE").is_some() {
        return false;
    }
    std::env::current_exe().is_ok_and(|exe| {
        let path = exe.to_string_lossy().to_lowercase();
        path.contains("\\target\\debug\\") || path.contains("\\target\\release\\")
    })
}

/// Vérifie en arrière-plan ; enregistre l'heure de la vérification dans la config.
pub fn spawn_check(status: Shared, engine: crate::engine::State) {
    {
        let mut current = status.lock().unwrap();
        // Une mise à jour en cours de téléchargement ou déjà prête ne doit pas être écrasée.
        if matches!(*current, Status::Checking | Status::Downloading { .. } | Status::Ready { .. }) {
            return;
        }
        *current = Status::Checking;
    }
    std::thread::spawn(move || {
        let result = check(&current_version());
        {
            let mut shared = engine.lock().unwrap();
            shared.config.last_update_check = now_secs();
            shared.config.save();
        }
        *status.lock().unwrap() = match result {
            Ok(Some(update)) => Status::Available(update),
            Ok(None) => Status::UpToDate,
            Err(error) => Status::Error(error),
        };
    });
}

/// Télécharge, vérifie et décompresse en arrière-plan.
pub fn spawn_install(status: Shared, update: Update) {
    *status.lock().unwrap() = Status::Downloading { done: 0, total: update.size };
    std::thread::spawn(move || {
        let progress = status.clone();
        let result = install(&update, &move |done| {
            *progress.lock().unwrap() = Status::Downloading { done, total: update.size };
        });
        *status.lock().unwrap() = match result {
            Ok(dir) => Status::Ready { dir, tag: update.tag },
            Err(error) => Status::Error(error),
        };
    });
}

// --- Téléchargement et vérification ---------------------------------------------------------

fn staging_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("SimAutoStart")
        .join("update")
}

/// Supprime les dossiers de mises à jour précédentes (après une installation réussie).
pub fn cleanup_staging() {
    let _ = std::fs::remove_dir_all(staging_root());
}

/// SHA-256 d'un fichier, en hexadécimal minuscule.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Premier mot d'un fichier `.sha256` (`<hash>  <nom>`), s'il ressemble à un SHA-256.
fn parse_sha256(text: &str) -> Option<String> {
    let word = text.split_whitespace().next()?.to_lowercase();
    (word.len() == 64 && word.chars().all(|c| c.is_ascii_hexdigit())).then_some(word)
}

fn download(url: &str, destination: &Path, on_progress: &dyn Fn(u64)) -> Result<(), String> {
    let mut child = curl()
        .args(["--max-time", "900", "-o"])
        .arg(destination)
        .arg(url)
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl.exe introuvable ({e})"))?;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                on_progress(std::fs::metadata(destination).map_or(0, |m| m.len()));
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(format!("téléchargement interrompu ({e})")),
        }
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "échec du téléchargement : {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// Télécharge, vérifie l'empreinte, décompresse. Renvoie le dossier des fichiers prêts.
fn install(update: &Update, on_progress: &dyn Fn(u64)) -> Result<PathBuf, String> {
    let sha_url = update
        .sha_url
        .as_deref()
        .ok_or("cette version ne publie pas d'empreinte SHA-256 : installation refusée")?;
    let tag: String = update
        .tag
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect();
    let dir = staging_root().join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("dossier temporaire impossible ({e})"))?;

    let zip = dir.join("package.zip");
    download(&update.zip_url, &zip, on_progress)?;

    let expected = parse_sha256(&curl_text(sha_url)?)
        .ok_or("fichier d'empreinte SHA-256 illisible : installation refusée")?;
    let actual = sha256_file(&zip).map_err(|e| format!("lecture de l'archive impossible ({e})"))?;
    if expected != actual {
        let _ = std::fs::remove_file(&zip);
        return Err(format!(
            "empreinte SHA-256 incorrecte (attendue {}…, obtenue {}…) : installation refusée",
            &expected[..12],
            &actual[..12]
        ));
    }

    let files = dir.join("files");
    std::fs::create_dir_all(&files).map_err(|e| e.to_string())?;
    let status = Command::new("tar.exe")
        .arg("-xf")
        .arg(&zip)
        .arg("-C")
        .arg(&files)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("tar.exe introuvable ({e})"))?;
    if !status.success() {
        return Err("décompression de l'archive impossible".to_string());
    }
    if !files.join(APP_EXE).is_file() {
        return Err(format!("{APP_EXE} absent de l'archive : installation refusée"));
    }
    let _ = std::fs::remove_file(&zip);
    Ok(files)
}

// --- Application de la mise à jour ----------------------------------------------------------

/// Lance l'exécutable décompressé en mode `--apply-update`. L'appelant doit ensuite quitter
/// l'appli (sans quoi le processus d'aide attendrait 30 s avant de continuer).
pub fn start_apply(files_dir: &Path) -> Result<(), String> {
    let staged = files_dir.join(APP_EXE);
    let target = std::env::current_exe().map_err(|e| e.to_string())?;
    Command::new(&staged)
        .arg("--apply-update")
        .arg(&target)
        .arg(std::process::id().to_string())
        .current_dir(files_dir)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("lancement de la mise à jour impossible ({e})"))
}

fn log(message: &str) {
    use std::io::Write;
    let path = staging_root().parent().map(|p| p.join("update.log"));
    if let Some(path) = path {
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "[{}] {message}", now_secs());
        }
    }
}

fn wait_for_exit(pid: u32, timeout_ms: u32) {
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if !handle.is_null() {
            WaitForSingleObject(handle, timeout_ms);
            CloseHandle(handle);
        }
    }
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_files(&path, out);
            } else {
                out.push(path);
            }
        }
    }
}

/// Copie `source` dans `target_dir`. L'exécutable de `source` va dans `target_exe` (même si
/// l'appli installée a été renommée). Chaque fichier est écrit sous un nom temporaire puis
/// renommé par-dessus l'ancien, avec quelques essais (un antivirus peut verrouiller un instant).
pub fn apply_files(source: &Path, target_dir: &Path, target_exe: &Path) -> Result<usize, String> {
    let mut files = Vec::new();
    collect_files(source, &mut files);
    let mut copied = 0;
    for file in files {
        let relative = file.strip_prefix(source).map_err(|e| e.to_string())?;
        let is_app_exe = relative.components().count() == 1
            && file.file_name().is_some_and(|n| n.eq_ignore_ascii_case(APP_EXE));
        let destination = if is_app_exe { target_exe.to_path_buf() } else { target_dir.join(relative) };
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let temporary = PathBuf::from(format!("{}.update-new", destination.display()));
        let mut last_error = String::new();
        let mut done = false;
        for _ in 0..20 {
            match std::fs::copy(&file, &temporary).and_then(|_| std::fs::rename(&temporary, &destination)) {
                Ok(()) => {
                    done = true;
                    break;
                }
                Err(e) => {
                    last_error = e.to_string();
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
        }
        if !done {
            let _ = std::fs::remove_file(&temporary);
            return Err(format!("{}: {last_error}", destination.display()));
        }
        copied += 1;
    }
    Ok(copied)
}

/// Mode « aide » : `SimAutoStart.exe --apply-update <exe installé> <pid de l'appli>`.
/// Renvoie `true` si ce mode était demandé (l'appelant doit alors quitter sans ouvrir de fenêtre).
pub fn handle_apply_mode() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let Some(index) = args.iter().position(|a| a == "--apply-update") else {
        return false;
    };
    let (Some(target), Some(pid)) = (args.get(index + 1), args.get(index + 2)) else {
        return true;
    };
    let target_exe = PathBuf::from(target);
    let Some(target_dir) = target_exe.parent().map(Path::to_path_buf) else {
        return true;
    };
    log(&format!("mise à jour demandée pour {}", target_exe.display()));
    wait_for_exit(pid.parse().unwrap_or(0), 30_000);

    let source = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
    match source.map(|source| apply_files(&source, &target_dir, &target_exe)) {
        Some(Ok(count)) => log(&format!("{count} fichiers copiés")),
        Some(Err(error)) => log(&format!("ERREUR pendant la copie : {error}")),
        None => log("ERREUR : dossier source introuvable"),
    }
    // On relance l'appli dans tous les cas : la nouvelle version, ou l'ancienne si la copie a échoué.
    if let Err(error) = Command::new(&target_exe).current_dir(&target_dir).spawn() {
        log(&format!("ERREUR au relancement : {error}"));
    }
    true
}

// --- Tests ----------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn versions_are_ordered_like_semver() {
        assert!(v("0.1.0-beta.2") < v("0.1.0-beta.10"), "comparaison numérique, pas textuelle");
        assert!(v("0.1.0-beta.3") < v("0.1.0"), "une version finale l'emporte sur sa pré-version");
        assert!(v("0.1.0") < v("0.1.1-alpha"));
        assert!(v("0.1.0-alpha") < v("0.1.0-beta"));
        assert!(v("0.1.0-beta") < v("0.1.0-beta.1"), "plus d'identifiants = plus récent");
        assert!(v("0.9.9") < v("0.10.0"));
        assert_eq!(v("v0.1.0-beta.2+build5"), v("0.1.0-beta.2"));
    }

    #[test]
    fn invalid_versions_are_rejected() {
        for bad in ["", "1.2", "1.2.3.4", "a.b.c", "latest", "v1.x.0"] {
            assert!(Version::parse(bad).is_none(), "{bad:?} ne doit pas être accepté");
        }
        assert_eq!(v("v0.1.0-beta.2").to_string(), "0.1.0-beta.2");
    }

    const BASE: &str = "https://github.com/fraustiz/SimAutoStart/releases/download";

    fn release(tag: &str, draft: bool, assets: &[(&str, &str)]) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag, "draft": draft, "html_url": format!("https://github.com/fraustiz/SimAutoStart/releases/tag/{tag}"),
            "assets": assets.iter().map(|(name, url)| serde_json::json!({"name": name, "browser_download_url": url, "size": 1234})).collect::<Vec<_>>()
        })
    }

    #[test]
    fn newest_release_with_an_archive_is_selected() {
        let zip3 = "SimAutoStart-0.1.0-beta.3-windows-x64.zip";
        let json = serde_json::json!([
            release("v0.1.0-beta.4", true, &[("SimAutoStart-0.1.0-beta.4-windows-x64.zip", &format!("{BASE}/v0.1.0-beta.4/x.zip"))]),
            release("v0.1.0-beta.3", false, &[(zip3, &format!("{BASE}/v0.1.0-beta.3/{zip3}")), (&format!("{zip3}.sha256"), &format!("{BASE}/v0.1.0-beta.3/{zip3}.sha256"))]),
            release("v0.1.0-beta.2", false, &[("SimAutoStart-0.1.0-beta.2-windows-x64.zip", &format!("{BASE}/v0.1.0-beta.2/b2.zip"))]),
        ])
        .to_string();

        let update = parse_releases(&json, &v("0.1.0-beta.2")).unwrap().expect("beta.3 est plus récente");
        assert_eq!(update.tag, "v0.1.0-beta.3", "le brouillon beta.4 doit être ignoré");
        assert!(update.zip_url.ends_with(zip3));
        assert!(update.sha_url.as_deref().is_some_and(|u| u.ends_with(".sha256")));
        assert_eq!(update.size, 1234);

        assert_eq!(parse_releases(&json, &v("0.1.0-beta.3")).unwrap(), None, "déjà à jour");
        assert_eq!(parse_releases(&json, &v("0.2.0")).unwrap(), None, "version locale plus récente");
    }

    #[test]
    fn unsafe_or_incomplete_releases_are_ignored() {
        let name = "SimAutoStart-0.1.0-beta.9-windows-x64.zip";
        // archive hébergée ailleurs que dans le dépôt : refusée
        let foreign = serde_json::json!([release("v0.1.0-beta.9", false, &[(name, "https://evil.example.com/a.zip")])]).to_string();
        assert_eq!(parse_releases(&foreign, &v("0.1.0-beta.1")).unwrap(), None);
        // release sans archive : ignorée
        let none = serde_json::json!([release("v0.1.0-beta.9", false, &[])]).to_string();
        assert_eq!(parse_releases(&none, &v("0.1.0-beta.1")).unwrap(), None);
        // sans fichier d'empreinte : l'archive est trouvée mais sha_url reste vide (install refusée)
        let nosha = serde_json::json!([release("v0.1.0-beta.9", false, &[(name, &format!("{BASE}/v0.1.0-beta.9/{name}"))])]).to_string();
        assert_eq!(parse_releases(&nosha, &v("0.1.0-beta.1")).unwrap().unwrap().sha_url, None);
        // réponses invalides
        assert!(parse_releases("pas du json", &v("0.1.0")).is_err());
        assert!(parse_releases("{\"message\":\"rate limit\"}", &v("0.1.0")).is_err());
    }

    #[test]
    fn sha256_text_and_file_hashing() {
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(parse_sha256(&format!("{hash}  fichier.zip\n")).as_deref(), Some(hash));
        assert_eq!(parse_sha256(&format!("{}  x", hash.to_uppercase())).as_deref(), Some(hash));
        assert_eq!(parse_sha256("zzz  x"), None);
        assert_eq!(parse_sha256(""), None);

        let dir = std::env::temp_dir().join(format!("sas-test-sha-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("abc.txt"), b"abc").unwrap();
        assert_eq!(sha256_file(&dir.join("abc.txt")).unwrap(), hash, "SHA-256 de « abc »");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_files_replaces_in_place_and_keeps_the_installed_exe_name() {
        let root = std::env::temp_dir().join(format!("sas-test-apply-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (source, target) = (root.join("source"), root.join("target"));
        std::fs::create_dir_all(source.join("fr-FR")).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(source.join("SimAutoStart.exe"), b"nouvelle version").unwrap();
        std::fs::write(source.join("lib.dll"), b"lib v2").unwrap();
        std::fs::write(source.join("fr-FR").join("res.pri"), b"res v2").unwrap();
        // l'appli installée a été renommée, et possède un fichier à remplacer
        std::fs::write(target.join("MonAppli.exe"), b"ancienne version").unwrap();
        std::fs::write(target.join("lib.dll"), b"lib v1").unwrap();
        std::fs::write(target.join("perso.txt"), b"a moi").unwrap();

        let copied = apply_files(&source, &target, &target.join("MonAppli.exe")).unwrap();
        assert_eq!(copied, 3);
        assert_eq!(std::fs::read(target.join("MonAppli.exe")).unwrap(), b"nouvelle version");
        assert!(!target.join("SimAutoStart.exe").exists(), "pas de second exécutable");
        assert_eq!(std::fs::read(target.join("lib.dll")).unwrap(), b"lib v2");
        assert_eq!(std::fs::read(target.join("fr-FR").join("res.pri")).unwrap(), b"res v2");
        assert_eq!(std::fs::read(target.join("perso.txt")).unwrap(), b"a moi", "fichiers étrangers intacts");
        let leftovers: Vec<_> = std::fs::read_dir(&target).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".update-new")).collect();
        assert!(leftovers.is_empty(), "aucun fichier temporaire ne doit rester");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Test réseau (GitHub) : `cargo test -- --ignored`. Télécharge réellement la release beta.2,
    /// vérifie son empreinte et la décompresse, dans un dossier temporaire.
    #[test]
    #[ignore = "utilise le réseau et télécharge ≈ 21 Mo"]
    fn real_release_downloads_verifies_and_extracts() {
        let update = check(&v("0.1.0-beta.1")).expect("GitHub répond").expect("une version plus récente existe");
        assert!(update.version >= v("0.1.0-beta.2"));
        assert!(update.sha_url.is_some(), "la release publie son empreinte");

        let temp = std::env::temp_dir().join(format!("sas-test-install-{}", std::process::id()));
        // SAFETY: test ignoré par défaut, lancé seul ; aucun autre thread ne lit l'environnement.
        unsafe { std::env::set_var("LOCALAPPDATA", &temp) };
        let dir = install(&update, &|_| {}).expect("installation (téléchargement + empreinte + décompression)");
        assert!(dir.join("SimAutoStart.exe").is_file());
        assert!(dir.join("Microsoft.UI.Xaml.dll").is_file() || dir.join("microsoft.ui.xaml.dll").is_file());
        let _ = std::fs::remove_dir_all(&temp);
    }
}
