#!/usr/bin/env python3
"""Make Lanternflame's placeholder sound effects (assets/sounds/*.wav).

The sounds are generated from simple waves and noise, so they are ours to
use freely. Run from the project folder:  python3 tools/make_sounds.py
Replace any file with a real recording of the same name whenever you like.
"""
import math
import random
import struct
import wave
from pathlib import Path

RATE = 22050
OUT = Path(__file__).resolve().parent.parent / "assets" / "sounds"
random.seed(7)


def silence(seconds):
    return [0.0] * int(RATE * seconds)


def tone(freq, seconds, start=0.0, decay=6.0, shape="sine", glide=None, attack=0.005):
    """A note. `freq` may glide to `glide` Hz over its length."""
    n = int(RATE * seconds)
    out = []
    phase = 0.0
    for i in range(n):
        t = i / RATE
        f = freq if glide is None else freq + (glide - freq) * (i / n)
        phase += 2 * math.pi * f / RATE
        if shape == "sine":
            v = math.sin(phase)
        elif shape == "square":
            v = 0.6 if math.sin(phase) >= 0 else -0.6
        else:  # soft saw
            v = ((phase / math.pi) % 2.0) - 1.0
            v *= 0.6
        env = min(1.0, t / attack) * math.exp(-decay * t)
        out.append(v * env)
    return out


def noise(seconds, decay=8.0, smooth=0.0, sweep=None, attack=0.003):
    """Noise; `smooth` (0-0.99) makes it duller, `sweep` = (start, end) smoothness."""
    n = int(RATE * seconds)
    out = []
    last = 0.0
    for i in range(n):
        t = i / RATE
        s = smooth if sweep is None else sweep[0] + (sweep[1] - sweep[0]) * (i / n)
        last = last * s + random.uniform(-1, 1) * (1 - s)
        env = min(1.0, t / attack) * math.exp(-decay * t)
        out.append(last * env * (1.0 + 2.0 * s))
    return out


def mix(*parts):
    """Mix (offset_seconds, samples, volume) parts together."""
    length = max(int(o * RATE) + len(p) for o, p, _ in parts)
    out = [0.0] * length
    for offset, samples, volume in parts:
        start = int(offset * RATE)
        for i, v in enumerate(samples):
            out[start + i] += v * volume
    return out


def save(name, samples):
    peak = max(1e-6, max(abs(v) for v in samples))
    scale = 0.85 / peak
    # Short fade-out so nothing clicks.
    fade = int(RATE * 0.01)
    for i in range(min(fade, len(samples))):
        samples[-1 - i] *= i / fade
    with wave.open(str(OUT / f"{name}.wav"), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(v * scale * 32767)) for v in samples))


def notes(freqs, gap, length, decay=5.0, shape="sine"):
    return mix(*[(i * gap, tone(f, length, decay=decay, shape=shape), 1.0) for i, f in enumerate(freqs)])


OUT.mkdir(parents=True, exist_ok=True)

# Weapon hits.
save("slash", noise(0.16, decay=18, sweep=(0.2, 0.8)))
save("slash_heavy", mix((0, noise(0.28, decay=10, sweep=(0.3, 0.9)), 1.0), (0, tone(120, 0.25, decay=12), 0.6)))
save("whoosh", noise(0.4, decay=5, sweep=(0.95, 0.5), attack=0.12))
save("slam", mix((0, tone(90, 0.45, decay=7, glide=40), 1.0), (0, noise(0.3, decay=12, smooth=0.9), 0.8)))
save("bash", mix((0, tone(170, 0.15, decay=25, glide=110), 1.0), (0, noise(0.1, decay=30, smooth=0.6), 0.5)))
# Magic.
save("fire", mix((0, noise(0.32, decay=9, smooth=0.5), 0.8), (0, tone(70, 0.3, decay=10), 0.5)))
save("fire_big", mix((0, noise(0.65, decay=5, smooth=0.75), 1.0), (0, tone(55, 0.6, decay=5, glide=35), 0.7)))
save("holy", mix((0, tone(880, 0.5, decay=7), 0.6), (0, tone(1320, 0.5, decay=9), 0.4), (0, tone(1760, 0.3, decay=14), 0.2)))
save("heal", notes([660, 990], 0.09, 0.5, decay=6))
save("buff", notes([523, 659, 784], 0.06, 0.3, decay=10))
save("taunt", mix((0, tone(110, 0.32, decay=7, shape="saw"), 1.0), (0, tone(113, 0.32, decay=7, shape="saw"), 0.8)))
save("boom", mix((0, noise(0.75, decay=5, smooth=0.95), 1.0), (0, tone(60, 0.7, decay=5, glide=30), 0.9)))
# Being hit.
save("hit", mix((0, tone(140, 0.12, decay=30, glide=80), 1.0), (0, noise(0.08, decay=40, smooth=0.7), 0.6)))
save("tick", noise(0.05, decay=60, smooth=0.5))
save("crit", mix((0, noise(0.2, decay=16, smooth=0.1), 0.6), (0, tone(1200, 0.2, decay=14), 0.4), (0, tone(150, 0.15, decay=25), 0.8)))
# Interface.
save("blip", mix((0, tone(330, 0.06, decay=10, shape="square"), 1.0), (0.08, tone(262, 0.08, decay=10, shape="square"), 1.0)))
# Lantern flame.
shimmer = [v * (0.6 + 0.4 * math.sin(i / RATE * 2 * math.pi * 9)) for i, v in enumerate(tone(400, 1.4, decay=1.0, glide=1100, attack=0.3))]
save("flame_change", shimmer)
save("flame_caught", mix((0, noise(0.4, decay=8, smooth=0.8), 0.8), (0.05, tone(784, 0.6, decay=5), 0.5), (0.05, tone(1175, 0.6, decay=6), 0.3)))
# Falling, rising, travelling.
save("defeated", notes([392, 330, 262], 0.16, 0.5, decay=4))
save("revived", notes([262, 330, 392, 523], 0.1, 0.5, decay=4))
save("portal", mix((0, noise(0.6, decay=4, sweep=(0.9, 0.4), attack=0.15), 0.8), (0, tone(300, 0.6, decay=3, glide=700, attack=0.1), 0.4)))
# The fight.
save("pull", mix((0, tone(70, 0.6, decay=5), 1.0), (0, noise(0.2, decay=15, smooth=0.9), 0.8), (0.15, tone(147, 0.7, decay=3, shape="saw"), 0.4)))
save("victory", mix(
    (0.0, notes([523, 659, 784], 0.12, 0.4, decay=5), 0.8),
    (0.42, tone(1047, 1.1, decay=2.5), 0.6),
    (0.42, tone(784, 1.1, decay=2.5), 0.4),
    (0.42, tone(659, 1.1, decay=2.5), 0.4),
))
save("wipe", notes([330, 311, 262, 196], 0.25, 0.7, decay=3, shape="saw"))
print("made", len(list(OUT.glob("*.wav"))), "sounds in", OUT)
