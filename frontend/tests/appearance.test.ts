import assert from 'node:assert/strict';
import { test } from 'node:test';
import 'vue';

test('appearance persists, follows the system and synchronizes windows without changing detection settings', async () => {
  const values = new Map([['nerfed.appearance', 'invalid']]);
  let storageFails = false;
  const storage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { if (storageFails) throw new Error('storage unavailable'); values.set(key, value); },
  };
  const media = Object.assign(new EventTarget(), { matches: true });
  const page = Object.assign(new EventTarget(), { localStorage: storage, matchMedia: () => media });
  const root = { dataset: {} as Record<string, string> };
  Object.assign(globalThis, { window: page, document: { documentElement: root } });
  const { appearance, appearanceError, setAppearance } = await import('../src/appearance.ts');
  assert.equal(appearance.value, 'system');
  assert.equal(root.dataset.theme, 'dark', 'First render must respect a dark system preference');
  setAppearance('light');
  assert.equal(root.dataset.theme, 'light');
  assert.equal(values.get('nerfed.appearance'), 'light');
  media.dispatchEvent(new Event('change'));
  assert.equal(root.dataset.theme, 'light', 'Explicit preferences must ignore system changes');
  setAppearance('system');
  media.matches = false;
  media.dispatchEvent(new Event('change'));
  assert.equal(root.dataset.theme, 'light');
  media.matches = true;
  media.dispatchEvent(new Event('change'));
  assert.equal(root.dataset.theme, 'dark');
  page.dispatchEvent(Object.assign(new Event('storage'), { storageArea: storage, key: 'nerfed.appearance', newValue: 'light' }));
  assert.equal(appearance.value, 'light');
  assert.equal(root.dataset.theme, 'light', 'Other windows must receive the new appearance');
  page.dispatchEvent(Object.assign(new Event('storage'), { storageArea: storage, key: 'unrelated', newValue: 'dark' }));
  assert.equal(root.dataset.theme, 'light', 'Unrelated storage changes must be ignored');
  page.dispatchEvent(Object.assign(new Event('storage'), { storageArea: storage, key: null, newValue: null }));
  assert.equal(appearance.value, 'system', 'Clearing storage must return to the system preference');
  assert.equal(root.dataset.theme, 'dark');
  storageFails = true;
  setAppearance('light');
  assert.equal(root.dataset.theme, 'light', 'Storage failure must not prevent applying an appearance');
  assert.match(appearanceError.value, /未能保存/);
  storageFails = false;
  setAppearance('dark');
  assert.equal(appearanceError.value, '');
  assert.equal(values.size, 1, 'Only the appearance preference may be stored');
});
