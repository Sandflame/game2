#!/usr/bin/env python3
"""Make Lanternflame's placeholder sound effects (assets/sounds/*.wav).

These are built from soft waves and filtered noise, so they are ours to use
freely. They are deliberately quiet and gentle: placeholders until real
recordings replace them. Run from the project folder:

    python3 tools/make_sounds.py

To use better sounds, put a file with the same name in assets/sounds/ (or
point `sounds.ron` at a new file). Free CC0 packs such as Kenney's
"RPG Audio", "Impact Sounds" and "Interface Sounds" (kenney.nl) work well.
"""
import math
import random
import struct
import wave
from pathlib import Path

RATE = 22050
OUT = Path(__file__).resolve().parent.parent / "assets" / "sounds"
random.seed(11)
TAU = 2 * math.pi


def n(seconds):
    return int(RATE * seconds)


# --- building blocks -------------------------------------------------------

def envelope(length, attack, decay):
    """Soft rise over `attack` seconds, then an exponential fade."""
    out = []
    for i in range(length):
        t = i / RATE
        rise = min(1.0, t / attack) if attack > 0 else 1.0
        rise = rise * rise * (3 - 2 * rise)  # ease in
        out.append(rise * math.exp(-decay * t))
    return out


def sine(freq, seconds, attack=0.01, decay=4.0, glide=None, vibrato=0.0):
    length = n(seconds)
    env = envelope(length, attack, decay)
    out, phase = [], 0.0
    for i in range(length):
        f = freq if glide is None else freq * (glide / freq) ** (i / length)
        f *= 1.0 + vibrato * math.sin(TAU * 5.0 * i / RATE)
        phase += TAU * f / RATE
        out.append(math.sin(phase) * env[i])
    return out


def bell(freq, seconds, decay=3.0):
    """A soft chime: a few gently detuned partials."""
    return mix(
        (0, sine(freq, seconds, 0.004, decay), 1.0),
        (0, sine(freq * 2.0, seconds, 0.004, decay * 1.6), 0.35),
        (0, sine(freq * 3.01, seconds, 0.004, decay * 2.5), 0.12),
        (0, sine(freq * 1.003, seconds, 0.004, decay), 0.5),
    )


def noise(seconds):
    return [random.uniform(-1, 1) for _ in range(n(seconds))]


def band(samples, low, high):
    """Band-pass filter; `low`/`high` may be (start, end) pairs to sweep."""
    out = []
    lp = hp_in = hp_out = 0.0
    count = len(samples)
    for i, x in enumerate(samples):
        k = i / max(1, count - 1)
        lo = low[0] + (low[1] - low[0]) * k if isinstance(low, tuple) else low
        hi = high[0] + (high[1] - high[0]) * k if isinstance(high, tuple) else high
        a = 1 - math.exp(-TAU * hi / RATE)
        lp += a * (x - lp)
        b = math.exp(-TAU * lo / RATE)
        hp_out = b * (hp_out + lp - hp_in)
        hp_in = lp
        out.append(hp_out)
    return out


def shaped(samples, attack, decay):
    env = envelope(len(samples), attack, decay)
    return [s * e for s, e in zip(samples, env)]


def mix(*parts):
    """Mix (offset seconds, samples, volume) parts."""
    length = max(n(o) + len(p) for o, p, _ in parts)
    out = [0.0] * length
    for offset, samples, volume in parts:
        start = n(offset)
        for i, v in enumerate(samples):
            out[start + i] += v * volume
    return out


def reverb(samples, amount=0.25, tail=0.6):
    """A small room: a few feedback echoes, softened."""
    out = samples + [0.0] * n(tail)
    for delay, gain in ((0.031, 0.5), (0.047, 0.45), (0.071, 0.4), (0.097, 0.35)):
        d = n(delay)
        line = [0.0] * len(out)
        for i in range(len(out)):
            prev = line[i - d] if i >= d else 0.0
            src = out[i] if i < len(samples) else 0.0
            line[i] = src + prev * gain
        for i in range(len(out)):
            out[i] += line[i] * amount * 0.25
    return out


def save(name, samples, loudness=0.5):
    """Normalise to a modest level (never full scale) and write."""
    peak = max(1e-6, max(abs(v) for v in samples))
    scale = loudness / peak
    fade = min(len(samples), n(0.02))
    for i in range(fade):
        samples[-1 - i] *= i / fade
    with wave.open(str(OUT / f"{name}.wav"), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(v * scale * 32767)) for v in samples))


def whoosh(seconds, low, high, attack=0.05, decay=6.0):
    return shaped(band(noise(seconds), low, high), attack, decay)


def thud(freq=90, seconds=0.25, decay=14.0):
    return mix(
        (0, sine(freq, seconds, 0.003, decay, glide=freq * 0.55), 1.0),
        (0, shaped(band(noise(seconds), 60, 400), 0.002, decay * 1.5), 0.5),
    )


OUT.mkdir(parents=True, exist_ok=True)

