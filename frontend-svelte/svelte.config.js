import adapter from '@sveltejs/adapter-node'

/** @type {import('@sveltejs/kit').Config} */
const config = {
  kit: {
    // Serveur Node autonome : `build/` + `server.js` (voir `server.js`, qui
    // ajoute le relais WebSocket que SvelteKit ne gère pas lui-même).
    adapter: adapter({ out: 'build' }),
  },
  compilerOptions: {
    runes: true,
  },
}

export default config
