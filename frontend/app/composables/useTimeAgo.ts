import type { ComputedRef } from 'vue'

/**
 * Temps relatif traduit (« il y a 3h », « 3h ago », « vor 3h »…).
 *
 * Le calcul vit dans `~/utils/format#relativeTime` — pur, donc testable sans
 * contexte i18n. Ici on ne fait que le rendre : libellé selon la langue et
 * horloge partagée (`useNow`), pour que l'affichage « tique » tout seul.
 *
 * @param read timestamp unix en secondes, ou `null` si la date est inconnue.
 */
export function useTimeAgo(read: () => number | null): ComputedRef<string> {
  const { t } = useI18n()
  const now = useNow()

  return computed(() => {
    const rel = relativeTime(toDate(read()), now.value)
    if (!rel) return t('common.none')
    if (rel.key === 'now') return t('time.now')
    return t(`time.${rel.key}`, { count: rel.count })
  })
}
