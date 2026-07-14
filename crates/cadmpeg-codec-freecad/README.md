# cadmpeg-codec-freecad

Pure-Rust read support for ZIP-packaged FreeCAD `.FCStd` documents. The crate is input-only and
preserves format-native metadata in the `fcstd` namespace.

The current typed transfer includes exact text and binary B-rep geometry/topology, persistent
element-name bindings, sketches and constraints, attachments, expressions and spreadsheets, and
an expanding construction history. Neutral operations include extrusions, revolutions, dress-ups,
analytic primitives, booleans, lofts, sweeps, and representable uniform linear or polar patterns.
Pattern configurations requiring nonuniform spacing or multiple linear directions remain linked
native records until the neutral IR can express their complete transform sequence. This evolving
subset does not yet constitute an L8 support claim.
