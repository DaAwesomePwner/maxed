"""Check generated provenance and optional real CLI domain exports, offline."""
import argparse
import hashlib
import json
import struct
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def validate(domain_directory=None):
    manifest = json.loads((ROOT / 'manifest.json').read_text(encoding='utf-8'))
    source = (ROOT.parent / 'maxed-demo' / 'story.pcap').read_bytes()
    assert hashlib.sha256(source).hexdigest() == manifest['source_sha256']
    originals = []
    pos = 24
    while pos < len(source):
        sec, usec, length, original = struct.unpack_from('<IIII', source, pos)
        originals.append((sec, usec, length, original, source[pos + 16:pos + 16 + length]))
        pos += 16 + length
    for case in manifest['cases']:
        if case['kind'] != 'packet-replay':
            continue
        data = (ROOT / (case['name'] + '.pcap')).read_bytes()
        assert hashlib.sha256(data).hexdigest() == case['sha256']
        assert data[:24] == source[:24]
        expected = [frame for frame in originals if case['cutoff_microseconds'] is None
                    or frame[0] * 1_000_000 + frame[1] <= case['cutoff_microseconds']]
        pos = 24
        for sec, usec, length, original, payload in expected:
            assert struct.unpack_from('<IIII', data, pos) == (sec + manifest['offset_seconds'], usec, length, original)
            assert data[pos + 16:pos + 16 + length] == payload
            pos += 16 + length
        assert pos == len(data)
        if domain_directory:
            rows = [json.loads(line) for line in (domain_directory / f"home-{case['name']}.jsonl").read_text().splitlines()]
            assert rows[0]['inputSha256'] == case['sha256']
            runs = [row['value'] for row in rows[1:] if row['kind'] == 'run']
            fights = [row['value'] for row in rows[1:] if row['kind'] == 'fight' and row['value']['map'] == 'Snake Pit']
            assert len(runs) == len(fights) == 1
            run, fight = runs[0], fights[0]
            local = [player for player in fight['players'] if player['local']]
            assert len(local) == 1
            assert run['started'] == 1772949600000000
            assert local[0]['share'] == 1
            if case['name'] == 'complete':
                assert (fight['damage'], local[0]['dps'], run['dropCount'], run['outcome']) == (300, 150, 1, 'cleared')
                assert run['completeCapture'] is True
            else:
                assert run['outcome'] == 'unknown' and run['completedAt'] is None
                assert run['dropCount'] == 0 and run['completeCapture'] is False
                if case['name'] == 'zero-duration':
                    assert (fight['damage'], local[0]['hits'], local[0]['dps']) == (100, 1, None)
                    assert fight['firstHit'] == fight['lastHit']
                else:
                    assert (fight['damage'], local[0]['dps']) == (300, 150)
    # Independent UTC constants used by the timezone-injected UI regression.
    for iso, expected in [('2026-03-08T06:00:00', 1772949600000000), ('2026-03-09T05:00:00', 1773032400000000),
                          ('2026-11-01T05:00:00', 1793509200000000), ('2026-11-02T06:00:00', 1793599200000000)]:
        assert int(datetime.fromisoformat(iso).replace(tzinfo=timezone.utc).timestamp()) * 1_000_000 == expected
    print('PASS: three synthetic packet variants, provenance, timestamps' + (' and real CLI semantic assertions' if domain_directory else ''))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--domain-directory', type=Path)
    validate(parser.parse_args().domain_directory)
