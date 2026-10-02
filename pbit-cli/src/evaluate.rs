//! `pbit evaluate`: the decision-API adapter. A judge (a decision model behind a System One style API) answers each question on
//! its own; `pbit evaluate` takes the same request plus the judge's answers and the workflow's rules and returns the most likely
//! answer set that obeys every rule, in the judge's response shape, with odds per question and the gate's verdict.
//!
//! Request = the System One request (`model`, `state`, `questions` keyed by question id; question `type` noul | choice | score
//! with `instructions` and `criteria`; `images`), as published in https://api.typesafe.ai/openapi.json (v0.2.0) and the Workers AI
//! schema of `@cf/cloudflare/clef` / `clef-flash`, plus one optional `pbit` block: `judge` (the judge's System One response),
//! `weights` / `logw` (per-question weights given directly), `floor`, `rules` (the pbit-ir constructs over question ids).
//! `state`, `instructions`, `images` and the criteria texts are carried, never read: pbit reads numbers, not text.
//!
//! Compilation (no new engine code): one pbit-ir variable per question, its options as values (noul: "false", "true"; choice: the
//! criteria keys; score: the levels "0".."L-1"), log-weight = ln max(p, floor) of the judge's probability (floor 1e-6), rules
//! copied as they are. The program runs exactly as `pbit run` runs it (`--program` prints it).
//! Response = `model`, `answers` (keyed by question id, each in the judge's answer shape plus a `pbit` object), `usage`, then every
//! field of the `pbit run` document. Docs: docs/pbit-ir-json.md "Decision API".
use crate::json::{arr, at, fields, keyed, num, number, obj, opt, req, schema, str as jstr, text, value, weight, InErr, Json};
use std::collections::{HashMap, HashSet};

