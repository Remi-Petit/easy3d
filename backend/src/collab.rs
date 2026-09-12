//! Édition collaborative des notes (CRDT Yjs, côté serveur).
//!
//! Chaque note est un **document CRDT** identifié par le chemin relatif de
//! l'élément (`DemaAuto`, `DemaAuto/boitier.stl`, …) — exactement la clé
//! utilisée par [`crate::notes`] pour le fichier Markdown.
//!
//! Les clients parlent le protocole de synchronisation standard de l'écosystème
//! Yjs (`y-protocols`, celui de `y-websocket`), implémenté ici par [`yrs`] :
//!
//! - `SyncStep1` / `SyncStep2` / `Update` pour le document,
//! - `Awareness` pour les curseurs et pseudos des autres participants.
//!
//! Le serveur tient donc un document autoritaire par note ouverte, ce qui
//! permet à un nouvel arrivant de récupérer l'état complet, et il ne fait que
//! **relayer** les trames entre participants : la fusion des modifications
//! concurrentes est faite par le CRDT lui-même, sans arbitrage central.
//!
//! La persistance reste un fichier Markdown dans `.easy3d-notes/`, réécrit de
//! façon différée après chaque modification — c'est lui que voit le scanner,
//! et donc la pastille 📝 des cartes.

use crate::api::AppState;
use crate::notes;
use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;
use yrs::encoding::read::Cursor;
use yrs::sync::{
    Awareness, DefaultProtocol, Message as YMessage, MessageReader, Protocol, SyncMessage as YSync,
};
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::{Doc, GetString, ReadTxn, StateVector, Text, Transact, Update};

/// Nom du type racine partagé. Doit être identique côté client
/// (`ydoc.getText('markdown')`).
pub const TEXT_KEY: &str = "markdown";

/// Regroupement avant écriture du `.md` sur disque.
const FLUSH_DELAY: Duration = Duration::from_millis(250);

/// Profondeur du tampon de diffusion d'une note.
const CHANNEL_CAPACITY: usize = 256;

/// Registre des documents collaboratifs ouverts, une entrée par note.
///
/// Les documents restent ouverts après le départ du dernier participant : ils
/// ne pèsent que quelques kilo-octets et cela évite de perdre les dernières
/// modifications avant leur écriture sur disque.
#[derive(Default)]
pub struct Rooms {
    map: Mutex<HashMap<String, Arc<Room>>>,
}

impl Rooms {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ouvre (ou rejoint) le document partagé d'une note.
    ///
    /// `rel` doit désigner un élément **existant** du catalogue : on ne crée pas
    /// de document pour une chaîne arbitraire. Renvoie `None` sinon.
    pub fn join(state: &AppState, rel: &str) -> Option<(Arc<Room>, broadcast::Receiver<Vec<u8>>)> {
        let root = state.root();
        // Même validation que pour le fichier de note, puis existence exigée.
        notes::note_path(&root, rel)?;
        if !root.join(rel).exists() {
            return None;
        }

        let mut map = state.collab.map.lock().unwrap();
        let room = match map.get(rel) {
            Some(room) => room.clone(),
            None => {
                let room = Arc::new(Room::load(&root, rel));
                map.insert(rel.to_string(), room.clone());
                // L'abonnement doit précéder l'éventuelle décision de fermeture
                // du document : ici, on ouvre, donc pas de course.
                spawn_flush(room.clone(), state.clone(), rel.to_string());
                room
            }
        };
        let rx = room.tx.subscribe();
        Some((room, rx))
    }
}

/// Un document partagé et son canal de diffusion.
pub struct Room {
    /// Document CRDT + informations de présence (curseurs, pseudos).
    awareness: Mutex<Awareness>,
    /// Diffuse aux connexions d'une même note les trames à relayer.
    tx: broadcast::Sender<Vec<u8>>,
    /// Dernier contenu écrit sur disque (évite les écritures inutiles).
    last: Mutex<String>,
    /// Une modification attend d'être écrite.
    dirty: AtomicBool,
}

