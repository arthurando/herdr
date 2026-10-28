"""Summarize numeric Herdr latency telemetry; ignore only an incomplete final line."""
import argparse
import collections
import json
import math
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('trace')
parser.add_argument('--output')
args = parser.parse_args()
raw = Path(args.trace).read_text(encoding='utf-8')
lines = raw.splitlines()
rows = []
partial_tail = False
for index, line in enumerate(lines):
    try:
        row = json.loads(line)
    except json.JSONDecodeError:
        if index != len(lines)-1 or raw.endswith('\n'):
            raise
        partial_tail = True
        break
    if row['stage'] not in ('start', 'stop'):
        assert set(row) == {'stage', 'id', 'at_us', 'elapsed_us', 'count', 'remote'}
        assert all(isinstance(row[key], int) for key in ('id', 'at_us', 'elapsed_us', 'count'))
        assert isinstance(row['remote'], bool)
    rows.append(row)
groups = collections.defaultdict(list)
for row in rows:
    if 'elapsed_us' in row:
        groups[(row['stage'], row['remote'])].append(row['elapsed_us'])
stats = []
for (stage, remote), values in sorted(groups.items()):
    values.sort()
    stats.append(dict(stage=stage, remote=remote, samples=len(values),
                      p50_us=values[math.ceil(len(values)*.5)-1],
                      p95_us=values[math.ceil(len(values)*.95)-1], max_us=max(values)))
summary = dict(records=len(rows), partial_tail=partial_tail,
               start=next((r for r in rows if r['stage']=='start'), None),
               stop=next((r for r in rows if r['stage']=='stop'), None),
               stages=dict(collections.Counter(r['stage'] for r in rows)), durations=stats,
               limit='Durations cover named local scopes. Input-to-remote-output correlation is not causal proof.')
text = json.dumps(summary, indent=2)
if args.output:
    Path(args.output).write_text(text, encoding='utf-8')
print(text)
