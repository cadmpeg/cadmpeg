# cadmpeg-codec-usd

`cadmpeg-codec-usd` decodes and encodes OpenUSD layers for [`cadmpeg-ir`][ir].

<!-- generated: capability usd -->

Support: L2 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#openusd-layers)).

<!-- /generated: capability usd -->

A layer is a USDA text file or a USDC binary file; the first bytes select the
encoding. The codec classifies the layer from its header version. A USDC layer
is identified and refused. A USDA layer is parsed and composed within itself:
selected variants, internal references and payloads, inherits, specializes,
`over`, `class`, `active`, and instancing. Arcs to other layers stay external
prototypes, and sublayers are not composed.

## Decode

| USD | `cadmpeg-ir` |
| --- | --- |
| scene prim (`Xform`, `Scope`, untyped, gprims) | product definition and occurrence |
| transform-op stack | occurrence placement, translation in millimetres |
| instance prims of one prototype | shared definitions |
| reference or payload to another layer | occurrence of an external prototype |
| `Mesh` | general body and tessellation, fan-triangulated |
| face `GeomSubset` | tessellation triangle group |
| `BasisCurves`, `NurbsCurves` | wire body: NURBS curve carriers, edges, vertices |
| `NurbsPatch` | sheet body: one face on a NURBS surface |
| constant `primvars:displayColor` | body color |
| `UsdPreviewSurface` material | appearance bound to the tessellation |
| `metersPerUnit`, `upAxis`, `defaultPrim` | source attributes; lengths in millimetres |

Every construct with no neutral form is named by a `usd/*` loss.

## Encode

The writer emits a USDA layer in millimetres with one root prim per root
occurrence. Each occurrence becomes an `Xform` prim with a `matrix4d` placement, and a
definition placed more than once is written once under the abstract
`Prototypes` prim. Tessellations become `Mesh` prims, faces on NURBS surfaces
of untessellated bodies become `NurbsPatch` prims, wire edges and free curves
become `NurbsCurves` prims, and appearances become `UsdPreviewSurface`
materials. What the layer cannot carry is reported as a `usd/export.*` loss.

## Specification

[`docs/formats/usd.md`](../../docs/formats/usd.md) specifies the encodings and
the schemas. The USDA grammar follows the published OpenUSD text-format
grammar; OpenUSD is open-source software from Pixar under the Tomorrow Open
Source Technology License.

[ir]: https://docs.rs/cadmpeg-ir