impl Room {
    /// Ouvre le document d'une note.
    ///
    /// Priorité à l'**état CRDT** persisté (`.easy3d-notes/<rel>.ydoc`) : c'est
    /// la même histoire Yjs que celle détenue par les clients, donc une
    /// reconnexion après redémarrage fusionne sans rien dupliquer. À défaut
    /// (première ouverture, état illisible), on amorce un document neuf depuis
    /// le Markdown — et on persiste aussitôt cet état.
    fn load(root: &Path, rel: &str) -> Self {
        let content = notes::read(root, rel).unwrap_or_default();
        let doc = Doc::new();

        let loaded = notes::read_state(root, rel)
            .and_then(|bytes| Update::decode_v1(&bytes).ok())
            .map(|update| doc.transact_mut().apply_update(update).is_ok())
            .unwrap_or(false);

        if !loaded {
            let text = doc.get_or_insert_text(TEXT_KEY);
            let mut txn = doc.transact_mut();
            text.push(&mut txn, &content);
        }

        let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        let room = Room {
            awareness: Mutex::new(Awareness::new(doc)),
            tx,
            last: Mutex::new(content),
            dirty: AtomicBool::new(false),
        };

        // Persiste l'état dès l'ouverture : c'est lui qui rend les redémarrages
        // suivants idempotents (mêmes identifiants d'insertion pour les clients).
        let _ = notes::write_state(root, rel, &room.encode_state());

        room
    }

    /// État binaire complet du document CRDT (pour la persistance).
    fn encode_state(&self) -> Vec<u8> {
        let awareness = self.awareness.lock().unwrap();
        let doc = awareness.doc();
        doc.transact()
            .encode_state_as_update_v1(&StateVector::default())
    }

    /// Contenu courant du document partagé.
    pub fn text(&self) -> String {
        let awareness = self.awareness.lock().unwrap();
        let doc = awareness.doc();
        // Attention à l'ordre : `get_or_insert_text` réclame l'accès exclusif au
        // document quand il doit créer le type racine. L'appeler après
        // `transact()` le ferait attendre sa propre transaction de lecture.
        let text = doc.get_or_insert_text(TEXT_KEY);
        let txn = doc.transact();
        text.get_string(&txn)
    }

    /// Marque le document comme modifié (écriture différée sur disque).
    fn touch(&self) {
        self.dirty.store(true, Ordering::SeqCst);
    }
}

/// Tâche périodique qui écrit la note sur disque après modification.
fn spawn_flush(room: Arc<Room>, state: AppState, rel: String) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(FLUSH_DELAY);
        loop {
            ticker.tick().await;
            if !room.dirty.swap(false, Ordering::SeqCst) {
                continue;
            }
            if !flush(&room, &state, &rel) {
                // Écriture impossible (disque, droits) : on retentera au tour
                // suivant plutôt que de perdre silencieusement la note.
                room.touch();
            }
        }
    });
}

/// Écrit la note sur disque si son contenu a changé.
///
/// Renvoie `false` si le fichier n'a pas pu être écrit. Une note vide **supprime**
/// le `.md` (voir [`notes::write`]) ; quand une note apparaît ou disparaît, la
/// liste des modèles est rediffusée pour que la pastille 📝 suive partout.
fn flush(room: &Room, state: &AppState, rel: &str) -> bool {
    let text = room.text();
    let mut last = room.last.lock().unwrap();
    if *last == text {
        return true;
    }

    let root = state.root();
    if notes::write(&root, rel, &text).is_err() {
        return false;
    }

    // L'état CRDT suit le Markdown : c'est lui qui permet de repartir sur le
    // même document au prochain démarrage. Une note vidée est effacée (les
    // deux fichiers), sinon elle ressusciterait depuis ce document.
    if text.trim().is_empty() {
        notes::remove_state(&root, rel);
    } else if notes::write_state(&root, rel, &room.encode_state()).is_err() {
        return false;
    }

    let presence_changed = last.trim().is_empty() != text.trim().is_empty();
    *last = text;
    drop(last);

    if presence_changed {
        crate::api::broadcast_snapshot(state);
    }
    true
}

