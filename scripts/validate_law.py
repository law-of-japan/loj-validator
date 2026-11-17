"""Run independent law-repository checks without executing code from the law repo."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

CHECK_TIMEOUT_SECONDS = 120
MAX_REPORT_BYTES = 16 * 1024 * 1024


def valid_report(report, exit_code):
    if not isinstance(report, dict):
        return False
    if exit_code != 0 and isinstance(report.get('error'), dict):
        return True
    results = report.get('results')
    return isinstance(results, list) and bool(results) and all(
        isinstance(item, dict)
        and isinstance(item.get('check'), str) and bool(item['check'])
        and item.get('result') in ('pass', 'fail', 'incomplete', 'notApplicable')
        for item in results
    )


def git_run(repository, *args, check=True):
    env = {key: value for key, value in os.environ.items() if not key.upper().startswith('GIT_')}
    env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull, GIT_TERMINAL_PROMPT='0')
    return subprocess.run(
        ['git', '-c', f'core.hooksPath={os.devnull}', '-c', 'gc.auto=0',
         '-C', str(repository), *args],
        text=True, capture_output=True, check=check, env=env, timeout=120,
    )


def git(repository, *args):
    return git_run(repository, *args).stdout.strip()


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')


def validate(repository, validator, output, change_base=''):
    output = Path(output).resolve()
    summary = []
    try:
        output.mkdir(parents=True, exist_ok=True)
        if change_base and not re.fullmatch('[0-9a-f]{40}', change_base):
            raise ValueError('change-base must be a full commit SHA')
        # Keep Git history outside the artifact directory even on interruption.
        with tempfile.TemporaryDirectory(prefix='loj-ci-') as scratch:
            status = _validate(repository, validator, output, change_base, Path(scratch), summary)
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        status = 2 if isinstance(error, ValueError) else 3
        kind = 'configuration' if status == 2 else 'incomplete'
        message = str(error)
        print(f'{kind}: {message}', file=sys.stderr)
        summary.append({'check': 'runner', 'exitCode': status, 'error': {'kind': kind, 'message': message}})
    try:
        write_json(output / 'summary.json', summary)
    except OSError as error:
        print(f'incomplete: cannot write summary: {error}', file=sys.stderr)
        return 3
    return status


def _validate(repository, validator, output, change_base, scratch, summary):
    repository, validator, output = (Path(p).resolve() for p in (repository, validator, output))
    output.mkdir(parents=True, exist_ok=True)
    if not (repository / 'history.yaml').is_file():
        raise ValueError('A law repository with history.yaml is required')
    head = git(repository, 'rev-parse', 'HEAD')
    if change_base == head:
        raise ValueError('change-base must precede the checked head')
    if change_base:
        ancestor = git_run(repository, 'merge-base', '--is-ancestor', change_base, head, check=False)
        if ancestor.returncode == 1:
            raise ValueError('change-base must be an ancestor of the checked head')
        ancestor.check_returncode()
    executable = validator / 'target/release' / ('loj-validator.exe' if os.name == 'nt' else 'loj-validator')
    provenance = {
        'repositoryCommit': head,
        'validatorCommit': git(validator, 'rev-parse', 'HEAD'),
        'specCommit': (validator / 'vendor/loj-spec/COMMIT').read_text().strip(),
        'binarySha256': hashlib.sha256(executable.read_bytes()).hexdigest(),
        'note': 'Directory reports are not canonical-commit certification. L2 and L4 are not checked.',
    }
    write_json(output / 'provenance.json', provenance)
    jobs = [(level, [str(repository), '--level', level]) for level in ('L0', 'L1', 'L3')]
    jobs.append(('rebuild', [str(repository), '--check', 'rebuild']))
    if change_base:
        # Never compare the synthetic PR merge commit. Export only the checked head
        # and its ancestors under a branch ref, as required by the bundle reader.
        bundle = scratch / 'change.bundle'
        ref = 'refs/heads/loj-ci-validation'
        previous = git_run(repository, 'show-ref', '--verify', '--quiet', ref, check=False)
        if previous.returncode == 0:
            raise ValueError('Temporary validation ref already exists')
        if previous.returncode != 1:
            previous.check_returncode()
        git(repository, 'update-ref', ref, head, '0' * 40)
        try:
            git(repository, 'bundle', 'create', str(bundle), ref)
        finally:
            original_error = sys.exc_info()[1]
            try:
                git(repository, 'update-ref', '-d', ref, head)
            except (OSError, subprocess.SubprocessError) as error:
                if original_error is None:
                    raise
                print(f'incomplete: temporary ref cleanup also failed: {error}', file=sys.stderr)
        jobs.append(('change', [str(bundle), '--base', change_base, '--head', head]))
    for name, args in jobs:
        command = [str(executable), 'check', *args, '--format', 'json']
        report_path = output / f'{name}.json'
        try:
            # Stream to disk; only bounded reports are subsequently parsed in memory.
            with report_path.open('wb') as stdout, (output / f'{name}.stderr.txt').open('wb') as stderr:
                result = subprocess.run(command, stdout=stdout, stderr=stderr, timeout=CHECK_TIMEOUT_SECONDS)
        except (OSError, subprocess.TimeoutExpired) as error:
            message = str(error)
            write_json(output / f'{name}.json', {'error': {'kind': 'incomplete', 'message': message}})
            summary.append({'check': name, 'command': command, 'exitCode': 3, 'error': {'kind': 'incomplete', 'message': message}})
            print(f'{name}: incomplete: {message}', file=sys.stderr)
            continue
        # A malformed or empty report is an execution failure, never a pass.
        try:
            if report_path.stat().st_size > MAX_REPORT_BYTES:
                summary.append({'check': name, 'command': command, 'exitCode': 3,
                                'validatorExitCode': result.returncode,
                                'error': {'kind': 'incomplete', 'message': 'Report exceeds 16 MiB parsing limit'}})
                print(f'{name}: incomplete: report exceeds parsing limit', file=sys.stderr)
                continue
            valid_json = valid_report(json.loads(report_path.read_bytes()), result.returncode)
        except (ValueError, UnicodeError):
            valid_json = False
        summary.append({'check': name, 'command': command, 'exitCode': result.returncode, 'validJson': valid_json})
        print(f'{name}: exit {result.returncode}, JSON={valid_json}', flush=True)
    return 0 if all(item['exitCode'] == 0 and item.get('validJson', False) for item in summary) else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repository', required=True)
    parser.add_argument('--validator', required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--change-base', default='')
    args = parser.parse_args()
    raise SystemExit(validate(args.repository, args.validator, args.output, args.change_base))
