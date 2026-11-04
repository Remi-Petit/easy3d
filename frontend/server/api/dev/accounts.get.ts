import { createError, defineEventHandler } from 'h3'
import { useRuntimeConfig } from '#imports'
import { devLoginEnabled, readDevRealm } from '../../utils/devRealm'

/**
 * Comptes du royaume de **développement**, pour la page `/dev/login`.
 *
 * Répond **404** quand la page est fermée (`NUXT_PUBLIC_DEV_LOGIN` absent) : une
 * installation ordinaire n'expose ni la page ni cette liste. Ce n'est pas une
 * question de secret — les mots de passe sont dans le dépôt — mais de ne pas
 * laisser croire qu'une route de développement fait partie du produit.
 */
export default defineEventHandler((event) => {
  if (!devLoginEnabled(useRuntimeConfig(event))) {
    throw createError({ statusCode: 404, statusMessage: 'Not Found' })
  }
  try {
    return readDevRealm()
  } catch (erreur) {
    throw createError({
      statusCode: 500,
      statusMessage: 'Royaume de développement illisible',
      data: { cause: erreur instanceof Error ? erreur.message : String(erreur) },
    })
  }
})
