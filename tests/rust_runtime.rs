use is_gpt_nerfed::{
    runtime::{self, Action, Effect, Handle, ProbeTarget, Snapshot},
    storage::{self, Settings, Store},
};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

struct Fixture {
    root: PathBuf,
    store: Store,
    handle: Handle,
    revisions: Arc<Mutex<Vec<u64>>>,
}
impl Fixture {
    fn new(scenario: &str, automatic: bool) -> Self {
        let root =
            std::env::temp_dir().join(format!("nerfed runtime 中文 {}", rand::random::<u64>()));
        let store = Store {
            codex: root.join("codex"),
            home: root.join("ledger"),
        };
        fs::create_dir_all(&store.codex).unwrap();
        fs::create_dir_all(&store.home).unwrap();
        fs::write(
            store.codex.join("auth.json"),
            r#"{"tokens":{"account_id":"runtime-test-account"}}"#,
        )
        .unwrap();
        let rollout = root.join("session.jsonl");
        fs::write(&rollout, format!("{{\"type\":\"turn_context\",\"timestamp\":\"{}\",\"payload\":{{\"model\":\"gpt-6.1-sol\",\"reasoning_effort\":\"high\"}}}}\n", storage::iso())).unwrap();
        let con = rusqlite::Connection::open(store.codex.join("state_5.sqlite")).unwrap();
        con.execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, thread_source TEXT, cwd TEXT, model TEXT, reasoning_effort TEXT, archived INTEGER, updated_at_ms INTEGER, title TEXT, originator TEXT);").unwrap();
        con.execute("INSERT INTO threads VALUES ('user-thread',?1,'cli',?2,'gpt-6.1-sol','high',0,?3,'Runtime test','Codex Desktop')",
            rusqlite::params![rollout.to_string_lossy(), root.to_string_lossy(), (storage::now() * 1000.0) as i64]).unwrap();
        drop(con);
        let binary = root.join("fake codex.cmd");
        fs::write(
            &binary,
            format!(
                "@echo off\r\nchcp 65001 >nul\r\n\"{}\" --scenario={scenario} %*\r\n",
                env!("CARGO_BIN_EXE_nerfed-test-server")
            ),
        )
        .unwrap();
        let cfg = Settings {
            automatic,
            interval_minutes: 5,
            timeout_seconds: 30,
            codex_bin: binary.to_string_lossy().into_owned(),
            ..Default::default()
        };
        store.save_settings(&cfg).unwrap();
        if scenario == "shared" {
            let old = serde_json::json!({
                "id": "previous-observation", "thread_id": "user-thread", "mode": "fork", "status": "done",
                "expected": "gpt-6.1-sol", "verdict": "UNLISTED", "queries": 3, "used_outputs": 3,
                "account_id": store.account().id,
                "finished": (chrono::Utc::now() - chrono::Duration::minutes(6)).to_rfc3339()
            });
            fs::write(store.home.join("probes.jsonl"), format!("{old}\n")).unwrap();
        }
        let revisions = Arc::new(Mutex::new(Vec::new()));
        let events = revisions.clone();
        let handle = runtime::launch(store.clone(), cfg, move |effect| {
            if let Effect::State(state) = effect {
                events.lock().unwrap().push(state.revision);
            }
        })
        .unwrap();
        Self {
            root,
            store,
            handle,
            revisions,
        }
    }
    fn wait(&self, condition: impl Fn(&Snapshot) -> bool) -> Snapshot {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let s = self.handle.snapshot();
            if condition(&s) {
                return s;
            }
            assert!(
                Instant::now() < deadline,
                "runtime condition timed out: {} {:?}",
                s.status,
                s.error
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.handle.post(Action::Quit);
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.store.lock("rust-desktop.lock").is_err() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn automatic_detection_and_snapshots_work_without_any_window() {
    let f = Fixture::new("shared", true);
    let done = f.wait(|s| s.task.is_none() && s.history.len() == 2);
    assert_eq!(done.history.len(), 2);
    assert_eq!(done.history[0].used_outputs, 3);
    assert_eq!(done.presence.mood, "match");
    assert_eq!(done.presence.thread_id.as_deref(), Some("user-thread"));
    assert_eq!(done.presence.label, "参考指纹匹配");
    assert_eq!(
        done.presence.reference_model.as_deref(),
        Some("gpt-6-astra")
    );
    assert_eq!(done.history[1].assessment.verdict, "UNLISTED");
    let reopened = f.handle.snapshot();
    assert_eq!(reopened.history[0].id, done.history[0].id);
    let serialized = serde_json::to_value(&reopened).unwrap();
    assert!(serialized["threads"][0].get("rollout_path").is_none());
    f.handle
        .execute(Action::ShowMain {
            follow_status: true,
        })
        .unwrap();
    assert_eq!(
        f.handle.snapshot().navigation.unwrap().thread_id.as_deref(),
        Some("user-thread")
    );
    let revisions = f.revisions.lock().unwrap();
    assert!(revisions.windows(2).all(|v| v[0] < v[1]));
}

#[test]
fn duplicate_jobs_are_rejected_and_ui_actions_do_not_cancel_the_running_probe() {
    let f = Fixture::new("hang", false);
    let ready = f.wait(|s| s.last_refreshed.is_some() && s.task.is_none());
    assert!(
        f.handle
            .execute(Action::Probe(ProbeTarget::Thread {
                id: "unknown-thread".into()
            }))
            .is_err()
    );
    let target = || ProbeTarget::Thread {
        id: "user-thread".into(),
    };
    f.handle.execute(Action::Probe(target())).unwrap();
    assert!(f.handle.execute(Action::Probe(target())).is_err());
    f.handle
        .execute(Action::ShowMain {
            follow_status: true,
        })
        .unwrap();
    f.handle.execute(Action::Companion(true)).unwrap();
    assert_eq!(f.handle.snapshot().task.unwrap().kind, "probe");
    assert_eq!(f.store.settings().unwrap().queries, ready.settings.queries);
    let started = Instant::now();
    f.handle.execute(Action::Cancel).unwrap();
    let done = f.wait(|s| s.task.is_none() && !s.history.is_empty());
    assert_eq!(done.history[0].status, "cancelled");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(
        serde_json::from_value::<ProbeTarget>(
            serde_json::json!({"kind":"thread","id":"user-thread","rollout_path":"C:/forged.jsonl"})
        )
        .is_err()
    );
}

#[test]
fn automatic_consent_and_saved_settings_survive_independent_window_actions() {
    let f = Fixture::new("shared", false);
    let ready = f.wait(|s| s.last_refreshed.is_some() && s.task.is_none());
    let mut draft = ready.settings.clone();
    draft.queries = 1;
    draft.interval_minutes = 91;
    f.handle.execute(Action::ToggleAutomatic).unwrap();
    assert!(f.handle.snapshot().confirm_auto);
    assert!(!f.store.settings().unwrap().automatic);
    f.handle.execute(Action::Companion(true)).unwrap();
    let saved = f.store.settings().unwrap();
    assert_eq!(saved.queries, ready.settings.queries);
    assert_eq!(saved.interval_minutes, ready.settings.interval_minutes);
    draft.automatic = true;
    assert!(
        f.handle
            .execute(Action::Save {
                settings: draft,
                confirm_automatic: false
            })
            .is_err()
    );
    f.handle.execute(Action::ConfirmAutomatic(false)).unwrap();
    assert!(!f.handle.snapshot().confirm_auto);
    f.handle.execute(Action::ToggleAutomatic).unwrap();
    f.handle.execute(Action::ConfirmAutomatic(true)).unwrap();
    assert!(f.store.settings().unwrap().automatic);
    assert_eq!(f.store.settings().unwrap().queries, 3);
    f.wait(|s| s.history.len() == 2);
}
