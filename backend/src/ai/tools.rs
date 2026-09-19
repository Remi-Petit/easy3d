//! Outils que le modèle de langage peut appeler pendant une recherche.
//!
//! Trois lectures ([`catalog_overview`], [`search_models`], [`read_note`]) et une
//! conclusion ([`submit_results`]). Le résultat part en JSON compact : le modèle
//! paye chaque caractère, et un catalogue entier ne tiendrait de toute façon pas
//! dans une conversation.
//!
//! La conclusion est **vérifiée** : les chemins proposés par le modèle sont
//! confrontés au catalogue réel, ce qui rend impossible de proposer un fichier
//! inventé (les modèles en inventent, surtout quand la réponse est vide).

use crate::api::{AppState, tidy_path};
use crate::formats;
use crate::notes;
use crate::scanner;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::ToolCall;

/// Nombre de résultats renvoyés à un appel de recherche.
const HITS_DEFAULT: usize = 20;
const HITS_MAX: usize = 40;

/// Nombre de propositions acceptées dans une conclusion.
const RESULTS_MAX: usize = 12;

/// Longueurs maximales des textes transmis au modèle.
const NOTE_EXCERPT: usize = 200;
const NOTE_FULL: usize = 2000;
const REASON_MAX: usize = 300;

/// Déclaration d'un outil, dans une forme commune à tous les fournisseurs (le
/// schéma est du JSON Schema, que les quatre savent lire).
pub struct Spec {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
}

/// Proposition finale : un élément du catalogue, et pourquoi il correspond.
#[derive(Debug, Serialize)]
pub struct Hit {
    pub rel: String,
    /// `file` ou `folder`.
    pub kind: &'static str,
    pub reason: String,
}

/// Suite d'une conversation, ou fin de la recherche.
pub enum Ran {
    Continue(String),
    Final(Vec<Hit>),
}

/// Les outils annoncés au modèle, dans l'ordre où il doit s'en servir.
pub fn specs() -> Vec<Spec> {
    vec![
        Spec {
            name: "catalog_overview",
            description: "Vue d'ensemble du catalogue : dossiers de premier niveau (nombre de \
                          fichiers, note éventuelle), fichiers restés à la racine, formats \
                          acceptés. À appeler en premier pour savoir où chercher.",
            schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        },
        Spec {
            name: "search_models",
            description: "Cherche des fichiers et dossiers par nom, dossier, note Markdown ou \
                          métadonnées de G-code (matière, hauteur de couche, temps d'impression...). \
                          `query` accepte plusieurs mots : tous doivent correspondre. Sans `query`, \
                          renvoie les éléments les plus récemment modifiés.",
            schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Mots à chercher (nom, note, métadonnée). Vide = les plus récents."
                    },
                    "folder": {
                        "type": "string",
                        "description": "Ne garder que les éléments de ce dossier (chemin relatif, ex. « Maison »)."
                    },
                    "kind": {
                        "type": "string",
                        "enum": ["file", "folder"],
                        "description": "Ne garder que les fichiers ou que les dossiers."
                    },
                    "ext": {
                        "type": "string",
                        "description": "Extension sans le point (stl, 3mf, gcode, obj, png...)."
                    },
                    "has_note": {
                        "type": "boolean",
                        "description": "Ne garder que les éléments qui ont (ou non) une note."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Nombre maximal de résultats (défaut 20, maximum 40)."
                    }
                },
                "additionalProperties": false
            }),
        },
        Spec {
            name: "read_note",
            description: "Lit la note Markdown d'un fichier ou d'un dossier déjà repéré.",
            schema: json!({
                "type": "object",
                "properties": {
                    "rel": {
                        "type": "string",
                        "description": "Chemin relatif exact, tel que renvoyé par les outils."
                    }
                },
                "required": ["rel"],
                "additionalProperties": false
            }),
        },
        Spec {
            name: "submit_results",
            description: "Conclut la recherche. À appeler **obligatoirement** en dernier, avec les \
                          éléments retenus (liste vide si rien ne correspond) et, pour chacun, une \
                          raison courte fondée sur un fait de la fiche.",
            schema: json!({
                "type": "object",
                "properties": {
                    "results": {
                        "type": "array",
                        "description": "Résultats, du plus pertinent au moins pertinent.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "rel": {
                                    "type": "string",
                                    "description": "Chemin relatif exact renvoyé par les outils."
                                },
                                "reason": {
                                    "type": "string",
                                    "description": "Pourquoi cet élément correspond (une phrase)."
                                }
                            },
                            "required": ["rel", "reason"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["results"],
                "additionalProperties": false
            }),
        },
    ]
}

