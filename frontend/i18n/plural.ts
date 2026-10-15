/**
 * Règles de pluriel que vue-i18n ne devine pas.
 *
 * Le découpage par défaut de vue-i18n est binaire : « 1 » d'un côté, tout le
 * reste de l'autre — ce qui suffit au français, à l'anglais, à l'allemand, à
 * l'espagnol, à l'italien. Les langues slaves en ont **trois** (« 1 файл »,
 * « 2 файла », « 5 файлов ») et le roumain change de forme au-delà de dix-neuf
 * (« 20 de fișiere »). Sans règle, un message à trois formes est choisi par
 * `min(nombre, 2)` : « 2 » et « 5 » tombent alors sur la même, et l'une des deux
 * est fausse.
 *
 * D'où ces règles, qui suivent le CLDR (cardinal) : elles reçoivent le nombre,
 * rendent l'index de la forme à employer — dans l'ordre où le message est écrit
 * (`une | quelques | beaucoup`) — et ne concernent que les langues listées en
 * bas. Partout ailleurs, le défaut de vue-i18n est déjà juste.
 *
 * Le fichier est du TypeScript sans dépendance : `i18n.config.ts` s'en sert pour
 * configurer `createI18n`, et le test unitaire l'importe directement.
 */

/** Une | quelques | beaucoup — russe, ukrainien, croate. */
function slave(choice: number): number {
  const n = Math.abs(choice)
  // 1, 21, 31… mais pas 11 : « 1 файл », « 21 файл ».
  if (n % 10 === 1 && n % 100 !== 11) return 0
  // 2 à 4, 22 à 24… mais pas 12 à 14 : « 2 файла ».
  if (n % 10 >= 2 && n % 10 <= 4 && (n % 100 < 12 || n % 100 > 14)) return 1
  // 0, 5 à 20, 25 à 30… : « 5 файлов ».
  return 2
}

/** Le polonais compte comme le russe, sauf pour 1, qui reste seul. */
function polonais(choice: number): number {
  const n = Math.abs(choice)
  if (n === 1) return 0
  if (n % 10 >= 2 && n % 10 <= 4 && (n % 100 < 12 || n % 100 > 14)) return 1
  return 2
}

/** Tchèque et slovaque : 1 seul, 2 à 4 quelques, le reste beaucoup. */
function tcheque(choice: number): number {
  const n = Math.abs(choice)
  if (n === 1) return 0
  if (n >= 2 && n <= 4) return 1
  return 2
}

/**
 * Roumain : « 1 fișier », « 2 fișiere », mais « 20 **de** fișiere ».
 *
 * C'est la forme « de + nom » qu'on entend au-delà de dix-neuf (et pour zéro),
 * d'où une troisième forme dans les messages.
 */
function roumain(choice: number): number {
  const n = Math.abs(choice)
  if (n === 1) return 0
  if (n === 0 || (n % 100 >= 1 && n % 100 <= 19)) return 1
  return 2
}

/** Règles passées à `createI18n` (`i18n.config.ts`), par code de langue. */
export const pluralizationRules: Record<string, (choice: number) => number> = {
  ru: slave,
  uk: slave,
  hr: slave,
  pl: polonais,
  cs: tcheque,
  sk: tcheque,
  ro: roumain,
}
