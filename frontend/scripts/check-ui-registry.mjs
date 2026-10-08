import { readFile, access } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const scriptDir = path.dirname(fileURLToPath(import.meta.url))
const frontendRoot = path.resolve(scriptDir, '..')
const registryPath = path.join(frontendRoot, 'src/ui/component-registry.json')

const registry = JSON.parse(await readFile(registryPath, 'utf8'))
const errors = []
const validCategories = new Set([
  'primitive', 'control', 'input', 'navigation', 'feedback',
  'data-display', 'layout', 'pattern', 'shell', 'surface-adapter',
])
const validStatuses = new Set(['draft', 'experimental', 'active', 'deprecated', 'removed'])
const validLevels = new Set(['L0', 'L1', 'L2', 'L3'])
const validLabLocales = new Set(['zh-CN', 'en'])
const validLabDirections = new Set(['ltr', 'rtl'])
const validSlotKinds = new Set(['content', 'signal', 'icon', 'meta', 'decoration', 'focus'])
const validSlotPositions = new Set(['before', 'after', 'overlay', 'underlay'])
const validMotionTargets = new Set(['specimen', 'probe'])
// slot:<id> 必须引用组件声明的 designSlot；selector:<expr> 必须是 sandbox 白名单选择器前缀。
function isValidMotionTarget(target, slotIds) {
  if (validMotionTargets.has(target)) return true
  if (typeof target === 'string' && target.startsWith('slot:')) {
    const slotId = target.slice('slot:'.length)
    return slotId.length > 0 && slotIds.has(slotId)
  }
  if (typeof target === 'string' && target.startsWith('selector:')) {
    return target.length > 'selector:'.length
  }
  return false
}
const validMotionTriggers = new Set(['mount', 'hover', 'press', 'state-change', 'manual'])
const validMotionEngines = new Set(['anime', 'gsap', 'native'])
const validFutureMotionEngines = new Set(['three'])
const requiredMotionEnginePriority = ['anime', 'gsap', 'native']

if (registry.schemaVersion !== '1.1.0') errors.push('schemaVersion must be 1.1.0')
if (!Array.isArray(registry.components)) errors.push('components must be an array')

