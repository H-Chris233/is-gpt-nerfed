<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import Chip from './Chip.vue';
import Icon from './Icon.vue';
import { useDesktop } from './state';
import { time } from './presentation';
import type { PetLayout } from './types';

const { snapshot, error, connect, perform } = useDesktop();
const win = getCurrentWindow();
const layout = ref<PetLayout>({ expanded: false, chip_x: 0, chip_y: 0 });
const presence = computed(() => snapshot.value?.presence);
let timer: ReturnType<typeof setTimeout> | undefined;
let desired = false;
let changing = false;
let hovered = false;
let dragging = false;
let dragged = false;
let press: { x: number; y: number; pointer: number } | null = null;
let unlisten: UnlistenFn | undefined;
let unscale: UnlistenFn | undefined;
function clearTimer() { if (timer) clearTimeout(timer); timer = undefined; }
async function expand(value: boolean) {
  desired = value;
  if (changing) return;
  changing = true;
  try { layout.value = await invoke<PetLayout>('companion_layout', { expanded: value }); }
  catch (e) { error.value = String(e); desired = layout.value.expanded; }
  finally { changing = false; if (desired !== layout.value.expanded) void expand(desired); }
}
function enter() {
  hovered = true; clearTimer();
  if (!dragging && !press && !layout.value.expanded) timer = setTimeout(() => { void expand(true); }, 200);
}
function leave() {
  hovered = false; clearTimer();
  timer = setTimeout(() => {
    const keyboardFocus = document.hasFocus() && document.activeElement?.matches('.pet-shell :focus-visible');
    if (!hovered && !dragging && !keyboardFocus) void expand(false);
  }, 350);
}
function focusInside(event: FocusEvent) {
  if (layout.value.expanded) clearTimer();
  else if ((event.target as HTMLElement).matches(':focus-visible')) enter();
}
function down(event: PointerEvent) {
  if (event.button !== 0) return;
  dragged = false; clearTimer(); press = { x: event.screenX, y: event.screenY, pointer: event.pointerId };
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
}
async function move(event: PointerEvent) {
  if (!press || dragging || Math.hypot(event.screenX - press.x, event.screenY - press.y) < 5) return;
  dragging = true; dragged = true; clearTimer();
  const element = event.currentTarget as HTMLElement;
  if (element.hasPointerCapture(press.pointer)) element.releasePointerCapture(press.pointer);
  press = null;
  await expand(false);
  try { await win.startDragging(); }
  catch (e) { error.value = String(e); }
  finally { dragging = false; }
}
function release() { press = null; }
function open() { void perform('show_main', { followStatus: true }); }
function chipClick(event: MouseEvent) { if (event.detail === 0 || (!dragged && !dragging)) open(); }
async function menu() { clearTimer(); await invoke('companion_menu').catch(e => { error.value = String(e); }); }
onMounted(async () => {
  unlisten = await listen<PetLayout>('companion-layout', e => { clearTimer(); desired = e.payload.expanded; layout.value = e.payload; });
  unscale = await win.onScaleChanged(() => { void expand(layout.value.expanded); });
  await connect();
});
onUnmounted(() => { clearTimer(); unlisten?.(); unscale?.(); });
</script>

<template>
  <div class="pet-shell" :class="{ expanded: layout.expanded, 'chip-right': layout.chip_x > 100 }" :data-mood="presence?.mood || 'loading'" @pointerenter="enter" @pointerleave="leave" @focusin="focusInside" @focusout="leave" @contextmenu.prevent="menu">
    <section v-if="layout.expanded" class="pet-card">
      <div class="pet-copy"><span class="eyebrow">NERFED <i>/</i> 模型观察</span><h1><i class="status-dot" />{{ presence?.label || '正在连接' }}</h1><p class="pet-model">{{ presence?.model || '等待下一次观察' }}<span v-if="presence?.reference_model"> · 参考 {{ presence.reference_model }}</span></p><p class="pet-time">{{ error || (presence?.finished ? `最近一次 · ${time(presence.finished)}` : presence?.detail) }}</p><button class="pet-open" @click="open">查看详情<Icon name="arrow" :size="14" /></button></div>
      <button class="pet-hide icon-button" :class="{ 'below-chip': layout.chip_x > 100 && layout.chip_y < 1 }" aria-label="收起桌宠" title="收起桌宠" @click="perform('set_companion', { visible: false })"><Icon name="close" :size="13" /></button>
    </section>
    <button class="pet-chip" :style="{ left: `${layout.chip_x}px`, top: `${layout.chip_y}px` }" :aria-label="`桌面芯片：${presence?.label || '正在连接'}。点击查看详情，拖动移动，右键更多操作。`" @pointerdown="down" @pointermove="move" @pointerup="release" @pointercancel="release" @click="chipClick" @focus="enter" @blur="leave"><Chip :mood="presence?.mood || 'loading'" /><i class="pet-status-dot" /></button>
  </div>
</template>
