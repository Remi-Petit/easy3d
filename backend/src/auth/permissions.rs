//! Catalogue des droits.
//!
//! **Source unique** : ce fichier. Le frontend n'a aucune liste de droits en dur —
//! il interroge `GET /permissions` et construit ses cases à cocher (même principe
//! que les formats de fichiers et les fournisseurs d'IA). Ajouter un droit se
//! limite donc à une ligne dans [`ALL`], plus les gardes qui l'exigent côté
//! serveur.
//!
//! Un droit s'écrit `domaine.action`, en minuscules : c'est un **identifiant
//! stable**, qui finit dans la base (rôles, droits directs) et dans la
//! configuration des agents. Il ne se traduit jamais ; seul son libellé l'est
//! (clés `perm.*`, i18n ×4).
//!
//! Il n'existe **aucun droit de refus** : les droits s'additionnent (rôles ∪
//! droits posés sur le compte). Le rôle `admin` est un superutilisateur **codé**
//! (voir `rbac::is_admin`) : il n'a pas à lister ce fichier pour tout pouvoir, et
//! il ne peut donc pas se verrouiller dehors par erreur.

use serde::Serialize;

/// Consulter le catalogue (et télécharger les modèles).
pub const CATALOG_READ: &str = "catalog.read";
/// Ajouter des fichiers au catalogue.
pub const MODEL_UPLOAD: &str = "model.upload";
/// Renommer un fichier ou un dossier.
pub const MODEL_RENAME: &str = "model.rename";
/// Supprimer un fichier ou un dossier.
pub const MODEL_DELETE: &str = "model.delete";
/// Écrire les notes (l'éditeur temps réel, qui passe par `/collab`).
pub const NOTE_WRITE: &str = "note.write";
/// Interroger la recherche assistée.
pub const AI_USE: &str = "ai.use";
/// Choisir le fournisseur d'IA et sa clé.
pub const AI_CONFIG: &str = "ai.config";
/// Voir les réglages.
pub const CONFIG_READ: &str = "config.read";
/// Modifier les réglages (affichage, dossier des modèles, re-scan).
pub const CONFIG_WRITE: &str = "config.write";
/// Voir les comptes.
pub const USERS_READ: &str = "users.read";
/// Créer et modifier des comptes, et leurs droits.
pub const USERS_WRITE: &str = "users.write";
/// Voir les rôles.
pub const ROLES_READ: &str = "roles.read";
/// Créer et modifier des rôles.
pub const ROLES_WRITE: &str = "roles.write";

/// Un droit reconnu par le serveur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permission {
    /// Identifiant `domaine.action`, tel qu'il est stocké en base.
    pub id: &'static str,
    /// Domaine de regroupement, pour l'affichage (`catalog`, `ai`…).
    pub group: &'static str,
}

/// Tous les droits connus, dans l'ordre d'affichage.
///
/// L'ordre est celui de l'interface (les groupes se suivent) : le catalogue, ce
/// qu'on y fait, l'IA, les réglages, puis les comptes.
pub const ALL: &[Permission] = &[
    Permission { id: CATALOG_READ, group: "catalog" },
    Permission { id: MODEL_UPLOAD, group: "catalog" },
    Permission { id: MODEL_RENAME, group: "catalog" },
    Permission { id: MODEL_DELETE, group: "catalog" },
    Permission { id: NOTE_WRITE, group: "catalog" },
    Permission { id: AI_USE, group: "ai" },
    Permission { id: AI_CONFIG, group: "ai" },
    Permission { id: CONFIG_READ, group: "settings" },
    Permission { id: CONFIG_WRITE, group: "settings" },
    Permission { id: USERS_READ, group: "accounts" },
    Permission { id: USERS_WRITE, group: "accounts" },
    Permission { id: ROLES_READ, group: "accounts" },
    Permission { id: ROLES_WRITE, group: "accounts" },
];

/// Tous les identifiants connus, dans l'ordre d'affichage.
pub fn all_ids() -> Vec<&'static str> {
    ALL.iter().map(|permission| permission.id).collect()
}

