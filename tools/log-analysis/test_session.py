"""Tests for session.py. Run: python3 -m unittest discover -s tools/log-analysis"""
import os
import tempfile
import time
import unittest

import session


class PickTest(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp()
        self.saved = session.ROOT
        session.ROOT = self.root
        # Oldest first, so that the last one is the most recent.
        for k, project in enumerate(("-work-app", "-work-tool")):
            os.makedirs(os.path.join(self.root, project))
            path = os.path.join(self.root, project, "s.jsonl")
            open(path, "w").close()
            os.utime(path, (time.time() + k, time.time() + k))

    def tearDown(self):
        session.ROOT = self.saved
        for project in ("-work-app", "-work-tool"):
            os.remove(os.path.join(self.root, project, "s.jsonl"))
            os.rmdir(os.path.join(self.root, project))
        os.rmdir(self.root)

    def test_no_project_is_left_out_unless_asked(self):
        self.assertIn("-work-tool", session.pick(None))
        self.assertIn("-work-app", session.pick(None, ["tool"]))
        self.assertIn("-work-tool", session.pick("tool", ["tool"]))


if __name__ == "__main__":
    unittest.main()
