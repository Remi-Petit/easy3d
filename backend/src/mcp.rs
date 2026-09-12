//! Serveur MCP (Model Context Protocol) : easy3d vu par un agent.
//!
//! Monté sur `/mcp` du **même** serveur axum (transport *Streamable HTTP*), donc
//! pas de second process à lancer : `cargo run` suffit.
//!
//! Ce n'est pas qu'une commodité : les outils partagent l'[`AppState`] vivant —
//! configuration rechargée à chaud, registre des notes collaboratives — et
//! chaque écriture emprunte le **même chemin** que l'interface :
//!
//! - les notes passent par le **CRDT** ([`collab`]), jamais par le `.md` : une
//!   écriture concurrente avec un onglet ouvert fusionne au lieu d'écraser, et
//!   la persistance (`.md` + `.ydoc`) suit le flush habituel ;
//! - la configuration passe par [`crate::api::apply_config`], comme
//!   `PUT /config` : le frontend est rafraîchi par le WebSocket.
//!
//! Les outils **destructeurs** (`delete_note`, `set_models_root`) ne sont
//! proposés qu'avec `EASY3D_MCP_ALLOW_WRITE=1` : par défaut, un agent ne peut
//! pas supprimer de travail.

use crate::api::{self, AppState};
use crate::collab;
use crate::config::{Config, DisplayMode};
use crate::formats::{self, Viewer};
use crate::notes;
use crate::scanner::{self, FileInfo};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `true` si `EASY3D_MCP_ALLOW_WRITE` autorise les outils destructeurs.
///
/// Valeurs acceptées : `1`, `true`, `yes` (insensible à la casse).
pub fn destructive_allowed_from_env() -> bool {
    std::env::var("EASY3D_MCP_ALLOW_WRITE")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

/// Serveur MCP d'easy3d.
#[derive(Clone)]
pub struct Easy3dMcp {
    /// État partagé du serveur HTTP : mêmes données, mêmes effets de bord.
    state: AppState,
    /// Autorise les outils destructeurs (cf. [`destructive_allowed_from_env`]).
    allow_destructive: bool,
    /// Table des outils, générée par `#[tool_router]`.
    tool_router: ToolRouter<Self>,
}

impl Easy3dMcp {
    /// Serveur avec les permissions de l'environnement.
    pub fn new(state: AppState) -> Self {
        Self::with_permissions(state, destructive_allowed_from_env())
    }

    /// Serveur avec des permissions explicites (utilisé par les tests).
    pub fn with_permissions(state: AppState, allow_destructive: bool) -> Self {
        Self {
            state,
            allow_destructive,
            tool_router: Self::tool_router(),
        }
    }

    /// Vérifie qu'un élément existe dans le catalogue.
    ///
    /// Les notes se rattachent à un élément **existant** (même règle que
    /// `Rooms::join`) : on ne crée pas de note orpheline. Valide aussi le chemin
    /// (mêmes garde-fous que le dossier de notes : pas de `..`, pas d'absolu).
    fn require_element(&self, rel: &str) -> Result<(), ErrorData> {
        let root = self.state.root();
        if notes::note_path(&root, rel).is_none() {
            return Err(ErrorData::invalid_params(
                format!("chemin invalide : « {rel} »"),
                None,
            ));
        }
        if !root.join(rel).exists() {
            return Err(ErrorData::invalid_params(
                format!("« {rel} » n'existe pas dans le catalogue"),
                None,
            ));
        }
        Ok(())
    }

    /// Écrit une note, en passant par le document collaboratif.
    fn write_note(&self, rel: &str, content: &str) -> Result<(), ErrorData> {
        collab::Rooms::set_text(&self.state, rel, content)
            .map_err(|msg| ErrorData::internal_error(msg, None))
    }

    /// Refuse un outil destructeur si l'environnement ne l'autorise pas.
    fn require_destructive(&self, tool: &str) -> Result<(), ErrorData> {
        if self.allow_destructive {
            return Ok(());
        }
        Err(ErrorData::invalid_params(
            format!(
                "l'outil « {tool} » est désactivé : démarrer le serveur avec \
                 EASY3D_MCP_ALLOW_WRITE=1 pour l'autoriser"
            ),
            None,
        ))
    }

    /// Tous les éléments du catalogue : `(dossier, entrée)`.
    ///
    /// Les dossiers apparaissent une fois (sous le nom de dossier) puisqu'ils
    /// peuvent porter une note, comme dans l'interface.
    fn elements(&self) -> Vec<(Option<String>, FileInfo)> {
        let scan = scanner::scan_models(&self.state.root());
        let mut out = Vec::new();
        for (name, folder) in scan.folders {
            out.push((
                Some(name),
                FileInfo {
                    path: String::new(),
                    rel: folder.name.clone(),
                    created: None,
                    modified: folder.modified,
                    image: None,
                    note: folder.note.clone(),
                },
            ));
            out.extend(
                folder
                    .files
                    .into_iter()
                    .map(|f| (Some(folder.name.clone()), f)),
            );
        }
        out.extend(scan.files.into_iter().map(|f| (None, f)));
        out.sort_by(|a, b| a.1.rel.cmp(&b.1.rel));
        out
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Easy3dMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("easy3d", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Catalogue de modèles 3D easy3d (STL, OBJ, 3MF, G-code) et ses notes Markdown.\n\
                 Commencer par list_models pour découvrir le catalogue, puis get_model pour le \
                 détail d'un fichier et read_note pour sa note.\n\
                 Une note est rattachée à un élément **existant** du catalogue (fichier ou \
                 dossier) ; son écriture est immédiatement visible dans l'application.",
            )
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Entrées / sorties des outils
// ─────────────────────────────────────────────────────────────────────────

/// Entrée des outils qui désignent un élément du catalogue.
#[derive(Debug, Deserialize, JsonSchema)]
struct ElementArgs {
    /// Chemin relatif dans le catalogue : `DemaAuto/boitier.stl` pour un
    /// fichier, `DemaAuto` pour un dossier.
    rel: String,
}

/// Entrée de `list_models`.
#[derive(Debug, Deserialize, JsonSchema)]
struct ListModelsArgs {
    /// Filtre sur le chemin, insensible à la casse (ex : `boitier`).
    #[serde(default)]
    query: Option<String>,
    /// Ne garder que ce dossier (nom exact, sans `models/`).
    #[serde(default)]
    folder: Option<String>,
    /// Ne garder que cette extension, sans le point (ex : `stl`).
    #[serde(default)]
    ext: Option<String>,
    /// Nombre maximum d'entrées renvoyées (défaut 50, maximum 500).
    #[serde(default)]
    limit: Option<u32>,
}

/// Entrée de `list_notes`.
#[derive(Debug, Deserialize, JsonSchema)]
struct ListNotesArgs {
    /// Ne considérer que ce dossier (nom exact).
    #[serde(default)]
    folder: Option<String>,
    /// Nombre maximum de notes renvoyées (défaut 50, maximum 500).
    #[serde(default)]
    limit: Option<u32>,
}

/// Entrée des outils d'écriture de note.
#[derive(Debug, Deserialize, JsonSchema)]
struct WriteNoteArgs {
    /// Chemin relatif de l'élément (fichier ou dossier) du catalogue.
    rel: String,
    /// Contenu Markdown **complet** de la note.
    content: String,
}

/// Entrée de `append_note`.
#[derive(Debug, Deserialize, JsonSchema)]
struct AppendNoteArgs {
    /// Chemin relatif de l'élément (fichier ou dossier) du catalogue.
    rel: String,
    /// Texte Markdown à ajouter à la fin de la note.
    text: String,
}

/// Entrée de `set_display_mode`.
#[derive(Debug, Deserialize, JsonSchema)]
struct SetDisplayModeArgs {
    /// `3d` pour la vue interactive, `image` pour l'aperçu statique.
    mode: DisplayModeArg,
}

/// Entrée de `set_models_root`.
#[derive(Debug, Deserialize, JsonSchema)]
struct SetModelsRootArgs {
    /// Dossier des modèles (absolu, ou relatif à `backend/`). Doit exister.
    path: String,
}

/// Mode d'affichage, tel qu'attendu par l'agent.
#[derive(Debug, Deserialize, JsonSchema)]
enum DisplayModeArg {
    /// Vue interactive (WebGL).
    #[serde(rename = "3d")]
    ThreeD,
    /// Aperçu statique (image) avec repli 3D.
    #[serde(rename = "image")]
    Image,
}

impl From<DisplayModeArg> for DisplayMode {
    fn from(value: DisplayModeArg) -> Self {
        match value {
            DisplayModeArg::ThreeD => DisplayMode::ThreeD,
            DisplayModeArg::Image => DisplayMode::Image,
        }
    }
}

/// Résumé d'un élément du catalogue.
#[derive(Debug, Serialize, JsonSchema)]
struct ModelSummary {
    /// Chemin relatif (clé du catalogue et des notes).
    rel: String,
    /// Dossier d'appartenance (`null` si l'élément est à la racine).
    folder: Option<String>,
    /// `true` si l'élément est un dossier.
    is_folder: bool,
    /// Extension en minuscules (vide pour un dossier).
    ext: String,
    /// Visionneuse : `mesh`, `gcode` ou `none`.
    viewer: Viewer,
    /// Une note est rattachée.
    has_note: bool,
    /// Un aperçu image existe (image sœur ou vignette générée).
    has_preview: bool,
    /// Dernière modification, en secondes unix.
    modified: Option<u64>,
}

/// Sortie de `list_models`.
#[derive(Debug, Serialize, JsonSchema)]
struct ListModelsOutput {
    /// Entrées correspondant aux filtres.
    count: usize,
    /// Entrées effectivement renvoyées (bornées par `limit`).
    returned: usize,
    models: Vec<ModelSummary>,
}

/// Sortie de `get_model`.
#[derive(Debug, Serialize, JsonSchema)]
struct ModelDetail {
    rel: String,
    folder: Option<String>,
    ext: String,
    viewer: Viewer,
    /// Le format est reconnu par le backend (et peut produire un aperçu).
    format_has_preview: bool,
    /// Image d'aperçu associée, si elle existe.
    image: Option<String>,
    has_note: bool,
    /// Taille du fichier en octets (`null` pour un dossier).
    size_bytes: Option<u64>,
    created: Option<u64>,
    modified: Option<u64>,
    /// Chemin absolu sur le disque.
    abs_path: String,
}

/// Sortie des outils de note.
#[derive(Debug, Serialize, JsonSchema)]
struct NoteContent {
    rel: String,
    /// Une note existe après l'opération (toujours `true` après une écriture).
    exists: bool,
    content: String,
}

/// Résumé d'une note.
#[derive(Debug, Serialize, JsonSchema)]
struct NoteSummary {
    rel: String,
    /// Nombre de caractères.
    chars: usize,
}

/// Sortie de `list_notes`.
#[derive(Debug, Serialize, JsonSchema)]
struct ListNotesOutput {
    /// Éléments **avec** une note.
    notes: Vec<NoteSummary>,
    /// Nombre d'éléments sans note (candidats à documenter).
    without_note: usize,
}

/// Sortie de `get_config`.
#[derive(Debug, Serialize, JsonSchema)]
struct ConfigView {
    /// Dossier tel qu'écrit dans la configuration (peut être relatif).
    models_root: Option<String>,
    /// Dossier que cette configuration désigne (résolu).
    resolved_models_root: String,
    /// Dossier réellement surveillé en ce moment. Il peut différer de
    /// `resolved_models_root` pendant quelques dizaines de millisecondes après
    /// un changement de dossier : c'est le watcher qui rebascule la surveillance.
    watched_models_root: String,
    display_mode: DisplayMode,
    /// Chemin du fichier de configuration.
    config_path: String,
    /// Les outils destructeurs sont-ils autorisés ?
    destructive_tools_allowed: bool,
}

/// Contenu d'une note : le CRDT s'il est ouvert (à jour à la milliseconde),
/// sinon le Markdown sur disque (jusqu'à 250 ms de retard).
fn live_or_disk(state: &AppState, rel: &str) -> Option<String> {
    match collab::Rooms::live_text(state, rel) {
        // Un document **vidé** vaut note supprimée (le flush des fichiers suit) :
        // renvoyer la chaîne vide ferait croire que la note existe encore.
        Some(text) if !text.trim().is_empty() => Some(text),
        Some(_) => None,
        None => notes::read(&state.root(), rel),
    }
}

/// Résumé d'un élément du catalogue.
fn summary(folder: Option<String>, file: FileInfo) -> ModelSummary {
    let is_folder = file.path.is_empty();
    ModelSummary {
        ext: formats::ext_of(&file.rel),
        viewer: formats::viewer_of(&file.rel),
        has_note: file.note.is_some(),
        has_preview: file.image.is_some(),
        modified: file.modified,
        is_folder,
        folder,
        rel: file.rel,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Outils
// ─────────────────────────────────────────────────────────────────────────

#[tool_router]
impl Easy3dMcp {
    /// Formats de fichiers reconnus par easy3d.
    #[tool(
        description = "List the file formats supported by easy3d: name, extensions, \
                       whether a PNG preview can be generated, and which viewer is used."
    )]
    fn list_formats(&self) -> Json<Vec<formats::FormatInfo>> {
        Json(formats::describe())
    }

    /// Catalogue des modèles, avec filtres.
    #[tool(
        description = "List the models available in the easy3d catalogue, optionally \
                       filtered by name (query), folder or extension. Folders appear as \
                       entries too, since they can carry a note. Use a small limit and \
                       narrow filters to keep the answer readable."
    )]
    fn list_models(&self, Parameters(args): Parameters<ListModelsArgs>) -> Json<ListModelsOutput> {
        let mut entries = self.elements();

        if let Some(query) = args
            .query
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
        {
            let query = query.to_lowercase();
            entries.retain(|(_, f)| f.rel.to_lowercase().contains(&query));
        }
        if let Some(folder) = args
            .folder
            .as_deref()
            .map(str::trim)
            .filter(|f| !f.is_empty())
        {
            entries.retain(|(name, f)| {
                // Un dossier se liste lui-même, sinon on filtre sur son parent.
                name.as_deref() == Some(folder) || f.rel == folder
            });
        }
        if let Some(ext) = args.ext.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
            let ext = ext.trim_start_matches('.').to_lowercase();
            entries.retain(|(_, f)| formats::ext_of(&f.rel) == ext);
        }

        let count = entries.len();
        let limit = args.limit.unwrap_or(50).min(500) as usize;
        let models: Vec<ModelSummary> = entries
            .into_iter()
            .take(limit)
            .map(|(folder, file)| summary(folder, file))
            .collect();

        Json(ListModelsOutput {
            count,
            returned: models.len(),
            models,
        })
    }

    /// Détail d'un élément du catalogue.
    #[tool(
        description = "Get the details of one catalogue entry: relative path, format and \
                       viewer, preview image, whether it has a note, size, dates and the \
                       absolute path on disk."
    )]
    fn get_model(
        &self,
        Parameters(args): Parameters<ElementArgs>,
    ) -> Result<Json<ModelDetail>, ErrorData> {
        self.require_element(&args.rel)?;
        let root = self.state.root();
        let (folder, file) = self
            .elements()
            .into_iter()
            .find(|(_, f)| f.rel == args.rel)
            .ok_or_else(|| {
                ErrorData::invalid_params(format!("« {} » n'existe pas", args.rel), None)
            })?;

        let metadata = std::fs::metadata(root.join(&args.rel)).ok();
        let size_bytes = metadata.as_ref().and_then(|m| m.is_file().then_some(m.len()));

        Ok(Json(ModelDetail {
            format_has_preview: formats::can_have_preview(&file.rel),
            abs_path: root.join(&args.rel).display().to_string(),
            has_note: file.note.is_some(),
            image: file.image,
            viewer: formats::viewer_of(&file.rel),
            ext: formats::ext_of(&file.rel),
            created: file.created,
            modified: file.modified,
            folder,
            rel: args.rel,
            size_bytes,
        }))
    }

    /// Notes existantes, et nombre d'éléments à documenter.
    #[tool(
        description = "List the Markdown notes attached to catalogue entries, and how many \
                       entries have none yet — useful to find what still needs documentation."
    )]
    fn list_notes(&self, Parameters(args): Parameters<ListNotesArgs>) -> Json<ListNotesOutput> {
        let folder = args
            .folder
            .as_deref()
            .map(str::trim)
            .filter(|f| !f.is_empty());

        let mut notes_found = Vec::new();
        let mut without_note = 0usize;

        for (parent, file) in self.elements() {
            if let Some(folder) = folder
                && parent.as_deref() != Some(folder)
                && file.rel != folder
            {
                continue;
            }
            if notes::note_path(&self.state.root(), &file.rel).is_none() {
                continue;
            }
            match live_or_disk(&self.state, &file.rel) {
                Some(content) => notes_found.push(NoteSummary {
                    chars: content.chars().count(),
                    rel: file.rel,
                }),
                None => without_note += 1,
            }
        }

        let count = notes_found.len();
        let limit = args.limit.unwrap_or(50).min(500) as usize;
        notes_found.truncate(limit);

        Json(ListNotesOutput {
            notes: notes_found,
            without_note: without_note + count.saturating_sub(limit),
        })
    }

    /// Lit la note d'un élément.
    #[tool(
        description = "Read the Markdown note attached to a catalogue entry. Returns \
                       exists=false when the entry has no note yet."
    )]
    fn read_note(
        &self,
        Parameters(args): Parameters<ElementArgs>,
    ) -> Result<Json<NoteContent>, ErrorData> {
        self.require_element(&args.rel)?;
        let content = live_or_disk(&self.state, &args.rel);
        Ok(Json(NoteContent {
            exists: content.is_some(),
            content: content.unwrap_or_default(),
            rel: args.rel,
        }))
    }

    /// Crée la note d'un élément.
    #[tool(
        description = "Create the Markdown note of a catalogue entry. Fails if a note \
                       already exists (use update_note or append_note instead). The note \
                       is written through the collaborative document, so open editors see \
                       it immediately and concurrent edits merge instead of being lost."
    )]
    fn create_note(
        &self,
        Parameters(args): Parameters<WriteNoteArgs>,
    ) -> Result<Json<NoteContent>, ErrorData> {
        self.require_element(&args.rel)?;
        if live_or_disk(&self.state, &args.rel).is_some() {
            return Err(ErrorData::invalid_params(
                format!(
                    "une note existe déjà pour « {} » : utiliser update_note ou append_note",
                    args.rel
                ),
                None,
            ));
        }
        if args.content.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "contenu vide : une note vide n'est pas enregistrée (utiliser delete_note pour supprimer)",
                None,
            ));
        }

        self.write_note(&args.rel, &args.content)?;
        Ok(Json(NoteContent {
            rel: args.rel,
            exists: true,
            content: args.content,
        }))
    }

    /// Remplace le contenu d'une note.
    #[tool(
        description = "Replace the whole content of an existing note. Fails if there is no \
                       note yet (use create_note)."
    )]
    fn update_note(
        &self,
        Parameters(args): Parameters<WriteNoteArgs>,
    ) -> Result<Json<NoteContent>, ErrorData> {
        self.require_element(&args.rel)?;
        if live_or_disk(&self.state, &args.rel).is_none() {
            return Err(ErrorData::invalid_params(
                format!("aucune note pour « {} » : utiliser create_note", args.rel),
                None,
            ));
        }

        if args.content.trim().is_empty() {
            // Cohérent avec `notes::write` : un contenu vide supprime la note.
            return Err(ErrorData::invalid_params(
                "contenu vide : utiliser delete_note pour supprimer la note",
                None,
            ));
        }

        self.write_note(&args.rel, &args.content)?;
        Ok(Json(NoteContent {
            rel: args.rel,
            exists: true,
            content: args.content,
        }))
    }

    /// Ajoute du texte à la fin d'une note.
    #[tool(
        description = "Append Markdown text at the end of a note, creating the note if it \
                       does not exist yet. Handy to document a model without rewriting the \
                       whole note."
    )]
    fn append_note(
        &self,
        Parameters(args): Parameters<AppendNoteArgs>,
    ) -> Result<Json<NoteContent>, ErrorData> {
        self.require_element(&args.rel)?;
        if args.text.trim().is_empty() {
            return Err(ErrorData::invalid_params("texte à ajouter vide", None));
        }

        let current = live_or_disk(&self.state, &args.rel).unwrap_or_default();
        let mut content = current.trim_end().to_string();
        if !content.is_empty() {
            content.push_str("\n\n");
        }
        content.push_str(args.text.trim_end());
        content.push('\n');

        self.write_note(&args.rel, &content)?;
        Ok(Json(NoteContent {
            rel: args.rel,
            exists: true,
            content,
        }))
    }

    /// Supprime la note d'un élément (outil destructeur).
    #[tool(
        description = "Delete the note of a catalogue entry (also removes the CRDT state). \
                       Destructive: disabled unless the server runs with \
                       EASY3D_MCP_ALLOW_WRITE=1."
    )]
    fn delete_note(
        &self,
        Parameters(args): Parameters<ElementArgs>,
    ) -> Result<Json<NoteContent>, ErrorData> {
        self.require_destructive("delete_note")?;
        self.require_element(&args.rel)?;
        if live_or_disk(&self.state, &args.rel).is_none() {
            return Err(ErrorData::invalid_params(
                format!("aucune note à supprimer pour « {} »", args.rel),
                None,
            ));
        }

        // Note vidée : le flush supprime le `.md` **et** l'état CRDT, et
        // rediffuse le catalogue (la pastille 📝 disparaît partout).
        self.write_note(&args.rel, "")?;
        Ok(Json(NoteContent {
            rel: args.rel,
            exists: false,
            content: String::new(),
        }))
    }

    /// Configuration appliquée par le serveur.
    #[tool(
        description = "Get the current configuration: the models directory (configured, \
                       resolved and currently watched), the display mode and the config \
                       file path."
    )]
    fn get_config(&self) -> Json<ConfigView> {
        Json(config_view(
            &self.state,
            &self.state.config(),
            self.allow_destructive,
        ))
    }

    /// Change le mode d'affichage du catalogue.
    #[tool(
        description = "Switch the catalogue display mode between the interactive 3D view \
                       (\"3d\") and the static image preview (\"image\"). Applied \
                       immediately, also visible in the running application."
    )]
    fn set_display_mode(
        &self,
        Parameters(args): Parameters<SetDisplayModeArgs>,
    ) -> Result<Json<ConfigView>, ErrorData> {
        let mut config = self.state.config();
        config.display.mode = args.mode.into();
        let written = apply(&self.state, config)?;
        Ok(Json(config_view(
            &self.state,
            &written,
            self.allow_destructive,
        )))
    }

    /// Change le dossier des modèles surveillé (outil destructeur).
    #[tool(
        description = "Point easy3d at another models directory. The directory must exist. \
                       Destructive (the catalogue switches to that directory): disabled \
                       unless the server runs with EASY3D_MCP_ALLOW_WRITE=1."
    )]
    fn set_models_root(
        &self,
        Parameters(args): Parameters<SetModelsRootArgs>,
    ) -> Result<Json<ConfigView>, ErrorData> {
        self.require_destructive("set_models_root")?;
        let mut config = self.state.config();
        config.models_root = Some(args.path);
        let written = apply(&self.state, config)?;
        Ok(Json(config_view(
            &self.state,
            &written,
            self.allow_destructive,
        )))
    }
}

