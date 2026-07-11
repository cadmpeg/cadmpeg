# cadmpeg-codec-rhino

`cadmpeg-codec-rhino` identifies Rhino `.3dm` files by their embedded
`3D Geometry File Format ` signature.

Inspection and decoding are not implemented yet. The crate is registered with
the cadmpeg CLI so `.3dm` inputs can be selected explicitly and detected by
content.

Requires Rust 1.88 or later. Licensed under Apache-2.0.
