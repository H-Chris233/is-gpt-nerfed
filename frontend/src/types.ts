export interface Settings {
  automatic: boolean; companion: boolean; interval_minutes: number;
  queries: number; timeout_seconds: number; notifications: boolean; codex_bin: string;
}
export interface Session {
  id: string; title: string; model: string; effort: string; originator: string; cwd: string; updated: number;
  reference_model?: string | null;
}
export interface Evidence { ts: string; kind: string; severity: string; detail: string; why?: string }
export interface Scan { turns: number; ctx: number | null; tier: string | null; active: Evidence[]; evidence: Evidence[] }
export interface Candidate { model: string; probability: number; score: number }
export interface Probe {
  id: string; thread_id: string | null; mode: string; started: string; finished: string;
  expected: string; originator: string; codex: string; account_id: string; status: string;
  queries: number; used_outputs: number; elapsed_s: number; verdict: string; direction: string | null;
  reference_model?: string | null;
  analysis: { prediction: string; probability: number; results: Candidate[] } | null;
  errors: string[]; hard_evidence: Evidence[];
}
export type Mood = 'idle' | 'loading' | 'busy' | 'match' | 'unknown' | 'warning' | 'alert' | 'offline' | 'cancelled';
export interface Presence {
  mood: Mood; label: string; title: string; detail: string; model: string; finished: string;
  thread_id: string | null; probe_id: string | null;
  reference_model?: string | null;
}
export interface Snapshot {
  revision: number; version: string; settings: Settings; threads: Session[]; history: Probe[];
  scans: Record<string, Scan>; scan_errors: Record<string, string>;
  hooks: { checked_at: string; hooks: { event: string; enabled: boolean; trusted: boolean; legacy: boolean }[]; notes: string[] } | null;
  hook_error: string | null; connection: { path: string; version: string } | null;
  account: { id: string; label: string }; defaults: [string, string, string];
  task: { kind: 'probe' | 'refresh' | 'scan'; started: number; thread_id: string | null; model: string; cancelling: boolean } | null;
  status: string; error: string | null; last_refreshed: string | null; confirm_auto: boolean; quitting: boolean;
  presence: Presence; navigation: { revision: number; thread_id: string | null; probe_id: string | null } | null; data_dir: string;
}
export interface PetLayout { expanded: boolean; chip_x: number; chip_y: number }
