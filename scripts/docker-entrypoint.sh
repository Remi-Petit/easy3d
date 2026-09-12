#!/bin/bash
#
# PID 1 du conteneur easy3d : l'API Rust et le serveur SSR Nuxt tournent
# **ensemble**, dans un seul conteneur.
#
# Pourquoi un superviseur plutôt qu'un `&` dans un shell :
#
#  - `docker stop` n'envoie SIGTERM qu'au PID 1. Sans relais, les deux enfants
#    seraient tués brutalement (SIGKILL) après le délai de grâce — mauvais pour
#    l'API, qui écrit les notes sur disque de façon différée ;
#  - si un seul des deux process meurt, le conteneur doit s'arrêter : un
#    conteneur « vivant » avec une moitié en panne se diagnostique très mal.
set -euo pipefail

api_port="${PORT:-8090}"
ui_port="${NITRO_PORT:-3000}"
echo "easy3d : API/MCP sur :${api_port}, interface sur :${ui_port}"

# Chacun démarre sans attendre l'autre : le backend peut mettre quelques
# secondes à générer les aperçus d'un gros catalogue, et l'interface sait
# afficher « backend indisponible » en attendant.
easy3d &
api=$!

node /app/.output/server/index.mjs &
ui=$!

shutdown() {
    trap - TERM INT
    kill -TERM "$api" "$ui" 2>/dev/null || true
    wait "$api" "$ui" 2>/dev/null || true
}
trap shutdown TERM INT

# `wait -n` rend la main dès que **l'un** des deux process s'arrête.
status=0
wait -n || status=$?
echo "easy3d : un process s'est arrêté (code $status), arrêt du conteneur"

shutdown
exit "$status"
