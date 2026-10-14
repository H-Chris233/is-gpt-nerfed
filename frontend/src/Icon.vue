<script setup lang="ts">
withDefaults(defineProps<{ name: string; size?: number }>(), { size: 18 });
const paths: Record<string, string[]> = {
  processor: ['M7 5h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2Z', 'M9 9h6v6H9z', 'M9 2v3m6-3v3M9 19v3m6-3v3M2 9h3m-3 6h3M19 9h3m-3 6h3'],
  observe: ['M12 3v3m0 12v3M3 12h3m12 0h3', 'M8 5.1a8 8 0 1 0 10.9 10.9', 'M12 8a4 4 0 1 0 4 4', 'm16 3 1.3 3.7L21 8l-3.7 1.3L16 13l-1.3-3.7L11 8l3.7-1.3L16 3Z'],
  clock: ['M20.5 12a8.5 8.5 0 1 1-2.4-5.9', 'M12 7v5l3 2', 'M20.5 4v5h-5'],
  settings: ['M5 3v6m0 4v8M12 3v10m0 4v4M19 3v2m0 4v12', 'M2.5 9h5v4h-5zM9.5 13h5v4h-5zM16.5 5h5v4h-5z'],
  search: ['M10.5 17a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13Zm5-1 5 5'],
  refresh: ['M20 8A8.5 8.5 0 0 0 5 5.5L2 9m0-6v6h6', 'M4 16a8.5 8.5 0 0 0 15 2.5l3-3.5m0 6v-6h-6'],
  chip: ['M7 5h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2Z', 'M9 2v3m6-3v3M9 19v3m6-3v3M2 9h3m-3 6h3M19 9h3m-3 6h3M9 10v1m6-1v1m-5 3h4'],
  close: ['m6 6 12 12M18 6 6 18'], minimize: ['M5 12h14'], maximize: ['M6 6h12v12H6z'],
  chevron: ['m9 5 7 7-7 7'], arrow: ['M4 12h16m-6-6 6 6-6 6'],
  play: ['m9 5 11 7-11 7V5Z'], check: ['m5 12 4 4L19 6'],
  info: ['M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20ZM12 11v6m0-10v.1'],
  alert: ['m10.3 3.5-8 14A2 2 0 0 0 4 20h16a2 2 0 0 0 1.7-2.5l-8-14a2 2 0 0 0-3.4 0ZM12 8v5m0 3v.1'],
  shield: ['M12 2 3 6v6c0 5 9 10 9 10s9-5 9-10V6l-9-4Z', 'm8 11 3 3 5-5'],
  folder: ['M3 5h6l2 3h10v11H3V5Z'], stop: ['M7 7h10v10H7z'],
  eye: ['M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z', 'M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z'],
  log: ['M5 3h14v18H5V3Zm4 5h6m-6 4h6m-6 4h3'],
  spark: ['m12 2 2.6 7.4L22 12l-7.4 2.6L12 22l-2.6-7.4L2 12l7.4-2.6L12 2Z'],
  bell: ['M6 8a6 6 0 0 1 12 0c0 7 3 7 3 9H3c0-2 3-2 3-9Zm4 12h4'],
  exit: ['M10 3H4v18h6m5-15 6 6-6 6m-6-6h12'],
};
</script>
<template><svg :width="size" :height="size" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path v-for="(d, i) in paths[name] || paths.info" :key="i" :d="d" /></svg></template>
