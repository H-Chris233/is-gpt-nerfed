use crate::{
    modeltrace::{self, Bank, Output},
    platform::{self, Binary, Job},
    scanner,
    storage::{self, Probe, Settings, Store, Thread},
};
use anyhow::{Context, Result, bail};
use rand::Rng;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub struct AppServer {
    child: Child,
    input: Option<ChildStdin>,
    job: Option<Job>,
    reader: Option<JoinHandle<()>>,
    rx: Receiver<Result<Value, String>>,
    notifications: VecDeque<Value>,
    next_id: u64,
    cancel: Arc<AtomicBool>,
}
impl AppServer {
    pub fn start(binary: &Binary, originator: &str, cancel: Arc<AtomicBool>) -> Result<Self> {
        Self::connect(binary, originator, cancel, false)
    }
    fn connect(
        binary: &Binary,
        originator: &str,
        cancel: Arc<AtomicBool>,
        inspect: bool,
    ) -> Result<Self> {
        let mut args = vec![];
        if binary.no_daemon {
            args.push("--no-daemon".into());
        }
        args.extend(
            [
                "app-server",
                "--stdio",
                "-c",
                "notify=[]",
                "-c",
                if inspect {
                    "features.hooks=true"
                } else {
                    "features.hooks=false"
                },
            ]
            .map(str::to_owned),
        );
        let mut cmd = platform::command(&binary.path, &args)?;
        cmd.env("NERFED_PROBE_PROCESS", "1")
            .env("CODEX_INTERNAL_ORIGINATOR_OVERRIDE", originator);
        for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("CODEX_SANDBOX")) {
            cmd.env_remove(key);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("无法启动 Codex app-server")?;
        let job = match Job::assign(&child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let input = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .context("app-server stdout unavailable")?;
        let (tx, rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = Vec::new();
            loop {
                line.clear();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if line.len() > 8 * 1024 * 1024 {
                            let _ = tx.send(Err("app-server message exceeded 8 MiB".into()));
                            break;
                        }
                        if let Ok(v) = serde_json::from_slice(&line)
                            && tx.send(Ok(v)).is_err()
                        {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = tx.send(Err(error.to_string()));
                        break;
                    }
                }
            }
        });
        let mut app = Self {
            child,
            input,
            job: Some(job),
            reader: Some(reader),
            rx,
            notifications: VecDeque::new(),
            next_id: 0,
            cancel,
        };
        app.request("initialize",json!({"clientInfo":{"name":"is-gpt-nerfed","title":"is-gpt-nerfed Windows","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}),Duration::from_secs(20))?;
        app.send(json!({"method":"initialized"}))?;
        Ok(app)
    }
    fn check_cancelled(&self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            bail!("检测已取消");
        }
        Ok(())
    }
    fn send(&mut self, message: Value) -> Result<()> {
        let input = self.input.as_mut().context("app-server input closed")?;
        serde_json::to_writer(&mut *input, &message)?;
        input.write_all(b"\n")?;
        input.flush()?;
        Ok(())
    }
    fn receive(&mut self, wait: Duration) -> Result<Option<Value>> {
        self.check_cancelled()?;
        match self.rx.recv_timeout(wait) {
            Ok(Ok(message)) => {
                if message.get("id").is_some() && message.get("method").is_some() {
                    self.send(json!({"id":message["id"],"error":{"code":-32601,"message":"Fingerprint probes do not execute tools or grant permissions"}}))?;
                    let mut params = message["params"].clone();
                    params["_request_method"] = message["method"].clone();
                    return Ok(Some(json!({"method":"_server_request","params":params})));
                }
                Ok(Some(message))
            }
            Ok(Err(error)) => bail!("app-server transport: {error}"),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => bail!("codex app-server exited"),
        }
    }
    pub fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        self.check_cancelled()?;
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"id":id,"method":method,"params":params}))?;
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(message) = self.receive(Duration::from_millis(100))? {
                if message["id"].as_u64() == Some(id) {
                    if !message["error"].is_null() {
                        bail!(
                            "codex {method}: {}",
                            message["error"]["message"]
                                .as_str()
                                .unwrap_or("request failed")
                        );
                    }
                    return Ok(message["result"].clone());
                }
                if message.get("method").is_some() {
                    self.notifications.push_back(message);
                }
            }
        }
        bail!("codex {method} timed out after {}s", timeout.as_secs())
    }
    fn event(&mut self) -> Result<Option<Value>> {
        self.check_cancelled()?;
        if let Some(message) = self.notifications.pop_front() {
            return Ok(Some(message));
        }
        self.receive(Duration::from_millis(100))
    }
    fn interrupt(&mut self, forks: &[Fork]) {
        for f in forks.iter().filter(|f| !f.turn.is_empty() && !f.done) {
            self.next_id += 1;
            let id = self.next_id;
            let _=self.send(json!({"id":id,"method":"turn/interrupt","params":{"threadId":f.id,"turnId":f.turn}}));
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct HookStatus {
    pub checked_at: String,
    pub hooks: Vec<HookInfo>,
    pub notes: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct HookInfo {
    pub event: String,
    pub enabled: bool,
    pub trusted: bool,
    pub legacy: bool,
}
pub fn inspect_hooks(binary: &Binary, cancel: Arc<AtomicBool>) -> Result<HookStatus> {
    let mut app = AppServer::connect(binary, "is-gpt-nerfed", cancel, true)?;
    let result = app.request("hooks/list", json!({}), Duration::from_secs(20))?;
    let mut status = HookStatus {
        checked_at: storage::iso(),
        ..Default::default()
    };
    for group in result["data"]
        .as_array()
        .context("Codex 未返回 hook 列表")?
    {
        for hook in group["hooks"].as_array().into_iter().flatten() {
            let plugin = hook["pluginId"].as_str().unwrap_or("");
            let source = hook["sourcePath"].as_str().unwrap_or("").replace('\\', "/");
            if plugin != "is-gpt-nerfed"
                && !plugin.starts_with("is-gpt-nerfed@")
                && !source.contains("/is-gpt-nerfed/")
            {
                continue;
            }
            let command = hook["command"].as_str().unwrap_or("");
            status.hooks.push(HookInfo {
                event: hook["eventName"].as_str().unwrap_or("unknown").into(),
                enabled: hook["enabled"].as_bool().unwrap_or(false),
                trusted: hook["trustStatus"].as_str() == Some("trusted"),
                legacy: command.contains("python") || command.contains("sh -c"),
            });
        }
        for message in ["errors", "warnings"]
            .into_iter()
            .flat_map(|key| group[key].as_array().into_iter().flatten())
        {
            if let Some(text) = message.as_str().filter(|s| s.contains("is-gpt-nerfed")) {
                status.notes.push(text.into());
            }
        }
    }
    Ok(status)
}
impl Drop for AppServer {
    fn drop(&mut self) {
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(1);
        while self.child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
        // Closing our Job kills the private server's whole tree, never the user's Codex process.
        self.job.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub fn prompt(language: &str, count: usize) -> String {
    if language == "zh" {
        format!(
            "直接选择 {count} 个 1 到 355（含边界）的整数。允许重复；不要排序、平衡频数、修复重复，也不要刻意构造等差规律。\n直接回答一个 JSON 整数数组，不要解释。不要调用工具、读文件、运行代码或让其他模型代答；不要继续之前的任务。"
        )
    } else {
        format!(
            "Directly choose {count} integers from 1 through 355, inclusive. Allow repeats. Do not sort, balance frequencies, repair duplicates, or deliberately create an arithmetic pattern.\nReply directly with one JSON integer array and no explanation. Do not call tools, read files, execute code or ask another model. Do not continue the preceding task."
        )
    }
}
#[derive(Clone)]
pub enum Target {
    Thread(Thread),
    Fresh {
        model: String,
        effort: String,
        originator: String,
    },
}
impl Target {
    pub fn model(&self) -> &str {
        match self {
            Self::Thread(t) => &t.model,
            Self::Fresh { model, .. } => model,
        }
    }
    pub fn originator(&self) -> &str {
        match self {
            Self::Thread(t) => {
                if t.originator.is_empty() {
                    "codex_cli_rs"
                } else {
                    &t.originator
                }
            }
            Self::Fresh { originator, .. } => originator,
        }
    }
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Thread(t) => Some(&t.id),
            Self::Fresh { .. } => None,
        }
    }
}
struct Fork {
    id: String,
    turn: String,
    count: usize,
    language: &'static str,
    text: Option<String>,
    other: Vec<String>,
    error: Option<String>,
    done: bool,
}
fn checked_fork(
    response: &Value,
    parent: &str,
    model: &str,
    provider: &str,
    effort: &str,
) -> Result<Fork> {
    let fork = &response["thread"];
    if fork["ephemeral"] != true
        || fork["id"].as_str().is_none_or(str::is_empty)
        || fork["path"].as_str().is_some_and(|s| !s.is_empty())
        || fork["id"].as_str() == Some(parent)
        || fork["forkedFromId"].as_str() != Some(parent)
    {
        bail!("Codex 未返回目标会话的临时 fork");
    }
    if response["model"].as_str() != Some(model)
        || response["modelProvider"].as_str() != Some(provider)
        || (!effort.is_empty()
            && response["reasoningEffort"]
                .as_str()
                .is_some_and(|v| v != effort))
    {
        bail!("fork 未继承目标模型或思考强度");
    }
    Ok(new_fork(fork["id"].as_str().unwrap().into()))
}
fn new_fork(id: String) -> Fork {
    let mut rng = rand::rng();
    Fork {
        id,
        turn: String::new(),
        count: rng.random_range(292..=332),
        language: if rng.random_bool(0.5) { "zh" } else { "en" },
        text: None,
        other: vec![],
        error: None,
        done: false,
    }
}

fn create_forks(
    app: &mut AppServer,
    target: &Target,
    queries: usize,
    progress: &dyn Fn(String),
) -> Result<(String, Vec<Fork>)> {
    let mut forks = Vec::new();
    match target {
        Target::Thread(hint) => {
            progress("读取目标会话，确认模型与思考强度".into());
            let data = app.request(
                "thread/read",
                json!({"threadId":hint.id,"includeTurns":false}),
                Duration::from_secs(20),
            )?;
            let thread = &data["thread"];
            if thread["id"].as_str() != Some(&hint.id)
                || thread["ephemeral"] == true
                || thread["path"].as_str().is_none_or(str::is_empty)
            {
                bail!("会话尚未保存或不是所选会话");
            }
            let model = thread["model"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(&hint.model)
                .to_string();
            if model.is_empty() {
                bail!("Codex 和本地记录均未提供模型");
            }
            let provider = thread["modelProvider"].as_str().unwrap_or("openai");
            let effort = thread["reasoningEffort"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(&hint.effort);
            let cwd = thread["cwd"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(&hint.cwd);
            if cwd.is_empty() {
                bail!("会话缺少工作目录");
            }
            let deadline = Instant::now() + Duration::from_secs(60);
            let mut last_error = None;
            let (first, params) = loop {
                app.check_cancelled()?;
                let turns=app.request("thread/turns/list",json!({"threadId":hint.id,"limit":6,"itemsView":"notLoaded","sortDirection":"desc"}),Duration::from_secs(20))?;
                let candidates = turns["data"].as_array().context("Codex 未返回会话轮次")?;
                let mut chosen = None;
                for turn in candidates
                    .iter()
                    .filter(|t| {
                        matches!(
                            t["status"].as_str(),
                            Some("completed" | "interrupted" | "failed")
                        )
                    })
                    .take(3)
                {
                    let mut params = json!({"threadId":hint.id,"lastTurnId":turn["id"],"ephemeral":true,"excludeTurns":true,"model":model,"modelProvider":provider,"cwd":cwd});
                    if !effort.is_empty() {
                        params["config"] = json!({"model_reasoning_effort":effort});
                    }
                    match app.request("thread/fork", params.clone(), Duration::from_secs(45)) {
                        Ok(response) => {
                            chosen = Some((
                                checked_fork(&response, &hint.id, &model, provider, effort)?,
                                params,
                            ));
                            break;
                        }
                        Err(error) => {
                            let text = error.to_string().to_lowercase();
                            if !text.contains("in-progress")
                                && !text.contains("in progress")
                                && !text.contains("inprogress")
                            {
                                return Err(error);
                            }
                            last_error = Some(error);
                        }
                    }
                }
                if let Some(chosen) = chosen {
                    break chosen;
                }
                if Instant::now() >= deadline {
                    return Err(last_error
                        .unwrap_or_else(|| anyhow::anyhow!("会话没有可 fork 的已完成轮次")));
                }
                progress("等待目标会话完成当前轮次（最多 60 秒）".into());
                std::thread::sleep(Duration::from_millis(250));
            };
            forks.push(first);
            for _ in 1..queries {
                let response =
                    app.request("thread/fork", params.clone(), Duration::from_secs(45))?;
                forks.push(checked_fork(&response, &hint.id, &model, provider, effort)?);
            }
            Ok((model, forks))
        }
        Target::Fresh { model, effort, .. } => {
            if model.is_empty() {
                bail!("请在新会话检测中输入模型");
            }
            let cwd = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .unwrap_or_else(|_| ".".into());
            for _ in 0..queries {
                let mut params = json!({"ephemeral":true,"model":model,"cwd":cwd});
                if !effort.is_empty() {
                    params["config"] = json!({"model_reasoning_effort":effort});
                }
                let response = app.request("thread/start", params, Duration::from_secs(45))?;
                let t = &response["thread"];
                if t["id"].as_str().is_none_or(str::is_empty)
                    || t["ephemeral"] != true
                    || t["path"].as_str().is_some_and(|s| !s.is_empty())
                    || response["model"].as_str().is_some_and(|v| v != model)
                    || (!effort.is_empty()
                        && response["reasoningEffort"]
                            .as_str()
                            .is_some_and(|v| v != effort))
                {
                    bail!("Codex 未按指定设置创建临时新会话");
                }
                forks.push(new_fork(t["id"].as_str().unwrap().into()));
            }
            Ok((model.clone(), forks))
        }
    }
}

fn collect(
    app: &mut AppServer,
    forks: &mut [Fork],
    timeout: Duration,
    progress: &dyn Fn(String),
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    for f in &mut *forks {
        let result = app.request(
            "turn/start",
            json!({"threadId":f.id,"input":[{"type":"text","text":prompt(f.language,f.count)}],
                "approvalPolicy":"untrusted","sandboxPolicy":{"type":"readOnly"}}),
            Duration::from_secs(20),
        );
        match result {
            Ok(v) => {
                f.turn = v["turn"]["id"]
                    .as_str()
                    .context("Codex 未返回检测轮次 ID")?
                    .into();
            }
            Err(error) => {
                f.error = Some(error.to_string());
                f.done = true;
            }
        }
    }
    while forks.iter().any(|f| !f.done) && Instant::now() < deadline {
        if let Err(error) = app.check_cancelled() {
            app.interrupt(forks);
            return Err(error);
        }
        let message = match app.event() {
            Ok(Some(m)) => m,
            Ok(None) => continue,
            Err(error) => {
                app.interrupt(forks);
                return Err(error);
            }
        };
        let p = &message["params"];
        let Some(f) = forks
            .iter_mut()
            .find(|f| Some(f.id.as_str()) == p["threadId"].as_str() && !f.done)
        else {
            continue;
        };
        match message["method"].as_str().unwrap_or("") {
            "_server_request" => {
                f.error = Some(format!("probe requested {}; refused", p["_request_method"]));
                f.done = true;
            }
            "item/started" => {
                if !matches!(
                    p["item"]["type"].as_str(),
                    Some("userMessage" | "agentMessage" | "reasoning" | "hookPrompt")
                ) {
                    f.error = Some("probe attempted a tool; no sample accepted".into());
                    f.done = true;
                }
            }
            "item/completed" => {
                let item = &p["item"];
                if item["type"] == "agentMessage" {
                    if matches!(item["phase"].as_str(), Some("final_answer" | "final")) {
                        f.text = item["text"].as_str().map(str::to_owned);
                    } else if item["phase"].is_null()
                        && let Some(text) = item["text"].as_str()
                    {
                        f.other.push(text.into());
                    }
                }
            }
            "turn/completed" => {
                if p["turn"]["id"].as_str().is_some_and(|id| id != f.turn) {
                    continue;
                }
                if p["turn"]["status"] != "completed" {
                    f.error = Some(format!(
                        "probe turn ended with status {}",
                        p["turn"]["status"]
                    ));
                }
                if f.text.is_none() && !f.other.is_empty() {
                    f.text = Some(f.other.join("\n"));
                }
                if f.text.is_none() && f.error.is_none() {
                    f.error = Some("probe did not return a final text answer".into());
                }
                f.done = true;
                progress(format!(
                    "已完成 {}/{} 份回答",
                    forks.iter().filter(|f| f.done).count(),
                    forks.len()
                ));
            }
            "error" if p["willRetry"] != true => {
                f.error = Some(format!(
                    "probe inference failed: {}",
                    p["error"]["message"]
                        .as_str()
                        .or_else(|| p["message"].as_str())
                        .unwrap_or("error")
                ));
                f.done = true;
            }
            _ => {}
        }
    }
    app.interrupt(forks);
    for f in forks.iter_mut().filter(|f| !f.done) {
        f.error = Some("fork timed out: no answer before deadline".into());
        f.done = true;
    }
    Ok(())
}
fn round(
    binary: &Binary,
    target: &Target,
    queries: usize,
    cfg: &Settings,
    cancel: Arc<AtomicBool>,
    progress: &dyn Fn(String),
    inference: bool,
) -> Result<(String, Vec<Output>, Vec<String>, usize)> {
    let mut app = AppServer::start(binary, target.originator(), cancel)?;
    let (model, mut forks) = create_forks(&mut app, target, queries, progress)?;
    if inference {
        collect(
            &mut app,
            &mut forks,
            Duration::from_secs(cfg.timeout_seconds),
            progress,
        )?;
    }
    let timeouts = forks
        .iter()
        .filter(|f| {
            f.error
                .as_ref()
                .is_some_and(|s| s.contains("fork timed out"))
        })
        .count();
    let errors = forks.iter().filter_map(|f| f.error.clone()).collect();
    let outputs = forks
        .into_iter()
        .filter(|f| f.error.is_none())
        .filter_map(|f| {
            f.text.map(|text| Output {
                text,
                expected_count: f.count,
            })
        })
        .collect();
    Ok((model, outputs, errors, timeouts))
}
pub fn doctor(binary: &Binary, thread: &Thread) -> Result<()> {
    round(
        binary,
        &Target::Thread(thread.clone()),
        1,
        &Settings::default(),
        Arc::new(AtomicBool::new(false)),
        &|_| {},
        false,
    )?;
    Ok(())
}
pub fn probe(
    store: &Store,
    binary: &Binary,
    target: Target,
    cfg: &Settings,
    cancel: Arc<AtomicBool>,
    progress: &dyn Fn(String),
) -> Result<Probe> {
    let _lock = store.lock("rust-probe.lock")?;
    let started = storage::iso();
    let clock = Instant::now();
    let account = store.account();
    let mut expected = target.model().to_owned();
    let mut outputs = Vec::new();
    let mut errors = Vec::new();
    let mut queries = cfg.queries;
    progress("连接私有 Codex app-server，禁用 hooks".into());
    match round(
        binary,
        &target,
        queries,
        cfg,
        cancel.clone(),
        progress,
        true,
    ) {
        Ok((model, out, err, timeouts)) => {
            expected = model;
            outputs = out;
            errors = err;
            if timeouts > 0 && !cancel.load(Ordering::Relaxed) {
                progress(format!(
                    "{timeouts} 份回答超时，补充一次样本（会额外消耗额度）"
                ));
                queries += timeouts;
                match round(
                    binary,
                    &target,
                    timeouts,
                    cfg,
                    cancel.clone(),
                    progress,
                    true,
                ) {
                    Ok((_, out, err, _)) => {
                        outputs.extend(out);
                        errors.extend(err);
                    }
                    Err(e) => errors.push(e.to_string()),
                }
            }
        }
        Err(error) => errors.push(error.to_string()),
    }
    progress("分析指纹并保存本地结果".into());
    let bank = Bank::embedded()?;
    let analysis = match modeltrace::analyze(&outputs, &bank) {
        Ok(a) => Some(a),
        Err(e) => {
            errors.push(e.to_string());
            None
        }
    };
    let scan = match &target {
        Target::Thread(t) => match store.scan(t) {
            Ok(s) => Some(s),
            Err(e) => {
                errors.push(format!("本地日志扫描失败：{e}"));
                None
            }
        },
        Target::Fresh { .. } => None,
    };
    let hard = scan
        .as_ref()
        .map(|s| {
            s.active()
                .into_iter()
                .filter(|e| e.severity == "hard")
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let cancelled = cancel.load(Ordering::Relaxed);
    let assessment = if cancelled {
        scanner::assess(&expected, None, false, &store.catalog())
    } else {
        scanner::assess(
            &expected,
            analysis.as_ref(),
            !hard.is_empty(),
            &store.catalog(),
        )
    };
    let probe = Probe {
        id: format!("{:016x}", rand::rng().random::<u64>()),
        thread_id: target.id().map(str::to_owned),
        mode: if matches!(target, Target::Fresh { .. }) {
            "fresh"
        } else {
            "fork"
        }
        .into(),
        started,
        finished: storage::iso(),
        expected,
        originator: target.originator().into(),
        codex: binary.version.clone(),
        account_id: account.id,
        status: if cancelled {
            "cancelled"
        } else if analysis.is_some() {
            "done"
        } else {
            "failed"
        }
        .into(),
        queries,
        used_outputs: analysis.as_ref().map_or(0, |a| a.used_outputs),
        elapsed_s: clock.elapsed().as_secs_f64(),
        assessment,
        analysis,
        errors,
        outputs,
        hard_evidence: hard,
    };
    store.save_probe(&probe)?;
    Ok(probe)
}