/// `true` si l'identifiant est connu du serveur.
///
/// Sert de filtre à l'écriture : on refuse d'enregistrer un droit que le
/// serveur n'appliquera jamais (faute de frappe, ou droit d'une version plus
/// récente que le binaire).
pub fn is_known(id: &str) -> bool {
    ALL.iter().any(|permission| permission.id == id)
}

/// Droit exigé par un **outil MCP**, s'il est connu.
///
/// Les outils n'ont pas de route à eux : c'est ici que se dit ce qu'il faut pour
/// les appeler — le pendant, pour MCP, de la table des routes HTTP. Un outil
/// inconnu renvoie `None`, et c'est le routeur MCP qui répondra « outil
/// inconnu » : la liste des outils et celle des droits restent ainsi au même
/// endroit que le reste.
pub fn for_tool(tool: &str) -> Option<&'static str> {
    match tool {
        // Lire le catalogue, ses notes et les formats reconnus.
        "list_formats" | "list_models" | "find_models" | "get_model" | "list_notes"
        | "read_note" => Some(CATALOG_READ),
        // Écrire une note (le canal CRDT ne distingue pas lecture et écriture).
        "create_note" | "update_note" | "append_note" | "delete_note" => Some(NOTE_WRITE),
        "get_config" => Some(CONFIG_READ),
        "set_display_mode" | "set_models_root" => Some(CONFIG_WRITE),
        _ => None,
    }
}

/// Un droit, tel que `GET /permissions` le publie.
#[derive(Debug, Clone, Serialize)]
pub struct PermissionInfo {
    /// Identifiant stable, à recopier tel quel (`model.delete`).
    pub id: &'static str,
    /// Domaine de regroupement des cases à cocher (`catalog`, `accounts`…).
    pub group: &'static str,
}

/// Droits publiés pour l'interface d'administration.
pub fn describe() -> Vec<PermissionInfo> {
    ALL.iter()
        .map(|permission| PermissionInfo {
            id: permission.id,
            group: permission.group,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_identifiants_sont_uniques_et_bien_formes() {
        let ids = all_ids();
        let mut uniques = ids.clone();
        uniques.sort_unstable();
        uniques.dedup();
        assert_eq!(uniques.len(), ids.len(), "identifiant en double : {ids:?}");

        for id in ids {
            assert_eq!(id, id.trim());
            assert!(
                id.split_once('.').is_some_and(|(domaine, action)| !domaine.is_empty() && !action.is_empty()),
                "identifiant mal formé : {id}"
            );
            assert!(
                id.chars().all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'),
                "identifiant à minuscules attendu : {id}"
            );
        }
    }

    #[test]
    fn chaque_identifiant_est_reconnu_et_publie() {
        for id in all_ids() {
            assert!(is_known(id), "connu : {id}");
        }
        assert!(!is_known("model.purge"));
        assert!(!is_known(""));
        assert_eq!(describe().len(), ALL.len());
    }

    #[test]
    fn chaque_outil_mcp_exige_un_droit_connu() {
        // Liste recopiée de `mcp.rs` : le test échoue si un outil est ajouté là
        // sans être classé ici (il s'ouvrirait à tous les comptes connectés).
        for outil in [
            "list_formats",
            "list_models",
            "find_models",
            "get_model",
            "list_notes",
            "read_note",
            "create_note",
            "update_note",
            "append_note",
            "delete_note",
            "get_config",
            "set_display_mode",
            "set_models_root",
        ] {
            let droit = for_tool(outil).unwrap_or_else(|| panic!("outil non classé : {outil}"));
            assert!(is_known(droit), "droit inconnu pour {outil} : {droit}");
        }

        assert_eq!(for_tool("outil_invente"), None);
    }

    #[test]
    fn les_groupes_servent_a_regrouper_l_affichage() {
        let groupes: Vec<&str> = describe().iter().map(|p| p.group).collect();
        for groupe in &groupes {
            assert!(!groupe.is_empty() && groupe.chars().all(|c| c.is_ascii_lowercase()));
        }
        assert!(groupes.contains(&"catalog"));
        assert!(groupes.contains(&"accounts"));
    }
}
