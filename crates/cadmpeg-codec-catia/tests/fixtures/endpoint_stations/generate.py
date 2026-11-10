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


def container(main):
    magic = b"V5_CFV2\0"
    directory_at = 16 + len(main)
    directory = bytearray(b"CATIA_V5 CB0001\0")
    for name, offset, length in [("MainDataStream", 16, len(main)), ("SurfacicReps", directory_at, 0)]:
        row = bytearray(0x54)
        row[12:16] = pack(">I", length)
        encoded = name.encode("utf-16-le")
        row[16:16 + len(encoded)] = encoded
        row[80:84] = pack(">I", 1)
        directory.extend(row + pack(">5I", offset, length, length, 0, 0))
    directory.extend(b"CB__END")
    return (magic + pack(">II", 16 + directory_at + len(directory), 0)
            + magic + pack(">II", directory_at, len(directory)) + main + directory)


def sheet(ambiguous=False):
    main = frame(0x0E, doubles([1, 2, 3, 0, 0.6, 0.8, 1, -4, 9]))
    group = bytes([0x81, 3, 5, 1])
    group += pack("<6f", -4, 4, -4, 4, -4, 4) + bytes([8, 0xD6, 0x28])
    main += bytes([0xB2, 0x13, 0x5B, len(group), 5]) + group
    main += frame(0x61, bytes([0x81, 3, 0, 0x13, 3]))
    main += frame(0x5F, bytes([0x82]) + reference(1, 3) + reference(1, 2) + bytes([3]))
    ring = bytes([0x89])
    for i in range(4):
        ring += reference(2, 4 + i) + reference(2, 8 + i)
    ring += reference(2, 3) + bytes([0x84, 0x41, 0, 0, 1])
    main += frame(0x62, ring)
    main += frame(0x27, bytes([0x84, 0]) + doubles([
        0, 0, 0, 1, 0, 0, 0, 1, 0, -4, 4, -4, 4,
    ]))
    knots = [-1, 2, 4]
    for i in range(4):
        n = len(knots)
        axis = [k - (0 if ambiguous else 2) for k in knots]
        horizontal = i % 2 == 0
        u = axis if horizontal else [1 if i == 1 else 0] * n
        v = [1 if i == 2 else 0] * n if horizontal else axis
        payload = bytes([1, 21, 4 * n + 1, 12]) + doubles(knots) + bytes([4 * n + 1, 2])
        payload += doubles(u) + doubles(v)
        payload += doubles([int(horizontal)] * n) + doubles([int(not horizontal)] * n)
        payload += bytes([5]) + doubles([0] * (2 * n)) + doubles([0, 4]) + bytes([7])
        main += frame(0x20, payload)
    edges = [(0, 1), (1, 2), (3, 2), (0, 3)]
    for i, (a, b) in enumerate(edges):
        main += frame(0x5E, bytes([1]) + b"".join(
            reference(8 + i, target) for target in [12 + a, 12 + b, 16 + 2*i, 17 + 2*i]
        ) + bytes([1]))
    for _ in range(4):
        main += frame(0x5D, bytes([1]))
    for i, (a, b) in enumerate(edges):
        for endpoint, vertex in enumerate([a, b]):
            record = 16 + 2*i + endpoint
            if ambiguous:
                tail = bytes([0x80]) + doubles([1 + endpoint]) + bytes([5]) + pack("<Q", 0x3D00000000000000)
                main += frame(6, bytes([0x83]) + reference(record, 12 + vertex)
                              + reference(record, 4 + i) + reference(record, 4 + (i+1)%4) + tail)
            else:
                tail = bytes([0x80]) + doubles([endpoint]) + bytes([4, 0x45])
                main += frame(6, bytes([0x82]) + reference(record, 12 + vertex)
                              + reference(record, 4 + i) + tail)
    return container(main)


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
    priority = b""
    priority += frame(0x5D, reference(0, 1) + bytes([0]))
    priority += frame(5, bytes([0x81]) + reference(1, 2))
    priority += frame(6, bytes([0x82]) + reference(2, 0) + reference(2, 4)
                      + bytes([0x80]) + doubles([0.5]) + bytes([0x0D]))
    priority += spline([0, 4, 8])
    positive = spline([1, 2, 3, 4], 1)
    priority += positive[:9] + bytes([7]) + positive[10:]
    priority += frame(0x25, mapping)
    priority += frame(0x5E, b"".join(reference(6, target) for target in [5, 0, 7, 2, 9]) + bytes([1]))
    priority += frame(0x5D, reference(7, 8) + bytes([0])) + frame(5, bytes([0x81]) + reference(8, 9))
    priority += frame(6, bytes([0x82]) + reference(9, 7) + reference(9, 4)
                      + bytes([0x80]) + doubles([0.75]) + bytes([0x0D]))
    priority += frame(0x27, bytes([0x84, 0]) + doubles([
        0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 8, -1, 1,
    ]))
    legacy = {f"legacy-first-tag-{name}.bin": graph(
        bytes([0x80]) + doubles([0.5]) + bytes([9]) + doubles([value]),
        ordinary + [([-4, 0, 4], 0)])
        for name, value in [("positive", 2.0), ("negative-fraction", -0.5), ("negative", -2.0)]}
    ambiguous = graph(bytes([0x80]) + doubles([0.5]) + bytes([5])
                      + pack("<Q", 0x3D00000000000000), ordinary * 2)
    return {
        **legacy,
        "ambiguous-first-tag.bin": ambiguous,
        "mapped-priority.bin": priority,
        "extended-station-sheet.catpart": sheet(),
        "ambiguous-station-sheet.catpart": sheet(True),
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
