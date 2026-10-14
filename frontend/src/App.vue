<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import Chip from './Chip.vue';
import Icon from './Icon.vue';
import ProbeEvidence from './ProbeEvidence.vue';
import { useDesktop } from './state';
import { duration, effortOptions, passive, sessionVerdict, time, verdict } from './presentation';
import { appearance, appearanceError, appearanceOptions, setAppearance } from './appearance';
import type { Probe, Settings } from './types';
import license from '../../LICENSE?raw';
import modeltraceLicense from '../../plugin/assets/modeltrace/LICENSE-ModelTrace.txt?raw';

const { snapshot: s, error, pending, connect, perform } = useDesktop();
const win = getCurrentWindow();
type ResizeDirection = Parameters<typeof win.startResizeDragging>[0];
const page = ref<'observe' | 'history' | 'settings'>('observe');
const selected = ref('');
const filter = ref('');
const historyFilter = ref('all');
const openHistory = ref('');
const dismissedError = ref('');
const toast = ref('');
const dirty = ref(false);
const clock = ref(Date.now());
let clockTimer: ReturnType<typeof setInterval> | undefined;
let toastTimer: ReturnType<typeof setTimeout> | undefined;
const draft = reactive<Settings>({ automatic: false, companion: false, interval_minutes: 30, queries: 3, timeout_seconds: 300, notifications: true, codex_bin: '' });
const fresh = reactive({ model: '', effort: 'high', originator: 'Codex Desktop' });
const consent = ref<HTMLDialogElement>();
const about = ref<HTMLDialogElement>();
const consentSource = ref<'settings' | 'tray'>('settings');
const consentSettings = computed(() => consentSource.value === 'tray' ? s.value?.settings || draft : draft);
const session = computed(() => s.value?.threads.find(t => t.id === selected.value));
const sessions = computed(() => s.value?.threads.filter(t => `${t.title} ${t.model}`.toLocaleLowerCase().includes(filter.value.toLocaleLowerCase())) || []);
const probe = computed(() => s.value?.history.find(p => p.thread_id === selected.value));
const scan = computed(() => s.value?.scans[selected.value]);
const scanError = computed(() => s.value?.scan_errors[selected.value]);
const passiveView = computed(() => passive(scan.value, scanError.value));
const busy = computed(() => !!s.value?.task || pending.value > 0 || !!s.value?.quitting);
const detecting = computed(() => s.value?.task?.kind === 'probe');
const elapsed = computed(() => duration(clock.value / 1000 - (s.value?.task?.started || clock.value / 1000)));
const taskHere = computed(() => detecting.value && s.value?.task?.thread_id === selected.value);
const visibleError = computed(() => { const value = error.value || s.value?.error || ''; return value === dismissedError.value ? '' : value; });
function sessionView(id: string) { return sessionVerdict(s.value, id); }
const view = computed(() => sessionView(selected.value));
const history = computed(() => s.value?.history.filter(p => historyFilter.value === 'all' || (historyFilter.value === 'attention'
  ? ['alert', 'warning', 'unknown'].includes(verdict(p, s.value?.account.id || '').mood)
  : ['failed', 'cancelled'].includes(p.status) || p.verdict === 'INVALID')) || []);
const directions: ResizeDirection[] = ['North', 'South', 'East', 'West', 'NorthEast', 'NorthWest', 'SouthEast', 'SouthWest'];