/// A judge probability p becomes the log-weight ln(max(p, FLOOR)): p = 0 gives ln 1e-6 = -13.815511 (an answer the judge rules
/// out stays possible, 1e-6 times as likely as a certain one, so a rule can still force it). `pbit.floor` overrides it.
pub const FLOOR: f64 = 1e-6;
/// The rule blocks a request may carry in `pbit.rules`: the pbit-ir constructs, over question ids and option names.
pub const RULES: [&str; 7] = ["pairs", "caps", "all_different", "implies", "tables", "precedes", "linear"];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind { Noul, Choice, Score }
impl Kind { pub fn name(self) -> &'static str { match self { Kind::Noul => "noul", Kind::Choice => "choice", Kind::Score => "score" } } }

pub struct Question { pub id: String, pub kind: Kind, pub options: Vec<String>, /// score: the criteria keyed by level ("0", ...)
    pub legend: Option<Json>, /// the compiled log-weights per option (None = no weights given: every option 0)
    pub w: Option<Vec<f64>> }

/// A compiled request: the questions, what is echoed (`model`, `usage`) and the pbit-ir program.
pub struct Request { pub qs: Vec<Question>, pub model: String, pub usage: Json, pub program: Json }

/// Log-weights are rounded to 6 decimals (the JSON printer's precision), so `--program` prints the program that runs.
fn round6(x: f64) -> f64 { (x * 1e6).round() / 1e6 }
fn logw(p: f64, floor: f64) -> f64 { round6(p.max(floor).ln()) }
/// A probability: a number in [0, 1].
fn prob(j: &Json, path: &str) -> Result<f64, InErr> {
    let p = number(j, path)?; if !(0.0..=1.0).contains(&p) { return Err(value(path, format!("{p} is not a probability (0 to 1)"))); } Ok(p)
}

/// Request -> questions + program. Strict, as every pbit document (unknown fields, duplicate ids, wrong types: exit 2), except
/// `pbit.judge`, a vendor's response, of which only `answers` (`type`, `noul`, `probabilities`), `model` and `usage` are read.
pub fn compile(j: &Json) -> Result<Request, InErr> {
    fields(j, "", &["model", "state", "questions", "images", "pbit"])?;
    let model = opt(j, "model").map(|m| text(m, "model").map(str::to_string)).transpose()?;
    let qj = keyed(req(j, "questions", "")?, "questions")?;
    if qj.is_empty() { return Err(value("questions", "no questions (at least one)")); }
    let mut qs = Vec::with_capacity(qj.len());
    for (id, q) in qj {
        let qp = at("questions", id); fields(q, &qp, &["type", "instructions", "criteria"])?;
        let (tp, cp) = (at(&qp, "type"), at(&qp, "criteria"));
        let (kind, options, legend) = match text(req(q, "type", &qp)?, &tp)? {
            "noul" => { if let Some(c) = opt(q, "criteria") { fields(c, &cp, &["true", "false"])?; } (Kind::Noul, vec!["false".to_string(), "true".to_string()], None) }
            "choice" => { let c = keyed(req(q, "criteria", &qp)?, &cp)?;
                if c.is_empty() { return Err(value(&cp, format!("question {id}: no options (criteria needs at least one)"))); }
                (Kind::Choice, c.iter().map(|(k, _)| k.clone()).collect(), None) }
            "score" => { let c = arr(req(q, "criteria", &qp)?, &cp)?;
                if c.is_empty() { return Err(value(&cp, format!("question {id}: no levels (criteria needs at least one)"))); }
                if c.len() > 65535 { return Err(crate::json::limit(&cp, format!("{} levels; at most 65535", c.len()))); }
                (Kind::Score, (0..c.len()).map(|l| l.to_string()).collect(), Some(Json::Obj(c.iter().enumerate().map(|(l, d)| (l.to_string(), d.clone())).collect()))) }
            t => return Err(value(&tp, format!("question {id}: unknown type {t:?} (noul, choice or score)"))) };
        qs.push(Question { id: id.clone(), kind, options, legend, w: None });
    }
    let qidx: HashMap<&str, usize> = qs.iter().enumerate().map(|(i, q)| (q.id.as_str(), i)).collect();
    let pb = opt(j, "pbit"); if let Some(p) = pb { fields(p, "pbit", &["judge", "weights", "logw", "floor", "rules"])?; }
    if let Some(r) = pb.and_then(|p| opt(p, "rules")) { fields(r, "pbit.rules", &RULES)?; rule_options(r, &qs, &qidx)?; }
    let get = |k: &str| pb.and_then(|p| opt(p, k));
    let floor = match get("floor") { None => FLOOR, Some(f) => { let x = number(f, "pbit.floor")?;
        if !(x > 0.0 && x <= 1.0) { return Err(value("pbit.floor", "the probability floor must be above 0 and at most 1")); } x } };
    // per question: log-weights from at most one source (none = uniform, every option 0)
    let mut h: Vec<Option<Vec<f64>>> = vec![None; qs.len()]; let mut src: Vec<&str> = vec![""; qs.len()];
    let mut set = |i: usize, w: Vec<f64>, from: &'static str, path: &str| -> Result<(), InErr> {
        if !src[i].is_empty() { return Err(value(path, format!("question {} has weights in {} already (one source per question)", qs[i].id, src[i]))); }
        src[i] = from; h[i] = Some(w); Ok(()) };
    let find = |id: &str, path: &str| qidx.get(id).copied().ok_or_else(|| value(path, format!("unknown question {id}")));
    // probabilities keyed by option -> log-weights in option order (a missing option has p = 0)
    let by_option = |q: &Question, m: &[(String, Json)], path: &str| -> Result<Vec<f64>, InErr> {
        let mut p = vec![0.0; q.options.len()];
        for (o, x) in m { let k = q.options.iter().position(|v| v == o).ok_or_else(|| value(&at(path, o), format!("question {}: unknown option {o}", q.id)))?;
            p[k] = prob(x, &at(path, o))?; }
        Ok(p.into_iter().map(|p| logw(p, floor)).collect()) };
    let (mut judge_model, mut usage) = (None, None);
    if let Some(jd) = get("judge") {
        let jp = "pbit.judge"; if jd.as_obj().is_none() { return Err(schema(jp, "must be an object (the judge's System One response)")); }
        judge_model = opt(jd, "model").map(|m| text(m, &at(jp, "model")).map(str::to_string)).transpose()?;
        usage = opt(jd, "usage").cloned();
        let ap = at(jp, "answers");
        for (id, a) in keyed(req(jd, "answers", jp)?, &ap)? { let p = at(&ap, id); let i = find(id, &p)?; let q = &qs[i];
            if a.as_obj().is_none() { return Err(schema(&p, "an answer must be an object")); }
            let t = text(req(a, "type", &p)?, &at(&p, "type"))?;
            if t != q.kind.name() { return Err(value(&at(&p, "type"), format!("question {id} is a {} question, its answer a {t}", q.kind.name()))); }
            let w = if q.kind == Kind::Noul { let y = prob(req(a, "noul", &p)?, &at(&p, "noul"))?; vec![logw(1.0 - y, floor), logw(y, floor)] }
                else { let pp = at(&p, "probabilities"); by_option(q, keyed(req(a, "probabilities", &p)?, &pp)?, &pp)? };
            set(i, w, "pbit.judge", &p)?; }
    }
    if let Some(ws) = get("weights") { for (id, x) in keyed(ws, "pbit.weights")? { let p = at("pbit.weights", id); let i = find(id, &p)?; let q = &qs[i];
        // noul: P(true), as the judge's `noul`; choice / score: probabilities keyed by option, as the judge's `probabilities`
        let w = if q.kind == Kind::Noul { let y = prob(x, &p)?; vec![logw(1.0 - y, floor), logw(y, floor)] } else { by_option(q, keyed(x, &p)?, &p)? };
        set(i, w, "pbit.weights", &p)?; } }
    if let Some(ws) = get("logw") { for (id, x) in keyed(ws, "pbit.logw")? { let p = at("pbit.logw", id); let i = find(id, &p)?; let q = &qs[i];
        // log-weights keyed by option, used as they are (a missing option = 0, as pbit-ir's `h`)
        let mut w = vec![0.0; q.options.len()];
        for (o, v) in keyed(x, &p)? { let k = q.options.iter().position(|y| y == o).ok_or_else(|| value(&at(&p, o), format!("question {id}: unknown option {o}")))?; w[k] = weight(v, &at(&p, o))?; }
        set(i, w, "pbit.logw", &p)?; } }
    for (q, w) in qs.iter_mut().zip(&h) { q.w = w.clone(); }
    // the alphabet: score levels first in order (precedes / linear read values as ordered slots), then "false", "true", then
    // every choice option in order of first appearance; each variable allows its own options only
    let levels = qs.iter().filter(|q| q.kind == Kind::Score).map(|q| q.options.len()).max().unwrap_or(0);
    let mut values: Vec<String> = (0..levels).map(|l| l.to_string()).collect();
    if qs.iter().any(|q| q.kind == Kind::Noul) { values.extend(["false".to_string(), "true".to_string()]); }
    let mut seen: HashSet<String> = values.iter().cloned().collect();
    for q in qs.iter().filter(|q| q.kind == Kind::Choice) { for o in &q.options { if seen.insert(o.clone()) { values.push(o.clone()); } } }
    let vars: Vec<Json> = qs.iter().zip(&h).map(|(q, w)| { let mut v = vec![("id", jstr(&q.id)), ("allowed", Json::Arr(q.options.iter().map(|o| jstr(o)).collect()))];
        if let Some(w) = w { v.push(("h", Json::Obj(q.options.iter().zip(w).map(|(o, &x)| (o.clone(), num(x))).collect()))); }
        obj(v) }).collect();
    let mut program = vec![("pbit_ir", num(1.0)), ("comment", jstr(&format!("compiled by pbit evaluate: {} questions", qs.len()))), ("values", Json::Arr(values.iter().map(|v| jstr(v)).collect())), ("vars", Json::Arr(vars))];
    if let Some(r) = get("rules") { for (k, x) in fields(r, "pbit.rules", &RULES)? { if !x.is_null() { program.push((RULES[RULES.iter().position(|r| r == k).unwrap()], x.clone())); } } }
    // the program as printed (`--program`), so a rule weight with more than 6 decimals runs as `pbit run` would read it
    let program = crate::json::parse(&crate::json::write(&obj(program), false)).map_err(|e| value("", format!("internal: the compiled program does not re-parse: {}", e.msg)))?;
    Ok(Request { qs, model: judge_model.or(model).unwrap_or_else(|| "pbit".to_string()),
        usage: usage.unwrap_or_else(|| obj(vec![("input_tokens", num(0.0)), ("output_tokens", num(0.0))])), program })
}

