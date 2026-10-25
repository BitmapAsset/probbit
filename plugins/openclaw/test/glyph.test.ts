import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { buildBlock, footerLine, FOOTER_MAX, glyphLine, glyphParts } from "../src/block.js";
import { CONFIG_JSON_SCHEMA, resolveConfig } from "../src/config.js";
import { registerStance } from "../src/index.js";
import type { StanceDoc } from "../src/probbit.js";
import { cliEvent, createHost, GROUP_KEY, groupCtx, PROBBIT, tempStateDir } from "./host.js";

const DOC: StanceDoc = {
  line: "Stance: say it plainly; warm; short and complete.",
  status: "ok",
  mood: { valence: { level: "even", p: 0.714 }, pressure: { level: "steady", p: 0.8 } },
  habits: { active: [], bound: [] },
  persona: { name: "Assistant", version: "1.0.0" },
};
const GLYPH: StanceDoc = { ...DOC, glyph: "🙂", glyph_why: "mood valence=even pressure=steady" };

test("glyph footer: after a symbol prefix directly, after a name with a space; a document without a glyph is as before", () => {
  assert.equal(footerLine(59, GLYPH, {}, { prefix: "◆" }), "◆🙂 stance #59 · valence even · pressure steady · say it plainly");
  assert.equal(footerLine(59, GLYPH), "Assistant 🙂 stance #59 · valence even · pressure steady · say it plainly");
  assert.equal(footerLine(59, DOC), "Assistant stance #59 · valence even · pressure steady · say it plainly");
  assert.equal(footerLine(3, { ...GLYPH, glyph: "🙂👀" }, {}, { prefix: "◆" }).startsWith("◆🙂👀 stance #3 · "), true);
});

test("glyph footer: at most FOOTER_MAX code points and the glyph kept, with long input and moved lists", () => {
  const long = footerLine(123_456, { ...GLYPH, line: `Stance: ${"a very long clause ".repeat(12)}` }, {
    inputs: ["praise", "correction", "failure", "security", "launch", "claim_done", "time 0.75"],
    moved: ["valence", "pressure", "caution", "humour", "verbosity", "warmth"],
  });
  assert.ok([...long].length <= FOOTER_MAX, long);
  assert.ok(long.includes("🙂"));
});

test("glyph line: the glyph and the odds of the mood levels taken; none without a glyph", () => {
  assert.equal(
    glyphLine(GLYPH),
    "Mood glyph: 🙂 (valence even 0.71, pressure steady 0.80). Use at most one mood emoji in this reply and make it this one.",
  );
  assert.equal(glyphLine(DOC), undefined);
  assert.equal(glyphLine({ ...GLYPH, mood: {} }), "Mood glyph: 🙂. Use at most one mood emoji in this reply and make it this one.");
});

const SURPRISE: StanceDoc = { ...GLYPH, glyph: "🙂👀", glyph_why: "mood valence=even pressure=steady +surprise z=2.31" };
const HABIT: StanceDoc = { ...GLYPH, glyph: "🔒", glyph_why: "habit security_careful", habits: { active: ["security_careful"], bound: [] } };

