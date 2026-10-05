//! Offline protocol fixture. Never accesses a real account or starts inference.
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
fn send(value: Value) {
    println!("{value}");
    io::stdout().flush().unwrap();
}
fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if args.iter().any(|a| a == "--version") {
        println!("codex-cli 0.160.0");
        return;
    }
    if args.iter().any(|a| a == "--help") {
        println!("--no-daemon");
        return;
    }
    let scenario = args
        .iter()
        .find_map(|s| s.strip_prefix("--scenario="))
        .unwrap_or("match");
    let log = args.iter().find_map(|s| s.strip_prefix("--log="));
    if let Some(log) = log {
        use std::fs::OpenOptions;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .unwrap();
        writeln!(f, "{}", json!({"startup_args":args})).unwrap();
    }
    let rows: Vec<Value> = include_str!("../fixtures/reference_subset.jsonl")
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let texts: Vec<_> = rows
        .iter()
        .filter(|r| r["model_id"] == "gpt-6-astra")
        .collect();
    let mut count = 0;
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let message: Value = serde_json::from_str(&line).unwrap();
        if let Some(log) = log {
            use std::fs::OpenOptions;
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(log)
                .unwrap();
            writeln!(f, "{line}").unwrap();
        }
        let id = &message["id"];
        let p = &message["params"];
        match message["method"].as_str().unwrap_or("") {
            "hooks/list" => send(json!({"id":id,"result":{"data":[{"hooks":[
                {"pluginId":"is-gpt-nerfed@custom-market","eventName":"Stop","enabled":true,"trustStatus":"trusted","command":"sh -c 'python3 nerfed hook'"},
                {"pluginId":"other-plugin@market","eventName":"Stop","enabled":true,"trustStatus":"trusted","command":"other"}
            ],"warnings":["is-gpt-nerfed legacy entry"]}]}})),
            "initialize" => send(json!({"id":id,"result":{"userAgent":"fake-rust"}})),
            "initialized" => {}
            "thread/read" => {
                let mut t = json!({"id":p["threadId"],"path":"C:/Test 中文 Space/rollout.jsonl","model":if scenario=="unlisted"{"gpt-6.1-sol"}else{"gpt-6-astra"},"modelProvider":"openai","reasoningEffort":"high","cwd":"C:/Test 中文 Space","ephemeral":false});
                if scenario == "hinted" {
                    t.as_object_mut().unwrap().remove("model");
                    t.as_object_mut().unwrap().remove("modelProvider");
                }
                send(json!({"id":id,"result":{"thread":t}}));
            }
            "thread/turns/list" => send(
                json!({"id":id,"result":{"data":if scenario=="fallback"{json!([{"id":"live","status":"completed"},{"id":"prev","status":"completed"}])}else{json!([{"id":"done","status":"completed"}])}}}),
            ),
            "thread/fork" => {
                if scenario == "fallback" && p["lastTurnId"] == "live" {
                    send(
                        json!({"id":id,"error":{"code":-32000,"message":"identifies an in-progress turn"}}),
                    );
                    continue;
                }
                count += 1;
                let fid = format!("fork-{count}");
                send(
                    json!({"id":id,"result":{"thread":{"id":fid,"ephemeral":scenario!="persistent","forkedFromId":p["threadId"]},"model":p["model"],"modelProvider":p["modelProvider"],"reasoningEffort":p["config"]["model_reasoning_effort"]}}),
                );
            }
            "thread/start" => {
                count += 1;
                send(
                    json!({"id":id,"result":{"thread":{"id":format!("fresh-{count}"),"ephemeral":true},"model":p["model"],"reasoningEffort":p["config"]["model_reasoning_effort"]}}),
                );
            }
            "turn/start" => {
                if p["approvalPolicy"] != "untrusted" || p["sandboxPolicy"]["type"] != "readOnly" {
                    send(
                        json!({"id":id,"error":{"code":-32602,"message":"invalid probe approval or sandbox policy"}}),
                    );
                    continue;
                }
                let tid = &p["threadId"];
                let turn = format!("turn-{tid}");
                send(json!({"id":id,"result":{"turn":{"id":turn}}}));
                if scenario == "hang" {
                    continue;
                }
                if scenario == "approval" {
                    send(
                        json!({"id":1000,"method":"item/commandExecution/requestApproval","params":{"threadId":tid,"turnId":turn,"command":"do not execute"}}),
                    );
                }
                if scenario == "tool" {
                    send(
                        json!({"method":"item/started","params":{"threadId":tid,"item":{"type":"commandExecution"}}}),
                    );
                }
                send(
                    json!({"method":"item/completed","params":{"threadId":tid,"item":{"type":"agentMessage","phase":"final_answer","text":texts[(count-1)%texts.len()]["text"]}}}),
                );
                send(
                    json!({"method":"turn/completed","params":{"threadId":tid,"turn":{"id":turn,"status":if scenario=="failed"{"failed"}else{"completed"}}}}),
                );
            }
            "turn/interrupt" => send(json!({"id":id,"result":{}})),
            _ => {}
        }
    }
}
