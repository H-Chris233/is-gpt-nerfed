import type { Mood, Probe, Scan, Snapshot } from './types';

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
export function passive(scan: Scan | undefined, error?: string): VerdictView {
  if (error) return { mood: 'offline', label: '被动扫描未完成', title: '本地日志，暂时无法读取。', detail: '本次读取失败，上次扫描结果不能代表最新状态。请查看错误原因。' };
  if (!scan) return { mood: 'idle', label: '等待日志扫描', title: '等待本地日志的第一条线索。', detail: '程序会自动扫描会话日志，不消耗推理额度，无需开启主动检测。' };
  if (scan.active.some(e => e.severity === 'hard')) return { mood: 'alert', label: '发现降级证据', title: '有些变化，需要你留意。', detail: '日志中存在仍有效的降级证据，请查看下方本地记录。' };
  if (scan.active.some(e => e.severity === 'soft')) return { mood: 'warning', label: '发现设置变化', title: '本地记录，有些变化。', detail: '日志记录了需要核实的模型或思考强度变化，请确认是否由你调整。' };
  if (scan.active.some(e => e.severity === 'good')) return { mood: 'match', label: '记录到升级', title: '本地日志，记录到一次升级。', detail: '日志记录了向更新或更大模型的变化，可查看具体依据。' };
  return { mood: 'idle', label: '被动观察中', title: '本地日志，正在持续观察。', detail: '本地日志未发现有效异常。被动扫描不消耗推理额度，模型指纹可另行主动检测。' };
}
export function sessionVerdict(state: Pick<Snapshot, 'threads' | 'history' | 'scans' | 'scan_errors' | 'task' | 'account'> | null | undefined, id: string): VerdictView {
  const item = state?.threads.find(t => t.id === id);
  const result = state?.history.find(p => p.thread_id === id);
  const observed = passive(state?.scans[id], state?.scan_errors[id]);
  if (state?.task?.kind === 'probe' && state.task.thread_id === id) return { mood: 'busy', label: state.task.cancelling ? '正在取消' : '检测进行中', title: '正在辨认，这一次的指纹。', detail: '通过临时会话生成样本，原会话会保持完整。' };
  if (observed.mood === 'alert') return observed;
  const active = verdict(result, state?.account.id || 'local');
  if (active.mood === 'alert' || active.mood === 'warning') return active;
  if (observed.mood === 'offline' || observed.mood === 'warning') return observed;
  if (item?.reference_model && result?.verdict === 'UNLISTED' && result.expected === item.model && result.status === 'done' && state?.account.id !== 'local' && result.account_id === state?.account.id) return { mood: 'idle', label: '共享参考已就绪', title: 'Astra 参考，已经准备好。', detail: '6.1 Sol 已配置共享参考。这份旧记录不会自动改判，重新检测即可更新结果。' };
  return result ? active : observed;
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
