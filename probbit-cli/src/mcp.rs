//! `probbit mcp`: a Model Context Protocol server on stdio, hand-written JSON-RPC 2.0 on the crate's own JSON reader (no crates).
//! One message per line in (stdin) and out (stdout); stdout carries nothing but JSON-RPC messages, the log goes to stderr, and
//! the server exits 0 when stdin closes.
//!
//! Checked against the MCP specification revision 2026-07-28 (the current revision on 2026-10-01) and served "dual-era":
//! a request whose `params._meta` carries `io.modelcontextprotocol/protocolVersion` (and `clientCapabilities`) is answered
//! statelessly by that revision (`server/discover`, `tools/list`, `tools/call`, `ping`; results carry `resultType` and the server
//! identity); a client that opens with `initialize` (the handshake revisions 2025-11-25, 2025-06-18, 2025-03-26, 2024-11-05) is
//! served by the revision negotiated there. An unknown revision gets `UnsupportedProtocolVersion` (-32022) with the list.
//!
//! Tools: `probbit_decide`, `probbit_run`, `probbit_stats`, `probbit_demo`, `probbit_evaluate`. Each runs this binary's own command in a child process (the
//! document on its stdin, the `flags` object as command-line flags) and returns the command's stdout JSON unchanged, as text and
//! as `structuredContent`: a tool answer is the CLI's answer byte for byte. Exit 2 (bad input or flag) and error objects come back
//! as tool errors (`isError`); `infeasible` (exit 1), `refused` / `declined` (exit 3) are answers.
//!
//! `probbit_persona_init` and `probbit_persona_turn` (docs/persona.md) run in this process and are stateless: the persona (an inline
//! document or a path) and the state go in, the state comes back. Their documents are the CLI's (`probbit persona init` / `turn`);
//! a refusal or a fallback stance is an answer, a bad persona, state or input a tool error with the `persona` error object.
//! `probbit_persona_fuzz` (docs/persona.md §5.6) runs in this process too: `probbit persona fuzz --json`'s document; a counterexample
//! is an answer, a bad persona or rule a tool error.
//! `probbit_live_event` and `probbit_live_verify` (docs/persona.md §5.7) run in this process, stateless as well: one event of a
//! resident individual (the host's clock gives `elapsed_hours`) and, with `strand_path`, its line appended to a strand file on the
//! server's disk; verify replays a strand. A divergence is an answer, a bad persona, state, event or strand a tool error.
use crate::json::{self, num, obj, str as jstr, Json};
use std::io::{BufRead, Write};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const MODERN: &str = "2026-07-28";
const LEGACY: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
/// Flags that take on / off (booleans map to them); every other boolean is a switch (true = present, false = absent).
const ON_OFF: [&str; 3] = ["collective", "cluster", "cycles"];

fn log(s: &str) { let _ = writeln!(std::io::stderr(), "probbit mcp: {s}"); }
fn send(msg: &Json) {
    let mut o = std::io::stdout().lock();
    if o.write_all(json::write(msg, false).as_bytes()).and_then(|_| o.write_all(b"\n")).and_then(|_| o.flush()).is_err() { std::process::exit(0); } // the client went away
}
fn error(id: Json, code: i64, msg: &str, data: Option<Json>) -> Json {
    let mut e = vec![("code", num(code as f64)), ("message", jstr(msg))]; if let Some(d) = data { e.push(("data", d)); }
    obj(vec![("jsonrpc", jstr("2.0")), ("id", id), ("error", obj(e))])
}
/// How the client speaks: a modern request (per-request `_meta`) or a session opened by `initialize` at a legacy revision
#[derive(Clone, PartialEq)]
enum Era { Modern, Legacy(String) }
fn result(id: Json, era: &Era, mut r: Vec<(&str, Json)>) -> Json {
    if *era == Era::Modern { r.insert(0, ("resultType", jstr("complete"))); r.push(("_meta", obj(vec![("io.modelcontextprotocol/serverInfo", server_info())]))); }
    obj(vec![("jsonrpc", jstr("2.0")), ("id", id), ("result", obj(r))])
}
fn server_info() -> Json { obj(vec![("name", jstr("probbit")), ("title", jstr("probbit virtual p-bit processor")), ("version", jstr(VERSION))]) }
const INSTRUCTIONS: &str = "probbit is a virtual p-bit processor for joint decisions under hard rules. probbit_decide routes tasks to workers (allowed sets, quotas, clamps, affinity) and returns a plan that obeys every rule, odds per task and a verdict: exact, diagnostics_passed, partial (act on `released`, escalate `escalated`), refused or infeasible. probbit_run does the same for a general probbit-ir program. probbit_evaluate takes a decision model's System One request and the judge's per-question probabilities plus rules over question ids, and returns the most likely answer set that obeys every rule in the judge's response shape, with odds and the gate's verdict. Pass \"flags\": {\"summary\": true} for a compact answer. probbit_demo makes a sample routing document; probbit_stats measures the machine. probbit_persona_init and probbit_persona_turn give an agent an individual temperament that lives outside the model: start an individual with probbit_persona_init, then each turn pass the persona, the returned state and the turn's inputs to probbit_persona_turn and put the returned stance `line` into the model's prompt (after any cached prefix). A persona with a drives block also takes `goals` in the inputs (signals per goal id: cue, progress, novelty, setback, win, deadline_hours), and its stance names the goal that gets the next unit of effort (`pursue`, with odds) and the drive values (`drives`). probbit_persona_fuzz tests a persona's character: a rule in habit syntax and a population of individuals -> the shortest event script whose stance breaks it, per individual. probbit_live_event keeps a resident individual: feed it each event with the hours since the previous one (moods decay, feedback nudges learned weights within caps; habits never move) and optionally log it to a strand that probbit_live_verify replays byte for byte.";