/// Exécute un appel d'outil.
pub fn run(state: &AppState, call: &ToolCall) -> Ran {
    match call.name.as_str() {
        "catalog_overview" => Ran::Continue(overview(state)),
        "search_models" => Ran::Continue(search_models(state, &call.arguments)),
        "read_note" => Ran::Continue(read_note(state, &call.arguments)),
        "submit_results" => Ran::Final(submit_results(state, &call.arguments)),
        other => Ran::Continue(format!(
            "outil inconnu : « {other} ». Outils disponibles : {}.",
            specs()
                .iter()
                .map(|s| s.name)
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Un élément du catalogue, tel que le voient les outils.
struct Entry {
    rel: String,
    kind: &'static str,
    size: u64,
    created: Option<u64>,
    modified: Option<u64>,
    folder: String,
    note: Option<String>,
    /// Métadonnées de découpe (G-codes uniquement).
    meta: BTreeMap<String, String>,
}

impl Entry {
    /// Nom du fichier ou du dossier, dernier segment du chemin.
    fn name(&self) -> &str {
        self.rel.rsplit('/').next().unwrap_or(&self.rel)
    }

    fn ext(&self) -> String {
        formats::ext_of(&self.rel)
    }

    /// Version transmissible de l'élément, avec ce qui l'a fait remonter.
    fn found(&self, hint: Option<String>) -> Found {
        Found {
            rel: self.rel.clone(),
            kind: self.kind,
            ext: self.ext(),
            size: self.size,
            created: self.created,
            modified: self.modified,
            folder: self.folder.clone(),
            note: self.note.clone(),
            meta: self.meta.clone(),
            hint,
        }
    }
}

/// Critères de recherche.
///
/// Partagés par l'outil de recherche de l'assistant ([`search_models`]) et par
/// l'outil MCP `find_models` : une seule implémentation du score, donc un seul
/// comportement à expliquer et à tester.
#[derive(Debug, Clone)]
pub struct Filter {
    /// Mots à chercher dans le nom, le dossier, la note ou les métadonnées.
    pub query: String,
    /// Restreint à un dossier (préfixe du chemin relatif).
    pub folder: Option<String>,
    /// `file` ou `folder`.
    pub kind: Option<String>,
    /// Extension sans le point (`stl`, `gcode`…).
    pub ext: Option<String>,
    /// Ne garder que les éléments documentés (ou non documentés).
    pub has_note: Option<bool>,
    /// Nombre maximal de résultats.
    pub limit: usize,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            query: String::new(),
            folder: None,
            kind: None,
            ext: None,
            has_note: None,
            limit: HITS_DEFAULT,
        }
    }
}

/// Un élément trouvé, et ce qui l'a fait remonter.
#[derive(Debug, Clone)]
pub struct Found {
    pub rel: String,
    /// `file` ou `folder`.
    pub kind: &'static str,
    pub ext: String,
    /// Taille en octets (`0` pour un dossier).
    pub size: u64,
    pub created: Option<u64>,
    pub modified: Option<u64>,
    /// Dossier d'appartenance (chaîne vide à la racine).
    pub folder: String,
    /// Note Markdown brute, si l'élément en a une.
    pub note: Option<String>,
    /// Métadonnées de découpe (G-codes uniquement).
    pub meta: BTreeMap<String, String>,
    /// Ce qui a fait correspondre l'élément (`name:…`, `note:…`, `meta:…`).
    pub hint: Option<String>,
}

impl Found {
    pub fn is_folder(&self) -> bool {
        self.kind == "folder"
    }

    /// Nom du fichier ou du dossier, dernier segment du chemin.
    pub fn name(&self) -> &str {
        self.rel.rsplit('/').next().unwrap_or(&self.rel)
    }

    /// Âge lisible (`3d`), si la date est connue.
    pub fn age(&self) -> Option<String> {
        self.modified.map(age_label)
    }

    /// Fiche compacte, telle qu'elle part au modèle.
    fn to_json(&self) -> Value {
        let mut item = json!({
            "rel": self.rel,
            "kind": self.kind,
            "name": self.name(),
            "folder": self.folder,
        });
        if !self.is_folder() {
            item["ext"] = json!(self.ext);
            item["size"] = json!(size_label(self.size));
        }
        if let Some(created) = self.created {
            item["created"] = json!(created);
        }
        if let Some(modified) = self.modified {
            item["modified"] = json!(modified);
            item["age"] = json!(age_label(modified));
        }
        if let Some(note) = &self.note {
            item["note"] = json!(excerpt(note, NOTE_EXCERPT));
        }
        if !self.meta.is_empty() {
            item["meta"] = json!(self.meta);
        }
        if let Some(hint) = &self.hint {
            item["match"] = json!(hint);
        }
        item
    }
}

/// Résultat d'une recherche : les éléments renvoyés, et combien il y en avait.
pub struct FoundSet {
    /// Résultats, bornés par [`Filter::limit`].
    pub results: Vec<Found>,
    /// Nombre **total** d'éléments correspondants.
    pub total: usize,
}

/// Photographie du catalogue : dossiers, puis fichiers.
///
/// Relu à chaque appel d'outil (le catalogue change pendant la conversation) ;
/// les métadonnées de G-code ne sont lues que sur les fichiers concernés, et
/// seulement leur en-tête.
fn catalog(state: &AppState) -> Vec<Entry> {
    let root = state.root();
    let mut out = Vec::new();

    let scan = scanner::scan_models(&root);
    for (name, folder) in &scan.folders {
        out.push(Entry {
            rel: name.clone(),
            kind: "folder",
            size: 0,
            created: None,
            modified: folder.modified,
            folder: String::new(),
            note: folder.note.clone(),
            meta: BTreeMap::new(),
        });
        for sub in &folder.subfolders {
            out.push(Entry {
                rel: sub.rel.clone(),
                kind: "folder",
                size: 0,
                created: None,
                modified: sub.modified,
                folder: parent_of(&sub.rel),
                note: sub.note.clone(),
                meta: BTreeMap::new(),
            });
        }
    }

    for file in scanner::scan_files(&root) {
        let meta = formats::gcode_metadata(&file.rel, Path::new(&file.path));
        out.push(Entry {
            rel: file.rel.clone(),
            kind: "file",
            size: std::fs::metadata(&file.path).map(|m| m.len()).unwrap_or(0),
            created: file.created,
            modified: file.modified,
            folder: parent_of(&file.rel),
            note: file.note.clone(),
            meta,
        });
    }

    out
}

/// Dossier parent d'un chemin relatif (chaîne vide à la racine).
fn parent_of(rel: &str) -> String {
    rel.rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default()
}

/// Vue d'ensemble : où sont les choses, et dans quels formats.
fn overview(state: &AppState) -> String {
    let root = state.root();
    let catalog = catalog(state);

    let mut folders: Vec<Value> = Vec::new();
    let mut loose: Vec<Value> = Vec::new();
    for entry in &catalog {
        let item = entry.found(None).to_json();
        if entry.kind == "folder" {
            // Les sous-dossiers sont résumés par leur parent : l'énumération
            // complète viendrait à la place des fichiers, qui sont le sujet.
            if entry.folder.is_empty() {
                folders.push(item);
            }
        } else if entry.folder.is_empty() {
            loose.push(item);
        }
    }

    let mut formats: Vec<String> = catalog
        .iter()
        .filter(|e| e.kind == "file")
        .map(|e| formats::ext_of(&e.rel))
        .filter(|e| !e.is_empty())
        .collect();
    formats.sort_unstable();
    formats.dedup();

    let subfolders = catalog
        .iter()
        .filter(|e| e.kind == "folder" && !e.folder.is_empty())
        .count();

    to_json(json!({
        "now": now(),
        "root": tidy_path(&root),
        "counts": {
            "files": catalog.iter().filter(|e| e.kind == "file").count(),
            "top_level_folders": catalog
                .iter()
                .filter(|e| e.kind == "folder" && e.folder.is_empty())
                .count(),
            "subfolders": subfolders,
        },
        "formats": formats,
        "top_level_folders": folders,
        "loose_files": loose,
        "hint": "search_models fouille noms, notes et métadonnées ; read_note lit une note ; \
                 submit_results conclut."
    }))
}

/// Recherche : filtres, score, extrait de fiche pour chaque résultat.
fn search_models(state: &AppState, args: &Value) -> String {
    let filter = Filter {
        query: text(args, "query").unwrap_or_default(),
        folder: text(args, "folder"),
        kind: text(args, "kind"),
        ext: text(args, "ext"),
        has_note: args.get("has_note").and_then(Value::as_bool),
        limit: args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(HITS_DEFAULT)
            .clamp(1, HITS_MAX),
    };

    let found = find(state, &filter);
    let results: Vec<Value> = found.results.iter().map(Found::to_json).collect();

    let mut payload = json!({
        "now": now(),
        "matched": found.total,
        "returned": results.len(),
        "results": results,
    });
    if found.total > results.len() {
        payload["note"] = json!(format!(
            "{} autres éléments correspondent (non affichés) : affine avec folder, ext, has_note \
             ou une requête plus précise.",
            found.total - results.len()
        ));
    }
    to_json(payload)
}

/// Cherche dans le catalogue : filtres d'abord, score ensuite.
pub fn find(state: &AppState, filter: &Filter) -> FoundSet {
    let folder = filter.folder.clone().unwrap_or_default();
    let kind = filter.kind.clone().unwrap_or_default();
    let ext = filter
        .ext
        .clone()
        .unwrap_or_default()
        .trim_start_matches('.')
        .to_ascii_lowercase();

    // Mots-clés : tous doivent correspondre (ET), ce qui laisse l'appelant
    // préciser sa question plutôt que de recevoir un catalogue entier.
    let words: Vec<String> = filter
        .query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() > 1)
        .map(str::to_string)
        .collect();
    let phrase = filter.query.trim().to_lowercase();

    let mut scored: Vec<(usize, Found)> = Vec::new();
    for entry in &catalog(state) {
        if !folder.is_empty() && !entry.rel.starts_with(&folder) {
            continue;
        }
        if !kind.is_empty() && entry.kind != kind {
            continue;
        }
        if !ext.is_empty() && entry.ext() != ext {
            continue;
        }
        if let Some(wanted) = filter.has_note
            && entry.note.is_some() != wanted
        {
            continue;
        }

        if words.is_empty() {
            scored.push((0, entry.found(None)));
            continue;
        }

        // Le nom pèse plus lourd que la note, qui pèse plus lourd que le chemin.
        let name = entry.name().to_lowercase();
        let rel = entry.rel.to_lowercase();
        let note = entry.note.clone().unwrap_or_default().to_lowercase();
        let mut score = 0;
        let mut miss = false;
        for word in &words {
            if name.contains(word.as_str()) {
                score += 6;
            } else if note.contains(word.as_str()) {
                score += 4;
            } else if rel.contains(word.as_str())
                || entry.meta.iter().any(|(k, v)| {
                    k.contains(word.as_str()) || v.to_lowercase().contains(word.as_str())
                })
            {
                score += 3;
            } else {
                // Tous les mots doivent correspondre (ET) : c'est ce qui permet
                // d'affiner la question au lieu de tout recevoir.
                miss = true;
                break;
            }
        }
        if miss {
            continue;
        }
        if !phrase.is_empty() && name.contains(&phrase) {
            score += 10;
        }
        scored.push((score, entry.found(match_hint(entry, &words))));
    }

    // À score égal, le plus récemment modifié d'abord : c'est presque toujours
    // celui qui intéresse.
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.modified.unwrap_or(0).cmp(&a.1.modified.unwrap_or(0)))
            .then_with(|| a.1.rel.cmp(&b.1.rel))
    });

    let total = scored.len();
    let limit = filter.limit.clamp(1, HITS_MAX);
    let results = scored
        .into_iter()
        .take(limit)
        .map(|(_, found)| found)
        .collect();

    FoundSet { results, total }
}

