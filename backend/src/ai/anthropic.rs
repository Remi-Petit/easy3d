//! Fournisseur **Anthropic** (`POST /v1/messages`).
//!
//! Deux différences notables avec le format OpenAI, qui justifient un fichier à
//! part : la conversation est faite de **blocs** (`text`, `tool_use`,
//! `tool_result`) et non de messages à plat, et les résultats d'outils se
//! rangent dans un message **utilisateur** — il n'existe pas de rôle `tool`.

use super::Provider;
use super::tools::Spec;
use super::{Reply, Request, Resolved, ToolCall, Turn, error_message, short_body};
use serde_json::{Value, json};

const BASE_URL: &str = "https://api.anthropic.com/v1";
const MODEL: &str = "claude-3-5-haiku-latest";

/// Version de l'API, exigée par Anthropic dans un en-tête dédié.
const API_VERSION: &str = "2023-06-01";

/// Longueur maximale de la réponse. Volontairement courte : le modèle raisonne
/// par appels d'outils, pas par longs paragraphes.
const MAX_TOKENS: u32 = 2048;

/// Le fournisseur Anthropic.
pub static ANTHROPIC: Anthropic = Anthropic;

pub struct Anthropic;

impl Provider for Anthropic {
    fn id(&self) -> &'static str {
        "anthropic"
    }

    fn label(&self) -> &'static str {
        "Anthropic (Claude)"
    }

    fn default_base_url(&self) -> &'static str {
        BASE_URL
    }

    fn default_model(&self) -> &'static str {
        MODEL
    }

    fn build(&self, cfg: &Resolved, system: &str, turns: &[Turn], tools: &[Spec]) -> Request {
        let mut headers = vec![
            ("anthropic-version".to_string(), API_VERSION.to_string()),
            // Anthropic refuse un contenu vide : une chaîne vide suffit à
            // signaler l'absence de clé (Ollama, par exemple, n'en veut pas).
            (
                "x-api-key".to_string(),
                cfg.api_key.clone().unwrap_or_default(),
            ),
        ];
        headers.retain(|(_, value)| !value.is_empty());

        Request {
            url: format!("{}/messages", cfg.base_url),
            headers,
            body: json!({
                "model": cfg.model,
                "max_tokens": MAX_TOKENS,
                "system": system,
                "messages": build_messages(turns),
                "tools": tools.iter().map(|tool| json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.schema,
                })).collect::<Vec<Value>>(),
            }),
        }
    }

    fn parse(&self, _status: u16, body: &str) -> Result<Reply, String> {
        let value: Value = serde_json::from_str(body)
            .map_err(|e| format!("réponse illisible d'Anthropic : {e}"))?;

        if let Some(message) = error_message(body) {
            return Err(message);
        }

        let blocks = value
            .get("content")
            .and_then(Value::as_array)
            .ok_or("réponse d'Anthropic sans contenu.")?;

        let mut text = String::new();
        let mut calls = Vec::new();
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(chunk) = block.get("text").and_then(Value::as_str) {
                        text.push_str(chunk);
                    }
                }
                Some("tool_use") => {
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if name.is_empty() {
                        continue;
                    }
                    calls.push(ToolCall {
                        id: block
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        name,
                        // Ici les arguments arrivent déjà en JSON, pas en chaîne.
                        arguments: block.get("input").cloned().unwrap_or_else(|| json!({})),
                    });
                }
                _ => {}
            }
        }

        Ok(Reply {
            text: if text.trim().is_empty() {
                None
            } else {
                Some(text)
            },
            calls,
        })
    }

    fn error(&self, status: u16, body: &str) -> String {
        match error_message(body) {
            Some(message) => format!("Anthropic a répondu {status} : {message}"),
            None => format!("Anthropic a répondu {status} : {}", short_body(body)),
        }
    }
}

