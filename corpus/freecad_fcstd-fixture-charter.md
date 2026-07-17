# FCStd public fixture charter

Fixtures used for a tested or proven FCStd support claim must be independently authored and
released under CC0. Each donation records the author, license grant, generator version, structural
profile, SHA-256 digest, and the semantic cases intentionally present. Synthetic parser inputs and
fuzz seeds do not establish a ladder score.

The corpus must include GUI and headless documents; thumbnails present and absent; stored,
deflated, ZIP64, and data-descriptor archives; and text and binary exact-shape entries. The matrix
must vary optional persistent element maps and string-hasher tables independently.

Geometry coverage includes primitive, analytic, Bezier, NURBS, trimmed, offset, swept, revolved,
and degenerate carriers. Topology coverage includes solids, sheets, wires, compounds, compsolids,
multiple shells, voids, seams, degenerate edges, and non-manifold sharing.

Design coverage includes sketches, every constraint and dimension family, expressions,
attachments, representative Part and PartDesign histories, suppression and visibility states,
groups, parts, links, link arrays, external references, assemblies, and joints.

Document coverage includes materials, per-body and per-face appearance, annotations,
spreadsheets, TechDraw pages and views, embedded files, Mesh, Points, FEM, CAM, and inert
Python-backed application data. Extension objects must exercise retained identity, properties,
links, XML, and side-entry payloads.

Negative fixtures cover duplicate and unsafe ZIP names, encryption, invalid CRC and sizes,
truncation, malformed XML and B-rep records, invalid references, excessive nesting, expansion
ratio, entry count, aggregate allocation, and runtime limits.

The generated corpus profile crosses schema/file version and side-entry layout with semantic
domains. It reports fixture and assertion counts for every row in
`docs/formats/freecad_fcstd-coverage.md`; filenames alone are not coverage evidence.
