import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { parseDeclaredInputs, readDeclaredInputs, type DeclaredInputsCache } from "../src/probbit.js";
import { PERSONA_SRC, PERSONA_V01 } from "./host.js";

const V02_INPUTS = ["praise", "correction", "failure", "owner_upset", "security", "time_pressure", "launch", "claim_done", "win", "wants_energy"];

test("persona inputs: the bundled persona declares the ten inputs, win and wants_energy among them", () => {
  const ids = parseDeclaredInputs(readFileSync(PERSONA_SRC, "utf8"));
  assert.deepEqual([...(ids ?? [])].sort(), [...V02_INPUTS].sort());
});

test("persona inputs: the v0.1 fixture declares eight, without win or wants_energy", () => {
  const ids = parseDeclaredInputs(readFileSync(PERSONA_V01, "utf8"));
  assert.deepEqual([...(ids ?? [])].sort(), V02_INPUTS.filter((i) => i !== "win" && i !== "wants_energy").sort());
});

test("persona inputs: only the top-level inputs block counts; comments, flow items and quotes are handled", () => {
  const yaml = [
    "traits:",
    "  - id: warmth",
    "inputs:   # the flags",
    "  - id: praise",
    "    kind: flag",
    "# a column-0 comment does not end the block",
    "  - {id: win, kind: flag}",
    '  - id: "wants_energy"',
    "history:",
    "  - id: praise_recently",
    "    of: praise",
  ].join("\n");
  assert.deepEqual([...(parseDeclaredInputs(yaml) ?? [])], ["praise", "win", "wants_energy"]);
  assert.equal(parseDeclaredInputs("traits:\n  - id: warmth\n"), undefined, "no inputs block");
  assert.equal(parseDeclaredInputs("inputs: []\n"), undefined, "an inline block is not parsed");
  assert.equal(parseDeclaredInputs("inputs:\nhistory:\n  - id: x\n"), undefined, "empty block");
});

test("persona inputs: cached by mtime and size; a rewrite is picked up; a missing file is undefined", () => {
  const dir = mkdtempSync(join(tmpdir(), "probbit-persona-"));
  const path = join(dir, "p.yaml");
  writeFileSync(path, "inputs:\n  - id: praise\n");
  const cache: DeclaredInputsCache = new Map();
  const first = readDeclaredInputs(path, cache);
  assert.deepEqual([...(first ?? [])], ["praise"]);
  assert.equal(readDeclaredInputs(path, cache), first, "same Set object: served from the cache");
  writeFileSync(path, "inputs:\n  - id: praise\n  - id: win\n");
  const later = new Date(Date.now() + 5_000);
  utimesSync(path, later, later);
  assert.deepEqual([...(readDeclaredInputs(path, cache) ?? [])], ["praise", "win"]);
  assert.equal(readDeclaredInputs(join(dir, "missing.yaml"), cache), undefined);
});
