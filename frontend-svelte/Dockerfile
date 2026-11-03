# Image de test : le serveur SvelteKit (adaptateur Node) + le relais WebSocket.
#
# Volontairement minimal : c'est un banc de comparaison avec l'image Nuxt, pas
# un livrable. Le build se fait sur l'hôte (`bun run build`), l'image ne fait
# que poser le résultat et la seule dépendance d'exécution (`ws`).
FROM node:22-bookworm-slim

WORKDIR /app

COPY package.json ./
RUN npm install --omit=dev --no-audit --no-fund ws@8

COPY build ./build
COPY server.js ./

# Mêmes variables que le service Nuxt : c'est ce qui rend la comparaison
# honnête (même backend, même port, même mode).
ENV PORT=3000 \
    NODE_ENV=production \
    NUXT_HPCCAT_API_BASE=http://127.0.0.1:8090

EXPOSE 3000
CMD ["node", "server.js"]
