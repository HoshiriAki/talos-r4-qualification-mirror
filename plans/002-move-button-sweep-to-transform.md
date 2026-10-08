# 002 — Move the button signal sweep to transform

- **Status**: DONE
- **Commit**: 6c0c85f
- **Severity**: HIGH
- **Category**: Performance and interruptibility
- **Estimated scope**: 1 file, approximately 20 lines

## Problem

The execution-primary signal sweep animates `left`, which requires layout and
paint on every frame. It is a frequently triggered hover animation and uses a
keyframe that restarts when retriggered.

```css
/* frontend/src/ui/components/TalosButton.vue:148 — current */
.talos-button__signal {
  position: absolute;
  inset-block: 0;
  left: -42%;
  width: 36%;
  background: var(--accent);
  transform: skewX(-18deg);
  opacity: 0;
  pointer-events: none;
}

/* frontend/src/ui/components/TalosButton.vue:159 — current */
.talos-button--execution-primary:hover:not(:disabled) .talos-button__signal {
  animation: talos-button-sweep var(--duration-page) var(--ease-page) 1;
}

/* frontend/src/ui/components/TalosButton.vue:172 — current */
@keyframes talos-button-sweep {
  0% { left: -42%; opacity: 0; }
  15% { opacity: 1; }
  100% { left: 112%; opacity: 0; }
}
```

## Target

Keep the sweep absolutely positioned at the left edge and animate only
`transform` and `opacity`. Restrict hover motion to a fine pointer. Reduce the
duration to 200ms with the strong UI ease-out.

```css
/* target */
.talos-button__signal {
  position: absolute;
  inset-block: 0;
  left: 0;
  width: 36%;
  background: var(--accent);
  transform: translateX(-140%) skewX(-18deg);
  opacity: 0;
  pointer-events: none;
  will-change: transform, opacity;
}

@media (hover: hover) and (pointer: fine) {
  .talos-button--execution-primary:hover:not(:disabled) .talos-button__signal {
    animation: talos-button-sweep 200ms cubic-bezier(0.23, 1, 0.32, 1) 1;
  }
}

@keyframes talos-button-sweep {
  0% { transform: translateX(-140%) skewX(-18deg); opacity: 0; }
  15% { opacity: 1; }
  100% { transform: translateX(310%) skewX(-18deg); opacity: 0; }
}
```

The executing state must continue to override the sweep:

```css
.talos-button--executing .talos-button__signal {
  left: auto;
  right: 0;
  width: 4px;
  opacity: 1;
  transform: none;
  animation: none;
  will-change: auto;
}
```

## Repo conventions to follow

- `TalosButton.vue` already uses transform-only press feedback at lines 74–88.
- Use the exact strong ease-out
  `cubic-bezier(0.23, 1, 0.32, 1)`.
- Preserve the current Industrial Avant signal geometry and accent color.

## Steps

1. Replace the signal's `left: -42%` starting position with `left: 0` and the
   target translate/skew transform.
2. Wrap the hover animation selector in
   `@media (hover: hover) and (pointer: fine)`.
3. Replace the duration and easing with the exact 200ms target.
4. Rewrite `talos-button-sweep` so it animates transform and opacity only.
5. Add `will-change: auto` to the executing override.
6. Keep the existing reduced-motion block; verify that it still suppresses the
   sweep.

## Boundaries

- Do NOT change button sizes, colors, variants, labels, or click behavior.
- Do NOT change the Registry's research presets in this plan.
- Do NOT add Anime.js, GSAP, or another runtime dependency.
- If the cited code has drifted since commit `6c0c85f`, STOP and report.

## Verification

- **Mechanical**:
  - Run `frontend\node_modules\.bin\vue-tsc.CMD --noEmit`.
  - Run `frontend\node_modules\.bin\vite.CMD build`.
  - Search `TalosButton.vue` and confirm `@keyframes talos-button-sweep`
    contains no `left`, `right`, `width`, margin, or padding animation.
- **Feel check**:
  - Hover the execution-primary button repeatedly with animation playback at
    10%.
  - Confirm the sweep crosses cleanly in 200ms and never changes button
    geometry.
  - Confirm tapping on a touch-emulated viewport does not trigger the hover
    sweep.
  - Set the button to executing and confirm only the fixed right-edge signal is
    visible.
- **Done when**: the Performance panel shows no layout event attributable to
  the sweep and the visual path still travels fully from left to right.
