<script setup lang="ts">
import Icon from './Icon.vue';
import type { Mood } from './types';
withDefaults(defineProps<{ mood?: Mood; animated?: boolean }>(), { mood: 'idle', animated: true });
</script>
<template><span class="chip-art" :class="[{ animated }, `chip-${mood}`]" aria-hidden="true"><Icon name="processor" :size="48" /><i class="chip-status" /></span></template>
