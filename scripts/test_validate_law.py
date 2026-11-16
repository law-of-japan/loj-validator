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

    def run_checks(self, codes, stdout=b'{"results": []}'):
        results = [subprocess.CompletedProcess([], code, stdout, b'diagnostic') for code in codes]
        with patch.object(validate_law, 'git', return_value='a' * 40), patch.object(validate_law.subprocess, 'run', side_effect=results) as run:
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
        status, _ = self.run_checks([0] * 4, b'not JSON')
        self.assertEqual(status, 1)

    def test_package_cannot_masquerade_as_repository(self):
        (self.repo / 'history.yaml').unlink()
        with self.assertRaisesRegex(ValueError, 'history.yaml'):
            validate_law.validate(self.repo, self.validator, self.output)

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
            return subprocess.CompletedProcess(command, 0, b'{}', b'')

        with patch.object(validate_law, 'git', side_effect=git), patch.object(validate_law.subprocess, 'run', side_effect=run):
            self.assertEqual(validate_law.validate(self.repo, self.validator, self.output, base), 0)
        self.assertEqual(len(calls), 5)
        self.assertEqual(len(bundles), 1)
        self.assertFalse(bundles[0].exists())
        self.assertNotIn('loj-ci-validation', real_git(self.repo, 'show-ref'))


if __name__ == '__main__':
    unittest.main()
