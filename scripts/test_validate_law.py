import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import validate_law


class ValidationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / 'law'
        self.validator = self.root / 'validator'
        self.output = self.root / 'reports'
        self.repo.mkdir()
        (self.repo / 'history.yaml').write_text('synthetic fixture')
        binary = self.validator / 'target/release' / ('loj-validator.exe' if os.name == 'nt' else 'loj-validator')
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b'synthetic executable')
        spec = self.validator / 'vendor/loj-spec/COMMIT'
        spec.parent.mkdir(parents=True)
        spec.write_text('b' * 40)

    def run_checks(self, codes, stdout=b'{"results": [{"check": "L0", "result": "pass"}]}'):
        codes = iter(codes)

        def execute(command, **kwargs):
            kwargs['stdout'].write(stdout)
            kwargs['stderr'].write(b'diagnostic')
            return subprocess.CompletedProcess(command, next(codes))

        with patch.object(validate_law, 'git', return_value='a' * 40), patch.object(validate_law.subprocess, 'run', side_effect=execute) as run:
            status = validate_law.validate(self.repo, self.validator, self.output)
        return status, run

    def test_all_independent_checks_and_provenance(self):
        status, run = self.run_checks([0, 0, 0, 0])
        self.assertEqual(status, 0)
        self.assertEqual(run.call_count, 4)
        summary = json.loads((self.output / 'summary.json').read_text())
        self.assertEqual([item['check'] for item in summary], ['L0', 'L1', 'L3', 'rebuild'])
        provenance = json.loads((self.output / 'provenance.json').read_text())
        self.assertEqual(provenance['repositoryCommit'], 'a' * 40)
        self.assertEqual(provenance['specCommit'], 'b' * 40)
        self.assertEqual((self.output / 'L3.stderr.txt').read_bytes(), b'diagnostic')

    def test_failure_error_and_incomplete_do_not_skip_other_checks(self):
        for code in (1, 2, 3):
            with self.subTest(code=code):
                status, run = self.run_checks([code, 0, 0, 0])
                self.assertEqual(status, 1)
                self.assertEqual(run.call_count, 4)

    def test_malformed_report_cannot_pass(self):
        for code in (0, 1, 2, 3):
            for stdout in (b'not JSON', b'[]', b'null', b'{}', b'{"results": []}', b'{"results": [{}]}'):
                with self.subTest(code=code, stdout=stdout):
                    status, _ = self.run_checks([code] * 4, stdout)
                    self.assertEqual(status, 1)

    def test_package_cannot_masquerade_as_repository(self):
        (self.repo / 'history.yaml').unlink()
        self.assertEqual(validate_law.validate(self.repo, self.validator, self.output), 2)
        self.assert_runner_error('configuration', 'history.yaml')

    def assert_runner_error(self, kind, message):
        summary = json.loads((self.output / 'summary.json').read_text())
        self.assertEqual(summary[-1]['error']['kind'], kind)
        self.assertIn(message, summary[-1]['error']['message'])

    def test_invalid_base_is_rejected_before_git_or_provenance(self):
        with patch.object(validate_law, 'git') as git:
            self.assertEqual(validate_law.validate(self.repo, self.validator, self.output, 'main'), 2)
            git.assert_not_called()
        self.assertFalse((self.output / 'provenance.json').exists())
        self.assert_runner_error('configuration', 'full commit SHA')

    def test_equal_base_is_rejected_before_ancestry_check(self):
        with patch.object(validate_law, 'git', return_value='a' * 40), patch.object(validate_law, 'git_run') as run:
            self.assertEqual(validate_law.validate(self.repo, self.validator, self.output, 'a' * 40), 2)
            run.assert_not_called()
        self.assert_runner_error('configuration', 'precede')

    def test_nonancestor_and_git_failure_are_distinct(self):
        for code, expected, kind in [(1, 2, 'configuration'), (128, 3, 'incomplete')]:
            with self.subTest(code=code), patch.object(validate_law, 'git', return_value='a' * 40), patch.object(validate_law, 'git_run', return_value=subprocess.CompletedProcess(['git'], code)):
                self.assertEqual(validate_law.validate(self.repo, self.validator, self.output, 'b' * 40), expected)
            self.assert_runner_error(kind, 'ancestor' if code == 1 else '128')

    def test_existing_temporary_ref_is_preserved(self):
        with patch.object(validate_law, 'git', return_value='a' * 40) as git, patch.object(validate_law, 'git_run', return_value=subprocess.CompletedProcess(['git'], 0)):
            self.assertEqual(validate_law.validate(self.repo, self.validator, self.output, 'b' * 40), 2)
            self.assertFalse(any('update-ref' in call.args for call in git.call_args_list))
        self.assert_runner_error('configuration', 'already exists')

    def test_missing_binary_is_incomplete(self):
        for binary in (self.validator / 'target/release').iterdir():
            binary.unlink()
        with patch.object(validate_law, 'git', return_value='a' * 40):
            self.assertEqual(validate_law.validate(self.repo, self.validator, self.output), 3)
        self.assert_runner_error('incomplete', 'loj-validator')

    def test_spawn_failure_preserves_other_checks(self):
        for error in (FileNotFoundError('binary disappeared'), subprocess.TimeoutExpired('validator', 120)):
            with self.subTest(error=error), patch.object(validate_law, 'git', return_value='a' * 40), patch.object(validate_law.subprocess, 'run', side_effect=error) as run:
                self.assertEqual(validate_law.validate(self.repo, self.validator, self.output), 1)
                self.assertEqual(run.call_count, 4)
            summary = json.loads((self.output / 'summary.json').read_text())
            self.assertEqual(summary[0]['exitCode'], 3)
            self.assertEqual(summary[0]['error']['kind'], 'incomplete')

    def test_large_report_is_incomplete(self):
        with patch.object(validate_law, 'MAX_REPORT_BYTES', 1):
            self.assertEqual(self.run_checks([0] * 4)[0], 1)
        summary = json.loads((self.output / 'summary.json').read_text())
        self.assertEqual(summary[0]['exitCode'], 3)
        self.assertEqual(summary[0]['validatorExitCode'], 0)

    def test_cleanup_failure_does_not_hide_bundle_failure(self):
        def git(repo, *args):
            if args[0] == 'bundle':
                raise OSError('bundle failed')
            if args[:2] == ('update-ref', '-d'):
                raise OSError('cleanup failed')
            return 'a' * 40

        results = [subprocess.CompletedProcess(['git'], 0), subprocess.CompletedProcess(['git'], 1)]
        with patch.object(validate_law, 'git', side_effect=git), patch.object(validate_law, 'git_run', side_effect=results):
            self.assertEqual(validate_law.validate(self.repo, self.validator, self.output, 'b' * 40), 3)
        self.assert_runner_error('incomplete', 'bundle failed')

    def test_git_environment_is_isolated(self):
        with patch.dict(os.environ, {'GIT_DIR': 'untrusted', 'GIT_CONFIG_COUNT': '1'}), patch.object(validate_law.subprocess, 'run') as run:
            validate_law.git_run(self.repo, 'rev-parse', 'HEAD')
        env = run.call_args.kwargs['env']
        self.assertNotIn('GIT_DIR', env)
        self.assertNotIn('GIT_CONFIG_COUNT', env)
        self.assertEqual(env['GIT_CONFIG_NOSYSTEM'], '1')
        self.assertEqual(env['GIT_TERMINAL_PROMPT'], '0')
        self.assertEqual(env['GIT_CONFIG_GLOBAL'], os.devnull)
        self.assertIn(f'core.hooksPath={os.devnull}', run.call_args.args[0])

    def test_change_bundle_contains_detached_head_and_is_removed(self):
        subprocess.run(['git', 'init', str(self.repo)], check=True, capture_output=True)
        for key, value in [('user.name', 'Test'), ('user.email', 'test@example.invalid')]:
            validate_law.git(self.repo, 'config', key, value)
        validate_law.git(self.repo, 'add', '.')
        validate_law.git(self.repo, 'commit', '-m', 'base')
        base = validate_law.git(self.repo, 'rev-parse', 'HEAD')
        validate_law.git(self.repo, 'checkout', '--detach')
        (self.repo / 'history.yaml').write_text('changed synthetic fixture')
        validate_law.git(self.repo, 'commit', '-am', 'head')
        head = validate_law.git(self.repo, 'rev-parse', 'HEAD')
        real_git, real_run = validate_law.git, subprocess.run
        calls = []
        bundles = []

        def git(repo, *args):
            return 'c' * 40 if repo == self.validator else real_git(repo, *args)

        def run(command, **kwargs):
            if command[0] == 'git':
                return real_run(command, **kwargs)
            calls.append(command)
            if '--base' in command:
                bundle = Path(command[2])
                bundles.append(bundle)
                self.assertNotEqual(bundle.parent, self.output)
                refs = real_git(self.repo, 'bundle', 'list-heads', str(bundle))
                self.assertIn(head + ' refs/heads/loj-ci-validation', refs)
                self.assertEqual(command[command.index('--base') + 1], base)
                self.assertEqual(command[command.index('--head') + 1], head)
            kwargs['stdout'].write(b'{"results": [{"check": "L0", "result": "pass"}]}')
            return subprocess.CompletedProcess(command, 0)

        with patch.object(validate_law, 'git', side_effect=git), patch.object(validate_law.subprocess, 'run', side_effect=run):
            self.assertEqual(validate_law.validate(self.repo, self.validator, self.output, base), 0)
        self.assertEqual(len(calls), 5)
        self.assertEqual(len(bundles), 1)
        self.assertFalse(bundles[0].exists())
        self.assertNotIn('loj-ci-validation', real_git(self.repo, 'show-ref'))


if __name__ == '__main__':
    unittest.main()
