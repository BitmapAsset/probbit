"""Tests for `probbit mcp`, the Model Context Protocol server, over pipes with a dependency-free client (stdlib unittest; run:
python3 python/test_mcp.py). Both eras: a legacy client opens with `initialize`; a 2026-07-28 client puts the protocol version
and its capabilities in every request's `_meta`. Every tool answer is compared with the CLI's own output for the same input."""
import json, os, subprocess, tempfile, unittest
import probbit

EX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples")
MODERN = {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientCapabilities": {},
          "io.modelcontextprotocol/clientInfo": {"name": "test_mcp", "version": "1"}}
VOLATILE = ("ms", "telemetry", "phases")


def cli(args, doc=None):
    p = subprocess.run([probbit.find_binary(), *args], input=json.dumps(doc) if doc is not None else "", capture_output=True, encoding="utf-8")
    return p.returncode, p.stdout


def stable(d):
    return {k: v for k, v in d.items() if k not in VOLATILE}


class Client:
    def __init__(self):
        self.p = subprocess.Popen([probbit.find_binary(), "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                  encoding="utf-8", bufsize=1)
        self.n = 0

    def send_raw(self, line):
        self.p.stdin.write(line + "\n"); self.p.stdin.flush()

    def recv(self):
        line = self.p.stdout.readline()
        assert line.endswith("\n") and "\n" not in line[:-1], repr(line)
        return json.loads(line)

    def request(self, method, params=None, meta=None):
        self.n += 1
        msg = {"jsonrpc": "2.0", "id": self.n, "method": method}
        if params is not None or meta is not None:
            msg["params"] = dict(params or {})
            if meta is not None:
                msg["params"]["_meta"] = meta
        self.send_raw(json.dumps(msg))
        r = self.recv()
        assert r.get("id") == self.n and r.get("jsonrpc") == "2.0", r
        return r

    def close(self):
        self.p.stdin.close()
        code = self.p.wait(timeout=20)
        rest, err = self.p.stdout.read(), self.p.stderr.read()
        self.p.stdout.close(); self.p.stderr.close()
        return code, rest, err


class Mcp(unittest.TestCase):
    def setUp(self):
        self.c = Client()

    def tearDown(self):
        if self.c.p.poll() is None:
            self.c.close()

    def legacy(self, version="2025-06-18"):
        r = self.c.request("initialize", {"protocolVersion": version, "capabilities": {}, "clientInfo": {"name": "test_mcp", "version": "1"}})
        self.c.send_raw(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}))  # a notification: no answer
        return r["result"]

    def call(self, name, arguments, meta=None):
        r = self.c.request("tools/call", {"name": name, "arguments": arguments}, meta)
        self.assertIn("result", r, r)
        return r["result"]

    def test_legacy_handshake_lists_eight_tools(self):
        init = self.legacy()
        self.assertEqual(init["protocolVersion"], "2025-06-18"); self.assertEqual(init["serverInfo"]["name"], "probbit")
        self.assertIn("tools", init["capabilities"]); self.assertNotIn("resultType", init)
        tools = self.c.request("tools/list")["result"]["tools"]
        self.assertEqual([t["name"] for t in tools], ["probbit_decide", "probbit_run", "probbit_stats", "probbit_demo", "probbit_evaluate", "probbit_persona_init", "probbit_persona_turn", "probbit_persona_fuzz", "probbit_live_event", "probbit_live_verify"])
        for t in tools:
            self.assertEqual(t["inputSchema"]["type"], "object"); self.assertTrue(t["description"])
        self.assertIn("flags", tools[0]["inputSchema"]["properties"]); self.assertIn("vars", tools[1]["inputSchema"]["properties"])
        ev = tools[4]["inputSchema"]
        self.assertEqual(sorted(ev["properties"]["probbit"]["properties"]["rules"]["properties"]), sorted(["pairs", "caps", "all_different", "implies", "tables", "precedes", "linear"]))
        self.assertIn("cap", ev["$defs"]); self.assertIn("program", ev["properties"]["flags"]["properties"])
        self.assertEqual(self.c.request("ping")["result"], {})
        self.assertEqual(self.legacy("1999-01-01")["protocolVersion"], "2025-11-25")  # unknown -> the newest handshake revision

    def test_tools_answer_exactly_as_the_cli(self):
        self.legacy("2025-11-25")
        demo = self.call("probbit_demo", {"tasks": 12, "seed": 7})
        code, out = cli(["demo", "--tasks", "12", "--seed", "7"])
        self.assertFalse(demo["isError"]); self.assertEqual(demo["content"][0]["text"], out.rstrip("\n"))
        doc = demo["structuredContent"]
        d = self.call("probbit_decide", dict(doc, flags={"sweeps": 200, "polish_ms": 0, "summary": True}))
        code, out = cli(["decide", "--sweeps", "200", "--polish-ms", "0", "--summary"], doc)
        self.assertFalse(d["isError"]); self.assertEqual(stable(d["structuredContent"]), stable(json.loads(out)))
        self.assertEqual(json.loads(d["content"][0]["text"]), d["structuredContent"])
        with open(os.path.join(EX, "knapsack-20.json")) as f:
            prog = json.load(f)
        r = self.call("probbit_run", dict(prog, flags={"op": "sample", "sweeps": 300, "polish_ms": 0, "collective": False}))
        code, out = cli(["run", "--op", "sample", "--sweeps", "300", "--polish-ms", "0", "--collective", "off"], prog)
        self.assertEqual(stable(r["structuredContent"]), stable(json.loads(out)))
        s = self.call("probbit_stats", {"sweeps": 50})
        self.assertIn("self_test", s["structuredContent"]); self.assertFalse(s["isError"])

    def test_answers_that_say_no_are_not_errors_and_errors_are(self):
        self.legacy()
        inf = self.call("probbit_run", {"probbit_ir": 1, "values": ["a"], "vars": [{"id": "x"}, {"id": "y"}], "caps": [{"value": "a", "limit": 1}]})
        self.assertFalse(inf["isError"]); self.assertEqual(inf["structuredContent"]["verdict"], "infeasible")
        bad = self.call("probbit_run", {"probbit_ir": 1, "values": ["a"], "vars": [{"id": "x", "zz": 1}]})
        self.assertTrue(bad["isError"]); self.assertEqual(bad["structuredContent"]["error"]["code"], "schema")
        flag = self.call("probbit_decide", {"workers": [{"id": "A", "cap": 1}], "tasks": [{"scores": {"A": 1}}], "flags": {"bogus": 1}})
        self.assertTrue(flag["isError"]); self.assertIn("unknown argument", flag["content"][0]["text"]); self.assertNotIn("structuredContent", flag)
        self.assertEqual(self.c.request("tools/call", {"name": "nope", "arguments": {}})["error"]["code"], -32602)
        self.assertEqual(self.c.request("tools/call", {"name": "probbit_demo", "arguments": [1]})["error"]["code"], -32602)
        self.assertEqual(self.c.request("resources/list")["error"]["code"], -32601)

    def test_evaluate_answers_exactly_as_the_cli(self):
        self.legacy("2025-11-25")
        with open(os.path.join(EX, "evaluate", "support-12.json")) as f:
            req = json.load(f)
        for flags, args in (({}, []), ({"summary": True}, ["--summary"]), ({"op": "sample", "sweeps": 2000, "polish_ms": 0}, ["--op", "sample", "--sweeps", "2000", "--polish-ms", "0"])):
            r = self.call("probbit_evaluate", dict(req, flags=flags))
            code, out = cli(["evaluate", *args], req)
            self.assertFalse(r["isError"]); self.assertEqual(code, 0); self.assertEqual(stable(r["structuredContent"]), stable(json.loads(out)))
            self.assertEqual(r["structuredContent"]["answers"]["team"]["probbit"]["value"], "technical")  # the judge said billing; the rules move it
        prog = self.call("probbit_evaluate", dict(req, flags={"program": True}))
        code, out = cli(["evaluate", "--program"], req)
        self.assertEqual(prog["content"][0]["text"], out.rstrip("\n")); self.assertEqual(prog["structuredContent"]["probbit_ir"], 1)
        refused = self.call("probbit_evaluate", dict(req, flags={"op": "sample", "sweeps": 400, "polish_ms": 0}))  # exit 3: an answer
        self.assertFalse(refused["isError"]); self.assertEqual(refused["structuredContent"]["verdict"], "refused")
        self.assertTrue(all(not a["probbit"]["released"] for a in refused["structuredContent"]["answers"].values()))
        bad = self.call("probbit_evaluate", {"questions": {"a": {"type": "boolean"}}})
        self.assertTrue(bad["isError"]); self.assertEqual(bad["structuredContent"]["error"], {"code": "value", "path": "questions.a.type", "message": bad["structuredContent"]["error"]["message"]})

    def test_persona_tools_answer_exactly_as_the_cli(self):
        self.legacy("2025-11-25")
        path = os.path.join(EX, "persona", "tutor.yaml")
        with open(os.path.join(EX, "persona", "tutor.json")) as f:
            doc = json.load(f)
        init = self.call("probbit_persona_init", {"persona": doc, "seed": 2})
        code, out = cli(["persona", "init", path, "--seed", "2"])
        self.assertFalse(init["isError"]); self.assertEqual(code, 0); self.assertEqual(init["content"][0]["text"], out.rstrip("\n"))
        state = init["structuredContent"]
        import tempfile
        with tempfile.TemporaryDirectory() as d:
            sp = os.path.join(d, "state.json")
            with open(sp, "w") as f:
                f.write(out)
            for inputs in ({"confused": True, "error": True}, {"loss": True, "sentiment": "negative"}, {}):
                r = self.call("probbit_persona_turn", {"persona_path": path, "state": state, "inputs": inputs})
                code, out = cli(["persona", "turn", path, "--state", sp, "--inputs", json.dumps(inputs)])
                self.assertFalse(r["isError"]); self.assertEqual(code, 0)
                self.assertEqual(r["structuredContent"]["stance"], json.loads(out))  # MCP answer == CLI answer
                with open(sp) as f:
                    self.assertEqual(r["structuredContent"]["state"], json.load(f))  # and the same next state
                self.assertEqual(json.loads(r["content"][0]["text"]), r["structuredContent"])
                state = r["structuredContent"]["state"]
        refused = self.call("probbit_persona_turn", {"persona": dict(doc, engine={"op": "sample", "sweeps": 8}), "state": self.call("probbit_persona_init", {"persona": dict(doc, engine={"op": "sample", "sweeps": 8})})["structuredContent"], "inputs": {}})
        self.assertFalse(refused["isError"]); self.assertIn(refused["structuredContent"]["stance"]["status"], ("refused", "partial", "fallback"))  # a refusal is an answer
        bad = self.call("probbit_persona_turn", {"persona": doc, "state": state, "inputs": {"stakes": 2}})
        self.assertTrue(bad["isError"]); self.assertEqual(bad["structuredContent"]["error"]["code"], "persona"); self.assertEqual(bad["structuredContent"]["error"]["path"], "inputs.stakes")
        for args, where in (({"persona": doc, "persona_path": path, "state": state}, "arguments"), ({"persona": doc, "state": state, "extra": 1}, "arguments.extra"),
                            ({"persona": doc}, "arguments.state"), ({"persona": [1], "state": state}, "arguments.persona")):
            e = self.call("probbit_persona_turn", args)
            self.assertTrue(e["isError"]); self.assertEqual(e["structuredContent"]["error"]["path"], where)

    def test_persona_fuzz_answers_exactly_as_the_cli(self):
        self.legacy("2025-11-25")
        tutor = os.path.join(EX, "..", "probbit-cli", "tests", "fixtures", "persona", "tutor-0.5.0.yaml")
        rule = "{when: {sentiment: negative}, then: {humour: {at_most: light}}}"
        r = self.call("probbit_persona_fuzz", {"persona_path": tutor, "never": rule, "seeds": "0-4", "scripts": 10})
        code, out = cli(["persona", "fuzz", tutor, "--never", rule, "--seeds", "0-4", "--scripts", "10", "--json"])
        self.assertFalse(r["isError"]); self.assertEqual(code, 1)  # a counterexample: an answer (exit 1 on the CLI)
        self.assertEqual(r["content"][0]["text"], out.rstrip("\n"))  # MCP answer == CLI answer, byte for byte
        self.assertTrue(r["structuredContent"]["found"])
        self.assertEqual(r["structuredContent"]["properties"][0]["shortest"]["script"], [{"sentiment": "negative"}])
        with open(os.path.join(EX, "persona", "tutor.json"), encoding="utf-8") as f:
            doc = json.load(f)
        inline = self.call("probbit_persona_fuzz", {"persona": doc, "never": {"when": {"loss": True}, "then": {"humour": ["none"]}}, "seeds": [0, 1], "scripts": 5})
        self.assertFalse(inline["isError"]); self.assertFalse(inline["structuredContent"]["found"])  # a habit never breaks
        for args, where in (({"persona_path": tutor}, "arguments"), ({"persona_path": tutor, "never": rule, "seeds": "9-1"}, "arguments.seeds"),
                            ({"persona_path": tutor, "never": rule, "extra": 1}, "arguments.extra"), ({"persona_path": tutor, "never": {"when": {"sentimentx": "negative"}, "then": {"humour": ["none"]}}}, "arguments.never.when.sentimentx")):
            e = self.call("probbit_persona_fuzz", args)
            self.assertTrue(e["isError"]); self.assertEqual(e["structuredContent"]["error"]["path"], where)

    def test_live_event_logs_a_strand_the_cli_verifies(self):
        self.legacy("2025-11-25")
        path = os.path.join(EX, "persona", "tutor.yaml")
        with tempfile.TemporaryDirectory() as d:
            strand = os.path.join(d, "pip.strand")
            r = self.call("probbit_live_event", {"persona_path": path, "seed": 2, "event": {"loss": True}, "strand_path": strand})
            self.assertFalse(r["isError"]); self.assertEqual(r["structuredContent"]["stance"]["stance"]["humour"]["level"], "none")
            state = r["structuredContent"]["state"]
            r = self.call("probbit_live_event", {"persona_path": path, "state": state, "event": {"praise": True, "elapsed_hours": 6.5}, "strand_path": strand})
            self.assertEqual(r["structuredContent"]["strand"]["events"], 2)
            code, out = cli(["live", "verify", strand])
            self.assertEqual(code, 0); self.assertEqual(json.loads(out)["last_line"], r["structuredContent"]["strand"]["head"])  # MCP writes, the CLI replays
            v = self.call("probbit_live_verify", {"strand_path": strand})
            self.assertFalse(v["isError"]); self.assertEqual(v["content"][0]["text"], out.rstrip("\n"))
            with open(strand, encoding="utf-8") as f:
                text = f.read()
            bad = self.call("probbit_live_verify", {"strand": text.replace('"elapsed_hours":6.5', '"elapsed_hours":7')})
            self.assertFalse(bad["isError"]); self.assertEqual(bad["structuredContent"], {"ok": False, "line": 3, "diverges": "the stance differs"})  # an answer
            for args, where in (({"persona_path": path, "seed": 2, "strand_path": strand}, "state"), ({"persona_path": path, "extra": 1}, "arguments.extra"),
                                ({"persona_path": path, "event": {"elapsed_hours": -1}}, "arguments.event.elapsed_hours")):
                e = self.call("probbit_live_event", args)
                self.assertTrue(e["isError"]); self.assertEqual(e["structuredContent"]["error"]["path"], where)

    def test_structured_content_from_2025_06_18_on(self):
        self.legacy("2024-11-05")
        r = self.call("probbit_demo", {"tasks": 3})
        self.assertNotIn("structuredContent", r); json.loads(r["content"][0]["text"])

    def test_modern_requests_carry_their_metadata(self):
        d = self.c.request("server/discover", meta=MODERN)["result"]
        self.assertEqual(d["resultType"], "complete"); self.assertIn("2026-07-28", d["supportedVersions"])
        self.assertEqual(d["_meta"]["io.modelcontextprotocol/serverInfo"]["name"], "probbit"); self.assertIn("tools", d["capabilities"])
        t = self.c.request("tools/list", meta=MODERN)["result"]
        self.assertEqual(t["resultType"], "complete"); self.assertEqual(len(t["tools"]), 10)
        r = self.call("probbit_demo", {"tasks": 4, "seed": 2}, MODERN)
        self.assertEqual(r["resultType"], "complete"); self.assertEqual(len(r["structuredContent"]["tasks"]), 4)
        e = self.c.request("tools/list", meta={"io.modelcontextprotocol/protocolVersion": "1999-01-01", "io.modelcontextprotocol/clientCapabilities": {}})["error"]
        self.assertEqual(e["code"], -32022); self.assertEqual(e["data"]["requested"], "1999-01-01"); self.assertIn("2026-07-28", e["data"]["supported"])
        e = self.c.request("tools/list", meta={"io.modelcontextprotocol/protocolVersion": "2026-07-28"})["error"]
        self.assertEqual(e["code"], -32602)  # clientCapabilities is required
        self.assertEqual(self.c.request("tools/list")["error"]["code"], -32602)  # no _meta and no initialize before it

    def test_framing_batches_and_shutdown(self):
        self.c.send_raw("﻿" + json.dumps({"jsonrpc": "2.0", "id": "a", "method": "ping"}))  # a byte-order mark on the first line
        self.assertEqual(self.c.recv(), {"jsonrpc": "2.0", "id": "a", "result": {}})
        self.c.send_raw("this is not json")
        e = self.c.recv(); self.assertEqual((e["id"], e["error"]["code"]), (None, -32700))
        self.c.send_raw(json.dumps({"jsonrpc": "2.0", "method": "ping"}))  # notification-shaped ping: no answer
        self.c.send_raw(json.dumps([{"jsonrpc": "2.0", "id": 7, "method": "ping"}, {"jsonrpc": "2.0", "method": "notifications/initialized"}]))
        b = self.c.recv(); self.assertEqual(b, [{"jsonrpc": "2.0", "id": 7, "result": {}}])
        self.c.send_raw(json.dumps({"jsonrpc": "1.0", "id": 8, "method": "ping"}))
        self.assertEqual(self.c.recv()["error"]["code"], -32600)
        code, rest, err = self.c.close()
        self.assertEqual(code, 0); self.assertEqual(rest, ""); self.assertIn("ready on stdio", err)


if __name__ == "__main__":
    unittest.main()
