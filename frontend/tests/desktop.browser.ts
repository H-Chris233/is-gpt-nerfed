import { createApp } from 'vue';
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import App from '../src/App.vue';
import '../src/style.css';
import type { Snapshot } from '../src/types';

// Browser-only fixtures never connect to Codex or start an inference request.
const state: Snapshot = {
  revision: 1, version: '0.6.0',
  settings: { automatic: false, companion: true, interval_minutes: 30, queries: 3, timeout_seconds: 300, notifications: true, codex_bin: '' },
  threads: [
    { id: 'design', title: '优化桌面悬浮窗体验', model: 'gpt-6-astra', effort: 'high', originator: 'Codex Desktop', cwd: 'E:/Projects/nerfed', updated: 1791936000 },
    { id: 'review', title: '检查模型路由与日志', model: 'gpt-6.1-sol', effort: 'high', originator: 'Codex Desktop', cwd: 'E:/Projects/nerfed', updated: 1791934200, reference_model: 'gpt-6-astra' },
    { id: 'plan', title: '规划下一个版本', model: 'new-model', effort: 'medium', originator: 'Codex Desktop', cwd: 'E:/Projects/nerfed', updated: 1791931200 },
  ],
  history: [{ id: 'probe', thread_id: 'design', mode: 'fork', started: '2026-10-14T01:58:00Z', finished: '2026-10-14T02:00:00Z', expected: 'gpt-6-astra', originator: 'Codex Desktop', codex: '0.114.0', account_id: 'fixture', status: 'done', queries: 3, used_outputs: 3, elapsed_s: 28.4, verdict: 'MATCH', direction: null, analysis: null, errors: [], hard_evidence: [] }],
  scans: { design: { turns: 24, ctx: 128000, tier: 'default', checked_at: '2026-10-14T02:00:00Z', active: [], evidence: [] } },
  scan_errors: {}, hooks: { checked_at: '2026-10-14T02:00:00Z', hooks: [], notes: [] }, hook_error: null,
  connection: { path: 'C:/Program Files/Codex/codex.exe', version: '0.114.0' }, account: { id: 'fixture', label: '预览账户 · 模拟数据' },
  defaults: ['gpt-6-astra', 'high', 'Codex Desktop'], task: null, status: '', error: null,
  last_refreshed: '2026-10-14T02:00:00Z', confirm_auto: false, quitting: false,
  presence: { mood: 'match', label: '指纹匹配', title: '', detail: '本次指纹与所选模型一致，可结合实际使用继续观察。', model: 'gpt-6-astra', finished: '2026-10-14T02:00:00Z', session_title: '优化桌面悬浮窗体验', thread_id: 'design', probe_id: 'probe' },
  navigation: null, data_dir: 'C:/Users/Preview/AppData/Local/Nerfed',
};
mockWindows('main');
mockIPC((cmd, args) => {
  if (cmd === 'get_snapshot') return structuredClone(state);
  if (cmd === 'set_companion') state.settings.companion = Boolean((args as { visible: boolean }).visible);
  else if (cmd === 'save_settings') state.settings = structuredClone((args as { settings: Snapshot['settings'] }).settings);
  else if (cmd === 'window_ready') document.body.dataset.ready = 'true';
  else if (cmd === 'start_probe') throw new Error('这是界面预览，不会发起真实检测。');
  else return;
  state.revision++;
  void emit('app-state', structuredClone(state));
}, { shouldMockEvents: true });
createApp(App).mount('#app');
