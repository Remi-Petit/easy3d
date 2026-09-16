#!/usr/bin/env node
/**
 * Démarre le serveur de dev Nuxt des tests e2e.
 *
 * Port et base d'API **distincts** de ceux du développement courant : les e2e ne
 * doivent ni viser le conteneur (3100 + son API sur 3101), ni le backend de
 * l'hôte (8090), ni dépendre de ce qui tourne déjà.
 *
 * Script Node plutôt qu'une ligne de commande avec variables d'environnement :
 * la syntaxe POSIX ne passe pas sous Windows.
 */
import { spawn } from 'node:child_process'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))

const child = spawn('npx', ['nuxt', 'dev', '--port', '3200'], {
  cwd: resolve(here, '../..'),
  env: { ...process.env, NUXT_HPCCAT_API_BASE: 'http://127.0.0.1:8091' },
  stdio: 'inherit',
  // `npx` est un script : sous Windows il faut passer par `cmd`.
  shell: process.platform === 'win32',
})

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => child.kill(signal))
}
child.on('exit', (code) => process.exit(code ?? 0))