test("glyph line under the surprise modifier: the base glyph is the one mood emoji, the modifier is reported apart", () => {
  assert.equal(
    glyphLine(SURPRISE),
    "Mood glyph: 🙂 (valence even 0.71, pressure steady 0.80); surprise registered this turn (z 2.3), shown as 👀 in the footer. Use at most one mood emoji in this reply and make it 🙂.",
  );
  const block = buildBlock(7, SURPRISE, true, {}, { prefix: "◆" }).split("\n");
  assert.equal(block[3], glyphLine(SURPRISE));
  assert.match(block[4]!, /"◆🙂👀 stance #7 · /u);
  // a base or modifier with U+FE0F splits at the first code point and its selector
  assert.deepEqual(glyphParts({ ...SURPRISE, glyph: "🛠\uFE0F👀" }), { base: "🛠\uFE0F", modifier: "👀", z: 2.31 });
  assert.deepEqual(glyphParts({ ...SURPRISE, glyph: "🙂👁\uFE0F" }), { base: "🙂", modifier: "👁\uFE0F", z: 2.31 });
  assert.deepEqual(glyphParts(GLYPH), { base: "🙂" });
  assert.equal(glyphParts(DOC), undefined);
});

test("glyph line for a habit in force that did not bind: the habit's glyph, the plain shape", () => {
  assert.equal(glyphLine(HABIT), "Mood glyph: 🔒 (valence even 0.71, pressure steady 0.80). Use at most one mood emoji in this reply and make it this one.");
  const block = buildBlock(2, HABIT, true).split("\n");
  assert.equal(block[2], "mood: valence even, pressure steady. habits bound: none.");
  assert.match(block[4]!, /"Assistant 🔒 stance #2 · /u);
});

test("glyph footer: U+FE0F counts as one code point toward FOOTER_MAX", () => {
  const tool = footerLine(9, { ...GLYPH, glyph: "🛠\uFE0F" }, {}, { prefix: "◆" });
  assert.ok(tool.startsWith("◆🛠\uFE0F stance #9 · "), tool);
  assert.equal([...tool].length, [...footerLine(9, GLYPH, {}, { prefix: "◆" })].length + 1);
});

test("glyph block: the line after the mood line; glyphInReply off omits it and the footer keeps the glyph", () => {
  const on = buildBlock(4, GLYPH, true).split("\n");
  assert.equal(on[3], "Mood glyph: 🙂 (valence even 0.71, pressure steady 0.80). Use at most one mood emoji in this reply and make it this one.");
  assert.match(on[4]!, /"Assistant 🙂 stance #4 · /u);
  const off = buildBlock(4, GLYPH, true, {}, {}, false).split("\n");
  assert.equal(off.length, 4);
  assert.ok(!off.some((l) => l.startsWith("Mood glyph:")));
  assert.match(off[3]!, /"Assistant 🙂 stance #4 · /u);
  assert.deepEqual(buildBlock(4, DOC, true), buildBlock(4, DOC, true, {}, {}, false));
});

test("config: glyphInReply defaults to true, reads a boolean, warns otherwise; the schema lists it", () => {
  assert.equal(resolveConfig({}, "/home/x").config.glyphInReply, true);
  assert.equal(resolveConfig({ glyphInReply: false }, "/home/x").config.glyphInReply, false);
  const bad = resolveConfig({ glyphInReply: "no" }, "/home/x");
  assert.equal(bad.config.glyphInReply, true);
  assert.deepEqual(bad.warnings, ["glyphInReply: expected a boolean"]);
  assert.equal(CONFIG_JSON_SCHEMA.properties.glyphInReply.type, "boolean");
});

test("integration (real probbit): the footer's glyph is the one the strand logs, event by event", async () => {
  const dir = tempStateDir();
  let clock = 1_791_320_000_000;
  const host = createHost({ lanes: { [GROUP_KEY]: "team-room" }, stateDir: dir, binary: PROBBIT, footerPrefix: "◆" });
  registerStance(host.api, { now: () => clock });
  const texts = ["perfect, thanks — that's exactly it", "no, that's wrong: the floor comes from the other source", "rotate the token and restart the gateway"];
  const footers: string[] = [];
  for (const [i, text] of texts.entries()) {
    clock += 1_800_000;
    const { result } = await host.promptBuild(cliEvent(text), groupCtx(`g-${i + 1}`));
    const block = (result as { appendContext?: string }).appendContext ?? "";
    assert.match(block, /\nMood glyph: \p{Extended_Pictographic}+ \(valence \w+ \d\.\d\d, pressure \w+ \d\.\d\d\)\. /u);
    footers.push(/End the visible reply with one line: "(.*)"\.$/u.exec(block)?.[1] ?? "");
  }
  const strand = readFileSync(join(dir, "strands", "team-room.strand"), "utf8").split("\n").filter(Boolean).slice(1);
  const lines = strand.map((l) => JSON.parse(l) as { glyph?: string; inputs?: { security?: boolean } });
  const glyphs = lines.map((l) => l.glyph);
  assert.equal(glyphs.length, 3);
  for (const [i, g] of glyphs.entries()) {
    assert.ok(g, `event ${i + 1}: no glyph on the strand line`);
    assert.ok(footers[i]!.startsWith(`◆${g} stance #${i + 1} · `), `${footers[i]} vs ${g}`);
  }
  // the habit in force: on every security event the bundled persona's security_careful glyph, bound or not
  assert.ok(lines.some((l) => l.inputs?.security === true), "the third message must read as a security topic");
  for (const l of lines) assert.equal(l.glyph === "🔒", l.inputs?.security === true, JSON.stringify(l.inputs));
});