/// Serve until stdin closes.
pub fn serve() {
    log(&format!("{VERSION} ready on stdio (MCP {MODERN}; initialize for {})", LEGACY.join(", ")));
    let mut session: Option<String> = None; let mut buf = Vec::new(); let mut first = true; let stdin = std::io::stdin();
    loop {
        buf.clear();
        match stdin.lock().read_until(b'\n', &mut buf) { Ok(0) => break, Ok(_) => {}, Err(e) => { log(&format!("stdin: {e}")); break } }
        while buf.last().is_some_and(|&b| b == b'\n' || b == b'\r') { buf.pop(); }
        if first && buf.starts_with(&[0xEF, 0xBB, 0xBF]) { buf.drain(..3); } first = false; // a byte-order mark (Windows PowerShell 5.1 pipes)
        if buf.iter().all(|b| b.is_ascii_whitespace()) { continue; }
        let msg = match std::str::from_utf8(&buf).map_err(|e| e.to_string()).and_then(|s| json::parse(s).map_err(|e| e.msg)) {
            Ok(m) => m, Err(e) => { send(&error(Json::Null, -32700, &format!("parse error: {e}"), None)); continue } };
        match &msg {
            Json::Arr(v) if v.is_empty() => send(&error(Json::Null, -32600, "invalid request: empty batch", None)),
            Json::Arr(v) => { let out: Vec<Json> = v.iter().filter_map(|m| handle(m, &mut session)).collect(); if !out.is_empty() { send(&Json::Arr(out)); } }
            m => if let Some(r) = handle(m, &mut session) { send(&r); } }
    }
    log("stdin closed; bye");
}

