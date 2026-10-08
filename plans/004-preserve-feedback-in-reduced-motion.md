# 004 — Preserve useful feedback in reduced-motion mode

- **Status**: DONE
- **Commit**: 6c0c85f
- **Severity**: MEDIUM
- **Category**: Accessibility
- **Estimated scope**: 3 files, approximately 35 lines

## Problem

The global reduced-motion rule sets every transition and animation duration to
zero. UI Lab and TalosButton repeat this approach locally. This removes spatial
motion, but also removes useful color, border, opacity, focus, and press-state
feedback.

```css
/* frontend/src/assets/styles/base.css:477 — current */
@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    transition-duration: 0s !important;
    animation-duration: 0s !important;
  }
}

/* frontend/src/ui-lab/UiLabPage.vue:1620 — current */
@media (prefers-reduced-motion: reduce) {
  .command-bar,.manifest,.workbench,.motion-console,.evidence,.specimen-stage,
  .registry-item.selected::after { animation: none!important; }
  .segment button,.slot-anchor { transition-duration: 0ms!important; }
}

/* frontend/src/ui/components/TalosButton.vue:178 — current */
@media (prefers-reduced-motion: reduce) {
  .talos-button,
  .talos-button__signal {
    animation: none !important;
    transition-duration: 0ms !important;
  }
}
```

## Target

Remove spatial movement while retaining 150ms opacity/color/border feedback.
The global rule must not wipe every transition. Use exact selectors so
components can keep non-spatial feedback.

```css
/* frontend/src/assets/styles/base.css — target */
@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    scroll-behavior: auto !important;
  }
}
```

```css
/* frontend/src/ui-lab/UiLabPage.vue — target */
@media (prefers-reduced-motion: reduce) {
  .command-bar,
  .manifest,
  .workbench,
  .motion-console,
  .evidence,
  .specimen-assembly,
  .registry-item.selected::after {
    animation: none !important;
  }

  .preview,
  .specimen-visual {
    transition: opacity 150ms cubic-bezier(0.23, 1, 0.32, 1) !important;
  }

  .segment button,
  .slot-anchor {
    transition:
      opacity 150ms cubic-bezier(0.23, 1, 0.32, 1),
      color 150ms ease,
      border-color 150ms ease,
      background-color 150ms ease !important;
  }
}
```

```css
/* frontend/src/ui/components/TalosButton.vue — target */
@media (prefers-reduced-motion: reduce) {
  .talos-button__signal {
    animation: none !important;
  }

  .talos-button {
    transition:
      border-color 150ms ease,
      background-color 150ms ease,
      color 150ms ease,
      opacity 150ms ease;
  }

  .talos-button:active:not(:disabled) {
    transform: none;
  }

  .talos-button--execution-primary:hover:not(:disabled) {
    box-shadow: inset 4px 0 0 var(--accent);
  }
}
```

## Repo conventions to follow

- The application already detects `prefers-reduced-motion` in
  `UiLabPage.vue:1105` for animation-engine playback.
- Keep opacity/color feedback; remove translation, scaling, sweep, and layout
  movement.
- Use `ease` for color changes and exact
  `cubic-bezier(0.23, 1, 0.32, 1)` for opacity.

## Steps

1. Replace the global duration-zeroing rule with only
   `scroll-behavior: auto !important`.
2. Update UI Lab's local reduced-motion block to suppress entrances and sweep
   animation but retain the target opacity/color/border transitions.
3. Update TalosButton's block to remove sweep and press scaling while retaining
   the target non-spatial transitions.
4. Search all changed selectors to ensure no later reduced-motion rule restores
   transform animation.
5. Do not alter `motion-engine.ts`; its explicit System/Reduced laboratory
   policy is an animation-research control rather than normal application UI.

## Boundaries

- Do NOT remove focus outlines or disabled-state opacity.
- Do NOT change JS media-query handling or the Motion Policy control.
- Do NOT create a blanket `transition: none`.
- Do NOT modify unrelated page-specific reduced-motion policies.
- If the cited code has drifted since commit `6c0c85f`, STOP and report.

## Verification

- **Mechanical**:
  - Run `frontend\node_modules\.bin\vue-tsc.CMD --noEmit`.
  - Run `frontend\node_modules\.bin\vite.CMD build`.
  - Search the changed files and confirm they contain no blanket
    `transition-duration: 0s !important`.
- **Feel check**:
  - Enable `prefers-reduced-motion: reduce` in browser rendering settings.
  - Reload UI Lab: command bar, manifest, workstation, Evidence, and specimen
    must appear without translation or scale.
  - Toggle theme, slot guides, and component state; color, opacity, and borders
    must still communicate the change in approximately 150ms.
  - Press and hover TalosButton; it must not scale or sweep, but border/color
    feedback must remain.
  - Return to no-preference and confirm the normal motion sequence is restored.
- **Done when**: reduced-motion mode has no spatial entrance, press scale, or
  signal sweep, while focus, color, border, and opacity state feedback remains
  perceptible.
