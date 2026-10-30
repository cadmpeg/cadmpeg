<!-- SPDX-License-Identifier: CC-BY-4.0 -->

# OpenUSD layers: Open Items

This document lists the parts of the OpenUSD layer encodings and schemas that we do not know. The specification `usd.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Attribute names and token values are technical names.

## 1. Schemas

### USD-01. Trim-loop region rule

**Question.** Which part of the `(u, v)` domain of a `NurbsPatch` do the trim loops keep: the region inside an odd number of loops, or the region that the loop orientations select?

**Known.** `usd.md` §14.1 gives the trim-curve encoding. The UsdGeom schema states that the encoding follows the RenderMan `RiTrimCurve` call, and gives no region rule or loop orientation.

**Need.** We must know the rule to map trim loops to outer and inner face loops, and to write face loops as trim loops that other applications read with the same region.

## 2. BrepArray

### USD-02. Trim curve of an edgeuse

**Question.** How does a curve of `BrepCurveUvNurbAPI` name the edgeuse that it trims?

**Known.** `usd.md` §17.1 states that curve `i` is the trim curve of edgeuse `i` when the API holds one curve for each edgeuse. The schema gives the number of curves and no edgeuse index. The proposal text states that an edgeuse stores the index of its trim curve. When the API is absent, or its count differs from the edgeuse count, the decoder projects each edge curve onto the surface of each face that uses it.

**Need.** We must read a prim whose trim curves cover some edgeuses only.

### USD-03. Radial entry type

**Question.** Which side of its face does `edgeuse:thisRadialEntryType` give, relative to the right-hand traversal about the edge?

**Known.** `usd.md` §17.2 states that `topEntry` is entry from the `same` side. The decoder builds the radial order from the edges and does not read the token. The encoder writes `topEntry` for an `opposite` edgeuse and `bottomEntry` for a `same` edgeuse, which is the side that a right-hand traversal about the edge direction enters when the face is to the left of its loop seen from `N`.

**Conflict.** In the proposal's cube example, the faces have outward surface normals and the same loop rule, but edgeuses 0 and 6 (`same`) are `topEntry` and edgeuse 5 (`same`) is `bottomEntry`. No one rule about the edge direction gives these three tokens.

**Need.** We must write the token that other readers expect, and read non-manifold edges whose radial order the token sets.

### USD-04. Loop direction

**Question.** Does a loop go counterclockwise about the surface normal `N`, or about the normal of one faceuse?

**Known.** `usd.md` §17.2 states that the outer loop goes counterclockwise seen from the side toward `N`. The proposal text gives no loop direction. Each loop of the proposal examples goes counterclockwise about `N`.

**Need.** We must read faces whose surface normal points into the solid region.

### USD-05. Outer shell of the outside region

**Question.** Which shell is the outer shell of a void region that no shell encloses, such as the region outside a B-rep with two solid lumps?

**Known.** `usd.md` §17.1 states that the first shell of a region is its outer shell. Each proposal example has one shell in that region. The encoder writes the shell of each solid lump first, then the sheet shells.

**Need.** We must read B-reps with more than one lump in one region.

### USD-06. Front side of a sheet face

**Question.** Which faceuse of a sheet face gives the side that the face's source model calls its front?

**Known.** `usd.md` §17.2 defines a sheet face. The encoder writes the faceuse of the front side first. The decoder reads the first faceuse of a sheet face as its front side.

**Need.** We must keep the face sense of a closed sheet, which other formats state.

### USD-07. Counts of faces, edges, and vertices of a B-rep

**Question.** Does a B-rep state the counts of its faces, edges, and vertices?

**Known.** `usd.md` §17.1 partitions regions, shells, faceuses, loops, and edgeuses by count arrays. The `BrepArray` documentation names `brep:edgeCount` and a start shell index, and the schema has no such attribute. The decoder finds the faces of a B-rep from its faceuses, and the edges and vertices from the faces.

**Need.** We must read a B-rep that has wire edges or point shells and no faces.

### USD-08. Subset element types

**Question.** Are `face` and `brep` element types of `GeomSubset` on a `BrepArray` prim, and which family names go with them?

**Known.** `usd.md` §17.4 gives the proposal examples, which bind materials through subsets with `elementType = "face"` and `elementType = "brep"`. `UsdGeomSubset` registers neither token for a `BrepArray`. The decoder reports these subsets as `usd/brep.subset-not-transferred`.

**Need.** We must read face and B-rep appearances.
