use crate::{
    modeltrace::{Analysis, Output},
    scanner::{Catalog, Scan, Verdict},
};
use anyhow::{Context, Result, bail};
use base64::Engine;
use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub fn iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}
pub fn now() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}
pub fn timestamp(s: &str) -> f64 {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|v| v.timestamp_millis() as f64 / 1000.0)
        .unwrap_or(0.0)
}
#[derive(Clone)]
pub struct Store {
    pub codex: PathBuf,
    pub home: PathBuf,
}
#[derive(Default)]
pub struct ScanBatch {
    pub scans: BTreeMap<String, Scan>,
    pub errors: BTreeMap<String, String>,
}
impl Store {
    pub fn local() -> Result<Self> {
        let user = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .context("无法确定用户目录")?;
        let codex = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(user).join(".codex"));
        let home = std::env::var_os("NERFED_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| codex.join("is-gpt-nerfed"));
        fs::create_dir_all(&home)?;
        Ok(Self { codex, home })
    }
    pub fn settings(&self) -> Result<Settings> {
        let path = self.home.join("rust-desktop.json");
        if !path.exists() {
            return Ok(Settings::default());
        }
        let cfg: Settings = serde_json::from_slice(&fs::read(&path)?)
            .context("设置文件损坏，请修复或移走 rust-desktop.json")?;
        cfg.validate()?;
        Ok(cfg)
    }
    pub fn save_settings(&self, cfg: &Settings) -> Result<()> {
        cfg.validate()?;
        write_json(&self.home.join("rust-desktop.json"), cfg)
    }
    pub fn lock(&self, name: &str) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.home.join(name))?;
        file.try_lock()
            .map_err(|e| anyhow::anyhow!("检测程序或任务已在运行：{e}"))?;
        Ok(file)
    }
    pub fn database(&self) -> Result<Option<PathBuf>> {
        if !self.codex.exists() {
            return Ok(None);
        }
        let mut files = fs::read_dir(&self.codex)?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let number = name
                    .strip_prefix("state_")?
                    .strip_suffix(".sqlite")?
                    .parse::<u32>()
                    .ok()?;
                Some((number, e.path()))
            })
            .collect::<Vec<_>>();
        files.sort_by_key(|(n, _)| *n);
        Ok(files.last().map(|(_, p)| p.clone()))
    }
    pub fn threads(&self) -> Result<Vec<Thread>> {
        let Some(db) = self.database()? else {
            return Ok(Vec::new());
        };
        let con = Connection::open_with_flags(
            db,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        con.busy_timeout(Duration::from_secs(2))?;
        let columns = con
            .prepare("PRAGMA table_info(threads)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if !columns.iter().any(|s| s == "id") {
            bail!("Codex 数据库缺少 threads.id");
        }
        let names = [
            "id",
            "rollout_path",
            "thread_source",
            "cwd",
            "model",
            "reasoning_effort",
            "archived",
            "updated_at_ms",
            "title",
            "name",
            "originator",
        ];
        let projection = names
            .iter()
            .map(|s| {
                if columns.iter().any(|c| c == s) {
                    s.to_string()
                } else if *s == "updated_at_ms" && columns.iter().any(|c| c == "updated_at") {
                    "updated_at * 1000 AS updated_at_ms".into()
                } else {
                    format!("NULL AS {s}")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let query =
            format!("SELECT {projection} FROM threads ORDER BY updated_at_ms DESC LIMIT 500");
        let mut statement = con.prepare(&query)?;
        let rows = statement.query_map([], |r| {
            Ok(Thread {
                id: r.get(0)?,
                rollout_path: r.get::<_, Option<String>>(1)?.unwrap_or_default().into(),
                source: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                cwd: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                model: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                effort: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                archived: r.get::<_, Option<i64>>(6)?.unwrap_or(0) != 0,
                updated: r.get::<_, Option<i64>>(7)?.unwrap_or(0) as f64 / 1000.0,
                title: r
                    .get::<_, Option<String>>(9)?
                    .filter(|s| !s.is_empty())
                    .or(r.get::<_, Option<String>>(8)?)
                    .unwrap_or_default(),
                originator: r.get::<_, Option<String>>(10)?.unwrap_or_default(),
            })
        })?;
        Ok(rows
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(Thread::eligible)
            .take(150)
            .collect())
    }
    pub fn catalog(&self) -> Catalog {
        let data: Value = read_json(&self.codex.join("models_cache.json")).unwrap_or(Value::Null);
        let mut cat = Catalog::new();
        let models: Vec<&Value> = match &data["models"] {
            Value::Array(v) => v.iter().collect(),
            Value::Object(v) => v.values().collect(),
            _ => vec![],
        };
        for m in models {
            if let Some(slug) = m["slug"].as_str() {
                cat.insert(slug.to_lowercase(), m.clone());
            }
        }
        cat
    }
    pub fn scan(&self, thread: &Thread) -> Result<Scan> {
        let path = self
            .home
            .join("rust-scans")
            .join(format!("{}.json", safe_name(&thread.id)));
        let mut scan: Scan = if path.exists() {
            read_json(&path)?
        } else {
            Scan::default()
        };
        scan.update(&thread.rollout_path, &self.catalog())?;
        write_json(&path, &scan)?;
        Ok(scan)
    }
    pub fn cached_scan(&self, tid: &str) -> Option<Scan> {
        read_json(
            &self
                .home
                .join("rust-scans")
                .join(format!("{}.json", safe_name(tid))),
        )
        .ok()
    }
    pub fn scan_recent(
        &self,
        threads: &[Thread],
        cancel: &std::sync::atomic::AtomicBool,
    ) -> ScanBatch {
        let mut batch = ScanBatch::default();
        for t in threads {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            // One unavailable rollout must not discard every other session's refreshed state.
            let scan = if now() - t.updated < 48.0 * 3600.0 {
                match self.scan(t) {
                    Ok(scan) => Some(scan),
                    Err(error) => {
                        batch.errors.insert(t.id.clone(), format!("{error:#}"));
                        self.cached_scan(&t.id)
                    }
                }
            } else {
                self.cached_scan(&t.id)
            };
            if let Some(scan) = scan {
                batch.scans.insert(t.id.clone(), scan);
            }
        }
        batch
    }
    pub fn history(&self) -> Result<Vec<Probe>> {
        let path = self.home.join("probes.jsonl");
        if !path.exists() {
            return Ok(vec![]);
        }
        let mut rows = Vec::new();
        for line in BufReader::new(File::open(path)?).lines() {
            let line = line?;
            if let Ok(v) = serde_json::from_str::<Value>(&line)
                && let Some(id) = v["id"].as_str()
            {
                // Older Python records remain readable; never relabel their account provenance.
                let full: Value = read_json(
                    &self
                        .home
                        .join("probes")
                        .join(format!("{}.json", safe_name(id))),
                )
                .unwrap_or(v.clone());
                rows.push(Probe::from_value(&v, &full));
            }
        }
        rows.reverse();
        rows.truncate(200);
        Ok(rows)
    }
    pub fn save_probe(&self, probe: &Probe) -> Result<()> {
        write_json(
            &self
                .home
                .join("probes")
                .join(format!("{}.json", safe_name(&probe.id))),
            probe,
        )?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.home.join("probes.jsonl"))?;
        serde_json::to_writer(&mut file, probe)?;
        file.write_all(b"\n")?;
        file.sync_data()?;
        Ok(())
    }
    pub fn account(&self) -> Account {
        let d: Value = read_json(&self.codex.join("auth.json")).unwrap_or(Value::Null);
        let payload = d["tokens"]["id_token"]
            .as_str()
            .and_then(|s| s.split('.').nth(1))
            .and_then(|s| {
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(s.trim_end_matches('='))
                    .ok()
            })
            .and_then(|v| serde_json::from_slice::<Value>(&v).ok())
            .unwrap_or(Value::Null);
        let id = d["tokens"]["account_id"]
            .as_str()
            .or_else(|| payload["https://api.openai.com/auth"]["chatgpt_account_id"].as_str());
        let api = d["OPENAI_API_KEY"].as_str().filter(|s| !s.is_empty());
        let Some(id) = id.map(str::to_owned).or_else(|| {
            api.map(|s| format!("apikey:{}", &format!("{:x}", Sha256::digest(s))[..16]))
        }) else {
            return Account {
                id: "local".into(),
                label: "未检测到登录凭据".into(),
            };
        };
        let hash = format!("{:x}", Sha256::digest(id.as_bytes()));
        // Display no email or account identifier. A credential's presence is not a live login check.
        Account {
            id: hash[..12].into(),
            label: "已检测到本地登录凭据（有效性在检测时确认）".into(),
        }
    }
    pub fn defaults(&self, threads: &[Thread]) -> (String, String) {
        let cfg = fs::read_to_string(self.codex.join("config.toml"))
            .ok()
            .and_then(|s| s.parse::<toml::Value>().ok());
        let model = cfg
            .as_ref()
            .and_then(|v| v.get("model"))
            .and_then(toml::Value::as_str)
            .map(str::to_owned);
        let effort = cfg
            .as_ref()
            .and_then(|v| v.get("model_reasoning_effort"))
            .and_then(toml::Value::as_str)
            .map(str::to_owned);
        (
            model.unwrap_or_else(|| threads.first().map(|t| t.model.clone()).unwrap_or_default()),
            effort.unwrap_or_else(|| {
                threads
                    .first()
                    .map(|t| t.effort.clone())
                    .unwrap_or_default()
            }),
        )
    }
}
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(path).with_context(
        || format!("无法读取 {}", path.display()),
    )?)?)
}
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().context("missing parent directory")?;
    fs::create_dir_all(parent)?;
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = File::create(&tmp)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.sync_all()?;
    drop(file);
    crate::platform::replace_file(&tmp, path)?;
    Ok(())
}
fn safe_name(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(120)
        .collect()
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Thread {
    pub id: String,
    pub title: String,
    pub rollout_path: PathBuf,
    pub source: String,
    pub cwd: String,
    pub model: String,
    pub effort: String,
    pub archived: bool,
    pub updated: f64,
    pub originator: String,
}
impl Thread {
    pub fn eligible(&self) -> bool {
        let source = self.source.to_lowercase();
        let origin = self.originator.to_lowercase();
        !self.archived
            && self.rollout_path.is_file()
            && !self.model.contains("auto-review")
            && !["subagent", "review", "probe"]
                .iter()
                .any(|s| source.contains(s))
            && !origin.starts_with("codex_exec")
            && !origin.starts_with("is-gpt-nerfed")
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
    pub automatic: bool,
    pub interval_minutes: u32,
    pub queries: usize,
    pub timeout_seconds: u64,
    pub notifications: bool,
    pub codex_bin: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            automatic: false,
            interval_minutes: 30,
            queries: 3,
            timeout_seconds: 300,
            notifications: true,
            codex_bin: String::new(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        if !(5..=1440).contains(&self.interval_minutes)
            || !(1..=3).contains(&self.queries)
            || !(30..=600).contains(&self.timeout_seconds)
        {
            bail!("设置范围无效：间隔 5–1440 分钟、样本 1–3、超时 30–600 秒");
        }
        if !self.codex_bin.is_empty() && !Path::new(&self.codex_bin).is_file() {
            bail!("指定的 Codex 文件不存在");
        }
        if !self.codex_bin.is_empty()
            && !Path::new(&self.codex_bin)
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| {
                    ["exe", "cmd", "bat", "ps1"]
                        .iter()
                        .any(|ext| s.eq_ignore_ascii_case(ext))
                })
        {
            bail!("请选择 Codex 的 EXE、CMD、BAT 或 PS1 文件");
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Account {
    pub id: String,
    pub label: String,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Probe {
    pub id: String,
    pub thread_id: Option<String>,
    pub mode: String,
    pub started: String,
    pub finished: String,
    pub expected: String,
    pub originator: String,
    pub codex: String,
    pub account_id: String,
    pub status: String,
    pub queries: usize,
    pub used_outputs: usize,
    pub elapsed_s: f64,
    #[serde(flatten)]
    pub assessment: Verdict,
    pub analysis: Option<Analysis>,
    pub errors: Vec<String>,
    pub outputs: Vec<Output>,
    pub hard_evidence: Vec<crate::scanner::Evidence>,
}
impl Probe {
    fn from_value(v: &Value, full: &Value) -> Self {
        let s = |key: &str| {
            v[key]
                .as_str()
                .or_else(|| full[key].as_str())
                .unwrap_or("")
                .to_owned()
        };
        let analysis = serde_json::from_value(full["analysis"].clone()).ok();
        Self {
            id: s("id"),
            thread_id: v["thread_id"].as_str().map(str::to_owned),
            mode: s("mode"),
            started: s("started"),
            finished: s("finished"),
            expected: s("expected"),
            originator: s("originator"),
            codex: s("codex"),
            account_id: v["account_id"]
                .as_str()
                .or_else(|| full["account"]["id"].as_str())
                .unwrap_or("")
                .into(),
            status: s("status"),
            queries: v["queries"].as_u64().unwrap_or(0) as usize,
            used_outputs: v["used_outputs"].as_u64().unwrap_or(0) as usize,
            elapsed_s: v["elapsed_s"].as_f64().unwrap_or(0.0),
            assessment: Verdict {
                verdict: s("verdict"),
                direction: v["direction"].as_str().map(str::to_owned),
                confidence: v["confidence"].as_str().map(str::to_owned),
                p_expected: v["p_expected"].as_f64(),
                margin: v["margin"].as_f64(),
            },
            analysis,
            errors: full["errors"]
                .as_array()
                .map(|v| {
                    v.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            outputs: vec![],
            hard_evidence: serde_json::from_value(full["hard_evidence"].clone())
                .unwrap_or_default(),
        }
    }
    pub fn label(&self) -> &str {
        verdict_label(&self.assessment.verdict)
    }
}
pub fn verdict_label(v: &str) -> &str {
    match v {
        "MATCH" => "指纹匹配",
        "SUSPICIOUS" => "指纹可疑",
        "UNLISTED" => "模型未收录",
        "INVALID" => "无有效结果",
        "UNKNOWN" => "缺少所选模型",
        "DOWNGRADED!" => "日志发现降级",
        "MISMATCH" => "指纹不匹配",
        _ => "未检测",
    }
}

pub fn due_thread<'a>(
    threads: &'a [Thread],
    history: &[Probe],
    seen: &mut BTreeMap<String, f64>,
    attempts: &BTreeMap<String, f64>,
    cfg: &Settings,
    account: &str,
    time: f64,
) -> Option<&'a Thread> {
    if !cfg.automatic {
        return None;
    }
    let interval = cfg.interval_minutes as f64 * 60.0;
    threads.iter().find(|t| {
        let first = *seen.entry(t.id.clone()).or_insert(time);
        let last = history
            .iter()
            .filter(|p| p.thread_id.as_deref() == Some(&t.id) && p.account_id == account)
            .map(|p| timestamp(&p.finished))
            .fold(attempts.get(&t.id).copied().unwrap_or(0.0), f64::max);
        let base = if last > 0.0 { last } else { first };
        time - t.updated < 15.0 * 60.0 && t.updated > last && time - base >= interval
    })
}