/// Rules name options per question, but pbit-ir's value names are one alphabet shared by every question, where another
/// question's option is accepted and quietly excludes (an `implies` target) or is skipped (a table tuple, a cap member). A user of
/// `evaluate` never sees that alphabet, so every (question, option) a rule names must be one of that question's options, and a value
/// cap must count at least one question that has the value: exit 2 at the rule's path. Malformed shapes and unknown question ids are
/// left to the program's own checks (located by `locate`).
fn rule_options(r: &Json, qs: &[Question], qidx: &HashMap<&str, usize>) -> Result<(), InErr> {
    let has = |q: &str, o: &str, path: String| match qidx.get(q) {
        Some(&i) if !qs[i].options.iter().any(|x| x == o) => Err(value(&path, format!("question {q} has no option {o} (its options: {})", qs[i].options.join(", ")))),
        _ => Ok(()) };
    let items = |k: &str| r.get(k).and_then(Json::as_arr).unwrap_or(&[]);
    fn arr(j: Option<&Json>) -> &[Json] { j.and_then(Json::as_arr).unwrap_or(&[]) }
    for (n, c) in items("implies").iter().enumerate() {
        let (i, t) = (c.get("if"), c.get("then"));
        if let (Some(x), Some(a)) = (i.and_then(|i| i.get("var")).and_then(Json::as_str), i.and_then(|i| i.get("value")).and_then(Json::as_str)) { has(x, a, format!("pbit.rules.implies[{n}].if.value"))?; }
        if let Some(y) = t.and_then(|t| t.get("var")).and_then(Json::as_str) {
            for (m, v) in arr(t.and_then(|t| t.get("in"))).iter().enumerate() { if let Some(v) = v.as_str() { has(y, v, format!("pbit.rules.implies[{n}].then.in[{m}]"))?; } } }
    }
    for (n, c) in items("tables").iter().enumerate() {
        let vars: Vec<Option<&str>> = arr(c.get("vars")).iter().map(Json::as_str).collect();
        for key in ["forbid", "allow"] { for (m, t) in arr(c.get(key)).iter().enumerate() { for (e, (v, q)) in arr(Some(t)).iter().zip(&vars).enumerate() {
            if let (Some(v), Some(q)) = (v.as_str(), q) { has(q, v, format!("pbit.rules.tables[{n}].{key}[{m}][{e}]"))?; } } } }
    }
    for (n, c) in items("caps").iter().enumerate() {
        for (m, mb) in arr(c.get("members")).iter().enumerate() { if let Some([q, v]) = mb.as_arr() {
            if let (Some(q), Some(v)) = (q.as_str(), v.as_str()) { has(q, v, format!("pbit.rules.caps[{n}].members[{m}][1]"))?; } } }
        // a value cap counts the listed questions (default: all) that have the value; one with no such question is vacuous
        if let Some(v) = c.get("value").and_then(Json::as_str) {
            let over: Vec<&Question> = match c.get("vars").and_then(Json::as_arr) { Some(vars) => vars.iter().filter_map(|q| q.as_str().and_then(|q| qidx.get(q)).map(|&i| &qs[i])).collect(), None => qs.iter().collect() };
            if !over.is_empty() && !over.iter().any(|q| q.options.iter().any(|o| o == v)) {
                return Err(value(&format!("pbit.rules.caps[{n}].value"), format!("no question it counts has the option {v}"))); } }
    }
    for (n, c) in items("linear").iter().enumerate() { for (m, t) in arr(c.get("terms")).iter().enumerate() { if let Some([q, v, _]) = t.as_arr() {
        if let (Some(q), Some(v)) = (q.as_str(), v.as_str()) { has(q, v, format!("pbit.rules.linear[{n}].terms[{m}][1]"))?; } } } }
    Ok(())
}

