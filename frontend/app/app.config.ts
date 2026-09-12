/**
 * Configuration Nuxt UI.
 *
 * On aligne la couleur primaire des composants sur l'accent maison
 * (`#6366f1`, soit indigo-500) et le neutre sur la teinte ardoise de la
 * palette, pour que le shell Nuxt UI et le reste de l'app (CSS maison)
 * partagent la même apparence.
 */
export default defineAppConfig({
  ui: {
    colors: {
      primary: 'indigo',
      neutral: 'slate',
    },
  },
})
