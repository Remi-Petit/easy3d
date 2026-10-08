<script setup lang="ts">
import type { AccountDraft, AccountView, RoleRef, RoleView } from '~/composables/useAccounts'
import type { PermissionInfo } from '~/utils/permissions'

/**
 * Formulaire d'un compte — création et modification.
 *
 * Les deux pages de saisie (`/admin/accounts/new` et
 * `/admin/accounts/edit/<uuid>`) montrent **les mêmes champs** : ils vivent ici,
 * une fois. La page ne décide que de la suite — créer, enregistrer, revenir.
 *
 * Le formulaire tient son propre brouillon : il part des valeurs du compte et
 * n'en sort qu'à la validation. Il n'appelle **rien** : `submit` remonte le
 * patch, la page appelle le serveur et sait où aller ensuite (et c'est elle qui
 * affiche le refus).
 */
const props = withDefaults(
  defineProps<{
    /** Compte à modifier ; `null` (défaut) pour une création. */
    account?: AccountView | null
    /** Rôles proposés à la coche (uuid + nom, servis par `/api/users`). */
    roleRefs: RoleRef[]
    /**
     * Les mêmes rôles, **avec leurs droits** (`/api/roles`). C'est ce qui permet
     * de montrer, en grisé, ce qu'un rôle apporte déjà — et donc de distinguer
     * « accordé par un rôle » de « accordé à ce compte ». Vide quand le compte
     * n'a pas le droit de lire les rôles : les puces restent utilisables.
     */
    roles?: RoleView[]
    /** Catalogue des droits directs, servi par le backend. */
    permissions: PermissionInfo[]
    /** Écriture en cours : les boutons se bloquent. */
    busy?: boolean
    /** `false` : lecture seule (le droit manque). */
    writable?: boolean
  }>(),
  { account: null, busy: false, writable: true },
)

const emit = defineEmits<{
  /** Le patch à enregistrer (mot de passe compris seulement s'il est saisi). */
  submit: [patch: AccountDraft]
  /** Suppression confirmée — jamais proposée en création. */
  remove: []
  /** Le mot de passe local a été retiré (confirmation déjà obtenue). */
  revoke: []
  /** Détachement du fournisseur d'identité, avec le mot de passe qui le remplace. */
  detach: [password: string | undefined]
}>()

const { t } = useI18n()

/** `true` quand le formulaire modifie un compte existant. */
const edition = computed(() => props.account != null)

/**
 * Superutilisateur : les droits du rôle livré `admin` sont acquis, et les
 * décocher n'aurait aucun effet — l'interface les montre donc verrouillés.
 */
const estSuperuser = computed(() =>
  (props.account?.roles ?? []).some((role) => role.name === 'admin'),
)

/**
 * Compte rattaché au fournisseur d'identité : il **n'entre pas par mot de passe**
 * (c'est le serveur qui le refuse, `password_sso`), même quand le mot de passe est
 * le bon — c'est ce qui empêche une porte de survivre à une désactivation chez le
 * fournisseur.
 *
 * Rien n'est effacé pour autant : le mot de passe reste en base, inerte, et
 * resservira si le compte est détaché. Ses deux infobulles ne disent donc pas la
 * même chose selon qu'on a affaire à un administrateur — pour lui, le mot de
 * passe reste une entrée : c'est la porte de secours de l'installation.
 */
const rattache = computed(() => props.account?.oidc === true)

/** Retrait du mot de passe : confirmation en deux temps, sur place. */
const retrait = ref(false)

/** Détachement du SSO : confirmation sur place, avec le mot de passe à poser. */
const detachement = ref(false)
const motDePasseDetachement = ref('')

/**
 * Le détachement n'est possible qu'avec un mot de passe : soit le compte en a
 * déjà un (il resservira tel quel), soit on en pose un ici. Sans lui, le compte
 * ne serait plus joignable du tout — le serveur le refuse aussi
 * (`password_required`).
 */
const detachementPossible = computed(
  () => props.account?.has_password === true || motDePasseDetachement.value.length >= 8,
)

function detacher() {
  if (!detachementPossible.value) return
  emit('detach', motDePasseDetachement.value || undefined)
}

