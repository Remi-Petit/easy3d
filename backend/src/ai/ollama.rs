//! Fournisseur **Ollama** : un modèle local, sans clé API.
//!
//! Ollama expose une API compatible avec celle d'OpenAI ; ce fichier ne fait donc
//! que fixer les points d'entrée par défaut et signaler qu'aucune clé n'est
//! nécessaire, en réutilisant la construction de requête et la lecture de
//! réponse d'[`super::openai`]. Si Ollama s'écarte un jour du format, il suffira
//! de remplacer ces deux appels.

use super::Provider;
use super::openai;
use super::tools::Spec;
use super::{Reply, Request, Resolved, Turn};

/// Adresse par défaut d'un Ollama installé sur la même machine.
///
/// Les autres adresses de serveurs locaux (celle vue depuis un conteneur,
/// LM Studio) vivent dans `ai-presets.yml`, relu à chaud : voir
/// [`super::presets`].
const BASE_URL: &str = "http://localhost:11434/v1";

/// Modèle par défaut : un modèle courant, capable d'appeler des outils.
const MODEL: &str = "qwen3:8b";

/// Le fournisseur Ollama.
pub static OLLAMA: Ollama = Ollama;

pub struct Ollama;

impl Provider for Ollama {
    fn id(&self) -> &'static str {
        "ollama"
    }

    fn label(&self) -> &'static str {
        "Ollama (local)"
    }

    fn needs_key(&self) -> bool {
        false
    }

    fn default_base_url(&self) -> &'static str {
        BASE_URL
    }

    fn default_model(&self) -> &'static str {
        MODEL
    }

    fn headers(&self, _cfg: &Resolved) -> Vec<(String, String)> {
        // Pas d'en-tête d'autorisation : le service est local.
        Vec::new()
    }

    fn build(&self, cfg: &Resolved, system: &str, turns: &[Turn], tools: &[Spec]) -> Request {
        Request::post(
            format!("{}/chat/completions", cfg.base_url),
            self.headers(cfg),
            openai::build_body(cfg, system, turns, tools),
        )
    }

    fn parse(&self, _status: u16, body: &str) -> Result<Reply, String> {
        openai::parse_body(body)
    }

    fn error(&self, status: u16, body: &str) -> String {
        match super::error_message(body) {
            Some(message) => format!("Ollama a répondu {status} : {message}"),
            None => format!(
                "Ollama a répondu {status} : {}. Le serveur est-il lancé ?",
                super::short_body(body)
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg() -> Resolved {
        Resolved {
            provider: "ollama",
            base_url: BASE_URL.to_string(),
            model: MODEL.to_string(),
            api_key: None,
        }
    }

    #[test]
    fn aucune_cle_n_est_demandee() {
        assert!(!OLLAMA.needs_key());

        let request = OLLAMA.build(&cfg(), "s", &[Turn::User("a".to_string())], &[]);
        assert!(request.headers.is_empty(), "{:?}", request.headers);
        assert_eq!(request.url, "http://localhost:11434/v1/chat/completions");
    }

    #[test]
    fn la_requete_reprend_le_format_openai() {
        let request = OLLAMA.build(
            &cfg(),
            "tu cherches",
            &[Turn::User("une roue".to_string())],
            &super::super::tools::specs(),
        );

        assert_eq!(request.body["model"], MODEL);
        assert_eq!(request.body["messages"][0]["content"], "tu cherches");
        assert_eq!(request.body["tool_choice"], "auto");
        assert!(request.body["tools"].as_array().unwrap().len() >= 3);
    }

    #[test]
    fn la_reponse_reprend_la_lecture_openai() {
        let reply = OLLAMA
            .parse(
                200,
                r#"{"choices":[{"message":{"tool_calls":[{"id":"c","function":{
                    "name":"submit_results","arguments":"{\"results\":[]}"}}]}}]}"#,
            )
            .unwrap();

        assert_eq!(reply.calls[0].name, "submit_results");
        assert_eq!(reply.calls[0].arguments, json!({"results": []}));
    }

    #[test]
    fn un_serveur_eteint_donne_un_message_utile() {
        let message = OLLAMA.error(500, "<html>connection refused</html>");
        assert!(message.contains("Ollama"), "{message}");
        assert!(message.contains("lancé"), "{message}");
    }

    #[test]
    fn la_liste_des_modeles_locaux_se_passe_de_cle() {
        let request = OLLAMA.models_request(&cfg());

        assert_eq!(request.method, "GET");
        assert_eq!(request.url, "http://localhost:11434/v1/models");
        assert!(request.headers.is_empty(), "{:?}", request.headers);
    }

    #[test]
    fn les_modeles_locaux_sont_lus() {
        let body = r#"{"data":[{"id":"qwen3:8b"},{"id":"llama3.2:3b"}]}"#;
        assert_eq!(
            OLLAMA.parse_models(200, body).unwrap(),
            vec!["qwen3:8b", "llama3.2:3b"]
        );
    }
}
