import type { UiLabMotionDefinition, UiLabMotionEngine } from '@/ui'

export type UiLabMotionEngineSelection = 'auto' | UiLabMotionEngine

export const UI_LAB_MOTION_ENGINE_PRIORITY = [
  'anime',
  'gsap',
  'native',
] as const satisfies readonly UiLabMotionEngine[]

export const UI_LAB_MOTION_ENGINE_SOURCES = Object.freeze({
  anime: import.meta.env.VITE_UI_LAB_ANIME_MODULE_URL
    || 'https://cdn.jsdelivr.net/npm/animejs@4.5.0/+esm',
  gsap: import.meta.env.VITE_UI_LAB_GSAP_MODULE_URL
    || 'https://cdn.jsdelivr.net/npm/gsap@3.15.0/+esm',
})

export interface UiLabMotionPlayback {
  readonly engine: UiLabMotionEngine
  play(): void
  pause(): void
  restart(): void
  seek(progress: number): void
  setPlaybackRate(rate: number): void
  getProgress(): number
  getPlaying(): boolean
  destroy(): void
}

interface CreateMotionPlaybackOptions {
  selection: UiLabMotionEngineSelection
  target: HTMLElement
  motion: UiLabMotionDefinition
  reducedMotion: boolean
  loop: boolean
  playbackRate: number
  onUpdate?: (progress: number) => void
  onComplete?: () => void
}

interface AnimePlaybackLike {
  progress: number
  speed: number
  paused: boolean
  completed: boolean
  play(): AnimePlaybackLike
  pause(): AnimePlaybackLike
  restart(): AnimePlaybackLike
  seek(time: number, muteCallbacks?: boolean): AnimePlaybackLike
  cancel(): AnimePlaybackLike
}

interface AnimeModuleLike {
  animate(target: HTMLElement, parameters: Record<string, unknown>): AnimePlaybackLike
}

interface GsapTimelineLike {
  play(): GsapTimelineLike
  pause(): GsapTimelineLike
  restart(): GsapTimelineLike
  progress(): number
  progress(value: number, suppressEvents?: boolean): GsapTimelineLike
  paused(): boolean
  timeScale(value: number): GsapTimelineLike
  kill(): void
  to(target: HTMLElement, vars: Record<string, unknown>, position?: number): GsapTimelineLike
}

interface GsapLike {
  timeline(options: Record<string, unknown>): GsapTimelineLike
  set(target: HTMLElement, vars: Record<string, unknown>): void
}

interface GsapModuleLike {
  gsap?: GsapLike
  default?: GsapLike
}

function clampProgress(progress: number) {
  return Math.min(1, Math.max(0, progress))
}

function durationFor(motion: UiLabMotionDefinition, reducedMotion: boolean) {
  return reducedMotion ? 1 : Math.max(1, motion.durationMs)
}

function clearMotionStyles(target: HTMLElement) {
  target.style.removeProperty('transform')
  target.style.removeProperty('filter')
  target.style.removeProperty('opacity')
}

function engineCandidates(selection: UiLabMotionEngineSelection) {
  if (selection === 'auto') return [...UI_LAB_MOTION_ENGINE_PRIORITY]
  const selectedIndex = UI_LAB_MOTION_ENGINE_PRIORITY.indexOf(selection)
  return selectedIndex >= 0
    ? UI_LAB_MOTION_ENGINE_PRIORITY.slice(selectedIndex)
    : [...UI_LAB_MOTION_ENGINE_PRIORITY]
}

async function importRemoteModule<T>(url: string): Promise<T> {
  const timeoutMs = 4_000
  let timeout: ReturnType<typeof setTimeout> | undefined
  try {
    return await Promise.race([
      import(/* @vite-ignore */ url) as Promise<T>,
      new Promise<T>((_, reject) => {
        timeout = setTimeout(() => reject(new Error(`motion engine import timed out after ${timeoutMs}ms`)), timeoutMs)
      }),
    ])
  } finally {
    if (timeout) clearTimeout(timeout)
  }
}

function animeKeyframes(motion: UiLabMotionDefinition) {
  return Object.fromEntries(
    motion.keyframes.map((frame) => [
      `${Math.round(frame.offset * 10000) / 100}%`,
      frame.style,
    ]),
  )
}

async function createAnimePlayback(options: CreateMotionPlaybackOptions): Promise<UiLabMotionPlayback> {
  const module = await importRemoteModule<AnimeModuleLike>(UI_LAB_MOTION_ENGINE_SOURCES.anime)
  if (typeof module.animate !== 'function') throw new Error('Anime.js animate() export is unavailable')

  const duration = durationFor(options.motion, options.reducedMotion)
  const animation = module.animate(options.target, {
    keyframes: animeKeyframes(options.motion),
    duration,
    delay: options.reducedMotion ? 0 : options.motion.delayMs,
    ease: options.motion.easing.anime,
    loop: options.loop ? true : Math.max(0, options.motion.iterations - 1),
    playbackRate: options.playbackRate,
    autoplay: false,
    persist: true,
    onUpdate: (instance: AnimePlaybackLike) => {
      options.onUpdate?.(clampProgress(instance.progress))
    },
    onComplete: () => options.onComplete?.(),
  })

  return {
    engine: 'anime',
    play: () => { animation.play() },
    pause: () => { animation.pause() },
    restart: () => { animation.restart() },
    seek: (progress) => { animation.seek(clampProgress(progress) * duration, true) },
    setPlaybackRate: (rate) => { animation.speed = Math.max(0, rate) },
    getProgress: () => clampProgress(animation.progress),
    getPlaying: () => !animation.paused && !animation.completed,
    destroy() {
      animation.cancel()
      clearMotionStyles(options.target)
    },
  }
}

