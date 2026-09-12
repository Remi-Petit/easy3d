//! Test d'intégration du **transport MCP** : les outils sont testés dans
//! `src/mcp.rs`, ici on vérifie ce que voit un vrai client — le Router axum
//! (montage de `/mcp`), le handshake `initialize`, l'en-tête de session, le
//! format des réponses (SSE) et les schémas d'outils publiés.
//!
//! Passer par le `Router` (et non par les méthodes du serveur) est le but :
//! c'est la seule façon de détecter une régression de plomberie — mauvais
//! montage, réponse non conforme, outil qui disparaît de `tools/list`.

use axum::Router;
use axum::body::Body;
use axum::http::header::{ACCEPT, CONTENT_TYPE, HOST};
use axum::http::{HeaderMap, Request, StatusCode};
use easy3d::api::{AppState, routes};
use easy3d::config::Config;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::time::Duration;
use tower::ServiceExt;

/// Catalogue jetable : un fichier, une configuration qui le désigne.
fn test_app() -> (tempfile::TempDir, Router) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("piece.stl"), "solid x").unwrap();

    let config = Config {
        models_root: Some(dir.path().display().to_string()),
        ..Config::default()
    };
    let (ws, _) = tokio::sync::broadcast::channel::<String>(16);
    // `config_path` reste dans le dossier temporaire : aucun outil de ce test
    // n'y touche, mais un futur appel ne doit pas écrire dans le dépôt.
    let state = AppState::new(dir.path().to_path_buf(), ws, config)
        .with_config_path(dir.path().join("config.yml"));

    (dir, routes(state))
}

/// Requête JSON-RPC vers `/mcp`.
///
/// L'URI est en **forme absolue** et porte un `Host` : rmcp valide l'en-tête
/// (protection contre le DNS rebinding) et retombe sur l'autorité de l'URI
/// quand un intermédiaire l'a retiré — comme le fait un vrai client HTTP.
async fn send(
    app: &Router,
    message: Value,
    session: Option<&str>,
) -> (StatusCode, HeaderMap, String) {
    let mut builder = Request::builder()
        .method("POST")
        .uri("http://127.0.0.1/mcp")
        .header(HOST, "127.0.0.1")
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/event-stream");
    if let Some(session) = session {
        builder = builder.header("mcp-session-id", session);
    }

    let response = app
        .clone()
        .oneshot(builder.body(Body::from(message.to_string())).unwrap())
        .await
        .unwrap();

    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        headers,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

/// Corps d'une réponse MCP : JSON brut, ou SSE (`data: {...}`).
fn payload(body: &str) -> Value {
    body.lines()
        .filter_map(|line| line.trim().strip_prefix("data:"))
        .filter_map(|json| serde_json::from_str::<Value>(json.trim()).ok())
        .next_back()
        .unwrap_or_else(|| serde_json::from_str(body).expect("réponse JSON-RPC illisible"))
}

/// Ouvre une session et renvoie son identifiant.
async fn handshake(app: &Router) -> String {
    let (status, headers, body) = send(
        app,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "easy3d-tests", "version": "0" }
            }
        }),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "initialize : {body}");
    let session = headers
        .get("mcp-session-id")
        .expect("initialize ouvre une session")
        .to_str()
        .unwrap()
        .to_string();

    let result = payload(&body)["result"].clone();
    assert_eq!(result["serverInfo"]["name"], "easy3d");
    // Sans la capacité `tools`, un client n'exposerait aucun outil.
    assert!(result["capabilities"]["tools"].is_object());

    session
}

/// Appelle un outil et renvoie son texte de réponse.
async fn call_tool(app: &Router, session: &str, id: u32, tool: &str, arguments: Value) -> String {
    let (status, _, body) = send(
        app,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": tool, "arguments": arguments }
        }),
        Some(session),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{tool} : {body}");

    let result = payload(&body)["result"].clone();
    assert_ne!(
        result["isError"],
        json!(true),
        "{tool} en erreur : {result}"
    );
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("{tool} : réponse sans texte ({result})"))
        .to_string()
}

