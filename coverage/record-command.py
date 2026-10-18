"""Retain exact invocation, source revision, exit status and raw stdout/stderr."""
import datetime
import hashlib
import json
import pathlib
import subprocess
import sys


def source_snapshot():
    names = subprocess.check_output([
        'git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'
    ]).decode().split('\0')
    return {
        name: hashlib.sha256(pathlib.Path(name).read_bytes()).hexdigest()
        for name in sorted(set(names))
        if name and pathlib.Path(name).is_file()
        and pathlib.Path(name).suffix in ['.rs', '.sh', '.ps1', '.proto', '.toml']
    }


output = pathlib.Path(sys.argv[1])
output.mkdir(parents=True, exist_ok=True)
metadata = {
    'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
    'branch': subprocess.check_output(['git', 'branch', '--show-current'], text=True).strip(),
    'worktree': str(pathlib.Path.cwd()),
    'started_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'command': sys.argv[2:],
    'source_sha256': source_snapshot(),
}
(output / 'command.json').write_text(json.dumps(metadata, indent=2) + '\n')
with (output / 'run.log').open('w') as handle:
    code = subprocess.run(sys.argv[2:], stdout=handle, stderr=subprocess.STDOUT).returncode
metadata.update(
    unit_command_exit_code=code,
    finished_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
    source_sha256_after=source_snapshot(),
)
metadata['source_changes_during_command'] = [
    name for name in sorted(set(metadata['source_sha256']) | set(metadata['source_sha256_after']))
    if metadata['source_sha256'].get(name) != metadata['source_sha256_after'].get(name)
]
(output / 'command.json').write_text(json.dumps(metadata, indent=2) + '\n')
sys.exit(code)