const draft = reactive({
  username: '',
  email: '',
  password: '',
  roles: [] as string[],
  permissions: [] as string[],
  disabled: false,
})

/**
 * Le brouillon part du compte. Le mot de passe, lui, ne se relit pas : le champ
 * reste vide, et **vide veut dire « ne le change pas »** — jamais « efface-le ».
 */
watch(
  () => props.account,
  (compte) => {
    draft.username = compte?.username ?? ''
    draft.email = compte?.email ?? ''
    draft.password = ''
    draft.roles = compte?.roles.map((role) => role.uuid) ?? []
    draft.permissions = [...(compte?.direct ?? [])]
    draft.disabled = compte?.disabled ?? false
  },
  { immediate: true },
)

function has(items: string[], id: string) {
  return items.includes(id)
}

function toggleIn(list: string[], id: string) {
  return has(list, id) ? list.filter((connu) => connu !== id) : [...list, id]
}

/**
 * Droits apportés par les rôles **cochés** : ils s'affichent cochés dans la
 * liste, sans être décochables (le serveur ajoute, il ne retire pas).
 *
 * C'est ce qui rend le clic sur un rôle lisible : on voit ce qu'il donne, et on
 * peut ensuite ajouter — à ce compte seul — ce qu'il ne donne pas.
 */
const herites = computed(() => {
  const connus = new Set<string>()
  for (const uuid of draft.roles) {
    const role = props.roles?.find((candidat) => candidat.uuid === uuid)
    for (const permission of role?.permissions ?? []) connus.add(permission)
  }
  return [...connus]
})

/** Ce que le formulaire envoie : le mot de passe seulement s'il a été saisi. */
function patch(): AccountDraft {
  const body: AccountDraft = {
    username: draft.username,
    email: draft.email,
    roles: [...draft.roles],
    permissions: [...draft.permissions],
    disabled: draft.disabled,
  }
  if (draft.password) body.password = draft.password
  return body
}

/** Les champs obligatoires sont là : l'enregistrement peut partir. */
const complet = computed(() => !!draft.username.trim() && !!draft.email.trim())

function soumets() {
  if (!complet.value) return
  emit('submit', patch())
}

/**
 * Désactiver / réactiver est un **enregistrement**, pas un état local : le
 * serveur peut le refuser (dernier administrateur), et c'est lui qui fait foi.
 */
function basculeActivation() {
  draft.disabled = !draft.disabled
  emit('submit', patch())
}

/** Suppression en deux temps : le second clic confirme. */
const confirmation = ref(false)
</script>

