"""Run independent law-repository checks without executing code from the law repo."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def git(repository, *args):
    return subprocess.check_output(
        ['git', '-C', str(repository), *args], text=True
    ).strip()


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')


def validate(repository, validator, output, change_base=''):
    # Keep Git history outside the artifact directory even on interruption.
    with tempfile.TemporaryDirectory(prefix='loj-ci-') as scratch:
        return _validate(repository, validator, output, change_base, Path(scratch))


def _validate(repository, validator, output, change_base, scratch):
    repository, validator, output = (Path(p).resolve() for p in (repository, validator, output))
    output.mkdir(parents=True, exist_ok=True)
    if not (repository / 'history.yaml').is_file():
        raise ValueError('A law repository with history.yaml is required')
    head = git(repository, 'rev-parse', 'HEAD')
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
        if not re.fullmatch('[0-9a-f]{40}', change_base):
            raise ValueError('change-base must be a full commit SHA')
        # Never compare the synthetic PR merge commit. Export only the checked head
        # and its ancestors under a branch ref, as required by the bundle reader.
        subprocess.run(['git', '-C', str(repository), 'merge-base', '--is-ancestor', change_base, head], check=True)
        if change_base == head:
            raise ValueError('change-base must precede the checked head')
        bundle = scratch / 'change.bundle'
        ref = 'refs/heads/loj-ci-validation'
        previous = subprocess.run(['git', '-C', str(repository), 'show-ref', '--verify', '--quiet', ref])
        if previous.returncode != 1:
            raise ValueError('Temporary validation ref already exists or cannot be checked')
        git(repository, 'update-ref', ref, head)
        try:
            git(repository, 'bundle', 'create', str(bundle), ref)
        finally:
            git(repository, 'update-ref', '-d', ref)
        jobs.append(('change', [str(bundle), '--base', change_base, '--head', head]))
    summary = []
    for name, args in jobs:
        command = [str(executable), 'check', *args, '--format', 'json']
        result = subprocess.run(command, capture_output=True)
        (output / f'{name}.json').write_bytes(result.stdout)
        (output / f'{name}.stderr.txt').write_bytes(result.stderr)
        # A malformed or empty report is an execution failure, never a pass.
        try:
            json.loads(result.stdout)
            valid_json = True
        except (ValueError, UnicodeError):
            valid_json = False
        summary.append({'check': name, 'command': command, 'exitCode': result.returncode, 'validJson': valid_json})
        print(f'{name}: exit {result.returncode}, JSON={valid_json}', flush=True)
    write_json(output / 'summary.json', summary)
    return 0 if all(item['exitCode'] == 0 and item['validJson'] for item in summary) else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repository', required=True)
    parser.add_argument('--validator', required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--change-base', default='')
    args = parser.parse_args()
    raise SystemExit(validate(args.repository, args.validator, args.output, args.change_base))
