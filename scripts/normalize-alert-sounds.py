"""Rebuild cues from hash-checked vendored originals, never processed outputs."""
import array
from collections import deque
import hashlib
import io
import math
from pathlib import Path
import sys
import wave

REPO = Path(__file__).resolve().parents[1]
ROOT = REPO / "app/src-tauri/resources/sounds"
SOURCE_DIR = ROOT / "originals"
ORIGINALS = {
    "flawless": "ead9947a88a9d89d9e7cb58f0a451df0dddd4e414da39a5054a53aa3675b2a00",
    "personal_best": "daea8553303689e70beb31eb10a52f0e9e58d34052e4cf824a6a3572334f1d83",
    "white_bag": "429c4d316d6269a8b66e97698d560f0fde01fb48dbd5ab794208ec5215e897f6",
}
PEAK = 10 ** (-1 / 20)


def levels(samples):
    return (20 * math.log10(max(abs(x) for x in samples)),
            20 * math.log10(math.sqrt(sum(x * x for x in samples) / len(samples))))


def peak_normalize(samples):
    gain = PEAK / max(abs(x) for x in samples)
    return [x * gain for x in samples]


def limit(samples, channels, rate):
    # Stereo-linked detector: 1 ms look-ahead, 0.1 ms attack, 50 ms release.
    # The normalized input peaks at -1 dBFS; cap reduction at 15 dB.
    peaks = [max(abs(x) for x in samples[i:i + channels])
             for i in range(0, len(samples), channels)]
    lookahead = round(rate * 0.001)
    attack = math.exp(-1 / (rate * 0.0001))
    release = math.exp(-1 / (rate * 0.050))
    ceiling = 10 ** (-16 / 20)
    minimum_gain = 10 ** (-15 / 20)
    future = deque()
    result = []
    for i in range(len(peaks) - 1, -1, -1):
        while future and future[0] > i + lookahead:
            future.popleft()
        while future and peaks[future[-1]] <= peaks[i]:
            future.pop()
        future.append(i)
        # Store the linked future maximum; retain the original peaks for the deque.
        result.append(peaks[future[0]])
    result.reverse()
    def target_gain(peak):
        return max(minimum_gain, min(1.0, ceiling / peak)) if peak else 1.0

    gain = target_gain(result[0])
    output = []
    for frame, peak in enumerate(result):
        target = target_gain(peak)
        coefficient = attack if target < gain else release
        gain = target + coefficient * (gain - target)
        assert minimum_gain <= gain <= 1.0
        output.extend(x * gain for x in samples[frame * channels:(frame + 1) * channels])
    return output


def normalize(name):
    path = ROOT / f"{name}.wav"
    source_bytes = (SOURCE_DIR / f"{name}.wav").read_bytes()
    assert hashlib.sha256(source_bytes).hexdigest() == ORIGINALS[name], "Original source changed"
    with wave.open(io.BytesIO(source_bytes), "rb") as source:
        params = source.getparams()
        assert params.sampwidth == 2 and params.comptype == "NONE"
        pcm = array.array("h", source.readframes(params.nframes))
    if sys.byteorder != "little":
        pcm.byteswap()
    samples = [x / 32768 for x in pcm]
    before = levels(samples)
    samples = peak_normalize(samples)
    if name == "white_bag":
        samples = peak_normalize(limit(samples, params.nchannels, params.framerate))
    pcm = array.array("h", (round(x * 32768) for x in samples))
    after = levels([x / 32768 for x in pcm])
    rms_band = (-18.1, -16.1) if name == "white_bag" else (-11.3, -5.3)
    assert after[0] <= -0.9 and rms_band[0] <= after[1] <= rms_band[1], after
    if sys.byteorder != "little":
        pcm.byteswap()
    with wave.open(str(path), "wb") as target:
        target.setparams(params)
        target.writeframes(pcm.tobytes())
    print(f"{name}: peak/RMS {before[0]:.2f}/{before[1]:.2f} -> "
          f"{after[0]:.2f}/{after[1]:.2f} dBFS; SHA256 {hashlib.sha256(path.read_bytes()).hexdigest()}")


if __name__ == "__main__":
    for name in ORIGINALS:
        normalize(name)
