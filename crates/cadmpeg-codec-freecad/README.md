# cadmpeg-codec-freecad

Pure-Rust read support for ZIP-packaged FreeCAD `.FCStd` documents. The crate is input-only and
preserves format-native metadata in the `fcstd` namespace.

The current typed transfer includes exact text and binary B-rep geometry/topology, persistent
element-name bindings, sketches and constraints, attachments, datum frames, expressions and
spreadsheets, and an expanding construction history. Neutral operations include extrusions,
revolutions, dress-ups,
analytic primitives, booleans, lofts, sweeps, thickness, draft, branch-complete holes, and
datum-resolved linear, polar, or mirror patterns.
Pattern configurations requiring nonuniform spacing or multiple linear directions remain linked
native records until the neutral IR can express their complete transform sequence. The manifested
public corpus establishes L8 tested for the documented schema-4/file-1 read envelope; the generated
support profile evaluates every gate cumulatively and prevents isolated extras from inflating the score.
