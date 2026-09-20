/**
 * Droits, côté interface : ce qui se vérifie sans serveur.
 *
 * Les identifiants viennent du backend (`GET /permissions`) — l'interface n'a
 * **aucune** liste de droits en dur : ajouter un droit côté Rust suffit à le
 * faire apparaître dans les cases à cocher (même principe que les formats de
 * fichiers et les fournisseurs d'IA). Ce qui est fixé ici, c'est la façon de les
 * regrouper et de retrouver leur libellé (clés `perm.*`, i18n ×4).
 */

/** Un droit, tel que `GET /permissions` le publie. */
export interface PermissionInfo {
  id: string
  group: string
}

/** Un domaine de droits (`catalog`, `accounts`…) et ses droits. */
export interface PermissionGroup {
  group: string
  permissions: PermissionInfo[]
}

/**
 * Regroupe les droits par domaine, **dans l'ordre annoncé par le backend**.
 *
 * L'ordre compte : c'est celui du catalogue côté serveur (catalogue, IA,
 * réglages, comptes), et il fait la lecture des cases à cocher — un tri
 * alphabétique mettrait « supprimer » avant « voir ».
 */
export function byGroup(permissions: PermissionInfo[]): PermissionGroup[] {
  const groups: PermissionGroup[] = []
  for (const permission of permissions) {
    const known = groups.find((entry) => entry.group === permission.group)
    if (known) known.permissions.push(permission)
    else groups.push({ group: permission.group, permissions: [permission] })
  }
  return groups
}

/** Clé i18n du libellé d'un droit (`perm.model.delete`). */
export function permissionKey(id: string): string {
  return `perm.${id}`
}

/** Clé i18n du libellé d'un domaine (`permGroup.catalog`). */
export function groupKey(group: string): string {
  return `permGroup.${group}`
}

/**
 * Libellé d'un droit dans la langue courante.
 *
 * `translate` est le `t` d'i18n, `has` son `te`. Un droit que l'interface ne
 * connaît pas (backend plus récent que le frontend) affiche alors son
 * **identifiant** : `model.purge` se lit, `perm.model.purge` ne dit rien.
 */
export function permissionLabel(
  id: string,
  translate: (key: string) => string,
  has: (key: string) => boolean,
): string {
  const key = permissionKey(id)
  return has(key) ? translate(key) : id
}

/** `true` si le droit figure dans la liste effective. */
export function granted(effective: readonly string[] | undefined, id: string): boolean {
  return (effective ?? []).includes(id)
}

/**
 * Identifiants utilisés par les composants.
 *
 * Ils sont recopiés du backend (`auth::permissions`) : ces constantes ne
 * **décident** rien, elles évitent d'écrire des chaînes en dur dans les gabarits,
 * où une faute de frappe se traduirait par un bouton qui disparaît sans raison
 * apparente. La liste complète, elle, vient toujours de `GET /permissions`.
 */
export const PERM = {
  catalogRead: 'catalog.read',
  modelUpload: 'model.upload',
  modelRename: 'model.rename',
  modelDelete: 'model.delete',
  noteWrite: 'note.write',
  aiUse: 'ai.use',
  aiConfig: 'ai.config',
  configRead: 'config.read',
  configWrite: 'config.write',
  usersRead: 'users.read',
  usersWrite: 'users.write',
  rolesRead: 'roles.read',
  rolesWrite: 'roles.write',
} as const
