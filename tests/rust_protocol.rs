use is_gpt_nerfed::{
    appserver::{self, Target},
    platform::{Binary, discover},
    storage::{Settings, Store, Thread},
};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
struct Fixture {
    root: PathBuf,
    store: Store,
    binary: Binary,
    thread: Thread,
    log: PathBuf,
}
impl Fixture {
    fn new(scenario: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("nerfed 中文 Space {}", rand::random::<u64>()));
        fs::create_dir_all(&root).unwrap();
        let log = root.join("rpc.jsonl");
        let binary_path = root.join("假 Codex.cmd");
        fs::write(
            &binary_path,
            format!(
                "@echo off\r\nchcp 65001 >nul\r\n\"{}\" --scenario={scenario} \"--log={}\" %*\r\n",
                env!("CARGO_BIN_EXE_nerfed-test-server"),
                log.display()
            ),
        )
        .unwrap();
        let rollout = root.join("rollout.jsonl");
        fs::write(&rollout, "{}\n").unwrap();
        let store = Store {
            codex: root.join("codex"),
            home: root.join("ledger"),
        };
        fs::create_dir_all(&store.home).unwrap();
        let thread = Thread {
            id: "target-user-thread".into(),
            title: "中文会话".into(),
            rollout_path: rollout,
            source: "cli".into(),
            cwd: root.display().to_string(),
            model: if scenario == "unlisted" {
                "gpt-6.1-sol"
            } else {
                "gpt-6-astra"
            }
            .into(),
            effort: "high".into(),
            archived: false,
            updated: 1.0,
            originator: "Codex Desktop".into(),
        };
        let binary = Binary {
            path: binary_path,
            version: "0.160.0".into(),
            no_daemon: true,
        };
        Self {
            root,
            store,
            binary,
            thread,
            log,
        }
    }
    fn run(&self) -> is_gpt_nerfed::storage::Probe {
        appserver::probe(
            &self.store,
            &self.binary,
            Target::Thread(self.thread.clone()),
            &Settings::default(),
            Arc::new(AtomicBool::new(false)),
            &|_| {},
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn fork_settings_hooks_and_utf8_paths_are_preserved() {
    let f = Fixture::new("match");
    let p = f.run();
    assert_eq!(p.assessment.verdict, "MATCH");
    assert_eq!(p.used_outputs, 3);
    let messages = fs::read_to_string(&f.log)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str::<serde_json::Value>(s).unwrap())
        .collect::<Vec<_>>();
    assert!(
        messages
            .iter()
            .filter_map(|m| m["startup_args"].as_array())
            .all(|args| args.iter().any(|a| a == "features.hooks=false"))
    );
    for m in messages.iter().filter(|m| m["method"] == "thread/fork") {
        assert_eq!(m["params"]["threadId"], f.thread.id);
        assert_eq!(m["params"]["ephemeral"], true);
        assert_eq!(m["params"]["config"]["model_reasoning_effort"], "high");
    }
    assert_eq!(
        messages
            .iter()
            .filter(|m| m["method"] == "turn/start")
            .count(),
        3
    );
    assert_eq!(f.store.history().unwrap()[0].assessment.verdict, "MATCH");
}

#[test]
fn hook_inspection_filters_other_plugins_and_never_starts_inference() {
    let f = Fixture::new("match");
    let status = appserver::inspect_hooks(&f.binary, Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(status.hooks.len(), 1);
    assert!(status.hooks[0].legacy);
    assert!(status.hooks[0].trusted);
    assert_eq!(status.notes.len(), 1);
    let text = fs::read_to_string(&f.log).unwrap();
    assert!(text.contains("features.hooks=true"));
    assert!(text.contains("hooks/list"));
    assert!(!text.contains("turn/start"));
}

#[test]
#[ignore = "read-only inspection of this plugin's installed hooks"]
fn live_hook_inspection() {
    let binary = discover("").unwrap().expect("Codex not found");
    let status = appserver::inspect_hooks(&binary, Arc::new(AtomicBool::new(false))).unwrap();
    println!(
        "is-gpt-nerfed hooks: {}; notes: {}",
        status.hooks.len(),
        status.notes.len()
    );
    for hook in status.hooks {
        println!(
            "{} enabled={} trusted={} legacy={}",
            hook.event, hook.enabled, hook.trusted, hook.legacy
        );
    }
}
#[test]
fn unlisted_hinted_and_busy_fallback_are_handled() {
    for scenario in ["unlisted", "hinted", "fallback"] {
        let f = Fixture::new(scenario);
        let p = f.run();
        assert_eq!(
            p.assessment.verdict,
            if scenario == "unlisted" {
                "UNLISTED"
            } else {
                "MATCH"
            },
            "{scenario}"
        );
    }
}
#[test]
fn tools_approvals_failed_turns_and_persistent_forks_are_rejected() {
    for scenario in ["tool", "approval", "failed", "persistent"] {
        let f = Fixture::new(scenario);
        let p = f.run();
        assert_eq!(p.assessment.verdict, "INVALID", "{scenario}");
        assert_eq!(p.used_outputs, 0);
    }
}
#[test]
fn cancel_reclaims_private_server_without_waiting_for_probe_timeout() {
    let f = Fixture::new("hang");
    let cancel = Arc::new(AtomicBool::new(false));
    let trigger = cancel.clone();
    let start = Instant::now();
    let timer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        trigger.store(true, Ordering::Relaxed);
    });
    let p = appserver::probe(
        &f.store,
        &f.binary,
        Target::Thread(f.thread.clone()),
        &Settings::default(),
        cancel,
        &|_| {},
    )
    .unwrap();
    timer.join().unwrap();
    assert_eq!(p.status, "cancelled");
    assert_eq!(p.assessment.verdict, "INVALID");
    assert!(start.elapsed() < Duration::from_secs(5));
}
#[test]
fn doctor_creates_only_a_fork_and_no_inference() {
    let f = Fixture::new("match");
    appserver::doctor(&f.binary, &f.thread).unwrap();
    let text = fs::read_to_string(&f.log).unwrap();
    assert!(text.contains("thread/fork"));
    assert!(!text.contains("turn/start"));
}
#[test]
#[ignore = "read-only live Codex fork check; no inference"]
fn live_codex_doctor() {
    let store = Store::local().unwrap();
    let threads = store.threads().unwrap();
    let binary = discover("").unwrap().expect("Codex not found");
    println!("Codex {} at {}", binary.version, binary.path.display());
    let t = threads.first().expect("no saved user thread");
    appserver::doctor(&binary, t).unwrap();
    println!("live ephemeral fork succeeded; 0 inference requests");
}

#[test]
#[ignore = "one real three-answer fingerprint acceptance run"]
fn live_fingerprint_probe() {
    let store = Store::local().unwrap();
    let threads = store.threads().unwrap();
    let current = std::env::var("CODEX_THREAD_ID").ok();
    let target = threads
        .iter()
        .find(|t| Some(&t.id) == current.as_ref())
        .or_else(|| threads.first())
        .expect("no saved user thread");
    let binary = discover("").unwrap().expect("Codex not found");
    println!(
        "Target model {} @ {}; Codex {}",
        target.model, target.effort, binary.version
    );
    let result = appserver::probe(
        &store,
        &binary,
        Target::Thread(target.clone()),
        &Settings::default(),
        Arc::new(AtomicBool::new(false)),
        &|stage| println!("{stage}"),
    )
    .unwrap();
    println!(
        "Verdict {}; valid answers {}/{}; elapsed {:.1}s; record {}",
        result.assessment.verdict, result.used_outputs, result.queries, result.elapsed_s, result.id
    );
    for error in &result.errors {
        println!("Probe error: {error}");
    }
    assert_eq!(result.status, "done");
    assert_eq!(result.used_outputs, 3);
}
