"""Independent pcap structure/address checks plus pinned provenance verification."""
import hashlib
import argparse
import ipaddress
import json
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def frames(data):
    require(len(data) >= 24, "Truncated pcap header")
    require(struct.unpack("<IHHIIII", data[:24]) == (0xA1B2C3D4, 2, 4, 0, 0, 65535, 1),
            "Unexpected pcap header")
    pos, previous, output = 24, 0, []
    while pos < len(data):
        require(pos + 16 <= len(data), "Truncated pcap record header")
        sec, usec, size, original = struct.unpack_from("<IIII", data, pos)
        require(usec < 1000000 and 54 <= size == original <= 65535,
                "Invalid pcap record timestamp or size")
        at = sec * 1000000 + usec
        require(at >= previous, "Pcap timestamps are not monotonic")
        previous = at
        packet = data[pos + 16:pos + 16 + size]
        require(len(packet) == size and packet[12:14] == b"\x08\x00",
                "Truncated packet or unexpected Ethernet type")
        require(packet[14] == 0x45 and packet[23] == 6, "Expected IPv4/TCP packet")
        require(int.from_bytes(packet[16:18], "big") == size - 14, "Invalid IPv4 packet length")
        for address in (packet[26:30], packet[30:34]):
            require(any(ipaddress.ip_address(address) in ipaddress.ip_network(net)
                        for net in ("192.0.2.0/24", "198.51.100.0/24", "203.0.113.0/24")),
                    "Packet address is outside documentation networks")
        tcp_header = (packet[46] >> 4) * 4
        require(20 <= tcp_header <= size - 34, "Invalid TCP header length")
        output.append((at, packet))
        pos += 16 + size
    require(pos == len(data) and output, "Empty pcap or invalid record boundary")
    return output


def refusal_checks(manifest):
    for mode in ("input", "output", "generator"):
        with tempfile.TemporaryDirectory(prefix="maxed-c1-provenance-") as temporary:
            copied = Path(temporary) / "maxed-runs"
            copied.mkdir()
            for source in manifest["inputs"]:
                path = copied.parent / source
                path.parent.mkdir(exist_ok=True)
                path.write_bytes((ROOT.parent / source).read_bytes())
            for name in ["generate.py", "manifest.json", *manifest["outputs"]]:
                (copied / name).write_bytes((ROOT / name).read_bytes())
            altered = {"input": copied.parent / "maxed-demo/story.pcap",
                       "output": copied / "demo.pcap", "generator": copied / "generate.py"}[mode]
            altered.write_bytes(altered.read_bytes() + b"\n")
            before = {p: p.read_bytes() for p in copied.parent.rglob("*") if p.is_file()}
            result = subprocess.run([sys.executable, *(["-O"] * sys.flags.optimize),
                                     str(copied / "generate.py")], capture_output=True)
            require(result.returncode != 0 and b"hash mismatch" in result.stderr.lower(),
                    f"{mode} tampering did not refuse with a hash mismatch")
            after = {p: p.read_bytes() for p in copied.parent.rglob("*") if p.is_file()}
            require(before == after, f"{mode} mismatch wrote files before refusing")
    print("PASS: changed input, output and generator each refuse without writes")


def validate(native_log=None):
    manifest = json.loads((ROOT / "manifest.json").read_text())
    require(sha(ROOT / "generate.py") == manifest["generator_sha256"], "Generator hash mismatch")
    for source, expected in manifest["inputs"].items():
        require(sha(ROOT.parent / source) == expected, f"Input hash mismatch: {source}")
    for output, expected in manifest["outputs"].items():
        require(sha(ROOT / output) == expected, f"Output hash mismatch: {output}")
    source = frames((ROOT / "demo.pcap").read_bytes())
    for name, cutoff in (("complete", None), ("disconnected", 8030000), ("zero-duration", 6030000)):
        expected = [(at + 1772949595000000, packet) for at, packet in source if cutoff is None or at <= cutoff]
        actual = frames((ROOT / f"{name}.pcap").read_bytes())
        require(actual == expected, f"Packet or timestamp transformation mismatch: {name}")
        print(f"{name}: {len(actual)} whole synthetic frames")
    require(manifest["identities"] == ["DemoHero", "DemoAccount", "DemoAlly", "DemoAllyAccount"],
            "Unexpected synthetic identities")
    require(len(manifest["expected"]) == 4 and len(manifest["scenarios"]) == 5,
            "Expected four corpora and five scenarios")
    refusal_checks(manifest)
    if native_log:
        raw = native_log.read_bytes()
        log = raw.decode("utf-16" if raw.startswith((b"\xff\xfe", b"\xfe\xff")) else "utf-8")
        require("running 5 tests" in log and "test result: ok. 5 passed; 0 failed" in log,
                "Native log must discover and pass five tests")
        for name in manifest["scenarios"]:
            require(f"test runtime::history_replay_tests::{name} ... ok" in log,
                    f"Native log lacks passing test: {name}")
        for name in manifest["expected"]:
            require(f"C1 {name} fights/run: n=1 min=1 median=1 max=1" in log,
                    f"Native log lacks terminal measurement: {name}")
        print("PASS: native log discovers and passes all five named C1 tests and four terminal corpora")
    print("PASS: hashes, generator pin, pcap boundaries, documentation addresses and exact timestamp/prefix transforms")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--native-log", type=Path)
    validate(parser.parse_args().native_log)