async function createGsapPlayback(options: CreateMotionPlaybackOptions): Promise<UiLabMotionPlayback> {
  const module = await importRemoteModule<GsapModuleLike>(UI_LAB_MOTION_ENGINE_SOURCES.gsap)
  const gsap = module.gsap ?? module.default
  if (!gsap) throw new Error('GSAP module export is unavailable')

  const durationMs = durationFor(options.motion, options.reducedMotion)
  const durationSeconds = durationMs / 1000
  const frames = options.motion.keyframes
  let timeline: GsapTimelineLike

  timeline = gsap.timeline({
    paused: true,
    delay: options.reducedMotion ? 0 : options.motion.delayMs / 1000,
    repeat: options.loop ? -1 : Math.max(0, options.motion.iterations - 1),
    onUpdate: () => options.onUpdate?.(clampProgress(timeline.progress())),
    onComplete: () => options.onComplete?.(),
  })

  gsap.set(options.target, frames[0]?.style ?? {})
  for (let index = 1; index < frames.length; index += 1) {
    const previous = frames[index - 1]
    const frame = frames[index]
    if (!previous || !frame) continue
    const segmentDuration = Math.max(0, frame.offset - previous.offset) * durationSeconds
    timeline.to(options.target, {
      ...frame.style,
      duration: segmentDuration,
      ease: options.motion.easing.gsap,
    }, previous.offset * durationSeconds)
  }
  timeline.timeScale(options.playbackRate)

  return {
    engine: 'gsap',
    play: () => { timeline.play() },
    pause: () => { timeline.pause() },
    restart: () => { timeline.restart() },
    seek: (progress) => { timeline.progress(clampProgress(progress), true) },
    setPlaybackRate: (rate) => { timeline.timeScale(Math.max(0.01, rate)) },
    getProgress: () => clampProgress(timeline.progress()),
    getPlaying: () => !timeline.paused(),
    destroy() {
      timeline.kill()
      clearMotionStyles(options.target)
    },
  }
}

function createNativePlayback(options: CreateMotionPlaybackOptions): UiLabMotionPlayback {
  const duration = durationFor(options.motion, options.reducedMotion)
  const keyframes = options.motion.keyframes.map((frame) => ({
    ...frame.style,
    offset: frame.offset,
  })) as Keyframe[]
  const animation = options.target.animate(keyframes, {
    duration,
    delay: options.reducedMotion ? 0 : options.motion.delayMs,
    easing: options.motion.easing.native,
    iterations: options.loop ? Infinity : options.motion.iterations,
    fill: 'both',
  })
  animation.pause()
  animation.playbackRate = options.playbackRate
  animation.onfinish = () => options.onComplete?.()

  const readProgress = () => {
    const currentTime = Number(animation.currentTime ?? 0)
    const cycleTime = options.loop ? currentTime % duration : currentTime
    return clampProgress(cycleTime / duration)
  }

  let frame = 0
  const reportProgress = () => {
    cancelAnimationFrame(frame)
    if (animation.playState !== 'running') return
    options.onUpdate?.(readProgress())
    frame = requestAnimationFrame(reportProgress)
  }

  return {
    engine: 'native',
    play() {
      animation.play()
      reportProgress()
    },
    pause() {
      animation.pause()
      cancelAnimationFrame(frame)
    },
    restart() {
      animation.currentTime = 0
      animation.play()
      reportProgress()
    },
    seek(progress) {
      animation.currentTime = clampProgress(progress) * duration
      options.onUpdate?.(clampProgress(progress))
    },
    setPlaybackRate(rate) {
      animation.playbackRate = Math.max(0.01, rate)
    },
    getProgress: readProgress,
    getPlaying: () => animation.playState === 'running',
    destroy() {
      cancelAnimationFrame(frame)
      animation.cancel()
      clearMotionStyles(options.target)
    },
  }
}

export async function createMotionPlayback(options: CreateMotionPlaybackOptions): Promise<UiLabMotionPlayback> {
  const errors: string[] = []
  for (const engine of engineCandidates(options.selection)) {
    try {
      if (engine === 'anime') return await createAnimePlayback(options)
      if (engine === 'gsap') return await createGsapPlayback(options)
      return createNativePlayback(options)
    } catch (error) {
      errors.push(`${engine}: ${error instanceof Error ? error.message : String(error)}`)
    }
  }
  throw new Error(`No UI Lab motion engine could initialize. ${errors.join(' | ')}`)
}
