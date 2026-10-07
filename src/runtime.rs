use crate::{
    appserver::{self, Target},
    platform::{self, Binary},
    scanner::{Evidence, Scan},
    storage::{self, Account, Probe, Settings, Store, Thread},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[derive(Clone, Serialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub model: String,
    pub effort: String,
    pub originator: String,
    pub cwd: String,
    pub updated: f64,
    pub reference_model: Option<String>,
}
impl From<&Thread> for Session {
    fn from(t: &Thread) -> Self {
        let expected = crate::scanner::normalize(&t.model);
        let reference = crate::modeltrace::reference_model(&expected);
        Self {
            id: t.id.clone(),
            title: t.title.clone(),
            model: t.model.clone(),
            effort: t.effort.clone(),
            originator: t.originator.clone(),
            cwd: t.cwd.clone(),
            updated: t.updated,
            reference_model: (reference != expected).then(|| reference.to_owned()),
        }
    }
}
#[derive(Clone, Serialize)]
pub struct ScanView {
    pub turns: u64,
    pub ctx: Option<u64>,
    pub tier: Option<String>,
    pub active: Vec<Evidence>,
    pub evidence: Vec<Evidence>,
}
impl From<&Scan> for ScanView {
    fn from(s: &Scan) -> Self {
        Self {
            turns: s.turns,
            ctx: s.ctx,
            tier: s.tier.clone(),
            active: s.active().into_iter().cloned().collect(),
            evidence: s.evidence.clone(),
        }
    }
}
#[derive(Clone, Serialize)]
pub struct Connection {
    pub path: String,
    pub version: String,
}
#[derive(Clone, Serialize)]
pub struct TaskView {
    pub kind: &'static str,
    pub started: f64,
    pub thread_id: Option<String>,
    pub model: String,
    pub cancelling: bool,
}
#[derive(Clone, Serialize, Debug)]
pub struct Presence {
    pub mood: &'static str,
    pub label: &'static str,
    pub title: &'static str,
    pub detail: &'static str,
    pub model: String,
    pub finished: String,
    pub thread_id: Option<String>,
    pub probe_id: Option<String>,
    pub reference_model: Option<String>,
}
impl Default for Presence {
    fn default() -> Self {
        Self {
            mood: "idle",
            label: "准备就绪",
            title: "让每一次回答，都有迹可循。",
            detail: "选择一个会话，从模型留下的线索开始观察。",
            model: String::new(),
            finished: String::new(),
            thread_id: None,
            probe_id: None,
            reference_model: None,
        }
    }
}
impl Presence {
    pub fn from_probe(p: &Probe) -> Self {
        let expected = crate::scanner::normalize(&p.expected);
        let reference = crate::modeltrace::reference_model(&expected);
        let newly_available =
            (p.status == "done" && p.assessment.verdict == "UNLISTED" && reference != expected)
                .then(|| reference.to_owned());
        let (mood, label, title, detail) = if p.status == "cancelled" {
            (
                "cancelled",
                "检测已取消",
                "暂时停一下，也没关系。",
                "这次检测已经取消，没有得出结论。",
            )
        } else if p.status == "failed" {
            (
                "offline",
                "检测未完成",
                "这次检测，还没有答案。",
                "没有获得有效结论，请查看错误原因后重试。",
            )
        } else {
            match p.assessment.verdict.as_str() {
                "MATCH" if p.assessment.reference_model.is_some() => (
                    "match",
                    "参考指纹匹配",
                    "与 Astra 参考指纹一致。",
                    "6.1 Sol 复用 Astra 的指纹参考，不代表两者模型身份相同。",
                ),
                "MISMATCH" if p.assessment.reference_model.is_some() => (
                    "warning",
                    "参考指纹不匹配",
                    "与 Astra 参考指纹存在差异。",
                    "此结果描述共享参考的差异，不能单独据此判断 6.1 Sol 降级。",
                ),
                "SUSPICIOUS" if p.assessment.reference_model.is_some() => (
                    "warning",
                    "参考指纹有待核实",
                    "共享参考，还需要更多线索。",
                    "当前样本尚不足以确认与 Astra 共享参考的匹配情况。",
                ),
                "MATCH" => (
                    "match",
                    "指纹匹配",
                    "熟悉的指纹，依然在。",
                    "本次指纹与所选模型一致，可结合实际使用继续观察。",
                ),
                "UNLISTED" if newly_available.is_some() => (
                    "idle",
                    "共享参考已就绪",
                    "Astra 参考，已经准备好。",
                    "6.1 Sol 已配置共享参考。这份旧记录不会自动改判，重新检测即可更新结果。",
                ),
                "UNLISTED" => (
                    "unknown",
                    "模型未收录",
                    "新面孔，还需要一点认识。",
                    "所选模型尚未被指纹库收录，暂时无法判断是否降级。",
                ),
                "DOWNGRADED!" => (
                    "alert",
                    "发现降级证据",
                    "有些变化，需要你留意。",
                    "本地日志记录了降级证据，请查看具体变化。",
                ),
                "MISMATCH" => (
                    "alert",
                    "指纹不匹配",
                    "这次的指纹，有些不同。",
                    "本次样本更接近其他模型，请结合检测依据核实。",
                ),
                "SUSPICIOUS" => (
                    "warning",
                    "有待核实",
                    "发现一些值得留意的线索。",
                    "目前证据不足以确认模型变化，可以稍后重新检测。",
                ),
                _ => (
                    "offline",
                    "暂无有效结论",
                    "这次检测，还没有答案。",
                    "有效样本不足以判断，请查看检测明细。",
                ),
            }
        };
        Self {
            mood,
            label,
            title,
            detail,
            model: p.expected.clone(),
            finished: p.finished.clone(),
            thread_id: p.thread_id.clone(),
            probe_id: Some(p.id.clone()),
            reference_model: p.assessment.reference_model.clone().or(newly_available),
        }
    }
}
#[derive(Clone, Serialize)]
pub struct Navigation {
    pub revision: u64,
    pub thread_id: Option<String>,
    pub probe_id: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct Snapshot {
    pub revision: u64,
    pub version: &'static str,
    pub settings: Settings,
    pub threads: Vec<Session>,
    pub history: Vec<Probe>,
    pub scans: BTreeMap<String, ScanView>,
    pub scan_errors: BTreeMap<String, String>,
    pub hooks: Option<appserver::HookStatus>,
    pub hook_error: Option<String>,
    pub connection: Option<Connection>,
    pub account: Account,
    pub defaults: [String; 3],
    pub task: Option<TaskView>,
    pub status: String,
    pub error: Option<String>,
    pub last_refreshed: Option<String>,
    pub confirm_auto: bool,
    pub quitting: bool,
    pub presence: Presence,
    pub navigation: Option<Navigation>,
    pub data_dir: String,
}
impl Snapshot {
    fn new(store: &Store, settings: Settings) -> Self {
        Self {
            revision: 0,
            version: env!("CARGO_PKG_VERSION"),
            settings,
            threads: vec![],
            history: vec![],
            scans: BTreeMap::new(),
            scan_errors: BTreeMap::new(),
            hooks: None,
            hook_error: None,
            connection: None,
            account: Account {
                id: "local".into(),
                label: "正在读取本地环境".into(),
            },
            defaults: [String::new(), String::new(), "Codex Desktop".into()],
            task: None,
            status: "正在读取本地会话".into(),
            error: None,
            last_refreshed: None,
            confirm_auto: false,
            quitting: false,
            presence: Presence::default(),
            navigation: None,
            data_dir: store.home.to_string_lossy().into_owned(),
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProbeTarget {
    Thread {
        id: String,
    },
    Fresh {
        model: String,
        effort: String,
        originator: String,
    },
}
pub enum Action {
    Refresh,
    Scan(String),
    Probe(ProbeTarget),
    Cancel,
    Save {
        settings: Settings,
        confirm_automatic: bool,
    },
    ToggleAutomatic,
    ConfirmAutomatic(bool),
    Companion(bool),
    ShowMain {
        follow_status: bool,
    },
    Quit,
}
pub enum Effect {
    State(Box<Snapshot>),
    Notify(String),
    ShowMain,
    Companion(bool),
    Exit,
}
enum Message {
    Action(
        Action,
        Option<mpsc::SyncSender<std::result::Result<(), String>>>,
    ),
    Progress(String),
    Loaded(Result<Loaded>),
    Scanned(Result<(String, Scan)>),
    Probed(Result<Box<Probe>>),
}
struct Loaded {
    threads: Vec<Thread>,
    history: Vec<Probe>,
    scans: BTreeMap<String, Scan>,
    scan_errors: BTreeMap<String, String>,
    hooks: Option<appserver::HookStatus>,
    hook_error: Option<String>,
    binary: Option<Binary>,
    account: Account,
    defaults: [String; 3],
}
struct Task {
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Drop for Task {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
#[derive(Clone)]
pub struct Handle {
    tx: mpsc::Sender<Message>,
    state: Arc<Mutex<Snapshot>>,
}
impl Handle {
    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn execute(&self, action: Action) -> std::result::Result<(), String> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx
            .send(Message::Action(action, Some(tx)))
            .map_err(|_| "程序已退出".to_string())?;
        rx.recv().map_err(|_| "后台任务管理器已退出".to_string())?
    }
    pub fn post(&self, action: Action) {
        let _ = self.tx.send(Message::Action(action, None));
    }
}
pub fn launch(
    store: Store,
    cfg: Settings,
    effect: impl Fn(Effect) + Send + 'static,
) -> Result<Handle> {
    let lock = store.lock("rust-desktop.lock")?;
    let (tx, rx) = mpsc::channel();
    let state = Snapshot::new(&store, cfg);
    let handle = Handle {
        tx: tx.clone(),
        state: Arc::new(Mutex::new(state.clone())),
    };
    let shared = handle.state.clone();
    std::thread::Builder::new()
        .name("nerfed-runtime".into())
        .spawn(move || {
            let mut core = Core {
                store,
                _lock: lock,
                state,
                shared,
                tx,
                task: None,
                threads: vec![],
                binary: None,
                last_refresh: Instant::now(),
                rediscover: false,
                seen: BTreeMap::new(),
                attempts: BTreeMap::new(),
                effect: Box::new(effect),
                pending_effects: Vec::new(),
            };
            core.refresh(true);
            core.publish();
            loop {
                match rx.recv_timeout(Duration::from_secs(1)) {
                    Ok(Message::Action(action, reply)) => {
                        let result = core.action(action).map_err(|e| format!("{e:#}"));
                        if let Err(error) = &result {
                            core.state.error = Some(error.clone());
                        }
                        core.publish();
                        if let Some(reply) = reply {
                            let _ = reply.send(result);
                        }
                    }
                    Ok(message) => {
                        core.receive(message);
                        core.publish();
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                if core.state.quitting && core.task.is_none() {
                    (core.effect)(Effect::Exit);
                    break;
                }
                if core.tick() {
                    core.publish();
                }
            }
        })?;
    Ok(handle)
}
struct Core {
    store: Store,
    _lock: File,
    state: Snapshot,
    shared: Arc<Mutex<Snapshot>>,
    tx: mpsc::Sender<Message>,
    task: Option<Task>,
    threads: Vec<Thread>,
    binary: Option<Binary>,
    last_refresh: Instant,
    rediscover: bool,
    seen: BTreeMap<String, f64>,
    attempts: BTreeMap<String, f64>,
    effect: Box<dyn Fn(Effect) + Send>,
    pending_effects: Vec<Effect>,
}
impl Core {
    fn idle(&self) -> Result<()> {
        if self.state.quitting {
            bail!("程序正在退出");
        }
        if self.task.is_some() {
            bail!("已有任务正在运行，请等待完成或取消检测");
        }
        Ok(())
    }
    fn thread(&self, id: &str) -> Result<Thread> {
        self.threads
            .iter()
            .find(|t| t.id == id)
            .filter(|t| t.eligible())
            .cloned()
            .context("会话不存在或已不可用，请刷新后重试")
    }
    fn action(&mut self, action: Action) -> Result<()> {
        if self.state.quitting && !matches!(action, Action::Quit | Action::Cancel) {
            bail!("程序正在退出");
        }
        match action {
            Action::Refresh => {
                self.idle()?;
                self.refresh(true);
            }
            Action::Scan(id) => {
                self.idle()?;
                let t = self.thread(&id)?;
                self.scan(t);
            }
            Action::Probe(target) => {
                self.idle()?;
                let target = match target {
                    ProbeTarget::Thread { id } => Target::Thread(self.thread(&id)?),
                    ProbeTarget::Fresh {
                        model,
                        effort,
                        originator,
                    } => {
                        let model = model.trim();
                        if model.is_empty()
                            || model.len() > 160
                            || !model
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || "._-/".contains(c))
                        {
                            bail!("请输入有效的模型 ID");
                        }
                        if !crate::scanner::EFFORTS.contains(&effort.as_str()) {
                            bail!("请选择有效的思考强度");
                        }
                        if !["Codex Desktop", "codex_cli_rs"].contains(&originator.as_str()) {
                            bail!("请选择支持的客户端来源");
                        }
                        Target::Fresh {
                            model: model.into(),
                            effort,
                            originator,
                        }
                    }
                };
                self.probe(target)?;
            }
            Action::Cancel => {
                if let Some(task) = &self.task
                    && self.state.task.as_ref().is_some_and(|t| t.kind == "probe")
                {
                    task.cancel.store(true, Ordering::Relaxed);
                    if let Some(view) = &mut self.state.task {
                        view.cancelling = true;
                    }
                    self.state.status = "正在取消检测…".into();
                }
            }
            Action::Save {
                settings,
                confirm_automatic,
            } => {
                if settings.automatic && !self.state.settings.automatic && !confirm_automatic {
                    bail!("开启自动检测前需要确认额度消耗");
                }
                let companion = settings.companion != self.state.settings.companion;
                self.save(settings)?;
                if companion {
                    self.pending_effects
                        .push(Effect::Companion(self.state.settings.companion));
                }
            }
            Action::ToggleAutomatic => {
                if self.state.settings.automatic {
                    let mut cfg = self.state.settings.clone();
                    cfg.automatic = false;
                    self.save(cfg)?;
                } else {
                    self.state.confirm_auto = true;
                    self.pending_effects.push(Effect::ShowMain);
                }
            }
            Action::ConfirmAutomatic(confirmed) => {
                if confirmed && self.state.confirm_auto {
                    let mut cfg = self.state.settings.clone();
                    cfg.automatic = true;
                    self.save(cfg)?;
                }
                self.state.confirm_auto = false;
            }
            Action::Companion(visible) => {
                let mut cfg = self.state.settings.clone();
                cfg.companion = visible;
                self.save(cfg)?;
                self.pending_effects.push(Effect::Companion(visible));
            }
            Action::ShowMain { follow_status } => {
                if follow_status {
                    self.state.navigation = Some(Navigation {
                        revision: self.state.revision + 1,
                        thread_id: self.state.presence.thread_id.clone(),
                        probe_id: self.state.presence.probe_id.clone(),
                    });
                }
                self.pending_effects.push(Effect::ShowMain);
            }
            Action::Quit => {
                self.state.quitting = true;
                if let Some(task) = &self.task {
                    task.cancel.store(true, Ordering::Relaxed);
                }
                self.state.status = "正在停止检测并回收私有进程…".into();
            }
        }
        Ok(())
    }
    fn save(&mut self, cfg: Settings) -> Result<()> {
        self.store.save_settings(&cfg)?;
        let rediscover = cfg.codex_bin != self.state.settings.codex_bin;
        let schedule_changed = cfg.automatic != self.state.settings.automatic
            || cfg.interval_minutes != self.state.settings.interval_minutes;
        self.state.settings = cfg;
        if schedule_changed {
            self.seen.clear();
        }
        if self.task.is_none() {
            self.state.status = "设置已保存".into();
        }
        self.state.error = None;
        if rediscover {
            self.rediscover = true;
            // If a probe is running, discover the new binary after it finishes.
            self.binary = None;
            self.state.connection = None;
            self.last_refresh = Instant::now() - Duration::from_secs(30);
            if self.task.is_none() {
                self.refresh(true);
            }
        }
        Ok(())
    }
    fn refresh(&mut self, discover: bool) {
        let discover = discover || self.rediscover;
        self.rediscover = false;
        let store = self.store.clone();
        let cfg = self.state.settings.clone();
        let binary = self.binary.clone();
        let hooks = self.state.hooks.clone();
        let hook_error = self.state.hook_error.clone();
        let tx = self.tx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker = std::thread::spawn(move || {
            let result = (|| {
                let threads = store.threads()?;
                let history = store.history()?;
                let account = store.account();
                let (model, effort) = store.defaults(&threads);
                let originator = threads
                    .first()
                    .map(|t| t.originator.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("Codex Desktop")
                    .into();
                let batch = store.scan_recent(&threads, &worker_cancel);
                let binary = if discover {
                    platform::discover(&cfg.codex_bin)?
                } else {
                    binary
                };
                let (hooks, hook_error) = if discover && let Some(binary) = &binary {
                    match appserver::inspect_hooks(binary, worker_cancel) {
                        Ok(s) => (Some(s), None),
                        Err(e) => (None, Some(e.to_string())),
                    }
                } else {
                    (hooks, hook_error)
                };
                Ok(Loaded {
                    threads,
                    history,
                    scans: batch.scans,
                    scan_errors: batch.errors,
                    hooks,
                    hook_error,
                    binary,
                    account,
                    defaults: [model, effort, originator],
                })
            })();
            let _ = tx.send(Message::Loaded(result));
        });
        self.last_refresh = Instant::now();
        self.task = Some(Task {
            cancel,
            worker: Some(worker),
        });
        self.state.task = Some(TaskView {
            kind: "refresh",
            started: storage::now(),
            thread_id: None,
            model: String::new(),
            cancelling: false,
        });
    }
    fn scan(&mut self, thread: Thread) {
        let store = self.store.clone();
        let tx = self.tx.clone();
        let id = thread.id.clone();
        let worker = std::thread::spawn(move || {
            let result = store.scan(&thread).map(|s| (thread.id, s));
            let _ = tx.send(Message::Scanned(result));
        });
        self.task = Some(Task {
            cancel: Arc::new(AtomicBool::new(false)),
            worker: Some(worker),
        });
        self.state.task = Some(TaskView {
            kind: "scan",
            started: storage::now(),
            thread_id: Some(id),
            model: String::new(),
            cancelling: false,
        });
        self.state.status = "正在扫描本地日志，不消耗推理额度".into();
        self.state.error = None;
    }
    fn probe(&mut self, target: Target) -> Result<()> {
        let binary = self
            .binary
            .clone()
            .context("未找到 Codex，请安装并登录，或在偏好设置中指定路径")?;
        let thread_id = target.id().map(str::to_string);
        let model = target.model().into();
        self.attempts
            .insert(target.id().unwrap_or("fresh").into(), storage::now());
        let store = self.store.clone();
        let cfg = self.state.settings.clone();
        let tx = self.tx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker = std::thread::spawn(move || {
            let progress = |text| {
                let _ = tx.send(Message::Progress(text));
            };
            let result = appserver::probe(&store, &binary, target, &cfg, worker_cancel, &progress);
            let _ = tx.send(Message::Probed(result.map(Box::new)));
        });
        self.task = Some(Task {
            cancel,
            worker: Some(worker),
        });
        self.state.task = Some(TaskView {
            kind: "probe",
            started: storage::now(),
            thread_id,
            model,
            cancelling: false,
        });
        self.state.status = "正在启动检测".into();
        self.state.error = None;
        Ok(())
    }
    fn receive(&mut self, message: Message) {
        if let Message::Progress(text) = message {
            if !self.state.quitting {
                self.state.status = text;
            }
            return;
        }
        self.task.take();
        self.state.task = None;
        match message {
            Message::Loaded(Ok(s)) => {
                self.state.threads = s.threads.iter().map(Session::from).collect();
                self.threads = s.threads;
                self.state.history = s.history;
                self.state.scans = s
                    .scans
                    .iter()
                    .map(|(id, s)| (id.clone(), ScanView::from(s)))
                    .collect();
                self.state.scan_errors = s.scan_errors;
                self.state.hooks = s.hooks;
                self.state.hook_error = s.hook_error;
                self.state.connection = s.binary.as_ref().map(|b| Connection {
                    path: b.path.to_string_lossy().into_owned(),
                    version: b.version.clone(),
                });
                self.binary = s.binary;
                self.state.account = s.account;
                self.state.defaults = s.defaults;
                self.state.last_refreshed = Some(storage::iso());
                self.state.error = None;
                self.state.status = format!("已更新 {} 个用户会话", self.threads.len());
            }
            Message::Loaded(Err(e)) | Message::Scanned(Err(e)) | Message::Probed(Err(e)) => {
                self.state.error = Some(format!("{e:#}"));
                self.state.status = "操作未完成，可以重试".into();
            }
            Message::Scanned(Ok((id, scan))) => {
                self.state.scan_errors.remove(&id);
                self.state.scans.insert(id, ScanView::from(&scan));
                self.state.status = "本地日志扫描已完成".into();
                self.state.error = None;
            }
            Message::Probed(Ok(probe)) => {
                self.state.status = if probe.status == "cancelled" {
                    "检测已取消".into()
                } else {
                    format!("检测完成：{} · {:.0} 秒", probe.label(), probe.elapsed_s)
                };
                if self.state.settings.notifications
                    && probe.status != "cancelled"
                    && matches!(
                        probe.assessment.verdict.as_str(),
                        "MISMATCH" | "DOWNGRADED!" | "SUSPICIOUS" | "UNLISTED"
                    )
                {
                    self.pending_effects.push(Effect::Notify(format!(
                        "{} · 所选模型 {}{}",
                        probe.label(),
                        probe.expected,
                        if probe.assessment.verdict == "UNLISTED" {
                            "，无法判断是否降级"
                        } else {
                            ""
                        }
                    )));
                }
                self.state.history.insert(0, *probe);
                self.last_refresh = Instant::now() - Duration::from_secs(30);
            }
            Message::Progress(_) | Message::Action(_, _) => {}
        }
    }
    fn tick(&mut self) -> bool {
        if self.state.quitting || self.task.is_some() {
            return false;
        }
        if self.last_refresh.elapsed() >= Duration::from_secs(30) {
            self.refresh(self.binary.is_none());
            return true;
        }
        if self.binary.is_some() && self.state.account.id != "local" {
            let candidates = self
                .threads
                .iter()
                .filter(|t| !self.state.scan_errors.contains_key(&t.id))
                .cloned()
                .collect::<Vec<_>>();
            if let Some(thread) = storage::due_thread(
                &candidates,
                &self.state.history,
                &mut self.seen,
                &self.attempts,
                &self.state.settings,
                &self.state.account.id,
                storage::now(),
            )
            .cloned()
            {
                if let Err(e) = self.probe(Target::Thread(thread)) {
                    self.state.error = Some(e.to_string());
                }
                return true;
            }
        }
        false
    }
    fn presence(&self) -> Presence {
        if self.state.quitting {
            return Presence {
                mood: "busy",
                label: "正在退出",
                title: "正在妥善收好这次观察。",
                detail: "后台任务和私有进程正在停止。",
                ..Presence::default()
            };
        }
        if let Some(task) = &self.state.task
            && task.kind == "probe"
        {
            return Presence {
                mood: "busy",
                label: if task.cancelling {
                    "正在取消"
                } else {
                    "正在检测"
                },
                title: "正在辨认，这一次的指纹。",
                detail: "通过临时会话生成样本，原会话会保持完整。",
                model: task.model.clone(),
                thread_id: task.thread_id.clone(),
                ..Presence::default()
            };
        }
        if self.state.error.is_some() {
            return Presence {
                mood: "offline",
                label: "操作未完成",
                title: "遇到一点状况，稍后再试。",
                detail: "具体原因已记录，请查看错误提示。",
                ..Presence::default()
            };
        }
        if self.state.last_refreshed.is_none() {
            return Presence {
                mood: "loading",
                label: "正在连接",
                title: "准备好，认识你的模型。",
                detail: "正在读取本地会话与连接状态。",
                ..Presence::default()
            };
        }
        if let Some((id, _)) = self.state.scans.iter().find(|(id, scan)| {
            !self.state.scan_errors.contains_key(*id)
                && scan.active.iter().any(|e| e.severity == "hard")
        }) {
            let thread = self.threads.iter().find(|t| t.id == *id);
            return Presence {
                mood: "alert",
                label: "发现降级证据",
                title: "有些变化，需要你留意。",
                detail: "会话日志中存在仍有效的降级证据，请查看具体变化。",
                model: thread.map(|t| t.model.clone()).unwrap_or_default(),
                thread_id: Some(id.clone()),
                ..Presence::default()
            };
        }
        if let Some(probe) = self.state.history.iter().find(|p| {
            self.state.account.id != "local"
                && !p.account_id.is_empty()
                && p.account_id == self.state.account.id
        }) {
            return Presence::from_probe(probe);
        }
        if self.binary.is_none() {
            return Presence {
                mood: "offline",
                label: "等待连接",
                title: "连接 Codex，开始观察。",
                detail: "安装并登录 Codex，或在偏好设置中指定程序路径。",
                ..Presence::default()
            };
        }
        if !self.state.history.is_empty() {
            return Presence {
                mood: "unknown",
                label: "结果待核实",
                title: "换了账户，重新认识一次。",
                detail: "已有结果属于其他账户或账户未知，不能代表当前账户。",
                ..Presence::default()
            };
        }
        Presence::default()
    }
    fn publish(&mut self) {
        self.state.revision += 1;
        self.state.presence = self.presence();
        *self.shared.lock().unwrap_or_else(|e| e.into_inner()) = self.state.clone();
        (self.effect)(Effect::State(Box::new(self.state.clone())));
        // A newly opened window must read the state that caused it to open.
        for effect in std::mem::take(&mut self.pending_effects) {
            (self.effect)(effect);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn companion_changes_preserve_the_detection_clock_and_progress() {
        let root = std::env::temp_dir().join(format!("nerfed-schedule-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&root).unwrap();
        let store = Store {
            codex: root.join("codex"),
            home: root.clone(),
        };
        let cfg = Settings {
            automatic: true,
            ..Default::default()
        };
        let mut state = Snapshot::new(&store, cfg);
        state.status = "正在收集样本".into();
        let (tx, _rx) = mpsc::channel();
        let shared = Arc::new(Mutex::new(state.clone()));
        let window_state = shared.clone();
        let saw_enabled = Arc::new(AtomicBool::new(false));
        let observed = saw_enabled.clone();
        let mut core = Core {
            _lock: store.lock("test.lock").unwrap(),
            store,
            shared,
            state,
            tx,
            task: Some(Task {
                cancel: Arc::new(AtomicBool::new(false)),
                worker: None,
            }),
            threads: vec![],
            binary: None,
            last_refresh: Instant::now(),
            rediscover: false,
            seen: BTreeMap::from([("session".into(), 123.0)]),
            attempts: BTreeMap::new(),
            effect: Box::new(move |effect| {
                if matches!(effect, Effect::Companion(true)) {
                    observed.store(
                        window_state.lock().unwrap().settings.companion,
                        Ordering::Relaxed,
                    );
                }
            }),
            pending_effects: Vec::new(),
        };
        core.action(Action::Companion(true)).unwrap();
        assert_eq!(core.seen.get("session"), Some(&123.0));
        assert_eq!(core.state.status, "正在收集样本");
        assert!(core.state.settings.companion);
        core.publish();
        assert!(saw_enabled.load(Ordering::Relaxed));
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }
}