# Weapons: airy swishes, not clicks.
save("slash", whoosh(0.22, (900, 500), (5000, 2500), 0.02, 14), 0.35)
save("slash_heavy", mix((0, whoosh(0.32, (600, 300), (3500, 1500), 0.03, 9), 1.0), (0.08, thud(80, 0.3), 0.6)), 0.45)
save("whoosh", whoosh(0.45, (300, 900), (1500, 3500), 0.12, 6), 0.35)
save("bash", thud(110, 0.22, 16), 0.45)
save("slam", reverb(mix((0, thud(70, 0.6, 6), 1.0), (0, whoosh(0.5, 40, 300, 0.005, 6), 0.7)), 0.3), 0.55)
# Magic.
fire = mix((0, whoosh(0.5, (200, 500), (1200, 2500), 0.04, 5), 1.0),
           *[(random.uniform(0.02, 0.35), shaped(band(noise(0.02), 2000, 6000), 0.001, 120), 0.25) for _ in range(9)])
save("fire", fire, 0.35)
save("fire_big", reverb(mix((0, whoosh(0.8, (120, 400), (900, 2200), 0.06, 3.5), 1.0), (0, thud(60, 0.6, 5), 0.6)), 0.25), 0.45)
save("holy", reverb(mix((0, bell(784, 0.8, 4), 1.0), (0.05, bell(1175, 0.7, 5), 0.5)), 0.35, 0.8), 0.3)
save("heal", reverb(mix((0, bell(523, 0.9, 3.5), 0.8), (0.09, bell(659, 0.9, 3.5), 0.7), (0.18, bell(784, 0.9, 3.5), 0.6)), 0.4, 0.9), 0.3)
save("buff", reverb(mix((0, sine(392, 0.5, 0.06, 5, glide=523), 1.0), (0, sine(587, 0.5, 0.06, 6, glide=784), 0.4)), 0.3), 0.25)
save("taunt", mix((0, sine(110, 0.5, 0.03, 5, vibrato=0.015), 1.0), (0, sine(165, 0.5, 0.03, 6, vibrato=0.015), 0.4), (0, thud(90, 0.2), 0.5)), 0.4)
save("boom", reverb(mix((0, thud(55, 0.9, 4.5), 1.0), (0, whoosh(0.8, 30, 250, 0.01, 4.5), 0.9)), 0.3, 0.8), 0.55)
# Being hit.
save("hit", thud(130, 0.18, 22), 0.4)
save("tick", shaped(band(noise(0.04), 300, 1500), 0.002, 80), 0.15)
save("crit", mix((0, thud(120, 0.25, 15), 1.0), (0, bell(1568, 0.3, 12), 0.3)), 0.45)
# Interface.
save("blip", mix((0, sine(660, 0.12, 0.005, 25), 1.0), (0.07, sine(523, 0.14, 0.005, 25), 0.8)), 0.15)
# Lantern flame.
shimmer = mix(*[(i * 0.12, bell(f, 0.6, 5), 0.6) for i, f in enumerate([523, 659, 784, 988, 1175])],
              (0, whoosh(1.2, (200, 600), (800, 2000), 0.5, 2.5), 0.5))
save("flame_change", reverb(shimmer, 0.35, 0.8), 0.25)
save("flame_caught", reverb(mix((0, whoosh(0.5, (150, 400), (900, 2000), 0.02, 6), 1.0), (0.04, bell(784, 0.8, 4), 0.6)), 0.3), 0.35)
# Falling, rising, travelling.
save("defeated", reverb(mix((0, sine(392, 0.7, 0.02, 3), 1.0), (0.22, sine(330, 0.7, 0.02, 3), 0.9), (0.44, sine(262, 1.0, 0.02, 2.5), 0.9)), 0.35, 0.9), 0.35)
save("revived", reverb(mix(*[(i * 0.12, bell(f, 0.9, 3.5), 0.8) for i, f in enumerate([392, 523, 659, 784])]), 0.4, 0.9), 0.3)
save("portal", reverb(mix((0, whoosh(0.9, (200, 800), (1000, 3000), 0.25, 3), 1.0), (0, sine(330, 0.9, 0.2, 3, glide=660), 0.25)), 0.35), 0.35)
# The fight.
save("pull", reverb(mix((0, thud(65, 0.8, 4), 1.0), (0.1, sine(98, 1.0, 0.15, 2.5, vibrato=0.01), 0.5), (0.1, sine(147, 1.0, 0.15, 3, vibrato=0.01), 0.3)), 0.3, 0.9), 0.45)
save("victory", reverb(mix(
    (0.00, bell(523, 1.2, 2.5), 0.8),
    (0.15, bell(659, 1.2, 2.5), 0.8),
    (0.30, bell(784, 1.4, 2.0), 0.8),
    (0.50, bell(1047, 1.8, 1.6), 0.9),
), 0.4, 1.2), 0.4)
save("wipe", reverb(mix((0, sine(262, 1.0, 0.05, 2.5), 1.0), (0.35, sine(233, 1.0, 0.05, 2.5), 0.9), (0.7, sine(196, 1.4, 0.05, 2.0), 0.9)), 0.35, 1.0), 0.35)
print("made", len(list(OUT.glob("*.wav"))), "sounds in", OUT)
