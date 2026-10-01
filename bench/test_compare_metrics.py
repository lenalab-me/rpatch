import json
import tempfile
import unittest
from pathlib import Path

from compare_metrics import classify, summarize, summarize_trace


class ComparisonMetricsTests(unittest.TestCase):
    def test_discovery_and_dry_run_are_separate_from_edits(self):
        self.assertEqual(classify('Bash', {'command': 'rpatch --help'}), ('rpatch_cli_discovery', False))
        self.assertEqual(classify('Bash', {'command': "rpatch --diff --dry-run <<'EOF'\nx\nEOF"}), ('rpatch_cli', True))
        self.assertEqual(classify('Bash', {'command': 'cat edits | /tmp/bin/rpatch --diff'}), ('rpatch_cli', False))
        self.assertEqual(classify('mcp__rpatch__rpatch', {'input': 'x', 'dry_run': True}), ('rpatch_mcp', True))

    def write_run(self, root, arm, events, code=0):
        directory = root / 'trial-1' / arm
        directory.mkdir(parents=True)
        (directory / 'task.jsonl').write_text('\n'.join(json.dumps(item) for item in events))
        (directory / 'task.run.json').write_text(json.dumps({
            'runner_exit_code': code, 'verify_exit_code': 0,
        }))

    def test_rate_limit_with_success_subtype_is_excluded(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for arm in ('cli', 'mcp'):
                self.write_run(root, arm, [
                    {'type': 'result', 'subtype': 'success', 'is_error': True,
                     'terminal_reason': 'api_error', 'api_error_status': 429},
                ], code=1)
            summary = summarize(root)
            self.assertTrue(all(not row['eligible'] for row in summary['rows']))
            self.assertEqual(summary['paired_comparison']['mcp']['paired_eligible_runs'], 0)
            self.assertEqual(summary['rows'][0]['api_error_status'], 429)

    def test_connected_mcp_pair_counts_edits_and_errors(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for arm, tool in [('cli', 'Edit'), ('mcp', 'mcp__rpatch__rpatch')]:
                self.write_run(root, arm, [
                    {'type': 'system', 'subtype': 'init', 'mcp_servers':
                     [{'name': 'rpatch', 'status': 'connected'}] if arm == 'mcp' else []},
                    {'type': 'assistant', 'message': {'content': [
                        {'type': 'tool_use', 'id': 'edit-1', 'name': tool, 'input': {'input': 'x'}},
                    ]}},
                    {'type': 'user', 'message': {'content': [
                        {'type': 'tool_result', 'tool_use_id': 'edit-1', 'is_error': False,
                         'content': json.dumps({'isError': arm == 'mcp'})},
                    ]}},
                    {'type': 'result', 'subtype': 'success', 'is_error': False},
                ])
            summary = summarize(root)
            self.assertEqual(summary['paired_comparison']['cli']['rpatch_share'], 0)
            self.assertEqual(summary['paired_comparison']['mcp']['rpatch_share'], 1)
            self.assertEqual(summary['paired_comparison']['mcp']['paired_eligible_runs'], 1)
            mcp = next(row for row in summary['rows'] if row['arm'] == 'mcp')
            self.assertEqual(mcp['counts']['rpatch_mcp_failed'], 1)


class CodexMetricsTests(unittest.TestCase):
    def test_shell_wrapper_is_unwrapped(self):
        self.assertEqual(classify('Bash', {'command': "/bin/zsh -c 'rpatch --help'"}), ('rpatch_cli_discovery', False))
        self.assertEqual(classify('Bash', {'command': "/bin/zsh -c 'rpatch --diff < edits.json'"}), ('rpatch_cli', False))

    def test_codex_domain_error_and_dry_run_are_counted(self):
        events = [
            {'type': 'item.completed', 'item': {
                'id': 'one', 'type': 'mcp_tool_call', 'server': 'rpatch', 'tool': 'rpatch',
                'arguments': {'input': 'x', 'dry_run': True}, 'status': 'completed',
                'result': {'content': [{'type': 'text', 'text': '(dry run: nothing written)\n'}], 'structured_content': None}, 'error': None,
            }},
            {'type': 'item.completed', 'item': {
                'id': 'two', 'type': 'mcp_tool_call', 'server': 'rpatch', 'tool': 'rpatch',
                'arguments': {'input': 'x'}, 'status': 'failed',
                'result': {'content': [{'type': 'text', 'text': 'rpatch: nothing written\n'}], 'structured_content': None}, 'error': None,
            }},
            {'type': 'item.completed', 'item': {
                'id': 'three', 'type': 'file_change', 'changes': [{'path': 'a.txt'}], 'status': 'completed',
            }},
            {'type': 'turn.completed', 'usage': {'input_tokens': 100, 'output_tokens': 20}},
        ]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'events.jsonl'
            path.write_text('\n'.join(json.dumps(event) for event in events))
            result = summarize_trace(path)
        self.assertTrue(result['complete'])
        self.assertEqual(result['counts']['rpatch_mcp_dry_run'], 1)
        self.assertEqual(result['counts']['rpatch_mcp_failed'], 1)
        self.assertEqual(result['counts']['native_edit'], 1)
        self.assertEqual(result['editing_calls'], 2)
        self.assertEqual(result['usage']['input_tokens'], 100)


if __name__ == '__main__':
    unittest.main()
