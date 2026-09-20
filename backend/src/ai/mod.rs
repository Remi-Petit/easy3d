//! Recherche assistée : l'utilisateur décrit ce qu'il cherche, un modèle de
//! langage interroge le catalogue et propose des fichiers.
//!
//! ## Les outils tournent dans le processus
//!
//! Le modèle ne reçoit jamais le catalogue entier : il dispose de quelques
//! [outils](tools) qui lisent le disque **sur place**, sans réseau
//! supplémentaire (le serveur MCP expose les mêmes données par ailleurs, mais
//! passer par HTTP pour s'appeler soi-même n'ajouterait que de la latence).
//!
//! Le vrai coût d'une recherche, ce sont les **allers-retours vers le modèle** :
//! un par appel d'outil. D'où deux garde-fous — un outil de recherche locale
//! (`search_models`) pour que le modèle trouve en un ou deux tours plutôt qu'en
//! lisant le catalogue, et un plafond d'étapes ([`MAX_HOPS`]) pour qu'une
//! question impossible ne fasse pas tourner la facture.
//!
//! ## Un fichier par fournisseur
//!
//! Chaque fournisseur (OpenAI, Anthropic, Ollama…) a son fichier et n'expose que
//! du **calcul pur** : [`Provider::build`] fabrique la requête, [`Provider::parse`]
//! lit la réponse. L'appel HTTP lui-même est fait une seule fois, ici, par
//! [`send`]. Deux conséquences : ajouter un fournisseur ne touche à rien d'autre,
//! et tout se teste sans réseau (voir les tests de chaque fichier, et ceux de la
//! boucle complète en bas de ce module).

mod anthropic;
mod ollama;
mod openai;
pub mod presets;
pub mod tools;

use crate::api::AppState;
use crate::config::Ai;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// Nombre maximal d'allers-retours vers le modèle dans une recherche.
///
/// Six étapes laissent largement de quoi s'orienter, chercher, relire une note
/// et conclure, tout en bornant le coût d'une question sans réponse.
pub const MAX_HOPS: usize = 6;

/// Taille maximale du corps d'une réponse acceptée (garde-fou mémoire).
const MAX_BODY: usize = 512 * 1024;

/// Délai maximal d'un appel au fournisseur.
const TIMEOUT: Duration = Duration::from_secs(90);

/// Demande d'outil formulée par le modèle.
#[derive(Clone, Debug)]
pub struct ToolCall {
    /// Identifiant à réutiliser dans le tour `Tool` (OpenAI l'exige).
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Réponse d'un modèle : du texte, des appels d'outils, ou les deux.
#[derive(Clone, Debug, Default)]
pub struct Reply {
    pub text: Option<String>,
    pub calls: Vec<ToolCall>,
}

/// Un tour de conversation, dans une forme commune à tous les fournisseurs.
#[derive(Clone, Debug)]
pub enum Turn {
    User(String),
    Assistant(Reply),
    Tool {
        id: String,
        name: String,
        content: String,
    },
}

/// Requête HTTP prête à partir.
pub struct Request {
    /// `POST` (conversation) ou `GET` (liste des modèles).
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Corps JSON ; ignoré pour un `GET`.
    pub body: Value,
}

impl Request {
    /// Requête de conversation.
    pub fn post(url: String, headers: Vec<(String, String)>, body: Value) -> Self {
        Self {
            method: "POST",
            url,
            headers,
            body,
        }
    }

    /// Requête de lecture (liste des modèles).
    pub fn get(url: String, headers: Vec<(String, String)>) -> Self {
        Self {
            method: "GET",
            url,
            headers,
            body: Value::Null,
        }
    }
}

/// Paramètres effectifs d'un appel : la config, complétée par les défauts du
/// fournisseur.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub provider: &'static str,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

/// Point d'entrée connu d'un fournisseur : un nom de service et l'adresse qui
/// va avec.
///
/// Le fournisseur choisi dit **comment** parler (dialecte OpenAI, Anthropic…) ;
/// cette liste dit **à qui** : plusieurs services exposent le même protocole
/// tel quel (DeepSeek, OpenRouter, Groq…), et l'utilisateur n'a pas à retenir
/// leur adresse.
///
/// Elle vient du fichier [`presets`], pas du code : c'est de la donnée, que l'on
/// modifie — et ajoute — sans recompiler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preset {
    /// Nom du service, tel qu'il s'affiche sur la puce.
    pub label: String,
    /// Adresse à écrire dans le champ correspondant.
    pub base_url: String,
    /// Modèle conseillé, **vide** quand il n'y a rien à conseiller (un serveur
    /// local n'a que les modèles que l'utilisateur y a installés). Ce n'est
    /// jamais qu'une proposition : la vraie liste vient de « Tester ».
    #[serde(default)]
    pub model: String,
}

