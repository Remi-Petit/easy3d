//! Hachage des mots de passe — **Argon2id** (RustCrypto), et rien d'autre.
//!
//! L'empreinte enregistrée est une chaîne PHC complète
//! (`$argon2id$v=19$m=19456,t=2,p=1$<sel>$<empreinte>`) : le sel **et** les
//! paramètres voyagent avec le hachage. Conséquence utile : durcir les
//! paramètres plus tard n'invalide pas les mots de passe déjà enregistrés (on
//! peut re-hacher à la volée à la prochaine connexion réussie, puisque
//! [`hash`] produit toujours une empreinte au format courant).
//!
//! ⚠️ **Ce coût est volontaire** : ~19 Mio de mémoire et plusieurs dizaines de
//! millisecondes par vérification, c'est exactement ce qui rend une attaque par
//! dictionnaire hors ligne chère. Ces fonctions sont donc **bloquantes** : les
//! appeler depuis un handler async passe par `tokio::task::spawn_blocking`
//! (voir `auth::login`), jamais directement — sinon le runtime Tokio entier
//! attend, temps réel compris.

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};

/// Longueur minimale d'un mot de passe accepté (à la création ou au changement).
pub const MIN_LEN: usize = 8;

/// Longueur maximale d'un mot de passe accepté.
///
/// Argon2 n'a pas de plafond utile, mais on ne veut pas hacher 10 Mio de
/// données parce qu'un formulaire les a envoyées. 256 caractères sont au-delà de
/// toute phrase de passe raisonnable.
pub const MAX_LEN: usize = 256;

/// Vérifie la longueur d'un mot de passe.
///
/// Renvoie un **code** (`password_too_short` / `password_too_long`) plutôt
/// qu'une phrase : les messages sont traduits côté interface (i18n ×4), comme
/// les autres codes d'erreur de l'API.
pub fn check_length(password: &str) -> Result<(), &'static str> {
    let len = password.chars().count();
    if len < MIN_LEN {
        Err("password_too_short")
    } else if len > MAX_LEN {
        Err("password_too_long")
    } else {
        Ok(())
    }
}

/// Mots de passe refusés en toutes lettres.
///
/// Volontairement **court** : c'est un filet pour les évidences qu'une longueur
/// minimale de huit caractères laisse passer (`12345678`), pas une liste de
/// fuites. La comparer, c'est déjà retirer les quelques chaînes qu'un script
/// essaie en premier — et le contrôle de fuite complet demanderait un appel
/// réseau, donc une dépendance externe au bon fonctionnement du serveur.
const COMMON: &[&str] = &[
    "12345678",
    "123456789",
    "1234567890",
    "11111111",
    "00000000",
    "12341234",
    "87654321",
    "password",
    "password1",
    "password123",
    "passw0rd",
    "p@ssword",
    "qwertyui",
    "qwerty123",
    "azertyui",
    "azerty123",
    "1q2w3e4r",
    "iloveyou",
    "welcome1",
    "sunshine",
    "letmein1",
    "trustno1",
    "baseball",
    "football",
    "superman",
    "motdepasse1",
    "admin1234",
    "rootroot",
    "changeme",
];

