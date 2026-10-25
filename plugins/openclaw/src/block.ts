import type { StanceDoc } from "./probbit.js";

/** 3-5 words of the stance line: whole leading clauses when they fit, without the "Stance:" lead or trailing punctuation. */
export function footerWords(line: string): string {
  const clauses = line
    .replace(/^\s*stance:\s*/i, "")
    .split(";")
    .map((c) => c.trim().replace(/[\s,.:]+$/, ""))
    .filter(Boolean);
  const picked: string[] = [];
  let count = 0;
  for (const clause of clauses) {
    const n = clause.split(/\s+/).length;
    if (count > 0 && count + n > 5) break;
    picked.push(clause);
    count += n;
    if (count >= 3) break;
  }
  if (count < 3 && picked.length < clauses.length) picked.push(clauses[picked.length] ?? "");
  return picked.join("; ").split(/\s+/).filter(Boolean).slice(0, 5).join(" ").replace(/[\s;,.:]+$/, "");
}

function level(doc: StanceDoc, mood: string): string {
  return doc.mood?.[mood]?.level ?? "unknown";
}

/** The name that heads the block and opens the footer: the persona's name from the stance document, else "agent". */
export function blockName(doc: StanceDoc, name?: string): string {
  const n = (name ?? doc.persona?.name ?? "").trim();
  return n || "agent";
}

/** What the footer reports beyond the stance itself: the inputs this event carried and what moved since the last one. */
export type FooterFacts = {
  /** Inputs fed to the engine this event (flag ids, and `time 0.xx` for time_pressure); elapsed_hours is not listed. */
  inputs?: string[];
  /** Mood and trait names whose level changed from the lane's previous event; undefined when that event is unknown. */
  moved?: string[];
};

/** Naming: `name` heads the block (default: the persona's name); `prefix` opens the footer line (default: the name). */
export type BlockNames = { name?: string; prefix?: string };

export const FOOTER_MAX = 120;

function width(s: string): number {
  return [...s].length;
}

/** Mood names (valence, pressure, then the rest sorted) and stance trait names (sorted) whose level differs. */
export function movedNames(prev: StanceDoc | undefined, cur: StanceDoc): string[] | undefined {
  if (!prev) return undefined;
  const out: string[] = [];
  const moods = new Set([...Object.keys(prev.mood ?? {}), ...Object.keys(cur.mood ?? {})]);
  const moodOrder = ["valence", "pressure", ...[...moods].filter((m) => m !== "valence" && m !== "pressure").sort()];
  for (const m of moodOrder) {
    if (!moods.has(m)) continue;
    if ((prev.mood?.[m]?.level ?? "") !== (cur.mood?.[m]?.level ?? "")) out.push(m);
  }
  const traits = [...new Set([...Object.keys(prev.stance ?? {}), ...Object.keys(cur.stance ?? {})])].sort();
  for (const t of traits) {
    if ((prev.stance?.[t]?.level ?? "") !== (cur.stance?.[t]?.level ?? "")) out.push(t);
  }
  return out;
}

/** The input list as the footer prints it: flags, then `time 0.xx`. */
export function footerInputs(flags: string[], timePressure: number | undefined): string[] {
  return [...flags, ...(timePressure !== undefined ? [`time ${timePressure.toFixed(2)}`] : [])];
}

/** ` · <prefix>: a, b, +k` showing the first `keep` names; "" for an empty list. */
function listPart(prefix: string, names: string[], keep: number): string {
  if (names.length === 0) return "";
  const more = names.length - keep;
  return ` · ${prefix}: ${[...names.slice(0, keep), ...(more > 0 ? [`+${more}`] : [])].join(", ")}`;
}

/** The glyph as it follows the footer's prefix: right after a symbol prefix (`◆🙂`), after a space when the prefix ends in a letter or digit. */
function glyphAfter(prefix: string, glyph: string | undefined): string {
  if (!glyph) return "";
  return /[\p{L}\p{N}]$/u.test(prefix) ? ` ${glyph}` : glyph;
}