/// A `pbit run` error on the compiled program, located in the request: rule paths (`implies[0].then.var`) under `pbit.rules`;
/// anything else (the size limits on questions x options) at `questions`.
pub fn locate(mut e: InErr) -> InErr {
    let head = e.path.split(['.', '[']).next().unwrap_or("");
    e.path = if RULES.contains(&head) { format!("pbit.rules.{}", e.path) } else { "questions".to_string() }; e
}

/// The `pbit run` document of the compiled program -> the response: `engine`, `model`, `answers`, `usage`, then the document's
/// other fields. `answers` (absent when there is no plan: infeasible, declined, a refusal before sampling, a numeric error) holds,
/// per question in request order, the judge's answer shape filled from pbit: noul `noul` = P(true); choice `choice` = the plan's
/// option, `confidence` = its probability, `probabilities` per option; score `score` = the expected level, `confidence` = the
/// plan level's probability, `legend`, `probabilities` per level; all probabilities under the rules. Plus `pbit`: `value` (the
/// plan's option), `p` (its probability), `judge` (the judge's own answer: its most likely option, on an exact tie the one the
/// plan holds; null without weights), `changed` (they differ) and
/// `released` (the gate released it; exact answers release everything).
pub fn respond(r: &Request, doc: Json) -> Json {
    let Json::Obj(v) = doc else { return doc };
    let get = |k: &str| v.iter().find(|(x, _)| x == k).map(|(_, x)| x);
    let mut out: Vec<(String, Json)> = vec![];
    if let Some(e) = get("engine") { out.push(("engine".into(), e.clone())); }
    out.push(("model".into(), jstr(&r.model)));
    if let (Some(plan), Some(marg)) = (get("plan"), get("marginals")) {
        let rel: HashSet<&str> = get("released").and_then(Json::as_arr).map_or_else(HashSet::new, |a| a.iter().filter_map(Json::as_str).collect());
        let answers = r.qs.iter().map(|q| {
            let m = marg.get(&q.id); let p = |o: &str| m.and_then(|m| m.get(o)).and_then(Json::as_f64).unwrap_or(0.0);
            let val = plan.get(&q.id).and_then(Json::as_str).unwrap_or("").to_string();
            // the judge's own answer: its most likely option; on an exact tie the tied option the plan holds (a tie is no change)
            let judge = q.w.as_ref().map(|w| { let top = w.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                match q.options.iter().position(|o| *o == val) { Some(k) if w[k] == top => val.clone(), _ => q.options[w.iter().position(|&x| x == top).unwrap_or(0)].clone() } });
            let ext = obj(vec![("value", jstr(&val)), ("p", num(p(&val))), ("judge", judge.as_deref().map_or(Json::Null, jstr)),
                ("changed", Json::Bool(judge.as_deref().is_some_and(|j| j != val))), ("released", Json::Bool(rel.contains(q.id.as_str())))]);
            let probs = || Json::Obj(q.options.iter().map(|o| (o.clone(), num(p(o)))).collect());
            let a = match q.kind {
                Kind::Noul => vec![("type", jstr("noul")), ("noul", num(p("true"))), ("pbit", ext)],
                Kind::Choice => vec![("type", jstr("choice")), ("choice", jstr(&val)), ("confidence", num(p(&val))), ("probabilities", probs()), ("pbit", ext)],
                Kind::Score => vec![("type", jstr("score")), ("score", num(q.options.iter().enumerate().map(|(l, o)| l as f64 * p(o)).sum())), ("confidence", num(p(&val))),
                    ("legend", q.legend.clone().unwrap_or(Json::Obj(vec![]))), ("probabilities", probs()), ("pbit", ext)] };
            (q.id.clone(), obj(a)) }).collect();
        out.push(("answers".into(), Json::Obj(answers)));
    }
    out.push(("usage".into(), r.usage.clone()));
    out.extend(v.into_iter().filter(|(k, _)| k != "engine"));
    Json::Obj(out)
}
