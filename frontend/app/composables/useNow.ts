import { onBeforeUnmount, onMounted, ref } from 'vue'

/**
 * Horloge partagée qui se met à jour **chaque seconde**.
 *
 * Permet d'afficher des temps relatifs (« il y a Xs / Xmin / Xh ») qui « tiquent »
 * en continu, indépendamment des pushs WebSocket.
 *
 * Partagée au niveau module : un seul `setInterval` pour toute l'app, gardé en
 * vie par un compteur de références. Rien ne démarre sur le serveur (SSR) car
 * l'interval n'est lancé qu'au premier `onMounted`.
 */
const now = ref(new Date())
let timer: ReturnType<typeof setInterval> | null = null
let refCount = 0

function startTimer() {
  if (timer) return
  timer = setInterval(() => {
    now.value = new Date()
  }, 1000)
}

function stopTimer() {
  if (timer) {
    clearInterval(timer)
    timer = null
  }
}

export function useNow() {
  onMounted(() => {
    refCount++
    startTimer()
  })
  onBeforeUnmount(() => {
    refCount--
    if (refCount <= 0) stopTimer()
  })
  return now
}