/// One message -> its response (None for a notification).
fn handle(msg: &Json, session: &mut Option<String>) -> Option<Json> {
    let id = msg.get("id").cloned();
    let (Some(method), Some("2.0")) = (msg.get("method").and_then(Json::as_str), msg.get("jsonrpc").and_then(Json::as_str)) else {
        return Some(error(id.unwrap_or(Json::Null), -32600, "invalid request: a JSON-RPC 2.0 object with \"jsonrpc\": \"2.0\" and a \"method\"", None)) };
    let id = id?; // notifications (initialized, cancelled, ...): nothing to answer; calls run one at a time
    if !matches!(id, Json::Str(_) | Json::Num(_)) { return Some(error(Json::Null, -32600, "invalid request: the id must be a string or an integer", None)); }
    let params = msg.get("params"); let meta = params.and_then(|p| p.get("_meta"));
    if method == "initialize" {
        let want = params.and_then(|p| p.get("protocolVersion")).and_then(Json::as_str).unwrap_or("");
        let v = if LEGACY.contains(&want) { want } else { LEGACY[0] }.to_string();
        log(&format!("initialize: client asked for {want:?}, serving {v} (legacy handshake)")); *session = Some(v.clone());
        return Some(result(id, &Era::Legacy(v.clone()), vec![("protocolVersion", jstr(&v)), ("capabilities", obj(vec![("tools", obj(vec![("listChanged", Json::Bool(false))]))])),
            ("serverInfo", server_info()), ("instructions", jstr(INSTRUCTIONS))]));
    }
    let era = match meta.and_then(|m| m.get("io.modelcontextprotocol/protocolVersion")).and_then(Json::as_str) {
        Some(v) if v == MODERN || LEGACY.contains(&v) => {
            if meta.and_then(|m| m.get("io.modelcontextprotocol/clientCapabilities")).is_none() { return Some(error(id, -32602, "invalid params: _meta lacks io.modelcontextprotocol/clientCapabilities", None)); }
            Era::Modern }
        Some(v) => return Some(error(id, -32022, "Unsupported protocol version", Some(obj(vec![("supported", Json::Arr([MODERN].iter().chain(LEGACY.iter()).map(|s| jstr(s)).collect())), ("requested", jstr(v))])))),
        None => match (session.clone(), method) { (Some(v), _) => Era::Legacy(v), (None, "ping") => Era::Legacy(LEGACY[0].into()),
            (None, _) => return Some(error(id, -32602, "invalid params: no _meta io.modelcontextprotocol/protocolVersion (MCP 2026-07-28), and no initialize before (legacy revisions)", None)) } };
    match method {
        "ping" => Some(result(id, &era, vec![])),
        "server/discover" => Some(result(id, &era, vec![("supportedVersions", Json::Arr([MODERN].iter().chain(LEGACY.iter()).map(|s| jstr(s)).collect())),
            ("capabilities", obj(vec![("tools", obj(vec![("listChanged", Json::Bool(false))]))])), ("instructions", jstr(INSTRUCTIONS))])),
        "tools/list" => Some(result(id, &era, vec![("tools", tools())])),
        "tools/call" => {
            let Some(name) = params.and_then(|p| p.get("name")).and_then(Json::as_str) else { return Some(error(id, -32602, "invalid params: tools/call needs a tool \"name\"", None)) };
            match call(name, params.and_then(|p| p.get("arguments"))) {
                Err(m) => Some(error(id, -32602, &m, None)),
                Ok((text, structured, is_error)) => {
                    let mut r = vec![("content", Json::Arr(vec![obj(vec![("type", jstr("text")), ("text", jstr(&text))])]))];
                    let structured_ok = match &era { Era::Modern => true, Era::Legacy(v) => v.as_str() >= "2025-06-18" };
                    if let (Some(s), true) = (structured, structured_ok) { r.push(("structuredContent", s)); }
                    r.push(("isError", Json::Bool(is_error))); Some(result(id, &era, r)) } } }
        _ => Some(error(id, -32601, &format!("method not found: {method}"), None)),
    }
}

