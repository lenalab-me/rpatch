"""Tests for plugin/hooks/edit_guard.py. Run: python3 -m unittest discover -s tools/hooks"""
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest

HOOK = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "plugin", "hooks", "edit_guard.py")
spec = importlib.util.spec_from_file_location("edit_guard", HOOK)
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


def run_hook(payload, env=None):
    r = subprocess.run([sys.executable, HOOK], input=payload, capture_output=True, text=True,
                       env={**os.environ, **(env or {})}, timeout=20)
    return r.returncode, r.stdout


class EditGuardTest(unittest.TestCase):
    TRACKED = ("tracked.txt", "sub/inner.txt", "sub/status.txt", ".gitignore", "p", "script.sed", "a[b].txt")

    @classmethod
    def setUpClass(cls):
        # A repo outside the temp dirs the hook lets through, as real projects are.
        cls.repo = os.path.realpath(tempfile.mkdtemp(dir=os.path.expanduser("~"), prefix=".edit-guard-test-"))
        try:
            for name in cls.TRACKED + ("untracked.txt", "sub/only-in-sub.txt"):
                path = os.path.join(cls.repo, name)
                os.makedirs(os.path.dirname(path), exist_ok=True)
                with open(path, "w") as f:
                    f.write("a\n")
            for args in (["init", "-q"], ["add", "--", *cls.TRACKED]):
                subprocess.run(["git", "-C", cls.repo, *args], check=True)
        except BaseException:
            subprocess.run(["rm", "-rf", cls.repo])
            raise

    @classmethod
    def tearDownClass(cls):
        subprocess.run(["rm", "-rf", cls.repo], check=True)

    def rule(self, command):
        verdict = guard.decide(command, self.repo, guard.Files())
        return verdict[0] if verdict else None

    def test_python_replace(self):
        self.assertEqual(self.rule("python3 - <<'EOF'\ns=open('t').read()\nopen('t','w').write(s.replace('a','b'))\nEOF"),
                         "python-replace")
        self.assertIsNone(self.rule("python3 - <<'EOF'\nprint(open('t').read().replace('a','b'))\nEOF"))

    def test_sed_in_place_on_tracked_files(self):
        for command in ("sed -i 's/a/b/' tracked.txt", "sed -i '' 's/a/b/' tracked.txt",
                        "sed -E -i 's/a/b/' tracked.txt", "sed -i.bak -e 's/a/b/' tracked.txt",
                        "cd sub && sed -i 's/a/b/' inner.txt", "perl -pi -e 's/a/b/' tracked.txt",
                        "npm test && sed -i 's/a/b/' tracked.txt"):
            self.assertEqual(self.rule(command), "sed-in-place", command)

    def test_sed_that_is_not_in_place_or_not_on_tracked_files(self):
        for command in ("sed -n 1,5p tracked.txt", "sed 's/a/b/' tracked.txt > /tmp/out",
                        "sed -i 's/a/b/' untracked.txt", "sed -i 's/a/b/' new.txt",
                        "sed -i 's/a/b/' /tmp/x.txt", "sed -i 's/a/b/' \"$f\"", "sed -i 's/a/b/' *.txt",
                        "cd sub && sed -i 's/a/b/' tracked.txt", "perl -ne 'print' tracked.txt"):
            self.assertIsNone(self.rule(command), command)

    def test_overwrite_of_tracked_files(self):
        for command in ("cat > tracked.txt <<'EOF'\nx\nEOF", "cat <<'EOF' > tracked.txt\nx\nEOF",
                        "echo x > tracked.txt", "printf 'x\\n' > .gitignore", "echo x 1> tracked.txt",
                        "ls | tee tracked.txt", "cat > sub/inner.txt <<'EOF'\nx\nEOF",
                        "FOO=1 cat > tracked.txt <<'EOF'\nx\nEOF"):
            self.assertEqual(self.rule(command), "overwrite-tracked", command)

    def test_writes_that_pass(self):
        for command in ("cat >> tracked.txt <<'EOF'\nx\nEOF", "cat >| tracked.txt <<'EOF'\nx\nEOF",
                        "cat > new.txt <<'EOF'\nx\nEOF", "cat > untracked.txt <<'EOF'\nx\nEOF",
                        "echo x 2> tracked.txt", "ls | tee -a tracked.txt", "ls | tee /dev/null",
                        "cat > /tmp/t.txt <<'EOF'\nx\nEOF", "cat > \"$T/tracked.txt\" <<'EOF'\nx\nEOF",
                        "jq . tracked.txt > out.json", "cat tracked.txt > /dev/null"):
            self.assertIsNone(self.rule(command), command)

    def test_text_inside_quotes_and_bodies_is_not_a_command(self):
        for command in ("git commit -m \"cat > tracked.txt; sed -i s/a/b/ tracked.txt\"",
                        "cat > notes.md <<'EOF'\nsed -i 's/a/b/' tracked.txt\necho x > tracked.txt\nEOF",
                        "echo 'echo x > tracked.txt'", "# cat > tracked.txt",
                        "rpatch <<'EOF'\n=== notes.md\n<<<<<<< OLD\ncat > tracked.txt\n=======\nx\n>>>>>>> NEW\nEOF",
                        "cat <<< 'x' > /tmp/y"):
            self.assertIsNone(self.rule(command), command)

    # Shapes the 2026-10-07 interrogate review reproduced (A: Fable 5.1, B: GPT-6.1-Sol, C: Grok 4.7).

    def test_review_misses_are_blocked(self):
        cases = {
            "sed-in-place": [
                "sed -i \\\n  's/a/b/' tracked.txt",                    # A2: continuation
                "sed -i 's/a/b/' \\\n  tracked.txt",                    # A2
                "sed -i 's/a/b/\ns/c/d/' tracked.txt",                 # A2: newline in quotes
                "perl -pi -e's/a/b/' tracked.txt",                     # B4, C8: attached -e
                "perl -i -p filter.pl tracked.txt",                    # B4: script file
                "perl -pi -E 'say' tracked.txt",                       # A3, C8: -E
                "if sed -i 's/a/b/' tracked.txt; then :; fi",          # A9, C6
                "{ sed -i 's/a/b/' tracked.txt; }",                    # A9, C6
                "timeout 5 sed -i 's/a/b/' tracked.txt",               # A9
                "(cd sub && ls); sed -i 's/a/b/' tracked.txt",         # A4, B1: cd ends with the subshell
                "cd -P sub && sed -i 's/a/b/' inner.txt",              # C2: cd options
            ],
            "overwrite-tracked": [
                "echo \"line1\nline2\" > tracked.txt",                  # A2: newline in quotes
                "printf 'a\nb\n' > tracked.txt",                        # A2
                "printf x \\\n> tracked.txt",                           # C5: continuation
                "if printf x > tracked.txt; then :; fi",               # C6
                "sudo -u root printf x > tracked.txt",                 # C6: wrapper options
                "env -i printf x > tracked.txt",                       # C6
                "command -p printf x > tracked.txt",                   # C6
                "printf x &> tracked.txt",                             # C7
                "printf x >& tracked.txt",                             # C7
                "echo $(date) > tracked.txt",                          # A9
                "cat >tracked.txt<<'EOF'\nx\nEOF",                     # A9: no spaces
                "printf x > \"a[b].txt\"",                              # C4: brackets in quotes are literal
                "(cd /tmp); printf x > tracked.txt",                   # B1, C2
            ],
        }
        for rule, commands in cases.items():
            for command in commands:
                self.assertEqual(self.rule(command), rule, command)

    def test_review_false_positives_pass(self):
        for command in (
            "sed -i '' p untracked.txt",                                # B3, C1: the program is not a file
            "sed -i script.sed /tmp/new.txt",                           # C1
            "sed -fscript.sed tracked.txt",                             # A3: -f, not -i
            "perl -Mstrict -ne 'print if /a/' tracked.txt",             # A3: -M is not -i
            "perl -Ilib -ne 'print' tracked.txt",                       # A3
            "perl -MList::Util=sum -lane 'print sum(@F)' tracked.txt",  # A3
            "(cd sub); echo x > inner.txt",                             # B1: cd ended with the subshell
            "cd sub | cat; echo x > inner.txt",                         # B1: cd in a pipeline
            "(cd sub && ls) && echo done > status.txt",                 # A4
            "(cd sub); sed -i 's/a/b/' only-in-sub.txt",                # C2
            "cat <<123 > new.txt\necho x > tracked.txt\n123",           # B2: digit delimiter
            "cat <<'END TEXT' > new.txt\necho x > tracked.txt\nEND TEXT",  # B2
            "cat <<\\EOF > new.txt\necho x > tracked.txt\nEOF",         # B2, A5
            "cat > new.sh <<'EOF'\ncd\nEOF\necho x > status.txt",       # B2, A2: a body line `cd`
            "git commit -m \"$(cat <<'EOF'\nUse 5\" margin\n\nsed -i 's/a/b/' tracked.txt was wrong\nEOF\n)\"",  # A5
            "ssh host \\\n  sed -i 's/a/b/' tracked.txt",                # A2: a remote command
            "echo x > '~/tracked.txt'",                                  # B5, C3: a quoted ~ is literal
            "ls | tee /tmp/out.log < tracked.txt",                      # A6: < is input
            "cd /nonexistent-dir-for-test || printf x > status.txt",    # C2: dir unknown
            "pushd sub && popd && printf x > tracked.txt",              # C2: after popd the dir is unknown
        ):
            self.assertIsNone(self.rule(command), command)

    def test_slow_input_stays_fast(self):
        import time
        command = "cat > notes.tex <<'EOF'\nHe wrote \"" + "\\alpha" * 40 + "\nEOF\necho \"" + "\\x" * 40
        start = time.monotonic()
        self.rule(command)
        self.assertLess(time.monotonic() - start, 1.0)

    def test_temp_roots_need_a_separator(self):
        self.assertTrue(guard.is_temp("/tmp/x"))
        self.assertFalse(guard.is_temp("/tmp-backup/x"))

    def test_plugin_version_follows_the_crate(self):
        root = os.path.join(os.path.dirname(HOOK), "..", "..")
        with open(os.path.join(root, "Cargo.toml")) as f:
            crate = next(l.split('"')[1] for l in f if l.startswith("version"))
        with open(os.path.join(root, "plugin", ".claude-plugin", "plugin.json")) as f:
            self.assertEqual(json.load(f)["version"], crate)

    def test_hook_output_and_reason(self):
        payload = json.dumps({"tool_name": "Bash", "cwd": self.repo,
                              "tool_input": {"command": "sed -i 's/a/b/' tracked.txt"}})
        code, out = run_hook(payload)
        self.assertEqual(code, 0)
        decision = json.loads(out)["hookSpecificOutput"]
        self.assertEqual(decision["permissionDecision"], "deny")
        self.assertTrue(decision["permissionDecisionReason"].startswith("Blocked by a hook: `sed -i` edits tracked.txt"))
        self.assertTrue(decision["permissionDecisionReason"].endswith("[edit-guard: sed-in-place]"))

    def test_off_switch_and_errors_let_commands_through(self):
        payload = json.dumps({"tool_name": "Bash", "cwd": self.repo,
                              "tool_input": {"command": "sed -i 's/a/b/' tracked.txt"}})
        self.assertEqual(run_hook(payload, {"RPATCH_GUARD": "off"}), (0, ""))
        self.assertEqual(run_hook("not json"), (0, ""))
        self.assertEqual(run_hook(json.dumps({"tool_input": None})), (0, ""))


if __name__ == "__main__":
    unittest.main()
