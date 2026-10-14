import { ref } from 'vue';

export type Appearance = 'system' | 'light' | 'dark';
const key = 'nerfed.appearance';
const system = window.matchMedia('(prefers-color-scheme: dark)');
function parse(value: string | null): Appearance { return value === 'light' || value === 'dark' ? value : 'system'; }
function read(): Appearance {
  try { return parse(window.localStorage.getItem(key)); }
  catch { return 'system'; }
}
export const appearance = ref<Appearance>(read());
export const appearanceError = ref('');
export const appearanceOptions: { value: Appearance; label: string }[] = [
  { value: 'light', label: '浅色' }, { value: 'dark', label: '深色' }, { value: 'system', label: '跟随系统' },
];
function apply() {
  document.documentElement.dataset.theme = appearance.value === 'system' ? system.matches ? 'dark' : 'light' : appearance.value;
}
export function setAppearance(value: Appearance) {
  appearance.value = value;
  appearanceError.value = '';
  apply();
  try { window.localStorage.setItem(key, value); }
  catch { appearanceError.value = '外观已应用，但未能保存。下次启动将使用之前的设置。'; }
}
// Both native windows share the same WebView profile and origin.
window.addEventListener('storage', event => {
  if (event.storageArea === window.localStorage && (event.key === key || event.key === null)) {
    appearance.value = parse(event.newValue);
    appearanceError.value = '';
    apply();
  }
});
system.addEventListener('change', apply);
apply();
