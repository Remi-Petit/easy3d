#!/usr/bin/env node
/**
 * Versions d'easy3d : lecture, vérification et mise à jour.
 *
 * Les deux fichiers qui font foi sont :
 *   - `backend/Cargo.toml`  (bloc [package])
 *   - `frontend/package.json`
 *
 * Usage :
 *   bun scripts/bump-version.mjs 0.2.0            # écrit la version dans les deux
 *   bun scripts/bump-version.mjs --check v0.2.0   # vérifie (0 = accord, 1 = écart)
 *
 * `--check` est utilisé par `.github/workflows/release.yml` : un tag dont la
 * version ne correspond pas aux fichiers ne peut pas déclencher de release.
 * En cas de succès, `--check` n'écrit **que** la version sur stdout (les
 * messages vont sur stderr), pour être capturée telle quelle par le workflow.
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const CARGO = join(root, 'backend/Cargo.toml')
const PACKAGE = join(root, 'frontend/package.json')

/** Ligne `version` du bloc `[package]` — pas celles des dépendances. */
const CARGO_VERSION = /(\[package\][\s\S]*?^version = ")([^"]*)(")/m
/** Ligne `"version"` du package.json. */
const PACKAGE_VERSION = /^(\s*"version": ")([^"]*)(")/m

/** Version déclarée dans chacun des deux fichiers. */
function versions() {
  const cargo = readFileSync(CARGO, 'utf8').match(CARGO_VERSION)?.[2]
  const pkg = readFileSync(PACKAGE, 'utf8').match(PACKAGE_VERSION)?.[2]
  if (!cargo || !pkg) {
    console.error(`Version introuvable dans ${cargo ? PACKAGE : CARGO}`)
    process.exit(1)
  }
  return { cargo, pkg }
}

/** Écrit la version dans les deux fichiers, sans les reformater. */
function bump(version) {
  for (const [path, pattern] of [
    [CARGO, CARGO_VERSION],
    [PACKAGE, PACKAGE_VERSION],
  ]) {
    const before = readFileSync(path, 'utf8')
    const after = before.replace(pattern, (_m, prefix, _value, suffix) => prefix + version + suffix)
    if (after === before) {
      console.error(`déjà à jour : ${path}`)
      continue
    }
    writeFileSync(path, after)
    console.error(`écrit ${version} dans ${path}`)
  }
}

const [, , command, expected] = process.argv

if (command === '--check') {
  if (!expected) {
    console.error('Usage : bun scripts/bump-version.mjs --check <tag|version>')
    process.exit(1)
  }
  const tag = expected.replace(/^v/, '')
  const { cargo, pkg } = versions()
  console.error(`tag=${expected} backend/Cargo.toml=${cargo} frontend/package.json=${pkg}`)
  if (tag !== cargo || tag !== pkg) {
    console.error(
      `Version incohérente : le tag ${expected} exige ${tag} dans backend/Cargo.toml ` +
        `(lu : ${cargo}) et frontend/package.json (lu : ${pkg}). ` +
        `Utiliser « bun scripts/bump-version.mjs ${tag} ».`,
    )
    process.exit(1)
  }
  // Seule sortie sur stdout : la version, capturée par le workflow.
  console.log(tag)
} else {
  const version = command
  if (!/^\d+\.\d+\.\d+(?:-[\w.]+)?$/.test(version ?? '')) {
    console.error('Usage : bun scripts/bump-version.mjs <version>   (ex : 0.2.0)')
    console.error('        bun scripts/bump-version.mjs --check v0.2.0')
    process.exit(1)
  }
  bump(version)
  console.error(
    `\nPour publier :\n  git add -A && git commit -m "chore(release): v${version}"\n` +
      `  git tag v${version} && git push origin v${version}`,
  )
}

