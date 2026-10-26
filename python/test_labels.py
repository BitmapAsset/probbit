"""Tests for the strand label sidecar in python/probbit.py and the `labels=` argument (stdlib unittest; no probbit binary:
run: python3 python/test_labels.py)."""
import json, os, tempfile, time, unittest

import probbit
import probbit_host as ph

NO_BINARY = os.path.join(tempfile.gettempdir(), "no-such-probbit-binary")
STRAND = '{"probbit_strand":1}\n{"n":1}\n'


class Sidecar(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.TemporaryDirectory()
        self.strand = os.path.join(self.d.name, "support.strand")
        with open(self.strand, "w", encoding="utf-8") as f:
            f.write(STRAND)
        self.side = os.path.join(self.d.name, "support.labels.json")

    def tearDown(self):
        self.d.cleanup()

    def read(self, path):
        with open(path, encoding="utf-8", newline="") as f:
            return f.read()

    def test_path_replaces_the_last_extension(self):
        self.assertEqual(probbit.labels_path("strands/support.strand"), "strands/support.labels.json")
        self.assertEqual(probbit.labels_path("foo"), "foo.labels.json")
        self.assertEqual(probbit.labels_path("a.b/life.v2.strand"), "a.b/life.v2.labels.json")
        self.assertEqual(probbit.labels_path("dir.x/foo"), "dir.x/foo.labels.json")

    def test_format_sorted_compact_raw_utf8_one_newline(self):
        out = probbit.write_labels(self.strand, {"session": "#3fa9c2d1", "agent": "día", "host": "my harness 2.1", "lane": "support"})
        text = self.read(self.side)
        self.assertEqual(text, '{"agent":"día","host":"my harness 2.1","lane":"support","session":"#3fa9c2d1"}\n')
        self.assertEqual(text, json.dumps(out, sort_keys=True, separators=(",", ":"), ensure_ascii=False) + "\n")
        self.assertEqual(self.read(self.strand), STRAND, "the strand is not touched")
        self.assertEqual(sorted(os.listdir(self.d.name)), ["support.labels.json", "support.strand"], "no temp file left")

    def test_merge_keeps_keys_not_named_and_skips_an_equal_write(self):
        with open(self.side, "w", encoding="utf-8") as f:
            f.write('{"note":"hand-added","host":"old"}')
        self.assertEqual(probbit.write_labels(self.strand, {"host": "new", "agent": "a"}), {"agent": "a", "host": "new", "note": "hand-added"})
        self.assertEqual(self.read(self.side), '{"agent":"a","host":"new","note":"hand-added"}\n')
        past = time.time() - 3600
        os.utime(self.side, (past, past))
        before = os.stat(self.side).st_mtime_ns
        probbit.write_labels(self.strand, {"host": "new"})
        self.assertEqual(os.stat(self.side).st_mtime_ns, before, "equal content: not rewritten")
        self.assertEqual(self.read(self.strand), STRAND)

    def test_validation(self):
        bad = [{"bad key": "x"}, {"": "x"}, {"k" * 65: "x"}, {"k": ""}, {"k": "x" * 65}, {"k": "a\nb"}, {"k": "a\tb"},
               {"k": "\x7f"}, {"k": 3}, {3: "x"}, {str(i): "x" for i in range(9)}, ["agent", "x"]]
        for labels in bad:
            with self.assertRaises(ValueError, msg=repr(labels)):
                probbit.write_labels(self.strand, labels)
        self.assertFalse(os.path.exists(self.side))
        self.assertEqual(probbit.check_labels({"k": "é" * 64}), {"k": "é" * 64}, "64 characters, not bytes")
        self.assertEqual(len(probbit.check_labels({str(i): "x" for i in range(8)})), 8)

    def test_a_merge_past_8_keys_or_a_broken_sidecar_is_refused_and_left_as_is(self):
        full = json.dumps({c: c for c in "abcdefgh"})
        with open(self.side, "w", encoding="utf-8") as f:
            f.write(full)
        with self.assertRaises(ValueError):
            probbit.write_labels(self.strand, {"agent": "x"})
        self.assertEqual(self.read(self.side), full)
        probbit.write_labels(self.strand, {"a": "replaced"})   # replacing a key is still 8
        for broken in ("[1]", '{"a": 1}', "not json"):
            with open(self.side, "w", encoding="utf-8") as f:
                f.write(broken)
            with self.assertRaises(ValueError):
                probbit.write_labels(self.strand, {"agent": "x"})
            self.assertEqual(self.read(self.side), broken)


class LabelsArgument(unittest.TestCase):
    """Bad labels are refused before the binary is ever run (the binary here does not exist)."""

    def test_stance_host_checks_labels_at_construction(self):
        with tempfile.TemporaryDirectory() as d:
            strand = os.path.join(d, "run.strand")
            with self.assertRaises(ValueError):
                ph.StanceHost("persona.yaml", strand=strand, labels={"host": "a\nb"}, binary=NO_BINARY)
            with self.assertRaises(ValueError):
                ph.StanceHost("persona.yaml", labels={"host": "x"}, binary=NO_BINARY)   # labels need a strand
            h = ph.StanceHost("persona.yaml", strand=strand, labels={"host": "my harness"}, binary=NO_BINARY)
            self.assertEqual(h.labels, {"host": "my harness"})
            self.assertFalse(os.path.exists(os.path.join(d, "run.labels.json")), "nothing written before an event")

    def test_live_event_checks_labels_before_the_event(self):
        with tempfile.TemporaryDirectory() as d:
            strand = os.path.join(d, "run.strand")
            with self.assertRaises(ValueError):
                probbit.live_event({"probbit_persona": 1}, labels={"host": "x"}, binary=NO_BINARY)
            with self.assertRaises(ValueError):
                probbit.live_event({"probbit_persona": 1}, strand=strand, labels={"bad key": "x"}, binary=NO_BINARY)
            with self.assertRaises(probbit.ProbbitError):   # good labels: on to the (missing) binary
                probbit.live_event({"probbit_persona": 1}, strand=strand, labels={"host": "x"}, binary=NO_BINARY)
            self.assertEqual(os.listdir(d), [])


if __name__ == "__main__":
    unittest.main()