/// Conversation au format Anthropic : les blocs d'un tour sont regroupés, et les
/// résultats d'outils rejoignent le message **utilisateur** qui suit l'appel.
fn build_messages(turns: &[Turn]) -> Vec<Value> {
    let mut messages: Vec<Value> = Vec::new();

    for turn in turns {
        match turn {
            Turn::User(text) => messages.push(json!({
                "role": "user",
                "content": [{"type": "text", "text": text}],
            })),
            Turn::Assistant(reply) => {
                let mut blocks: Vec<Value> = Vec::new();
                if let Some(text) = &reply.text {
                    blocks.push(json!({"type": "text", "text": text}));
                }
                for call in &reply.calls {
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": call.id,
                        "name": call.name,
                        "input": call.arguments,
                    }));
                }
                if !blocks.is_empty() {
                    messages.push(json!({"role": "assistant", "content": blocks}));
                }
            }
            Turn::Tool { id, content, .. } => {
                let block = json!({
                    "type": "tool_result",
                    "tool_use_id": id,
                    "content": content,
                });
                // Plusieurs outils appelés d'affilée : un seul message
                // utilisateur les porte tous, comme le veut l'API.
                match messages.last_mut() {
                    Some(last)
                        if last["role"] == "user"
                            && last["content"][0]["type"] == "tool_result" =>
                    {
                        last["content"].as_array_mut().unwrap().push(block);
                    }
                    _ => messages.push(json!({"role": "user", "content": [block]})),
                }
            }
        }
    }

    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Resolved {
        Resolved {
            provider: "anthropic",
            base_url: BASE_URL.to_string(),
            model: MODEL.to_string(),
            api_key: Some("sk-ant-test".to_string()),
        }
    }

    fn turn_avec_appel() -> Vec<Turn> {
        vec![
            Turn::User("la pièce en PETG".to_string()),
            Turn::Assistant(Reply {
                text: Some("Je cherche.".to_string()),
                calls: vec![ToolCall {
                    id: "toolu_1".to_string(),
                    name: "search_models".to_string(),
                    arguments: json!({"query": "PETG"}),
                }],
            }),
            Turn::Tool {
                id: "toolu_1".to_string(),
                name: "search_models".to_string(),
                content: "{\"results\":[]}".to_string(),
            },
        ]
    }

    #[test]
    fn la_requete_utilise_les_blocs_et_les_entetes_anthropic() {
        let request = ANTHROPIC.build(
            &cfg(),
            "tu cherches",
            &turn_avec_appel(),
            &super::super::tools::specs(),
        );

        assert_eq!(request.url, "https://api.anthropic.com/v1/messages");
        assert!(
            request
                .headers
                .contains(&("anthropic-version".to_string(), API_VERSION.to_string()))
        );
        assert!(
            request
                .headers
                .contains(&("x-api-key".to_string(), "sk-ant-test".to_string()))
        );

        let body = &request.body;
        assert_eq!(body["model"], MODEL);
        assert_eq!(body["max_tokens"], MAX_TOKENS);
        assert_eq!(body["system"], "tu cherches");

        // La consigne système n'est **pas** un message : elle a son champ.
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");

        let assistant = &body["messages"][1];
        assert_eq!(assistant["content"][0]["type"], "text");
        assert_eq!(assistant["content"][1]["type"], "tool_use");
        assert_eq!(assistant["content"][1]["id"], "toolu_1");
        assert_eq!(assistant["content"][1]["input"]["query"], "PETG");

        // Le résultat d'outil est un bloc **utilisateur**.
        let result = &body["messages"][2];
        assert_eq!(result["role"], "user");
        assert_eq!(result["content"][0]["type"], "tool_result");
        assert_eq!(result["content"][0]["tool_use_id"], "toolu_1");

        assert_eq!(body["tools"][0]["name"], "catalog_overview");
        assert!(body["tools"][0]["input_schema"]["type"].is_string());
    }

    #[test]
    fn deux_resultats_d_outils_partagent_un_message() {
        let turns = vec![
            Turn::User("cherche".to_string()),
            Turn::Assistant(Reply {
                text: None,
                calls: vec![
                    ToolCall {
                        id: "a".to_string(),
                        name: "catalog_overview".to_string(),
                        arguments: json!({}),
                    },
                    ToolCall {
                        id: "b".to_string(),
                        name: "search_models".to_string(),
                        arguments: json!({"query": "x"}),
                    },
                ],
            }),
            Turn::Tool {
                id: "a".to_string(),
                name: "catalog_overview".to_string(),
                content: "{}".to_string(),
            },
            Turn::Tool {
                id: "b".to_string(),
                name: "search_models".to_string(),
                content: "{}".to_string(),
            },
        ];

        let messages = build_messages(&turns);
        assert_eq!(messages.len(), 3, "{messages:#?}");
        assert_eq!(messages[2]["content"].as_array().unwrap().len(), 2);
        assert_eq!(messages[2]["content"][1]["tool_use_id"], "b");
    }

    #[test]
    fn sans_cle_aucun_entete_x_api_key() {
        let mut cfg = cfg();
        cfg.api_key = None;
        let request = ANTHROPIC.build(&cfg, "s", &[Turn::User("a".to_string())], &[]);

        assert!(!request.headers.iter().any(|(name, _)| name == "x-api-key"));
        assert_eq!(request.headers.len(), 1);
    }

    #[test]
    fn la_reponse_en_blocs_est_lue() {
        let body = r#"{"content":[
            {"type":"text","text":"Cherchons."},
            {"type":"tool_use","id":"toolu_2","name":"search_models","input":{"query":"roue"}}],
            "stop_reason":"tool_use"}"#;

        let reply = ANTHROPIC.parse(200, body).unwrap();
        assert_eq!(reply.text.as_deref(), Some("Cherchons."));
        assert_eq!(reply.calls.len(), 1);
        assert_eq!(reply.calls[0].id, "toolu_2");
        assert_eq!(reply.calls[0].arguments["query"], "roue");
    }

    #[test]
    fn un_bloc_de_texte_seul_ne_produit_aucun_appel() {
        let body = r#"{"content":[{"type":"text","text":"Rien trouvé."}]}"#;
        let reply = ANTHROPIC.parse(200, body).unwrap();

        assert!(reply.calls.is_empty());
        assert_eq!(reply.text.as_deref(), Some("Rien trouvé."));
        // Un texte vide ne remonte pas comme réponse.
        let reply = ANTHROPIC
            .parse(200, r#"{"content":[{"type":"text","text":"  "}]}"#)
            .unwrap();
        assert!(reply.text.is_none());
    }

    #[test]
    fn une_reponse_sans_contenu_est_une_erreur() {
        assert!(
            ANTHROPIC
                .parse(200, "{}")
                .unwrap_err()
                .contains("sans contenu")
        );
        assert!(
            ANTHROPIC
                .parse(200, "pas du json")
                .unwrap_err()
                .contains("illisible")
        );
    }

    #[test]
    fn l_erreur_est_transmise_en_clair() {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        let message = ANTHROPIC.error(401, body);

        assert!(message.contains("Anthropic"), "{message}");
        assert!(message.contains("invalid x-api-key"), "{message}");
    }
}
