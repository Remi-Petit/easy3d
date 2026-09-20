use easy3d::{ai, api, config, formats, thumbnail, watcher};
use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, new_debouncer};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;
use tokio::sync::broadcast;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // Rappel des formats reconnus (registre `formats`) : utile pour vérifier
    // qu'un format ajouté est bien pris en compte au démarrage.
    let names: Vec<&str> = formats::all().iter().map(|f| f.name()).collect();
    println!("Formats reconnus : {}", names.join(", "));

    // Charge la configuration YAML (mode d'affichage, dossier des modèles…).
    let config = config::Config::load();

    // Adresses connues des fournisseurs d'IA (`ai-presets.yml`, à côté de la
    // config) : un fichier **à part**, que l'on édite à la main et qui est relu
    // à chaud — c'est de la donnée, pas du code (voir `ai::presets`).
    let presets_path = ai::presets::path();
    match ai::presets::ensure_file(&presets_path) {
        Ok(true) => println!("Adresses connues créées : {}", presets_path.display()),
        Ok(false) => {}
        Err(e) => eprintln!("⚠️  {} non créé : {e}", presets_path.display()),
    }
    let presets = match ai::presets::load(&presets_path) {
        Ok(presets) => presets,
        Err(e) => {
            eprintln!("⚠️  {e} — les adresses livrées sont utilisées.");
            ai::presets::defaults()
        }
    };
    warn_unknown(&presets_path, &presets);
    println!(
        "Adresses connues des fournisseurs : {} (relu à chaud).",
        presets_path.display()
    );

    // Chemin du fichier de config, résolu une fois (surveillé ensuite à chaud).
    // Canonicalisé quand possible : les chemins d'événements du watcher sont
    // comparés à cette référence.
    let config_path = std::fs::canonicalize(config::Config::config_path())
        .unwrap_or_else(|_| config::Config::config_path());

    let resolved = config.resolve_models_root();
    // Canonicalise la racine : évite les `..` (ex : `backend/../models`) et
    // fiabilise les tests de préfixe (`starts_with`) du watcher.
    let root = std::fs::canonicalize(&resolved).unwrap_or_else(|_| PathBuf::from(&resolved));

    // Dossier des aperçus générés par le backend (`<models_root>/.easy3d-thumbs`).
    let thumbs_root = root.join(thumbnail::THUMB_DIR);

    // Un changement de **style** de rendu vide le cache : les aperçus sont des
    // artefacts, ils se régénèrent juste après.
    if thumbnail::apply_style(&thumbs_root) {
        println!("Nouveau style d'aperçus : cache vidé, tout est régénéré.");
    }

    // Génère un aperçu PNG pour chaque modèle déjà présent, avant de démarrer.
    // (Garde un aperçu à jour ; coût une fois au lancement.)
    thumbnail::generate_all(&root, &thumbs_root);

    // Puis retire les aperçus qui n'ont plus de modèle : ceux de l'ancien
    // nommage (`piece.png` pour `piece.stl`, qui faisait collision entre deux
    // formats homonymes) et ceux d'un fichier supprimé hors de l'interface.
    let pruned = thumbnail::prune(&root, &thumbs_root);
    if pruned > 0 {
        println!("{pruned} aperçu(s) obsolète(s) supprimé(s).");
    }

    // Canal broadcast : diffuse la liste des modèles (JSON) à tous les clients WS.
    let (ws_tx, _) = broadcast::channel::<String>(16);

    let state = api::AppState::new(root.clone(), ws_tx, config).with_presets(presets);

    // API → tâche async sur le pool Tokio (multi-thread).
    let server = tokio::spawn({
        let state = state.clone();
        async move {
            if let Err(e) = api::serve(state).await {
                eprintln!("Erreur serveur : {e}");
            }
        }
    });

    // Watcher (bloquant) → thread dédié, car notify bloque le thread OS.
    // Il surveille les modèles **et** la config : à chaque changement détecté,
    // il rescanne (et recharge la config le cas échéant) puis diffuse via le
    // canal broadcast — le frontend se met à jour sans recharger la page.
    std::thread::spawn({
        let state = state.clone();
        move || {
            if let Err(e) = watch_dir(root, config_path, presets_path, state) {
                eprintln!("Erreur watcher : {e}");
            }
        }
    });

    // Garde le runtime en vie tant que le serveur tourne.
    server.await?;

    Ok(())
}