watch(() => s.value?.threads, threads => { if (threads && !threads.some(t => t.id === selected.value)) selected.value = threads[0]?.id || ''; });
watch(() => s.value?.settings, cfg => { if (!cfg) return; if (!dirty.value) Object.assign(draft, cfg); else draft.companion = cfg.companion; });
watch(() => s.value?.defaults.join('|'), () => {
  if (s.value && !fresh.model) {
    fresh.model = s.value.defaults[0]; fresh.effort = effortOptions.includes(s.value.defaults[1]) ? s.value.defaults[1] : 'high';
    fresh.originator = ['Codex Desktop', 'codex_cli_rs'].includes(s.value.defaults[2]) ? s.value.defaults[2] : 'Codex Desktop';
  }
});
watch(() => s.value?.navigation?.revision, () => {
  const nav = s.value?.navigation;
  if (!nav) return;
  filter.value = '';
  if (nav.thread_id && s.value?.threads.some(t => t.id === nav.thread_id)) { selected.value = nav.thread_id; page.value = 'observe'; }
  else if (nav.probe_id) { page.value = 'history'; historyFilter.value = 'all'; openHistory.value = nav.probe_id; }
  else page.value = 'observe';
});
watch(detecting, active => {
  if (clockTimer) clearInterval(clockTimer);
  clock.value = Date.now();
  if (active) clockTimer = setInterval(() => { clock.value = Date.now(); }, 1000);
});
watch(() => s.value?.confirm_auto, async pending => {
  if (pending) { consentSource.value = 'tray'; await nextTick(); if (!consent.value?.open) consent.value?.showModal(); }
});
async function act(name: string, args?: Record<string, unknown>) { dismissedError.value = ''; return perform(name, args); }
function notify(message: string) { toast.value = message; if (toastTimer) clearTimeout(toastTimer); toastTimer = setTimeout(() => { toast.value = ''; }, 3000); }
async function startProbe() { if (session.value) await act('start_probe', { target: { kind: 'thread', id: session.value.id } }); }
async function save(confirmed = false) {
  if (!s.value) return false;
  const ok = await act('save_settings', { settings: { ...draft, companion: s.value.settings.companion }, confirmAutomatic: confirmed });
  if (ok) { dirty.value = false; notify('偏好设置已保存'); }
  return ok;
}
function requestAuto() {
  if (draft.automatic) { draft.automatic = false; dirty.value = true; }
  else { consentSource.value = 'settings'; consent.value?.showModal(); }
}
async function acceptAuto() {
  if (consentSource.value === 'tray') {
    if (await act('confirm_automatic', { confirmed: true })) { consent.value?.close(); notify('自动检测已开启'); }
  } else {
    const previous = draft.automatic; draft.automatic = true;
    if (await save(true)) consent.value?.close(); else draft.automatic = previous;
  }
}
async function dismissConsent() { consent.value?.close(); if (consentSource.value === 'tray') await act('confirm_automatic', { confirmed: false }); }
function historyTitle(p: Probe) { return s.value?.threads.find(t => t.id === p.thread_id)?.title || (p.mode === 'fresh' ? '全新会话' : '历史会话'); }
async function resize(event: PointerEvent, direction: ResizeDirection) { if (event.button === 0) await win.startResizeDragging(direction).catch(e => { error.value = String(e); }); }
onMounted(connect);
onUnmounted(() => { if (clockTimer) clearInterval(clockTimer); if (toastTimer) clearTimeout(toastTimer); });
</script>