/// Run one tool: `Err` = a protocol error (unknown tool, arguments not an object); `Ok((text, structured JSON, isError))`.
fn call(name: &str, args: Option<&Json>) -> Result<(String, Option<Json>, bool), String> {
    let args: Vec<(String, Json)> = match args { None | Some(Json::Null) => vec![], Some(Json::Obj(v)) => v.clone(), Some(_) => return Err("invalid params: \"arguments\" must be an object".into()) };
    if name == "probbit_persona_init" || name == "probbit_persona_turn" { return Ok(persona_tool(name, &args)); }
    if name == "probbit_persona_fuzz" { return Ok(fuzz_tool(&args)); }
    if name == "probbit_live_event" || name == "probbit_live_verify" { return Ok(live_tool(name, &args)); }
    let (cmd, doc, flags) = match name {
        "probbit_decide" | "probbit_run" | "probbit_evaluate" => { let (f, d): (Vec<_>, Vec<_>) = args.into_iter().partition(|(k, _)| k == "flags");
            let flags = match f.into_iter().next().map(|(_, x)| x) { None | Some(Json::Null) => vec![], Some(Json::Obj(v)) => v, Some(_) => return Ok(("\"flags\" must be an object".into(), None, true)) };
            (match name { "probbit_decide" => "decide", "probbit_run" => "run", _ => "evaluate" }, Some(Json::Obj(d)), flags) }
        "probbit_stats" => ("stats", None, args), "probbit_demo" => ("demo", None, args),
        _ => return Err(format!("Unknown tool: {name}")) };
    let mut argv = vec![cmd.to_string()];
    for (k, x) in &flags { let f = format!("--{}", k.replace('_', "-"));
        match x { Json::Bool(b) if ON_OFF.contains(&k.as_str()) => { argv.push(f); argv.push(if *b { "on" } else { "off" }.into()); }
            Json::Bool(true) => argv.push(f), Json::Bool(false) | Json::Null => {},
            Json::Num(v) => { argv.push(f); argv.push(if v.fract() == 0.0 && v.abs() < 9e15 { format!("{}", *v as i64) } else { format!("{v}") }); }
            Json::Str(s) => { argv.push(f); argv.push(s.clone()); }
            _ => return Ok((format!("flag {k}: a number, string or boolean"), None, true)) } }
    let t0 = std::time::Instant::now();
    let input = doc.map(|d| json::write(&d, false)).unwrap_or_default();
    let exe = std::env::current_exe().map_err(|e| format!("internal: cannot locate the probbit binary: {e}"))?;
    let child = std::process::Command::new(exe).args(&argv).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn();
    let mut child = match child { Ok(c) => c, Err(e) => return Ok((format!("cannot start probbit {cmd}: {e}"), None, true)) };
    let mut stdin = child.stdin.take(); let w = std::thread::spawn(move || { if let Some(s) = stdin.as_mut() { let _ = s.write_all(input.as_bytes()); } drop(stdin); });
    let out = match child.wait_with_output() { Ok(o) => o, Err(e) => return Ok((format!("probbit {cmd}: {e}"), None, true)) }; let _ = w.join();
    let code = out.status.code().unwrap_or(-1); let text = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
    let parsed = json::parse(&text).ok().filter(|j| matches!(j, Json::Obj(_)));
    let is_error = !matches!(code, 0 | 1 | 3) || parsed.as_ref().map_or(true, |j| j.get("error").is_some());
    log(&format!("tools/call {name} ({}) -> exit {code}, {} bytes, {:.0} ms", argv[1..].join(" "), text.len(), t0.elapsed().as_secs_f64() * 1e3));
    if parsed.is_none() { let e = String::from_utf8_lossy(&out.stderr).trim().to_string(); return Ok((if e.is_empty() { format!("probbit {cmd} exited {code}") } else { e }, None, true)); }
    Ok((text, parsed, is_error))
}

/// `probbit_persona_init` / `probbit_persona_turn`, in process -> (text, structured JSON, isError)
fn persona_tool(name: &str, args: &[(String, Json)]) -> (String, Option<Json>, bool) {
    let t0 = std::time::Instant::now();
    let eng = crate::persona_engine(); let r = crate::persona::tool(name, args, &eng, true);
    let (doc, is_error) = match r { Ok(d) => (d, false), Err(e) => (e.to_json(), true) };
    log(&format!("tools/call {name} -> {}, {:.1} ms", if is_error { "error" } else { "ok" }, t0.elapsed().as_secs_f64() * 1e3));
    (crate::persona::canon(&doc), Some(doc), is_error)
}

/// `probbit_persona_fuzz`, in process -> (text, structured JSON, isError); a counterexample is an answer
fn fuzz_tool(args: &[(String, Json)]) -> (String, Option<Json>, bool) {
    let t0 = std::time::Instant::now();
    let eng = crate::persona_engine(); let r = crate::fuzz::tool(args, &eng);
    let (doc, is_error) = match r { Ok(d) => (d, false), Err(e) => (e.to_json(), true) };
    log(&format!("tools/call probbit_persona_fuzz -> {}, {:.1} ms", if is_error { "error" } else { "ok" }, t0.elapsed().as_secs_f64() * 1e3));
    (crate::persona::canon(&doc), Some(doc), is_error)
}

/// `probbit_live_event` / `probbit_live_verify`, in process -> (text, structured JSON, isError); a divergence is an answer
fn live_tool(name: &str, args: &[(String, Json)]) -> (String, Option<Json>, bool) {
    let t0 = std::time::Instant::now();
    let eng = crate::persona_engine(); let r = crate::live::tool(name, args, &eng);
    let (doc, is_error) = match r { Ok(d) => (d, false), Err(e) => (e.to_json(), true) };
    log(&format!("tools/call {name} -> {}, {:.1} ms", if is_error { "error" } else { "ok" }, t0.elapsed().as_secs_f64() * 1e3));
    (crate::persona::canon(&doc), Some(doc), is_error)
}

