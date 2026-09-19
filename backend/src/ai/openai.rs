//! Fournisseur **OpenAI** (et compatibles : `POST /chat/completions`).
//!
//! C'est aussi le format qu'imitent la plupart des serveurs locaux ou
//! intermédiaires ; [Ollama](super::ollama) s'en sert tel quel. Le corps et la
//! lecture de la réponse sont donc exposés à l'intérieur du module `ai`, pour
//! être réutilisés sans dupliquer la logique.

use super::Provider;
use super::tools::Spec;
use super::{Reply, Request, Resolved, ToolCall, Turn, error_message, short_body};
use serde_json::{Value, json};

/// Points d'entrée par défaut.
const BASE_URL: &str = "https://api.openai.com/v1";
const MODEL: &str = "gpt-4o-mini";

/// Le fournisseur OpenAI.
pub static OPENAI: OpenAi = OpenAi;

pub struct OpenAi;

impl Provider for OpenAi {
    fn id(&self) -> &'static str {
        "openai"
    }

    fn label(&self) -> &'static str {
        "OpenAI"
    }

    fn default_base_url(&self) -> &'static str {
        BASE_URL
    }

    fn default_model(&self) -> &'static str {
        MODEL
    }

    fn headers(&self, cfg: &Resolved) -> Vec<(String, String)> {
        match &cfg.api_key {
            Some(key) => vec![("authorization".to_string(), format!("Bearer {key}"))],
            // Service local sans authentification (Ollama).
            None => Vec::new(),
        }
    }

    fn build(&self, cfg: &Resolved, system: &str, turns: &[Turn], tools: &[Spec]) -> Request {
        Request::post(
            format!("{}/chat/completions", cfg.base_url),
            self.headers(cfg),
            build_body(cfg, system, turns, tools),
        )
    }

    fn parse(&self, _status: u16, body: &str) -> Result<Reply, String> {
        parse_body(body)
    }

    fn error(&self, status: u16, body: &str) -> String {
        match error_message(body) {
            Some(message) => format!("OpenAI a répondu {status} : {message}"),
            None => format!("OpenAI a répondu {status} : {}", short_body(body)),
        }
    }
}

/// Corps d'une requête de conversation, dans le format `chat/completions`.
pub(super) fn build_body(cfg: &Resolved, system: &str, turns: &[Turn], tools: &[Spec]) -> Value {
    let mut messages = vec![json!({"role": "system", "content": system})];

    for turn in turns {
        match turn {
            Turn::User(text) => messages.push(json!({"role": "user", "content": text})),
            Turn::Assistant(reply) => {
                let mut message = json!({"role": "assistant"});
                if let Some(text) = &reply.text {
                    message["content"] = json!(text);
                }
                if !reply.calls.is_empty() {
                    let calls: Vec<Value> = reply
                        .calls
                        .iter()
                        .map(|call| {
                            json!({
                                "id": call.id,
                                "type": "function",
                                "function": {
                                    "name": call.name,
                                    // Les arguments repartent en **chaîne** JSON :
                                    // c'est le format d'échange d'OpenAI.
                                    "arguments": call.arguments.to_string(),
                                }
                            })
                        })
                        .collect();
                    message["tool_calls"] = json!(calls);
                }
                messages.push(message);
            }
            Turn::Tool { id, content, .. } => messages.push(json!({
                "role": "tool",
                "tool_call_id": id,
                "content": content,
            })),
        }
    }

    let mut body = json!({
        "model": cfg.model,
        "messages": messages,
    });
    if !tools.is_empty() {
        let declarations: Vec<Value> = tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.schema,
                    }
                })
            })
            .collect();
        body["tools"] = json!(declarations);
        body["tool_choice"] = json!("auto");
    }
    body
}