<template>
  <div class="app-shell" :class="{ 'is-busy': detecting }">
    <div v-for="direction in directions" :key="direction" class="resize-edge" :class="direction" @pointerdown="resize($event, direction)" aria-hidden="true" />
    <header class="titlebar" data-tauri-drag-region>
      <div class="wordmark" data-tauri-drag-region><span class="brand-mark" data-tauri-drag-region><Icon name="observe" :size="22" /></span><span data-tauri-drag-region>nerfed<span class="brand-dot">.</span></span></div>
      <nav class="navigation" aria-label="主导航">
        <button :class="{ active: page === 'observe' }" :aria-current="page === 'observe' ? 'page' : undefined" @click="page = 'observe'"><Icon name="observe" :size="16" />观察台</button>
        <button :class="{ active: page === 'history' }" :aria-current="page === 'history' ? 'page' : undefined" @click="page = 'history'"><Icon name="clock" :size="16" />时间线</button>
        <button :class="{ active: page === 'settings' }" :aria-current="page === 'settings' ? 'page' : undefined" @click="page = 'settings'"><Icon name="settings" :size="16" />偏好设置</button>
      </nav>
      <div class="drag-space" data-tauri-drag-region />
      <button class="pet-toggle" :class="{ active: s?.settings.companion }" :aria-pressed="s?.settings.companion || false" :disabled="!s || s.quitting" @click="act('set_companion', { visible: !s?.settings.companion })" :title="s?.settings.companion ? '收起桌面芯片' : '让芯片陪在桌面'"><Icon name="chip" :size="17" /><span>桌面芯片</span><i v-if="s?.settings.companion" /></button>
      <div class="window-controls"><button title="最小化" aria-label="最小化" @click="win.minimize()"><Icon name="minimize" :size="14" /></button><button title="最大化 / 还原" aria-label="最大化或还原" @click="win.toggleMaximize()"><Icon name="maximize" :size="12" /></button><button class="close-window" title="收起到托盘" aria-label="关闭窗口并收起到托盘" @click="win.close()"><Icon name="close" :size="15" /></button></div>
    </header>

    <div v-if="visibleError" class="error-banner" role="alert"><Icon name="alert" :size="17" /><span>{{ visibleError }}</span><button class="icon-button" aria-label="收起错误提示" @click="dismissedError = visibleError"><Icon name="close" :size="15" /></button></div>

    <main v-if="s" class="main-content">
      <section v-if="page === 'observe'" class="observatory">
        <aside class="session-rail">
          <div class="rail-heading"><div><span class="eyebrow">YOUR SESSIONS</span><h2>近期会话 <small>{{ s.threads.length }}</small></h2></div><button class="icon-button" title="刷新会话和连接" aria-label="刷新会话和连接" :disabled="busy" @click="act('refresh')"><Icon name="refresh" :size="16" :class="{ spin: s.task?.kind === 'refresh' }" /></button></div>
          <label class="search-box"><Icon name="search" :size="16" /><input v-model="filter" type="search" placeholder="搜索会话或模型" aria-label="搜索会话或模型" spellcheck="false" /><kbd v-if="!filter">⌕</kbd></label>
          <div class="session-list">
            <button v-for="t in sessions" :key="t.id" class="session-row" :class="{ selected: selected === t.id }" :aria-pressed="selected === t.id" @click="selected = t.id">
              <span class="session-row-top"><span class="session-title" :title="t.title">{{ t.title || '未命名会话' }}</span><Icon name="chevron" :size="13" /></span>
              <span class="session-model">{{ t.model }} <i>·</i> {{ t.effort }}</span>
              <span class="session-row-bottom"><span class="status-text" :data-mood="sessionView(t.id).mood"><i class="status-dot" />{{ sessionView(t.id).label }}</span><time>{{ time(t.updated) }}</time></span>
              <span class="session-row-bottom" :title="s.scans[t.id]?.checked_at ? `最近成功扫描 · ${time(s.scans[t.id].checked_at, true)}` : '后台空闲时每 30 秒自动扫描本地日志'"><span class="status-text" :data-mood="passive(s.scans[t.id], s.scan_errors[t.id]).mood"><Icon name="log" :size="11" />{{ passive(s.scans[t.id], s.scan_errors[t.id]).label }}</span><span>{{ s.scans[t.id] ? `${s.scan_errors[t.id] ? '上次 ' : ''}${s.scans[t.id].turns} 轮` : '零额度' }}</span></span>
            </button>
            <div v-if="!sessions.length && s.task?.kind === 'refresh' && !s.last_refreshed" class="skeleton-list" aria-label="正在读取会话"><div v-for="n in 3" :key="n"><i /><i /><i /></div></div>
            <div v-else-if="!sessions.length" class="rail-empty"><Icon :name="filter ? 'search' : 'log'" :size="26" /><strong>{{ filter ? '没有找到匹配的会话' : '这里还很安静' }}</strong><p>{{ filter ? '换一个关键词试试。' : '在 Codex 完成一轮对话，再回来刷新。' }}</p><button v-if="filter" class="text-button" @click="filter = ''">清除搜索</button></div>
          </div>
          <div class="rail-note"><Icon name="shield" :size="14" /><span>只观察用户会话<br /><small>内部审批与子任务已排除</small></span></div>
        </aside>

        <div class="workspace-scroll">
          <section v-if="session" class="workspace-panel glass-panel">
            <div class="session-heading"><div class="heading-main"><span class="eyebrow">SESSION INSIGHT <span>/ 会话观察</span></span><h1 :title="session.title">{{ session.title || '未命名会话' }}</h1><div class="model-line"><span class="model-tag"><Icon name="chip" :size="13" /><span :title="session.model">{{ session.model }}</span></span><span>{{ session.effort }}</span><i>·</i><span>{{ session.originator || '客户端未记录' }}</span><span v-if="session.reference_model" class="reference-tag" :title="session.model + ' 使用共享指纹参考，没有增加独立候选'">共享参考 · {{ session.reference_model }}</span></div></div></div>
            <div class="verdict-hero" :data-mood="view.mood">
              <div class="hero-copy"><span class="status-badge"><i class="status-dot" />{{ view.label }}</span><h2>{{ view.title }}</h2><p>{{ view.detail }}</p><span class="hero-note"><Icon :name="taskHere ? 'clock' : 'eye'" :size="13" />{{ taskHere ? `已运行 ${elapsed}` : probe ? `最近指纹检测 · ${time(probe.finished, true)}` : '本地日志自动观察 · 无需发起指纹检测' }}</span></div>
              <div class="hero-art"><Chip :mood="view.mood" /></div>
            </div>
            <div class="action-row"><button class="button primary" :disabled="busy || !s.connection" @click="startProbe"><Icon name="observe" :size="17" />主动检测模型指纹<Icon name="arrow" :size="16" /></button><span class="quota-note">主动检测生成 {{ s.settings.queries }} 份回答<br /><small>消耗 Codex 额度 · 本地扫描不消耗</small></span></div>
            <div class="quiet-notice" :data-mood="passiveView.mood" role="status"><Icon name="shield" :size="15" /><p><span class="status-text"><i class="status-dot" />被动日志 · {{ passiveView.label }}</span><br /><template v-if="scan">{{ scanError ? '上次成功读取' : '已读取' }} {{ scan.turns }} 轮 · {{ scan.checked_at ? `最近成功扫描 ${time(scan.checked_at, true)}` : '扫描时间未记录' }}<br /></template>后台空闲时每 30 秒自动扫描，不消耗推理额度；关闭主动检测后仍会运行。</p></div>
            <div v-if="probe" class="result-metrics"><div><span>有效样本</span><strong>{{ probe.used_outputs }}<small> / {{ probe.queries }}</small></strong><div class="sample-dots"><i v-for="n in Math.max(1, probe.queries)" :key="n" :class="{ filled: n <= probe.used_outputs }" /></div></div><div><span>检测耗时</span><strong>{{ probe.elapsed_s < 10 ? probe.elapsed_s.toFixed(1) : Math.round(probe.elapsed_s) }}<small> 秒</small></strong></div><div><span>本地日志</span><strong>{{ scan?.turns ?? '—' }}<small> 轮</small></strong></div></div>
            <ProbeEvidence v-if="probe" :probe="probe" :account="s.account.id" />
            <div v-if="scanError" class="report-errors" role="alert">本次日志读取未完成：{{ scanError }}</div>
            <p v-for="(e, i) in scanError ? [] : scan?.active || []" :key="i" class="evidence-line" :class="e.severity"><Icon :name="e.severity === 'hard' ? 'alert' : 'info'" :size="15" /><span>{{ e.detail }}</span></p>
            <details class="disclosure"><summary><span><Icon name="shield" :size="15" />本地记录<span class="summary-hint">{{ scanError ? '本次读取失败' : !scan ? '尚未扫描' : scan.active.length ? `${scan.active.length} 条变化` : '未发现有效变化' }}</span></span><Icon name="chevron" :size="14" /></summary><div class="disclosure-body"><p class="fine-print">本地扫描不消耗推理额度，也无法证明服务端模型权重未被替换。</p><dl v-if="scan" class="meta-list"><dt>已读取</dt><dd>{{ scan.turns }} 轮</dd><dt>上下文窗口</dt><dd>{{ scan.ctx?.toLocaleString() || '未记录' }}</dd><dt>服务层级</dt><dd>{{ scan.tier || '未记录' }}</dd></dl><div v-if="scan?.evidence.length" class="evidence-list"><p v-for="(e, i) in [...scan.evidence].reverse()" :key="i"><time>{{ time(e.ts, true) }}</time><span>{{ e.detail }}</span></p></div><dl class="meta-list"><dt>会话 ID</dt><dd class="selectable">{{ session.id }}</dd><dt>工作目录</dt><dd class="selectable">{{ session.cwd }}</dd></dl></div></details>
          </section>
          <section v-else class="empty-workspace glass-panel"><div class="empty-chip"><Chip :mood="s.presence.mood" /></div><span class="eyebrow">A LITTLE CLARITY</span><h1>{{ s.last_refreshed ? '让每一次回答，都有迹可循。' : '正在认识你的本地环境…' }}</h1><p>{{ s.connection ? '选择左侧会话，或从一个全新的会话开始。' : '连接 Codex 后，就能在这里观察模型的状态。' }}</p><button v-if="!s.connection && s.last_refreshed" class="button primary" @click="page = 'settings'">设置 Codex 连接<Icon name="arrow" :size="16" /></button></section>
          <details class="fresh-session glass-panel"><summary><span class="fresh-icon"><Icon name="spark" :size="17" /></span><span><strong>从全新会话开始</strong><small>不携带聊天历史，单独观察指定模型</small></span><Icon name="chevron" :size="15" /></summary><div class="fresh-form"><label>模型 ID<input v-model="fresh.model" spellcheck="false" placeholder="例如 gpt-6-astra" maxlength="160" /></label><div class="form-grid"><label>思考强度<select v-model="fresh.effort"><option v-for="effort in effortOptions" :key="effort">{{ effort }}</option></select></label><label>客户端来源<select v-model="fresh.originator"><option>Codex Desktop</option><option>codex_cli_rs</option></select></label></div><button class="button primary" :disabled="busy || !s.connection || !fresh.model.trim()" @click="act('start_probe', { target: { kind: 'fresh', ...fresh, model: fresh.model.trim() } })"><Icon name="play" :size="15" />检测全新会话</button><p class="fine-print">同样会消耗 Codex 额度，每次生成 {{ s.settings.queries }} 份回答。</p></div></details>
        </div>
      </section>

      <section v-else-if="page === 'history'" class="page-scroll">
        <div class="page-heading"><div><h1>时间线</h1><p>查看历次检测结果与依据。</p></div><span class="count-label">{{ s.history.length }}<small>次记录</small></span></div>
        <div class="filter-tabs" aria-label="历史记录筛选"><button :class="{ active: historyFilter === 'all' }" @click="historyFilter = 'all'">全部记录</button><button :class="{ active: historyFilter === 'attention' }" @click="historyFilter = 'attention'">需要留意</button><button :class="{ active: historyFilter === 'incomplete' }" @click="historyFilter = 'incomplete'">未完成</button></div>
        <div v-if="!history.length" class="history-empty glass-panel"><Icon name="clock" :size="34" /><h2>{{ s.history.length ? '这里没有匹配的记录' : '第一条记录，等待你的观察' }}</h2><p>检测完成、失败或取消后，都会保存在时间线中。</p><button class="button secondary" @click="page = 'observe'">回到观察台<Icon name="arrow" :size="15" /></button></div>
        <div v-else class="timeline"><article v-for="p in history" :key="p.id" class="timeline-entry" :data-mood="verdict(p, s.account.id).mood"><div class="timeline-time"><strong>{{ time(p.finished) }}</strong><small>{{ time(p.finished, true).split(' ')[0] }}</small><i class="timeline-dot" /></div><details class="timeline-card glass-panel" :open="openHistory === p.id" @toggle="($event.target as HTMLDetailsElement).open && (openHistory = p.id)"><summary><div class="timeline-chip"><Chip :mood="verdict(p, s.account.id).mood" :animated="false" /></div><div class="timeline-info"><h2>{{ historyTitle(p) }}</h2><p><span class="status-text"><i class="status-dot" />{{ verdict(p, s.account.id).label }}</span><span>{{ p.expected }}</span></p></div><div class="timeline-count"><strong>{{ p.used_outputs }} / {{ p.queries }}</strong><small>有效样本</small></div><Icon name="chevron" :size="16" /></summary><div class="timeline-detail"><p class="report-caption">{{ verdict(p, s.account.id).detail }}</p><ProbeEvidence :probe="p" :account="s.account.id" /></div></details></article></div>
      </section>

      <section v-else class="page-scroll settings-page">
        <div class="page-heading"><div><h1>偏好设置</h1><p>让 Nerfed 适合你的工作方式。</p></div><span v-if="dirty" class="unsaved-label"><i />有未保存的更改</span></div>
        <section class="settings-card glass-panel appearance-setting" aria-labelledby="appearance-heading">
          <div class="section-heading"><div><h2 id="appearance-heading">外观</h2><p>即时应用于主界面与悬浮窗。</p></div></div>
          <div class="appearance-options" role="radiogroup" aria-labelledby="appearance-heading">
            <label v-for="option in appearanceOptions" :key="option.value" class="appearance-option">
              <input type="radio" name="appearance" :value="option.value" :checked="appearance === option.value" @change="setAppearance(option.value)" />
              <span class="appearance-preview" :class="option.value" aria-hidden="true"><i class="preview-sidebar" /><i class="preview-line" /><i class="preview-content" /></span>
              <span>{{ option.label }}</span>
            </label>
          </div>
          <p v-if="appearanceError" class="appearance-error" role="status">{{ appearanceError }}</p>
        </section>
        <form @submit.prevent="save()" @input="dirty = true">
          <section class="settings-card glass-panel companion-setting"><div class="companion-preview"><Chip :mood="s.presence.mood" /></div><div class="setting-copy"><h2>桌面悬浮窗</h2><p>悬停查看状态，拖动调整位置。</p><small>仅展示状态，不会自动发起检测。</small></div><button type="button" class="switch" :class="{ on: s.settings.companion }" role="switch" :aria-checked="s.settings.companion" aria-label="显示桌面芯片" @click="act('set_companion', { visible: !s.settings.companion })"><span /></button></section>
          <section class="settings-card glass-panel"><div class="section-heading"><span class="section-icon"><Icon name="observe" /></span><div><h2>主动检测计划</h2><p>被动日志扫描默认运行，不受此开关影响；主动检测按设定频率生成回答样本。</p></div></div><div class="setting-row"><div><h3 id="automatic-label">后台主动检测</h3><p>只检测最近 15 分钟活跃、且上次检测后有新活动的会话。</p></div><button type="button" class="switch" :class="{ on: draft.automatic }" role="switch" :aria-checked="draft.automatic" aria-labelledby="automatic-label" @click="requestAuto"><span /></button></div><div class="form-grid schedule-grid"><label>检测间隔<span class="unit-input"><input v-model.number="draft.interval_minutes" type="number" min="5" max="1440" step="1" required /><span>分钟</span></span></label><label>回答数量<select v-model.number="draft.queries"><option :value="1">1 份回答</option><option :value="2">2 份回答</option><option :value="3">3 份回答</option></select></label><label>单轮超时<span class="unit-input"><input v-model.number="draft.timeout_seconds" type="number" min="30" max="600" step="1" required /><span>秒</span></span></label></div><div class="quiet-notice"><Icon name="info" :size="15" /><p>主动检测消耗 Codex 额度；超时样本最多补充一次。单份回答不足以作高置信度的不匹配判定。</p></div><div class="setting-row notification-setting"><div><h3>有值得留意的结果时通知我</h3><p>本地日志出现新的降级证据或可疑变化，以及主动检测可疑、不匹配或模型未收录时显示 Windows 通知。</p></div><button type="button" class="switch" :class="{ on: draft.notifications }" role="switch" :aria-checked="draft.notifications" aria-label="启用结果通知" @click="draft.notifications = !draft.notifications; dirty = true"><span /></button></div></section>
          <section class="settings-card glass-panel"><div class="section-heading"><span class="section-icon"><Icon name="chip" /></span><div><h2>Codex 连接</h2><p>{{ s.account.label }}</p></div><span class="small-status" :class="{ connected: s.connection }"><i />{{ s.connection ? `已找到 ${s.connection.version}` : '等待连接' }}</span></div><label class="field-label">程序路径<input v-model="draft.codex_bin" class="path-input" placeholder="留空，自动发现 Codex" spellcheck="false" /><small>支持 EXE、CMD、BAT 与 PS1。留空时选择发现的最新版本。</small></label><p v-if="s.connection" class="resolved-path selectable">当前使用：{{ s.connection.path }}</p></section>
          <div class="save-bar"><span>{{ dirty ? '保存后应用到后台检测计划' : '设置与记录保存在本机' }}</span><button type="submit" class="button primary" :disabled="pending > 0 || s.quitting"><Icon name="check" :size="16" />保存偏好设置</button></div>
        </form>
        <section class="settings-card glass-panel"><div class="section-heading"><span class="section-icon"><Icon name="shield" /></span><div><h2>本地运行，安心保留</h2><p>关闭窗口会收起到托盘；退出程序后后台检测停止。</p></div></div><p class="data-path selectable">{{ s.data_dir }}</p><div class="inline-actions"><button class="button secondary" @click="act('open_data_dir')"><Icon name="folder" :size="16" />打开数据目录</button><button class="text-button" @click="about?.showModal()">关于与许可证<Icon name="chevron" :size="14" /></button></div><details class="disclosure"><summary><span>后台与 hooks 诊断</span><Icon name="chevron" :size="14" /></summary><div class="disclosure-body"><p>程序每 30 秒刷新本地会话，通过独立调度运行，无需安装 hooks。私有探测显式禁用 hooks，避免递归检测。</p><template v-if="s.hooks"><p v-if="!s.hooks.hooks.length" class="muted">Codex 中未发现本插件的 hooks，不影响 Windows 客户端扫描和检测。</p><p v-for="(hook, i) in s.hooks.hooks" :key="i">{{ hook.event }} · {{ hook.enabled ? '已启用' : '已禁用' }} · {{ hook.trusted ? '已信任' : '未信任' }}{{ hook.legacy ? ' · 旧 Python/POSIX 入口' : '' }}</p><p v-if="s.hooks.hooks.some(h => h.legacy)" class="evidence-alert">旧入口不属于此 Windows 客户端；注册或信任成功不代表它在 Windows 上实际执行。</p><p v-for="(note, i) in s.hooks.notes" :key="i">{{ note }}</p><small>最近检查 · {{ time(s.hooks.checked_at, true) }}</small></template><p v-else>{{ s.hook_error || '连接 Codex 后可读取实际注册与信任状态。' }}</p></div></details></section>
      </section>
    </main>
    <main v-else class="boot-screen"><Chip mood="loading" /><h1>一点清晰，即将开始。</h1><p>正在连接本地观察服务…</p><p v-if="error" role="alert">{{ error }}</p></main>

    <div v-if="detecting && s" class="task-strip" role="status"><span class="task-spinner" /><div><strong>{{ s.task?.cancelling ? '正在取消检测' : '正在采集模型指纹' }} <small>{{ s.task?.model }}</small></strong><p>{{ s.status }}</p></div><time>{{ elapsed }}</time><button class="button secondary small" :disabled="s.task?.cancelling || s.quitting" @click="act('cancel_probe')"><Icon name="stop" :size="14" />取消</button><div class="task-track"><i /></div></div>
    <footer class="statusbar"><span><i class="connection-dot" :class="{ connected: s?.connection }" />{{ s?.connection ? `Codex ${s.connection.version}` : '等待 Codex' }}<span class="status-separator">/</span><Icon name="shield" :size="12" />被动扫描 · 零额度</span><span class="footer-right"><button @click="page = 'settings'"><i class="tiny-dot" :class="{ on: s?.settings.automatic }" />{{ s?.settings.automatic ? '自动主动检测已开启' : '自动主动检测已关闭' }}</button><span class="status-separator">·</span><button @click="about?.showModal()">v{{ s?.version || '0.6.0' }}</button><button class="footer-exit" title="停止后台检测并退出" @click="act('quit')">退出<Icon name="exit" :size="12" /></button></span></footer>
    <Transition name="toast"><div v-if="toast" class="toast" role="status"><Icon name="check" :size="16" />{{ toast }}</div></Transition>

    <dialog ref="consent" class="dialog consent-dialog" @cancel.prevent="dismissConsent"><div class="dialog-icon"><Icon name="observe" :size="26" /></div><span class="eyebrow">A LITTLE HELP IN THE BACKGROUND</span><h2>让观察，按时发生。</h2><p>近期活跃会话按 <strong>{{ consentSettings.interval_minutes }} 分钟</strong>间隔检测，收起窗口后仍会运行。</p><div class="consent-cost"><Icon name="info" :size="18" /><span>每次生成 <strong>{{ consentSettings.queries }} 份回答</strong>，消耗你的 Codex 额度。超时样本最多补充一次。</span></div><div class="dialog-actions"><button class="button secondary" @click="dismissConsent">暂不开启</button><button class="button primary" :disabled="pending > 0" @click="acceptAuto">开启并保存<Icon name="arrow" :size="16" /></button></div></dialog>
    <dialog ref="about" class="dialog about-dialog"><div class="dialog-title"><div class="wordmark">nerfed<span class="brand-dot">.</span></div><button class="icon-button" aria-label="关闭关于窗口" @click="about?.close()"><Icon name="close" /></button></div><h2>让每一次回答，都有迹可循。</h2><p>Windows · v{{ s?.version }} · Rust + Tauri</p><p>ModelTrace 指纹库与评分方法来自 xqy2006。指纹归因仅作为辅助证据。</p><details class="disclosure"><summary>开源许可证<Icon name="chevron" :size="14" /></summary><pre>{{ modeltraceLicense }}
{{ license }}</pre></details></dialog>
  </div>
</template>