/// The tool list: input schemas are the commands' own contracts (the router document, the probbit-ir v1 schema) plus `flags`.
fn tools() -> Json {
    let p = |s: &str| json::parse(s).expect("tool schema");
    let flags_common = r#""budget_ms": {"type": "number", "minimum": 0, "description": "sampler wall-clock budget, ms (default 200)"},
        "seed": {"type": "integer", "minimum": 0, "description": "random seed (default 7)"},
        "sweeps": {"type": "integer", "minimum": 1, "description": "fixed work per chain instead of the budget (with polish_ms 0: a pure function of input + seed)"},
        "exact_limit": {"type": "integer", "minimum": 0}, "exact_ms": {"type": "number", "minimum": 0}, "frontier_states": {"type": "integer", "minimum": 0},
        "polish_ms": {"type": "number", "minimum": 0}, "polish_sweeps": {"type": "integer", "minimum": 0},
        "collective": {"type": "boolean"}, "cluster": {"type": "boolean"}, "cycles": {"type": "boolean"},
        "chains": {"type": "integer", "minimum": 1, "maximum": 100000}, "threads": {"type": "integer", "minimum": 1, "maximum": 1024},
        "cpu_limit": {"type": "integer", "minimum": 1, "maximum": 100}, "mem_limit_mb": {"type": "integer", "minimum": 0}, "priority": {"enum": ["low", "normal"]},
        "max_input_mb": {"type": "integer", "minimum": 0},
        "summary": {"type": "boolean", "description": "compact answer: verdict, counts, gate, the 5 worst released and escalated items, telemetry"}"#;
    let decide_flags = format!(r#"{{"type": "object", "additionalProperties": false, "description": "probbit decide flags without the dashes (probbit decide --help)",
        "properties": {{{flags_common}, "mode": {{"enum": ["auto", "exact", "sample"]}}}}}}"#);
    let run_flags = format!(r#"{{"type": "object", "additionalProperties": false, "description": "probbit run flags without the dashes (probbit run --help)",
        "properties": {{{flags_common}, "op": {{"enum": ["decide", "exact", "sample"]}}, "deadline_ms": {{"type": "number", "exclusiveMinimum": 0}}}}}}"#);
    let router = format!(r#"{{"type": "object", "additionalProperties": false, "required": ["workers", "tasks"],
        "description": "A router document (README, Problem format): workers with quotas, tasks with scores (natural-log odds per worker), optional allowed workers, workflow group, clamp; affinity = bonus when tasks of one group share a worker.",
        "properties": {{
          "workers": {{"type": "array", "minItems": 1, "items": {{"type": "object", "additionalProperties": false, "required": ["id"],
            "properties": {{"id": {{"type": "string"}}, "cap": {{"type": "integer", "minimum": 0}}, "capacity": {{"type": "integer", "minimum": 0}}}}}}}},
          "tasks": {{"type": "array", "minItems": 1, "items": {{"type": "object", "additionalProperties": false, "required": ["scores"],
            "properties": {{"id": {{"type": "string"}}, "scores": {{"type": "object", "additionalProperties": {{"type": "number"}}}}, "allowed": {{"type": "array", "items": {{"type": "string"}}}},
              "group": {{"type": ["string", "number"]}}, "clamp": {{"type": "string"}}, "text": {{"type": "string"}}}}}}}},
          "affinity": {{"type": "number", "minimum": 0}}, "comment": {{"type": "string"}}, "flags": {decide_flags}}}}}"#);
    let mut ir = p(include_str!("probbit-ir.schema.json"));
    let evaluate = evaluate_schema(&ir, &format!(r#"{{"type": "object", "additionalProperties": false, "description": "probbit evaluate flags without the dashes (probbit evaluate --help): the probbit run flags, plus program",
        "properties": {{{flags_common}, "op": {{"enum": ["decide", "exact", "sample"]}}, "deadline_ms": {{"type": "number", "exclusiveMinimum": 0}},
          "program": {{"type": "boolean", "description": "return the compiled probbit-ir program instead of running it"}}}}}}"#));
    if let Json::Obj(v) = &mut ir { v.retain(|(k, _)| k != "$id" && k != "examples");
        for (k, x) in v.iter_mut() { if k == "description" { *x = jstr("A probbit-ir v1 program (docs/probbit-ir-json.md): variables over named values, scores, pairs, caps and the v2 constructs, plus optional flags."); }
            if k == "properties" { if let Json::Obj(pr) = x { pr.push(("flags".into(), p(&run_flags))); } } } }
    let tool = |name: &str, title: &str, desc: &str, schema: Json| obj(vec![("name", jstr(name)), ("title", jstr(title)), ("description", jstr(desc)), ("inputSchema", schema),
        ("annotations", obj(vec![("readOnlyHint", Json::Bool(true)), ("openWorldHint", Json::Bool(false))]))]);
    Json::Arr(vec![
        tool("probbit_decide", "Route tasks to workers", "probbit decide: assign every task to one worker under hard rules (allowed sets, quotas, clamps) with workflow affinity. Returns the same JSON as the CLI: a plan that obeys every rule, odds per task, verdict exact | diagnostics_passed | partial (act on `released`, escalate `escalated`) | refused | infeasible, gate diagnostics and telemetry.", p(&router)),
        tool("probbit_run", "Run a probbit-ir program", "probbit run: a general constrained categorical program (variables, values, scores, pairs, caps, all_different, implies, tables, precedes, linear). Same answer shape as probbit_decide with `marginals` instead of `odds`.", ir),
        tool("probbit_stats", "Processor spec sheet", "probbit stats: machine, build features, effective controls and a measured self-test (site updates per second).",
            p(r#"{"type": "object", "additionalProperties": false, "properties": {"sweeps": {"type": "integer", "minimum": 1}, "chains": {"type": "integer", "minimum": 1}, "threads": {"type": "integer", "minimum": 1}}}"#)),
        tool("probbit_demo", "Sample routing document", "probbit demo: a seeded synthetic agent-routing document (PII and prod-DB rules, quotas, workflow affinity) to pass to probbit_decide.",
            p(r#"{"type": "object", "additionalProperties": false, "properties": {"tasks": {"type": "integer", "minimum": 1, "description": "tasks to generate (default 12)"}, "seed": {"type": "integer", "minimum": 0}, "hard": {"type": "boolean", "description": "tight quotas + strong affinity"}}}"#)),
        tool("probbit_evaluate", "Decide after a judge", "probbit evaluate: a decision model's System One request (questions noul | choice | score keyed by id) with the judge's answers or per-question weights and rules over question ids (caps, implies, tables, precedes, linear, all_different, pairs). Returns the judge's response shape (model, answers keyed by question id, usage) filled with the most likely answer set that obeys every rule, odds per option under the rules and a probbit object per answer (value, p, judge, changed, released), plus the probbit run document (verdict, gate, violations, plan_logw, released / escalated, telemetry). Without rules the answers are the judge's own.", evaluate),
        tool("probbit_persona_init", "Start a persona individual", "probbit persona init: a persona (inline document or a file path) and a seed -> the individual's initial state (genes from the seed, resting stance, empty mood and history). Pass the state to probbit_persona_turn. Stateless: the server keeps nothing.",
            p(&format!(r#"{{"type": "object", "additionalProperties": false, "properties": {{{PERSONA_ARG}, "seed": {{"type": "integer", "minimum": 0, "description": "the individual (default: the persona's identity.seed)"}}}}}}"#))),
        tool("probbit_persona_turn", "This turn's stance from a persona", "probbit persona turn: a persona (inline document or a file path), the individual's state and this turn's inputs (the persona's evidence: sentiment, error, loss, praise, criticism, stakes, time_pressure, claim_done, elapsed_hours, and the persona's own; `goals` for a persona with a drives block) -> {stance, state}. The stance has a level for every trait with exact odds, the mood, the habits in force and the ones that changed the stance (habits are hard rules: 0 violations by construction), status ok | partial | refused | fallback with an escalate reason, a one-line why, and `line`: a short stance line to put into the model's prompt (after any cached prefix). Pass the returned state back on the next turn. The persona never writes the reply; it decides how the reply is written.",
            p(&format!(r#"{{"type": "object", "additionalProperties": false, "required": ["state"], "properties": {{{PERSONA_ARG},
              "state": {{"type": "object", "description": "the state returned by probbit_persona_init or by the previous probbit_persona_turn"}},
              "inputs": {{"type": "object", "description": "this turn's evidence, keyed by input id; flags true/false, level inputs a level name, numbers 0..max; inputs the persona does not declare are listed in stance.ignored", "properties": {{{GOALS_ARG}}}, "additionalProperties": {{"type": ["boolean", "number", "string"]}}}},
              "flags": {{"type": "object", "additionalProperties": false, "properties": {{"timing": {{"type": "boolean", "description": "add a non-canonical timing object to the stance"}}, "no_inertia": {{"type": "boolean", "description": "ignore mood inertia for this turn (diagnostics)"}}}}}}}}}}"#))),
        tool("probbit_persona_fuzz", "Test a persona's character", "probbit persona fuzz: a persona (inline document or a file path) and a character property in habit syntax (never: {when, then}; or props: a list of them) -> for each individual (seeds, default 0-99) the shortest event script whose stance breaks it, shrunk, with the replay command; `found` says whether any did. It tests the stance a host gets, not the words a model writes; none found is evidence, not a proof. The document of probbit persona fuzz --json.",
            p(&format!(r#"{{"type": "object", "additionalProperties": false, "properties": {{{PERSONA_ARG},
              "never": {{"type": ["object", "string"], "description": "one rule in habit syntax, e.g. {{\"when\": {{\"sentiment\": \"negative\"}}, \"then\": {{\"humour\": {{\"at_most\": \"light\"}}}}}} (or one line of YAML)"}},
              "props": {{"type": ["array", "object"], "description": "several rules, each with an optional id (or {{\"props\": [...]}})"}},
              "seeds": {{"type": ["string", "array"], "description": "the individuals: \"0-99\" (default), \"1,4,9\" or a list of integers"}},
              "fuzz_seed": {{"type": "integer", "minimum": 0}}, "scripts": {{"type": "integer", "minimum": 0, "maximum": 1000000}}, "depth": {{"type": "integer", "minimum": 1, "maximum": 64}},
              "beam": {{"type": "integer", "minimum": 0, "maximum": 64}}, "grid": {{"type": "array", "items": {{"type": "number", "minimum": 0, "maximum": 1}}}},
              "hours": {{"type": "array", "items": {{"type": "number", "minimum": 0}}}}, "threads": {{"type": "integer", "minimum": 1, "maximum": 1024}}}}}}"#))),
        tool("probbit_live_event", "Feed one event to a resident individual", "probbit live, one event: a persona (inline document or a file path), the individual's state (or a seed for a new one) and one event (inputs keyed by input id, with elapsed_hours since the previous event from the host's clock; quantised to 1e-6 h) -> {stance, state}: moods decay by their half-lives over the elapsed hours, and with a learning block the reward / correction flags move the learned deltas within their caps (habits never move). With strand_path, the event's line is appended to that strand file on the server's disk (a new file gets the header; an existing one is continued, and state must be the state after its last line) and the result has strand {path, events, head}. Pass the returned state back with the next event.",
            p(&format!(r#"{{"type": "object", "additionalProperties": false, "properties": {{{PERSONA_ARG},
              "state": {{"type": "object", "description": "the state returned by the previous event (or by probbit_persona_init); give this or seed"}},
              "seed": {{"type": "integer", "minimum": 0, "description": "a new individual (default: the persona's identity.seed)"}},
              "event": {{"type": "object", "description": "the event's inputs keyed by input id, plus elapsed_hours (hours since the previous event, >= 0; default 0)", "properties": {{{GOALS_ARG}}}, "additionalProperties": {{"type": ["boolean", "number", "string"]}}}},
              "strand_path": {{"type": "string", "description": "a strand file on the server's disk to append the event's line to (created with its header if missing)"}}}}}}"#))),
        tool("probbit_live_verify", "Replay a strand", "probbit live verify: a strand (its text, or a file path on the server's disk) -> {ok: true, events, persona, seed, engine, final_state, last_line} when every line replays byte for byte from the header alone, else {ok: false, line, diverges} with the earliest line that differs and what (a changed input, a removed or reordered line, a changed header). A divergence is an answer, not a tool error.",
            p(r#"{"type": "object", "additionalProperties": false, "properties": {"strand": {"type": "string", "description": "the strand's text (header line, then one line per event)"}, "strand_path": {"type": "string", "description": "a strand file on the server's disk"}}}"#)),
    ])
}

/// The persona argument of both persona tools: an inline document or a path on the server's disk
const PERSONA_ARG: &str = r#""persona": {"type": "object", "description": "the persona document (the JSON form of a persona file, docs/persona.md); give this or persona_path"},
              "persona_path": {"type": "string", "description": "a persona file on the server's disk (YAML subset or JSON)"}"#;
/// `goals` in a turn's inputs or a live event: a persona with a drives block reads its goal signals there (docs/persona.md 2.9)
const GOALS_ARG: &str = r#""goals": {"type": "object", "description": "a persona with a drives block: signals per goal id (a persona without one lists goals in stance.ignored)", "additionalProperties": {"type": "object", "additionalProperties": false, "properties": {
                "cue": {"type": "boolean"}, "progress": {"type": "number", "minimum": 0, "maximum": 1}, "novelty": {"type": ["boolean", "number"], "minimum": 0, "maximum": 1},
                "setback": {"type": "number", "minimum": 0, "maximum": 1}, "win": {"type": "number", "minimum": 0, "description": "a win's size, at most the persona's expectation.win_max"},
                "deadline_hours": {"type": ["number", "null"], "minimum": 0, "description": "hours to the goal's deadline; null clears it"}}}}"#;
fn get_mut<'a>(j: &'a mut Json, k: &str) -> Option<&'a mut Json> { match j { Json::Obj(v) => v.iter_mut().find(|(x, _)| x == k).map(|(_, x)| x), _ => None } }
/// `probbit_evaluate`'s input schema: the System One request (TypeSafe OpenAPI 0.2.0 and the Workers AI clef schema: `model`,
/// `state`, `questions`, `images`) plus the `probbit` block, whose `rules` are the probbit-ir schema's own rule definitions.
fn evaluate_schema(ir: &Json, flags: &str) -> Json {
    let p = |s: &str| json::parse(s).expect("tool schema");
    let mut s = p(&format!(r#"{{"type": "object", "additionalProperties": false, "required": ["questions"],
        "description": "A System One request plus an optional probbit block (docs/probbit-ir-json.md, Decision API). state, instructions, criteria texts and images are carried, never read: probbit reads the judge's numbers and the rules.",
        "properties": {{
          "model": {{"type": "string", "description": "the judge's model name (echoed when probbit.judge has none)"}},
          "state": {{"description": "the content the questions are about (string, object or array); never read by probbit"}},
          "images": {{"type": "array", "description": "images for the judge; never read by probbit"}},
          "questions": {{"type": "object", "minProperties": 1, "description": "questions keyed by id; each becomes one probbit variable whose values are its options",
            "additionalProperties": {{"type": "object", "additionalProperties": false, "required": ["type"], "properties": {{
              "type": {{"enum": ["noul", "choice", "score"], "description": "noul: yes / no (values false, true); choice: one of the criteria keys; score: a level 0..L-1 of the criteria array"}},
              "instructions": {{"description": "the question for the judge; never read by probbit"}},
              "criteria": {{"description": "choice: object option -> description; score: array of level descriptions, lowest first; noul: optional object with true / false"}}}}}}}},
          "probbit": {{"type": "object", "additionalProperties": false, "description": "where the weights come from (one source per question; none = uniform) and the rules", "properties": {{
            "judge": {{"type": "object", "description": "the judge's System One response: answers keyed by question id (noul: noul = P(yes); choice and score: probabilities per option / level); model and usage are echoed"}},
            "weights": {{"type": "object", "description": "per question id: noul = P(true); choice / score = probabilities keyed by option / level", "additionalProperties": {{"type": ["number", "object"]}}}},
            "logw": {{"type": "object", "description": "per question id: natural-log weights keyed by option / level", "additionalProperties": {{"type": "object"}}}},
            "floor": {{"type": "number", "exclusiveMinimum": 0, "maximum": 1, "description": "probability floor: p becomes ln max(p, floor) (default 1e-6)"}},
            "rules": {{"type": "object", "additionalProperties": false, "description": "hard rules (and soft pairs) over question ids and option names, as in a probbit-ir program", "properties": {{}}}}}}}},
          "flags": {flags}}}}}"#));
    let rules: Vec<(String, Json)> = crate::evaluate::RULES.iter()
        .filter_map(|k| ir.get("properties").and_then(|x| x.get(k)).map(|x| (k.to_string(), x.clone()))).collect();
    let path = ["properties", "probbit", "properties", "rules", "properties"];
    if let Some(r) = path.iter().try_fold(&mut s, |x, k| get_mut(x, k)) { *r = Json::Obj(rules); }
    if let (Json::Obj(v), Some(d)) = (&mut s, ir.get("$defs")) { v.push(("$defs".into(), d.clone())); }
    s
}
