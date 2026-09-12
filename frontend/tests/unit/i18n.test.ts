import { readFileSync, readdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

/**
 * Couverture des langues.
 *
 * La langue de référence (`defaultLocale` de `i18n/locales.json`, le français)
 * fixe le contrat : toute clé qu'elle déclare doit exister dans les autres
 * langues, et **aucune autre** ne doit s'y ajouter (sinon on traîne des clés
 * mortes que personne ne pense à supprimer).
 *
 * Les variables (`{count}`, `{when}`…) sont vérifiées aussi : c'est l'erreur
 * qu'on ne voit qu'à l'exécution — une traduction qui perd un placeholder
 * affiche « il y a s » au lieu de la valeur.
 *
 * Ce test est **bloquant** : une langue incomplète casse `bun run test`. C'est
 * volontaire — un pourcentage affiché à la demande ne sert à rien si rien
 * n'empêche de livrer une traduction à moitié faite.
 */

type Messages = Record<string, unknown>

/** Aplatit un arbre de messages en chemins `a.b.c` → valeur. */
function flatten(messages: Messages, prefix = ''): Map<string, string> {
  const out = new Map<string, string>()
  for (const [key, value] of Object.entries(messages)) {
    const path = prefix ? `${prefix}.${key}` : key
    if (value && typeof value === 'object' && !Array.isArray(value)) {
      for (const [k, v] of flatten(value as Messages, path)) out.set(k, v)
    } else {
      out.set(path, String(value))
    }
  }
  return out
}

/** Variables `{nom}` d'une chaîne, sans doublon et triées. */
function placeholders(value: string): string[] {
  return [...new Set([...value.matchAll(/\{(\w+)\}/g)].map((m) => m[1]))].sort()
}

/**
 * Liste des langues, lue depuis le disque.
 *
 * `i18n/locales.json` est la seule source (config Nuxt, ce test, ligne du README) :
 * les langues ne sont donc pas recopiées ici, et en ajouter une n'oblige à
 * toucher que le dossier `i18n/locales/` et cette liste.
 */
type LocaleEntry = { code: string; name: string; language: string; file: string }

const i18nDir = resolve(dirname(fileURLToPath(import.meta.url)), '../../i18n')
const registry = JSON.parse(readFileSync(join(i18nDir, 'locales.json'), 'utf8')) as {
  defaultLocale: string
  locales: LocaleEntry[]
}
const localeEntries = registry.locales
const read = (entry: LocaleEntry): Messages =>
  JSON.parse(readFileSync(join(i18nDir, 'locales', entry.file), 'utf8')) as Messages

const REFERENCE = registry.defaultLocale
const reference = flatten(read(localeEntries.find((entry) => entry.code === REFERENCE)!))
const others: Record<string, Messages> = Object.fromEntries(
  localeEntries.filter((entry) => entry.code !== REFERENCE).map((e) => [e.code, read(e)]),
)
const missingKeys = (messages: Messages) =>
  [...reference.keys()].filter((key) => !flatten(messages).has(key))

const extraKeys = (messages: Messages) =>
  [...flatten(messages).keys()].filter((key) => !reference.has(key))

describe('couverture des langues', () => {
  it('la liste déclare la langue de référence', () => {
    expect(localeEntries.map((entry) => entry.code)).toContain(REFERENCE)
  })

  // La liste et le dossier doivent dire la même chose : un fichier de messages
  // oublié dans `locales.json` ne serait ni traduit dans le sélecteur de langue,
  // ni compté par le README ; une entrée sans fichier ferait échouer la lecture.
  it('la liste correspond aux fichiers de `i18n/locales/`', () => {
    const files = readdirSync(join(i18nDir, 'locales'))
      .filter((file) => file.endsWith('.json'))
      .sort()
    expect(localeEntries.map((entry) => entry.file).sort()).toEqual(files)
  })

  it('la référence déclare des clés', () => {
    expect(reference.size).toBeGreaterThan(50)
  })

  /**
   * Clés réellement appelées dans le code, relevées littéralement.
   *
   * Les clés construites (`t("time.${unit}")`) ou passées par variable ne
   * peuvent pas être vérifiées ici : elles sont simplement ignorées. Ce test
   * attrape le cas bête — un `t('filter.date')` oublié dans les fichiers de
   * langue — qui ne se voit qu'à l'écran, sous forme de clé brute affichée.
   */
  function usedKeys(): Map<string, string> {
    const appDir = resolve(dirname(fileURLToPath(import.meta.url)), '../../app')
    const files: string[] = []
    const walk = (dir: string) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const path = join(dir, entry.name)
        if (entry.isDirectory()) walk(path)
        else if (/\.(vue|ts)$/.test(entry.name)) files.push(path)
      }
    }
    walk(appDir)

    const found = new Map<string, string>()
    const patterns = [/\$?\bt\(\s*'([^'$]+)'/g, /keypath="([^"]+)"/g]
    for (const file of files) {
      const source = readFileSync(file, 'utf8')
      for (const pattern of patterns) {
        for (const match of source.matchAll(pattern)) {
          const key = match[1]?.replace(/\$\.$/, '')
          if (key && key.includes('.')) found.set(key, file.replace(appDir, 'app'))
        }
      }
    }
    return found
  }

  it('toutes les clés utilisées dans le code existent en référence', () => {
    const unknown = [...usedKeys().entries()]
      .filter(([key]) => !reference.has(key))
      .map(([key, file]) => `${key} (${file})`)
    expect(unknown, `clés absentes de ${REFERENCE}.json`).toEqual([])
  })

  // Garde-fou : sans ce test, une expression régulière cassée ferait passer le
  // test précédent en ne trouvant aucune clé (donc rien à vérifier).
  it('le relevé des clés utilisées trouve bien des clés', () => {
    expect(usedKeys().size).toBeGreaterThan(30)
  })

  for (const [code, messages] of Object.entries(others)) {
    it(`${code} : toutes les clés de ${REFERENCE} sont traduites`, () => {
      expect(missingKeys(messages), `${code} : clés manquantes`).toEqual([])
    })

    it(`${code} : aucune clé en trop`, () => {
      expect(extraKeys(messages), `${code} : clés inconnues de ${REFERENCE}`).toEqual([])
    })

    it(`${code} : mêmes variables dans chaque message`, () => {
      const flat = flatten(messages)
      const wrong = [...reference.entries()]
        .filter(([key, value]) => {
          const translated = flat.get(key)
          if (translated === undefined) return false
          return placeholders(translated).join() !== placeholders(value).join()
        })
        .map(([key, value]) => `${key} (attendu ${placeholders(value).join(', ') || '—'})`)
      expect(wrong, `${code} : variables différentes`).toEqual([])
    })

    it(`${code} : aucune valeur vide`, () => {
      const empty = [...flatten(messages).entries()]
        .filter(([, value]) => !value.trim())
        .map(([key]) => key)
      expect(empty, `${code} : messages vides`).toEqual([])
    })
  }
})
