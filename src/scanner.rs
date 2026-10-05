use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::LazyLock,
};

pub const EFFORTS: &[&str] = &[
    "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];
pub type Catalog = BTreeMap<String, Value>;

pub fn normalize(model: &str) -> String {
    let model = model.trim().to_lowercase();
    model
        .split_once('/')
        .map_or(model.clone(), |(_, s)| s.to_string())
}
fn generation(model: &str) -> Option<(u32, u32)> {
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?:^|-)(\d+)(?:[.\-](\d{1,2}))?(?:$|-)").unwrap());
    let m = RE.captures(model)?;
    Some((
        m[1].parse().ok()?,
        m.get(2).and_then(|v| v.as_str().parse().ok()).unwrap_or(0),
    ))
}
fn size(model: &str) -> u8 {
    if model.split('-').any(|s| matches!(s, "pro" | "ultra")) {
        return 3;
    }
    if model.contains("nano") {
        return 0;
    }
    if [
        "mini",
        "spark",
        "lite",
        "small",
        "flash",
        "reserve",
        "auto-review",
    ]
    .iter()
    .any(|tag| model.contains(tag))
    {
        return 1;
    }
    2
}
fn effort(s: &str) -> Option<usize> {
    EFFORTS.iter().position(|e| *e == s.to_lowercase())
}
fn max_effort(v: &Value) -> Option<usize> {
    v["supported_reasoning_levels"]
        .as_array()?
        .iter()
        .filter_map(|v| effort(v.as_str().or_else(|| v["effort"].as_str())?))
        .max()
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Comparison {
    pub direction: String,
    pub confidence: String,
    pub reason: String,
}
pub fn compare(old: &str, new: &str, cat: &Catalog) -> Comparison {
    let a = normalize(old);
    let b = normalize(new);
    let av = cat.get(&a).unwrap_or(&Value::Null);
    let bv = cat.get(&b).unwrap_or(&Value::Null);
    let out = |direction: &str, confidence: &str, reason: String| Comparison {
        direction: direction.into(),
        confidence: confidence.into(),
        reason,
    };
    if a == b {
        return out("lateral", "high", "same model".into());
    }
    if av["upgrade"].as_str().is_some_and(|v| normalize(v) == b) {
        return out(
            "upgrade",
            "high",
            format!("Codex's catalog names {b} as the successor of {a}"),
        );
    }
    if bv["upgrade"].as_str().is_some_and(|v| normalize(v) == a) {
        return out(
            "downgrade",
            "high",
            format!("Codex's catalog names {a} as the successor of {b}"),
        );
    }
    let ah = av["visibility"]
        .as_str()
        .is_some_and(|s| s.eq_ignore_ascii_case("hide"));
    let bh = bv["visibility"]
        .as_str()
        .is_some_and(|s| s.eq_ignore_ascii_case("hide"));
    if bh && !ah {
        return out(
            "downgrade",
            "high",
            format!("{b} is an internal model Codex does not list"),
        );
    }
    if ah && !bh {
        return out(
            "upgrade",
            "medium",
            format!("back from the internal model {a}"),
        );
    }
    if let (Some(ag), Some(bg)) = (generation(&a), generation(&b))
        && ag != bg
    {
        let newer = bg > ag;
        let fmt = |v: (u32, u32)| {
            if v.1 == 0 {
                v.0.to_string()
            } else {
                format!("{}.{}", v.0, v.1)
            }
        };
        return out(
            if newer { "upgrade" } else { "downgrade" },
            if size(&a) == size(&b) {
                "high"
            } else {
                "medium"
            },
            format!(
                "{} generation ({} vs {}){}",
                if newer { "newer" } else { "older" },
                fmt(bg),
                fmt(ag),
                if size(&a) == size(&b) {
                    ""
                } else {
                    " but a different size tier"
                }
            ),
        );
    }
    if size(&a) != size(&b) {
        let larger = size(&b) > size(&a);
        return out(
            if larger { "upgrade" } else { "downgrade" },
            "high",
            format!("{} size tier", if larger { "larger" } else { "smaller" }),
        );
    }
    if let (Some(ap), Some(bp)) = (av["priority"].as_f64(), bv["priority"].as_f64())
        && ap != bp
    {
        let above = bp < ap;
        return out(
            if above { "upgrade" } else { "downgrade" },
            "medium",
            format!(
                "Codex lists {b} {} {a}",
                if above { "above" } else { "below" }
            ),
        );
    }
    if let (Some(ae), Some(be)) = (max_effort(av), max_effort(bv))
        && ae != be
    {
        let higher = be > ae;
        return out(
            if higher { "upgrade" } else { "downgrade" },
            "low",
            format!(
                "supports a {} top reasoning effort",
                if higher { "higher" } else { "lower" }
            ),
        );
    }
    if let (Some(ac), Some(bc)) = (av["context_window"].as_u64(), bv["context_window"].as_u64())
        && ac > 0
        && bc > 0
        && ac != bc
    {
        let larger = bc > ac;
        return out(
            if larger { "upgrade" } else { "downgrade" },
            "low",
            format!(
                "{} context window",
                if larger { "larger" } else { "smaller" }
            ),
        );
    }
    out(
        "lateral",
        "low",
        "same generation and size; the catalog does not rank them".into(),
    )
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Evidence {
    pub ts: String,
    pub kind: String,
    pub severity: String,
    pub detail: String,
    pub turn_id: Option<String>,
    pub usage_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}
impl Evidence {
    pub fn dimension(&self) -> &str {
        match self.kind.as_str() {
            "silent_model_change" | "applied_model_change" | "hidden_model" => "model",
            "silent_effort_change" | "applied_effort_change" => "effort",
            "context_window_change" => "ctx",
            "service_tier_change" => "tier",
            _ => &self.kind,
        }
    }
}
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Scan {
    pub cursor: u64,
    pub model: Option<String>,
    pub first_model: Option<String>,
    pub effort: Option<String>,
    pub ctx: Option<u64>,
    pub tier: Option<String>,
    pub turns: u64,
    pub originator: Option<String>,
    pub cli_version: Option<String>,
    pub models_seen: BTreeMap<String, u64>,
    pub efforts_seen: BTreeMap<String, u64>,
    pub applied: Value,
    pub usage_percent: Option<f64>,
    pub evidence: Vec<Evidence>,
}
fn text(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}
impl Scan {
    fn add(
        &mut self,
        kind: &str,
        severity: &str,
        detail: String,
        p: &Value,
        ts: &Value,
        why: Option<String>,
    ) {
        let ts = text(ts).unwrap_or_else(crate::storage::iso);
        if self
            .evidence
            .iter()
            .any(|e| e.kind == kind && e.detail == detail && e.ts == ts)
        {
            return;
        }
        self.evidence.push(Evidence {
            ts,
            kind: kind.into(),
            severity: severity.into(),
            detail,
            turn_id: text(&p["turn_id"]),
            usage_percent: self.usage_percent,
            why,
        });
    }
    pub fn feed(&mut self, rec: &Value, cat: &Catalog) {
        let p = &rec["payload"];
        let ts = &rec["timestamp"];
        match rec["type"].as_str().unwrap_or("") {
            "session_meta" => {
                self.originator = text(&p["originator"]);
                self.cli_version = text(&p["cli_version"]);
            }
            "turn_context" => {
                self.turns += 1;
                if let Some(model) = text(&p["model"]) {
                    *self.models_seen.entry(model.clone()).or_default() += 1;
                    if cat.get(&model.to_lowercase()).is_some_and(|v| {
                        v["visibility"]
                            .as_str()
                            .is_some_and(|s| s.eq_ignore_ascii_case("hide"))
                    }) {
                        self.add(
                            "hidden_model",
                            "hard",
                            format!("hidden/internal model {model} ran a turn"),
                            p,
                            ts,
                            None,
                        );
                    }
                    if let Some(old) = self.model.clone() {
                        if old != model {
                            let cmp = compare(&old, &model, cat);
                            let good = cmp.direction == "upgrade"
                                && matches!(cmp.confidence.as_str(), "high" | "medium");
                            if self.applied["model"].as_str() == Some(&model) {
                                self.add(
                                    "applied_model_change",
                                    if cmp.direction == "downgrade" {
                                        "soft"
                                    } else if good {
                                        "good"
                                    } else {
                                        "info"
                                    },
                                    format!(
                                        "thread settings switched model {old} → {model} ({})",
                                        cmp.direction
                                    ),
                                    p,
                                    ts,
                                    Some(cmp.reason),
                                );
                            } else {
                                self.add(
                                    "silent_model_change",
                                    if cmp.direction == "downgrade" {
                                        "hard"
                                    } else if good {
                                        "good"
                                    } else {
                                        "soft"
                                    },
                                    format!(
                                        "model {old} → {model} with no settings change ({})",
                                        cmp.direction
                                    ),
                                    p,
                                    ts,
                                    Some(cmp.reason),
                                );
                            }
                        }
                    } else {
                        self.first_model = Some(model.clone());
                    }
                    self.model = Some(model);
                }
                if let Some(new) = text(&p["collaboration_mode"]["settings"]["reasoning_effort"])
                    .or_else(|| text(&p["reasoning_effort"]))
                    .or_else(|| text(&p["effort"]))
                {
                    *self.efforts_seen.entry(new.clone()).or_default() += 1;
                    if let Some(old) = self.effort.clone()
                        && old != new
                    {
                        let lower = matches!((effort(&old),effort(&new)),(Some(a),Some(b)) if b<a);
                        if self.applied["effort"].as_str() == Some(&new) {
                            self.add(
                                "applied_effort_change",
                                if lower { "soft" } else { "info" },
                                format!("thread settings switched reasoning effort {old} → {new}"),
                                p,
                                ts,
                                None,
                            );
                        } else {
                            self.add(
                                "silent_effort_change",
                                if lower { "hard" } else { "soft" },
                                format!("reasoning effort {old} → {new} with no settings change"),
                                p,
                                ts,
                                None,
                            );
                        }
                    }
                    self.effort = Some(new);
                }
            }
            "event_msg" => match p["type"].as_str().unwrap_or("") {
                "thread_settings_applied" => {
                    let s = &p["thread_settings"];
                    self.applied = serde_json::json!({"model":s["model"],"effort":s["reasoning_effort"],"tier":s["service_tier"]});
                    if let Some(tier) = text(&s["service_tier"]) {
                        if let Some(old) = self.tier.clone()
                            && old != tier
                        {
                            self.add(
                                "service_tier_change",
                                "info",
                                format!("service tier {old} → {tier}"),
                                p,
                                ts,
                                None,
                            );
                        }
                        self.tier = Some(tier);
                    }
                }
                "token_count" => {
                    if p["rate_limits"]["limit_name"]
                        .as_str()
                        .is_none_or(str::is_empty)
                        && let Some(pct) = p["rate_limits"]["primary"]["used_percent"].as_f64()
                    {
                        self.usage_percent = Some(pct);
                    }
                    if let Some(ctx) = p["info"]["model_context_window"]
                        .as_u64()
                        .filter(|n| *n > 0)
                    {
                        if let Some(old) = self.ctx
                            && old != ctx
                        {
                            self.add(
                                "context_window_change",
                                if ctx < old { "hard" } else { "info" },
                                format!("model context window {old} → {ctx}"),
                                p,
                                ts,
                                None,
                            );
                        }
                        self.ctx = Some(ctx);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    pub fn active(&self) -> Vec<&Evidence> {
        let mut latest = BTreeMap::new();
        for e in &self.evidence {
            latest.insert(e.dimension(), e);
        }
        latest
            .into_values()
            .filter(|e| matches!(e.severity.as_str(), "hard" | "soft" | "good"))
            .collect()
    }
    pub fn update(&mut self, path: &Path, cat: &Catalog) -> Result<()> {
        let mut file = File::open(path)?;
        let size = file.metadata()?.len();
        if self.cursor > size {
            *self = Self::default();
        }
        file.seek(SeekFrom::Start(self.cursor))?;
        let mut data = Vec::new();
        file.take(64 << 20).read_to_end(&mut data)?;
        if let Some(end) = data.iter().rposition(|b| *b == b'\n') {
            for line in data[..end].split(|b| *b == b'\n') {
                if let Ok(rec) = serde_json::from_slice::<Value>(line) {
                    self.feed(&rec, cat);
                }
            }
            self.cursor += end as u64 + 1;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Verdict {
    pub verdict: String,
    pub direction: Option<String>,
    pub confidence: Option<String>,
    pub p_expected: Option<f64>,
    pub margin: Option<f64>,
}
pub fn assess(
    expected: &str,
    analysis: Option<&crate::modeltrace::Analysis>,
    hard: bool,
    cat: &Catalog,
) -> Verdict {
    let mut out = Verdict {
        verdict: "INVALID".into(),
        direction: None,
        confidence: None,
        p_expected: None,
        margin: None,
    };
    if hard {
        out.verdict = "DOWNGRADED!".into();
        out.direction = Some("hard".into());
        return out;
    }
    let Some(a) = analysis else {
        return out;
    };
    let Some(top) = a.results.first() else {
        return out;
    };
    let exp = normalize(expected);
    if exp.is_empty() {
        out.verdict = "UNKNOWN".into();
        return out;
    }
    let Some(e) = a.results.iter().find(|r| r.model == exp) else {
        out.verdict = "UNLISTED".into();
        return out;
    };
    out.p_expected = Some(e.probability);
    out.margin = Some(top.score - e.score);
    if top.model == exp {
        out.verdict = "MATCH".into();
        out.confidence = Some(
            if top.probability >= 0.8 {
                "high"
            } else {
                "low"
            }
            .into(),
        );
        return out;
    }
    let confident = top.probability + 1e-9 >= 0.8
        && e.probability <= 0.2 + 1e-9
        && top.score - e.score + 1e-9 >= 0.5
        && a.used_outputs >= 2;
    out.verdict = if confident { "MISMATCH" } else { "SUSPICIOUS" }.into();
    out.direction = Some(compare(&exp, &top.model, cat).direction);
    out.confidence = Some(if confident { "high" } else { "low" }.into());
    out
}
