#!/usr/bin/env node
/**
 * Démarre le backend Rust des tests e2e, sur des données **jetables**.
 *
 * Les tests e2e écrivent (envoi de fichiers) et modifient la configuration : ils
 * ne doivent toucher ni `models/` ni `backend/config.yml`. On leur donne donc une
 * racine de modèles et un `config.yml` dans un dossier temporaire, désigné par
 * `EASY3D_CONFIG` — prioritaire sur tout le reste dans le backend (et sur
 * `MODELS_ROOT`, d'où le fichier plutôt que la variable).
 *
 * Le port est **8091**, pas 8090 : ce dernier appartient au backend de
développement de l'hôte, et les e2e ne doivent dépendre ni de lui ni du
 * conteneur (qui publie 3101).
 *
 * Le build se fait dans `backend/target/e2e` : sous Windows, un backend de
développement déjà lancé verrouille `target/debug/easy3d.exe`, et le lien
 * échouerait (« accès refusé ») — sans compter que les deux builds se
 * marcheraient dessus.
 *
 * Trois raisons d'être un script Node plutôt qu'une ligne `PORT=8090 cargo run` :
 * la syntaxe d'environnement du shell POSIX ne passe pas sous Windows (cmd), le
 * dossier temporaire doit être écrit avant le démarrage, et le nom du fichier de
 * config doit désigner ce dossier.
 */
import { spawn } from 'node:child_process'
import { mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const modelsRoot = join(tmpdir(), 'easy3d-e2e-models')
const configDir = join(tmpdir(), 'easy3d-e2e-config')

/** Un STL minimal mais **valide** : le viewer et les aperçus doivent pouvoir le lire. */
const stl = (nom) =>
  [
    `solid ${nom}`,
    '  facet normal 0 0 1',
    '    outer loop',
    '      vertex 0 0 0',
    '      vertex 1 0 0',
    '      vertex 0 1 0',
    '    endloop',
    '  endfacet',
    `endsolid ${nom}`,
    '',
  ].join('\n')

// Arborescence connue à chaque exécution : les tests s'appuient sur ces deux
// fichiers et sur ce dossier.
rmSync(modelsRoot, { recursive: true, force: true })
mkdirSync(join(modelsRoot, 'DemaAuto'), { recursive: true })
writeFileSync(join(modelsRoot, 'piece.stl'), stl('piece'))
writeFileSync(join(modelsRoot, 'DemaAuto', 'boitier.stl'), stl('boitier'))

rmSync(configDir, { recursive: true, force: true })
mkdirSync(configDir, { recursive: true })
// Guillemets simples : les contre-obliques d'un chemin Windows y sont littérales.
writeFileSync(
  join(configDir, 'config.yml'),
  `models_root: '${modelsRoot}'\ndisplay:\n  mode: image\n`,
)

console.log(`[e2e] modèles : ${modelsRoot}`)
console.log(`[e2e] config  : ${join(configDir, 'config.yml')}`)
console.log('[e2e] API     : http://127.0.0.1:8091')

const child = spawn('cargo', ['run'], {
  cwd: resolve(here, '../../../backend'),
  env: {
    ...process.env,
    PORT: '8091',
    EASY3D_CONFIG: join(configDir, 'config.yml'),
    CARGO_TARGET_DIR: resolve(here, '../../../backend/target/e2e'),
  },
  stdio: 'inherit',
  // `cargo` est un exécutable : pas besoin d'un shell, sauf sous Windows où le
  // lancement par nom passe par `cmd`.
  shell: process.platform === 'win32',
})

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => child.kill(signal))
}
child.on('exit', (code) => process.exit(code ?? 0))
