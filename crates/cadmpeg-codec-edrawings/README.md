# cadmpeg-codec-edrawings
`cadmpeg-codec-edrawings` inspects SolidWorks eDrawings `.eprt` (part) and
`.easm` (assembly) packages for [`cadmpeg-ir`][ir]. A package is a ZIP archive
whose `eModel` member holds a HOOPS Stream Format model.

<!-- generated: capability edrawings -->

Support: L2 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#solidworks-edrawings-eprt-and-easm)).

<!-- /generated: capability edrawings -->

The codec is read-only. It detects a package by its `eModel` member, lists
every member with its role, and classifies the package dialect from the stream
version in the `eModel` comment line. Decode opens the package, inflates the
model stream under the decode caps, and walks its records with exact length
rules. The stream holds display meshes only, so decode rebuilds each body as a
faceted B-rep (planar faces, straight edges) and reports the loss
`edrawings/geometry.reconstructed-from-mesh`. A part is one part definition.
An assembly is part definitions shared by their instances, group definitions
for the assembly and its sub-assemblies, and an occurrence tree with the
absolute placement of each instance in millimetres.

## Install
```sh
cargo add cadmpeg-codec-edrawings cadmpeg-ir cadmpeg-core
```

[ir]: https://docs.rs/cadmpeg-ir