/// Vue de la configuration **écrite** (celle que le watcher va appliquer).
fn config_view(state: &AppState, config: &Config, allow_destructive: bool) -> ConfigView {
    let resolved = config.resolve_models_root();
    ConfigView {
        resolved_models_root: api::tidy_path(&resolved),
        watched_models_root: api::tidy_path(&state.root()),
        models_root: config.models_root.clone(),
        display_mode: config.display.mode,
        config_path: state.config_path.display().to_string(),
        destructive_tools_allowed: allow_destructive,
    }
}

/// Enregistre et applique une configuration (même chemin que `PUT /config`).
///
/// Renvoie la configuration **écrite**. Un changement de dossier des modèles
/// n'est pas appliqué dans la seconde : c'est le watcher qui rebascule la
/// surveillance du répertoire (voir [`crate::api::apply_config`]).
fn apply(state: &AppState, config: Config) -> Result<Config, ErrorData> {
    let response = api::apply_config(state, config).map_err(|(status, message)| {
        if status == axum::http::StatusCode::BAD_REQUEST {
            ErrorData::invalid_params(message, None)
        } else {
            ErrorData::internal_error(message, None)
        }
    })?;
    Ok(response.config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;

    /// Catalogue jetable : un fichier à la racine, un dossier avec un G-code.
    ///
    /// La configuration **désigne ce dossier** (comme celle que le serveur a
    /// chargée en production) et son chemin est redirigé vers le dossier
    /// temporaire : sans ça, les outils de configuration réécriraient le
    /// `config.yml` du dépôt.
    fn test_server(allow_destructive: bool) -> (tempfile::TempDir, Easy3dMcp) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("piece.stl"), "solid x").unwrap();
        std::fs::create_dir_all(dir.path().join("DemaAuto")).unwrap();
        std::fs::write(dir.path().join("DemaAuto/boitier.gcode"), "G1 X0").unwrap();
        std::fs::write(dir.path().join("lisez-moi.txt"), "rien à voir").unwrap();

        let config = Config {
            models_root: Some(dir.path().display().to_string()),
            ..Config::default()
        };
        let (ws, _) = broadcast::channel::<String>(16);
        let state = AppState::new(dir.path().to_path_buf(), ws, config)
            .with_config_path(dir.path().join("config.yml"));

        (dir, Easy3dMcp::with_permissions(state, allow_destructive))
    }

    /// Laisse le flush collaboratif (250 ms) écrire le `.md` sur disque.
    async fn wait_flush() {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    #[test]
    fn annonce_les_formats_du_registre() {
        let (_dir, server) = test_server(false);
        let names: Vec<&str> = server.list_formats().0.iter().map(|f| f.name).collect();

        assert!(names.contains(&"STL"));
        assert!(names.contains(&"G-code"));
    }

    #[test]
    fn liste_et_filtre_le_catalogue() {
        let (_dir, server) = test_server(false);

        let all = server
            .list_models(Parameters(ListModelsArgs {
                query: None,
                folder: None,
                ext: None,
                limit: None,
            }))
            .0;
        // 2 fichiers + le dossier + le .txt : tout le catalogue.
        assert_eq!(all.count, 4);

        let gcode = server
            .list_models(Parameters(ListModelsArgs {
                query: None,
                folder: None,
                ext: Some(".gcode".into()),
                limit: None,
            }))
            .0;
        assert_eq!(gcode.count, 1);
        assert_eq!(gcode.models[0].rel, "DemaAuto/boitier.gcode");
        assert_eq!(gcode.models[0].folder.as_deref(), Some("DemaAuto"));
        assert_eq!(gcode.models[0].viewer, Viewer::Gcode);

        let search = server
            .list_models(Parameters(ListModelsArgs {
                query: Some("boitier".into()),
                folder: None,
                ext: None,
                limit: None,
            }))
            .0;
        assert_eq!(search.count, 1);
    }

    #[test]
    fn decrit_un_element_du_catalogue() {
        let (_dir, server) = test_server(false);
        let detail = server
            .get_model(Parameters(ElementArgs {
                rel: "piece.stl".into(),
            }))
            .unwrap()
            .0;

        assert_eq!(detail.ext, "stl");
        assert_eq!(detail.viewer, Viewer::Mesh);
        assert!(detail.format_has_preview);
        assert!(!detail.has_note);
        assert!(detail.size_bytes.is_some());

        // Un élément inconnu est refusé, pas inventé.
        let missing = server.get_model(Parameters(ElementArgs {
            rel: "inconnu.stl".into(),
        }));
        assert!(missing.is_err());
    }

    #[tokio::test]
    async fn cree_lit_et_ecrit_une_note() {
        let (dir, server) = test_server(false);

        server
            .create_note(Parameters(WriteNoteArgs {
                rel: "piece.stl".into(),
                content: "# Boîtier\n\nPremière version.".into(),
            }))
            .unwrap();

        // La lecture voit la note immédiatement (document CRDT ouvert)…
        let read = server
            .read_note(Parameters(ElementArgs {
                rel: "piece.stl".into(),
            }))
            .unwrap()
            .0;
        assert!(read.exists);
        assert!(read.content.contains("Première version."));

        // …et le Markdown arrive sur disque au flush suivant.
        wait_flush().await;
        let disk = notes::read(dir.path(), "piece.stl").expect("note écrite sur disque");
        assert!(disk.contains("Première version."));

        // Créer deux fois est refusé.
        assert!(
            server
                .create_note(Parameters(WriteNoteArgs {
                    rel: "piece.stl".into(),
                    content: "autre".into(),
                }))
                .is_err()
        );

        // Ajout, puis remplacement complet.
        let appended = server
            .append_note(Parameters(AppendNoteArgs {
                rel: "piece.stl".into(),
                text: "## Suite\n\nÀ imprimer en 0.2 mm.".into(),
            }))
            .unwrap()
            .0;
        assert!(appended.content.starts_with("# Boîtier"));
        assert!(appended.content.ends_with("0.2 mm.\n"));

        server
            .update_note(Parameters(WriteNoteArgs {
                rel: "piece.stl".into(),
                content: "# Remplacé".into(),
            }))
            .unwrap();
        let read = server
            .read_note(Parameters(ElementArgs {
                rel: "piece.stl".into(),
            }))
            .unwrap()
            .0;
        assert_eq!(read.content, "# Remplacé");
    }

    #[tokio::test]
    async fn une_note_se_rattache_a_un_element_du_catalogue() {
        let (_dir, server) = test_server(false);

        // Les dossiers portent aussi une note.
        server
            .create_note(Parameters(WriteNoteArgs {
                rel: "DemaAuto".into(),
                content: "# Dossier".into(),
            }))
            .unwrap();
        let read = server
            .read_note(Parameters(ElementArgs {
                rel: "DemaAuto".into(),
            }))
            .unwrap()
            .0;
        assert_eq!(read.content, "# Dossier");

        // Chemin hors catalogue ou fichier inexistant : refusés.
        for rel in ["../secret.txt", "absent.stl", "/etc/passwd"] {
            assert!(
                server
                    .create_note(Parameters(WriteNoteArgs {
                        rel: rel.into(),
                        content: "x".into(),
                    }))
                    .is_err(),
                "devrait refuser {rel}"
            );
        }
    }

    #[tokio::test]
    async fn la_suppression_est_conditionnee_au_drapeau() {
        let (dir, server) = test_server(false);
        server
            .create_note(Parameters(WriteNoteArgs {
                rel: "piece.stl".into(),
                content: "# À supprimer".into(),
            }))
            .unwrap();

        // Refusé : la suppression n'est pas autorisée par l'environnement.
        let refused = server.delete_note(Parameters(ElementArgs {
            rel: "piece.stl".into(),
        }));
        assert!(refused.is_err());
        assert!(notes::read(dir.path(), "piece.stl").is_none());
        wait_flush().await;
        assert!(notes::read(dir.path(), "piece.stl").is_some());

        // Autorisé : la note et son état CRDT disparaissent.
        let (_dir2, allowed) = test_server(true);
        allowed
            .create_note(Parameters(WriteNoteArgs {
                rel: "piece.stl".into(),
                content: "# À supprimer".into(),
            }))
            .unwrap();
        let deleted = allowed
            .delete_note(Parameters(ElementArgs {
                rel: "piece.stl".into(),
            }))
            .unwrap()
            .0;
        assert!(!deleted.exists);

        let read = allowed
            .read_note(Parameters(ElementArgs {
                rel: "piece.stl".into(),
            }))
            .unwrap()
            .0;
        assert!(!read.exists);

        // Supprimer une note inexistante est signalé, pas silencieux.
        assert!(
            allowed
                .delete_note(Parameters(ElementArgs {
                    rel: "piece.stl".into(),
                }))
                .is_err()
        );
    }

    #[tokio::test]
    async fn recense_les_notes_et_les_elements_a_documenter() {
        let (_dir, server) = test_server(false);
        server
            .create_note(Parameters(WriteNoteArgs {
                rel: "piece.stl".into(),
                content: "# Note".into(),
            }))
            .unwrap();

        let report = server
            .list_notes(Parameters(ListNotesArgs {
                folder: None,
                limit: None,
            }))
            .0;

        assert_eq!(report.notes.len(), 1);
        assert_eq!(report.notes[0].rel, "piece.stl");
        assert_eq!(report.notes[0].chars, "# Note".chars().count());
        // Les 3 autres entrées n'ont pas de note (dossier, gcode, txt).
        assert_eq!(report.without_note, 3);
    }

    #[tokio::test]
    async fn change_le_mode_d_affichage() {
        let (dir, server) = test_server(false);

        let before = server.get_config().0;
        assert_eq!(before.display_mode, DisplayMode::ThreeD);
        assert!(!before.destructive_tools_allowed);

        let after = server
            .set_display_mode(Parameters(SetDisplayModeArgs {
                mode: DisplayModeArg::Image,
            }))
            .unwrap()
            .0;
        assert_eq!(after.display_mode, DisplayMode::Image);

        // Appliqué à chaud **et** écrit dans le fichier de configuration.
        assert_eq!(server.state.config().display.mode, DisplayMode::Image);
        let yaml = std::fs::read_to_string(dir.path().join("config.yml")).unwrap();
        assert!(yaml.contains("image"), "config.yml : {yaml}");
    }

    #[tokio::test]
    async fn change_le_dossier_des_modeles_seulement_si_autorise() {
        let (dir, server) = test_server(false);
        let other = dir.path().join("autres");
        std::fs::create_dir_all(&other).unwrap();

        let refused = server.set_models_root(Parameters(SetModelsRootArgs {
            path: other.display().to_string(),
        }));
        assert!(refused.is_err());

        let (_dir2, allowed) = test_server(true);
        let view = allowed
            .set_models_root(Parameters(SetModelsRootArgs {
                path: other.display().to_string(),
            }))
            .unwrap()
            .0;
        assert_eq!(
            view.models_root.as_deref(),
            Some(other.display().to_string().as_str())
        );
        // Le catalogue, lui, surveille encore l'ancien dossier : c'est le
        // watcher qui rebascule (le frontend le suit par le WebSocket).
        assert_eq!(
            view.watched_models_root,
            api::tidy_path(&allowed.state.root())
        );

        // Un dossier inexistant est refusé (sinon le catalogue se viderait).
        let missing = allowed.set_models_root(Parameters(SetModelsRootArgs {
            path: dir.path().join("pas-la").display().to_string(),
        }));
        assert!(missing.is_err());
    }
}
