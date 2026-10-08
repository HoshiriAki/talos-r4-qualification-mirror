# UI Lab animation improvement plans

These plans were authored against commit `6c0c85f`. They are implementation
specifications only; source code has not been modified.

| Plan | Title | Severity | Status |
|---|---|---:|---|
| [001](001-separate-preview-scale-from-entry-motion.md) | Separate preview scaling from specimen entry motion | HIGH | DONE |
| [002](002-move-button-sweep-to-transform.md) | Move the button signal sweep to transform | HIGH | DONE |
| [003](003-tighten-ui-lab-entry-sequence.md) | Tighten the UI Lab entry sequence | MEDIUM | DONE |
| [004](004-preserve-feedback-in-reduced-motion.md) | Preserve useful feedback in reduced-motion mode | MEDIUM | DONE |

## Recommended execution order

1. **001** — fixes the active transform ownership conflict and establishes the
   correct animation layer.
2. **002** — removes the highest-cost per-frame component animation.
3. **003** — tightens the overall workstation entrance after transform
   ownership is clear.
4. **004** — applies the final accessibility behavior to the selectors created
   or renamed by plans 001–003.

## Dependencies

- Plan 003 assumes plan 001 moved acquisition animation from
  `.specimen-stage` to `.specimen-assembly`.
- Plan 004 must run last because its reduced-motion selectors reference
  `.specimen-assembly` from plan 001 and the final entrance declarations from
  plan 003.
- Plan 002 is technically independent, but executing it before plan 004 makes
  the reduced-motion verification definitive.

## Execution

Execute each plan in order with:

```text
improve-animations execute plans/001-separate-preview-scale-from-entry-motion.md
improve-animations execute plans/002-move-button-sweep-to-transform.md
improve-animations execute plans/003-tighten-ui-lab-entry-sequence.md
improve-animations execute plans/004-preserve-feedback-in-reduced-motion.md
```
