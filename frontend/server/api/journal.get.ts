import { backendCall } from '~~/server/utils/backend'

/**
 * Journal d'audit, tel que le backend le connaît.
 *
 * Même relais que les autres routes d'administration : le cookie part avec la
 * requête (le backend exige `users.read`), et le code d'état est rendu tel quel.
 * Aucun `?limit=` n'est recopié de la requête : la borne est décidée par le
 * backend, et l'interface ne doit pas pouvoir demander « tout le journal ».
 */
export default defineEventHandler((event) => backendCall(event, '/journal'))