/// Description d'un fournisseur, pour l'écran d'administration.
#[derive(Serialize)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub label: &'static str,
    /// `true` s'il faut une clé API (faux pour un Ollama local).
    pub needs_key: bool,
    pub base_url: &'static str,
    pub model: &'static str,
    /// Adresses connues de ce fournisseur, **la sienne en premier** (voir
    /// [`Preset`] et [`presets`]).
    pub presets: Vec<Preset>,
}

/// Interface commune aux fournisseurs de modèles.
///
/// Volontairement **synchrone** : fabriquer une requête et lire une réponse sont
/// des transformations de texte, pas des entrées-sorties. Le trait reste donc
/// utilisable en `dyn` sans `async_trait`, et se teste avec des chaînes de
/// caractères.
pub trait Provider: Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    /// Faut-il une clé API ?
    fn needs_key(&self) -> bool {
        true
    }
    fn default_base_url(&self) -> &'static str;
    fn default_model(&self) -> &'static str;

    /// En-têtes d'authentification (et de version), communs à tous les appels.
    fn headers(&self, cfg: &Resolved) -> Vec<(String, String)>;

    /// Fabrique la requête du prochain tour.
    fn build(&self, cfg: &Resolved, system: &str, turns: &[Turn], tools: &[tools::Spec])
    -> Request;

    /// Lit une réponse réussie.
    fn parse(&self, status: u16, body: &str) -> Result<Reply, String>;

    /// Requête qui liste les modèles accessibles avec cette clé.
    ///
    /// `GET {base}/models` est le point d'entrée commun : OpenAI et les serveurs
    /// qui l'imitent (Ollama, DeepSeek…) l'exposent tel quel, et Anthropic en
    /// fait autant avec ses propres en-têtes.
    fn models_request(&self, cfg: &Resolved) -> Request {
        Request::get(format!("{}/models", cfg.base_url), self.headers(cfg))
    }

    /// Lit la liste des modèles.
    ///
    /// Le format `{"data": [{"id": …}]}` est celui des trois fournisseurs, ce
    /// qui évite un fichier de plus pour une simple lecture.
    fn parse_models(&self, status: u16, body: &str) -> Result<Vec<String>, String> {
        if status >= 400 {
            return Err(self.error(status, body));
        }
        if let Some(message) = error_message(body) {
            return Err(message);
        }

        let value: Value = serde_json::from_str(body)
            .map_err(|e| format!("réponse illisible du fournisseur : {e}"))?;
        let mut models: Vec<String> = Vec::new();
        for item in value
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(id) = item.get("id").and_then(Value::as_str) else {
                continue;
            };
            if is_chat_model(id) && !models.iter().any(|known| known == id) {
                models.push(id.to_string());
            }
        }

        if models.is_empty() {
            return Err("le fournisseur n'annonce aucun modèle de conversation.".to_string());
        }
        Ok(models)
    }

    /// Message d'erreur lisible pour un statut en échec.
    fn error(&self, status: u16, body: &str) -> String {
        format!("le fournisseur a répondu {status} : {}", short_body(body))
    }
}

/// Écarte les modèles qui ne servent pas à une conversation.
///
/// Les fournisseurs renvoient embeddings, synthèse vocale et génération
/// d'images dans la même liste que les modèles de conversation : sans ce tri, le
/// choix serait noyé.
fn is_chat_model(id: &str) -> bool {
    const NON_CHAT: [&str; 7] = [
        "embed",
        "whisper",
        "tts",
        "dall-e",
        "moderation",
        "transcribe",
        "audio",
    ];
    let id = id.to_lowercase();
    !NON_CHAT.iter().any(|needle| id.contains(needle))
}

