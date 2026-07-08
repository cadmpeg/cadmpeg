# Creo Parametric `.prt` (PSB): Open Items

This document records PSB semantics outside [creo_prt.md](creo_prt.md).

## Geometry

- Define the general byte-0 to high-exponent mapping for eight-byte world-coordinate tokens and the remaining DICT sign lattices.
- Define per-instance overrides for cone half-angle and `geom_type = 26` torus/sphere radii.
- Define the packed B-spline surface bodies and surface-intersection curves.
- Define the remaining `fc` curve-body grammars, including `fc 05` variants, `fc 08`, `fc 13` field roles, `fc 02` slot semantics, and `fc 04`, `fc 07`, `fc 09`, and `fc 0a`.
- Define the binding from each `surface_of_extrusion` face to its sweep feature and direction.
- Define the exact model-space analytic equations for non-plane surface rows, including cylinder axis and location, cone apex and axis, and the `geom_type = 29` fillet/spline split.
- Define round and fillet evaluation, including non-prismatic radii, flank geometry, and generated face bindings.
- The negative DICT prefix lattice for scalar lanes that block geometry records is unspecified.

## Topology and coordinates

- Define body reconstruction for DEPDB feature recipes and sparse edge records.
- Define multi-loop outer/inner classification where parameter-space containment is unavailable.
- Define general vertex XYZ bindings and rowless face-use bindings.
- Define section-to-datum joins, relation-backed coordinates, and the `ed ba 10 0c 8d ee 90 b4 0c` solver sentinel.
- Define the DEPDB sketch-datum and sweep-axis resolver for parts without `ActDatums`, including the feature-definition datum defaults or standard-datum convention that supplies the `protextrude` axis.
- Define the binding between material/display elements and exact face references.
- The `AllFeatur.dtm_id_tab` and feature-graph join from a `gsec3d` outer `plane_id` to the sketch datum row are unspecified, including selection of a perpendicular orienting datum when the resolved reference is parallel to the sketch normal.
- The scalar grammar for cache-indexed named `ActDatums` outlines is unspecified, including `18 <index>` and the `a5`, `9f`, `5c`, and `45` token forms. Nonzero datum offsets and extents require this grammar.
- Pcurve A/B endpoint roles for oriented coedges and the partition of shared surface references into face instances are unspecified.
- Bindings for rowless face-use references outside the round-feature rowless-cylinder table are unspecified.
- Positional-replay field alignment for edge-treatment rows after the labeled template row is unspecified.
- The byte-backed relation that assigns shells to body identifiers when face-adjacency components and body-count fields disagree is unspecified.
- Face-instance bindings for `element_colors`, `NeuPrtSld`, and display-table elements are unspecified.
- Define the remaining RGB and component scalar lanes used by appearance records.

## Packed persistence data

- Define the geometry encoding in packed `VisibGeom`, `SolidPrimdata`, `SolidPersistTable`, and `DEPDB_DATA` bodies.
- The `DispDataTable` compressed-stream variant is unspecified, including its initial dictionary state and geometry bindings.
