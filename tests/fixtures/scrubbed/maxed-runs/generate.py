"""Copy only hash-admitted synthetic inputs; refuse changed code or existing outputs."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent
INPUTS = {
    "demo.pcap": ("maxed-demo/story.pcap", "fbbf9044a7c913c6594d2648b64f5c9a2556355b272b9f7573e17f3b19437f67"),
    "complete.pcap": ("maxed-home/complete.pcap", "f8b4459e90e6100b9464383c258c5ff875746dc06ed71d120ffa9bf74fe13f69"),
    "disconnected.pcap": ("maxed-home/disconnected.pcap", "96598bab31300e0acbbf3403491deb6959f3393c0b24df02dafc6524a0f37b3b"),
    "zero-duration.pcap": ("maxed-home/zero-duration.pcap", "7e3a038f2ec5d4e6ebd0157fc8075f1abb35ea204ec0f6636d2083e882625215"),
    "catalog.json": ("maxed-demo/catalog.json", "6d39065758db58797b946431a61432a00cf7532dea465d6f3b7592e902220d8d"),
}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def generate():
    # Validate everything before writing anything, including previously generated files.
    outputs = {}
    for name, (source, expected) in INPUTS.items():
        data = (ROOT.parent / source).read_bytes()
        if digest(data) != expected:
            raise ValueError(f"Synthetic input hash mismatch: {source}")
        outputs[name] = data
    generator_hash = digest((ROOT / "generate.py").read_bytes())
    manifest_path = ROOT / "manifest.json"
    if manifest_path.exists():
        old = json.loads(manifest_path.read_text(encoding="utf-8"))
        if old["generator_sha256"] != generator_hash:
            raise ValueError("Generator hash mismatch; review and repin before generation")
        for name, expected in old["outputs"].items():
            path = ROOT / name
            if path.exists() and digest(path.read_bytes()) != expected:
                raise ValueError(f"Existing output hash mismatch: {name}")
    manifest = {
        "provenance": "Invented maxed-demo bytes and admitted maxed-home timestamp/prefix derivatives only; no owner data.",
        "generator_sha256": generator_hash,
        "inputs": {source: sha for source, sha in INPUTS.values()},
        "outputs": {name: digest(data) for name, data in outputs.items()},
        "transformations": "Byte-for-byte copies. Home inputs shift demo timestamps by 1772949595 seconds; disconnected ends at original 8030000 us, zero-duration at 6030000 us. No packet body changes.",
        "identities": ["DemoHero", "DemoAccount", "DemoAlly", "DemoAllyAccount"],
        "credentials": "No HELLO, password, token, or account request. Documentation IPs only.",
        "expected": {
            "demo": {"started": 5000000, "damage": 300, "dps": None, "drops": 1, "outcome": "cleared"},
            "complete": {"started": 1772949600000000, "damage": 300, "dps": None, "drops": 1, "outcome": "cleared"},
            "disconnected": {"started": 1772949600000000, "damage": 300, "dps": None, "drops": 0, "outcome": "unknown"},
            "zero-duration": {"started": 1772949600000000, "damage": 100, "dps": None, "drops": 0, "outcome": "unknown"},
        },
        "scenarios": {
            "replay_run_fight_player_matches_stored_sources": "packet replay: parse/stream/protocol/engine/writer/native reads",
            "late_counter_revision_reopens_one_story": "decoded injection: omit Victory, next-area CREATE_SUCCESS plus identity UPDATE; observed comparison revision uses typed domain events",
            "saved_death_and_partial_story_evidence": "typed domain events, not DEATH packet decoding",
            "story_waits_for_writer_retry_and_survives_stop": "packet prefix plus adapter AppClosed and actual SQLite write lock; native read-error seam",
            "reconstruction_counters_do_not_become_run_counts": "decoded injection into Engine, real store persistence; not encrypted pcap",
        },
        "ui_fixtures": "None in C1; no UI acceptance claimed.",
    }
    encoded = (json.dumps(manifest, indent=2) + "\n").encode()
    if manifest_path.exists() and manifest_path.read_bytes() != encoded:
        raise ValueError("Manifest mismatch; review provenance before regeneration")
    for name, data in outputs.items():
        (ROOT / name).write_bytes(data)
    manifest_path.write_bytes(encoded)
    print("PASS: five pinned synthetic outputs generated")


if __name__ == "__main__":
    generate()