/// Indique au modèle **pourquoi** un élément est remonté (`name:boitier`,
/// `note:PLA`, `meta:filament_type=PETG`), pour qu'il rédige une raison juste.
fn match_hint(entry: &Entry, words: &[String]) -> Option<String> {
    for word in words {
        if entry.name().to_lowercase().contains(word.as_str()) {
            return Some(format!("name:{word}"));
        }
        if entry
            .note
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains(word.as_str())
        {
            return Some(format!("note:{word}"));
        }
        if let Some((key, value)) = entry
            .meta
            .iter()
            .find(|(k, v)| k.contains(word.as_str()) || v.to_lowercase().contains(word.as_str()))
        {
            return Some(format!("meta:{key}={value}"));
        }
    }
    None
}

/// Lit la note d'un élément du catalogue.
fn read_note(state: &AppState, args: &Value) -> String {
    let Some(rel) = text(args, "rel") else {
        return "il manque le paramètre « rel ».".to_string();
    };
    let root = state.root();
    let known = catalog(state).iter().any(|e| e.rel == rel);
    if !known {
        return format!(
            "« {rel} » n'existe pas dans le catalogue : reprends un chemin exact renvoyé par \
             search_models."
        );
    }

    match notes::read(&root, &rel) {
        Some(note) if !note.trim().is_empty() => json!({
            "rel": rel,
            "note": excerpt(&note, NOTE_FULL),
            "truncated": note.chars().count() > NOTE_FULL,
        })
        .to_string(),
        _ => format!("« {rel} » n'a pas de note."),
    }
}

