import { backendCall } from '~~/server/utils/backend'

/**
 * Contrôle du fournisseur d'identité (bouton « Tester » de l'administration).
 *
 * Même relais que les autres routes d'administration : le cookie part avec la
 * requête (le backend exige `config.write`), et le code d'état du backend est
 * rendu tel quel — un 502 dit « le fournisseur n'a pas répondu », ce que
 * l'interface affiche au lieu de le traduire en panne générale.
 */
export default defineEventHandler((event) => backendCall(event, '/auth/oidc/check'))
