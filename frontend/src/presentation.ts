import type { Mood, Probe, Snapshot } from './types';

export interface VerdictView { mood: Mood; label: string; title: string; detail: string }
export const waiting: VerdictView = { mood: 'idle', label: '等待观察', title: '从一次观察，开始了解。', detail: '检测此会话的模型指纹，为接下来的判断留下依据。' };
export function verdict(p: Probe | undefined, account: string): VerdictView {
  if (!p) return waiting;
  if (account === 'local' || !p.account_id || p.account_id !== account) return { mood: 'unknown', label: '账户待核实', title: '这份结果，需要重新确认。', detail: '结果来自其他账户或账户未知，不能代表当前账户。' };
  if (p.status === 'cancelled') return { mood: 'cancelled', label: '已取消', title: '暂时停一下，也没关系。', detail: '这次检测已经取消，没有得出结论。' };
  if (p.status === 'failed') return { mood: 'offline', label: '检测未完成', title: '这次检测，还没有答案。', detail: '未获得有效结论，请查看错误原因后重试。' };
  if (p.reference_model) {
    if (p.verdict === 'MATCH') return { mood: 'match', label: '参考指纹匹配', title: '与 Astra 参考指纹一致。', detail: '6.1 Sol 复用 Astra 的指纹参考，不代表两者模型身份相同。' };
    if (p.verdict === 'MISMATCH') return { mood: 'warning', label: '参考指纹不匹配', title: '与 Astra 参考指纹存在差异。', detail: '此结果描述共享参考的差异，不能单独据此判断 6.1 Sol 降级。' };
    if (p.verdict === 'SUSPICIOUS') return { mood: 'warning', label: '参考指纹有待核实', title: '共享参考，还需要更多线索。', detail: '当前样本尚不足以确认与 Astra 共享参考的匹配情况。' };
  }
  switch (p.verdict) {
    case 'MATCH': return { mood: 'match', label: '指纹匹配', title: '熟悉的指纹，依然在。', detail: '本次指纹与所选模型一致，可结合实际使用继续观察。' };
    case 'UNLISTED': return { mood: 'unknown', label: '模型未收录', title: '新面孔，还需要一点认识。', detail: '所选模型尚未被指纹库收录，暂时无法判断是否降级。' };
    case 'DOWNGRADED!': return { mood: 'alert', label: '发现降级证据', title: '有些变化，需要你留意。', detail: '日志记录了降级证据，请查看具体变化。' };
    case 'MISMATCH': return { mood: 'alert', label: '指纹不匹配', title: '这次的指纹，有些不同。', detail: '样本更接近其他模型，请结合检测依据核实。' };
    case 'SUSPICIOUS': return { mood: 'warning', label: '有待核实', title: '发现值得留意的线索。', detail: '目前证据不足以确认模型变化，可以稍后重新检测。' };
    default: return { mood: 'offline', label: '暂无有效结论', title: '这次检测，还没有答案。', detail: '有效样本不足以判断，请查看检测明细。' };
  }
}
export function newer(current: Pick<Snapshot, 'revision'> | null, next: Pick<Snapshot, 'revision'>): boolean { return !current || next.revision > current.revision; }
export function time(value: string | number | null | undefined, date = false): string {
  if (value == null || value === '') return '—';
  const parsed = new Date(typeof value === 'number' ? value * 1000 : value);
  if (Number.isNaN(parsed.getTime())) return '—';
  return new Intl.DateTimeFormat('zh-CN', date ? { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hour12: false } : { hour: '2-digit', minute: '2-digit', hour12: false }).format(parsed);
}
export function duration(seconds: number): string {
  return seconds < 60 ? `${Math.max(0, Math.round(seconds))} 秒` : `${Math.floor(seconds / 60)} 分 ${Math.floor(seconds % 60)} 秒`;
}
export const effortOptions = ['none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra'];
