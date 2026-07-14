# Creo Parametric `.prt` (PSB): Open Items

This document records unresolved PSB byte semantics outside [creo_prt.md](creo_prt.md).

## Geometry

- The inherited-slot state transitions in curve-equation `local_sys f9 04 03` bodies are unspecified.
- The remaining lane-specific DICT sign lattices are unspecified.
- Per-instance overrides for cone half-angle and `geom_type = 26` torus/sphere radii are unspecified.
- Packed B-spline surface bodies and surface-intersection curves are unspecified.
- The remaining `fc` curve-body grammars are unspecified, including `fc 05` variants, `fc 08`, `fc 13` field roles, `fc 02` slot semantics, and `fc 04`, `fc 07`, `fc 09`, and `fc 0a`.
- Rotational-sweep angular termination fields are unspecified; the recipe discriminator and resolved axis do not define one-sided, symmetric, two-sided, or full-turn travel.
- Model-space analytic equations for non-plane surface rows are unspecified, including cylinder axis and location, cone apex and axis, and the `geom_type = 29` fillet/spline split.
- Round and fillet byte semantics are unspecified, including non-prismatic radii, flank geometry, and generated face bindings.
- The negative DICT prefix lattice for scalar lanes that block geometry records is unspecified.

## Topology and coordinates

- The DEPDB fields binding feature recipes and sparse edge records into body topology are unspecified.
- The byte-backed outer/inner loop discriminator for multi-loop faces is unspecified.
- Fields binding vertex identifiers to XYZ coordinates and rowless face uses are unspecified.
- Section-to-datum joins, relation equations other than signed type-zero linear dimensions and type-14 radii, `skamp_ptr` incidence types other than 0 through 4 and type 14, and the `ed ba 10 0c 8d ee 90 b4 0c` solver sentinel are unspecified.
- The entity/locus roles of the three decoded four-slot `relat_ptr` operand vectors are unspecified.
- The DEPDB sketch-datum and sweep-axis relation for parts without `ActDatums` is unspecified, including the feature-definition datum defaults or standard-datum convention that supplies the `protextrude` axis.
- Sketch-datum resolution without a unique generated-datum parent-table remainder is unspecified, including selection of a perpendicular orienting datum when the nested reference datum is parallel to the sketch normal.
- In named `ActDatums` outline slots outside the paired standalone-zero axis slots, the value semantics of `18 <index>`, `a5`, `9f`, `5c`, and `45` are unspecified. Their values determine nonzero datum offsets and extents.
- The direction-bit rule assigning pcurve endpoint pairs A and B to traversal start and end is unspecified, as is the partition of shared surface references into face instances.
- Bindings for rowless face-use references outside the round-feature rowless-cylinder table are unspecified.
- Positional-replay field alignment for non-class-913 edge-treatment schemas is unspecified.
- The byte-backed relation that assigns shells to body identifiers when face-adjacency components and body-count fields disagree is unspecified.
- Face-instance bindings for `element_colors`, `NeuPrtSld`, and display-table elements are unspecified.
- The remaining RGB and component scalar lanes used by appearance records are unspecified.
- The suppression and deletion meanings of `MdlStatus` `o`, `x`, and `y` state prefixes are unspecified.

## Packed persistence data

- Geometry encoding in packed `VisibGeom`, `SolidPrimdata`, `SolidPersistTable`, and `DEPDB_DATA` bodies is unspecified.
- The `DispDataTable` compressed-stream variant is unspecified, including its initial dictionary state and geometry bindings.
- Traversal and row semantics of the configuration driver table referenced by a non-null `FamilyInf.drv_tbl_ptr` are unspecified.
