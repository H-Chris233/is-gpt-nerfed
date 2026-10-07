import assert from 'node:assert/strict';
import { test } from 'node:test';
import { newer, verdict } from '../src/presentation.ts';
import type { Probe } from '../src/types.ts';

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
