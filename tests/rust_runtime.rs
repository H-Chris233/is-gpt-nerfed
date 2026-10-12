use is_gpt_nerfed::{
    runtime::{self, Action, Effect, Handle, ProbeTarget, Snapshot},
    storage::{self, Settings, Store},
};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

struct Fixture {
    root: PathBuf,
    store: Store,
    handle: Handle,
    revisions: Arc<Mutex<Vec<u64>>>,
    notifications: Arc<Mutex<Vec<String>>>,
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
        if scenario == "passive" {
            writeln!(fs::OpenOptions::new().append(true).open(&rollout).unwrap(),
                "{}", serde_json::json!({"type":"turn_context","timestamp":"historical","payload":{"model":"gpt-6-luna","reasoning_effort":"high"}})).unwrap();
        }
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
        let notifications = Arc::new(Mutex::new(Vec::new()));
        let alerts = notifications.clone();
        let handle = runtime::launch(store.clone(), cfg, move |effect| match effect {
            Effect::State(state) => events.lock().unwrap().push(state.revision),
            Effect::Notify(message) => alerts.lock().unwrap().push(message),
            _ => {}
        })
        .unwrap();
        Self {
            root,
            store,
            handle,
            revisions,
            notifications,
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
    fn append(&self, records: &[serde_json::Value]) {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(self.root.join("session.jsonl"))
            .unwrap();
        for record in records {
            writeln!(file, "{record}").unwrap();
        }
    }
    fn refresh(&self) -> Snapshot {
        self.handle.execute(Action::Refresh).unwrap();
        self.wait(|s| s.task.is_none());
        // A synchronous action waits until the preceding snapshot's effects have been delivered.
        self.handle
            .execute(Action::ShowMain {
                follow_status: false,
            })
            .unwrap();
        self.handle.snapshot()
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

#[test]
fn companion_follows_the_latest_active_session_instead_of_other_sessions_results() {
    use serde_json::json;
    use std::os::windows::fs::OpenOptionsExt;

    let f = Fixture::new("hang", false);
    f.wait(|s| s.last_refreshed.is_some() && s.task.is_none());
    let turn = |model: &str| json!({"type":"turn_context","timestamp":model,"payload":{"model":model,"reasoning_effort":"high"}});
    f.append(&[turn("gpt-6-luna")]);
    let rollout = f.root.join("latest.jsonl");
    fs::write(&rollout, format!("{{\"type\":\"turn_context\",\"timestamp\":\"{}\",\"payload\":{{\"model\":\"gpt-6-sol\",\"reasoning_effort\":\"high\"}}}}\n", storage::iso())).unwrap();
    let con = rusqlite::Connection::open(f.store.codex.join("state_5.sqlite")).unwrap();
    con.execute("INSERT INTO threads VALUES ('latest-thread',?1,'cli',?2,'gpt-6-sol','high',0,?3,'Latest active session','Codex Desktop')",
        rusqlite::params![rollout.to_string_lossy(), f.root.to_string_lossy(), ((storage::now() + 60.0) * 1000.0) as i64]).unwrap();
    drop(con);
    let latest = f.refresh();
    assert_eq!(latest.threads[0].id, "latest-thread");
    assert_eq!(latest.presence.thread_id.as_deref(), Some("latest-thread"));
    assert_eq!(latest.presence.model, "gpt-6-sol");
    assert_eq!(latest.presence.session_title, "Latest active session");
    assert_eq!(latest.presence.label, "被动观察中");
    assert!(
        latest.scans["user-thread"]
            .active
            .iter()
            .any(|e| e.severity == "hard")
    );

    f.append(&[json!({"type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"model":"gpt-5.6-luna"}}}), turn("gpt-5.6-luna")]);
    let soft = f.refresh();
    assert!(
        soft.scans["user-thread"]
            .active
            .iter()
            .any(|e| e.severity == "soft")
    );
    assert_eq!(soft.presence.label, "被动观察中");

    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(f.root.join("session.jsonl"))
        .unwrap();
    let failed = f.refresh();
    assert!(failed.scan_errors.contains_key("user-thread"));
    assert_eq!(failed.presence.thread_id.as_deref(), Some("latest-thread"));
    assert_eq!(failed.presence.label, "被动观察中");
    drop(locked);

    for target in [
        ProbeTarget::Thread {
            id: "user-thread".into(),
        },
        ProbeTarget::Fresh {
            model: "gpt-6.1-sol".into(),
            effort: "high".into(),
            originator: "Codex Desktop".into(),
        },
    ] {
        f.handle.execute(Action::Probe(target)).unwrap();
        let active = f.handle.snapshot();
        assert_eq!(active.task.unwrap().kind, "probe");
        assert_eq!(active.presence.thread_id.as_deref(), Some("latest-thread"));
        assert_eq!(active.presence.label, "被动观察中");
        f.handle.execute(Action::Cancel).unwrap();
        f.wait(|s| s.task.is_none());
    }

    f.handle
        .execute(Action::Probe(ProbeTarget::Thread {
            id: "latest-thread".into(),
        }))
        .unwrap();
    let active = f.handle.snapshot();
    assert_eq!(active.presence.label, "正在检测");
    assert_eq!(active.presence.thread_id.as_deref(), Some("latest-thread"));
    f.handle.execute(Action::Cancel).unwrap();
    f.wait(|s| s.task.is_none());
    assert!(
        f.handle
            .execute(Action::Probe(ProbeTarget::Thread {
                id: "missing-thread".into()
            }))
            .is_err()
    );
    let failed = f.handle.snapshot();
    assert_eq!(failed.presence.label, "操作未完成");
    assert_eq!(failed.presence.thread_id.as_deref(), Some("latest-thread"));
    assert_eq!(failed.presence.session_title, "Latest active session");
    assert_eq!(failed.presence.model, "gpt-6-sol");

    let mut previous = json!({"id":"previous-probe","thread_id":"user-thread","status":"done","verdict":"MISMATCH","expected":"gpt-6.1-sol","account_id":f.store.account().id,"finished":storage::iso(),"reference_model":"gpt-6-astra"});
    for id in [json!("user-thread"), serde_json::Value::Null] {
        previous["thread_id"] = id;
        fs::write(f.store.home.join("probes.jsonl"), format!("{previous}\n")).unwrap();
        let other = f.refresh();
        assert_eq!(other.presence.thread_id.as_deref(), Some("latest-thread"));
        assert_eq!(other.presence.label, "被动观察中");
        assert!(other.presence.probe_id.is_none());
        assert!(other.presence.reference_model.is_none());
    }

    previous["thread_id"] = json!("latest-thread");
    fs::write(f.store.home.join("probes.jsonl"), format!("{previous}\n")).unwrap();
    let outdated = f.refresh();
    assert_eq!(outdated.presence.label, "被动观察中");
    assert_eq!(outdated.presence.model, "gpt-6-sol");
    assert!(outdated.presence.reference_model.is_none());
    previous["account_id"] = json!("another-account");
    fs::write(f.store.home.join("probes.jsonl"), format!("{previous}\n")).unwrap();
    assert_eq!(f.refresh().presence.label, "被动观察中");

    previous["expected"] = json!("GPT-6-SOL");
    previous["account_id"] = json!(f.store.account().id);
    previous["reference_model"] = serde_json::Value::Null;
    fs::write(f.store.home.join("probes.jsonl"), format!("{previous}\n")).unwrap();
    let own = f.refresh();
    assert_eq!(own.presence.label, "指纹不匹配");
    assert_eq!(own.presence.model, "gpt-6-sol");
    assert_eq!(own.presence.session_title, "Latest active session");
    assert_eq!(own.presence.probe_id.as_deref(), Some("previous-probe"));
    previous["account_id"] = json!("another-account");
    fs::write(f.store.home.join("probes.jsonl"), format!("{previous}\n")).unwrap();
    assert_eq!(f.refresh().presence.label, "结果待核实");

    let con = rusqlite::Connection::open(f.store.codex.join("state_5.sqlite")).unwrap();
    con.execute(
        "UPDATE threads SET updated_at_ms=?1 WHERE id='user-thread'",
        [((storage::now() + 120.0) * 1000.0) as i64],
    )
    .unwrap();
    drop(con);
    let switched = f.refresh();
    assert_eq!(switched.threads[0].id, "user-thread");
    assert_eq!(switched.presence.thread_id.as_deref(), Some("user-thread"));
    assert_eq!(switched.presence.model, "gpt-6.1-sol");
    assert_eq!(switched.presence.session_title, "Runtime test");
    assert_eq!(switched.presence.label, "发现设置变化");
    f.handle
        .execute(Action::ShowMain {
            follow_status: true,
        })
        .unwrap();
    assert_eq!(
        f.handle.snapshot().navigation.unwrap().thread_id.as_deref(),
        Some("user-thread")
    );
}

#[test]
fn passive_observation_notifies_only_new_active_risks_and_preserves_failures() {
    use serde_json::json;
    use std::os::windows::fs::OpenOptionsExt;

    let f = Fixture::new("passive", false);
    let ready = f.wait(|s| s.last_refreshed.is_some() && s.task.is_none());
    assert!(
        ready.scans["user-thread"]
            .active
            .iter()
            .any(|e| e.severity == "hard")
    );
    assert_eq!(ready.presence.mood, "alert");
    assert!(f.notifications.lock().unwrap().is_empty());
    let turn = |model: &str| json!({"type":"turn_context","timestamp":model,"payload":{"model":model,"reasoning_effort":"high"}});

    f.append(&[
        json!({"type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"model":"gpt-5.6-luna"}}}),
        turn("gpt-5.6-luna"),
    ]);
    let soft = f.refresh();
    assert_eq!(f.notifications.lock().unwrap().len(), 1);
    assert_eq!(soft.presence.label, "发现设置变化");
    f.refresh();
    assert_eq!(f.notifications.lock().unwrap().len(), 1);

    f.append(&[turn("gpt-5.5")]);
    assert_eq!(f.refresh().presence.mood, "alert");
    assert_eq!(f.notifications.lock().unwrap().len(), 2);

    let mut settings = f.handle.snapshot().settings;
    settings.notifications = false;
    f.handle
        .execute(Action::Save {
            settings: settings.clone(),
            confirm_automatic: false,
        })
        .unwrap();
    f.append(&[turn("gpt-5.4")]);
    f.refresh();
    assert_eq!(f.notifications.lock().unwrap().len(), 2);
    settings.notifications = true;
    f.handle
        .execute(Action::Save {
            settings,
            confirm_automatic: false,
        })
        .unwrap();
    f.refresh();
    assert_eq!(f.notifications.lock().unwrap().len(), 2);

    f.append(&[turn("gpt-5.3")]);
    f.handle
        .execute(Action::Scan("user-thread".into()))
        .unwrap();
    f.wait(|s| s.task.is_none());
    f.handle
        .execute(Action::ShowMain {
            follow_status: false,
        })
        .unwrap();
    assert_eq!(f.notifications.lock().unwrap().len(), 3);

    let last_checked = f.handle.snapshot().scans["user-thread"].checked_at.clone();
    f.append(&[turn("gpt-5.2")]);
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(f.root.join("session.jsonl"))
        .unwrap();
    let failed = f.refresh();
    assert!(failed.scan_errors.contains_key("user-thread"));
    assert!(failed.scans.contains_key("user-thread"));
    assert_eq!(failed.scans["user-thread"].checked_at, last_checked);
    assert_eq!(failed.presence.label, "被动扫描未完成");
    assert_eq!(f.notifications.lock().unwrap().len(), 3);
    f.handle
        .execute(Action::Scan("user-thread".into()))
        .unwrap();
    let failed = f.wait(|s| s.task.is_none());
    assert!(failed.scan_errors.contains_key("user-thread"));
    assert_eq!(failed.scans["user-thread"].checked_at, last_checked);
    assert_eq!(failed.presence.label, "被动扫描未完成");
    drop(locked);
    f.handle
        .execute(Action::Scan("user-thread".into()))
        .unwrap();
    let recovered = f.wait(|s| s.task.is_none());
    assert!(!recovered.scan_errors.contains_key("user-thread"));
    f.handle
        .execute(Action::ShowMain {
            follow_status: false,
        })
        .unwrap();
    assert_eq!(f.notifications.lock().unwrap().len(), 4);

    f.append(&[turn("gpt-6.1-sol")]);
    let upgraded = f.refresh();
    assert_eq!(upgraded.presence.label, "记录到升级");
    assert_eq!(f.notifications.lock().unwrap().len(), 4);
    assert!(upgraded.history.is_empty());
    fs::write(
        f.root.join("session.jsonl"),
        format!("{}\n", turn("gpt-6.1-sol")),
    )
    .unwrap();
    let normal = f.refresh();
    assert_eq!(normal.presence.label, "被动观察中");
    assert_eq!(normal.presence.mood, "idle");
    assert!(serde_json::to_value(&normal.scans["user-thread"]).unwrap()["checked_at"].is_string());
    f.append(&[turn("gpt-5.1"), turn("gpt-6.1-sol")]);
    assert_eq!(f.refresh().presence.label, "记录到升级");
    assert_eq!(f.notifications.lock().unwrap().len(), 4);

    let mut previous = json!({"id":"previous-probe","thread_id":"user-thread","status":"done","verdict":"MISMATCH","expected":"gpt-6.1-sol","account_id":f.store.account().id,"finished":storage::iso()});
    fs::write(f.store.home.join("probes.jsonl"), format!("{previous}\n")).unwrap();
    assert_eq!(f.refresh().presence.label, "指纹不匹配");
    previous["account_id"] = json!("another-account");
    fs::write(f.store.home.join("probes.jsonl"), format!("{previous}\n")).unwrap();
    assert_eq!(f.refresh().presence.label, "结果待核实");
}
