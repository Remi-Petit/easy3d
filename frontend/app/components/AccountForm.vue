<script setup lang="ts">
import type { AccountDraft, AccountView, RoleRef } from '~/composables/useAccounts'
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
    /** Rôles proposés à la coche. */
    roleRefs: RoleRef[]
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
    <!-- Le mot de passe est facultatif : un compte sans mot de passe ne se
         connecte que par le fournisseur d'identité (SSO). -->
    <p v-if="!edition" class="admin__hint">{{ t('accounts.passwordHint') }}</p>

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
  </div>
</template>
