import { createApp, nextTick } from 'vue';
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import Companion from '../src/Companion.vue';
import '../src/style.css';
import type { Presence, PetLayout } from '../src/types';

// Browser-only fixture: no Codex connection or inference requests are made.
const params = new URLSearchParams(location.search);
const scenario = params.get('state') || 'idle';
const variants: Record<string, Partial<Presence>> = {
  idle: { mood: 'idle', label: '被动观察中', detail: '本地日志未发现变化；被动观察不代表模型身份已验证。' },
  match: { mood: 'match', label: '指纹匹配', detail: '本次指纹与所选模型一致，可结合实际使用继续观察。' },
  busy: { mood: 'busy', label: '正在检测', detail: '通过临时会话生成样本，原会话会保持完整。', finished: '' },
  loading: { mood: 'loading', label: '正在连接', model: '', detail: '正在读取本地会话与连接状态。', finished: '' },
  unknown: { mood: 'unknown', label: '模型未收录', detail: '所选模型尚未被指纹库收录，暂时无法判断是否降级。' },
  warning: { mood: 'warning', label: '有待核实', detail: '目前证据不足以确认模型变化，可以稍后重新检测。' },
  alert: { mood: 'alert', label: '发现降级证据', detail: '本地日志记录了降级证据，请查看具体变化。' },
  offline: { mood: 'offline', label: '检测未完成', detail: '没有获得有效结论，请查看错误原因后重试。' },
  cancelled: { mood: 'cancelled', label: '检测已取消', detail: '这次检测已经取消，没有得出结论。' },
  reference: { mood: 'match', label: '参考指纹匹配', model: 'gpt-6.1-sol', reference_model: 'gpt-6-astra', detail: '6.1 Sol 复用 Astra 的指纹参考，不代表两者模型身份相同。' },
  long: { mood: 'warning', label: '参考指纹有待核实', session_title: '这是一段很长的会话标题，用于确认悬浮窗不会因标题溢出而破坏布局', model: 'a-very-long-unlisted-model-name-with-extra-version-details', reference_model: 'gpt-6-astra', detail: '当前样本尚不足以确认与 Astra 共享参考的匹配情况。具体信息可以在详情中查看。' },
};
let revision = 1;
const presence: Presence = { mood: 'idle', label: '准备就绪', title: '', detail: '', model: 'gpt-6-astra', session_title: '为灵感，留一点空间', thread_id: 'fixture', probe_id: null, finished: '2026-10-14T02:42:00Z', ...variants[scenario] };
const calls: { cmd: string; args: unknown }[] = [];
let layout: PetLayout = { expanded: params.get('collapsed') !== '1', chip_x: 0, chip_y: 0 };
function position(expanded: boolean) {
  layout = { expanded, chip_x: expanded && params.has('right') ? 248 : 0, chip_y: expanded && params.has('bottom') ? 88 : 0 };
  if (window.frameElement) {
    const frame = window.frameElement as HTMLIFrameElement;
    frame.style.width = expanded ? '344px' : '96px';
    frame.style.height = expanded ? '184px' : '96px';
  }
  return layout;
}
mockWindows('companion');
mockIPC((cmd, args) => {
  calls.push({ cmd, args });
  if (cmd === 'get_snapshot') return { revision, presence };
  if (cmd === 'companion_layout') return position(Boolean((args as { expanded: boolean }).expanded));
  if (cmd === 'window_ready') queueMicrotask(async () => {
    await emit('companion-layout', position(layout.expanded));
    document.body.dataset.ready = 'true';
    if (params.has('error')) await emit('desktop-error', '连接已中断，请打开详情检查。');
    if (params.has('test')) void check();
  });
}, { shouldMockEvents: true });
createApp(Companion).mount('#app');

async function check() {
  let assertions = 0;
  function assert(value: unknown, message: string) { if (!value) throw new Error(message); assertions++; }
  const settle = () => new Promise(resolve => setTimeout(resolve, 450));
  const query = <T extends HTMLElement>(selector: string) => document.querySelector<T>(selector)!;
  try {
    await nextTick();
    const shell = query('.pet-shell');
    const chip = query<HTMLButtonElement>('.pet-chip');
    assert(shell.dataset.mood === presence.mood, 'Presence mood must be preserved');
    assert(query('h1').textContent?.includes(presence.label), 'Backend verdict must remain visible');
    assert(query('.pet-detail').textContent === presence.detail, 'Full explanation must remain accessible');
    assert(chip.compareDocumentPosition(query('.pet-detail')) & Node.DOCUMENT_POSITION_FOLLOWING, 'Tab order must lead from the chip into the expanded panel');
    for (const node of document.querySelectorAll<HTMLElement>('.pet-copy, .pet-footer, .pet-open, .pet-model, h1')) {
      const box = node.getBoundingClientRect();
      assert(box.left >= 0 && box.right <= 344 && box.bottom <= 184, `${node.className || node.tagName} must fit the native window`);
      assert(node.scrollWidth <= node.clientWidth + 1, `${node.className || node.tagName} must not overflow horizontally`);
    }
    for (const x of [0, 248]) for (const y of [0, 88]) {
      await emit('companion-layout', { expanded: true, chip_x: x, chip_y: y });
      await nextTick();
      const a = chip.getBoundingClientRect();
      const b = query('.pet-hide').getBoundingClientRect();
      assert(a.right <= b.left || a.left >= b.right || a.bottom <= b.top || a.top >= b.bottom, 'Hide control must not overlap the drag target');
    }
    await emit('companion-layout', position(true));
    query<HTMLButtonElement>('.pet-open').click();
    await nextTick();
    assert(calls.some(c => c.cmd === 'show_main' && (c.args as { followStatus: boolean }).followStatus), 'Details must follow the companion session');
    query<HTMLButtonElement>('.pet-hide').click();
    await nextTick();
    assert(calls.some(c => c.cmd === 'set_companion' && (c.args as { visible: boolean }).visible === false), 'Hide must call the companion setting');
    shell.dispatchEvent(new PointerEvent('pointerleave'));
    await settle();
    assert(!document.querySelector('.pet-card'), 'Leaving must collapse the panel');
    shell.dispatchEvent(new PointerEvent('pointerenter'));
    await settle();
    assert(document.querySelector('.pet-card'), 'Hovering must expand the panel');
    chip.dispatchEvent(new MouseEvent('click', { detail: 0 }));
    assert(calls.filter(c => c.cmd === 'show_main').length === 2, 'Keyboard activation must open details');
    await emit('desktop-error', '测试连接错误');
    await nextTick();
    assert(shell.dataset.mood === 'offline' && query('h1').textContent?.includes('操作遇到问题'), 'Errors must not look like a successful result');
    assert(query('[role=status]').textContent?.includes('测试连接错误'), 'Errors must be announced accessibly');
    await emit('app-state', { revision: ++revision, presence: { ...presence, label: '更新后的结果' } });
    await emit('app-state', { revision: 0, presence: { ...presence, label: '过期结果' } });
    query<HTMLButtonElement>('.pet-open').click();
    await nextTick();
    assert(query('h1').textContent?.includes('更新后的结果'), 'Old events must not replace a newer result');
    assert(!calls.some(c => /probe|scan|save_settings/.test(c.cmd)), 'Viewing the companion must not start inference or change detection settings');
    document.body.dataset.result = `PASS ${assertions}`;
    await emit('app-state', { revision: ++revision, presence });
  } catch (error) {
    document.body.dataset.result = `FAIL ${error}`;
    console.error(error);
  }
}
