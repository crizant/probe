#!/usr/bin/env python3
"""Exercise the runner's subprocess protocol without a GPU or a desktop launch."""
import importlib.util
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "startup", Path(__file__).with_name("benchmark-desktop-startup.py"))
startup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(startup)
POPEN = subprocess.Popen


class StartupRunnerTests(unittest.TestCase):
    def run_peer(self, source, timeout=2):
        def launch(_command, **kwargs):
            return POPEN([sys.executable, "-c", source], **kwargs)
        with patch.object(startup.subprocess, "Popen", side_effect=launch):
            return startup.sample(Path("unused"), None, timeout)

    def test_milestones_and_isolated_session(self):
        result = self.run_peer('''
import json, os, pathlib
root = pathlib.Path(os.environ['PROBE_STARTUP_BENCHMARK_DIR'])
assert json.loads((root / 'desktop-session.json').read_text()) == {'schema_version': 2}
for name, elapsed in [('first_frame', 2), ('session_ready', 7)]:
    print('PROBE_STARTUP ' + json.dumps({'milestone': name, 'main_ms': elapsed}), flush=True)
''')
        self.assertEqual(result['session_ready']['main_ms'], 7)
        self.assertGreater(result['session_ready']['launch_observed_ms'], 0)

    def test_failed_restore_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, 'invalid session'):
            self.run_peer('print(\'PROBE_STARTUP {"error": "invalid session"}\', flush=True)')

    def test_uninstrumented_binary_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, 'before both'):
            self.run_peer('print("ordinary app output")')

    def test_hung_process_times_out(self):
        with self.assertRaisesRegex(RuntimeError, 'timed out'):
            self.run_peer('import time; time.sleep(30)', timeout=0.1)

    def test_tail_percentile(self):
        samples = [{'first_frame': {'main_ms': value}} for value in range(1, 11)]
        self.assertEqual(startup.summarize(samples)['first_frame']['main_ms'],
                         {'median_ms': 5.5, 'p90_ms': 9})


if __name__ == '__main__':
    unittest.main()
