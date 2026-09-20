/**
 * Jetons d'API : ce qui se vérifie sans serveur.
 *
 * Le serveur décide et applique la durée de vie (`expiry_from_days` côté
 * backend) ; ici, seulement deux choses : **quelles durées on propose**, et
 * **comment lire une date d'expiration** pour l'afficher.
 */

/**
 * Durées proposées à la création, en jours.
 *
 * `0` arrive en tête et est le choix **par défaut** : un jeton qui s'arrête tout
 * seul au bout de trois mois parce qu'on n'a pas touché à un menu serait une
 * surprise — et un agent qui cesse de fonctionner sans raison apparente coûte
 * plus cher que le risque d'un jeton oublié, que la page prend soin de rendre
 * visible (« jamais utilisé », date d'expiration).
 */
export const TOKEN_DURATIONS = [0, 7, 30, 90, 365] as const

/** Durée proposée par défaut (0 = sans expiration). */
export const TOKEN_DEFAULT_DAYS = 0

/** Ce qu'il y a à dire d'un jeton, du point de vue de sa date de fin. */
export type ExpiryKind = 'never' | 'expired' | 'soon' | 'later'

/** Jours restants avant qu'un jeton soit « bientôt expiré ». */
export const SOON_DAYS = 7

/**
 * Classe une date d'expiration, pour que la page choisisse sa présentation.
 *
 * `null` = sans expiration (le cas de tous les jetons créés avant le réglage).
 * « Bientôt » se compte en jours **entiers** : un jeton qui expire dans 6 jours
 * et 23 h doit être annoncé comme tel, pas comme « dans 7 jours ».
 */
export function expiryKind(
  expiresAt: number | null | undefined,
  now: number,
  soonDays: number = SOON_DAYS,
): ExpiryKind {
  if (expiresAt === null || expiresAt === undefined) return 'never'
  if (expiresAt <= now) return 'expired'
  const jours = Math.ceil((expiresAt - now) / 86_400)
  return jours <= soonDays ? 'soon' : 'later'
}

/**
 * Clé i18n d'une durée proposée : `account.days0`, `account.days30`…
 *
 * C'est la page qui vérifie que la clé existe (`te`) et retombe sinon sur le
 * libellé générique `account.daysMany` : une durée inconnue (valeur écrite à la
 * main, backend plus récent) ne doit pas afficher une clé brute.
 */
export function durationKey(days: number): string {
  return `account.days${days}`
}