/// Signale les identifiants de fournisseurs présents dans le fichier mais
/// inconnus du binaire : une faute de frappe dans une clé ne se voit autrement
/// nulle part (l'entrée est simplement ignorée).
fn warn_unknown(path: &Path, presets: &ai::presets::Presets) {
    for id in ai::presets::unknown(presets) {
        eprintln!(
            "⚠️  {} : fournisseur inconnu « {id} » (connus : {}). Entrée ignorée.",
            path.display(),
            ai::presets::known_ids().join(", ")
        );
    }
}

/// Relit le fichier des adresses s'il a bougé, et l'installe dans l'état.
///
/// Retourne `true` si les adresses ont changé (le frontend est alors
/// rediffusé). Un fichier illisible — c'est le cas normal pendant qu'on
/// l'édite — **conserve** les adresses en cours : une faute de frappe ne doit
/// pas vider les puces de l'interface.
fn reload_presets(
    state: &api::AppState,
    reloader: &mut ai::presets::Reloader,
    path: &Path,
) -> bool {
    match reloader.poll() {
        ai::presets::Reload::Unchanged => false,
        ai::presets::Reload::Changed(presets) => {
            warn_unknown(path, &presets);
            *state.presets.write().unwrap() = presets;
            println!("Adresses connues rechargées ({}).", path.display());
            true
        }
        ai::presets::Reload::Invalid(e) => {
            eprintln!("⚠️  {e} — adresses précédentes conservées.");
            false
        }
    }
}