/// Lit une réponse de conversation, dans le format `chat/completions`.
pub(super) fn parse_body(body: &str) -> Result<Reply, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|e| format!("réponse illisible du fournisseur : {e}"))?;

    if let Some(message) = error_message(body) {
        return Err(message);
    }

    let message = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .ok_or("réponse du fournisseur sans message.")?;

    let text = message
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut calls = Vec::new();
    if let Some(raw_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for raw in raw_calls {
            let Some(function) = raw.get("function") else {
                continue;
            };
            let Some(name) = function.get("name").and_then(Value::as_str) else {
                continue;
            };
            // Les arguments arrivent sous forme de chaîne JSON ; un modèle qui
            // se trompe est traité comme un appel sans paramètre, et l'outil lui
            // répondra de recommencer.
            let arguments = function
                .get("arguments")
                .and_then(Value::as_str)
                .and_then(|raw| serde_json::from_str(raw).ok())
                .unwrap_or_else(|| json!({}));
            calls.push(ToolCall {
                id: raw
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                name: name.to_string(),
                arguments,
            });
        }
    }

    Ok(Reply { text, calls })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Resolved {
        Resolved {
            provider: "openai",
            base_url: BASE_URL.to_string(),
            model: MODEL.to_string(),
            api_key: Some("sk-test".to_string()),
        }
    }

    fn specs() -> Vec<Spec> {
        super::super::tools::specs()
    }

    #[test]
    fn la_requete_porte_le_systeme_les_tours_et_les_outils() {
        let turns = vec![
            Turn::User("une pièce en PETG".to_string()),
            Turn::Assistant(Reply {
                text: None,
                calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "search_models".to_string(),
                    arguments: json!({"query": "PETG"}),
                }],
            }),
            Turn::Tool {
                id: "call_1".to_string(),
                name: "search_models".to_string(),
                content: "{\"results\":[]}".to_string(),
            },
        ];

        let request = OPENAI.build(&cfg(), "tu cherches", &turns, &specs());

        assert_eq!(request.url, "https://api.openai.com/v1/chat/completions");
        assert_eq!(request.headers[0].0, "authorization");
        assert_eq!(request.headers[0].1, "Bearer sk-test");

        let body = &request.body;
        assert_eq!(body["model"], MODEL);
        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "tu cherches");
        assert_eq!(body["messages"][1]["content"], "une pièce en PETG");

        // L'appel d'outil doit être rejoué à l'identique, arguments en chaîne.
        let call = &body["messages"][2]["tool_calls"][0];
        assert_eq!(call["id"], "call_1");
        assert_eq!(call["function"]["name"], "search_models");
        assert_eq!(call["function"]["arguments"], "{\"query\":\"PETG\"}");

        // … et le résultat rattaché à cet appel.
        let tool = &body["messages"][3];
        assert_eq!(tool["role"], "tool");
        assert_eq!(tool["tool_call_id"], "call_1");
        assert_eq!(tool["content"], "{\"results\":[]}");

        let declared: Vec<&str> = body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
            .collect();
        assert!(declared.contains(&"search_models"), "{declared:?}");
        assert!(declared.contains(&"submit_results"), "{declared:?}");
        assert!(body["tools"][0]["function"]["parameters"]["type"].is_string());
    }

    #[test]
    fn sans_cle_l_en_tete_d_autorisation_disparait() {
        let mut cfg = cfg();
        cfg.api_key = None;
        let request = OPENAI.build(&cfg, "s", &[Turn::User("a".to_string())], &[]);

        assert!(request.headers.is_empty());
        assert!(request.body.get("tools").is_none());
    }

    #[test]
    fn la_reponse_avec_appel_d_outil_est_lue() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":null,
            "tool_calls":[{"id":"call_9","type":"function","function":{
                "name":"search_models","arguments":"{\"query\":\"roue\"}"}}]}}]}"#;

        let reply = OPENAI.parse(200, body).unwrap();
        assert!(reply.text.is_none());
        assert_eq!(reply.calls.len(), 1);
        assert_eq!(reply.calls[0].name, "search_models");
        assert_eq!(reply.calls[0].arguments["query"], "roue");
    }

    #[test]
    fn la_reponse_en_texte_est_lue() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"Voilà."}}]}"#;
        let reply = OPENAI.parse(200, body).unwrap();
        assert_eq!(reply.text.as_deref(), Some("Voilà."));
        assert!(reply.calls.is_empty());
    }

    #[test]
    fn des_arguments_illisibles_ne_ferment_pas_la_conversation() {
        let body = r#"{"choices":[{"message":{"tool_calls":[{"id":"c","function":{
            "name":"search_models","arguments":"{ pas du json"}}]}}]}"#;

        let reply = OPENAI.parse(200, body).unwrap();
        assert_eq!(reply.calls[0].arguments, json!({}));
    }

    #[test]
    fn une_reponse_sans_message_est_une_erreur() {
        assert!(
            OPENAI
                .parse(200, "{}")
                .unwrap_err()
                .contains("sans message")
        );
        assert!(
            OPENAI
                .parse(200, "<html>")
                .unwrap_err()
                .contains("illisible")
        );
    }

    #[test]
    fn l_erreur_du_fournisseur_est_transmise_en_clair() {
        let body =
            r#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error"}}"#;
        let message = OPENAI.error(401, body);

        assert!(message.contains("401"), "{message}");
        assert!(message.contains("Incorrect API key"), "{message}");
        // Sans JSON exploitable, on garde au moins le corps, sur une ligne.
        let message = OPENAI.error(500, "<html>\n  <body>502</body>\n</html>");
        assert!(message.contains("<html>"), "{message}");
        assert!(!message.contains('\n'), "{message}");
    }

    #[test]
    fn la_liste_des_modeles_est_un_get_authentifie() {
        let request = OPENAI.models_request(&cfg());

        assert_eq!(request.method, "GET");
        assert_eq!(request.url, "https://api.openai.com/v1/models");
        assert_eq!(
            request.headers[0],
            ("authorization".to_string(), "Bearer sk-test".to_string())
        );
        // Un `GET` ne porte pas de corps : rien à sérialiser.
        assert_eq!(request.body, serde_json::Value::Null);
    }

    /// La liste d'un fournisseur mêle conversations, embeddings et génération
    /// d'images : on ne garde que ce qui peut tenir un échange.
    #[test]
    fn seuls_les_modeles_de_conversation_sont_proposes() {
        let body = r#"{"data":[{"id":"gpt-4o"},{"id":"gpt-4o"},
            {"id":"text-embedding-3-small"},{"id":"whisper-1"},
            {"id":"dall-e-3"},{"id":"o3-mini"}]}"#;

        let models = OPENAI.parse_models(200, body).unwrap();
        assert_eq!(
            models,
            vec!["gpt-4o", "o3-mini"],
            "doublons ou intrus restants"
        );
    }

    #[test]
    fn une_liste_vide_ou_refusee_est_dite() {
        assert!(
            OPENAI
                .parse_models(200, r#"{"data":[]}"#)
                .unwrap_err()
                .contains("aucun modèle")
        );

        let message = OPENAI
            .parse_models(401, r#"{"error":{"message":"Incorrect API key provided"}}"#)
            .unwrap_err();
        assert!(
            message.contains("401") && message.contains("Incorrect API key"),
            "{message}"
        );
    }
}
