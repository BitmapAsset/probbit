//! The browser module's persona ops (`persona_init` / `persona_turn`, C ABI ops 4 / 5) give the CLI's documents without threads:
//! the example personas' goldens (examples/persona/golden/, the `probbit persona` documents) on every workday turn, and the same
//! bytes through `probbit_call`. Documents are canonical JSON, so equal values are equal bytes.
#[allow(dead_code)]
#[path = "../../probbit-cli/src/json.rs"]
mod json;
use json::Json;

fn read(p: &str) -> String { std::fs::read_to_string(format!("{}/../examples/persona/{p}", env!("CARGO_MANIFEST_DIR"))).unwrap().replace("\r\n", "\n") }
fn parse(s: &str) -> Json { json::parse(s).unwrap_or_else(|e| panic!("not JSON ({}): {s:.200}", e.msg)) }

#[test]
fn persona_ops_give_the_cli_documents() {
    let turns = parse(&read("workday.json")).get("turns").unwrap().as_arr().unwrap().to_vec();
    for n in ["ops-engineer", "tutor", "trader-assistant"] {
        let doc = read(&format!("{n}.json")); let g = |f: &str| read(&format!("golden/{n}/{f}"));
        let (st, c) = probbit_wasm::persona_init(&format!(r#"{{"persona": {doc}}}"#)); assert_eq!(c, 0, "{st}");
        assert_eq!(format!("{st}\n"), g("state0.json"), "{n} state0");
        let gold = g("workday.jsonl"); let mut state = st;
        for (i, (t, line)) in turns.iter().zip(gold.lines()).enumerate() {
            let (out, c) = probbit_wasm::persona_turn(&format!(r#"{{"persona": {doc}, "state": {state}, "inputs": {}}}"#, json::write(t, false))); assert_eq!(c, 0, "{out}");
            let o = parse(&out); assert_eq!(o.get("stance"), Some(&parse(line)), "{n} turn {i}");
            state = json::write(o.get("state").unwrap(), false); }
        assert_eq!(parse(&state), parse(&g("final-state.json")), "{n} final state");
    }
    // the C ABI: op 4 / op 5 are the same functions; a bad input is one persona error object, exit 2
    let call = |op: u32, s: &str| -> (String, i32) { let p = probbit_wasm::probbit_alloc(s.len()); unsafe { std::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
        let n = probbit_wasm::probbit_call(op, p, s.len()); (String::from_utf8(std::slice::from_raw_parts(probbit_wasm::probbit_out_ptr(), n).to_vec()).unwrap(), probbit_wasm::probbit_out_code()) } };
    let doc = read("tutor.json"); let arg = format!(r#"{{"persona": {doc}, "seed": 3}}"#);
    let (a, c) = call(4, &arg); assert_eq!((a.clone(), c), probbit_wasm::persona_init(&arg));
    let t = format!(r#"{{"persona": {doc}, "state": {a}, "inputs": {{"loss": true}}, "flags": {{"no_inertia": true}}}}"#);
    assert_eq!(call(5, &t), probbit_wasm::persona_turn(&t));
    let (e, c) = call(5, &format!(r#"{{"persona": {doc}, "state": {a}, "inputs": {{"stakes": 9}}}}"#)); assert_eq!(c, 2);
    assert_eq!(parse(&e).get("error").and_then(|x| x.get("path")).and_then(Json::as_str), Some("inputs.stakes"));
    let (e, c) = call(4, r#"{"persona_path": "tutor.yaml"}"#); assert_eq!(c, 2); assert!(e.contains("no files here"), "{e}");
}

/// drives (§2.9): goal signals pass through op 5 as one more input; the adversary fixture's turns equal its golden (the CLI's
/// replay, which is the Python reference's trace)
#[test]
fn persona_ops_pass_goal_signals() {
    let fx = |p: &str| std::fs::read_to_string(format!("{}/../probbit-cli/tests/fixtures/persona/{p}", env!("CARGO_MANIFEST_DIR"))).unwrap().replace("\r\n", "\n");
    let doc = fx("drives-adversary.json"); let turns = parse(&fx("drives-adversary-script.json")).get("turns").unwrap().as_arr().unwrap().to_vec();
    let (mut state, c) = probbit_wasm::persona_init(&format!(r#"{{"persona": {doc}, "seed": 4}}"#)); assert_eq!(c, 0, "{state}");
    let gold = fx("drives-adversary-replay.jsonl"); assert_eq!(gold.lines().count(), turns.len());
    for (i, (t, line)) in turns.iter().zip(gold.lines()).enumerate() {
        let (out, c) = probbit_wasm::persona_turn(&format!(r#"{{"persona": {doc}, "state": {state}, "inputs": {}}}"#, json::write(t, false))); assert_eq!(c, 0, "{out}");
        let o = parse(&out); assert_eq!(o.get("stance"), Some(&parse(line)), "turn {i}"); state = json::write(o.get("state").unwrap(), false); }
}