/**
 * `<prefix> stance #N · valence <l> · pressure <l> · <line words>` (`<prefix><glyph> stance #N · …` when the stance document
 * carries the engine's glyph; a space between the two after a prefix that ends in a letter or digit), then ` · in: <inputs>` when the event carried inputs
 * and ` · moved: <names>` when levels changed. At most FOOTER_MAX characters. When both lists do not fit, names give way
 * to "+k" so that both parts stay visible: `moved` shortens first, then `in` (what fired outranks what moved). If even
 * "+k" cannot fit, `moved` drops, then `in`.
 */
export function footerLine(n: number, doc: StanceDoc, facts: FooterFacts = {}, names: BlockNames = {}): string {
  const prefix = (names.prefix ?? "").trim() || blockName(doc, names.name);
  const full = `${prefix}${glyphAfter(prefix, doc.glyph)} stance #${n} · valence ${level(doc, "valence")} · pressure ${level(doc, "pressure")} · ${footerWords(doc.line ?? "")}`;
  const head = width(full) <= FOOTER_MAX ? full : [...full].slice(0, FOOTER_MAX).join("").trimEnd();
  const inputs = facts.inputs ?? [];
  const moved = facts.moved ?? [];
  const room = FOOTER_MAX - width(head);
  for (let keepIn = inputs.length; keepIn >= 0; keepIn--) {
    for (let keepMoved = moved.length; keepMoved >= 0; keepMoved--) {
      const text = listPart("in", inputs, keepIn) + listPart("moved", moved, keepMoved);
      if (width(text) <= room) return head + text;
    }
  }
  for (const text of [listPart("in", inputs, inputs.length), listPart("in", inputs, 0)]) if (width(text) <= room) return head + text;
  return head;
}

/**
 * `Mood glyph: <glyph> (valence <l> <p>, pressure <l> <p>). …`: the engine's glyph and the odds of the mood levels taken, so
 * the reply's mood emoji is the engine's reading, not the model's guess. Undefined when the document has no glyph.
 */
export function glyphLine(doc: StanceDoc): string | undefined {
  if (!doc.glyph) return undefined;
  const odds = ["valence", "pressure"]
    .filter((m) => doc.mood?.[m]?.level !== undefined)
    .map((m) => `${m} ${level(doc, m)}${typeof doc.mood?.[m]?.p === "number" ? ` ${(doc.mood[m].p as number).toFixed(2)}` : ""}`);
  return `Mood glyph: ${doc.glyph}${odds.length ? ` (${odds.join(", ")})` : ""}. Use at most one mood emoji in this reply and make it this one.`;
}

/** The block the model sees for this turn. `glyphInReply` (default true) adds the glyph line when the document has a glyph. */
export function buildBlock(n: number, doc: StanceDoc, footer: boolean, facts: FooterFacts = {}, names: BlockNames = {}, glyphInReply = true): string {
  const line = (doc.line ?? "").trim();
  const status = doc.status && doc.status !== "ok" ? `, status ${doc.status}` : "";
  const moods = ["valence", "pressure", ...Object.keys(doc.mood ?? {}).filter((m) => m !== "valence" && m !== "pressure").sort()]
    .filter((m) => doc.mood?.[m] !== undefined)
    .map((m) => `${m} ${level(doc, m)}`);
  const bound = doc.habits?.bound?.length ? doc.habits.bound.join(", ") : "none";
  const lines = [
    `## ${blockName(doc, names.name)} stance (probbit, event ${n}${status})`,
    line,
    `mood: ${moods.length ? moods.join(", ") : "none"}. habits bound: ${bound}.`,
  ];
  const glyph = glyphInReply ? glyphLine(doc) : undefined;
  if (glyph) lines.push(glyph);
  if (footer) {
    const foot = footerLine(n, doc, facts, names);
    lines.push(`Write from this stance. End the visible reply with one line: "${foot}".`);
  } else {
    lines.push("Write from this stance.");
  }
  return lines.join("\n");
}
