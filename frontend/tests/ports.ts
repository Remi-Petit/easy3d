/**
 * Ports des campagnes e2e, lus dans l'environnement.
 *
 * Ils ont des **valeurs par défaut** — celles que les deux campagnes utilisaient
 * avant d'être réglables — mais elles peuvent être prises sur la machine :
 * sous Windows, une plage réservée (`netsh interface ipv4 show excludedportrange
 * protocol=tcp`) empêche le backend de test d'ouvrir son port, et la campagne
 * s'arrête avant le premier test (`os error 10013`).
 *
 * Le remède est `frontend/.env` : voir `frontend/.env.example`.
 *
 * Une valeur **illisible** arrête la campagne au lieu de retomber en silence sur
 * le défaut : c'est justement le port qu'on cherchait à éviter, et le repli
 * donnerait une erreur qui ne dit pas d'où elle vient. Une variable **vide**, en
 * revanche, vaut « non réglée » — c'est ce que fait un `.env` où la ligne est
 * laissée sans valeur.
 */
import { existsSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * Charge `frontend/.env` **ici**, et non via le lanceur.
 *
 * Playwright tourne sous Node, qui ne lit pas `.env` tout seul (contrairement à
 * `bun` — mais la commande exécutée est `playwright test`, un processus Node) :
 * le charger explicitement donne le même comportement quel que soit le lanceur.
 *
 * ⚠️ Exécuté au chargement du module, donc **avant** que les configurations ne
 * lisent `process.env` : les deux campagnes importent `port()` d'ici.
 *
 * Une variable **déjà posée** dans l'environnement gagne sur le fichier
 * (`loadEnvFile` n'écrase pas l'existant) : `E2E_API_PORT=… bun run test:e2e`
 * reste prioritaire.
 */
function chargerEnv(): void {
  const fichier = resolve(dirname(fileURLToPath(import.meta.url)), '..', '.env')
  if (existsSync(fichier)) process.loadEnvFile(fichier)
}

chargerEnv()

export function port(nom: string, defaut: number): number {
  const brut = process.env[nom]
  if (brut === undefined || brut.trim() === '') return defaut
  const valeur = Number(brut)
  if (!Number.isInteger(valeur) || valeur < 1 || valeur > 65535) {
    throw new Error(`${nom} : « ${brut} » n'est pas un port (attendu : entier de 1 à 65535)`)
  }
  return valeur
}
