import { onUnmounted, ref, shallowRef } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Snapshot } from './types';
import { newer } from './presentation';

export function useDesktop() {
  const snapshot = shallowRef<Snapshot | null>(null);
  const error = ref('');
  const pending = ref(0);
  const unlisten: UnlistenFn[] = [];
  function accept(next: Snapshot) { if (newer(snapshot.value, next)) snapshot.value = next; }
  async function sync() {
    try { accept(await invoke<Snapshot>('get_snapshot')); }
    catch (e) { error.value = String(e); }
  }
  function visibility() { if (!document.hidden) void sync(); }
  async function connect() {
    try {
      unlisten.push(await listen<Snapshot>('app-state', event => accept(event.payload)));
      unlisten.push(await listen<string>('desktop-error', event => { error.value = event.payload; }));
      await sync();
      document.addEventListener('visibilitychange', visibility);
      window.addEventListener('focus', visibility);
    } catch (e) { error.value = String(e); }
    finally { await invoke('window_ready').catch(e => { error.value = String(e); }); }
  }
  async function perform(name: string, args?: Record<string, unknown>): Promise<boolean> {
    pending.value++; error.value = '';
    try { await invoke(name, args); return true; }
    catch (e) { error.value = String(e); return false; }
    finally { pending.value--; }
  }
  onUnmounted(() => { unlisten.forEach(fn => fn()); document.removeEventListener('visibilitychange', visibility); window.removeEventListener('focus', visibility); });
  return { snapshot, error, pending, connect, perform };
}
