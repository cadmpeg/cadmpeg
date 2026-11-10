# SPDX-License-Identifier: Apache-2.0
"""Generate source-local vertex, roster, incidence, and pcurve record graphs."""
from pathlib import Path
from struct import pack


def doubles(values):
    return pack("<" + "d" * len(values), *values)


def frame(kind, payload):
    return bytes([0xB2, 3, kind, len(payload), 5]) + payload


def reference(source, target):
    return bytes([4 * (target - source - 1) + 3] if target > source
                 else [4 * (source - target) + 1])


def spline(knots, extrapolation=0):
    n = len(knots)
    prefix = bytes([1, 21, 4 * n + 1])
    prefix += bytes([8, 4 * extrapolation + 1]) if extrapolation else bytes([12])
    payload = prefix + doubles(knots) + bytes([4 * n + 1, 2])
    payload += doubles(knots) + doubles([0] * n)
    payload += doubles([1] * n) + doubles([0] * n)
    payload += bytes([5]) + doubles([0] * (2 * n))
    payload += doubles([max(0, knots[0]), knots[-1]]) + bytes([7])
    return frame(0x20, payload)


def graph(tail, curves, vertex_lane=True, incidence_curves=None):
    vertex = frame(0x5D, reference(0, 1) + bytes([0]))
    roster = frame(5, bytes([0x81]) + reference(1, 2))
    curve_targets = list(range(3, 3 + len(curves))) if incidence_curves is None else incidence_curves
    targets = ([0] if vertex_lane else []) + curve_targets
    incidence = frame(6, bytes([0x80 + len(targets)])
                      + b"".join(reference(2, target) for target in targets) + tail)
    return vertex + roster + incidence + b"".join(spline(*curve) for curve in curves)


def fixtures():
    ordinary = [([-1, 2, 4], 0)]
    extended = bytes([0x80]) + doubles([0.5]) + bytes([4, 0x45])
    mapping = bytes([0x82]) + reference(5, 3) + reference(5, 4) + bytes([0x0A, 0x87, 0x0D])
    mapping += doubles([0, 8, 1e-6, 0, 4, 2, 0, 1e-6])
    companion = graph(extended, [([-1, 4, 8], 0)] + ordinary, incidence_curves=[4])
    trim = frame(0x5D, bytes([1])) + companion[len(frame(0x5D, reference(0, 1) + bytes([0]))):]
    trim += frame(0x25, mapping)
    trim += frame(0x5E, b"".join(reference(6, target) for target in [5, 0, 7, 2, 9]) + bytes([1]))
    trim += frame(0x5D, bytes([1])) + frame(5, bytes([0x81]) + reference(8, 9))
    trim += frame(6, bytes([0x82]) + reference(9, 7) + reference(9, 4)
                  + bytes([0x80]) + doubles([1.0]) + bytes([4, 0x45]))
    trim += frame(0x27, bytes([0x84, 0]) + doubles([
        0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 8, -1, 1,
    ]))
    return {
        "mapped-trim.bin": trim,
        "mapped-companion.bin": companion + frame(0x25, mapping),
        "extended-first-lane.bin": graph(bytes([0x81]) + doubles([0.5]) + bytes([4, 0x45]), ordinary * 2),
        "extended-tag-and-scalar.bin": graph(
            bytes([0x80]) + doubles([0.5]) + bytes([4, 0x45]) + doubles([2.75]), ordinary * 2),
        "extended-span.bin": graph(extended, ordinary),
        "compact-span.bin": graph(bytes([0x80]) + doubles([0.5]) + bytes([9]), ordinary),
        "leading-extrapolation.bin": graph(
            bytes([0x80]) + doubles([0.25]) + bytes([5]), [([-2, -0.5, 0, 1.5], 2)]),
        "mixed-final-tag.bin": graph(
            bytes([0x80]) + doubles([0.5, 0.75]) + bytes([9]), ordinary * 2),
        "mixed-first-tag.bin": graph(
            bytes([0x10]) + doubles([0.5]) + bytes([9]) + doubles([12.25]),
            ordinary + [([10, 12, 15], 0)]),
        "extended-three-lanes.bin": graph(
            bytes([0]) + doubles([0.5]) + bytes([4, 0x45])
            + doubles([12.25, 0.75]) + bytes([4, 0x44]),
            ordinary + [([10, 12, 15], 0)] + ordinary, vertex_lane=False),
    }


if __name__ == "__main__":
    for name, data in fixtures().items():
        Path(__file__).with_name(name).write_bytes(data)
