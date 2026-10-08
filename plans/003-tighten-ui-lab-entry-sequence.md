# 003 — Tighten the UI Lab entry sequence

- **Status**: DONE
- **Commit**: 6c0c85f
- **Severity**: MEDIUM
- **Category**: Purpose, frequency, easing, and duration
- **Estimated scope**: 1 file, approximately 15 lines

## Problem

The frequently reopened UI Lab uses four entrance animations between 420ms and
620ms. These exceed the 300ms UI budget and make the workstation appear to
assemble slowly before it can be read.

```css
/* frontend/src/ui-lab/UiLabPage.vue:1477 — current */
.command-bar {
  animation: enter-down 420ms var(--ease-page) both;
}

/* frontend/src/ui-lab/UiLabPage.vue:1493 — current */
.manifest {
  animation: enter-up 540ms var(--ease-page) 80ms both;
}

/* frontend/src/ui-lab/UiLabPage.vue:1503 — current */
.workbench {
  animation: enter-up 600ms var(--ease-page) 130ms both;
}

/* frontend/src/ui-lab/UiLabPage.vue:1575 — current */
.motion-console,.evidence {
  animation: enter-up 620ms var(--ease-page) 190ms both;
}
```

`motion-console` is now inside `workbench`; independently animating both
creates nested transform motion and makes the center column feel detached.

## Target

Use one crisp sequence under 300ms. The workbench owns the workstation
entrance; the nested Motion Bench does not animate independently.

```css
/* target */
.command-bar {
  animation: enter-down 160ms cubic-bezier(0.23, 1, 0.32, 1) both;
}

.manifest {
  animation: enter-up 200ms cubic-bezier(0.23, 1, 0.32, 1) 30ms both;
}

.workbench {
  animation: enter-up 240ms cubic-bezier(0.23, 1, 0.32, 1) 60ms both;
}

.motion-console {
  animation: none;
}

.evidence {
  animation: enter-up 220ms cubic-bezier(0.23, 1, 0.32, 1) 100ms both;
}
```

Keep the current 10px command-bar and 16px section offsets unless visual
inspection reveals a documented design-system conflict.

## Repo conventions to follow

- Entrance keyframes already use transform and opacity only.
- TALOS is a crisp operational dashboard; do not add bounce.
- Decorative staggering must not delay interaction.
- Use exactly `cubic-bezier(0.23, 1, 0.32, 1)`.

## Steps

1. Set the command-bar animation to the exact 160ms target.
2. Set the manifest animation to 200ms with a 30ms delay.
3. Set the workbench animation to 240ms with a 60ms delay.
4. Split the combined `.motion-console,.evidence` animation declaration so
   Motion Bench has no independent entrance and Evidence uses the target
   220ms/100ms entrance.
5. Remove or update the later `.evidence { animation-delay: 230ms; }` override
   so it cannot override the target.
6. Preserve the existing reduced-motion selector until plan 004 executes.

## Boundaries

- Do NOT add animation to Registry rows, control modules, timeline tracks, or
  telemetry cells.
- Do NOT change layout, spacing, viewport ratios, or component contracts.
- Do NOT use springs or bounce.
- If the cited code has drifted since commit `6c0c85f`, STOP and report.

## Verification

- **Mechanical**:
  - Run `frontend\node_modules\.bin\vue-tsc.CMD --noEmit`.
  - Run `frontend\node_modules\.bin\vite.CMD build`.
  - Confirm every UI Lab entrance duration is at or below 240ms.
- **Feel check**:
  - Reload `/ui-lab` at normal speed and at 10% playback speed.
  - Confirm the command bar, manifest, workstation, and evidence establish
    hierarchy without making the page feel blocked.
  - Confirm Observation and Motion Bench move as one workstation surface.
  - Reload repeatedly and confirm the sequence does not become tiring.
- **Done when**: the workstation is fully settled within 300ms after its own
  delayed start and Motion Bench has no nested entrance transform.
