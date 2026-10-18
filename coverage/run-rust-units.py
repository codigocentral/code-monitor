"""Execute reviewed unit tests only; retain raw harness output and LLVM data.

Host/system/service integration tests stay inventoried and are never used to
increase unit coverage. Their production files remain in the LLVM objects.
"""
import datetime
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import xml.etree.ElementTree as ET

out = Path(sys.argv[1]).resolve()
out.mkdir(parents=True, exist_ok=True)
compile_log = Path(sys.argv[2])
artifacts = []
for line in compile_log.read_text().splitlines():
    try:
        item = json.loads(line)
    except json.JSONDecodeError:
        continue
    if item.get('reason') == 'compiler-artifact' and item.get('executable') and item['profile']['test']:
        artifacts.append(item)
assert len(artifacts) == (1 if '--component' in sys.argv[3:] else 3), [x['target']['name'] for x in artifacts]

def classification(crate, name):
    if name.startswith('bridge_units::'):
        return 'unit'
    if crate == 'code_monitor_build_units':
        return 'unit'
    if crate == 'shared':
        if name.endswith(('test_webhook_channel_send_error', 'test_slack_channel_send_error', 'test_discord_channel_send_error')):
            return 'integration: actual loopback HTTP attempts; replace transport before unit use'
        return 'unit'
    if crate == 'monitor_client':
        if '--strict-baseline' in sys.argv and name.startswith('dashboard::'):
            return 'integration: original dashboard constructor opens real SQLite indirectly'
        if '--strict-baseline' in sys.argv and name.endswith(('test_handle_command_connect_error', 'test_connect_to_server_error')):
            return 'integration: original gRPC connection has no transport double'
        if name.startswith('storage::tests::'):
            return 'integration: real SQLite adapter; not used in initial isolated unit baseline'
        return 'unit'
    if name.startswith('monitor::campaign_ip_units::'):
        return 'unit'
    if name.startswith('metrics::tests::'):
        return 'unit'
    if name.startswith(('config::tests::', 'tls::tests::', 'tests::')):
        return 'unit'
    if name.startswith('collectors::'):
        if name.endswith(('test_docker_collector_collect_when_unavailable', 'test_docker_collector_with_socket_path_does_not_panic', 'test_docker_collector_new_with_env_var_does_not_panic')):
            return 'smoke/integration: native Docker adapter or constructor does-not-panic probe'
        if name.endswith(('test_systemd_collect_empty_units', 'test_systemd_collect_failed_never_errors')):
            return 'integration: invokes platform systemctl without a double on Linux'
        return 'unit'
    if name.startswith('monitor::tests::test_parse_ip_addr_output') or name.startswith('monitor::tests::test_map_process_status') or name.startswith('monitor::tests::test_push_or_log'):
        return 'unit'
    return 'integration: real SystemMonitor, system sampling or service orchestration; needs provider doubles'

suite_root = ET.Element('testsuites')
selection = {}
unit_failed = False
raw = out / 'profiles'
raw.mkdir(exist_ok=True)
env = {key: value for key, value in os.environ.items() if not key.startswith('CODE_MONITOR_') and key not in ['DOCKER_HOST','DOCKER_TLS_VERIFY','DOCKER_CERT_PATH','DOCKER_CONTEXT']}
env.update(LLVM_PROFILE_FILE=str(raw / '%p-%m.profraw'), DOCKER_SOCKET=str(out / 'nonexistent-docker.sock'))
for artifact in artifacts:
    binary = artifact['executable']
    crate = artifact['target']['name'].replace('-', '_')
    if crate in ['monitor_client','monitor_server'] and '--strict-baseline' not in sys.argv:
        assert 'unit-doubles' in artifact['features'], 'compile with the explicit unit provider feature'
    listing = subprocess.check_output([binary, '--list', '--format=terse'], text=True)
    (out / f'{crate}-list.txt').write_text(listing)
    names = [line[:-6] for line in listing.splitlines() if line.endswith(': test')]
    assert names
    labels = {name: classification(crate, name) for name in names}
    excluded = [name for name, label in labels.items() if label != 'unit']
    selected = [name for name, label in labels.items() if label == 'unit']
    assert selected
    command = [binary, '--test-threads=1', '--format=pretty', '--color=never']
    for name in excluded:
        command.extend(['--skip', name])
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    run = subprocess.run(command, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env)
    (out / f'{crate}-run.log').write_text(run.stdout)
    records = re.findall(r'^test (\S+) \.\.\. (ok|FAILED|ignored)$', run.stdout, re.M)
    assert set(name for name, _ in records) == set(selected), (crate, len(records), len(selected))
    suite = ET.SubElement(suite_root, 'testsuite', name=crate, tests=str(len(records)), failures=str(sum(result == 'FAILED' for _, result in records)), errors='0', skipped=str(sum(result == 'ignored' for _, result in records)))
    for name, result in records:
        case = ET.SubElement(suite, 'testcase', name=name)
        if result != 'ok':
            ET.SubElement(case, 'failure' if result == 'FAILED' else 'skipped').text = run.stdout
    unit_failed |= run.returncode != 0 or any(result != 'ok' for _, result in records)
    selection[crate] = {'classification': labels, 'command': command, 'started_at': started, 'unit_command_exit_code': run.returncode, 'passed': sum(result == 'ok' for _, result in records), 'failed': sum(result == 'FAILED' for _, result in records), 'integration_not_run': len(excluded)}
    selection[crate]['compiled_features'] = artifact['features']
    if crate == 'monitor_client' and 'unit-doubles' in artifact['features']:
        baseline_catalog = Path('/Volumes/LaCie/GitLab/coverage-unitarios-2026-10-17/resultados/codigocentral-code-monitor/baseline/rust-strict/unit-selection.json')
        original = json.loads(baseline_catalog.read_text())['monitor_client']['classification']
        gated = [name for name in original if name.startswith('storage::tests::')]
        assert len(gated) == 13 and not (set(gated) & set(names))
        selection[crate]['native_sqlite_cases_retained_in_normal_test_configuration'] = gated
        selection[crate]['original_catalog'] = str(baseline_catalog)

    print(crate, selection[crate]['passed'], 'passed;', len(excluded), 'integration tests retained separately', flush=True)

ET.ElementTree(suite_root).write(out / 'unit.xml', encoding='utf-8', xml_declaration=True)
(out / 'unit-selection.json').write_text(json.dumps(selection, indent=2) + '\n')
profiles = list(raw.glob('*.profraw'))
assert profiles
subprocess.run([os.environ['LLVM_PROFDATA'], 'merge', '-sparse', *map(str, profiles), '-o', str(out / 'coverage.profdata')], check=True)
objects = []
for artifact in artifacts:
    objects.extend(['-object', artifact['executable']])
common = [os.environ['LLVM_COV'], 'export', '-instr-profile=' + str(out / 'coverage.profdata'), *objects, '-ignore-filename-regex=(/cache/|/rustc/)', '-num-threads=1']
with (out / 'coverage.info').open('w') as handle:
    subprocess.run([*common, '-format=lcov'], check=True, stdout=handle)
with (out / 'llvm-coverage.json').open('w') as handle:
    subprocess.run(common, check=True, stdout=handle)
print(json.loads((out / 'llvm-coverage.json').read_text())['data'][0]['totals'], flush=True)
subprocess.run([sys.executable, 'coverage/rust-production-scope.py', str(out), *[arg for arg in sys.argv[3:] if arg.startswith('--source-head=')]], check=True)
if unit_failed:
    raise SystemExit(1)