<template>
  <div class="accounts__form">
    <div class="admin__row">
      <input
        v-model="draft.username"
        class="admin__input"
        :disabled="!writable"
        :placeholder="t('accounts.userName')"
        :aria-label="t('accounts.userName')"
      />
      <input
        v-model="draft.email"
        class="admin__input"
        :disabled="!writable"
        :placeholder="t('accounts.email')"
        :aria-label="t('accounts.email')"
      />
    </div>

    <div class="admin__row">
      <input
        v-model="draft.password"
        type="password"
        class="admin__input"
        autocomplete="new-password"
        :disabled="!writable"
        :placeholder="edition ? t('accounts.newPassword') : t('accounts.password')"
        :aria-label="edition ? t('accounts.newPassword') : t('accounts.password')"
      />
    </div>
    <!--
      Deux cas, qui ne disent pas la même chose :
      - un compte **ratitaché** n'entre pas par mot de passe (c'est le fournisseur
        qui décide), mais le mot de passe posé ici est conservé et resservira au
        détachement ; pour un administrateur, il reste même une entrée ;
      - à la création, il est facultatif : sans lui, le compte n'entre que par le
        SSO.
    -->
    <p v-if="rattache && estSuperuser" class="admin__hint">
      {{ t('accounts.attachedAdminHint') }}
    </p>
    <p v-else-if="rattache" class="admin__hint">{{ t('accounts.attachedHint') }}</p>
    <p v-else-if="!edition" class="admin__hint">{{ t('accounts.passwordHint') }}</p>

    <span class="admin__label">{{ t('accounts.rolesOf') }}</span>
    <div class="admin__choices">
      <label
        v-for="role in roleRefs"
        :key="role.uuid"
        class="admin__choice"
        :class="{ 'admin__choice--on': has(draft.roles, role.uuid) }"
      >
        <input
          type="checkbox"
          :checked="has(draft.roles, role.uuid)"
          :disabled="!writable"
          @change="draft.roles = toggleIn(draft.roles, role.uuid)"
        />
        {{ role.name }}
      </label>
    </div>

    <span class="admin__label">{{ t('accounts.direct') }}</span>
    <PermissionPicker
      v-model="draft.permissions"
      :permissions="permissions"
      :inherited="herites"
      :disabled="!writable"
      :locked="estSuperuser"
    />

    <p class="admin__hint">{{ edition ? t('accounts.hint') : t('accounts.defaultHint') }}</p>

    <!--
      Les actions de fin : enregistrer d'abord, puis la désactivation, puis la
      suppression — dans l'ordre de ce qu'elles engagent. La suppression se
      confirme en deux temps, sur place.
    -->
    <div class="admin__field admin__field--actions">
      <button
        v-if="writable"
        type="button"
        class="admin__action admin__action--primary"
        :disabled="busy || !complet"
        @click="soumets()"
      >
        {{ edition ? t('accounts.save') : t('accounts.create') }}
      </button>
      <button
        v-if="edition && writable"
        type="button"
        class="admin__action"
        :disabled="busy || estSuperuser"
        @click="basculeActivation()"
      >
        {{ draft.disabled ? t('accounts.enable') : t('accounts.disable') }}
      </button>
      <button
        v-if="edition && writable"
        type="button"
        class="admin__action"
        :disabled="busy"
        @click="confirmation = !confirmation"
      >
        {{ confirmation ? t('accounts.confirm') : t('accounts.remove') }}
      </button>
      <button
        v-if="edition && writable && confirmation"
        type="button"
        class="admin__action admin__action--danger"
        :disabled="busy"
        @click="emit('remove')"
      >
        {{ t('accounts.remove') }}
      </button>
    </div>

    <!--
      Les gestes qui touchent à l'**entrée** du compte : retirer le mot de passe
      (il n'entrera plus que par le fournisseur) et, à l'inverse, l'en détacher.
      Tous deux se confirment sur place — et le détachement demande le mot de
      passe qui remplacera le SSO.
    -->
    <div
      v-if="edition && writable && (account?.has_password || account?.oidc)"
      class="admin__field admin__field--actions"
    >
      <button
        v-if="account?.has_password"
        type="button"
        class="admin__action"
        :disabled="busy"
        @click="retrait = !retrait"
      >
        {{ retrait ? t('accounts.confirm') : t('accounts.revokePassword') }}
      </button>
      <button
        v-if="retrait"
        type="button"
        class="admin__action admin__action--danger"
        :disabled="busy"
        @click="emit('revoke')"
      >
        {{ t('accounts.revokePassword') }}
      </button>
      <button
        v-if="account?.oidc"
        type="button"
        class="admin__action"
        :disabled="busy"
        @click="detachement = !detachement"
      >
        {{ detachement ? t('common.cancel') : t('accounts.detach') }}
      </button>
    </div>

    <div v-if="detachement" class="admin__field">
      <!--
        On ne demande un mot de passe que si le compte n'en a pas : sinon il
        resservira tel quel (règle non destructive), et le détachement n'a plus
        qu'un clic à faire.
      -->
      <template v-if="!account?.has_password">
        <span class="admin__label">{{ t('accounts.detachPassword') }}</span>
        <div class="admin__row">
          <input
            v-model="motDePasseDetachement"
            type="password"
            class="admin__input"
            autocomplete="new-password"
            :placeholder="t('accounts.newPassword')"
            :aria-label="t('accounts.detachPassword')"
          />
        </div>
      </template>
      <p class="admin__hint">{{ t('accounts.detachHint') }}</p>
      <div class="admin__field--actions">
        <button
          type="button"
          class="admin__action admin__action--danger"
          :disabled="busy || !detachementPossible"
          @click="detacher()"
        >
          {{ t('accounts.detachConfirm') }}
        </button>
      </div>
    </div>
  </div>
</template>
