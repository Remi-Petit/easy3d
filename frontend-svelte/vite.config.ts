import { sveltekit } from '@sveltejs/kit/vite'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [tailwindcss(), sveltekit()],
  server: {
    // Les messages i18n sont **partagés** avec l'app Nuxt (`../frontend/i18n`),
    // une seule source pour les deux implémentations : le serveur de dev doit
    // donc pouvoir lire en dehors de `frontend-svelte/`.
    fs: { allow: ['..'] },
  },
})
