// Tient à jour la ligne « langues » du README à partir des fichiers de
// `frontend/i18n/locales/`.
//
// Rien n'est écrit en dur : ajouter une langue (un fichier de messages + une
// entrée dans `frontend/i18n/locales.json`) suffit, le reste suit. Le bloc est
// délimité par des marqueurs, donc le texte autour reste écrit à la main.
//
//   node scripts/readme-langs.mjs           met le README à jour
//   node scripts/readme-langs.mjs --check    échoue si le README est périmé (CI)

import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const LIST = join(ROOT, 'frontend', 'i18n', 'locales.json')
const MESSAGES = join(ROOT, 'frontend', 'i18n', 'locales')
const README = join(ROOT, 'README.md')

const START = '<!-- langues:start -->'
const END = '<!-- langues:end -->'

function fail(message) {
  console.error(`readme-langs : ${message}`)
  process.exit(1)
}

/**
 * Aplatit un arbre de messages en chemins `nav.catalog`.
 *
 * Même découpage que le test de couverture (`frontend/tests/unit/i18n.test.ts`)
 * : une « clé » est le chemin complet, jamais la feuille seule.
 */
function flatten(messages, prefix = '') {
  const out = new Set()
  for (const [key, value] of Object.entries(messages)) {
    const path = prefix ? `${prefix}.${key}` : key
    if (value && typeof value === 'object' && !Array.isArray(value)) {
      for (const nested of flatten(value, path)) out.add(nested)
    } else {
      out.add(path)
    }
  }
  return out
}

const registry = JSON.parse(readFileSync(LIST, 'utf8'))
if (!Array.isArray(registry.locales) || registry.locales.length === 0) {
  fail(`aucune langue déclarée dans ${LIST}`)
}

function keysOf(entry) {
  try {
    return flatten(JSON.parse(readFileSync(join(MESSAGES, entry.file), 'utf8')))
  } catch (error) {
    return fail(`${entry.file} illisible (${error.message})`)
  }
}

const reference = registry.locales.find((entry) => entry.code === registry.defaultLocale)
if (!reference) fail(`la langue de référence « ${registry.defaultLocale} » n'est pas déclarée`)

const referenceKeys = keysOf(reference)
const total = referenceKeys.size
const languages = registry.locales.map((entry) => {
  const keys = keysOf(entry)
  return {
    name: entry.name,
    code: entry.code,
    translated: [...referenceKeys].filter((key) => keys.has(key)).length,
  }
})

const incomplete = languages.filter((language) => language.translated < total)
const coverage =
  incomplete.length === 0
    ? 'traduites à 100 %'
    : `à compléter : ${incomplete.map((l) => `${l.name} (${l.translated}/${total})`).join(', ')}`

const block =
  `${START}**${languages.length} ${languages.length > 1 ? 'langues' : 'langue'}** : ` +
  `${languages.map((l) => l.name).join(', ')} — ${total} clés, ${coverage}.${END}`

const summary = languages.map((l) => `${l.code} ${l.translated}/${total}`).join(' · ')

const readme = readFileSync(README, 'utf8')
const start = readme.indexOf(START)
const end = readme.indexOf(END)
if (start === -1 || end === -1 || readme.lastIndexOf(START) !== start) {
  fail(`marqueurs ${START} / ${END} absents ou en double dans README.md`)
}

if (readme.slice(start, end + END.length) === block) {
  console.log(`readme-langs : README à jour (${summary})`)
} else if (process.argv.includes('--check')) {
  fail(`README non à jour (${summary}).\n  Attendu : ${block}\n  Corriger : node scripts/readme-langs.mjs`)
} else {
  writeFileSync(README, readme.slice(0, start) + block + readme.slice(end + END.length), 'utf8')
  console.log(`readme-langs : README mis à jour (${summary})`)
}
