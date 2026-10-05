use is_gpt_nerfed::{
    modeltrace::{self, Bank, Output},
    scanner::{self, Scan},
    storage::{Settings, Store, Thread, due_thread},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf};

fn temp() -> PathBuf {
    let p = std::env::temp_dir().join(format!("nerfed tests 中文 {}", rand::random::<u64>()));
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn one_missing_rollout_does_not_break_refresh_or_other_sessions() {
    let root = temp();
    let store = Store {
        codex: root.join("codex"),
        home: root.join("ledger"),
    };
    let good_path = root.join("good.jsonl");
    fs::write(
        &good_path,
        "{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-6.1-sol\"}}\n",
    )
    .unwrap();
    let good = Thread {
        id: "good".into(),
        title: "test".into(),
        rollout_path: good_path,
        source: "cli".into(),
        cwd: String::new(),
        model: "gpt-6.1-sol".into(),
        effort: "high".into(),
        archived: false,
        updated: is_gpt_nerfed::storage::now(),
        originator: "Codex Desktop".into(),
    };
    let bad = Thread {
        id: "bad".into(),
        rollout_path: root.join("removed.jsonl"),
        ..good.clone()
    };
    let batch = store.scan_recent(&[bad, good], &std::sync::atomic::AtomicBool::new(false));
    assert_eq!(batch.scans.len(), 1);
    assert_eq!(batch.scans["good"].turns, 1);
    assert_eq!(batch.errors.len(), 1);
    assert!(batch.errors.contains_key("bad"));
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn scorer_matches_python_reference_to_nine_decimal_places() {
    let bank = Bank::embedded().unwrap();
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/rust_parity.json")).unwrap();
    assert_eq!(cases.len(), 25);
    for (i, case) in cases.iter().enumerate() {
        let outputs: Vec<Output> = serde_json::from_value(case["outputs"].clone()).unwrap();
        let got = modeltrace::analyze(&outputs, &bank).unwrap();
        let want = &case["analysis"];
        assert_eq!(got.prediction, want["prediction"], "case {i}");
        assert_eq!(
            got.used_outputs as u64,
            want["used_outputs"].as_u64().unwrap()
        );
        for (actual, expected) in got.results.iter().zip(want["results"].as_array().unwrap()) {
            assert_eq!(actual.model, expected["model"]);
            for (key, v) in [
                ("probability", actual.probability),
                ("score", actual.score),
                ("profile_similarity", actual.profile_similarity),
            ] {
                assert!(
                    (v - expected[key].as_f64().unwrap()).abs() < 1e-9,
                    "case {i}: {key} differs for {}",
                    actual.model
                );
            }
        }
    }
}
#[test]
fn scanner_matches_original_classification_and_reverted_evidence() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/rust_scanner_parity.json")).unwrap();
    for case in cases {
        let mut scan = Scan::default();
        for rec in case["records"].as_array().unwrap() {
            scan.feed(rec, &BTreeMap::new());
        }
        assert_eq!(scan.model.as_deref(), case["model"].as_str());
        assert_eq!(scan.effort.as_deref(), case["effort"].as_str());
        assert_eq!(scan.ctx, case["ctx"].as_u64());
        assert_eq!(scan.turns, case["turns"].as_u64().unwrap());
        assert_eq!(
            serde_json::to_value(&scan.evidence).unwrap(),
            case["evidence"]
        );
        let mut got = serde_json::to_value(scan.active())
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        let mut want = case["active"].as_array().unwrap().clone();
        got.sort_by_key(|v| v["kind"].to_string());
        want.sort_by_key(|v| v["kind"].to_string());
        assert_eq!(got, want);
    }
}
#[test]
fn unknown_model_and_single_answer_never_become_confident_mismatch() {
    let bank = Bank::embedded().unwrap();
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/rust_parity.json")).unwrap();
    let outputs: Vec<Output> = serde_json::from_value(cases[0]["outputs"].clone()).unwrap();
    let a = modeltrace::analyze(&outputs, &bank).unwrap();
    assert_eq!(
        scanner::assess("gpt-6.1-sol", Some(&a), false, &BTreeMap::new()).verdict,
        "UNLISTED"
    );
    assert_eq!(
        scanner::assess("gpt-6-astra", Some(&a), false, &BTreeMap::new()).verdict,
        "SUSPICIOUS"
    );
    assert_eq!(
        scanner::assess("gpt-6.1-sol", None, false, &BTreeMap::new()).verdict,
        "INVALID"
    );
    assert_eq!(
        scanner::assess("gpt-6.1-sol", Some(&a), true, &BTreeMap::new()).verdict,
        "DOWNGRADED!"
    );
    assert_eq!(
        modeltrace::parse_numbers("first 5 17 then 一二三 44 44 33"),
        vec![44, 44, 33]
    );
}
#[test]
fn scanner_keeps_incomplete_lines_and_resets_when_file_shrinks() {
    let root = temp();
    let path = root.join("rollout.jsonl");
    let one=json!({"type":"turn_context","timestamp":"t1","payload":{"model":"gpt-6-astra","effort":"high"}}).to_string();
    fs::write(&path, format!("{one}\n{{\"type\":")).unwrap();
    let mut scan = Scan::default();
    scan.update(&path, &BTreeMap::new()).unwrap();
    assert_eq!(scan.turns, 1);
    scan.update(&path, &BTreeMap::new()).unwrap();
    assert_eq!(scan.turns, 1);
    fs::write(&path, "{}\n").unwrap();
    scan.update(&path, &BTreeMap::new()).unwrap();
    assert_eq!(scan.turns, 0);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn account_and_scheduler_do_not_reuse_other_accounts_verdicts() {
    let root = temp();
    let rollout = root.join("log.jsonl");
    fs::write(&rollout, "{}\n").unwrap();
    let t = Thread {
        id: "user-thread".into(),
        title: "中文会话".into(),
        rollout_path: rollout,
        source: "cli".into(),
        cwd: "C:/Test中文 Space".into(),
        model: "gpt-6.1-sol".into(),
        effort: "high".into(),
        archived: false,
        updated: 1900.0,
        originator: "Codex Desktop".into(),
    };
    let cfg = Settings {
        automatic: true,
        interval_minutes: 30,
        ..Default::default()
    };
    let mut seen = BTreeMap::from([(t.id.clone(), 0.0)]);
    let mut attempts = BTreeMap::new();
    assert!(
        due_thread(
            std::slice::from_ref(&t),
            &[],
            &mut seen,
            &attempts,
            &cfg,
            "account",
            1900.0
        )
        .is_some()
    );
    attempts.insert(t.id.clone(), 1900.0);
    assert!(
        due_thread(
            std::slice::from_ref(&t),
            &[],
            &mut seen,
            &attempts,
            &cfg,
            "account",
            1901.0
        )
        .is_none()
    );
    let mut internal = t.clone();
    internal.model = "codex-auto-review".into();
    assert!(!internal.eligible());
    internal.model = t.model.clone();
    internal.source = "{\"subagent\": {}}".into();
    assert!(!internal.eligible());
    let store = Store {
        codex: root.join("codex"),
        home: root.join("ledger"),
    };
    fs::create_dir_all(&store.home).unwrap();
    let lock = store.lock("test.lock").unwrap();
    assert!(store.lock("test.lock").is_err());
    drop(lock);
    assert!(store.lock("test.lock").is_ok());
    fs::remove_dir_all(root).unwrap();
}