/// Conclusion : on ne garde que des chemins réellement présents.
fn submit_results(state: &AppState, args: &Value) -> Vec<Hit> {
    let empty = Vec::new();
    let results = args
        .get("results")
        .and_then(Value::as_array)
        .unwrap_or(&empty);

    let catalog = catalog(state);
    let mut hits: Vec<Hit> = Vec::new();

    for result in results.iter().take(RESULTS_MAX) {
        let Some(rel) = result.get("rel").and_then(Value::as_str) else {
            continue;
        };
        let rel = rel.trim().trim_start_matches("./").replace('\\', "/");
        // Chemin inconnu : le modèle a inventé, on l'écarte silencieusement
        // (inutile de lui faire un tour de plus pour ça).
        let Some(entry) = catalog.iter().find(|e| e.rel == rel) else {
            continue;
        };
        if hits.iter().any(|h| h.rel == entry.rel) {
            continue;
        }

        let reason = result
            .get("reason")
            .and_then(Value::as_str)
            .map(|r| excerpt(r, REASON_MAX))
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| "proposé par la recherche assistée".to_string());

        hits.push(Hit {
            rel: entry.rel.clone(),
            kind: entry.kind,
            reason,
        });
    }

    hits
}

/// Sérialise une charge utile, en la bornant : un outil ne doit jamais noyer la
/// conversation (le nom du dossier racine peut être long, les notes aussi).
fn to_json(value: Value) -> String {
    let text = value.to_string();
    if text.len() <= MAX_TOOL_OUTPUT {
        return text;
    }

    // Le catalogue est volumineux : on le vide de ses éléments plutôt que de
    // couper le JSON au milieu (le modèle lit mal un JSON tronqué).
    let mut trimmed = value;
    if let Some(results) = trimmed.get_mut("results").and_then(Value::as_array_mut) {
        results.truncate(5);
    }
    for key in ["loose_files", "top_level_folders", "formats"] {
        if let Some(array) = trimmed.get_mut(key).and_then(Value::as_array_mut) {
            array.truncate(5);
        }
    }
    trimmed["truncated"] = json!(true);
    trimmed.to_string()
}