/// Sert une connexion `/collab/{rel}` : une note = un document partagé.
pub async fn handle_socket(socket: WebSocket, state: AppState, rel: String) {
    let Some((room, mut rx)) = Rooms::join(&state, &rel) else {
        // Chemin invalide ou élément inexistant : on ferme sans rien dire.
        return;
    };

    let (mut sink, mut stream) = socket.split();

    for frame in handshake(&room) {
        if sink.send(WsMessage::Binary(frame.into())).await.is_err() {
            return;
        }
    }

    loop {
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(WsMessage::Binary(data))) => {
                    let handled = handle_frame(&room, &data);
                    room.touch();
                    for frame in handled.replies {
                        if sink.send(WsMessage::Binary(frame.into())).await.is_err() {
                            return;
                        }
                    }
                    // Relayé aux autres participants de la même note ; l'émetteur
                    // le reçoit aussi, ce qui est sans effet (un update CRDT est
                    // idempotent, et chacun ignore sa propre présence).
                    for frame in handled.relay {
                        let _ = room.tx.send(frame);
                    }
                }
                Some(Ok(WsMessage::Close(_))) | None => break,
                Some(Err(_)) => break,
                // Texte, ping, pong : sans effet sur le protocole.
                Some(Ok(_)) => {}
            },
            broadcast = rx.recv() => match broadcast {
                Ok(frame) => {
                    if sink.send(WsMessage::Binary(frame.into())).await.is_err() {
                        break;
                    }
                }
                // Participant en retard : on relance un cycle de synchronisation
                // complet plutôt que de perdre des modifications.
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    for frame in handshake(&room) {
                        if sink.send(WsMessage::Binary(frame.into())).await.is_err() {
                            break;
                        }
                    }
                }
                Err(_) => break,
            },
        }
    }
}

/// Messages envoyés à une nouvelle connexion : l'état du document
/// (`SyncStep1`, auquel le client répond par ce qui lui manque) puis la
/// présence connue.
///
/// **Un message par trame** : le client `y-websocket` ne décode qu'un message
/// par trame WebSocket.
fn handshake(room: &Room) -> Vec<Vec<u8>> {
    let awareness = room.awareness.lock().unwrap();
    let doc = awareness.doc();
    let sv: StateVector = doc.transact().state_vector();

    let mut frames = vec![encode(&YMessage::Sync(YSync::SyncStep1(sv)))];
    if let Ok(update) = awareness.update() {
        frames.push(encode(&YMessage::Awareness(update)));
    }
    frames
}

/// Ce qu'il faut faire d'une trame reçue d'un client.
#[derive(Default)]
struct Handled {
    /// À renvoyer à l'émetteur (réponses du protocole).
    replies: Vec<Vec<u8>>,
    /// À relayer aux autres participants de la même note.
    relay: Vec<Vec<u8>>,
}

/// Applique une trame reçue d'un client au document partagé.
fn handle_frame(room: &Room, data: &[u8]) -> Handled {
    let mut awareness = room.awareness.lock().unwrap();
    let (replies, relay) = apply_frame(&mut awareness, data);
    Handled { replies, relay }
}

/// Applique une trame du protocole de synchronisation à un document.
///
/// Renvoie les réponses destinées à l'émetteur, et les messages qui intéressent
/// les **autres** participants (modifications du document et présence).
///
/// Une trame peut contenir plusieurs messages ; un message illisible interrompt
/// le décodage, le reste n'étant pas exploitable.
fn apply_frame(awareness: &mut Awareness, data: &[u8]) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut decoder = DecoderV1::new(Cursor::new(data));
    let mut reader = MessageReader::new(&mut decoder);
    let mut replies = Vec::new();
    let mut relay = Vec::new();

    while let Some(result) = reader.next() {
        let Ok(message) = result else { break };

        // Seules les modifications du document et la présence intéressent les
        // autres participants ; les questions de synchronisation (SyncStep1,
        // AwarenessQuery) ne concernent que l'émetteur.
        let forwarded = matches!(
            message,
            YMessage::Sync(YSync::SyncStep2(_))
                | YMessage::Sync(YSync::Update(_))
                | YMessage::Awareness(_)
        )
        .then(|| encode(&message));

        match DefaultProtocol.handle_message(awareness, message) {
            Ok(Some(reply)) => replies.push(encode(&reply)),
            Ok(None) => {}
            // Message propriétaire d'un autre fournisseur : ignoré.
            Err(_) => {}
        }

        if let Some(frame) = forwarded {
            relay.push(frame);
        }
    }

    (replies, relay)
}

