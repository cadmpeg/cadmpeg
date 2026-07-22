# Creo Parametric `.prt` (PSB): Open Items

This document records unresolved PSB byte semantics outside [creo_prt.md](creo_prt.md).

## Geometry

- Curve-relation custom unit symbols, including custom units requiring an
  affine offset, have unspecified normalization semantics.
- Curve-equation `local_sys f9 04 03` inherited-slot transitions other than
  the defined rank-two body are unspecified.
- `crv_pnt_arr f9 02 04` inherited-slot bodies outside the direct eight-slot
  pcurve form have unspecified slot transitions.
- DICT sign lattices outside the defined scalar lanes are unspecified.
- `double_xar` slot bodies other than the defined literal and recursive
  placeholder images are unspecified, including variable-length `e5` forms.
- Per-instance cone half-angle bodies outside the terminal positive-DICT form
  and `geom_type = 26` torus/sphere radius bodies outside the tagged radius
  trailer are unspecified.
- Positional plane-envelope scalar prefixes outside the defined row lane are
  unspecified.
- The joins from later positional spline rows to their prototype data and from
  spline surfaces to surface-intersection curves are unspecified.
- The prototype-adjacent `tab_cyl` instance rows use a construction distinct
  from the repeated cubic replay; its point and parameter fields are unspecified.
- Replay-bound `tab_cyl` frames whose axis spans do not uniquely match the two
  directrix-coordinate ranges have an unspecified placement variant.
- The remaining `fc` curve-body grammars are unspecified, including `fc 05` variants, `fc 08`, `fc 13` field roles, `fc 02` slot semantics, and `fc 04`, `fc 07`, `fc 09`, and `fc 0a`. The decoded `fc 13` body contains repeated full sample groups followed by a shortened held-coordinate-plus-two-field terminal form; whether that form is a final sample or a trailer is unspecified.
- The equations relating `MdlRefInfo` conic types other than the defined
  type-30 ellipse to `t0`, `t1`, `c1`, `c2`, and `local_sys` are unspecified,
  including parabola and hyperbola carrier parameters.
- Rotational-sweep angular termination selectors other than the defined
  full-turn `angle_choice` form are unspecified, including one-sided,
  symmetric, and two-sided travel.
- Model-space analytic equations for remaining non-plane surface rows are
  unspecified, including positional cylinder variants outside the defined
  local-system, compact axis-aligned, referenced planar-envelope, and held-axis
  axial/radial, and repeated-diameter bodies and positional cone variants
  outside the defined support-apex suffix and planar-envelope bodies.
- The three-byte station token between a positional cone's model-reference
  token and half-angle has unspecified scalar semantics.
- Remaining round and fillet byte semantics are unspecified, including
  non-prismatic radii, flank geometry, and generated face bindings.
- The negative DICT prefix lattice for scalar lanes that block geometry records is unspecified.

## Topology and coordinates

- The DEPDB fields binding feature recipes and sparse edge records into body topology are unspecified.
- The byte-backed outer/inner loop discriminator for multi-loop faces is unspecified.
- Fields binding vertex identifiers to XYZ coordinates and rowless face uses are unspecified.
- Section-to-datum joins, relation equations other than signed type-zero linear
  dimensions and the defined type-five and type-14 radii, type-one angular
  relation direction selectors, `skamp_ptr` incidence types 10 through 13 and
  15, type-35 operands that do not resolve through a section entity, and the
  `ed ba 10 0c 8d ee 90 b4 0c` solver sentinel are unspecified.
- The geometric roles and selection order of multiple feature-definition `local_sys` and `transf` twelve-slot frames are unspecified.
- The entity/locus roles of the three decoded four-slot `relat_ptr` operand vectors are unspecified.
- The join from a `var_arr` value using the dimension-driven scalar sentinel to
  the dimension relation that drives that solver variable is unspecified.
  `uvar_id`, point key, relation identifier, relation dimension selector, and
  external dimension identifier occupy distinct namespaces.
- The semantics of the multi-valued `relat_ptr` `used` field are unspecified.
  It is solver state, not a Boolean constraint-activation flag.
- The geometry families and external-reference bindings of solver-only
  `skamp_ptr` entity identifiers are unspecified.
- The owner and namespace joins that expose model, feature, component, and
  scoped dimension items beyond decoded local `d<external_id>` identities to
  curve-expression `exists()` queries are unspecified.
- The DEPDB sweep-axis relation for parts without `ActDatums` is unspecified,
  including the feature-definition datum defaults or standard-datum convention
  that supplies the `protextrude` axis. The current regeneration snapshot is
  unspecified when several section definitions select the same internal
  sketch-plane entity.
- Sketch-datum resolution without a unique generated-datum parent-table remainder is unspecified, including selection of a perpendicular orienting datum when the nested reference datum is parallel to the sketch normal.
- In named `ActDatums` outline slots, the value semantics of `a5`, `9f`, `5c`,
  and `45` are unspecified. Their values determine nonzero datum offsets and
  extents.
- The partition of shared surface references into face instances is unspecified.
- The referents and traversal roles of `lo_restore` `direction` and
  `direction2` compact integers are unspecified.
- Bindings for rowless face-use references outside the round-feature rowless-cylinder table are unspecified.
- Positional-replay field alignment for non-class-913 edge-treatment schemas is unspecified.
- The byte-backed relation that assigns shells to body identifiers when face-adjacency components and body-count fields disagree is unspecified.
- Face-instance bindings for `element_colors`, `NeuPrtSld`, and display-table elements are unspecified.
- The remaining RGB and component scalar lanes used by appearance records are unspecified.
- The remaining stored-name meanings of `MdlStatus` `o`, `x`, `y`, and `z`
  prefixes are unspecified. They do not select the current same-ID state.

## Packed persistence data

- Geometry record semantics in packed `VisibGeom`, the `SolidPrimdata`
  triangle-strip continuation and its persistent-segment bindings,
  expanded `SolidPersistTable`, and `DEPDB_DATA` bodies are unspecified.
- The `DispDataTable` compressed-stream variant is unspecified, including its initial dictionary state and geometry bindings.
- Traversal and row semantics of the configuration driver table referenced by a non-null `FamilyInf.drv_tbl_ptr` are unspecified.
