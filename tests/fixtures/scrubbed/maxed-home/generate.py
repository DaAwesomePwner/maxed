"""Deterministic synthetic-only variants; reads the reviewed maxed-demo fixture."""
import hashlib
import json
import struct
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SOURCE = ROOT.parent / 'maxed-demo' / 'story.pcap'
EXPECTED = 'fbbf9044a7c913c6594d2648b64f5c9a2556355b272b9f7573e17f3b19437f67'
OFFSET = int(datetime(2026, 3, 8, 5, 59, 55, tzinfo=timezone.utc).timestamp())


def generate():
    source = SOURCE.read_bytes()
    if hashlib.sha256(source).hexdigest() != EXPECTED:
        raise ValueError('Reviewed synthetic source changed; review provenance before regeneration')
    if source[:4] != b'\xd4\xc3\xb2\xa1':
        raise ValueError('Expected classic little-endian microsecond pcap')
    frames = []
    pos = 24
    while pos < len(source):
        sec, usec, length, original = struct.unpack_from('<IIII', source, pos)
        data = source[pos + 16:pos + 16 + length]
        if len(data) != length:
            raise ValueError('Truncated synthetic source')
        frames.append((sec * 1_000_000 + usec, struct.pack('<IIII', sec + OFFSET, usec, length, original) + data))
        pos += 16 + length
    manifest = {'source_sha256': EXPECTED, 'offset_seconds': OFFSET,
                'clock': '2026-03-08T05:59:55Z; Chicago midnight occurs at story +5 seconds',
                'identities': ['DemoHero', 'DemoAccount', 'DemoAlly', 'DemoAllyAccount'],
                'credentials': 'None. Source requires no HELLO, passwords or tokens.', 'cases': []}
    for name, cutoff, description in [
        ('complete', None, 'Realm to dungeon to Nexus; 300 dungeon damage, null DPS (under three seconds), one bag, clear'),
        ('disconnected', 8_030_000, 'Packet prefix stopped before bag or victory; terminal partial run'),
        ('zero-duration', 6_030_000, 'Packet prefix with only first hit; known damage, no elapsed DPS denominator'),
    ]:
        output = source[:24] + b''.join(data for at, data in frames if cutoff is None or at <= cutoff)
        (ROOT / f'{name}.pcap').write_bytes(output)
        manifest['cases'].append({'name': name, 'kind': 'packet-replay', 'cutoff_microseconds': cutoff,
                                  'sha256': hashlib.sha256(output).hexdigest(), 'expectation': description})
    for name, expectation in [
        ('recovery-missing-identity', 'Midstream/stream-gap issues, unverified self, recovery grace; no claimed decoder coverage'),
        ('death-after-clear', 'Separate completion and death timestamps; one run and one local death'),
        ('consecutive-sessions', 'Different session IDs select disjoint runs; no wall-clock session inference'),
        ('zero-players', 'No self, denominator, rank or share'),
        ('late-counter-revision', 'Real decoded base packets omit victory; injected decoded CREATE_SUCCESS counter revises Left to Cleared'),
    ]:
        manifest['cases'].append({'name': name, 'kind': 'decoded-packet-injection' if name == 'late-counter-revision' else 'typed-dto', 'expectation': expectation})
    (ROOT / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')


if __name__ == '__main__':
    generate()
