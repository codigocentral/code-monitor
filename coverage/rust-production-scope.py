"""Project native LLVM points onto the production scope, excluding test code.

No coverage hit is changed. Every removed point must be within a syn-parsed
#[cfg(test)] module or test-only function. Native original reports are retained.
This is equivalent to collecting `src` without test files in languages that
store tests in separate files; Rust stores these modules inline in `src`.
"""
import hashlib
import collections
import json
import os
from pathlib import Path
import subprocess
import sys

folder = Path(sys.argv[1]).resolve()
native = json.loads((folder / 'llvm-coverage.json').read_text())['data'][0]
files = [item['filename'] for item in native['files']]
root = Path.cwd().resolve()
assert files and all(Path(name).is_relative_to(root) for name in files)
parser = Path('/Volumes/LaCie/GitLab/coverage-unitarios-2026-10-17/cache/session-13/cargo-code-monitor-scope/debug/code-monitor-unit-scope')
source_head = next((arg.split('=', 1)[1] for arg in sys.argv[2:] if arg.startswith('--source-head=')), None)
if source_head:
    previous = json.loads((folder / 'production-scope-audit.json').read_text())
    scopes = {name: value['test_only_ast_ranges'] for name, value in previous['source'].items()}
    for name in files:
        original = subprocess.check_output(['git', 'show', source_head + ':' + str(Path(name).relative_to(root))])
        assert hashlib.sha256(original).hexdigest() == previous['source'][name]['source_sha256']
else:
    scopes = json.loads(subprocess.check_output([str(parser), *files], text=True))

# LLVM groups generic instantiations by the first source region. Reconstruct
# its native summaries and require exact agreement for EVERY original file
# before projecting the test-only scopes. This retains native function and
# region semantics rather than treating each monomorphization as a function.
groups = collections.defaultdict(lambda: collections.defaultdict(list))
for function in native['functions']:
    if function['regions']:
        first = function['regions'][0]
        owner = function['filenames'][first[5]]
        if owner in scopes:
            groups[owner][tuple(first[:2])].append(function)

def group_metrics(instances, owner):
    region_records = []
    for function in instances:
        regions = {tuple(region[:4]): region[4] for region in function['regions']
                   if region[7] == 0 and function['filenames'][region[5]] == owner}
        region_records.append(regions)
    return {'called': any(function['count'] > 0 for function in instances),
            'regions_valid': max(map(len, region_records)),
            'regions_covered': max(sum(hit > 0 for hit in regions.values()) for regions in region_records)}

group_stats = {name: {key: group_metrics(instances, name) for key, instances in values.items()}
               for name, values in groups.items()}
for original_file in native['files']:
    name = original_file['filename']
    native_summary = original_file['summary']
    stats = list(group_stats[name].values())
    assert (len(stats), sum(value['called'] for value in stats)) == (native_summary['functions']['count'], native_summary['functions']['covered'])
    assert (sum(value['regions_valid'] for value in stats), sum(value['regions_covered'] for value in stats)) == (native_summary['regions']['count'], native_summary['regions']['covered'])

def in_test(name, line):
    return any(start <= line <= end for start, end in scopes[name])

proof = {'definition': 'Only syn-parsed #[cfg(test)] modules and test-only functions are outside production scope.',
         'raw_lcov': str(folder / 'coverage.info'),
         'raw_llvm_json': str(folder / 'llvm-coverage.json'),
         'raw_lcov_sha256': hashlib.sha256((folder / 'coverage.info').read_bytes()).hexdigest(),
         'native_group_reconstruction_verified_for_every_file': True,
         'source_head': source_head, 'source': {}, 'totals': {}}
lines_out = []
totals = {'lines_valid': 0, 'lines_covered': 0, 'functions_valid': 0, 'functions_covered': 0,
          'regions_valid': 0, 'regions_covered': 0}
for section in (folder / 'coverage.info').read_text().split('end_of_record'):
    lines = [line for line in section.splitlines() if line]
    if not lines:
        continue
    name = next(line[3:] for line in lines if line.startswith('SF:'))
    assert name in scopes
    definitions = {line[3:].split(',', 1)[1]: int(line[3:].split(',', 1)[0]) for line in lines if line.startswith('FN:')}
    test_functions = {symbol for symbol, line in definitions.items() if in_test(name, line)}
    kept_definitions = {symbol: line for symbol, line in definitions.items() if symbol not in test_functions}
    kept_hits = {}
    kept_lines = {}
    removed_lines = []
    removed_branches = []
    kept_branches = []
    output = []
    for line in lines:
        if line.startswith('FN:'):
            if line[3:].split(',', 1)[1] in kept_definitions:
                output.append(line)
        elif line.startswith('FNDA:'):
            hit, symbol = line[5:].split(',', 1)
            if symbol not in test_functions:
                assert symbol in kept_definitions
                kept_hits[symbol] = int(hit)
                output.append(line)
        elif line.startswith('DA:'):
            number, hit = line[3:].split(',')[:2]
            if in_test(name, int(number)):
                removed_lines.append(int(number))
            else:
                kept_lines[int(number)] = int(hit)
                output.append(line)
        elif line.startswith('BRDA:'):
            number = int(line[5:].split(',')[0])
            if in_test(name, number):
                removed_branches.append(line)
            else:
                kept_branches.append(line)
                output.append(line)
        elif not line.startswith(('LF:', 'LH:', 'FNF:', 'FNH:', 'BRF:', 'BRH:')):
            output.append(line)
    assert set(kept_hits) == set(kept_definitions)
    # Verify that all original production points survive with exact native hits.
    original = {int(line[3:].split(',')[0]): int(line[3:].split(',')[1]) for line in lines if line.startswith('DA:')}
    assert kept_lines == {line: hit for line, hit in original.items() if not in_test(name, line)}
    production_groups = [value for (line, _column), value in group_stats[name].items() if not in_test(name, line)]
    metrics = {'lines_valid': len(kept_lines), 'lines_covered': sum(hit > 0 for hit in kept_lines.values()),
               'functions_valid': len(production_groups), 'functions_covered': sum(value['called'] for value in production_groups),
               'regions_valid': sum(value['regions_valid'] for value in production_groups),
               'regions_covered': sum(value['regions_covered'] for value in production_groups)}
    for key in totals:
        totals[key] += metrics[key]
    output.extend([f"FNF:{metrics['functions_valid']}", f"FNH:{metrics['functions_covered']}",
                   f"LF:{metrics['lines_valid']}", f"LH:{metrics['lines_covered']}"])
    if kept_branches:
        output.extend([f'BRF:{len(kept_branches)}', f"BRH:{sum(line.rsplit(',',1)[1] not in ['-', '0'] for line in kept_branches)}"])
    output.append('end_of_record')
    lines_out.extend(output)
    proof['source'][name] = {'source_sha256': previous['source'][name]['source_sha256'] if source_head else hashlib.sha256(Path(name).read_bytes()).hexdigest(),
                             'test_only_ast_ranges': scopes[name], 'removed_test_lines': removed_lines,
                             'removed_test_functions': sorted(test_functions), 'removed_test_branches': removed_branches,
                             'native_instantiation_records_retained': len(kept_hits),
                             'production': metrics,
                             'uncovered_production_lines': [line for line, hit in kept_lines.items() if hit == 0]}
proof['totals'] = totals
(folder / 'production.info').write_text('\n'.join(lines_out) + '\n')
(folder / 'production-scope-audit.json').write_text(json.dumps(proof, indent=2) + '\n')
print(json.dumps(totals, indent=2))
