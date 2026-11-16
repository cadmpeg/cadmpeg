# Synthetic torus chart fixtures

These hand-encoded decoder fixtures use records specified in
`docs/formats/catia.md`. The generator and fixtures are licensed Apache-2.0.
Run `python3 generate.py` in this directory to reproduce their bytes.

Each file is 812 bytes and contains one sheet face with four native edges.
The torus has center (1, 2, 3), major radius 7 mm, minor radius 2 mm,
U scale 14, and V scale 4. The major active interval is [π/2, 3π/2]
inside domain [0, 2π]. The minor active interval is [0, π]
inside domain [−π/2, 3π/2].

| Fixture | Major trim angles | Minor trim angles | Expected bounded faces |
| --- | --- | --- | --- |
| `torus-trim-domains.catpart` | [0.25, 1.0] | [−0.6, −0.2] | 1 |
| `torus-active-ranges.catpart` | [2.0, 2.5] | [0.2, 0.6] | 1 |
| `torus-outside-major-domain.catpart` | [7.0, 7.5] | [−0.6, −0.2] | 0 |
| `torus-outside-minor-domain.catpart` | [0.25, 1.0] | [5.0, 5.5] | 0 |

The first fixture exercises trim coordinates beyond both active ranges and
inside both full domains. The other fixtures check the active-range case
and rejection outside either full domain.

Run the decode regressions from the repository root with
`cargo test -q -p cadmpeg-codec-catia --lib -- torus_sheet`.
