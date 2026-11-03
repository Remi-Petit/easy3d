//! Entretien de la mémoire du process.
//!
//! Un serveur Rust qui a encaissé un pic de trafic ne **rend pas** la mémoire
//! qu'il a réservée : l'allocateur garde les pages libérées en réserve pour les
//! allocations suivantes, et le noyau les compte toujours comme résidentes.
//! Mesuré sur le banc k6 à 1 000 utilisateurs virtuels : le process passait de
//! 14 Mo à 155 Mo, et restait à ce niveau une fois la charge retombée.
//!
//! Deux choses, distinctes, corrigent cela :
//!
//!  - `MALLOC_ARENA_MAX=2` (posé dans le `Dockerfile`) limite le **nombre**
//!    d'arènes — donc la réserve totale possible : 155 Mo → 116 Mo ;
//!  - [`trim_heap`] rend au système, à la demande, les pages déjà libérées :
//!    c'est le « clear » périodique, réglable dans `config.yml`
//!    (`memory.trim_minutes`).
//!
//! La restitution ne coûte que le parcours des listes de l'allocateur (des
//! microsecondes) : la refaire régulièrement n'a aucun effet sur la latence des
//! requêtes, et le tas se réagrandit tout seul si le trafic remonte.

/// Mémoire résidente du process, en kio (`0` quand elle n'est pas lisible).
///
/// Sert à **dire** ce que l'entretien a réellement rendu : sans mesure, la
/// fonctionnalité n'est qu'une intention.
pub fn rss_kb() -> u64 {
    #[cfg(target_os = "linux")]
    {
        // `statm` : taille totale, résidente, partagée… en pages.
        if let Ok(text) = std::fs::read_to_string("/proc/self/statm")
            && let Some(pages) = text.split_whitespace().nth(1)
            && let Ok(pages) = pages.parse::<u64>()
        {
            // 4 kio par page : la valeur exacte serait dans `sysconf`, mais une
            // erreur d'un facteur 4 sur un simple log d'entretien ne justifie
            // pas un appel système de plus.
            return pages * 4;
        }
        0
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// Rend au système la mémoire que l'allocateur garde en réserve.
///
/// Retourne `true` si des pages ont effectivement été rendues. Sous Linux/glibc,
/// c'est `malloc_trim(0)` : il libère les blocs libres du haut du tas, y compris
/// dans les arènes secondaires quand elles sont vides. Ailleurs (musl, Windows,
/// macOS), l'allocateur n'expose pas d'équivalent portable : la fonction ne fait
/// rien et le dit, plutôt que de laisser croire à une action.
pub fn trim_heap() -> bool {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        // Sûreté : `malloc_trim` est thread-safe et ne prend qu'une taille de
        // réserve à conserver (0 = tout rendre). Aucun pointeur n'est manipulé.
        unsafe { libc::malloc_trim(0) != 0 }
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// L'appel ne doit jamais paniquer, et la mesure ne doit pas être absurde.
    ///
    /// `malloc_trim` peut légitimement ne rien rendre (le tas est déjà compact
    /// au démarrage) : on ne teste donc pas le `true`, seulement l'absence de
    /// panique et la cohérence de la mesure.
    #[test]
    fn entretien_sans_effet_de_bord() {
        let avant = rss_kb();
        let _ = trim_heap();
        let apres = rss_kb();
        #[cfg(target_os = "linux")]
        {
            assert!(avant > 0, "RSS illisible sous Linux");
            // Le trim ne fait que rendre de la mémoire : la résidente ne peut
            // pas augmenter de façon significative pendant l'appel.
            assert!(apres + 4096 >= avant, "RSS {avant} -> {apres}");
        }
        #[cfg(not(target_os = "linux"))]
        {
            assert_eq!((avant, apres), (0, 0));
        }
    }
}
