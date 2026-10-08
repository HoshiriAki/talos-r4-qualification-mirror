# 001 — Separate preview scaling from specimen entry motion

- **Status**: DONE
- **Commit**: 6c0c85f
- **Severity**: HIGH
- **Category**: Physicality and cohesion
- **Estimated scope**: 1 file, approximately 20 lines

## Problem

The mobile viewport proxy and the specimen entrance both own `transform` on
`.specimen-stage`. The entrance keyframe finishes at `scale(1)` and uses
`animation-fill-mode: both`, so it overrides the persistent mobile
`scale(.5)`. This can make a 200% specimen overflow the 390 × 844 proxy after
the entrance completes.

```css
/* frontend/src/ui-lab/UiLabPage.vue:1554 — current */
.specimen-stage {
  position: relative;
  z-index: 5;
  width: min(94%,920px);
  padding: var(--space-6);
  animation: acquire 460ms var(--ease-page) both;
}

/* frontend/src/ui-lab/UiLabPage.vue:1599 — current */
@keyframes acquire {
  from { opacity: 0; transform: scale(.94); filter: blur(4px); }
  to { opacity: 1; transform: scale(1); filter: blur(0); }
}

/* frontend/src/ui-lab/UiLabPage.vue:1687 — current */
.lab-preview--mobile .specimen-stage {
  width: 200%;
  transform: scale(.5);
  transform-origin: center;
}
```

## Target

`.specimen-stage` must exclusively own device-proxy scaling. Move the entrance
motion to `.specimen-assembly`, which is already the direct child of the stage.
Use opacity and a 4px vertical offset so the acquisition remains visible
without competing with viewport scale.

```css
/* target */
.specimen-stage {
  position: relative;
  z-index: 5;
  width: min(94%,920px);
  padding: var(--space-6);
}

.specimen-assembly {
  animation: specimen-acquire 200ms cubic-bezier(0.23, 1, 0.32, 1) both;
}

@keyframes specimen-acquire {
  from { opacity: 0; transform: translateY(4px); }
  to { opacity: 1; transform: translateY(0); }
}
```

Keep the current mobile proxy rule unchanged:

```css
.lab-preview--mobile .specimen-stage {
  width: 200%;
  transform: scale(.5);
  transform-origin: center;
}
```

## Repo conventions to follow

- UI Lab motion lives in the scoped style section of
  `frontend/src/ui-lab/UiLabPage.vue`.
- Existing durations come from `frontend/src/assets/styles/base.css`.
- Use transform and opacity only during the entrance.
- The target strong ease-out is exactly
  `cubic-bezier(0.23, 1, 0.32, 1)`.

## Steps

1. Remove `animation: acquire 460ms var(--ease-page) both` from
   `.specimen-stage`.
2. Add the target `specimen-acquire` animation to `.specimen-assembly`.
3. Replace `@keyframes acquire` with the target `@keyframes specimen-acquire`.
4. Update the local reduced-motion selector from `.specimen-stage` to
   `.specimen-assembly`.
5. Confirm that no other selector or keyframe writes `transform` to
   `.specimen-stage`.

## Boundaries

- Do NOT change the viewport aspect ratios.
- Do NOT change the specimen scale controls or their numeric values.
- Do NOT add wrappers or dependencies; the existing assembly element is the
  animation layer.
- Do NOT touch the motion engine or Registry contracts.
- If the cited selectors have drifted since commit `6c0c85f`, STOP and report
  instead of improvising.

## Verification

- **Mechanical**:
  - Run `frontend\node_modules\.bin\vue-tsc.CMD --noEmit` from the repository
    root; expect exit code 0.
  - Run `frontend\node_modules\.bin\vite.CMD build` from the repository root;
    expect a successful production build.
- **Feel check**:
  - Open `/ui-lab`, select Mobile and specimen scale 200%.
  - Reload and watch the complete entrance at 10% playback speed.
  - Confirm the button and focus frame remain inside the mobile proxy before,
    during, and after the entrance.
  - Confirm the viewport proxy never grows from 18:39 to a wider frame.
  - Change Mobile → Tablet → Mobile and confirm the persistent scale remains
    stable.
- **Done when**: computed `transform` on `.specimen-stage` remains
  `matrix(0.5, 0, 0, 0.5, 0, 0)` after the entrance and no specimen overlay is
  clipped at the default 200% scale.
