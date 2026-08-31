import { computed, onScopeDispose, ref } from 'vue'

type WebKitAudioWindow = Window & typeof globalThis & {
  webkitAudioContext?: typeof AudioContext
}

// Rain sound is generated locally after an explicit click; it never loads audio or records microphone input.
export function useRainSound() {
  const enabled = ref(false)
  const available = ref(true)
  let context: AudioContext | null = null
  let master: GainNode | null = null
  let source: AudioBufferSourceNode | null = null
  let dropTimer: number | undefined

  const label = computed(() => {
    if (!available.value) return '无声'
    return enabled.value ? '静音' : '听雨'
  })

  function createNoiseBuffer(seconds: number) {
    if (!context) throw new Error('AudioContext is not ready')
    const length = Math.ceil(context.sampleRate * seconds)
    const buffer = context.createBuffer(1, length, context.sampleRate)
    const data = buffer.getChannelData(0)
    let seed = 49_327
    for (let index = 0; index < length; index += 1) {
      seed = (seed * 16_807) % 2_147_483_647
      data[index] = (seed / 2_147_483_647) * 2 - 1
    }
    return buffer
  }

  async function initializeAudio() {
    if (context) return
    const AudioContextType = window.AudioContext || (window as WebKitAudioWindow).webkitAudioContext
    if (!AudioContextType) throw new Error('AudioContext is unavailable')

    context = new AudioContextType()
    master = context.createGain()
    master.gain.value = 0.0001

    const rainSource = context.createBufferSource()
    rainSource.buffer = createNoiseBuffer(3)
    rainSource.loop = true
    const highpass = context.createBiquadFilter()
    highpass.type = 'highpass'
    highpass.frequency.value = 170
    const lowpass = context.createBiquadFilter()
    lowpass.type = 'lowpass'
    lowpass.frequency.value = 2_800
    rainSource.connect(highpass).connect(lowpass).connect(master).connect(context.destination)
    rainSource.start()
    source = rainSource
  }

  function scheduleDrop() {
    if (dropTimer !== undefined) window.clearTimeout(dropTimer)
    if (!enabled.value || !context || !master) return
    dropTimer = window.setTimeout(() => {
      if (!context || !master || !enabled.value) return
      const drop = context.createBufferSource()
      drop.buffer = createNoiseBuffer(0.08)
      const filter = context.createBiquadFilter()
      filter.type = 'bandpass'
      filter.frequency.value = 720 + Math.random() * 980
      filter.Q.value = 1.8
      const gain = context.createGain()
      const now = context.currentTime
      gain.gain.setValueAtTime(0.0001, now)
      gain.gain.exponentialRampToValueAtTime(0.025, now + 0.008)
      gain.gain.exponentialRampToValueAtTime(0.0001, now + 0.075)
      drop.connect(filter).connect(gain).connect(master)
      drop.start(now)
      drop.stop(now + 0.09)
      scheduleDrop()
    }, 420 + Math.random() * 980)
  }

  async function toggle() {
    if (!available.value) return
    try {
      await initializeAudio()
      if (!context || !master) return
      if (context.state === 'suspended') await context.resume()
      enabled.value = !enabled.value
      const now = context.currentTime
      master.gain.cancelScheduledValues(now)
      master.gain.setTargetAtTime(enabled.value ? 0.075 : 0.0001, now, 0.08)
      if (enabled.value) scheduleDrop()
      else if (dropTimer !== undefined) window.clearTimeout(dropTimer)
    } catch {
      enabled.value = false
      available.value = false
    }
  }

  async function syncVisibility() {
    if (!context || !enabled.value) return
    if (document.hidden) await context.suspend()
    else await context.resume()
  }

  document.addEventListener('visibilitychange', syncVisibility)

  onScopeDispose(() => {
    document.removeEventListener('visibilitychange', syncVisibility)
    if (dropTimer !== undefined) window.clearTimeout(dropTimer)
    source?.stop()
    void context?.close()
  })

  return { enabled, available, label, toggle }
}
