// Types du serveur (SvelteKit).
declare global {
  namespace App {
    interface Locals {
      /** Langue résolue par `hooks.server.ts` (cookie `easy3d_lang`). */
      langue: string
    }
  }
}

export {}
