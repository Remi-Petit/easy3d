<script setup lang="ts">
import { byGroup, groupKey, permissionLabel, type PermissionInfo } from '~/utils/permissions'

/**
 * Cases à cocher des droits, groupées par domaine.
 *
 * Le catalogue vient du backend (`GET /permissions`) : l'interface n'a aucune
 * liste de droits en dur, donc un droit ajouté côté Rust apparaît ici, avec son
 * libellé traduit s'il est connu et son identifiant sinon (voir
 * `permissionLabel`).
 */
const props = defineProps<{
  /** Catalogue des droits, tel que le backend le publie. */
  permissions: PermissionInfo[]
  /** Identifiants cochés. */
  modelValue: string[]
  /** Superutilisateur : tout est coché, rien n'est décochable. */
  locked?: boolean
  /** Lecture seule (pas le droit de modifier). */
  disabled?: boolean
}>()

const emit = defineEmits<{ 'update:modelValue': [string[]] }>()

const { t, te } = useI18n()

const groups = computed(() => byGroup(props.permissions))

/** Libellé d'un droit, ou son identifiant si l'interface ne le connaît pas. */
const label = (id: string) => permissionLabel(id, t, te)

function toggle(id: string) {
  if (props.locked || props.disabled) return
  const next = props.modelValue.includes(id)
    ? props.modelValue.filter((known) => known !== id)
    : [...props.modelValue, id]
  emit('update:modelValue', next)
}
</script>

<template>
  <div class="perm" :class="{ 'perm--locked': locked }">
    <fieldset v-for="group in groups" :key="group.group" class="perm__group">
      <legend class="perm__legend">{{ t(groupKey(group.group)) }}</legend>
      <label v-for="permission in group.permissions" :key="permission.id" class="perm__item">
        <input
          type="checkbox"
          :checked="locked || modelValue.includes(permission.id)"
          :disabled="locked || disabled"
          @change="toggle(permission.id)"
        />
        <span>{{ label(permission.id) }}</span>
      </label>
    </fieldset>

    <!--
      Superutilisateur : on dit **pourquoi** tout est coché, sinon la liste
      ressemble à un rôle ordinaire qu'on ne comprend pas ne pas pouvoir modifier.
    -->
    <p v-if="locked" class="perm__note">{{ t('accounts.superuser') }}</p>
  </div>
</template>
