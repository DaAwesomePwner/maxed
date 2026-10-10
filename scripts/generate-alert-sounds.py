"""Deterministic original Maxed cues, MIT licensed. Never modifies white_bag.wav."""
import argparse
import hashlib
import math
from pathlib import Path
import struct
import wave

ROOT = Path(__file__).resolve().parents[1] / "app/src-tauri/resources/sounds"
RATE = 22050
CUES = {
    "personal_best": ((523.25, 659.25, 783.99), 0.18, 0.4),
    "flawless": ((659.25, 783.99, 1046.5), 0.18, 0.4),
    "key_popped": ((880.0, 587.33), 0.07, 0.25),
    "chat_message": ((587.33, 880.0), 0.07, 0.2),
}
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("cue", nargs="?", choices=CUES, help="Generate only this cue (default: all)")
selected = parser.parse_args().cue
for name, (frequencies, seconds, amplitude) in CUES.items():
    if selected is not None and selected != name:
        continue
    frames = []
    for frequency in frequencies:
        count = int(RATE * seconds)
        for i in range(count):
            envelope = math.sin(math.pi * i / (count - 1)) ** 2
            frames.append(round(amplitude * 32767 * envelope * math.sin(2 * math.pi * frequency * i / RATE)))
    path = ROOT / f"{name}.wav"
    with wave.open(str(path), "wb") as out:
        out.setparams((1, 2, RATE, 0, "NONE", "not compressed"))
        out.writeframes(struct.pack(f"<{len(frames)}h", *frames))
    print(name, len(path.read_bytes()), hashlib.sha256(path.read_bytes()).hexdigest())
