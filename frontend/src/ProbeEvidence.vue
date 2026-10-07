<script setup lang="ts">
import type { Probe } from './types';
import { computed } from 'vue';
import { time } from './presentation';
import Icon from './Icon.vue';
const props = defineProps<{ probe: Probe; account: string }>();
const verified = computed(() => props.account !== 'local' && !!props.probe.account_id && props.probe.account_id === props.account);
</script>
<template>
  <div v-if="probe.errors.length" class="report-errors" :class="{ muted: probe.status === 'cancelled' }"><p v-for="(error, i) in probe.errors" :key="i">{{ error }}</p></div>
  <details class="disclosure">
    <summary><span><Icon name="observe" :size="15" />指纹候选与检测依据</span><Icon name="chevron" :size="14" /></summary>
    <div class="disclosure-body">
      <p class="fine-print">指纹归因是辅助证据，不能直接证明实际模型身份。所选模型未收录时，最近似候选不能作为降级结论。</p>
      <div v-if="probe.analysis" class="candidates">
        <div v-for="candidate in probe.analysis.results.slice(0, 6)" :key="candidate.model" class="candidate">
          <div><span>{{ candidate.model }}</span><span v-if="verified">{{ (candidate.probability * 100).toFixed(1) }}% <small>参考归因</small></span><span v-else class="muted">旧账户候选</span></div>
          <div v-if="verified" class="candidate-track"><i :style="{ width: `${Math.max(0, Math.min(100, candidate.probability * 100))}%` }" /></div>
        </div>
      </div>
      <p v-else class="muted">本次没有可用的指纹分析。</p>
      <p v-for="(e, i) in probe.hard_evidence" :key="i" class="evidence-alert">{{ e.detail }}</p>
      <dl class="meta-list"><dt>所选模型</dt><dd>{{ probe.expected }}</dd><template v-if="probe.reference_model"><dt>共享参考</dt><dd>{{ probe.reference_model }} · 复用参考，不是独立测得的指纹</dd></template><dt>检测时间</dt><dd>{{ time(probe.finished, true) }}</dd><dt>客户端</dt><dd>{{ probe.originator }} · Codex {{ probe.codex }}</dd><dt>检测编号</dt><dd class="selectable">{{ probe.id }}</dd><template v-if="probe.thread_id"><dt>目标会话</dt><dd class="selectable">{{ probe.thread_id }}</dd></template></dl>
    </div>
  </details>
</template>