/// Les fournisseurs connus, dans l'ordre où l'administration les propose.
pub static PROVIDERS: &[&(dyn Provider + Sync)] =
    &[&openai::OPENAI, &anthropic::ANTHROPIC, &ollama::OLLAMA];

/// Fournisseur correspondant à un identifiant de configuration.
pub fn provider_for(id: &str) -> Option<&'static (dyn Provider + Sync)> {
    PROVIDERS.iter().copied().find(|p| p.id() == id)
}

/// Catalogue des fournisseurs, pour l'écran d'administration.
///
/// Les adresses connues viennent du fichier [`presets`] (relu à chaud) :
/// `presets` est donc l'état courant, pas une constante. Un fournisseur absent
/// du fichier n'en a aucune — l'interface se contente alors du champ libre.
pub fn describe(presets: &presets::Presets) -> Vec<ProviderInfo> {
    PROVIDERS
        .iter()
        .map(|p| ProviderInfo {
            id: p.id(),
            label: p.label(),
            needs_key: p.needs_key(),
            base_url: p.default_base_url(),
            model: p.default_model(),
            presets: presets.get(p.id()).cloned().unwrap_or_default(),
        })
        .collect()
}

/// Complète la configuration avec les défauts du fournisseur.
///
/// Renvoie un message **destiné à l'utilisateur** quand la recherche ne peut pas
/// partir : il finit tel quel dans l'interface.
pub fn resolve(ai: &Ai) -> Result<Resolved, String> {
    let id = ai
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("aucun fournisseur d'IA configuré : choisis-en un dans l'administration.")?;
    let provider =
        provider_for(id).ok_or_else(|| format!("fournisseur d'IA inconnu : « {id} »."))?;

    let api_key = ai
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if provider.needs_key() && api_key.is_none() {
        return Err(format!(
            "clé API manquante pour {} : renseigne-la dans l'administration.",
            provider.label()
        ));
    }

    Ok(Resolved {
        provider: provider.id(),
        base_url: base_url_or_default(ai.base_url.as_deref(), provider.default_base_url()),
        model: text_or_default(ai.model.as_deref(), provider.default_model()),
        api_key,
    })
}

/// Adresse de base, avec un schéma ajouté si l'utilisateur a recopié
/// `localhost:11434` sans `http://`.
fn base_url_or_default(given: Option<&str>, default: &str) -> String {
    let url = text_or_default(given, default);
    let trimmed = url.trim_end_matches('/').to_string();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed
    } else {
        format!("http://{trimmed}")
    }
}

/// Première valeur non vide, sinon le défaut.
fn text_or_default(given: Option<&str>, default: &str) -> String {
    given
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default)
        .to_string()
}

/// Résultat d'une recherche assistée.
#[derive(Debug, Serialize)]
pub struct Outcome {
    /// Propositions retenues par le modèle, dans son ordre de pertinence.
    pub hits: Vec<tools::Hit>,
    /// Réponse libre du modèle, quand il n'a pas conclu par des résultats.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Nombre d'allers-retours effectués (information de diagnostic).
    pub hops: usize,
}

/// Réponse brute d'un fournisseur : code HTTP et corps.
type Raw = Result<(u16, String), String>;

/// Envoi d'une requête, représenté comme une valeur pour pouvoir être remplacé
/// par un scénario dans les tests.
type Answer = Pin<Box<dyn Future<Output = Raw> + Send>>;

/// Recherche assistée sur le catalogue courant.
pub async fn search(state: &AppState, query: &str) -> Result<Outcome, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("décris ce que tu cherches.".to_string());
    }

    let cfg = resolve(&state.config().ai)?;
    let provider = provider_for(cfg.provider)
        .ok_or_else(|| format!("fournisseur d'IA inconnu : « {} ».", cfg.provider))?;

    let client = client()?;
    search_with(&cfg, provider, state, query, move |req| {
        let client = client.clone();
        Box::pin(async move { send(&client, req).await })
    })
    .await
}

