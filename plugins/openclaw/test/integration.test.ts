import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { registerStance } from "../src/index.js";
import { cliEvent, createHost, GROUP_KEY, groupCtx, PROBBIT, tempStateDir } from "./host.js";

const LANE = "team-room";

test("integration (real probbit): praise -> correction -> security through the hook", async () => {
  const dir = tempStateDir();
  const t0 = 1_791_320_000_000;
  let clock = t0;
  const host = createHost({ lanes: { [GROUP_KEY]: LANE }, stateDir: dir, binary: PROBBIT });
  registerStance(host.api, { now: () => clock });

  const turns: Array<[string, number]> = [
    ["perfect, thanks — that's exactly it", 0],
    ["no, that's wrong: the floor comes from the other source", 1_800_000],
    ["rotate the token and restart the gateway", 1_800_000],
  ];
  const learned: unknown[] = [];
  const blocks: string[] = [];
  for (const [i, [text, advance]] of turns.entries()) {
    clock += advance;
    const { result, threw } = await host.promptBuild(cliEvent(text), groupCtx(`run-${i + 1}`));
    assert.equal(threw, false);
    const block = (result as { appendContext?: string }).appendContext;
    assert.ok(block, `turn ${i + 1}: no block (${JSON.stringify(host.logs)})`);
    blocks.push(block);
    const state = JSON.parse(readFileSync(join(dir, "state", `${LANE}.json`), "utf8")) as { learned: unknown; turn: number };
    learned.push(state.learned);
    assert.equal(state.turn, i + 1);
  }

  const strand = readFileSync(join(dir, "strands", `${LANE}.strand`), "utf8").split("\n").filter(Boolean);
  assert.equal(strand.length, 4, "header + 3 events");
  const events = strand.slice(1).map((l) => JSON.parse(l) as { n: number; inputs: Record<string, unknown> });
  assert.deepEqual(events.map((e) => e.n), [1, 2, 3]);
  assert.deepEqual(
    events.map((e) => e.inputs.elapsed_hours),
    [0, 0.5, 0.5],
  );

  const verify = JSON.parse(execFileSync(PROBBIT, ["live", "verify", join(dir, "strands", `${LANE}.strand`)], { encoding: "utf8" }));
  assert.equal(verify.ok, true);
  assert.equal(verify.events, 3);

  const log = readFileSync(join(dir, "log", `${LANE}.jsonl`), "utf8").split("\n").filter(Boolean);
  assert.equal(log.length, 3);
  const metas = log.map((l) => JSON.parse(l.split("\t")[3]!) as { method: string; flags: string[]; n: number });
  assert.deepEqual(metas.map((m) => m.flags), [["praise"], ["correction"], ["security"]]);
  assert.deepEqual(metas.map((m) => m.method), ["keyword", "keyword", "keyword"]);
  assert.deepEqual(metas.map((m) => m.n), [1, 2, 3]);
  assert.equal(readFileSync(join(dir, "last", `${LANE}.ts`), "utf8").trim(), String(Math.floor(clock / 1000)));

  const zero = JSON.stringify(learned[0]);
  assert.notEqual(JSON.stringify(learned[1]), zero, "turn 2 (a correction) must move the learned deltas");
  assert.match(blocks[2]!, /habits bound: .*\.|habits bound: none\./);

  console.log("EVIDENCE " + JSON.stringify({ blocks, learned, events, verify, metas }, null, 1));
});