/// Surveille le dossier des modèles **et** les fichiers de configuration.
///
/// - Changement dans les modèles → aperçus mis à jour + nouvelle liste diffusée.
/// - Changement de la config → rechargée à chaud (mode d'affichage, dossier des
///   modèles). Si `models_root` change, la surveillance bascule sur le nouveau
///   dossier.
/// - Changement des adresses connues (`ai-presets.yml`) → relues à chaud.
///
/// Dans tous les cas, un snapshot complet est diffusé aux clients WS pour
/// rafraîchir le frontend sans recharger la page.
fn watch_dir(
    models_root: PathBuf,
    config_path: PathBuf,
    presets_path: PathBuf,
    state: api::AppState,
) -> notify::Result<()> {
    let (tx, rx) = channel();
    // Suit le fichier des adresses : rien à relire tant que son empreinte n'a
    // pas bougé (voir `ai::presets::Reloader`).
    let mut presets = ai::presets::Reloader::new(&presets_path);

    // notify-debouncer-full regroupe la rafale d'événements et n'émet qu'un
    // résultat stable après le timeout (50ms = quasi instantané).
    let mut debouncer = new_debouncer(
        Duration::from_millis(50),
        None,
        move |result: DebounceEventResult| {
            let _ = tx.send(result);
        },
    )?;

    let mut current_root = models_root;
    debouncer.watch(&current_root, RecursiveMode::Recursive)?;

    // On surveille le **dossier** parent du fichier de config (mode non
    // récursif) plutôt que le fichier lui-même : cela reste fiable lorsque
    // l'éditeur remplace le fichier (`rename`), ce qui invaliderait une
    // surveillance directe.
    //
    // Un dossier absent ne doit **pas** faire tomber la surveillance des
    // modèles : le cas se produit dès que la config est déplacée (image Docker,
    // `EASY3D_CONFIG` pointant ailleurs). On le signale et on continue sans
    // rechargement à chaud de la configuration.
    let config_watched = match config_path.parent() {
        Some(dir) if dir.is_dir() => {
            debouncer.watch(dir, RecursiveMode::NonRecursive)?;
            true
        }
        _ => {
            eprintln!(
                "⚠️  Configuration non surveillée : {} est introuvable.\n\
                 Les modifications de {} ne seront pas appliquées à chaud.",
                config_path.parent().unwrap_or(&config_path).display(),
                config_path.display()
            );
            false
        }
    };

    if config_watched {
        println!(
            "Je surveille {} et {}… (Ctrl+C pour arrêter)",
            current_root.display(),
            config_path.display()
        );
    } else {
        println!(
            "Je surveille {}… (Ctrl+C pour arrêter)",
            current_root.display()
        );
    }

    // Re-scan périodique : le filet de sécurité des systèmes de fichiers qui ne
    // remontent pas d'événements (voir `config::watch_poll_recommendation`).
    // Relu **à chaque tour** : un changement dans /admin s'applique sans
    // redémarrer, et le journal dit ce qui est réellement en place.
    let mut logged_poll = None;

    loop {
        let poll = config::watch_poll_interval(&state.config(), &state.root());
        let seconds = poll.map_or(0, |d| d.as_secs());
        if logged_poll != Some(seconds) {
            logged_poll = Some(seconds);
            if seconds > 0 {
                println!("Re-scan périodique du catalogue toutes les {seconds} s.");
            } else {
                println!(
                    "Aucun re-scan périodique : les événements du système de fichiers \
                     suffisent sur ce dossier."
                );
            }
        }

        // Sans période, `recv` bloque jusqu'au prochain lot ; avec un re-scan, le
        // `recv_timeout` rend la main à intervalles réguliers : c'est là qu'on
        // rattrape ce qu'inotify n'a pas signalé.
        let result = match poll {
            Some(d) => rx.recv_timeout(d),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };

        let events = match result {
            Ok(Ok(events)) => events,
            Ok(Err(errors)) => {
                for error in errors {
                    eprintln!("Erreur : {error}");
                }
                continue;
            }
            Err(RecvTimeoutError::Timeout) => {
                if api::refresh_if_changed(&state) {
                    println!("Re-scan périodique : le catalogue a changé, liste rediffusée.");
                }
                // Les adresses connues aussi, mais pour une autre raison : un
                // montage virtualisé (Docker Desktop) ne signale **pas** une
                // modification faite depuis l'hôte, et c'est justement le cas
                // de ce fichier — on le relit donc à chaque tour.
                if reload_presets(&state, &mut presets, &presets_path) {
                    api::broadcast_snapshot(&state);
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };

        let mut models_changed = false;
        let mut config_changed = false;

        // Les adresses connues vivent dans le même dossier que la config : on
        // les distingue ici, le lot pouvant porter sur l'un ou l'autre.
        let presets_reloaded = if watcher::touches_file(&events, &presets_path) {
            reload_presets(&state, &mut presets, &presets_path)
        } else {
            false
        };

        // L'interprétation des événements vit dans `watcher` (testable) : ici on
        // ne fait que router le résultat du lot.
        let effect = watcher::handle_batch(&current_root, &config_path, &events);
        for msg in &effect.messages {
            println!("{msg}");
        }
        config_changed |= effect.config_changed;
        models_changed |= effect.models_changed;

        // ── Rechargement à chaud de la configuration ──────────────────────
        if config_changed {
            let reloaded = config::Config::load();
            let changed = reloaded != *state.config.read().unwrap();

            if changed {
                let new_root = reloaded.resolve_models_root();
                let new_root = std::fs::canonicalize(&new_root).unwrap_or(new_root);

                if new_root != current_root {
                    println!(
                        "Dossier des modèles : {} → {}",
                        current_root.display(),
                        new_root.display()
                    );
                    let _ = debouncer.unwatch(&current_root);
                    match debouncer.watch(&new_root, RecursiveMode::Recursive) {
                        Ok(()) => {
                            let thumbs = new_root.join(thumbnail::THUMB_DIR);
                            thumbnail::apply_style(&thumbs);
                            thumbnail::generate_all(&new_root, &thumbs);
                            thumbnail::prune(&new_root, &thumbs);
                            current_root = new_root;
                        }
                        Err(e) => {
                            // Échec : on rétablit la surveillance de l'ancien dossier.
                            eprintln!("Impossible de surveiller {} : {e}", new_root.display());
                            let _ = debouncer.watch(&current_root, RecursiveMode::Recursive);
                        }
                    }
                }

                *state.config.write().unwrap() = reloaded.clone();
                *state.root.write().unwrap() = current_root.clone();

                let mode = match reloaded.display.mode {
                    config::DisplayMode::Image => "image",
                    config::DisplayMode::ThreeD => "3d",
                };
                println!("Configuration rechargée (mode d'affichage : {mode}).");
            }
        }

        // Un seul rafraîchissement, quelle que soit la cause : le frontend
        // remplace son état à chaque message, donc deux diffusions coup sur
        // coup seraient du travail pour rien.
        if config_changed || models_changed || presets_reloaded {
            api::broadcast_snapshot(&state);
        }
    }

    Ok(())
}