/// Liste les modèles proposés par le fournisseur, pour un jeu de paramètres
/// donné.
///
/// C'est ce qui remplit la liste déroulante de l'administration : plutôt que de
/// faire saisir un nom de modèle (qui change, et que personne ne connaît par
/// cœur), on demande au fournisseur ce que la clé donne droit d'utiliser.
///
/// Rien n'est enregistré ici : la configuration transmise peut venir d'un
/// formulaire non encore validé.
pub async fn list_models(ai: &Ai) -> Result<Vec<String>, String> {
    let cfg = resolve(ai)?;
    let provider = provider_for(cfg.provider)
        .ok_or_else(|| format!("fournisseur d'IA inconnu : « {} ».", cfg.provider))?;

    let client = client()?;
    models_with(&cfg, provider, move |req| {
        let client = client.clone();
        Box::pin(async move { send(&client, req).await })
    })
    .await
}

/// Lecture de la liste des modèles, sur un transport fourni par l'appelant.
async fn models_with<F>(
    cfg: &Resolved,
    provider: &'static (dyn Provider + Sync),
    mut transport: F,
) -> Result<Vec<String>, String>
where
    F: FnMut(Request) -> Answer + Send,
{
    let (status, body) = transport(provider.models_request(cfg)).await?;
    provider.parse_models(status, &body)
}

/// Boucle de recherche, sur un transport fourni par l'appelant.
async fn search_with<F>(
    cfg: &Resolved,
    provider: &'static (dyn Provider + Sync),
    state: &AppState,
    query: &str,
    mut transport: F,
) -> Result<Outcome, String>
where
    F: FnMut(Request) -> Answer + Send,
{
    let specs = tools::specs();
    let system = system_prompt();
    let mut turns = vec![Turn::User(query.to_string())];

    for hop in 0..MAX_HOPS {
        let request = provider.build(cfg, system, &turns, &specs);
        let (status, body) = transport(request).await?;
        if status >= 400 {
            return Err(provider.error(status, &body));
        }
        if body.len() > MAX_BODY {
            return Err(format!(
                "réponse du fournisseur trop volumineuse ({} Mo).",
                body.len() / (1024 * 1024)
            ));
        }

        let reply = provider.parse(status, &body)?;
        let text = reply.text.clone().filter(|t| !t.trim().is_empty());
        let calls = reply.calls.clone();
        turns.push(Turn::Assistant(reply));

        // Réponse sans outil : le modèle a parlé au lieu de chercher. On la
        // remonte telle quelle plutôt que d'inventer un résultat.
        if calls.is_empty() {
            return Ok(Outcome {
                hits: Vec::new(),
                text,
                hops: hop + 1,
            });
        }

        let mut finished = None;
        for call in &calls {
            match tools::run(state, call) {
                tools::Ran::Continue(content) => turns.push(Turn::Tool {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    content,
                }),
                tools::Ran::Final(hits) => {
                    finished = Some(hits);
                    turns.push(Turn::Tool {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        content: "Résultats enregistrés.".to_string(),
                    });
                }
            }
        }

        if let Some(hits) = finished {
            return Ok(Outcome {
                hits,
                text,
                hops: hop + 1,
            });
        }
    }

    Err(format!(
        "le modèle n'a pas conclu en {MAX_HOPS} étapes : reformule la question, ou précise ce que tu cherches."
    ))
}

/// Consigne donnée au modèle, identique pour tous les fournisseurs.
///
/// Elle tient en trois idées : chercher avant de répondre, ne citer que des
/// chemins réellement renvoyés par les outils, et conclure par `submit_results`.
fn system_prompt() -> &'static str {
    "Tu es l'assistant de recherche d'« easy3d », un catalogue de fichiers d'impression 3D \
(maillages STL/OBJ/3MF, G-codes, images) rangés en dossiers, chacun pouvant porter une note \
Markdown, et dont les G-codes annoncent leurs métadonnées de découpe (matière, hauteur de \
couche, temps d'impression...).

L'utilisateur décrit ce qu'il cherche en langage naturel. Tu trouves les fichiers et dossiers \
qui correspondent, en te servant des outils :

- `catalog_overview` pour voir les dossiers et les formats disponibles ;
- `search_models` pour chercher (nom, dossier, note, métadonnées de G-code, extensions, \
présence de note). Enchaîne plusieurs appels pour affiner si le premier est trop large ;
- `read_note` pour lire une note qui semble décisive.

