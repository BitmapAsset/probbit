//! The dead-rule lint (`probbit persona lint PERSONA --host MANIFEST`): a habit whose `when` needs an input the host never
//! supplies (or a history feature of such a flag, or a sense the host never reads) can never come into force on that host.
//! The host manifest lists what a host supplies: {"host": NAME, "inputs": [ids], "senses": [ids]}. Arithmetic over the file:
//! every unsupplied input sits at its default and every history feature of an unsupplied flag at 0.
use super::*;

/// The manifest: (host name, inputs, senses); Err = its path and why
pub fn manifest(j: &Json) -> R<(String, Vec<String>, Vec<String>)> {
    let kv = keys(j, "host", &["host", "inputs", "senses", "source", "comment"], &["host", "inputs"])?;
    let name = get(kv, "host").and_then(Json::as_str).filter(|s| !s.is_empty()).ok_or_else(|| perr("host.host", "a non-empty name"))?.to_string();
    let ids = |k: &str| -> R<Vec<String>> { match get(kv, k) { None | Some(Json::Null) => Ok(vec![]), Some(x) => strs(x).ok_or_else(|| perr(&at("host", k), "a list of ids")) } };
    Ok((name, ids("inputs")?, ids("senses")?))
}
/// The sense a sense-lowered input reads (None for any other input)
fn sense_of(p: &Persona, id: &str) -> Option<String> {
    p.senses.as_ref()?.list.iter().find(|s| id.strip_prefix("sense_").and_then(|r| r.strip_prefix(s.id.as_str())).is_some_and(|r| r.starts_with('_'))).map(|s| s.id.clone())
}
/// Per habit whose `when` cannot hold on this host: {habit, path, needs, why}
pub fn dead(p: &Persona, inputs: &[String], senses: &[String], eng: Engine) -> R<Vec<Json>> {
    let st = init(p, None, false, eng); let (vals, hist, _, _) = resolve_inputs(p, &st, &[])?;
    let mut out = vec![];
    for (i, h) in p.habits.iter().enumerate() { for (k, c) in &h.when {
        // what the condition reads, and whether this host supplies it
        let (needs, supplied) = if let Some(sid) = sense_of(p, k) { (format!("sense {sid}"), senses.contains(&sid)) }
            else if let Some(x) = p.input(k) { if lowered(p, &x.id) { continue } (format!("input {k}"), inputs.contains(k)) }
            else if let Some(hf) = p.history.iter().find(|x| x.id == *k) { (format!("input {} (history feature {k})", hf.of), inputs.contains(&hf.of)) }
            else { continue };
        if supplied || cond_true(p, k, c, &vals, &hist) { continue }
        let s = |x: &str| Json::Str(x.to_string());
        out.push(Json::Obj(vec![("habit".into(), s(&h.id)), ("path".into(), s(&format!("habits[{i}].when.{k}"))), ("needs".into(), s(&needs)),
            ("why".into(), s(&format!("dead on this host: the host never supplies {needs}, so `{k}` stays at its default and this condition never holds")))]));
        break } }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(prog: &Json, f: &Flags) -> Json { run_program(prog, f, 1, 100, 0) }
    const DOC: &str = r#"{"probbit_persona":1,"identity":{"name":"H","version":"1"},
        "traits":[{"id":"caution","levels":["low","high"]}],
        "inputs":[{"id":"claim_done","kind":"flag"},{"id":"praise","kind":"flag"},{"id":"busy","kind":"flag"}],
        "history":[{"id":"praise_run","of":"praise","kind":"streak","cap":3}],
        "habits":[{"id":"verify_before_done","when":{"claim_done":true},"then":{"caution":["high"]}},
                  {"id":"calm_when_idle","when":{"busy":false},"then":{"caution":["low"]}},
                  {"id":"praised_twice","when":{"praise_run":{"at_least":2}},"then":{"caution":["low"]}}]}"#;
    #[test]
    fn a_habit_on_an_unsupplied_input_is_dead_and_alive_when_supplied() {
        let p = build(&json::parse(DOC).unwrap()).unwrap(); let ids = |v: &[Json]| v.iter().map(|d| d.get("habit").and_then(Json::as_str).unwrap().to_string()).collect::<Vec<_>>();
        // busy=false holds at the default, so calm_when_idle is never dead
        assert_eq!(ids(&dead(&p, &[], &[], &run).unwrap()), ["verify_before_done", "praised_twice"]);
        assert_eq!(ids(&dead(&p, &["claim_done".into()], &[], &run).unwrap()), ["praised_twice"]);
        assert!(dead(&p, &["claim_done".into(), "praise".into()], &[], &run).unwrap().is_empty());
        assert_eq!(canon(&dead(&p, &[], &[], &run).unwrap()[0]), r#"{"habit":"verify_before_done","needs":"input claim_done","path":"habits[0].when.claim_done","why":"dead on this host: the host never supplies input claim_done, so `claim_done` stays at its default and this condition never holds"}"#);
    }
    #[test]
    fn manifest_is_read_strictly() {
        assert!(manifest(&json::parse(r#"{"host":"x","inputs":["a"],"senses":["s"]}"#).unwrap()).is_ok());
        let e = manifest(&json::parse(r#"{"host":"x","input":["a"]}"#).unwrap()).unwrap_err(); assert_eq!(e.path, "host.input");
    }
    /// The shipped manifests against the habit both harnesses carry (Pepe: verify_before_done; Hermes: verify_before_report),
    /// each `when: {claim_done: true}`: dead under both, in force once a manifest supplies claim_done
    #[test]
    fn shipped_manifests_find_the_claim_done_habit_dead() {
        let p = build(&json::parse(&DOC.replace("busy", "failure")).unwrap()).unwrap();
        for m in [include_str!("../../examples/hosts/openclaw-pepe.json"), include_str!("../../examples/hosts/hermes.json")] {
            let (_, ins, sns) = manifest(&json::parse(m).unwrap()).unwrap();
            let d = dead(&p, &ins, &sns, &run).unwrap(); assert_eq!(d[0].get("habit").and_then(Json::as_str), Some("verify_before_done"));
            let mut plus = ins.clone(); plus.push("claim_done".into());
            assert!(dead(&p, &plus, &sns, &run).unwrap().iter().all(|x| x.get("habit").and_then(Json::as_str) != Some("verify_before_done"))); }
    }
}
