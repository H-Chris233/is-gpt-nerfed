import assert from 'node:assert/strict';
import { test } from 'node:test';
import { newer, passive, sessionVerdict, verdict } from '../src/presentation.ts';
import type { Probe, Scan } from '../src/types.ts';

test('unknown, stale and cancelled records cannot look like a verified match', () => {
  const p: Probe = { id: 'test', thread_id: 'session', mode: 'fork', started: '', finished: '', expected: 'new-model',
    originator: 'test', codex: 'test', account_id: 'current', status: 'done', queries: 3, used_outputs: 3,
    elapsed_s: 1, verdict: 'UNLISTED', direction: null, analysis: null, errors: [], hard_evidence: [] };
  assert.equal(verdict(p, 'current').mood, 'unknown');
  assert.match(verdict(p, 'current').detail, /无法判断/);
  p.verdict = 'MATCH';
  assert.equal(verdict(p, 'current').mood, 'match');
  assert.equal(verdict(p, 'other-account').mood, 'unknown');
  const shared = { ...p, expected: 'gpt-6.1-sol', reference_model: 'gpt-6-astra' };
  assert.equal(verdict(shared, 'current').label, '参考指纹匹配');
  assert.match(verdict(shared, 'current').detail, /不代表两者模型身份相同/);
  assert.equal(verdict({ ...shared, verdict: 'MISMATCH' }, 'current').mood, 'warning');
  const unknownAccount = { ...p, account_id: 'local' };
  assert.equal(verdict(unknownAccount, 'local').mood, 'unknown');
  p.status = 'cancelled';
  assert.equal(verdict(p, 'current').mood, 'cancelled');
  p.status = 'failed';
  assert.equal(verdict(p, 'current').mood, 'offline');
});
test('an initial read arriving after a newer event cannot revert the visible state', () => {
  assert.equal(newer(null, { revision: 1 }), true);
  assert.equal(newer({ revision: 8 }, { revision: 7 }), false);
  assert.equal(newer({ revision: 8 }, { revision: 8 }), false);
  assert.equal(newer({ revision: 8 }, { revision: 9 }), true);
});
test('passive observations remain visible without an active probe and never hide a read failure', () => {
  const scan: Scan = { turns: 4, ctx: null, tier: null, checked_at: '2026-10-08T01:00:00Z', active: [], evidence: [] };
  const state = { threads: [], history: [] as Probe[], scans: { session: scan }, scan_errors: {} as Record<string, string>, task: null, account: { id: 'current', label: '' } };
  assert.equal(passive(undefined).label, '等待日志扫描');
  assert.equal(sessionVerdict(state, 'session').label, '被动观察中');
  assert.match(passive(scan).detail, /不消耗/);
  for (const [severity, mood, label] of [['good', 'match', '记录到升级'], ['soft', 'warning', '发现设置变化'], ['hard', 'alert', '发现降级证据']]) {
    scan.active = [{ ts: scan.checked_at!, kind: 'silent_model_change', severity, detail: 'model changed' }];
    assert.equal(sessionVerdict(state, 'session').mood, mood);
    assert.equal(passive(scan).label, label);
  }
  state.history = [{ id: 'probe', thread_id: 'session', mode: 'fork', started: '', finished: '', expected: 'model', originator: '', codex: '', account_id: 'current', status: 'done', queries: 3, used_outputs: 3, elapsed_s: 1, verdict: 'MATCH', direction: null, analysis: null, errors: [], hard_evidence: [] }];
  scan.active[0].severity = 'soft';
  assert.equal(sessionVerdict(state, 'session').mood, 'warning');
  state.history[0].verdict = 'MISMATCH';
  assert.equal(sessionVerdict(state, 'session').label, '指纹不匹配');
  state.scan_errors.session = 'cannot read rollout';
  assert.equal(sessionVerdict(state, 'session').label, '指纹不匹配');
  state.history[0].verdict = 'MATCH';
  assert.equal(sessionVerdict(state, 'session').label, '被动扫描未完成');
  scan.active = [];
  assert.equal(passive(scan, state.scan_errors.session).mood, 'offline');
  assert.equal(sessionVerdict(undefined, 'session').label, '等待日志扫描');
});