Règles :
- Ne propose **que** des chemins (`rel`) renvoyés par les outils. N'invente jamais un chemin.
- Termine **toujours** par `submit_results`, jamais par une liste en texte libre. Une liste \
vide est une réponse valable quand rien ne correspond.
- Entre 1 et 8 résultats, du plus pertinent au moins pertinent, avec pour chacun une raison \
courte et vérifiable : un mot du nom, la note, une métadonnée de G-code, une date, un dossier.
- Les âges sont compacts : `3d` = 3 jours, `5h` = 5 heures, `2mo` = 2 mois, `1y` = un an.
- Écris les raisons dans la langue de la question de l'utilisateur.
- Si la question ne concerne pas le catalogue, réponds par `submit_results` avec une liste vide \
et explique-le dans une raison."
}

/// Client HTTP partagé : construire un client par appel recréerait le pool de
/// connexions et la négociation TLS à chaque étape.
fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(concat!("easy3d/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("client HTTP indisponible : {e}"))
}

/// Envoie une requête et renvoie statut + corps.
async fn send(client: &reqwest::Client, request: Request) -> Raw {
    // `GET` sert à lire la liste des modèles ; tout le reste est un `POST` JSON.
    let mut builder = if request.method == "GET" {
        client.get(&request.url)
    } else {
        client.post(&request.url)
    };
    for (name, value) in &request.headers {
        builder = builder.header(name, value);
    }

    let response = if request.method == "GET" {
        builder.send().await
    } else {
        builder.json(&request.body).send().await
    }
    .map_err(|e| format!("appel de {} impossible : {e}", request.url))?;
    let status = response.status().as_u16();
    let body = response
        .text()
        .await
        .map_err(|e| format!("réponse de {} illisible : {e}", request.url))?;

    // Un corps en erreur part directement dans l'interface : on le borne.
    if status >= 400 {
        return Ok((status, short_body(&body)));
    }
    Ok((status, body))
}

/// Extrait le message d'erreur d'un corps de réponse, s'il y en a un.
pub(crate) fn error_message(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let error = value.get("error")?;
    if let Some(text) = error.as_str() {
        return Some(text.to_string());
    }
    error
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Réduit un texte à une ligne courte (statuts, corps d'erreur).
pub(crate) fn short_body(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= 300 {
        one_line
    } else {
        let cut: String = one_line.chars().take(300).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Ai;
    use std::sync::Mutex;

    fn temp_state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("piece.stl"), b"solid x\n").unwrap();

        let (ws, _) = tokio::sync::broadcast::channel(4);
        let config = crate::config::Config {
            ai: ai_config("openai", Some("sk-test"), None),
            ..Default::default()
        };
        let state = AppState::new(&models, ws, config).with_config_path(dir.path().join("c.yml"));
        (dir, state)
    }

    /// Configuration d'IA réduite à ce qu'un test a besoin de fixer.
    fn ai_config(provider: &str, key: Option<&str>, base_url: Option<&str>) -> Ai {
        Ai {
            provider: Some(provider.to_string()),
            base_url: base_url.map(str::to_string),
            api_key: key.map(str::to_string),
            ..Default::default()
        }
    }

    fn json(value: &str) -> Value {
        serde_json::from_str(value).unwrap()
    }

    #[test]
    fn resolve_complete_les_defauts_du_fournisseur() {
        let cfg = resolve(&ai_config("ollama", None, None)).unwrap();

        assert_eq!(cfg.base_url, "http://localhost:11434/v1");
        assert_eq!(cfg.model, ollama::OLLAMA.default_model());
        assert!(cfg.api_key.is_none());
    }

    #[test]
    fn resolve_reclame_le_fournisseur_et_la_cle() {
        assert!(resolve(&Ai::default()).unwrap_err().contains("fournisseur"));

        let ai = ai_config("openai", None, None);
        assert!(resolve(&ai).unwrap_err().contains("clé API"));

        // Une clé faite d'espaces vaut une clé absente.
        let ai = ai_config("openai", Some("   "), None);
        assert!(resolve(&ai).unwrap_err().contains("clé API"));

        // Ollama n'en demande pas.
        assert!(resolve(&ai_config("ollama", None, None)).is_ok());

        let ai = ai_config("skynet", None, None);
        assert!(resolve(&ai).unwrap_err().contains("inconnu"));
    }

    #[test]
    fn resolve_ajoute_le_schema_manquant() {
        let ai = ai_config("ollama", None, Some("localhost:11434/v1/"));
        assert_eq!(resolve(&ai).unwrap().base_url, "http://localhost:11434/v1");

        let ai = ai_config("ollama", None, Some("https://proxy.interne/v1"));
        assert_eq!(resolve(&ai).unwrap().base_url, "https://proxy.interne/v1");
    }

    #[test]
    fn les_fournisseurs_ont_des_identifiants_uniques() {
        let ids: Vec<&str> = PROVIDERS.iter().map(|p| p.id()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len(), "identifiants en double : {ids:?}");
        assert!(
            describe(&presets::defaults())
                .iter()
                .all(|p| !p.label.is_empty())
        );
    }

    /// Les adresses servies à l'interface sont celles du fichier courant, et un
    /// fournisseur qu'il ignore n'en propose simplement aucune.
    #[test]
    fn le_catalogue_sert_les_adresses_du_fichier() {
        let mut known = presets::defaults();
        known.get_mut("ollama").unwrap().push(Preset {
            label: "Passerelle".to_string(),
            base_url: "https://gw.interne/v1".to_string(),
            model: String::new(),
        });
        known.remove("anthropic");

        let described = describe(&known);
        let ollama = described.iter().find(|p| p.id == "ollama").unwrap();
        assert!(ollama.presets.iter().any(|p| p.label == "Passerelle"));
        let anthropic = described.iter().find(|p| p.id == "anthropic").unwrap();
        assert!(anthropic.presets.is_empty());
    }

    #[test]
    fn error_message_lit_les_deux_formes_de_reponse() {
        assert_eq!(
            error_message(r#"{"error":{"message":"Invalid API key"}}"#).as_deref(),
            Some("Invalid API key")
        );
        assert_eq!(
            error_message(r#"{"error":"quota dépassé"}"#).as_deref(),
            Some("quota dépassé")
        );
        assert_eq!(error_message("<html>502</html>"), None);
    }

    /// La boucle complète, avec des réponses de modèle scriptées : le premier
    /// tour cherche, le second conclut. On vérifie au passage que le résultat de
    /// l'outil est bien **renvoyé** au modèle dans le tour suivant.
    #[tokio::test]
    async fn la_boucle_enchaine_les_outils_et_conclut() {
        let (_dir, state) = temp_state();
        let seen: Mutex<Vec<Value>> = Mutex::new(Vec::new());

        let outcome = search_with(
            &resolve(&state.config().ai).unwrap(),
            &openai::OPENAI,
            &state,
            "une pièce",
            |request| {
                seen.lock().unwrap().push(request.body.clone());
                // Le numéro du tour est lu **avant** le bloc asynchrone : dedans,
                // on ne capture qu'une valeur copiée.
                let hop = seen.lock().unwrap().len();
                Box::pin(async move {
                    let body = if hop == 1 {
                        json(
                            r#"{"choices":[{"message":{"content":null,"tool_calls":[
                                {"id":"c1","type":"function","function":{
                                 "name":"search_models","arguments":"{\"query\":\"piece\"}"}}]}}]}"#,
                        )
                    } else {
                        json(
                            r#"{"choices":[{"message":{"content":null,"tool_calls":[{"id":"c2","type":"function","function":{"name":"submit_results","arguments":"{\"results\":[{\"rel\":\"piece.stl\",\"reason\":\"nom demandé\"}]}"}}]}}]}"#,
                        )
                    };
                    Ok((200u16, body.to_string()))
                }) as Answer
            },
        )
        .await
        .unwrap();

        assert_eq!(outcome.hops, 2);
        assert_eq!(outcome.hits.len(), 1);
        assert_eq!(outcome.hits[0].rel, "piece.stl");
        assert_eq!(outcome.hits[0].reason, "nom demandé");

        let bodies = seen.lock().unwrap();
        assert_eq!(bodies.len(), 2);
        let second = bodies[1].to_string();
        assert!(
            second.contains("piece.stl") && second.contains("\"role\":\"tool\""),
            "le résultat de l'outil doit revenir dans la conversation : {second}"
        );
    }

    /// Une réponse en texte libre (sans appel d'outil) est remontée telle quelle,
    /// sans être prise pour une liste de résultats.
    #[tokio::test]
    async fn une_reponse_sans_outil_est_remontee_en_texte() {
        let (_dir, state) = temp_state();

        let outcome = search_with(
            &resolve(&state.config().ai).unwrap(),
            &openai::OPENAI,
            &state,
            "bonjour",
            |_req| {
                Box::pin(async move {
                    Ok((
                        200u16,
                        json(r#"{"choices":[{"message":{"content":"Je ne peux pas aider."}}]}"#)
                            .to_string(),
                    ))
                }) as Answer
            },
        )
        .await
        .unwrap();

        assert!(outcome.hits.is_empty());
        assert_eq!(outcome.text.as_deref(), Some("Je ne peux pas aider."));
        assert_eq!(outcome.hops, 1);
    }

    /// Le modèle qui n'en finit pas est arrêté au plafond, avec un message clair.
    #[tokio::test]
    async fn la_boucle_s_arrete_au_plafond() {
        let (_dir, state) = temp_state();

        let err = search_with(
            &resolve(&state.config().ai).unwrap(),
            &openai::OPENAI,
            &state,
            "une pièce",
            |_req| {
                Box::pin(async move {
                    Ok((
                        200u16,
                        json(
                            r#"{"choices":[{"message":{"content":null,"tool_calls":[
                                {"id":"c","type":"function","function":{
                                 "name":"catalog_overview","arguments":"{}"}}]}}]}"#,
                        )
                        .to_string(),
                    ))
                }) as Answer
            },
        )
        .await
        .unwrap_err();

        assert!(err.contains("reformule"), "{err}");
    }

    /// Une erreur du fournisseur remonte son message, pas un statut nu.
    #[tokio::test]
    async fn une_erreur_du_fournisseur_est_expliquee() {
        let (_dir, state) = temp_state();

        let err = search_with(
            &resolve(&state.config().ai).unwrap(),
            &openai::OPENAI,
            &state,
            "une pièce",
            |_req| {
                Box::pin(async move {
                    Ok((
                        401u16,
                        json(r#"{"error":{"message":"Invalid API key"}}"#).to_string(),
                    ))
                }) as Answer
            },
        )
        .await
        .unwrap_err();

        assert!(
            err.contains("401") && err.contains("Invalid API key"),
            "{err}"
        );
    }

    /// Ce qui est retenu de chaque requête émise, pour vérifier le transport.
    type Seen = Mutex<Vec<(&'static str, String, Vec<(String, String)>)>>;

    /// La liste des modèles passe par le même transport que la conversation :
    /// on la vérifie sans réseau, en contrôlant la requête émise.
    #[tokio::test]
    async fn la_liste_des_modeles_est_lue() {
        let cfg = resolve(&ai_config("openai", Some("sk-test"), None)).unwrap();
        let seen: Seen = Mutex::new(Vec::new());

        let models = models_with(&cfg, &openai::OPENAI, |request| {
            seen.lock().unwrap().push((
                request.method,
                request.url.clone(),
                request.headers.clone(),
            ));
            Box::pin(async move {
                Ok((
                    200u16,
                    json(r#"{"data":[{"id":"gpt-4o"},{"id":"o3-mini"}]}"#).to_string(),
                ))
            }) as Answer
        })
        .await
        .unwrap();

        assert_eq!(models, vec!["gpt-4o", "o3-mini"]);

        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].0, "GET");
        assert_eq!(seen[0].1, "https://api.openai.com/v1/models");
        assert!(
            seen[0]
                .2
                .iter()
                .any(|(name, value)| name == "authorization" && value == "Bearer sk-test"),
            "la clé doit accompagner la requête : {:?}",
            seen[0].2
        );
    }

    /// Une clé refusée empêche de lister les modèles, avec le message du
    /// fournisseur (c'est ce que verra l'utilisateur dans l'administration).
    #[tokio::test]
    async fn une_cle_refusee_empeche_de_lister_les_modeles() {
        let cfg = resolve(&ai_config("openai", Some("sk-faux"), None)).unwrap();

        let err = models_with(&cfg, &openai::OPENAI, |_req| {
            Box::pin(async move {
                Ok((
                    401u16,
                    json(r#"{"error":{"message":"Invalid API key"}}"#).to_string(),
                ))
            }) as Answer
        })
        .await
        .unwrap_err();

        assert!(
            err.contains("401") && err.contains("Invalid API key"),
            "{err}"
        );
    }
}
