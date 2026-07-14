# cadmpeg-codec-freecad

Pure-Rust read support for ZIP-packaged FreeCAD `.FCStd` documents. The crate is input-only and
preserves format-native metadata in the `fcstd` namespace.

The current typed transfer includes exact text and binary B-rep geometry/topology, persistent
element-name bindings, and a design-history tracer for planar sketch line/circle/arc geometry plus
pad, pocket, and linear-extrusion features. Unresolved application properties remain linked native
records; this subset does not constitute an L4 support claim.
