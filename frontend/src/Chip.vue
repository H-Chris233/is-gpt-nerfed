<script setup lang="ts">
import { computed } from 'vue';
import type { Mood } from './types';
import ok from '../../macos/Resources/face-ok.png';
import warn from '../../macos/Resources/face-warn.png';
import alert from '../../macos/Resources/face-alert.png';
const props = withDefaults(defineProps<{ mood?: Mood; animated?: boolean }>(), { mood: 'idle', animated: true });
const image = computed(() => props.mood === 'alert' ? alert : ['unknown', 'warning', 'offline'].includes(props.mood) ? warn : ok);
</script>
<template><span class="chip-art" :class="[{ animated }, `chip-${mood}`]" aria-hidden="true"><span v-if="mood === 'busy' || mood === 'loading'" class="chip-orbit" /><img :src="image" alt="" draggable="false" /><span class="chip-floor" /></span></template>
