# cadmpeg-codec-freecad

Pure-Rust read/write support for ZIP-packaged FreeCAD `.FCStd` documents. The codec preserves
format-native metadata in the `fcstd` namespace, applies checked semantic property and side-entry
edits, and builds source-less schema-4/file-1 application graphs.

The current typed transfer includes exact text and binary B-rep geometry/topology, persistent
element-name bindings, sketches and constraints, attachments, datum frames, expressions and
spreadsheets, and an expanding construction history. Neutral operations include extrusions,
revolutions, dress-ups,
analytic primitives, booleans, lofts, sweeps, thickness, draft, branch-complete holes, and
datum-resolved linear, polar, or mirror patterns.
Pattern configurations requiring nonuniform spacing or multiple linear directions remain linked
native records until the neutral IR can express their complete transform sequence. The manifested
public corpus establishes L9 tested for the documented schema-4/file-1 envelope. The generated
support profile writes and decodes every fixture, verifies semantic equivalence, typed mutation,
unsupported-record survival, deterministic source-less generation, and explicit target-version
refusal. An independent FreeCAD interoperability check is provided in
`tools/validate_fcstd_interop.py`.