/// Encode un message du protocole (lib0 v1, comme `y-protocols`).
fn encode(message: &YMessage) -> Vec<u8> {
    let mut encoder = EncoderV1::new();
    message.encode(&mut encoder);
    encoder.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use std::net::SocketAddr;
    use tokio_tungstenite::MaybeTlsStream;
    use tokio_tungstenite::WebSocketStream;

    /// Un participant, exactement comme `y-websocket` dans le navigateur.
    struct Peer {
        awareness: Awareness,
    }

    impl Peer {
        fn new(client_id: u64) -> Self {
            Peer {
                awareness: Awareness::new(Doc::with_client_id(client_id)),
            }
        }

        fn doc(&self) -> &Doc {
            self.awareness.doc()
        }

        fn text(&self) -> String {
            let doc = self.doc();
            // Créer le type **avant** d'ouvrir la transaction de lecture :
            // `get_or_insert_text` a besoin de l'accès exclusif au document.
            let text = doc.get_or_insert_text(TEXT_KEY);
            let txn = doc.transact();
            text.get_string(&txn)
        }

        fn insert(&self, index: u32, chunk: &str) {
            let doc = self.doc();
            let text = doc.get_or_insert_text(TEXT_KEY);
            let mut txn = doc.transact_mut();
            text.insert(&mut txn, index, chunk);
        }

        /// Traite une trame reçue du serveur et renvoie ses réponses.
        fn receive(&mut self, frame: &[u8]) -> Vec<Vec<u8>> {
            apply_frame(&mut self.awareness, frame).0
        }

        /// Demande de synchronisation envoyée à la connexion.
        fn sync_step1(&self) -> Vec<u8> {
            let sv = self.doc().transact().state_vector();
            encode(&YMessage::Sync(YSync::SyncStep1(sv)))
        }

        /// État complet du document local, à envoyer au serveur.
        fn update(&self) -> Vec<u8> {
            let update = self
                .doc()
                .transact()
                .encode_state_as_update_v1(&StateVector::default());
            encode(&YMessage::Sync(YSync::Update(update)))
        }
    }

    fn state(root: &Path) -> AppState {
        let (ws, _) = broadcast::channel::<String>(16);
        AppState::new(root.to_path_buf(), ws, crate::config::Config::default())
    }

    /// Prépare un catalogue jetable contenant `sub/model.stl`.
    fn catalogue() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/model.stl"), b"solid x").unwrap();
        dir
    }

    /// Fait dialoguer un pair avec le serveur pour une trame donnée.
    ///
    /// Renvoie les trames que le serveur diffuse aux **autres** pairs.
    fn exchange(room: &Room, peer: &mut Peer, frame: Vec<u8>) -> Vec<Vec<u8>> {
        let mut to_server = vec![frame];
        let mut to_others = Vec::new();
        let mut tours = 0;

        while let Some(frame) = to_server.pop() {
            tours += 1;
            assert!(tours < 100, "la synchronisation ne converge pas");

            let handled = handle_frame(room, &frame);
            // Réponses du serveur : le pair les traite et peut enchaîner.
            for reply in handled.replies {
                to_server.extend(peer.receive(&reply));
            }
            // Diffusion : le pair se reçoit lui-même (sans effet) et les autres
            // participants reçoivent la modification.
            for relayed in handled.relay {
                to_others.push(relayed.clone());
                to_server.extend(peer.receive(&relayed));
            }
        }

        to_others
    }

    /// Synchronise un pair avec la room (ce que fait `y-websocket` à l'ouverture).
    fn connect(room: &Room, peer: &mut Peer) {
        let frame = peer.sync_step1();
        exchange(room, peer, frame);
    }

    #[tokio::test]
    async fn un_pair_ecrit_dans_la_note() {
        let dir = catalogue();
        let state = state(dir.path());
        let (room, _rx) = Rooms::join(&state, "sub/model.stl").expect("room ouverte");

        let mut peer = Peer::new(1);
        connect(&room, &mut peer);
        assert_eq!(room.text(), "");

        peer.insert(0, "bonjour");
        let frame = peer.update();
        exchange(&room, &mut peer, frame);

        assert_eq!(room.text(), "bonjour");
        assert_eq!(peer.text(), "bonjour");

        // Le `.md` n'est pas encore écrit : c'est la tâche différée qui s'en charge.
        assert!(
            !dir.path()
                .join(notes::NOTES_DIR)
                .join("sub/model.stl.md")
                .exists()
        );

        assert!(flush(&room, &state, "sub/model.stl"));
        let ecrit =
            std::fs::read_to_string(dir.path().join(notes::NOTES_DIR).join("sub/model.stl.md"))
                .unwrap();
        assert_eq!(ecrit, "bonjour");
    }

    #[tokio::test]
    async fn deux_pairs_concurrents_ne_s_ecrasent_pas() {
        let dir = catalogue();
        let state = state(dir.path());
        let (room, _rx) = Rooms::join(&state, "sub/model.stl").expect("room ouverte");

        let mut a = Peer::new(1);
        let mut b = Peer::new(2);
        connect(&room, &mut a);
        connect(&room, &mut b);

        // Les deux écrivent **sans se synchronized d'abord** : le premier au
        // début, le second à la fin. Aucun des deux ne doit être perdu.
        a.insert(0, "AAA");
        let frame_a = a.update();
        let pour_b = exchange(&room, &mut a, frame_a);
        b.insert(b.text().len() as u32, "ZZZ");
        let frame_b = b.update();
        let pour_a = exchange(&room, &mut b, frame_b);

        for frame in pour_b {
            b.receive(&frame);
        }
        for frame in pour_a {
            a.receive(&frame);
        }

        assert_eq!(a.text(), b.text(), "les deux pairs doivent converger");
        assert!(a.text().contains("AAA"), "AAA perdu : {}", a.text());
        assert!(a.text().contains("ZZZ"), "ZZZ perdu : {}", a.text());
        assert_eq!(
            a.text().chars().count(),
            6,
            "rien d'autre ne doit apparaître"
        );
    }

    #[tokio::test]
    async fn une_note_partagee_est_relue_depuis_le_disque() {
        let dir = catalogue();
        std::fs::create_dir_all(dir.path().join(notes::NOTES_DIR).join("sub")).unwrap();
        std::fs::write(
            dir.path().join(notes::NOTES_DIR).join("sub/model.stl.md"),
            "# Déjà là",
        )
        .unwrap();

        let state = state(dir.path());
        let (room, _rx) = Rooms::join(&state, "sub/model.stl").expect("room ouverte");
        assert_eq!(room.text(), "# Déjà là");
    }

    #[tokio::test]
    async fn un_chemin_hors_catalogue_est_refuse() {
        let dir = catalogue();
        let state = state(dir.path());

        // Traversée, chemin absolu, ou fichier inexistant : pas de document.
        assert!(Rooms::join(&state, "../secret").is_none());
        assert!(Rooms::join(&state, "sub/../sub/model.stl").is_none());
        assert!(Rooms::join(&state, "C:/Windows/system32").is_none());
        assert!(Rooms::join(&state, "sub/inexistant.stl").is_none());
    }

    /// La note apparaît aussi **sans** client : c'est bien la persistance qui
    /// alimente le scanner (et donc la pastille 📝 des cartes).
    #[tokio::test]
    async fn la_note_est_ecrite_automatiquement() {
        let dir = catalogue();
        let state = state(dir.path());
        let (room, _rx) = Rooms::join(&state, "sub/model.stl").expect("room ouverte");

        let mut peer = Peer::new(1);
        connect(&room, &mut peer);
        peer.insert(0, "salut");
        let frame = peer.update();
        exchange(&room, &mut peer, frame);
        room.touch();

        let note = dir.path().join(notes::NOTES_DIR).join("sub/model.stl.md");
        let mut ecrit = false;
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if std::fs::read_to_string(&note).is_ok_and(|c| c == "salut") {
                ecrit = true;
                break;
            }
        }
        assert!(ecrit, "la note n'a pas été écrite automatiquement");
    }

    /// Un document entièrement vidé doit supprimer le `.md`.
    #[tokio::test]
    async fn vider_la_note_supprime_le_fichier() {
        let dir = catalogue();
        let note = dir.path().join(notes::NOTES_DIR).join("sub/model.stl.md");
        std::fs::create_dir_all(note.parent().unwrap()).unwrap();
        std::fs::write(&note, "à effacer").unwrap();

        let state = state(dir.path());
        let (room, _rx) = Rooms::join(&state, "sub/model.stl").expect("room ouverte");

        let mut peer = Peer::new(1);
        connect(&room, &mut peer);
        let doc = peer.doc();
        let text = doc.get_or_insert_text(TEXT_KEY);
        let mut txn = doc.transact_mut();
        let len = text.len(&txn);
        text.remove_range(&mut txn, 0, len);
        drop(txn);

        let frame = peer.update();
        exchange(&room, &mut peer, frame);
        assert_eq!(room.text(), "");
        assert!(flush(&room, &state, "sub/model.stl"));
        assert!(!note.exists(), "un contenu vide doit supprimer le fichier");
    }

    type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

    /// Démarre le serveur avec la seule route collaborative.
    async fn serveur() -> (SocketAddr, tempfile::TempDir, AppState) {
        let dir = catalogue();
        let state = state(dir.path());
        let app = crate::api::routes(state.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        (addr, dir, state)
    }

    async fn ouvrir(addr: SocketAddr, chemin: &str, client_id: u64) -> (Socket, Peer) {
        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("ws://{addr}/collab/{chemin}"))
                .await
                .unwrap();
        let mut peer = Peer::new(client_id);

        // Le serveur envoie son état, puis la présence connue.
        assert!(lire(&mut socket, &mut peer).await, "handshake incomplet");
        assert!(lire(&mut socket, &mut peer).await, "présence absente");

        // Puis on demande l'état complet, comme le fait `y-websocket`.
        envoyer(&mut socket, peer.sync_step1()).await;
        assert!(lire(&mut socket, &mut peer).await, "état du document");

        (socket, peer)
    }

    /// Lit une trame du serveur et l'applique au pair, qui répond comme le
    /// ferait `y-websocket` (un `SyncStep1` du serveur appelle un `SyncStep2`
    /// en retour). `false` si la connexion est fermée.
    async fn lire(socket: &mut Socket, peer: &mut Peer) -> bool {
        match tokio::time::timeout(Duration::from_secs(5), socket.next()).await {
            Ok(Some(Ok(frame))) => {
                let replies = peer.receive(&frame.into_data());
                for reply in replies {
                    if socket
                        .send(tokio_tungstenite::tungstenite::Message::Binary(
                            reply.into(),
                        ))
                        .await
                        .is_err()
                    {
                        return false;
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// Lit les trames reçues pendant `duree` (borné, jamais bloquant).
    async fn drainer(socket: &mut Socket, peer: &mut Peer, duree: Duration) {
        let echeance = tokio::time::Instant::now() + duree;
        while let Ok(Some(Ok(frame))) = tokio::time::timeout_at(echeance, socket.next()).await {
            let replies = peer.receive(&frame.into_data());
            for reply in replies {
                let _ = socket
                    .send(tokio_tungstenite::tungstenite::Message::Binary(
                        reply.into(),
                    ))
                    .await;
            }
        }
    }

    async fn envoyer(socket: &mut Socket, frame: Vec<u8>) {
        socket
            .send(tokio_tungstenite::tungstenite::Message::Binary(
                frame.into(),
            ))
            .await
            .unwrap();
    }

    /// Intégration : deux navigateurs sur la même note, via le vrai WebSocket.
    #[tokio::test]
    async fn deux_clients_synchronisent_la_note_par_websocket() {
        let (addr, dir, _state) = serveur().await;

        // Le second client utilise le chemin encodé (comme `y-websocket`).
        let (mut ws_a, mut a) = ouvrir(addr, "sub%2Fmodel.stl", 11).await;
        let (mut ws_b, mut b) = ouvrir(addr, "sub/model.stl", 22).await;

        // A écrit… sans que B soit au courant.
        a.insert(0, "écrit par A");
        envoyer(&mut ws_a, a.update()).await;

        // …et B finit par le voir arriver.
        let mut vu = b.text().contains("écrit par A");
        for _ in 0..20 {
            if vu {
                break;
            }
            drainer(&mut ws_b, &mut b, Duration::from_millis(200)).await;
            vu = b.text().contains("écrit par A");
        }
        assert!(
            vu,
            "B n'a jamais reçu la modification de A : {:?}",
            b.text()
        );

        // Et dans l'autre sens : A reçoit la réponse de B.
        b.insert(b.text().len() as u32, " et B aussi");
        envoyer(&mut ws_b, b.update()).await;
        let mut retour = false;
        for _ in 0..20 {
            if a.text() == b.text() {
                retour = true;
                break;
            }
            drainer(&mut ws_a, &mut a, Duration::from_millis(200)).await;
        }
        assert!(
            retour,
            "A n'a jamais reçu la modification de B : {:?}",
            a.text()
        );
        assert_eq!(a.text(), "écrit par A et B aussi");

        // Et le fichier de note finit par être écrit sur disque.
        let note = dir.path().join(notes::NOTES_DIR).join("sub/model.stl.md");
        let mut ecrit = false;
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if std::fs::read_to_string(&note).is_ok_and(|c| c == "écrit par A et B aussi") {
                ecrit = true;
                break;
            }
        }
        assert!(ecrit, "la note n'a pas été persistée");
    }

    /// Une note inexistante (ou un chemin interdit) ferme la connexion.
    #[tokio::test]
    async fn une_room_inconnue_ferme_la_connexion() {
        let (addr, _dir, _state) = serveur().await;
        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("ws://{addr}/collab/sub%2Finexistant.stl"))
                .await
                .unwrap();

        // Le serveur ne répond rien : la connexion se termine.
        let ouvert = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .ok()
            .flatten()
            .is_some_and(|m| matches!(m, Ok(tokio_tungstenite::tungstenite::Message::Binary(_))));
        assert!(
            !ouvert,
            "un chemin hors catalogue ne doit servir aucun document"
        );
    }

    /// Un redémarrage du serveur ne doit pas dupliquer la note.
    ///
    /// Scénario réel : un client garde la note ouverte, le backend est relancé,
    /// puis le client se resynchronise (à la reconnexion, `y-websocket` rejoue
    /// tout son état). Sans l'état CRDT persisté (`.ydoc`), le serveur
    /// ré-amorçait un document neuf : ses insertions se cumulaient avec celles
    /// encore détenues par le client, et la note apparaissait en double.
    #[tokio::test]
    async fn un_redemarrage_ne_duplique_pas_la_note() {
        let dir = catalogue();
        let root = dir.path();
        let rel = "sub/model.stl";

        // Un client ouvre la note et écrit.
        let mut peer = Peer::new(1);
        peer.insert(0, "# Note");

        let premier = state(root);
        let (room, _rx) = Rooms::join(&premier, rel).unwrap();
        let trame = peer.update();
        exchange(&room, &mut peer, trame);
        flush(&room, &premier, rel);
        assert_eq!(notes::read(root, rel).as_deref(), Some("# Note"));

        // Redémarrage : nouvel état serveur, mêmes fichiers sur disque.
        let redemarre = state(root);
        let (room, _rx) = Rooms::join(&redemarre, rel).unwrap();

        // Le client rejoue son état complet, comme à la reconnexion.
        let trame = peer.update();
        exchange(&room, &mut peer, trame);
        flush(&room, &redemarre, rel);

        assert_eq!(
            notes::read(root, rel).as_deref(),
            Some("# Note"),
            "la note a été dupliquée au redémarrage du serveur"
        );
    }
}
