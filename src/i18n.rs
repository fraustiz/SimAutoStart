//! Langue de l'interface : anglais par défaut, français si la langue d'affichage de Windows
//! est le français. `SIMAUTOSTART_LANG=en|fr` force la langue (utile pour les tests).

use std::sync::OnceLock;

#[link(name = "kernel32")]
extern "system" {
    /// Identifiant de langue (LANGID) de l'interface de l'utilisateur.
    fn GetUserDefaultUILanguage() -> u16;
}

const PRIMARY_LANGUAGE_MASK: u16 = 0x03FF;
const LANG_FRENCH: u16 = 0x0C;

fn detect() -> bool {
    if let Ok(forced) = std::env::var("SIMAUTOSTART_LANG") {
        return forced.eq_ignore_ascii_case("fr");
    }
    // SAFETY: appel sans argument ni effet de bord, disponible sur toute version de Windows.
    let langid = unsafe { GetUserDefaultUILanguage() };
    langid & PRIMARY_LANGUAGE_MASK == LANG_FRENCH
}

/// Vrai si l'interface doit être en français.
pub fn is_fr() -> bool {
    static FRENCH: OnceLock<bool> = OnceLock::new();
    *FRENCH.get_or_init(detect)
}

/// Choisit la version anglaise ou française d'un texte.
pub fn t(en: &'static str, fr: &'static str) -> &'static str {
    if is_fr() {
        fr
    } else {
        en
    }
}