/// Politique de mot de passe d'un **compte** : longueur, évidences, identité.
///
/// Trois choses, dans cet ordre — la longueur d'abord, parce que c'est le refus
/// le plus facile à comprendre et à corriger :
///
/// 1. entre [`MIN_LEN`] et [`MAX_LEN`] caractères (comptés en caractères, donc
///    une phrase de passe accentuée passe) ;
/// 2. pas une des chaînes de [`COMMON`], pas une répétition d'un seul caractère
///    (`aaaaaaaa`) ;
/// 3. pas l'identifiant, l'adresse, ni la partie locale de l'adresse :
///    `remi` / `remi@exemple.fr` comme mot de passe, c'est le premier essai de
///    n'importe qui.
///
/// Ce qui n'est **pas** fait : contrôle de fuite en ligne, expiration, longueur
/// imposée, interdiction des caractères. Ces réglages se discutent ; en attendant
/// la discussion, poser un mot de passe dans easy3d demande déjà huit caractères
/// et ne peut plus être `12345678`.
pub fn policy(password: &str, username: &str, email: &str) -> Result<(), &'static str> {
    check_length(password)?;

    let bas = password.to_lowercase();
    if COMMON.contains(&bas.as_str()) {
        return Err("password_too_common");
    }
    // Un seul caractère répété : la liste ci-dessus ne peut pas tout contenir,
    // et `ssssssss` n'a pas plus de valeur que `12345678`.
    let mut caracteres = bas.chars();
    if let Some(premier) = caracteres.next()
        && caracteres.all(|c| c == premier)
    {
        return Err("password_too_common");
    }

    let identifiant = username.trim().to_lowercase();
    let adresse = email.trim().to_lowercase();
    let local = adresse.split('@').next().unwrap_or_default().to_string();
    let evidents = [identifiant, adresse, local];
    if evidents
        .iter()
        .any(|evident| !evident.is_empty() && *evident == bas)
    {
        return Err("password_matches_identity");
    }

    Ok(())
}

/// Empreinte PHC d'un mot de passe, **sel aléatoire** compris.
///
/// Deux appels avec le même mot de passe donnent deux empreintes différentes :
/// c'est ce qui empêche de reconnaître, dans la base, deux comptes qui
/// partagent le même mot de passe.
pub fn hash(password: &str) -> Result<String, String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|e| format!("hachage impossible : {e}"))
}

/// Vérifie un mot de passe contre une empreinte PHC.
///
/// Renvoie `false` — et **pas** une erreur — quand l'empreinte est illisible :
/// une ligne abîmée en base ne doit jamais valider une connexion, et l'appelant
/// rend de toute façon le même message qu'un mauvais mot de passe.
pub fn verify(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l_empreinte_est_de_l_argon2id_et_valide_le_bon_mot_de_passe() {
        let hash = hash("correct horse battery staple").unwrap();

        // Le préfixe est le contrat : c'est lui qui dit l'algorithme et la
        // version à la relecture.
        assert!(hash.starts_with("$argon2id$v=19$"), "empreinte : {hash}");
        assert!(verify("correct horse battery staple", &hash));
        assert!(!verify("correct horse battery stapl", &hash));
        assert!(!verify("", &hash));
    }

    #[test]
    fn deux_hachages_du_meme_mot_de_passe_donnent_des_empreintes_differentes() {
        let a = hash("même mot de passe").unwrap();
        let b = hash("même mot de passe").unwrap();
        assert_ne!(a, b, "le sel doit être aléatoire");
        assert!(verify("même mot de passe", &a) && verify("même mot de passe", &b));
    }

    #[test]
    fn une_empreinte_corrompue_ne_valide_personne() {
        for casse in [
            "",
            "pas une empreinte",
            "$argon2id$v=19$m=19456,t=2,p=1$sel$",
            "$argon2id$v=19$m=19456,t=2,p=1$$empreinte",
            "$argon2i$v=19$m=19456,t=2,p=1$c2VsY2VsbGU$YWJjZGVm",
        ] {
            assert!(!verify("n'importe quoi", casse), "cas : {casse:?}");
        }
    }

    #[test]
    fn la_longueur_des_mots_de_passe_est_bornee() {
        assert_eq!(check_length("court"), Err("password_too_short"));
        assert_eq!(check_length(&"a".repeat(MIN_LEN)), Ok(()));
        assert_eq!(check_length(&"a".repeat(MAX_LEN)), Ok(()));
        assert_eq!(
            check_length(&"a".repeat(MAX_LEN + 1)),
            Err("password_too_long")
        );

        // Bornes comptées en **caractères**, pas en octets : une phrase de passe
        // accentuée ne doit pas être refusée parce qu'elle pèse plus lourd.
        assert_eq!(check_length(&"é".repeat(MIN_LEN)), Ok(()));
    }
}