/// Taille maximale d'un résultat d'outil.
const MAX_TOOL_OUTPUT: usize = 24 * 1024;

/// Texte d'un paramètre, nettoyé.
fn text(args: &Value, key: &str) -> Option<String> {
    let raw = args.get(key).and_then(Value::as_str)?.trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_string())
    }
}

/// Extrait : première ligne utile, espaces normalisés.
pub fn excerpt(text: &str, max: usize) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max {
        return one_line;
    }
    let cut: String = one_line.chars().take(max).collect();
    format!("{cut}…")
}

/// Taille lisible d'un fichier.
fn size_label(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Âge d'une date, en forme compacte (`5h`, `3d`, `2mo`, `1y`).
fn age_label(modified: u64) -> String {
    let seconds = now().saturating_sub(modified);
    match seconds {
        0..=90 => format!("{seconds}s"),
        s if s < 3600 => format!("{}min", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s if s < 2_592_000 => format!("{}d", s / 86_400),
        s if s < 31_536_000 => format!("{}mo", s / 2_592_000),
        s => format!("{}y", s / 31_536_000),
    }
}

/// Horodatage courant, en secondes unix.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::fs;

    fn state_with_catalog() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("models");
        fs::create_dir_all(root.join("Maison")).unwrap();
        fs::create_dir_all(root.join("Voiture/jantes")).unwrap();

        fs::write(root.join("Boitier dema auto.stl"), b"solid x\n").unwrap();
        fs::write(
            root.join("Maison/toit.gcode"),
            "; filament_type = PETG\n; layer_height = 0.2\n",
        )
        .unwrap();
        fs::write(root.join("Voiture/jantes/roue.3mf"), b"zip\n").unwrap();
        fs::write(root.join("Maison/notes.txt"), b"divers\n").unwrap();

        let config = crate::config::Config {
            models_root: Some(root.to_string_lossy().to_string()),
            ..Config::default()
        };
        let (ws, _) = tokio::sync::broadcast::channel(4);
        let state = AppState::new(&root, ws, config).with_config_path(dir.path().join("c.yml"));

        // Une note sur le boîtier, pour la recherche par note.
        notes::write(
            &root,
            "Boitier dema auto.stl",
            "Le **boîtier** de la carte, en PLA.",
        )
        .unwrap();
        (dir, state)
    }

    fn call(name: &str, args: &str) -> ToolCall {
        ToolCall {
            id: "1".to_string(),
            name: name.to_string(),
            arguments: serde_json::from_str(args).unwrap(),
        }
    }

    fn text_of(state: &AppState, tool: &str, args: &str) -> String {
        match run(state, &call(tool, args)) {
            Ran::Continue(text) => text,
            Ran::Final(_) => panic!("{tool} ne conclut pas la recherche"),
        }
    }

    fn hits_of(state: &AppState, args: &str) -> Vec<Hit> {
        match run(state, &call("submit_results", args)) {
            Ran::Final(hits) => hits,
            Ran::Continue(_) => panic!("submit_results doit conclure"),
        }
    }

    #[test]
    fn overview_resume_le_catalogue() {
        let (_dir, state) = state_with_catalog();
        let value: Value =
            serde_json::from_str(&text_of(&state, "catalog_overview", "{}")).unwrap();

        assert_eq!(value["counts"]["files"], 4);
        assert_eq!(value["counts"]["top_level_folders"], 2);
        assert_eq!(value["counts"]["subfolders"], 1);

        let folders: Vec<String> = value["top_level_folders"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["rel"].as_str().unwrap().to_string())
            .collect();
        assert!(folders.contains(&"Maison".to_string()), "{folders:?}");

        let loose: Vec<String> = value["loose_files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["rel"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(loose, vec!["Boitier dema auto.stl"]);

        let formats: Vec<String> = value["formats"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f.as_str().unwrap().to_string())
            .collect();
        assert!(formats.contains(&"gcode".to_string()), "{formats:?}");
    }

    #[test]
    fn la_recherche_trouve_par_nom() {
        let (_dir, state) = state_with_catalog();
        let value: Value =
            serde_json::from_str(&text_of(&state, "search_models", r#"{"query":"boitier"}"#))
                .unwrap();

        assert_eq!(value["returned"], 1);
        assert_eq!(value["results"][0]["rel"], "Boitier dema auto.stl");
        assert_eq!(value["results"][0]["match"], "name:boitier");
        assert_eq!(value["results"][0]["kind"], "file");
        assert!(
            value["results"][0]["size"]
                .as_str()
                .unwrap()
                .ends_with(" B")
        );
    }

    #[test]
    fn la_recherche_trouve_par_note() {
        let (_dir, state) = state_with_catalog();
        let value: Value = serde_json::from_str(&text_of(
            &state,
            "search_models",
            r#"{"query":"carte PLA"}"#,
        ))
        .unwrap();

        let rels: Vec<&str> = value["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["rel"].as_str().unwrap())
            .collect();
        assert_eq!(rels, vec!["Boitier dema auto.stl"], "{value}");
        assert_eq!(value["results"][0]["match"], "note:carte");
    }

    #[test]
    fn la_recherche_trouve_par_metadonnee_de_gcode() {
        let (_dir, state) = state_with_catalog();
        let value: Value =
            serde_json::from_str(&text_of(&state, "search_models", r#"{"query":"PETG"}"#)).unwrap();

        assert_eq!(value["results"][0]["rel"], "Maison/toit.gcode");
        assert_eq!(
            value["results"][0]["meta"]["filament_type"], "PETG",
            "les métadonnées doivent accompagner la fiche : {value}"
        );
        assert!(
            value["results"][0]["match"]
                .as_str()
                .unwrap()
                .starts_with("meta:"),
            "{value}"
        );
    }

    #[test]
    fn la_recherche_filtre_par_dossier_extension_et_note() {
        let (_dir, state) = state_with_catalog();

        let value: Value = serde_json::from_str(&text_of(
            &state,
            "search_models",
            r#"{"folder":"Voiture","ext":"3mf"}"#,
        ))
        .unwrap();
        assert_eq!(value["returned"], 1);
        assert_eq!(value["results"][0]["rel"], "Voiture/jantes/roue.3mf");
        assert_eq!(value["results"][0]["folder"], "Voiture/jantes");

        let value: Value =
            serde_json::from_str(&text_of(&state, "search_models", r#"{"has_note":true}"#))
                .unwrap();
        let rels: Vec<&str> = value["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["rel"].as_str().unwrap())
            .collect();
        assert_eq!(rels, vec!["Boitier dema auto.stl"], "{value}");

        let value: Value =
            serde_json::from_str(&text_of(&state, "search_models", r#"{"kind":"folder"}"#))
                .unwrap();
        assert_eq!(value["returned"], 3);
    }

    #[test]
    fn la_recherche_tous_mots_obligatoires() {
        let (_dir, state) = state_with_catalog();
        let value: Value = serde_json::from_str(&text_of(
            &state,
            "search_models",
            r#"{"query":"boitier inexistant"}"#,
        ))
        .unwrap();

        assert_eq!(value["matched"], 0);
        assert!(value["results"].as_array().unwrap().is_empty());
    }

    #[test]
    fn la_recherche_annonce_les_resultats_non_affiches() {
        let (_dir, state) = state_with_catalog();
        let value: Value = serde_json::from_str(&text_of(
            &state,
            "search_models",
            r#"{"query":"", "limit":2}"#,
        ))
        .unwrap();

        assert_eq!(value["returned"], 2);
        assert_eq!(value["matched"], 7);
        assert!(value["note"].as_str().unwrap().contains("5 autres"));
    }

    #[test]
    fn lire_une_note_inconnue_ou_absente() {
        let (_dir, state) = state_with_catalog();

        let text = text_of(&state, "read_note", r#"{"rel":"Boitier dema auto.stl"}"#);
        assert!(text.contains("carte"), "{text}");

        let text = text_of(&state, "read_note", r#"{"rel":"Maison/toit.gcode"}"#);
        assert_eq!(text, "« Maison/toit.gcode » n'a pas de note.");

        let text = text_of(&state, "read_note", r#"{"rel":"inventé.stl"}"#);
        assert!(text.contains("n'existe pas"), "{text}");
    }

    #[test]
    fn la_conclusion_ecarte_les_chemins_inventes() {
        let (_dir, state) = state_with_catalog();
        let hits = hits_of(
            &state,
            r#"{"results":[
                {"rel":"/etc/passwd","reason":"inventé"},
                {"rel":"Maison/toit.gcode","reason":"imprimé en PETG"},
                {"rel":"Maison/toit.gcode","reason":"doublon"},
                {"rel":"", "reason":"vide"},
                {"rel":"Voiture","reason":"le dossier des pièces de voiture"}
            ]}"#,
        );

        let rels: Vec<&str> = hits.iter().map(|h| h.rel.as_str()).collect();
        assert_eq!(rels, vec!["Maison/toit.gcode", "Voiture"]);
        assert_eq!(hits[0].kind, "file");
        assert_eq!(hits[1].kind, "folder");
        assert_eq!(hits[1].reason, "le dossier des pièces de voiture");
    }

    #[test]
    fn une_raison_absente_est_remplacee_et_bornee() {
        let (_dir, state) = state_with_catalog();
        let long = "x".repeat(500);
        let hits = hits_of(
            &state,
            &format!(r#"{{"results":[{{"rel":"Voiture","reason":"{long}"}},{{"rel":"Maison"}}]}}"#),
        );

        assert_eq!(hits.len(), 2);
        assert_eq!(
            hits[0].reason.chars().count(),
            REASON_MAX + 1,
            "raison non bornée"
        );
        assert!(hits[0].reason.ends_with('…'));
        assert_eq!(hits[1].reason, "proposé par la recherche assistée");
    }

    #[test]
    fn une_conclusion_vide_reste_vide() {
        let (_dir, state) = state_with_catalog();
        assert!(hits_of(&state, r#"{"results":[]}"#).is_empty());
        // Modèle bavard : des arguments qui ne ressemblent à rien.
        assert!(hits_of(&state, "{}").is_empty());
    }

    #[test]
    fn un_outil_inconnu_le_dit_et_liste_les_autres() {
        let (_dir, state) = state_with_catalog();
        let text = text_of(&state, "ouvre_la_porte", "{}");
        assert!(text.contains("outil inconnu"), "{text}");
        assert!(text.contains("search_models"), "{text}");
    }

    #[test]
    fn les_specs_sont_exploitables_par_un_modele() {
        let specs = specs();
        assert_eq!(specs.len(), 4);
        for spec in &specs {
            assert!(
                !spec.description.is_empty(),
                "{} sans description",
                spec.name
            );
            assert_eq!(spec.schema["type"], "object", "{}", spec.name);
            assert!(spec.schema["properties"].is_object(), "{}", spec.name);
        }

        let search = specs.iter().find(|s| s.name == "search_models").unwrap();
        assert!(search.schema["properties"]["has_note"]["description"].is_string());

        let submit = specs.iter().find(|s| s.name == "submit_results").unwrap();
        assert_eq!(
            submit.schema["properties"]["results"]["items"]["required"][0],
            "rel"
        );
    }

    #[test]
    fn les_tailles_et_ages_sont_lisibles() {
        assert_eq!(size_label(512), "512 B");
        assert_eq!(size_label(2048), "2.0 KB");
        assert_eq!(size_label(5 * 1024 * 1024), "5.0 MB");

        let now = now();
        assert_eq!(age_label(now), "0s");
        assert_eq!(age_label(now - 120), "2min");
        assert_eq!(age_label(now - 7200), "2h");
        assert_eq!(age_label(now - 3 * 86_400), "3d");
        assert_eq!(age_label(now - 60 * 86_400), "2mo");
        assert_eq!(age_label(now - 400 * 86_400), "1y");
    }
}
