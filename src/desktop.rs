use anyhow::{Context, Result};
use eframe::egui;
mod ui;
use is_gpt_nerfed::{
    appserver::{self, Target},
    platform::{self, Binary, Tray, TrayEvent},
    scanner::Scan,
    storage::{self, Account, Probe, Settings, Store, Thread},
};
use std::{
    collections::BTreeMap,
    fs::File,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

struct Snapshot {
    threads: Vec<Thread>,
    history: Vec<Probe>,
    scans: BTreeMap<String, Scan>,
    scan_errors: BTreeMap<String, String>,
    hooks: Option<appserver::HookStatus>,
    hook_error: Option<String>,
    binary: Option<Binary>,
    account: Account,
    defaults: (String, String),
}
enum Event {
    Progress(String),
    Snapshot(Result<Snapshot>),
    Scan(Result<(String, Scan)>),
    Probe(Result<Box<Probe>>),
}
struct Task {
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    started: Instant,
    kind: &'static str,
}
impl Drop for Task {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
struct Desktop {
    store: Store,
    _lock: File,
    tray: Tray,
    cfg: Settings,
    draft: Settings,
    rx: mpsc::Receiver<Event>,
    tx: mpsc::Sender<Event>,
    task: Option<Task>,
    threads: Vec<Thread>,
    history: Vec<Probe>,
    scans: BTreeMap<String, Scan>,
    scan_errors: BTreeMap<String, String>,
    hooks: Option<appserver::HookStatus>,
    hook_error: Option<String>,
    binary: Option<Binary>,
    account: Account,
    selected: Option<String>,
    tab: u8,
    filter: String,
    status: String,
    error: Option<String>,
    last_refresh: Instant,
    seen: BTreeMap<String, f64>,
    attempts: BTreeMap<String, f64>,
    fresh_model: String,
    fresh_effort: String,
    fresh_originator: String,
    hidden: bool,
    quitting: bool,
    confirm_auto: bool,
}
impl Desktop {
    fn new(store: Store, lock: File, cfg: Settings, tray: Tray, ctx: &egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            store,
            _lock: lock,
            tray,
            draft: cfg.clone(),
            cfg,
            rx,
            tx,
            task: None,
            threads: vec![],
            history: vec![],
            scans: BTreeMap::new(),
            scan_errors: BTreeMap::new(),
            hooks: None,
            hook_error: None,
            binary: None,
            account: Account {
                id: "local".into(),
                label: "正在检查本地环境".into(),
            },
            selected: None,
            tab: 0,
            filter: String::new(),
            status: "正在读取本地会话".into(),
            error: None,
            last_refresh: Instant::now(),
            seen: BTreeMap::new(),
            attempts: BTreeMap::new(),
            fresh_model: String::new(),
            fresh_effort: String::new(),
            fresh_originator: "Codex Desktop".into(),
            hidden: false,
            quitting: false,
            confirm_auto: false,
        };
        app.refresh(ctx, true);
        app
    }
    fn refresh(&mut self, ctx: &egui::Context, discover: bool) {
        if self.task.is_some() {
            return;
        }
        let store = self.store.clone();
        let cfg = self.cfg.clone();
        let binary = self.binary.clone();
        let hooks = self.hooks.clone();
        let hook_error = self.hook_error.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel.clone();
        let thread = std::thread::spawn(move || {
            let result = (|| {
                let threads = store.threads()?;
                let history = store.history()?;
                let account = store.account();
                let defaults = store.defaults(&threads);
                let batch = store.scan_recent(&threads, &cancel_worker);
                let binary = if discover {
                    platform::discover(&cfg.codex_bin)?
                } else {
                    binary
                };
                let (hooks, hook_error) = if discover && let Some(binary) = &binary {
                    match appserver::inspect_hooks(binary, cancel_worker) {
                        Ok(status) => (Some(status), None),
                        Err(error) => (None, Some(error.to_string())),
                    }
                } else {
                    (hooks, hook_error)
                };
                Ok(Snapshot {
                    threads,
                    history,
                    scans: batch.scans,
                    scan_errors: batch.errors,
                    hooks,
                    hook_error,
                    binary,
                    account,
                    defaults,
                })
            })();
            let _ = tx.send(Event::Snapshot(result));
            ctx.request_repaint();
        });
        self.last_refresh = Instant::now();
        self.task = Some(Task {
            cancel,
            thread: Some(thread),
            started: Instant::now(),
            kind: "refresh",
        });
    }
    fn scan(&mut self, ctx: &egui::Context) {
        if self.task.is_some() {
            return;
        }
        let Some(t) = self
            .threads
            .iter()
            .find(|t| Some(&t.id) == self.selected.as_ref())
            .cloned()
        else {
            return;
        };
        let store = self.store.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let thread = std::thread::spawn(move || {
            let result = store.scan(&t).map(|s| (t.id, s));
            let _ = tx.send(Event::Scan(result));
            ctx.request_repaint();
        });
        self.status = "正在扫描本地日志，不消耗推理额度".into();
        self.error = None;
        self.task = Some(Task {
            cancel,
            thread: Some(thread),
            started: Instant::now(),
            kind: "scan",
        });
    }
    fn start_probe(&mut self, ctx: &egui::Context, target: Target) {
        if self.task.is_some() {
            return;
        }
        let Some(binary) = self.binary.clone() else {
            self.error = Some("未找到 Codex，请先安装并登录 Codex，或在设置中选择其路径".into());
            return;
        };
        self.attempts
            .insert(target.id().unwrap_or("fresh").into(), storage::now());
        let store = self.store.clone();
        let cfg = self.cfg.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let thread = std::thread::spawn(move || {
            let progress = |text| {
                let _ = tx.send(Event::Progress(text));
                ctx.request_repaint();
            };
            let result = appserver::probe(&store, &binary, target, &cfg, worker_cancel, &progress);
            let _ = tx.send(Event::Probe(result.map(Box::new)));
            ctx.request_repaint();
        });
        self.status = "正在启动检测".into();
        self.error = None;
        self.task = Some(Task {
            cancel,
            thread: Some(thread),
            started: Instant::now(),
            kind: "probe",
        });
    }
    fn save(&mut self, ctx: &egui::Context) {
        match self.store.save_settings(&self.draft) {
            Ok(()) => {
                let rediscover = self.draft.codex_bin != self.cfg.codex_bin;
                self.cfg = self.draft.clone();
                self.seen.clear();
                self.status = "设置已保存".into();
                self.error = None;
                if rediscover {
                    self.refresh(ctx, true);
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    fn toggle_auto(&mut self, ctx: &egui::Context) {
        // Tray actions change the saved schedule, not an unfinished settings form.
        self.draft = self.cfg.clone();
        if self.cfg.automatic {
            self.draft.automatic = false;
            self.save(ctx);
        } else {
            self.confirm_auto = true;
        }
    }
    fn request_quit(&mut self, ctx: &egui::Context) {
        self.quitting = true;
        if let Some(task) = &self.task {
            task.cancel.store(true, Ordering::Relaxed);
            self.status = "正在停止检测并回收私有进程…".into();
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    fn tick(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.tray.events.try_recv() {
            match event {
                TrayEvent::Show => {
                    self.hidden = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayEvent::ToggleAutomatic => {
                    self.hidden = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    self.toggle_auto(ctx);
                }
                TrayEvent::Quit => self.request_quit(ctx),
            }
        }
        while let Ok(event) = self.rx.try_recv() {
            if let Event::Progress(text) = event {
                self.status = text;
                continue;
            }
            self.task.take();
            match event {
                Event::Snapshot(Ok(s)) => {
                    self.threads = s.threads;
                    self.history = s.history;
                    self.scans = s.scans;
                    self.scan_errors = s.scan_errors;
                    self.hooks = s.hooks;
                    self.hook_error = s.hook_error;
                    self.binary = s.binary;
                    self.account = s.account;
                    if self.fresh_model.is_empty() {
                        self.fresh_model = s.defaults.0;
                        self.fresh_effort = s.defaults.1;
                        if let Some(thread) =
                            self.threads.first().filter(|t| !t.originator.is_empty())
                        {
                            self.fresh_originator = thread.originator.clone();
                        }
                    }
                    if self
                        .selected
                        .as_ref()
                        .is_none_or(|id| !self.threads.iter().any(|t| t.id == *id))
                    {
                        self.selected = self.threads.first().map(|t| t.id.clone());
                    }
                    self.status = format!(
                        "已读取 {} 个用户会话 · 后台自动检测{}",
                        self.threads.len(),
                        if self.cfg.automatic {
                            "已开启"
                        } else {
                            "已关闭"
                        }
                    );
                    self.error = None;
                }
                Event::Snapshot(Err(e)) | Event::Scan(Err(e)) | Event::Probe(Err(e)) => {
                    self.error = Some(format!("{e:#}"));
                    self.status = "操作失败，可以重试".into();
                }
                Event::Scan(Ok((id, scan))) => {
                    self.scan_errors.remove(&id);
                    self.scans.insert(id, scan);
                    self.status = "本地日志扫描已完成".into();
                }
                Event::Probe(Ok(probe)) => {
                    self.status = if probe.status == "cancelled" {
                        "检测已取消".into()
                    } else {
                        format!("检测完成：{} · {:.0} 秒", probe.label(), probe.elapsed_s)
                    };
                    if self.cfg.notifications
                        && probe.status != "cancelled"
                        && matches!(
                            probe.assessment.verdict.as_str(),
                            "MISMATCH" | "DOWNGRADED!" | "SUSPICIOUS" | "UNLISTED"
                        )
                    {
                        self.tray.notify(format!(
                            "{} · 所选模型 {}{}",
                            probe.label(),
                            probe.expected,
                            if probe.assessment.verdict == "UNLISTED" {
                                "，无法判断是否降级"
                            } else {
                                ""
                            }
                        ));
                    }
                    self.history.insert(0, *probe);
                    self.last_refresh = Instant::now() - Duration::from_secs(30);
                }
                Event::Progress(_) => {}
            }
        }
        if self.quitting {
            if self.task.is_none() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else if self.task.is_none() {
            if self.last_refresh.elapsed() >= Duration::from_secs(30) {
                self.refresh(ctx, false);
            } else if self.binary.is_some() && self.account.id != "local" {
                let candidates = self
                    .threads
                    .iter()
                    .filter(|t| !self.scan_errors.contains_key(&t.id))
                    .cloned()
                    .collect::<Vec<_>>();
                if let Some(t) = storage::due_thread(
                    &candidates,
                    &self.history,
                    &mut self.seen,
                    &self.attempts,
                    &self.cfg,
                    &self.account.id,
                    storage::now(),
                )
                .cloned()
                {
                    self.start_probe(ctx, Target::Thread(t));
                }
            }
        }
        // eframe's logic runs even when hidden; no continuous drawing in the tray.
        ctx.request_repaint_after(Duration::from_millis(if self.task.is_some() {
            250
        } else {
            1000
        }));
    }
}
impl eframe::App for Desktop {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.hidden = true;
        }
        self.tick(ctx);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui::render(self, ui);
    }
}
pub fn run() -> Result<()> {
    let store = Store::local()?;
    let cfg = store.settings()?;
    let lock = store.lock("rust-desktop.lock")?;
    let tray = Tray::new()?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("is-gpt-nerfed — Windows")
            .with_icon(egui::IconData {
                rgba: platform::icon_rgba(),
                width: 32,
                height: 32,
            })
            .with_inner_size([1040.0, 660.0])
            .with_min_inner_size([800.0, 480.0])
            .with_clamp_size_to_monitor_size(true),
        ..Default::default()
    };
    eframe::run_native(
        "is-gpt-nerfed",
        options,
        Box::new(move |cc| {
            ui::setup(&cc.egui_ctx);
            Ok(Box::new(Desktop::new(store, lock, cfg, tray, &cc.egui_ctx)))
        }),
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
    .context("无法打开 Windows 图形界面")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_toggle_does_not_save_an_unfinished_settings_form() {
        let root =
            std::env::temp_dir().join(format!("nerfed-tray-settings-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&root).unwrap();
        let binary = root.join("offline-placeholder.exe");
        std::fs::write(&binary, b"not an executable").unwrap();
        let store = Store {
            codex: root.join("codex"),
            home: root.clone(),
        };
        let cfg = Settings {
            automatic: true,
            queries: 3,
            codex_bin: binary.to_string_lossy().into_owned(),
            ..Default::default()
        };
        store.save_settings(&cfg).unwrap();
        let ctx = egui::Context::default();
        let mut app = Desktop::new(
            store.clone(),
            store.lock("desktop-test.lock").unwrap(),
            cfg.clone(),
            Tray::new().unwrap(),
            &ctx,
        );
        app.draft.queries = 1;
        app.draft.interval_minutes = 99;
        app.draft.codex_bin = root.join("unsaved.exe").to_string_lossy().into_owned();
        app.toggle_auto(&ctx);
        let saved = store.settings().unwrap();
        assert!(!saved.automatic);
        assert_eq!(saved.queries, cfg.queries);
        assert_eq!(saved.interval_minutes, cfg.interval_minutes);
        assert_eq!(saved.codex_bin, cfg.codex_bin);
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}