const ids = new Set()
const names = new Set()
for (const [index, component] of (registry.components ?? []).entries()) {
  const at = `components[${index}]`
  if (!/^talos\.ui(?:\.[a-z0-9-]+)+$/.test(component.id ?? '')) errors.push(`${at}.id is invalid`)
  if (ids.has(component.id)) errors.push(`${at}.id duplicates ${component.id}`)
  if (names.has(component.name)) errors.push(`${at}.name duplicates ${component.name}`)
  ids.add(component.id)
  names.add(component.name)

  if (!validCategories.has(component.category)) errors.push(`${at}.category is invalid`)
  if (!validStatuses.has(component.status)) errors.push(`${at}.status is invalid`)
  if (!validLevels.has(component.conformance?.level)) errors.push(`${at}.conformance.level is invalid`)
  if (!Array.isArray(component.variants) || component.variants.length === 0) errors.push(`${at}.variants is empty`)
  if (!Array.isArray(component.states) || component.states.length === 0) errors.push(`${at}.states is empty`)
  if (new Set(component.variants).size !== component.variants.length) errors.push(`${at}.variants contains duplicates`)
  if (new Set(component.states).size !== component.states.length) errors.push(`${at}.states contains duplicates`)
  if (component.status === 'active' && component.conformance?.level !== 'L3') {
    errors.push(`${at} cannot be active without L3 conformance`)
  }

  const implementationPath = component.implementation?.path
  if (typeof implementationPath !== 'string' || !implementationPath.startsWith('src/ui/')) {
    errors.push(`${at}.implementation.path must be inside src/ui`)
  } else {
    try {
      await access(path.join(frontendRoot, implementationPath))
    } catch {
      errors.push(`${at}.implementation.path does not exist: ${implementationPath}`)
    }
  }

  if (component.lab) {
    const labAt = `${at}.lab`
    if (!Array.isArray(component.lab.locales) || component.lab.locales.length === 0) {
      errors.push(`${labAt}.locales is empty`)
    } else if (component.lab.locales.some((locale) => !validLabLocales.has(locale))) {
      errors.push(`${labAt}.locales contains an invalid locale`)
    }

    if (!Array.isArray(component.lab.directions) || component.lab.directions.length === 0) {
      errors.push(`${labAt}.directions is empty`)
    } else if (component.lab.directions.some((direction) => !validLabDirections.has(direction))) {
      errors.push(`${labAt}.directions contains an invalid direction`)
    }

    if (!Array.isArray(component.lab.motionEngines)) {
      errors.push(`${labAt}.motionEngines must be an array`)
    } else {
      if (component.lab.motionEngines.some((engine) => !validMotionEngines.has(engine))) {
        errors.push(`${labAt}.motionEngines contains an invalid engine`)
      }
      if (component.lab.motionEngines.join(',') !== requiredMotionEnginePriority.join(',')) {
        errors.push(`${labAt}.motionEngines must preserve anime, gsap, native priority`)
      }
    }

    if (component.lab.futureMotionEngines !== undefined) {
      if (!Array.isArray(component.lab.futureMotionEngines)) {
        errors.push(`${labAt}.futureMotionEngines must be an array`)
      } else if (component.lab.futureMotionEngines.some((engine) => !validFutureMotionEngines.has(engine))) {
        errors.push(`${labAt}.futureMotionEngines contains an invalid engine`)
      }
    }

    const componentSlotIds = new Set()
    if (!Array.isArray(component.lab.designSlots) || component.lab.designSlots.length === 0) {
      errors.push(`${labAt}.designSlots is empty`)
    } else {
      const slotIds = componentSlotIds
      for (const [slotIndex, slot] of component.lab.designSlots.entries()) {
        const slotAt = `${labAt}.designSlots[${slotIndex}]`
        if (!/^[a-z0-9-]+$/.test(slot.id ?? '')) errors.push(`${slotAt}.id is invalid`)
        if (slotIds.has(slot.id)) errors.push(`${slotAt}.id duplicates ${slot.id}`)
        slotIds.add(slot.id)
        if (!validSlotKinds.has(slot.kind)) errors.push(`${slotAt}.kind is invalid`)
        if (!validSlotPositions.has(slot.position)) errors.push(`${slotAt}.position is invalid`)
        if (typeof slot.required !== 'boolean') errors.push(`${slotAt}.required must be boolean`)
        if (typeof slot.enabledByDefault !== 'boolean') errors.push(`${slotAt}.enabledByDefault must be boolean`)
        if (slot.required && !slot.enabledByDefault) errors.push(`${slotAt} required slots must be enabled by default`)
      }
    }

    if (!Array.isArray(component.lab.motions)) {
      errors.push(`${labAt}.motions must be an array`)
    } else {
      const motionIds = new Set()
      for (const [motionIndex, motion] of component.lab.motions.entries()) {
        const motionAt = `${labAt}.motions[${motionIndex}]`
        if (!/^[a-z0-9-]+$/.test(motion.id ?? '')) errors.push(`${motionAt}.id is invalid`)
        if (motionIds.has(motion.id)) errors.push(`${motionAt}.id duplicates ${motion.id}`)
        motionIds.add(motion.id)
        if (!isValidMotionTarget(motion.target, componentSlotIds)) errors.push(`${motionAt}.target is invalid`)
        if (!validMotionTriggers.has(motion.trigger)) errors.push(`${motionAt}.trigger is invalid`)
        if (!Number.isFinite(motion.durationMs) || motion.durationMs < 0) errors.push(`${motionAt}.durationMs is invalid`)
        if (!Number.isFinite(motion.delayMs) || motion.delayMs < 0) errors.push(`${motionAt}.delayMs is invalid`)
        if (!Number.isInteger(motion.iterations) || motion.iterations < 1) errors.push(`${motionAt}.iterations is invalid`)
        if (!Array.isArray(motion.tracks) || motion.tracks.length === 0) errors.push(`${motionAt}.tracks is empty`)
        if (!motion.easing || typeof motion.easing !== 'object') {
          errors.push(`${motionAt}.easing must declare anime, gsap and native values`)
        } else {
          for (const engine of requiredMotionEnginePriority) {
            if (typeof motion.easing[engine] !== 'string' || motion.easing[engine].trim() === '') {
              errors.push(`${motionAt}.easing.${engine} is empty`)
            }
          }
        }
        if (!Array.isArray(motion.keyframes) || motion.keyframes.length < 2) {
          errors.push(`${motionAt}.keyframes must contain at least two frames`)
        } else {
          const offsets = motion.keyframes.map((frame) => frame.offset)
          if (offsets.some((offset) => typeof offset !== 'number' || offset < 0 || offset > 1)) {
            errors.push(`${motionAt}.keyframes contains an invalid offset`)
          }
          if (offsets.some((offset, frameIndex) => frameIndex > 0 && offset < offsets[frameIndex - 1])) {
            errors.push(`${motionAt}.keyframes offsets must be sorted`)
          }
          if (offsets[0] !== 0 || offsets.at(-1) !== 1) {
            errors.push(`${motionAt}.keyframes must start at 0 and end at 1`)
          }
        }
      }
    }
  }
}

if (errors.length) {
  console.error('TALOS UI registry check failed:')
  for (const error of errors) console.error(`- ${error}`)
  process.exit(1)
}

console.log(`TALOS UI registry OK: ${registry.components.length} components, ${ids.size} unique IDs`)