#[tokio::test]
async fn handshake_liste_et_appels_d_outils() {
    let (dir, app) = test_app();
    let session = handshake(&app).await;

    // Notification d'initialisation : acquittée, sans corps de réponse.
    let (status, _, _) = send(
        &app,
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        Some(&session),
    )
    .await;
    assert!(status.is_success(), "notifications/initialized : {status}");

    // tools/list : les outils annoncés portent bien un schéma d'entrée.
    let (status, _, body) = send(
        &app,
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let listed = payload(&body);
    let tools = listed["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list sans tableau d'outils : {listed}"));
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    for expected in [
        "list_formats",
        "list_models",
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
        assert!(
            names.contains(&expected),
            "outil absent de tools/list : {expected}"
        );
    }

    let list_models = tools.iter().find(|t| t["name"] == "list_models").unwrap();
    assert!(list_models["description"].is_string());
    let properties = &list_models["inputSchema"]["properties"];
    for argument in ["query", "folder", "ext", "limit"] {
        assert!(
            properties[argument].is_object(),
            "argument non annoncé dans le schéma : {argument}"
        );
    }

    // tools/call en lecture.
    let models: Value = serde_json::from_str(
        &call_tool(&app, &session, 3, "list_models", json!({ "ext": "stl" })).await,
    )
    .unwrap();
    assert_eq!(models["count"], 1);
    assert_eq!(models["models"][0]["rel"], "piece.stl");
    assert_eq!(models["models"][0]["viewer"], "mesh");

    // tools/call en écriture : la note passe par le CRDT…
    let created: Value = serde_json::from_str(
        &call_tool(
            &app,
            &session,
            4,
            "create_note",
            json!({ "rel": "piece.stl", "content": "# Transport" }),
        )
        .await,
    )
    .unwrap();
    assert_eq!(created["exists"], true);

    // …et se relit aussitôt par le même transport.
    let read: Value = serde_json::from_str(
        &call_tool(
            &app,
            &session,
            5,
            "read_note",
            json!({ "rel": "piece.stl" }),
        )
        .await,
    )
    .unwrap();
    assert_eq!(read["content"], "# Transport");

    // …puis arrive sur disque au flush suivant (250 ms).
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        easy3d::notes::read(dir.path(), "piece.stl").as_deref(),
        Some("# Transport")
    );

    // Une erreur d'outil remonte en résultat, pas en échec de transport.
    let (status, _, body) = send(
        &app,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": { "name": "read_note", "arguments": { "rel": "inconnu.stl" } }
        }),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let error = payload(&body);
    assert!(
        error["error"].is_object() || error["result"]["isError"] == json!(true),
        "un élément inconnu doit remonter une erreur : {error}"
    );
}

#[tokio::test]
async fn les_outils_destructeurs_sont_desactives_par_defaut() {
    let (_dir, app) = test_app();
    let session = handshake(&app).await;

    // Sans `EASY3D_MCP_ALLOW_WRITE`, le transport expose les outils mais refuse
    // de les exécuter : c'est le serveur (et non le client) qui tranche.
    let (status, _, body) = send(
        &app,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": "delete_note", "arguments": { "rel": "piece.stl" } }
        }),
        Some(&session),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let result = payload(&body);
    let refused = result["error"].is_object() || result["result"]["isError"] == json!(true);
    assert!(refused, "delete_note devrait être refusé : {result}");
}

#[tokio::test]
async fn une_requete_sans_session_est_rejetee() {
    let (_dir, app) = test_app();

    // En mode session (par défaut), `tools/list` exige l'en-tête de session :
    // sans lui, le serveur ne doit pas répondre normalement.
    let (status, _, _) = send(
        &app,
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} }),
        None,
    )
    .await;
    assert!(
        !status.is_success(),
        "requête sans session acceptée : {status}"
    );
}
