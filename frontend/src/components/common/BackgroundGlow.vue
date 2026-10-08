<script setup lang="ts">
// Synthetic Aurora — 3 large orbs + screen blend + dot grid + scanlines
// Colors: cyan, fuchsia, emerald — reference: React Framer Motion patterns
</script>

<template>
  <div class="bg-aurora" aria-hidden="true">
    <!-- Orb 1: cyan, top-left, wide drift -->
    <div class="aurora-orb aurora-orb--cyan" />
    <!-- Orb 2: fuchsia, right-center, drift left -->
    <div class="aurora-orb aurora-orb--fuchsia" />
    <!-- Orb 3: emerald, bottom, drift up -->
    <div class="aurora-orb aurora-orb--emerald" />
    <!-- Dot grid -->
    <div class="aurora-dots" />
    <!-- Scanlines -->
    <div class="aurora-scanlines" />
  </div>
</template>

<style scoped>
.bg-aurora {
  position: fixed;
  inset: 0;
  z-index: 0;
  pointer-events: none;
  overflow: hidden;
}

/* ── Aurora orbs — large, blurred, screen-blended ────────────────── */
.aurora-orb {
  position: absolute;
  border-radius: 9999px;
  mix-blend-mode: screen;
  filter: blur(60px);
  will-change: transform, opacity;
}

/* Cyan — top-left, 70vw */
.aurora-orb--cyan {
  width: 70%;
  height: 70%;
  background: var(--aurora-cyan);
  opacity: 0.40;
  top: -10%;
  left: -10%;
  animation: aurora-cyan 17s ease-in-out infinite;
}

/* Fuchsia — right-center, 60vw */
.aurora-orb--fuchsia {
  width: 60%;
  height: 80%;
  background: var(--aurora-fuchsia);
  opacity: 0.40;
  top: 10%;
  right: -10%;
  animation: aurora-fuchsia 25s ease-in-out infinite;
}

/* Emerald — bottom, 70vw */
.aurora-orb--emerald {
  width: 70%;
  height: 60%;
  background: var(--aurora-emerald);
  opacity: 0.40;
  bottom: -10%;
  left: 10%;
  animation: aurora-emerald 21s ease-in-out infinite;
}

/* ── Keyframes — linear sweep, large amplitude ──────────────────── */
@keyframes aurora-cyan {
  0%, 100% { transform: translate(0, 0) scale(1);      opacity: 0.40; }
  25%      { transform: translate(150px, 80px) scale(1.3); opacity: 0.70; }
  50%      { transform: translate(80px, 20px) scale(1.1);  opacity: 0.50; }
  75%      { transform: translate(-30px, 100px) scale(1.2); opacity: 0.60; }
}

@keyframes aurora-fuchsia {
  0%, 100% { transform: translate(0, 0) scale(1.2);      opacity: 0.30; }
  25%      { transform: translate(-120px, 150px) scale(1);   opacity: 0.60; }
  50%      { transform: translate(-60px, 70px) scale(1.15);   opacity: 0.45; }
  75%      { transform: translate(-180px, 20px) scale(0.9);   opacity: 0.55; }
}

@keyframes aurora-emerald {
  0%, 100% { transform: translate(0, 0) scale(1);       opacity: 0.40; }
  25%      { transform: translate(70px, -80px) scale(1.25); opacity: 0.65; }
  50%      { transform: translate(100px, -150px) scale(1.4); opacity: 0.80; }
  75%      { transform: translate(50px, -60px) scale(1.2);    opacity: 0.55; }
}

/* ── Dot grid — toggleable via --show-dots (1=on, 0=off) ────────── */
.aurora-dots {
  position: absolute;
  inset: 0;
  background-image: radial-gradient(var(--aurora-dot-color) 1px, transparent 1px);
  background-size: 32px 32px;
  opacity: calc(var(--aurora-dot-opacity) * var(--show-dots, 1));
}

/* ── Scanlines — toggleable via --show-scanlines (1=on, 0=off) ──── */
.aurora-scanlines {
  position: absolute;
  inset: 0;
  background: linear-gradient(
    transparent 50%,
    var(--aurora-scanline-color) 50%
  );
  background-size: 100% 4px;
  opacity: var(--show-scanlines, 1);
}

@media (prefers-reduced-motion: reduce) {
  .aurora-orb { animation: none !important; }
}
</style>
