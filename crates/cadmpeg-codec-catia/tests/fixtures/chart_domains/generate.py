# SPDX-License-Identifier: Apache-2.0
"""Generate a torus sheet whose trim crosses both stored active ranges."""
from math import pi, tau
from pathlib import Path
from struct import pack


def build(major=(0.25, 1.0), minor=(-0.6, -0.2)):
    main = bytearray()

    def frame(flag, kind, payload):
        main.extend(bytes([0xB2, flag, kind, len(payload), 5]) + payload)

    def doubles(values):
        return pack("<" + "d" * len(values), *values)

    def child(source, target):
        return 4 * (target - source - 1) + 3

    frame(3, 0x0E, doubles([1, 2, 3, 0, 0.6, 0.8, 1, -4, 9]))
    group = bytes([0x81, 3, 5, 1])
    group += pack("<6f", -20, 20, -20, 20, -20, 20) + bytes([8, 0xD6, 0x28])
    frame(0x13, 0x5B, group)
    frame(3, 0x61, bytes([0x81, 3, 0, 0x13, 3]))
    frame(3, 0x5F, bytes([0x82, child(1, 3), child(1, 2), 3]))
    ring = bytearray([0x89])
    for i in range(4):
        ring.extend([child(2, 4 + i), child(2, 8 + i)])
    ring.extend([child(2, 3), 0x84, 0x41, 0, 0, 1])
    frame(3, 0x62, ring)
    frame(3, 0x2B, doubles([
        1, 2, 3, 1, 0, 0, 0, 1, 0, 0, 0, 1, 7, 2,
        pi / 2, 3 * pi / 2, 0, tau,
        0, pi, -pi / 2, 3 * pi / 2, 14, 4, 0,
    ]))
    u0, u1 = (14 * angle for angle in major)
    v0, v1 = (4 * angle for angle in minor)
    vertices = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)]
    edges = [(0, 1), (1, 2), (3, 2), (0, 3)]
    for a, b in edges:
        x0, y0 = vertices[a]
        x1, y1 = vertices[b]
        control, values = (1, [x0, y0, y1]) if x0 == x1 else (2, [y0, x0, x1])
        frame(3, 0x18, bytes([5, control]) + doubles(values))
    for i, (a, b) in enumerate(edges):
        frame(3, 0x5E, bytes([1, child(8 + i, 12 + a), child(8 + i, 12 + b), 1, 1, 1]))
    for _ in vertices:
        frame(3, 0x5D, bytes([1]))

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


if __name__ == "__main__":
    fixtures = {
        "torus-trim-domains.catpart": build(),
        "torus-active-ranges.catpart": build((2.0, 2.5), (0.2, 0.6)),
        "torus-outside-major-domain.catpart": build((7.0, 7.5)),
        "torus-outside-minor-domain.catpart": build(minor=(5.0, 5.5)),
    }
    for name, data in fixtures.items():
        Path(__file__).with_name(name).write_bytes(data)
