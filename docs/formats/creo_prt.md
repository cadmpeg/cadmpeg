# Creo Parametric `.prt` (PSB): Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

This specification covers the Creo Parametric and Pro/ENGINEER `.prt` variant. Creo files use the PSB (Pro/E Session Binary) container.

## 1. Container

A PSB file begins with an ASCII UGC header and table of contents, followed by named binary sections.

```text
#UGC:2 P ...
...
#-END_OF_UGC_HEADER\n
#UGC_TOC ...
#END_OF_TOC_HEADER\n
#<SectionName>\n <payload>
```

The header record `#- CMNM <hhh><name>` stores the native model filename.
`hhh` is a three-digit ASCII hexadecimal byte count for `name`; padding after
those bytes is not part of the name.

A body-section header has the byte sequence `#\n#<name>\n`. The preceding section ends at the `#` byte before that sequence. Section names are complete printable runs. A decoder must require both the preceding `#` terminator and a printable name when locating a section boundary. ND-layout section names may include an `ND:0:<Name>:N` decoration or a `ModelView#N` suffix.

The ordered section directory stores each validated section's normalized name,
raw decorated name, semantic role, header offset, and byte length. It enumerates
decoded and opaque model data, auxiliary assets, and the thumbnail without
interpreting payload bytes as additional directory entries.

PSB does not use the Parasolid neutral-binary encoding. Parasolid terminology may describe some geometric concepts, but it does not define PSB byte semantics.

### 1.1 Layout families

| Layout |            Section count | Geometry representation                                               |
| ------ | -----------------------: | --------------------------------------------------------------------- |
| ND     | approximately 40 or more | Dense PSB rows in `VisibGeom`, including `srf_array` and `crv_array`. |
| DEPDB  |         approximately 12 | Sparse PSB views and feature/section records.                         |

### 1.2 Section map

| Section                          | Contents                                                                                                             |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `VisibGeom`                      | Visible PSB geometry. ND files store dense geometry rows here.                                                       |
| `NovisGeom`                      | Invisible and construction PSB geometry.                                                                             |
| `AllFeatur`                      | Feature rows, generated-entity tables, affected-geometry identifiers, feature references, and DEPDB section recipes. |
| `FeatDefs`                       | Feature definitions, section recipes, placement records, outlines, dimensions, and saved section entities.           |
| `Geomlists`                      | Body-count and quilt-discriminator fields.                                                                           |
| `ActDatums`                      | Active datum-plane geometry under `act_datum_geoms → srf_array`.                                                     |
| `DEPDB_DATA`                     | Persistence data used by DEPDB-layout parts, including embedded geometry namespaces and feature-definition records.  |
| `FamilyInf`                      | Family-table driver pointer for configurations.                                                                       |
| `NeuPrtSld` and display sections | Material, appearance, display, and tessellation data.                                                                |
| `THMB_IMG_MAIN`                  | JPEG thumbnail. The payload begins with `FF D8 FF` and does not contain model geometry.                              |

### 1.3 Units

`_principal_sys_units_id` identifies the active coordinate unit system.

| Value | Unit system                         |
| ----: | ----------------------------------- |
|  `51` | millimeter-Newton-Second (`mmNs`)   |
|  `55` | millimeter-Kilogram-Second (`mmKs`) |

Unit-definition records can include inactive units. `history_scale` is a version/history array and does not scale coordinates.

## 2. PSB primitive encoding

### 2.1 Compact integers

| Bytes       | Meaning                                                    |
| ----------- | ---------------------------------------------------------- |
| `00..7f`    | One-byte direct integer.                                   |
| `80..bf XX` | Two-byte big-endian integer: `((head - 0x80) << 8) \| XX`. |
| `c0..ff`    | Control or special-token range on typed paths.             |

Reference identifiers use a narrower grammar in `srf_array` row identifiers, `crv_array` suffixes, DEPDB suffixes, and terminator validation:

| Bytes       | Meaning                                                  |
| ----------- | -------------------------------------------------------- |
| `00..7f`    | Identifier in `[0, 127]`.                                |
| `80..bf XX` | Canonical two-byte identifier with value at least `128`. |
| `c0..ff`    | Invalid reference-identifier start byte.                 |

In `segtab`, `order_table`, and `ent_tab`, bytes `c0..ff` are single-byte null sentinels. `f6` does not begin a two-byte compact integer in those lanes.

### 2.2 Structural tokens

| Token                | Meaning                                                 |
| -------------------- | ------------------------------------------------------- |
| `e0 <type> <name>\0` | Named-record header.                                    |
| `f8 <count>`         | Array opener.                                           |
| `f9 <ndim> <count>`  | Count-bounded scalar body.                              |
| `f7 <id>`            | Entity reference.                                       |
| `fb`                 | Array close.                                            |
| `e2`                 | Nested compound-body opener or continuation.            |
| `e3`                 | Compound close or row terminator, depending on context. |
| `e1 e3`              | Short `crv_array` row terminator.                       |
| `e1 f5 05 f6 e3`     | Long `crv_array` row terminator.                        |

### 2.3 Scalar tokens

PSB scalar forms reconstruct IEEE-754 `double` bytes.

#### Three-byte IEEE-fill form

`<prefix> XX YY` reconstructs a double from `(byte0, XX, fill...)`.

| Prefix | `byte0` | Fill                  |
| ------ | ------- | --------------------- |
| `29`   | `3F`    | `YY` repeated 6 times |
| `2a`   | `3F`    | `YY 00 00 00 00 00`   |
| `2e`   | `40`    | `YY` repeated 6 times |
| `2f`   | `40`    | `YY 00 00 00 00 00`   |
| `42`   | `BF`    | `YY` repeated 6 times |
| `43`   | `BF`    | `YY 00 00 00 00 00`   |
| `47`   | `C0`    | `YY` repeated 6 times |
| `48`   | `C0`    | `YY 00 00 00 00 00`   |

Examples: `2f 43 00 = 38.0`, `2f 20 00 = 8.0`, `48 22 00 = -9.0`, `29 eb 33 = 0.85`.

#### Seven-byte DICT form

`<prefix> <tail6>` uses the prefix to select the first two IEEE bytes and uses the six tail bytes as the mantissa tail. In the positive DICT lane:

```text
byte1 = (prefix - 0x8B) & 0xFF
byte0 = 0x3F when byte1 >= 0x80, otherwise 0x40
```

Known prefixes include `71→3F E6`, `74→3F E9`, `76→3F EB`, `81→3F F6`, `8b→40 00`, `90→40 05`, `91→40 06`, `a1→40 16`, `a2→40 17`, and `b7→3F E4`. The negative saved-spline tangent form `b3` maps to `BF E0`. In the `var_arr` coordinate lane, `d7` is the sign counterpart of `90` and maps to `C0 05 <tail6>`.

The `var_arr` coordinate lane also defines the sign pairs
`7e→3F F3`/`c6→BF F3`, `80→3F F5`/`c8→BF F5`, and
`97→40 0C`/`dd→C0 0C`. Each prefix is followed by the remaining six IEEE
bytes. Its negative sub-unit form `d5 <tail6>` reconstructs
`BF <tail6> 00`. Its eight-byte world-coordinate form `2d <tail7>`
reconstructs `40 <tail7>`; the same form is positive in the saved-section
coordinate lane.

Lane-specific seven-byte forms include `6a <tail6>` for positive IEEE with leading byte `40` and implicit trailing `00`; `9e <tail6>` and `a3 <tail6>` for positive and negative forms paired with the section-local `46` cache; `b9`, `d1`, `d3`, `de`, and `df` for negative sub-unit forms with leading byte `BF`; and `41`, `4b`, `66`, `67`, `68`, `77`, and `82..8f` for positive sub-unit forms with leading byte `3F`. A paired form finds the `46 <byte1> <tail6>` token with the same six-byte tail and reconstructs `40 <byte1> <tail6>` for `9e` or `C0 <byte1> <tail6>` for `a3`.

In positional surface and curve row lanes, `71 <tail6>` is a seven-byte
sub-unit form reconstructed as `3F <tail6> 00`. In named scalar lanes, `71`
occupies eight source bytes and reconstructs as `3F <tail7>`.
In a positional surface row, `a0 <tail6>` is the negative DICT form
`C0 15 <tail6>`.
The positional surface-row lane defines the same-tail sign pair
`73→3F E8` and `bb→BF E8`.
The positional surface-row lane maps `d1`, `d3`, `de`, and `df` to IEEE
prefixes `3F FF`, `40 01`, `40 10`, and `40 11`, respectively.

Each record grammar defines the DICT lane for its scalar slots. A decoder must not apply DICT sign rules across unrelated record grammars.

#### World-coordinate tokens

World-coordinate tokens normally occupy eight bytes. Their final seven bytes hold the IEEE mantissa and low exponent. In the positional-outline/world lane, `46` denotes a positive token and `2d` denotes a negative token; `2d` consumes the complete eight-byte token in that lane. A field-specific compact world lane stores a negative coordinate as `2d <tail6>`, reconstructed as `C0 <tail6> 00`. The enclosing field frame distinguishes the seven-byte and eight-byte forms; the surface family does not.

#### Constants and cache references

`0d` encodes negative one, `0f` and `e6` encode zero, and `e4` encodes one. In row and `f9` scalar lanes, `e8 00` encodes standalone `1.0`; other contexts use a different selector grammar. `18 <index>` indexes a raw section-local `46` cache. Build that cache by scanning the raw section bytes, including `46` values that occur within other token tails. In a row or `f9` body, `18 <float-opener>` encodes a standalone zero and the following byte begins a new token. In a saved-line coordinate row, `18` immediately before the row close or trailing entity reference is a standalone zero. At the byte-bounded end of a positional scalar-slot array, terminal `18` is a standalone zero.

The following token may itself begin with `18`. In a positional surface row,
the surface-only `73`, `a0`, and `bb` openers also terminate the preceding
standalone `18` zero.

The seven-byte scalar `5e b2 b3 b4 b5 b6 b7` reconstructs IEEE-754 bytes `3f d3 b2 b3 b4 b5 b6 b7`.

## 3. Surface namespace: `srf_array`

`srf_array` provides surface and face-reference identifiers.

`VisibGeom` is the material model-geometry namespace when it contains
`srf_array` or `crv_array`. `NovisGeom` is a separate invisible and construction
namespace and its identifiers do not join the visible namespace. `DEPDB_DATA`
supplies the model-geometry namespace only when no visible geometry namespace
is present and the DEPDB payload contains an `srf_array` or `crv_array` label.
An unlabeled persistence payload does not define geometry rows.

| Item                  | Rule                                                                                |
| --------------------- | ----------------------------------------------------------------------------------- |
| Count header          | `srf_array\0 f8 <count>`                                                            |
| ND count              | Count from the selected geometry payload.                                           |
| DEPDB count           | Sum `srf_array` counts across concatenated geometry subsections.                    |
| Positional row header | `<geom_id_ci> <geom_type> <feat_id_ci> <orient> <boundary_type> <next_geom_ptr_ci>` |
| Orientation bytes     | `01`, `f6`                                                                          |
| Boundary bytes        | `00`, `01`, `06`, `f6`                                                              |

A counted surface-array frame ends at the next `srf_array`, `crv_array`,
`lo_array`, or `qlt_array` label. Header-shaped bytes outside that frame do not
belong to it. A byte range owned by a bounded named prototype parameter cannot
start a sibling surface row. The frame materializes only when the number of
unique validated rows equals its stored count; a count mismatch leaves the
frame opaque.

A positional surface parameter body ends at its compound close, the next validated surface-row header, or a named-record header. A named-record boundary has `e0`, a field-type byte in `00..24`, a nonempty ASCII identifier beginning with a letter, and a null terminator. An `e0` byte inside an opaque numeric or pointer token is not a boundary.

Row bodies end at a valid row-close marker, named-record header, or a following positional row header that matches the row schema. Scalar-token length takes precedence over structural-byte interpretation, so an `e3` byte inside a complete scalar does not close the row. The first row after `srf_array\0` can be a named-record row with the fields `geom_id`, `geom_type`, `feat_id`, `orient`, `boundary_type`, `next_geom_ptr`, `envlp`, `outline`, and `local_sys`.
`geom_id` is unique within one selected namespace. Multiple header-shaped byte
sequences carrying the same identifier are ambiguous and are not surface rows.
A nonzero `next_geom_ptr` may reference a rowless face use, so materialization of
its target is not a row-acceptance condition.
Plane envelope and post-envelope local-system bodies use the same grammar for
each defined boundary byte; `boundary_type` does not select their scalar layout.

### 3.1 Surface families

| `geom_type` | Surface family                                   |
| ----------- | ------------------------------------------------ |
| `22`        | Plane                                            |
| `24`        | Cylinder                                         |
| `25`        | Cone                                             |
| `26`        | Torus or sphere representation                   |
| `28`        | Spline surface                                   |
| `29`        | Fillet surface                                   |
| `2a`        | Linear-extrusion family, `ruled_srf` variant     |
| `2c`        | Linear-extrusion family, `tab_cyl` variant       |

A decoder must not infer the kind of a row without a materialized parameter row from adjacent rows or topology.

### 3.2 Surface prototypes

`srf_prim_ptr` records contain the surface prototype fields. The prototype block closes with `f1 f7 <entity_ref> e3`. A scalar field ending with bare `18` before that structural close stores zero.
The family name inside `srf_prim_ptr(<family>)` is retained independently of
the normalized surface family; `tab_cyl` and `ruled_srf` remain distinct names.

| Prototype                                             | Named fields                                                    |
| ----------------------------------------------------- | --------------------------------------------------------------- |
| `srf_prim_ptr(plane)`                                 | `local_sys f9 04 03`, envelope, and domain fields               |
| `srf_prim_ptr(cylinder)`                              | `local_sys f9 04 03`, `radius`                                  |
| `srf_prim_ptr(cone)`                                  | `local_sys f9 04 03`, `half_angle`                              |
| `srf_prim_ptr(torus)`                                 | `local_sys f9 04 03`, `radius1`, `radius2`                      |
| `srf_prim_ptr(fillet_srf)`                            | Nested spline, tangent, flip, and `i_pnts` fields               |
| `srf_prim_ptr(tab_cyl)` and `srf_prim_ptr(ruled_srf)` | Local-system, curve/spline, parameter, and control-point fields |

Named `i_pnts` and `c_pnts` fields inside a nested curve record following a
torus prototype belong to that curve, not to the analytic torus prototype.

A nested `curve(b_spline)` record uses compact integers for `id`, `type`,
`tan_cond`, and `degree`. Its `params` array and `c_pnts` reference array are
independent fields. A `c_pnts` body `f8 <count> f7 <start_id> fb` denotes the
contiguous entity-reference range `start_id .. start_id + count`. `flip`
retains its typed-wrapper bytes. `dum_array`, `data_dbls`, and `data_type` are
separate named fields. A count-prefixed compact-integer array is typed as such
only when exactly the declared number of compact integers consumes the entire
bounded field body; trailing bytes make the field opaque.

Named prototype fields describe the first surface instance. The first instance is the adjacent same-family positional surface row. The preceding adjacent row is the first instance when the prototype separates it from replay rows; otherwise the following adjacent row is the first instance. Positional row bodies carry the per-instance values for subsequent instances.

In the ND layout, a complete plane or torus prototype `local_sys` and family parameters define the first instance carrier. Slots 0 through 2 contain the first support direction. In the rank-two compressed form slots 3 through 5 are zero and slots 6 through 8 contain the second support direction. A torus prototype can instead store its second support direction directly in slots 3 through 5. Slots 9 through 11 contain the origin in either complete form. The normalized cross product of the two orthogonal, equal-scale support directions is the analytic axis. A bare terminal `18` in the bounded `local_sys` body occupies one zero slot. A plane passes through the local-system origin, uses the analytic axis as its normal, and uses the first support direction as its parameter-space reference direction. A zero torus `radius1` and positive `radius2` define a sphere centered at the local-system origin. Positive `radius1` and `radius2` define a torus with respective major and minor radii centered at that origin.

Cylinder and cone prototype local systems are parameter templates. Their terminal
triples do not establish model-space origins. Cylinder and cone carriers require
their positional construction or a feature placement.

The next valid named field or the enclosing `e3` compound close terminates a named prototype field, whichever occurs first. A named-field header has a field type no greater than `24` and a nonempty identifier made from ASCII letters, digits, underscores, or parentheses. An `e0` byte inside a scalar token does not terminate the field. Bytes after the structural close belong to subsequent instance or namespace records.
A parenthesized `srf_prim_ptr(<family>)` record also ends at the next legacy
`srf_prim_ptr\0` record. Fields owned by that sibling prototype do not belong
to the parenthesized record. A following top-level `entity_ptr(<family>)`
record also ends the prototype; its named fields belong to that peer entity.

`radius`, `radius1`, `radius2`, and `half_angle` are scalar-typed fields. A body that does not complete a scalar token remains opaque and is not reinterpreted as a compact integer.

Positional cylinder rows store cap-plane point data rather than a `local_sys` replay. Their per-instance radius does not inherit the prototype default; derive it from bound `fc 05` cap-circle geometry or from a byte-backed analytic construction.

A `tab_cyl` prototype can carry `i_pnts`, `end_tangts`, and `params` as
separate named fields. `params` uses `f8 <count>` and contains exactly `count`
curve parameters. Its `2d <tail7>` form reconstructs `40 <tail7>`. The `params`
header terminates the preceding `end_tangts` body even when the preceding
terminal `18` zero slot causes the generic token walk to span the header. A
terminal `18` in the bounded `end_tangts` body occupies one zero slot.
`end_tangts` uses the signed coordinate DICT lattice defined for the second
directrix-coordinate lane.

The direction/directrix form of a `geom_type = 2c` positional body begins with
a three-scalar model-space sweep-direction frame followed by the bytes
`00 0c 9a`. The directrix construction begins after this marker. Replay-bound
rows carry a six-scalar frame after the marker; that frame does not contain two
straight-directrix endpoints. An optional terminal `f7` entity reference
follows the frame. In a row without a cubic replay, the six-scalar frame stores
the start and end XYZ points of a straight directrix. A nonzero sweep direction
and nondegenerate straight directrix define an unbounded plane.

A repeated `tab_cyl` cubic-curve replay has this structure:

```text
<curve_id_ci> 13 e2 01 00 03
18 e6 0f e6
f8 04 f7 <control_point_0_ref> fb e2
f7 <successor_ref> <point_0_body>
18 f1 f7 <control_point_0_ref> e2 <point_1_body>
18 e2 <point_2_body>
18 e2 <point_3_body>
18 f2 f7 <terminal_ref> f6 e3
```

`13` is the curve type, `01` is the flip byte, `00` is the tangent condition,
and `03` is the cubic degree. The `f8 04` field names four contiguous control
point entities beginning at `control_point_0_ref`. The four packed point bodies
are bounded by the reference-bearing first separator, exactly two middle
separators, and the reference-bearing terminal trailer. A replay belongs to
the nearest preceding `geom_type = 2c` surface row after the previous replay
signature. Intervening rows from other surface families do not consume it.
Ambiguous separators or a missing unique owner leave the bytes opaque.
Each packed point body contains two directrix coordinates. A control point is
numeric only when two defined scalar tokens consume its entire bounded body;
partial scalar matches do not assign either coordinate.
In the first-coordinate lane, prefixes `5b..a3` use the positive DICT mapping.
Negative prefixes `b2..cf`, `d0..dc`, `dd`, and `de..df` derive their two
leading IEEE bytes by adding the prefix to `BF2D`, `BF2E`, `BF2F`, and `BF32`,
respectively. Negative prefixes `a5..a6` and `a7..ae` add to `BF2B` and `BF2C`.
Prefixes `2c`, `4e..4f`, `52`, `54`, and `58..5a` reconstruct
`3F <tail6> 00`; `45` reconstructs `BF <tail6> 00`.
The fixed-width forms are `28 <tail7> → 3F <tail7>`,
`2d <tail7> → 40 <tail7>`, `31 <tail6> → 40 <tail6> 00`,
`41 <tail7> → 3F <tail7>`, `46 <tail7> → C0 <tail7>`, and
`4a <tail6> → C0 <tail6> 00`.
In the second-coordinate lane, prefixes `5c` and `5e..a3` use the positive DICT mapping.
Negative prefixes `a4..a6`, `a7..b1`, and `b2..c7` add to `BF2B`, `BF2C`, and
`BF2D`. Prefixes `c8..cf`, `d0..dc`, `dd`, and `de..df` add to `BF2D`,
`BF2E`, `BF2F`, and `BF32`, respectively. Prefixes `2c`, `4c..4d`, `50`, and `54` reconstruct
`3F <tail6> 00`; `45` reconstructs `BF <tail6> 00`; `28` and `41`
reconstruct `3F <tail7>`.

A replay-bound six-scalar frame stores two opposite corners of the directrix
and extrusion bounds. Slots zero and three use the first directrix-coordinate
lane, slots two and five use the second directrix-coordinate lane, and slots one
and four store the sweep bounds. In a first-coordinate frame slot,
`4a <tail6>` reconstructs as the positive `40 <tail6> 00` exception. When exactly two
frame-axis spans equal the ranges of the first and second control-point
coordinates, those axes define the directrix chart. Each directrix axis is a
signed unit-slope affine map selected by the frame bounds and the layout's
required intercept magnitude. A missing or non-unique map leaves the frame
opaque. The remaining axis defines the extrusion vector. The four placed
points form a non-rational clamped cubic B-spline with knot vector
`[0,0,0,0,1,1,1,1]`.

The `_ 46 2f _ 46 2e` layout requires a first-axis intercept magnitude of 30,
a zero second-axis intercept, and retains the stored sweep-axis sign. The
`_ 42 7f..86 _ 18 7f..86` layout requires zero intercepts and retains the
stored sweep-axis sign. Its first and fourth slots accept the complete
first-coordinate scalar lane. In the `_ 2d _ _ 2d _` layout, slots one and
four also use the first-coordinate lane. Its directrix charts select exactly
one of two forms: a zero-offset form retaining the sweep-axis sign, or a
first-axis intercept magnitude of 30 with a zero second-axis intercept and a
reflected sweep-axis sign. A missing or non-unique form leaves the frame opaque.
Other six-scalar sequences after the marker are not directrix envelopes.

Cone `half_angle` uses the positive DICT rule and is expressed in radians. Valid values lie in `(0, pi/2)`.

### 3.3 Torus and sphere representation

A `srf_prim_ptr(torus)` prototype stores `e1[3], e2[3], e3[3], origin[3], radius1, radius2`. A sphere uses `radius1 = 0` and radius `radius2`; a torus uses nonzero `radius1`. Per-instance row-body overrides use a separate grammar.

In named `radius`, `radius1`, and `radius2` fields, compact tokens `0d` and
`0e` encode the positive values `0.25` and `0.5`, respectively. These tokens
belong to the positive radius lane; their generic signed-scalar meanings do not
apply.

Named prototype `local_sys f9 04 03` coordinate slots use the signed
directrix-coordinate DICT lattice and fixed-width coordinate forms. Stock-vector
and zero macros retain their local-system expansion rules. Generic positional
row scalar mappings do not apply to these slots.

Compact token `0e` encodes positive `0.5` in a named prototype local-system
coordinate slot. Its negative positional-row meaning does not apply.

Within a `geom_type = 26` positional row, `2d b1 b2 b3 b4 b5 b6` immediately
before a structural control byte or the bounded body end is a seven-byte
negative coordinate token. Its value is the big-endian IEEE-754 binary64 image
`c0 b1 b2 b3 b4 b5 b6 00`. The trailing low byte is implicit; the structural
control byte is not part of the scalar. An unframed `2d` scalar retains the
generic eight-byte form.

Decoded positional parameter scalars retain their source offset and token length. Structural field binding uses these spans; scalar order alone does not assign frame or radius roles.
The unresolved seven-byte `73` and `bb` forms retain their exact bytes as one
scalar slot. Bytes inside either token cannot open another scalar or terminate
the row.
Each bounded positional body transfers to the Creo native
`surface_parameters` arena with its surface identifier, family, boundary kind,
exact body bytes, ordered decoded or opaque scalar slots, and maximal opaque
spans covering every byte outside those slots. Scalar frames are the maximal
contiguous scalar-token sequences in byte order. The terminal scalar frame is
the final frame only when it ends at the body boundary.

Spline and fillet prototypes can carry `i_points`, `tangts`, `end_tangts`,
`end_u_tangts`, `end_v_tangts`, `end_uv_deriv`, `u_params`, `v_params`,
`ctr_spline`, `tan_spline`, `par_v_0`, `par_v_1`, and `offset_type` named
fields. An `f9 <dimensions> <count>` field declares exactly
`dimensions * count` scalar slots and retains unresolved slots in position.
`u_params` and `v_params` can instead use `f8 <count>` followed by exactly
`count` scalar slots; unresolved slots retain their declared positions.

In the spline point and derivative fields, `dimensions` is the number of
three-coordinate vectors and `count` is three. Vectors are serialized
consecutively. Each declared slot consumes one complete scalar token; an
unresolved seven-byte DICT token remains one opaque slot and its payload is not
searched for nested scalar openers. `i_points` uses eight-byte `28` and `41`
positive sub-unit forms in addition to eight-byte `2d`/`46` world coordinates,
the positive DICT lattice, and the `b3`/`b9` negative forms. `end_v_tangts`
uses the signed coordinate DICT lattice defined for the second directrix
coordinate lane. `u_params` and the seven-byte `v_params` forms use the
positive DICT lattice. `v_params` also uses the eight-byte `28` positive
sub-unit form.

A complete `splsrf` interpolation surface contains `i_points`,
`end_u_tangts`, `end_v_tangts`, `end_uv_deriv`, `u_params`, and `v_params`.
If `u_params` has `U` values and `v_params` has `V` values, `i_points` contains
`U * V` vectors in u-major order. `end_u_tangts` contains the `V` derivatives
at the lower-u boundary followed by the `V` derivatives at the upper-u
boundary. `end_v_tangts` contains the `U` derivatives at the lower-v boundary
followed by the `U` derivatives at the upper-v boundary. `end_uv_deriv`
contains the lower-u and upper-u mixed derivatives at the lower-v boundary,
then the corresponding pair at the upper-v boundary.

Both parameter arrays are strictly increasing. Each direction is a clamped
cubic interpolation basis. Its control count is the sample count plus two; its
full knot vector repeats the first parameter four times, contains each interior
sample parameter once, and repeats the final parameter four times. Position,
endpoint first-derivative, and corner mixed-derivative equations determine the
non-rational tensor-product control net. The stored points and derivatives are
model-space values.

### 3.4 Planes

Plane row bodies contain envelope/domain data, `local_sys f9 04 03`, and a row/topology tail. `local_sys` has twelve scalar slots:

```text
slots 0..2    first in-plane support direction
slots 3..5    [0, 0, 0] rank-2 marker
slots 6..8    second in-plane support direction
slots 9..11   support-frame origin
```

When the rank-2 guard holds, derive the normal as:

```text
normal = normalize(cross(slots[0..2], slots[6..8]))
```

The guard requires orthogonal, equal-scale nonzero support directions. `outline f9 02 03` stores two XYZ corners. In these positional scalar lanes, `73` and `bb` each begin a seven-byte scalar token. Repeated identical tokens denote equal stored values; tokens with different prefixes denote distinct values. Token equality remains defined when the scalar magnitude is not decoded.

When exactly one coordinate is held constant across both corners, its axis is the positive basis normal and its value is the model-space plane offset. The other two coordinate pairs need only be known to be distinct; their magnitudes are not required. Zero or multiple held coordinates do not establish a plane equation from the outline.

For a generated section plane selected through the parent-datum rule, multiple
held envelope coordinates are filtered against the orientation plane. The
unique perpendicular held axis defines the section plane.

For an axis-aligned plane, the held-coordinate outline defines the placed plane
equation. An axis-aligned `local_sys` support frame without that outline does not
establish the model-space offset outside its generating feature.
When an axis-aligned `local_sys` normal selects an outline coordinate whose two
stored tokens are equal, that coordinate supplies the plane offset. Equality of
the other outline coordinates need not be resolved because the support frame
already fixes the plane orientation.
A shortened standard outline can store the four bound scalars and first XYZ
corner followed by one terminal scalar token. The terminal token occupies the
coordinate selected by the axis-aligned support-frame normal. It establishes
the held coordinate when its exact token image equals that coordinate's token
in the first corner; the other two coordinates of the second corner are absent.

A `crv_array` edge whose two face references resolve to nonparallel placed
planes has the exact model-space carrier given by their intersection line. Its
direction is the normalized cross product of the plane normals; its origin is
the minimum-norm point satisfying both plane equations.

A topological vertex orbit with three linearly independent placed incident
planes is their unique intersection point. Additional incident placed planes
must contain the same point; otherwise the orbit has no placed vertex.

## 4. Curve namespace: `crv_array`

`crv_array` provides edge identifiers, half-edge topology, type bytes, and pcurve records.

| Item                   | Rule                                                           |
| ---------------------- | -------------------------------------------------------------- |
| ND count               | `crv_array\0 [f3] f8 <count>`                                  |
| DEPDB count            | `crv_array\0 f2 f8 <count>`                                    |
| Positional row header  | `<crv_id_ci> <type_byte> <feat_id_ci> <dir0_flag> <dir1_flag>` |
| Standard suffix        | `[F0, F1, E0, E1]` before `00 00 e3`                           |
| DEPDB one-sided suffix | `[0, X1, F1, 0]`; `127` terminates `X1`                        |
| Row terminators        | `e1 e3` or `e1 f5 05 f6 e3`                                    |

`F0` and `F1` reference faces in the `srf_array` namespace. `E0` and `E1` reference the next edge for the two half-edge sides. The suffix graph defines half-edges, loops, coedges, shells, and vertex orbits when both sides are present. `crv_pnt_dir` is a per-side orientation-flag array, not a tangent vector.

The raw `type_byte` does not by itself identify a curve family.

The parameter body is the byte range after the two direction flags and before
the selected four-reference suffix. Its scalar walk retains each decoded token
with body-relative offset, length, and exact bytes. Canonical `f7` entity
references retain the same span data. Maximal bytes claimed by neither class
form opaque spans, so the three span sets partition the complete body.

### 4.1 Pcurve endpoints

A curve body consisting of exactly eight scalar tokens and no reference or
opaque spans has this layout. Its values are parameter coordinates in the
corresponding face spaces.

| Slots  | Meaning                            |
| ------ | ---------------------------------- |
| `0..1` | Endpoint A in face `F0` parameters |
| `2..3` | Endpoint A in face `F1` parameters |
| `4..5` | Endpoint B in face `F0` parameters |
| `6..7` | Endpoint B in face `F1` parameters |

A trailing `18` after an eight-slot body supplies the final zero slot. `crv_pnt_arr f9 02 04` stores the same layout.

### 4.2 `fc` curve bodies

Non-eight-slot curve bodies begin with `fc <subtype>`. The subtype selects a body-grammar class.

| Subtype | Body family                              |
| ------- | ---------------------------------------- |
| `fc 02` | Short pcurve-style endpoint record       |
| `fc 05` | Cap-circle arc record family             |
| `fc 08` | World-coordinate control-polyline family |
| `fc 13` | Held-cap-ordinate control polyline       |

`fc 05` records store cap-circle control points in the order `A`, `B`, `t`, `C`, where `A` and `C` use eight-byte world-coordinate tokens and `B` and `t` use DICT or standalone-zero scalar tokens. `C` is the owning cylinder's axis-placement ordinate. The adjacent plane supplies the cap circle's axial coordinate. `t` is the angular curve parameter in radians. The signed relation between successive polar angles and `t` determines curve sense; subtracting the signed stored parameter from a point's polar angle determines the parameter-zero radial direction. For a model-X axis, `(A, B, C)` maps to `(Z, Y, X)`; for a model-Y axis it maps to `(X, Z, Y)`; for a model-Z axis it maps to `(Y, X, Z)`. The row-frame radial vector `(A, B)` maps to `(0, B, A)`, `(A, 0, B)`, or `(B, A, 0)`, respectively. `fc 13` stores a control polyline rather than an analytic circle.

An `fc 05` cap-circle body consists of complete four-scalar point groups after
the `fc 05` prefix followed by the single-byte `ff` body terminator. A body
without the terminator can end immediately after the final group. Other
unclaimed trailing bytes invalidate the analytic circle carrier.

Recognized eight-byte `46` and `2d` world-coordinate tokens in an `fc` body
retain their decoded millimeter value, exact bytes, body-relative offset, and
token length. Bytes between recognized tokens remain owned by the enclosing
curve parameter body as maximal opaque spans. The coordinate-token and opaque
span sets partition the complete retained body. Scalar order does not assign
point or parameter roles.

Within the `fc 05` scalar lane, `8b <tail6>` reconstructs the IEEE-754 bytes `40 00 <tail6>` and consumes seven stored bytes. This lane-specific interpretation takes precedence over the context-independent `8b` scalar form.

An `fc 05` cap pair belongs to one cylinder when each curve suffix binds one
side to the same `geom_type = 24` face and the other side to a `geom_type = 22`
face. The records must have equal radii and equal in-plane centers at distinct
constant cap ordinates. This binding establishes the cylinder radius and its
axis line in the owning feature's row frame. Model-space placement additionally
requires that feature's row-frame transform.

When both cap-plane outlines establish parallel axis-normal planes, the axis
direction, coordinate permutation, and cap offsets supply that transform
directly.

Each participating `fc 05` curve is a circle centered at the shared in-plane
center and its own transformed cap ordinate, with the cylinder axis and radius.
The curve identifier remains the `crv_array.crv_id`.

One `fc 05` curve bound to one cylinder face and one resolved axis-normal cap plane independently defines both its model-space circle and the cylinder carrier. The cap plane supplies the model-space axial coordinate. The in-plane fitted center, signed parameter sense, parameter-zero radial direction, and fitted radius define the remaining cylinder placement and radius. The cylinder axis passes through the cap-circle center.

## 5. Topology and section records

Build the B-rep half-edge graph from the `crv_array` suffixes. A single-loop face has an outer boundary by topology. Multi-loop faces require parameter-space containment to distinguish outer from inner loops. Shells follow connected components of face references.

Use the following order to select a body count:

1. A positive `Geomlists.n_bodies` value.
2. `Geomlists.first_quilt_ptr == 0` as a single-body discriminator.
3. Face-reference adjacency component count when it is the only byte-backed source.

ND layouts share `var_arr`, `segtab`, `order_table`, `ent_tab`, and `vert_tab`, joined by `ext_id`.

| Table         | Semantics                                                                                                                              |
| ------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `var_arr`     | Solved-coordinate table keyed by `pointid`; `type=1` is `u`, `type=2` is `v`; `value` is solved and `guess` is the pre-solve estimate. |
| `segtab`      | Two-dimensional segments; `type=2` is LINE and `type=3` is ARC. A line uses `f6` as its null `cntrid`; an arc uses a center `pointid`. |
| `order_table` | Generated-entity ordering table.                                                                                                       |
| `ent_tab`     | Trimmed profile entity chain.                                                                                                          |
| `vert_tab`    | Trim vertices and their two incident `segtab` entities.                                                                                |
| `relat_ptr`   | Counted sketch-constraint relations. The `f8` allocation count includes two structural entries; exactly `count - 2` positional rows follow the schema close. Each row ends at `e2` and stores `id`, `used`, three four-slot operand vectors `a`, `b`, `c`, then `sign`, dimension selector, and relation-type discriminator. |
| `skamp_ptr`   | Counted solver-incidence rows. Each row stores `id`, `type`, `flags`, `status`, and a counted ordered array of section-entity `ent_id`/`sense` pairs. |
| `triples_ptr` | Counted joins from relation and equation identifiers to `skamp_ptr` incidence identifiers. Each of the three fields independently admits the `f6` null sentinel. |

The first `var_arr` row is the named field prototype between the table header
and schema close. It is a data row and contributes to the declared count;
positional replay rows follow the close.
The `f8` count is the exact total row count; bytes following that many rows do
not belong to `var_arr`.

`skamp_ptr` accepts the table wrappers `f1`, `f3`, and `f4 05`. Its named row
is the first counted row. Positional rows repeat the nested item schema for the
first item, then store additional `ent_id`/`sense` pairs directly; `e2`
separates direct items when the item count exceeds two. The row trailer is
`f3` plus the table entity reference plus `e2`; a one-item row instead ends at
its item `e2`, and the final row may end at the following named record. Solver
integer fields extend the compact-integer lattice with `c0..df XX YY`, equal
to `((head-c0)<<16)|(XX<<8)|YY`.

For a two-item type-zero incidence, sense `2` selects the native first endpoint
and sense `3` selects the native second endpoint; the selected loci coincide.
When exactly one `segtab` row owns each referenced external identifier, this
incidence equates the corresponding stored `pointid` coordinates. A solved
coordinate on either endpoint therefore supplies the missing coordinate on the
other endpoint; conflicting solved coordinates remain distinct.
For an arc or circle operand, sense `4` selects its center. A type-14 incidence
stores a symmetry axis as a sense-zero line followed by two point loci selected
with senses `2`, `3`, or `4`. A type-3 incidence between a sense-zero entity
and a sense-`2`, sense-`3`, or sense-`4` point locus makes the entity and locus
coincident.
When the sense-zero entity is a `segtab` point, its `pointid` coordinate equals
the selected endpoint or arc-center `pointid` coordinate. Solved coordinates
propagate across that equality under the same unique-row and conflict rules as
type zero.
A two-item type-9 incidence with sense zero on one line and one point makes the
point coincident with the line.
A two-item sense-zero line incidence makes the lines perpendicular for type 5,
parallel for type 7, and equal in length for type 8.
A two-item type-6 incidence with sense zero on two arcs or circles makes their
radii equal. A solved positive radius propagates through the connected radius
component. A solved arc center and endpoint supply their Euclidean distance as
the radius. Conflicting solved radii leave the component unresolved.
For an `arcorient = 0` arc these map to the neutral end and start loci,
respectively, because the analytic arc orientation is reversed. A two-item
type-four incidence makes the referenced entities tangent at their selected
endpoint loci.
A two-item type-three incidence has one sense-zero point entity and one
endpoint-selected entity; the point and endpoint loci coincide.
A one-item type-one incidence makes the referenced entity horizontal. A
one-item type-two incidence makes the referenced entity vertical.
Stored horizontal/vertical selectors and unique type-one/type-two incidences
define the line's held `v`/`u` coordinate, respectively. For type three or type
nine, a selected point on such a line inherits that held coordinate from either
line endpoint. The equality propagates in either direction and does not
overwrite conflicting solved coordinates.
Type-five and type-seven line incidences propagate perpendicular and parallel
orientation, respectively, through their connected line component. A
contradictory incidence cycle or conflicting stored or unary orientation leaves
the component orientation unresolved.
A three-item type-fourteen incidence stores a sense-zero line followed by two
endpoint-selected loci. The loci are symmetric about the line, in stored order.
When the axis is uniquely horizontal or vertical and its held coordinate is
solved, one solved locus determines the other by copying the coordinate along
the axis and reflecting the perpendicular coordinate through the axis.
An incidence item may reference a complete saved-section entity through its
`order_table.ext_id`. When its type/sense pattern has no neutral constraint
mapping, retain the incidence type, ordered entity identifiers, and sense values
as one native sketch constraint; the absence of a typed locus interpretation
does not remove the solver relation. `relat_ptr`, `skamp_ptr`, `triples_ptr`,
`order_table`, and saved-section entities remain valid when `segtab_ptr` is
absent; segment-dependent refinement is withheld without dropping those design
records.
For an ordered saved line, senses `2` and `3` select its first and second stored
endpoints. For an ordered saved arc they select the neutral end and start loci,
respectively, because saved-arc evaluation reverses the stored endpoint order.
Sense-zero saved lines participate in type-one horizontal, type-two vertical,
type-five perpendicular, type-seven parallel, type-eight equal-length, and
type-fourteen symmetry-axis incidences through their `order_table` external
identifier under the same arity rules as `segtab` lines.

The first `triples_ptr` row is named and contributes to its declared count.
Positional rows contain `rel_id`, `eqn_id`, and `skamp_id` followed by `e2`;
the last row may terminate directly at the next structural or named record.
A relation joined to exactly one incidence through `rel_id` and `skamp_id`
inherits that incidence's ordered section-entity references and locus senses.
When the incidence contains exactly two items whose senses resolve to section
loci, those loci define the measured endpoints in stored order. This join is
independent of whether the relation discriminator has a neutral typed mapping.
A type-zero relation with sign zero, one, or `f6`, a defined `dimtab_ptr`
selector, and a two-locus joined incidence is the Euclidean distance between the
joined loci. A nonempty incidence without exactly two resolved loci remains an
entity-level distance. The more specific operand-vector and `verhor` forms below
refine that distance to horizontal or vertical endpoint loci; incomplete operand
vectors do not discard the incidence-backed distance.

Within the three four-slot `relat_ptr` operand vectors, `e5` expands to two
zero slots and `e6` expands to three zero slots. `e4` is the integer value one,
and `f6` is a null operand. Expansion is bounded independently at four slots
for each of `a`, `b`, and `c`.

For a type-zero linear-distance relation with operand-vector forms
`a = [point0, point1, null, 1]`, `b = [0, 0, 0, 0]` or
`b = [1, 1, 0, 1]`, and
`c = [15, 16, 15, 1]`, the referenced dimension supplies the distance between
the two points along the measured horizontal or vertical segment. Sign `1`
adds the dimension and sign `f6` subtracts it. Sign zero selects the segment
direction: first-direction `1` adds the dimension, while the null
first-direction selector subtracts it.

A type-zero relation with vectors `a=[first_point,second_point,null,1]`,
`b=[0,0,0,0]` or `b=[1,1,0,1]`, and `c=[15,16,15,1]` is a
segment-aligned linear dimension.
Its dimension selector is a zero-based index into `dimtab_ptr`. `verhor=1`
selects the section `u` difference and `verhor=0` selects the section `v`
difference. Sign `1` defines `second-first=+value`; sign `f6` defines
`second-first=-value`; sign zero stores only the unsigned magnitude.
The two point identifiers denote endpoint loci shared by every incident
`segtab` entity. The selected `dimtab_ptr` row is the driving parameter for the
horizontal or vertical distance constraint independently of whether both
endpoint coordinates are evaluated.

A type-14 relation with `a=[radius_id,0,0,0]`, `b=[0,0,0,0]`,
`c=[15,0,0,0]`, and sign `1` binds the selected dimension value to the
type-three `var_arr` radius with that key. An arc's `radius` field selects the
same radius key. The solved center point and positive radius define its
unbounded circular carrier before both arc endpoints are available.
The selected dimension is the neutral radius constraint parameter of the arc
whose `radius` field names that key.

The named `segtab` row before its schema close is likewise a data row. Its `type`, `dir`, `pointid`, `cntrid`, `arcorient`, `verhor`, radius, and `ext_id` fields contribute one segment to the declared table count.
Positional rows may insert the two-byte `c0 80` wrapper before `type`. The
wrapper does not change the following field layout. A compact `ext_id` value of
zero is an identifier; the `f6` control sentinel represents an absent value.
Segment type `5` is an isolated point entity. It stores one defined `pointid`;
the second point slot is a control sentinel.

An arc radius is the distance from its center to an endpoint in `var_arr`. A trim-vertex identifier is distinct from a `segtab` point identifier.

For `arcorient = 0`, an arc traverses clockwise from its first endpoint to its
second endpoint about `cntrid`. In a counterclockwise angular
parameterization, its start is the second endpoint angle and its end is the
first endpoint angle advanced by full turns until it exceeds the start. Its neutral curve orientation is therefore opposite the `ent_tab` start-to-end orientation.

`gsec2d_ptr.dimtab_ptr` stores ordered feature dimensions. Each row contains
`type`, `value`, `direct`, `aux_value`, and `ext_id`; type `0x0a` is an angular
dimension whose `value` is in radians. Types `0x01`, `0x02`, `0x03`, `0x04`,
and `0x05` are linear dimensions whose values use model millimeters. `ext_id` is the dimension identity
within the owning feature definition. Neutral parameter identity includes the
feature-definition identifier, owning model-feature identifier, and `ext_id`;
different definitions may reuse the same local `ext_id`. In positional dimension rows, a bare
`18` in the `aux_value` slot encodes zero and does not consume the following
compact `ext_id`.
Type `0x03` has radius display semantics.

A `segtab` line whose two endpoint identifiers each have complete type-1 and
type-2 `var_arr` values is the bounded segment between those two `[u, v]`
points. It is construction geometry when its `ext_id` is absent from
`ent_tab`.
Every `segtab` row remains a section design entity when its carrier coordinates
are incomplete; incomplete coordinates affect evaluation, not entity identity
or attached constraints.
For relation-backed endpoint ordinates, `dir[0] = 0` and two equal defined
endpoint `u` values define a vertical carrier; `dir[1] = 0` and two equal
defined endpoint `v` values define a horizontal carrier. The carrier remains
unbounded until the trim-vertex graph supplies both endpoints.
The `verhor` value is also an equality constraint between the corresponding
endpoint ordinates: value `0` equates `u`, and value `1` equates `v`. A defined
ordinate therefore supplies the same ordinate for the other endpoint when its
`var_arr` value is dimension-driven.

The `ent_tab` start and end vertex identifiers orient each trimmed entity.
Connected components of this incidence graph are profile chains. A component
is closed when every vertex has degree two and open when exactly two vertices
have degree one; any other degree pattern is not a profile chain.

When `ent_tab` is absent, emitted line and arc `segtab` rows use their two
`pointid` values as the incidence graph. A connected component is a profile
loop only when every point has degree two and traversal consumes every row and
returns to its starting point. Open, branched, isolated, and incompletely
decoded components remain construction geometry. For `arcorient=0`, profile
traversal reverses the analytic arc when it runs from the first `pointid` to
the second.

In a round-feature generated-entity table, a rowless face-use entry is a cylinder only when the table's following materialized `srf_array` entry is a cylinder. The two entries are angular sectors of one oriented cylinder; the rowless face use inherits the materialized sibling's carrier and orientation. The table class token alone does not identify the surface kind.

A cylinder coaxial with a torus intersects it in one tangent circle when the cylinder radius equals the torus outer radius `R + r` or its positive inner radius `|R - r|`. The circle lies in the torus central plane, has the common axis, and has the cylinder radius. Other coaxial radii produce multiple or no circle components and remain unresolved without a native branch binding.

A sphere whose center lies on a torus axis reduces their intersection to two circles in the axial meridian plane: one centered on the axis with the sphere radius and one centered at the torus major radius with the tube radius. External tangency or non-concentric internal tangency of those meridian circles produces one point with positive radial coordinate and therefore one model-space circle about the torus axis. Secant meridian circles produce multiple model-space circles and remain unresolved without a native branch binding.

Two externally or non-concentrically internally tangent spheres have one common
point on their center line. That point is a unique topological vertex when it
also lies on every other incident carrier; it is not a zero-radius curve.
A plane tangent to a sphere likewise contributes its projected contact point
to vertex incidence without creating a zero-radius circle.

Two coaxial tori reduce their intersection to their tube circles in a shared axial meridian plane. External tangency or non-concentric internal tangency of the tube circles produces one point with positive radial coordinate and therefore one model-space circle about the common axis. Secant tube circles produce multiple model-space circles and remain unresolved without a native branch binding.

A circular cone and a coaxial sphere intersect in one circle when substitution of the cone radial function into the sphere equation produces one repeated axial root. For cone radius `r0`, slope `k = tan(a)`, and sphere center at axial coordinate `c` from the cone origin, the axial equation is `(1 + k²)t² + 2(r0 k - c)t + r0² + c² - Rs² = 0`. A zero discriminant gives the single tangent circle at axial coordinate `t`; its radius is `|r0 + kt|`. Positive discriminants produce two circles and remain unresolved without a native branch binding.

A plane through a circular cone's apex is tangent to the cone when the absolute dot product of their unit normal and axis equals the sine of the cone half-angle. Their intersection is the single generator through the apex in the projection of the cone axis onto the plane. A plane normal to the cone axis intersects it in one circle away from the apex. Substitution of an oblique plane basis into the cone equation yields a diagonal quadratic whose signs distinguish ellipse, parabola, and hyperbola carriers. Completing the square gives the conic center or vertex, in-plane principal direction, radii, and parabola focal distance. A plane through the apex that cuts two generators produces a degenerate conic and remains unresolved without a native branch binding.

## 6. Features and datums

`MdlStatus` names encode feature kinds as `<Kind> id <N>`. Defined names include
`Annotation Feature`, `Cross Section`, `Datum Plane`, `Round`, `Chamfer`,
`Protrusion`, `Extrude`, `Revolve`, `Hole`, `Cut`, `Draft`, `Mirror`, and
`Surface`. The German operation-family names `Bezugsebene` and `Rundung`
denote the same datum-plane and round families as `Datum Plane` and `Round`,
respectively. `Annotation Feature` is a non-modeling annotation container.
`Cross Section` and its German operation-family name `Querschnitt` are
non-modeling cross-section definitions. `Mirror` identifies a reflection
operation.

Operation names end in ` id <N>` or ` ID <N>`; the stored case follows the
name's localization. An ASCII `o`, `x`, `y`, or `z` byte immediately preceding
an uppercase operation-family name is a state prefix, not part of the family
name. Multiple operation names with the same feature identifier are ordered
stored states; the last occurrence is the current state. Decoding the current
state does not discard the preceding state records. State ordinals are local to
one feature identifier and increase in byte order from zero. A stored state
retains the prefix-inclusive name bytes, the `id`/`ID` spelling, and the offset
of the optional prefix; a recipe-only state has no stored operation name.

Within one current-state record, `protextrude` identifies an additive linear
section sweep, `cutextrude` identifies a subtractive linear section sweep,
`protrevolve` identifies an additive rotational section sweep, and
`cutrevolve` identifies a subtractive rotational section sweep. The recipe
name precedes the `<Kind> id <N>` operation name and applies to that feature
state.
DEPDB stores the same join in
`f7 <record-ref> <feature-id> <schema-class> f6 <parent-id> <display-name> 00 f6 00 <recipe> 00`.
The feature identifier owns the operation even when no localized `ID <N>` name
is present. When such a name is present, the shared feature identifier decorates
the recipe operation with that display name. The record reference, feature
identifier, schema class, and parent identifier are compact integers.

A `feat_defs_<id>` record-name identifier in `FeatDefs` or `DEPDB_DATA` belongs
to the feature-definition record namespace. In a labelled definition,
`e0 01 feat_id 00 <canonical-reference> e0 00 gsec2d_ptr 00` identifies the
owning modeling feature and joins `MdlStatus` and `AllFeatur`; `f6` in this slot
is null. When `feat_id` is null, the unique `DatumIds` generated table
containing the section's `sketch_plane_entity_id` identifies the owning
modeling feature. The definition and feature identifiers are not
interchangeable.

An instantiated positional definition begins at
`e0 01 feat_id 00 <canonical-reference> e0 00 ref_model_info 00`. The reference
is its owning modeling feature identifier. This boundary ends the preceding
labelled template or positional instance.

An unlabeled positional definition begins at `e3 S2D<digits> 00`. The next
such boundary ends the instance. Its owner is the unique unclaimed feature
whose nonempty class-200 source-entity identifier set is contained in the
instance's `order_table.ext_id` set, provided that feature selects exactly one
unlabeled instance. Definitions without this reciprocal unique join have no
owner. They remain section definitions and retain their complete bounded body.
Replay order does not define feature identity.

DEPDB also stores an internal sketch-datum chain. A procedural recipe feature
`F` immediately followed in feature-state order by a non-recipe feature
`F + 1` owns the unique section definition whose `gsec3d_ptr.sketch_plane`
entity is `F + 2`. The intermediate feature is the section datum. When more
than one definition selects the same sketch-plane entity, the chain does not
select a regeneration snapshot and none of those definitions acquires the
owner.

In `DEPDB_DATA`, `gsec2d_ptr 00 e0 0a name 00 S2D<digits> 00` begins a
labelled section definition. Its labelled table records define the positional
table classes used by following unlabeled `S2D` definitions. The next labelled
`gsec2d_ptr`, unlabeled `S2D`, or feature-definition record ends its body.

`AllFeatur` edge-treatment rows are feature recipes. `strong_parents`, `geoms_affected`, `edgs_affected`, and `contours` contain compact-int identifiers for the current body; they are neither coordinate arrays nor global geometry counts. The first edge-treatment row supplies the labelled schema, and later round and chamfer rows replay that schema positionally.

Within an `AllFeatur` `lo_restore` body, named-record type-one fields
`direction` and `direction2` each contain one complete compact integer. They
belong to the loop-restoration edge records and are not section-sweep direction
or extent fields.

Named procedural-choice fields belong to their containing feature row. Complete compact integers, compact-integer arrays, entity references, empty alternatives, and fully decoded `f9` scalar arrays are operation parameters qualified by choice and field name. A repeated qualified field name denotes ordered occurrences of the same parameter slot. Incomplete scalar wrappers and undefined field bodies remain opaque.

Class 913 stores `geoms_affected` and `edgs_affected` as the first and second
affected-array schema positions. Each position has independent extent state
within one `AllFeatur` stream. `f8 <count>` replaces that position's current
extent; omission of `f8` reuses its preceding extent. Exactly that many compact
identifiers belong to the position before the next position begins. The first
row can carry the field labels; positional rows omit them without changing the
two positions.

For a class-913 cylindrical slot fillet, the first two `geoms_affected`
identifiers are the axial cap planes. The remaining identifiers are tangent
support faces. The constant fillet radius is half the perpendicular gap between
parallel support planes. Multiple parallel support pairs define one constant
radius only when all nonzero gaps have the same magnitude. When every generated
cylinder carrier is placed, their common positive radius independently defines
the constant fillet radius; differing radii define no constant-radius result.

The fixed prefix of an `AllFeatur` feature row contains `f6 <class> e1`. The compact integer is the root `FeatDefs` schema class for that feature. This class dispatches the row to its operation-definition grammar. Classes 916 and 917 are section-sweep definitions whose recipe discriminates linear extrusion from rotation, class 911 is a hole definition, class 913 is a round definition, class 914 is a chamfer definition, and class 923 is a datum-plane definition. In a DEPDB recipe prefix, the root schema class performs the same dispatch.

A mixed generated-entity table opens as
`f8 <count> f7 <table-class> fb e3`. The first entry can begin with
`f7 <entry-class>`; table and entry schema-class identifiers vary by schema
stream. Exactly `count` entries follow, each ending at `e3`.

When a section-sweep feature has one `dtm_id_tab` entry equal to its
`gsec3d_ptr.sketch_plane_entity_id`, generated-table entry classes 204 and 203
in the first two positions identify its section and opposite cap face uses.
When both identifiers materialize as plane surfaces owned by the feature,
complete, distinct, parallel equations make the class-204 plane the
section-plane equation; the class-203 plane is the opposite sweep cap.

The section-sweep recipe determines its Boolean effect independently of the
localized operation-family display name. A `prot` recipe joins an established
preceding body and creates a new body when no preceding modeled body exists. A
`cut` recipe removes material. A sweep whose generated topology already forms
an independent body has new-body semantics.

In a class-916 or class-917 positional feature row, feature form `2` selects a
rotational section sweep. Its `param_choice_ptr` body begins after
`83 df f6 e3` and stores the choices in the labelled prototype order. The
choice sequence
`00 00 ea 44 00 00 f6 f6 f6 00 00 00 00` places
`ea 44 00 00` in `angle_choice` and defines a complete 360-degree revolution.
The preceding zero is the inactive `depth_choice`; it is not a zero angular
extent.

When a class-911 hole owns exactly two complete outline-backed plane rows, their
stored order is the entry and termination order. The planes are parallel.
Projecting the second origin minus the first origin onto the first unit normal
gives the signed blind depth; its magnitude is the hole depth and its sign
orients the hole axis from the entry plane toward the termination plane. The
first plane row is the hole's native placement-face selection.

A class-911 simple-hole generated table has four entries in the order entry
plane, termination plane, first cylinder use, and second cylinder use. Both
plane outlines store diagonal corners of the same axis-normal square. The
midpoint of either square is on the hole axis; half either in-plane span is the
hole radius. The two squares have equal nonzero in-plane spans and equal radial
midpoints. Both cylinder uses share this carrier. Layouts with additional
entries do not use this simple-hole rule. The midpoint of the entry square is
the neutral hole position, twice the square half-span is its diameter, and the
four-entry form is a simple cylindrical hole.

A class-917 circular section sweep uses the same four-entry order: first cap
plane, second cap plane, first cylinder use, and second cylinder use. The cap
planes are distinct and parallel. A complete cap outline whose two in-plane
spans are equal and nonzero is the circle's axis-normal bounding square. Its
midpoint lies on the cylinder axis and half either span is the radius. When both
cap outlines are complete, their radial midpoints and radii agree. One complete
cap outline is sufficient because the second placed cap plane fixes the sweep
direction and axial span independently. Both cylinder uses share this carrier.
The owning feature definition is the native circular profile. The ordered cap
planes define the neutral extrusion direction and blind extent. A
`Protrusion` has join semantics when an earlier modeling feature establishes a
body; otherwise its Boolean operation remains unresolved.

A typed schema row that owns a materialized `srf_array` row is an active construction feature. The root schema class supplies its operation family independently of an `MdlStatus` operation name.

Every bounded `feat_defs_<id>` body transfers byte-for-byte to the Creo native
`feature_definitions` arena as
`creo:featdefs:feature_definition#<id>`. A model feature with exactly one owned
definition references that record through `native_ref`; ambiguous ownership
does not produce a reference. An unlabeled positional definition has no
record-name identifier; until an exact owner join supplies one, its native
record identity is `creo:featdefs:feature_definition#offset:<offset>`.

Feature-definition `local_sys f9 04 03` and `transf f9 04 03` bodies use the
twelve-slot local-system language. `18 e5` expands to `[0, 1, 0]`; `18 10`,
`18 e4`, `18 e6`, and bare `10` each occupy one zero slot. A frame is numeric
only when this language consumes the complete bounded body as twelve slots.

A class-923 feature with exactly one resolved plane carrier defines that datum plane by the carrier's model-space origin, normal, and in-plane reference direction.

For a linear section sweep, generated plane carriers parallel to the section normal bound the sweep axially. Their signed offsets are measured from the section origin along the section normal. The extreme nonzero offset on one side defines a blind extrusion from offset zero to that offset; its sign determines the sweep direction. Extreme offsets on opposite sides define a two-sided extrusion. Equal magnitudes select the symmetric form with total length equal to the sum of the magnitudes. Interior axis-normal planes do not shorten the sweep. The section-definition identifier is the profile reference; it denotes a neutral sketch profile only when the sketch contains a resolved profile chain. The first resolved section sweep in feature-definition order forms the base body. A later sweep requires its Boolean operation before it can be committed as an independent body.
Within the generating feature, a complete plane `local_sys` supplies the cap
support point and normal. A held-coordinate outline for the same surface takes
precedence.

For a rotational section sweep, the unique nondegenerate section line whose
two solved endpoints have `u = 0` is the revolution axis. Applying the section
frame to its endpoints establishes the model-space axis origin and direction.
A full rotation of a NURBS directrix is an exact tensor-product NURBS surface.
Its angular direction has degree two, nine poles at successive 45-degree
positions, weights alternating `1` and `sqrt(2)/2`, four quarter-turn spans,
and doubled internal quarter-turn knots. Its directrix direction retains the
directrix degree, knots, poles, and weights.

Evaluating one closed linear-sweep profile produces one side face per oriented profile entity. A line produces a planar side face and an arc produces a cylindrical side face. Each profile vertex produces an edge parallel to the sweep direction. The exact signed area is the sum of line chord terms and circular-arc sector terms. Its sign selects the cap and side face senses. The two cap loops use the profile edges in opposite directions, and every cap or longitudinal edge has exactly two face uses. Cap-face pcurves are the section entities in the cap plane's `(u,v)` frame: lines remain lines and arcs become exact rational quadratic arcs. A planar side face uses profile distance and sweep offset as its parameters. A cylindrical side face uses profile angle and sweep offset. Its cap-edge pcurves hold the sweep offset constant and its longitudinal-edge pcurves hold the profile parameter constant. A multi-profile solid sweep has one outer profile that strictly contains every hole profile. Hole profiles are pairwise disjoint, unnested, and oriented opposite the outer profile.

A feature owns each mixed generated-entity table bounded by its `AllFeatur` row. The array's compact-integer count is not limited to a one-byte or 64-entry range. Each declared entry has an optional `f7 1e` prefix, a canonical entity-reference identifier, a compact entry class, a positional body, and an `e3` close within the bounded feature row. A class `200` entry carries its source section entity's external identifier immediately after the class when that lane is populated; a structural marker in that position leaves the source absent. The record close follows these typed compact lanes; an `e3` byte can be the low byte of their canonical two-byte form. A table surface identifier denotes geometry generated or modified by that feature. When that surface is the carrier of a connected face, the face's owning body is an output of the feature.

`edg_id_tab_ptr`, `lo_id_tab_ptr`, `bnd_type`, `used_bodies`, `geom_lists`,
and `dtm_id_tab` declare feature-owned geometry tables. Each table retains its
declared compact count and the entity-class identifier following its `f7`
marker. The label selects the edge, loop, boundary, body, geometry-list, or
datum identifier namespace independently of that class identifier.

Named records in `AllFeatur` form one implicit entity table in walker order.
The zero-based walker ordinal is the entity identifier used by `f7` references.
Each reference retains its containing source entity, target entity, and target
resolution state. These walker identifiers are not sketch external identifiers
and do not directly select `segtab` or saved-section entities.

`strong_parents` is the ordered set of earlier modeling features consumed to
regenerate the owning feature. It is a dependency relation, not feature-tree
containment.

`parent_table f8 <count> <ids...>` is the owning feature's ordered
regeneration-parent table. Its compact integers are modeling feature
identifiers. Both `parent_table` and `strong_parents` contribute dependency
edges; neither establishes feature-tree containment.

A generated sketch-plane datum is identified by its unique `DatumIds` entry.
Its section plane is the parent datum other than the `gsec3d` orientation
reference in the unique `Parents` row containing that orientation-reference
feature. The `DatumIds` table owner and `Parents` row owner occupy independent
feature namespaces and need not be equal.

`dtm_id_tab [f1|f2] f8 <count> f7 <class> fb e2` is followed by exactly
`count` named `dtm_id` compact integers. These identifiers occupy the outer
datum namespace used by `gsec3d.plane_id`; they are distinct from
`ActDatums.srf_array.geom_id` values.

In `DEPDB_DATA`, section-level `dtm_id_tab` and `parent_table` records belong
to the unique procedural recipe feature stored in the same section.

An outer datum identifier resolves through the generated-entity table that
contains it. When that table's owning datum feature has one `parent_table` row,
the nested reference-plane geometry identifies one datum parent by
`ActDatums.srf_array.feat_id`; the other unique datum parent is the sketch
plane.

`ActDatums` stores datum-plane geometry as `act_datum_geoms → srf_array` records. Each section includes one named datum row and can include positional `<gid> 22 ...` rows. For datum planes, `outline` stores two diagonal corners. Let `k = argmin_i |p0[i] - p1[i]|`; the plane equation is `x_k = p0[k]`. Datum names do not define their geometric orientation.

The datum surface row's `feat_id` is the owning modeling feature identifier.
The row's `geom_id` remains the separate datum-geometry identifier used by
`gsec3d` plane references.

`FeatDefsDtm` `matrix` records are display or saved-view matrices under `View`, `viewattr`, `world_matrix`, and `model2world` records. They do not define datum-plane placement.

`gsec3d_ptr` binds a 2D section to its placement, saved-section data, plane references, reference planes, order table, and dimension tables. `plane_flip` negates the sketch normal and extrusion side when it is not `f6`.

In `gsec3d` placement, project the referenced datum normal into the sketch plane to obtain the in-plane sketch `u` direction, then derive `v` as `n × u`. The resulting section-to-model transform is a proper rigid transform and is not a stored global matrix.

When the sketch plane resolves to a placed plane carrier or axis-aligned
`ActDatums` plane and the reference plane is perpendicular, their section
transform is:

```text
n      = sketch_plane.normal
u      = reference_plane.normal
v      = cross(n, u)
origin = sketch_plane.offset * n + reference_plane.offset * u
model([s, t, 0]) = origin + s*u + t*v
```

A set `plane_flip` or section `flip` negates `n` and its plane offset. A set
reference `flip_flag` negates `u` and its plane offset. Apply the two sketch
normal flips independently before deriving `v`.

Parallel plane references and set flip fields do not use this transform case.

## 7. DEPDB layout

DEPDB `crv_array` rows are sparse topology views with one-sided `[0, X1, F1, 0]` suffixes. They do not encode final loops or trim topology. Reconstruct the final B-rep by evaluating the profile and its `protextrude` or `protrevolve` operation. Embedded `1f 9d 10` streams use Unix-compress LZW with header flag `10` and block mode `0`; they contain display, XML, color, and shader data.
`DEPDB_DATA` carries the same fixed-prefix `srf_array` rows and bounded surface
parameter records as visible-geometry namespaces. Row acceptance uses the
stored family, feature, orientation, boundary, and next-surface fields; the
DEPDB section boundary supplies the namespace bound.

## 8. Additional record semantics

### 8.1 Scalar and datum tokens

A `0x99` DICT prefix maps to IEEE prefix `40 0E` in positive reads and `C0 0E` in the mirrored saved-section lane.
In the saved-section scalar lane, `dd` maps to IEEE prefix `40 0c`; its six
payload bytes are the remaining IEEE bytes.
In the same lane, `b3`, `cb`, and `d6` map to IEEE prefixes `bf e0`, `bf f8`,
and `c0 04`, respectively; their six payload bytes are the remaining IEEE
bytes.
The positional `var_arr` scalar lane maps `64`, `69`, `9c`, `9d`, `9f`, `a0`,
`ad`, `b3`, `cb`, `cc`, `d0`, `d2`, and `d6` to IEEE prefixes `3f d9`,
`3f de`, `40 11`, `40 12`, `40 14`, `40 15`, `3f d9`, `bf e0`, `bf f8`,
`bf f9`, `bf fe`, `c0 00`, and `c0 04`. Its `28 <tail7>` form maps to
`[3f, tail7]`, and its `2d <tail7>` form maps to `[40, tail7]`.
The positional generated-arc scalar lane maps `9b`, `9c`, `9d`, `9e`, `9f`, `a0`, `5e`,
`60`, `64`, `ad`, `cc`, `d0`, `d2`, `d5`, `de`, and `df` to IEEE prefixes
`40 10`, `40 11`, `40 12`, `40 13`, `40 14`, `40 15`, `3f d3`, `3f d5`, `3f d9`, `3f d9`, `bf f9`, `bf fe`,
`c0 00`, `c0 03`, `c0 10`, and `c0 11`, respectively. Its eight-byte
`28 <tail7>` form maps to `[3f, tail7]`. Outside that positional arc lane,
saved-entity `d5` is the negative subunit form `[bf, tail6, 00]`.
An `18` immediately before any positional generated-arc scalar opener is a
standalone zero and does not consume that opener as a cache index.

In plane `local_sys` rows, `18 e5` encodes `[0, 1, 0]`. `18 10`, `18 e4`, `18 e6`, and bare `10` encode standalone zero values under their row-specific token rules.
The positional row scalar `0e` encodes `-0.5`.

Positional `ActDatums` plane rows contain flat `envlp(2x2)` and `outline(2x3)` scalar sequences without `f9` array openers. Their outlines use the held-coordinate plane rule of named rows. The datum-plane set includes the named datum row and positional `geom_type = 0x22` rows.

Named `srf_array` plane rows store `outline\0 f9 02 03` followed by two
model-space corner triples. A complete outline with exactly one equal
coordinate pair defines the corresponding axis-aligned plane and offset.

In the positional datum scalar lane, `a5` and `9f` each occupy seven bytes.
Their numeric values are not required by the held-coordinate rule: identical
raw tokens compare equal and distinct raw tokens compare unequal.

In a named datum outline, paired standalone-zero slots at positions `k` and `k+3` identify coordinate axis `k` and plane offset zero. Other outline slots do not affect this rule.
The `41` scalar form in this named outline lane occupies eight bytes: the
prefix followed by seven payload bytes.

`ref_planes` stores an outer reference followed by a nested `plane_id`. The nested identifier is the geometric datum identifier and joins `ActDatums.srf_array.geom_id`. A referenced datum normal orients a sketch in-plane axis only when it is perpendicular to the sketch-plane normal.

### 8.2 Section topology

DEPDB stores a section directly below `gsec2d_ptr` when it is not nested in a
`feat_defs_<id>` record. Its `name` value `S2D<N>` supplies the section
identifier. When the namespace contains one section and one procedural-recipe
record, the recipe record's feature identifier owns the section. The section
retains the same `segtab_ptr`, `dimtab_ptr`, `relat_ptr`, `var_arr`,
`gsec3d_ptr`, and `p_saved_result` grammars as a nested feature definition.

Positional `segtab_ptr` replay ends at the first following section-table label,
including `dimtab_ptr`, `relat_ptr`, `var_arr`, `gsec3d_ptr`, `order_ptr`, or
`p_saved_result`, or at the next sibling `S2D<N>` record. Bytes in later tables
or sibling section records are not segment rows.

In an instantiated positional definition, the `S2D<N>` name terminator is
followed immediately by the unlabeled `segtab_ptr` array body. Its `f8` extent
bounds the section-entry table and can include non-segment entries; decoded
line, arc, and point rows are the entries with segment type `2`, `3`, and `5`.
The entity-reference header and segment rows use the same framing and field
order as the labelled `segtab_ptr` table.

The positional dimension table repeats the labelled template's `dimtab_ptr`
table-class reference in an unlabeled `f8 <count> f7 <table-class> fb e2`
header. The following entity reference selects the dimension-row class. The
first row follows that reference; later rows follow
`f3 f7 <table-class> e2`. All rows use the labelled dimension field order.

The positional variable table repeats the labelled template's `var_arr`
table-class reference in the same unlabeled array header and then stores its
variable-row class reference. The first row ends with
`f1 f7 <table-class> e2`; later rows are separated by `e2`. Its `f8` extent is
the number of variable rows. Each row replays `type`, `key`, `value`, `guess`,
`known`, `homogeneity`, and `uvar_id` in that order.

The positional relation table repeats the labelled template's `relat_ptr`
table-class reference and relation-row class reference. Its first row is the
schema prototype and ends with `f1 f7 <table-class> e2`. The following
`f8_count - 2` rows replay `id`, `used`, operand vectors `a`, `b`, and `c`,
`sign`, `idim`, and `type`; each row ends with `e2`.

The positional solver-incidence table repeats the labelled `skamp_ptr` table
class in `f8 <count> f7 <table-class> fb e2`. Each row replays `id`, `type`,
`flags`, and `status`, followed by a counted nested item array. The nested
array repeats its own table and row classes and stores ordered `ent_id`/`sense`
pairs. `f1 f7 <item-table-class> e2` separates nested items, and
`f3 f7 <table-class> e2` separates incidence rows.

The positional relation-join table repeats the labelled `triples_ptr` table
class and stores exactly its `f8` count of `rel_id`, `eqn_id`, and `skamp_id`
triples. Each field independently uses `f6` for null.
`f1 f7 <table-class> e2` separates the prototype from the following triples;
bare `e2` separates later triples.

A positional `gsec3d_ptr` record begins with `07 S2D<N> 00`, followed by
`flip`, `own_ref_id`, `first_chain_ptr`, `quilt_id`, `plane_id`, and
`plane_flip`. Its reference-plane array then stores an `f8` extent, table-class
reference, `fb e2`, and row-class reference. Each row replays `plane_id`,
`ref_type`, `ext_ref_id`, `seg_id`, `sub_index`, and `flip_flag`; rows after the
first follow `f2 f7 <table-class> e2` and their nested row payload.
The in-plane orientation is the unique referenced plane not parallel to the
resolved sketch plane. Its normal projected into the sketch plane defines the
section `u` axis, and the intersection of the two plane equations defines the
section origin. Parallel support planes and non-plane references do not define
the section axis.

`order_table` entries are `ext_id`, `int_id`, and orientation-flag tuples. `ext_id` references a section entity and `int_id` is the section's internal ordering index. A class-200 feature-generated-table entry stores the same `ext_id` as its source identifier and stores the generated surface identifier as its leading entity identifier. This explicit equality joins line, arc, and spline section entities to their generated carriers; table position and family order do not define the join.

For a linear section sweep with a resolved model-space section frame, a complete
saved line joined through this chain generates a plane parallel to the sweep
direction, and a complete saved arc or circle generates a cylinder whose axis
is the sweep direction. The generated surface row must belong to the sweep
feature and have the matching plane or cylinder family.

The generated-table source identifier remains part of the owning feature's design record even when the corresponding positional section entity is not decoded. It identifies the source section entity; it is not a global geometry identifier or a generated-table ordinal.

The positional `order_table` opener is `f8 <count> f7 <table_class> fb e2 f7
<entry_class>`. The first tuple is the entry prototype and closes with `f1 f7
<table_class> e2`; the following `count - 1` tuples are stored entries.
Stored tuples are separated by `e2`. The final tuple may end directly at the
following named field without an `e2` separator.

A section arc bound this way supplies a cylinder radius from its `cntrid` and endpoint in `var_arr`; its axis direction is the resolved `gsec3d` extrude axis, and its axis point is the section arc center transformed into model space.

When a plane `srf_array.geom_id` equals a line segment's `ext_id` and both are
owned by the same section-sweep feature, the plane is the sweep of that line
along the resolved section normal. Its origin is either transformed line
endpoint and its normal is the cross product of the transformed line direction
and sweep direction.

A resolved `gsec3d` frame places every complete `var_arr` section point in model space. It places a `segtab` line as the line through its transformed endpoints and a `segtab` arc as a circle whose center is the transformed `cntrid` point, whose axis is the section normal, and whose parameter-zero direction is the section `u` axis.

The placed section is the owning sweep feature's profile input. For `protextrude`, the resolved section normal is the model-space sweep direction. Each solved sketch entity references the model-space carrier produced from the same `segtab` row.

`ent_tab` membership identifies solved trimmed section entities. `segtab` entities outside `ent_tab` are construction or envelope entities.

The positional `ent_tab.chains` opener is `f8 <bucket_count> f7
<table_class> fb e2`. Its first entry in a bucket repeats the entry class as
`f7 <entry_class> 00 e3`; later entries in that bucket inherit the class and
begin after a structural `e3`. Each entry stores `ext_id`, `ent_mode`,
`start_vtx`, `end_vtx`, nullable `center_vtx`, and a terminal zero. The opener
count is the number of hash buckets, including empty buckets, rather than the
number of entity entries.

`vert_tab` chains bind a solved trim-vertex identifier to two incident `segtab` external identifiers. This vertex namespace is the namespace used by `ent_tab.start_vtx` and `ent_tab.end_vtx`. A solved trim vertex is the intersection of its two defining `segtab` carriers evaluated from `var_arr` or the joined saved-section geometry; its identifier differs from a `segtab` point identifier. A neutral sketch line uses its `ent_tab` start and end intersections, not the untrimmed carrier endpoints.
When the two incident `segtab` rows have exactly one common endpoint
`pointid`, that point's complete `var_arr` coordinate is the trim-vertex
coordinate. This join applies to line-line, line-arc, and arc-arc incidences.
Without a unique common point, independently evaluated carriers must have one
unique intersection before a coordinate is assigned. Two circular carriers
define a trim coordinate at internal or external tangency. Secant circular
carriers have two roots and remain unresolved without an independent root
selector.

The positional `vert_tab.chains` opener uses the same bucket-count framing.
Each populated entry begins with `f7 <entry_class>` and stores two incident
`ent_tab.ext_id` values, one trim-vertex identifier, and a terminal zero.

`p_saved_result` contains evaluated section entities and does not define the authoritative solved trim topology. Saved line rows may contain `f0 f7 <ref>`, `f1 f7 <ref>`, or bare `f7 <ref>` references between their identity, attribute, and coordinate fields.
The line prototype can close with `f1 e3`; positional line rows follow that
close. Within saved-section three-scalar coordinate fields, `18 e5` expands to
the coordinate triple `[0, 1, 0]`. In a saved-line coordinate row, `41` occupies
eight bytes, and `74` and `75` are positive DICT prefixes. Entity references may
also follow the sixth coordinate before the row-closing `e3`. Consecutive
`18 18` bytes are two standalone zero scalar slots; the first `18` does not
consume the second as a dictionary index.

`save_entity_ptr(spline)` carries `i_pnts f9 <count> 03` followed by exactly
`count` section-space XYZ triples. Every coordinate is a scalar-lane value.
The saved spline identifier is null when the spline is not assigned an
`order_table.int_id`. `end_tangts f9 02 03` carries two endpoint tangent
triples. `params f8 <count>` carries one scalar interpolation parameter per
point. The first parameter is zero and each later parameter is the cumulative
section-space chord length through `i_pnts`. In the `params` lane, `18` before
a parameter prefix is standalone zero; `6d`, `85`, `93`, and `9e` use the
positive DICT head rule; and `2d <tail7>` reconstructs `40 <tail7>`.
The neutral curve is the clamped cubic interpolation spline with four endpoint
knots, one simple knot at each internal stored parameter, `count + 2` poles,
point interpolation at every stored parameter, and first derivatives equal to
the two stored endpoint tangent vectors.

A saved-line family may contain a named `entity(point)` prototype between
positional line rows. Positional line replay resumes after that prototype's
`f1 f7 <ref> e3` close. A line row may end directly at the following named
entity record without an `e3` separator. After its six endpoint coordinates,
the row may carry six-byte `82..8f` state tokens and standalone `0f`, `18`, or
`e6` state markers before the row boundary; these fields do not alter the two
stored XYZ endpoints. In this lane, `18 e0` stores a standalone zero followed
by a named-record opener and is not dictionary index `e0`.

A saved entity identifier is an `order_table.int_id`; joining through that row's `ext_id` binds its evaluated geometry to the corresponding `segtab` entity. A saved line with two complete section-space XY endpoints supplies that entity's line geometry when its `var_arr` endpoints are relation-backed. The saved-entity and solved-`segtab` sets are one-to-one by entity family. After explicit `order_table` joins, exactly one unmatched saved entity and one unmatched solved entity of the same family bind as the unique remaining pair; multiple unmatched pairs remain unresolved.

A saved line, arc, or circle with complete section-space geometry and an
`order_table` join defines a neutral sketch entity under that row's `ext_id`.
Without an `order_table` join, the saved entity retains its internal identifier
and is a construction sketch entity. A complete model-space section frame maps
that construction entity to a placed line or circle curve, but does not make it
a profile member or a generated surface.
Under a complete model-space section frame, saved line endpoints and saved arc
or circle centers map through the section axes; saved arcs and circles define
model-space circle carriers with the section normal and stored radius.
Under a resolved coplanar revolution axis, a circular section centered on the
axis generates a sphere and an offset circular section generates a torus.
It is a profile entity when a class-200 entry with the same `ext_id` binds it to
a same-feature generated plane or cylinder of the corresponding family. Without
that generated-carrier binding, the evaluated geometry remains a construction
entity and does not establish solved trim membership.
A generated saved circle is a closed one-entity profile. Its traversal uses the
stored increasing full-turn parameterization.
Generated saved lines and arcs use their evaluated section-space endpoints as
an incidence graph. A connected component is a closed profile when every
endpoint has exactly one coincident endpoint on another entity and traversal
consumes the component before returning to its starting endpoint. Open,
branched, self-incident, and incomplete components remain construction
geometry. Traversal reversal is recorded independently for each entity.

The named `entity(arc)` record is followed by positional generated-entity
rows. Each row begins after `e3` with its saved entity identifier and a header
ending at `e2`. The identifier joins `order_table.int_id`, and the joined
`order_table.ext_id` supplies the entity kind from `segtab`. An arc row's
scalar body stores `center(3)`, `radius`, `end1(3)`, `end2(3)`, `t0`, and `t1`
in that order. A line row stores `end1(3)` and `end2(3)`; a horizontal or
vertical line is valid only when the corresponding endpoint coordinate is
equal. A complete saved entity supplies section-space geometry when its
`var_arr` carrier is relation-backed. For an arc row with complete center and
radius fields, `ent_tab` start and end trim vertices supply the arc endpoints
when both vertices lie on that circle. `arcorient = 0` orders the second trim
vertex before the first in increasing angular parameter.
When the saved arc also stores both endpoints, `end1` binds
`ent_tab.start_vtx` and `end2` binds `ent_tab.end_vtx`; these coordinates seed
the solved trim-vertex graph.
Each endpoint binding is independent: a stored endpoint seeds its bound trim
vertex exactly when it lies on the saved center/radius carrier.
The saved center/radius pair defines the circular carrier independently of the
endpoint fields. Trim incidence may intersect that carrier before either arc
endpoint is available; bounded arc geometry still requires both endpoints.
In a positional feature definition, these generated-entity rows occur without
the `p_saved_result` and entity-family labels. The enclosing feature-definition
boundary limits the row region; a row is a saved entity only when its leading
identifier joins `order_table.int_id` and that order row's `ext_id` joins a
`segtab` row.
When both saved endpoints and exactly one center ordinate are defined, equal
endpoint distance uniquely determines the missing center ordinate and radius.
The endpoint chord must vary along the missing center axis; a stored radius,
when present, must equal the derived radius.

When an `order_table` omission lies between adjacent stored `segtab` rows whose internal identifiers differ by two, the omitted row has the intervening internal identifier if a saved entity of the same family carries that identifier. For an evaluated saved line, if one `ent_tab` trim endpoint equals exactly one saved endpoint, the other saved endpoint determines the opposite trim endpoint. A line without an inline carrier is then determined by its two trim endpoints only when they satisfy its stored horizontal or vertical selector.

The `segtab` positional replay stores `type`, three direction fields, two endpoint point identifiers, `cntrid`, `arcorient`, `verhor`, two radii, and `ext_id`. A raw `verhor` value of `f5` adds one field before `radius`.

`segtab` and `ent_tab` compact identifiers may use `e3` as the tail byte of a two-byte compact integer. Such a tail is data, not a row delimiter. A `segtab` replay row is accepted only when its complete positional fields end at `e2`. An `ent_tab` replay row begins after a structural `e3`, ends with its zero field, and its external identifier joins a decoded `segtab` row.

For line rows, `verhor = 0` constrains the line vertical in section coordinates and `verhor = 1` constrains it horizontal. Other `verhor` values are not direction selectors.

### 8.3 DEPDB profiles and operations

A `point` record stores a first section coordinate as an IEEE-fill scalar, a point identifier, and a second coordinate as an `18 <index>` reference into the record-local `0x46` cache.

`i_pnts f9 <n> 03`, `end_tangts f9 02 03`, and `params f8 <n>` encode an interpolation-point spline with endpoint tangent angles and parameter values.
When its saved entity identifier joins `order_table.int_id`, the corresponding
`order_table.ext_id` is the spline's section-entity identity. A generated
class-200 entry with that source identifier binds the spline into the owning
sweep profile and to its generated spline surface. Clamped spline profile
connectivity uses the first and last evaluated control points.

A curve-from-equation entity stores `expression f8 <count>` followed by exactly `count` NUL-terminated UTF-8 source lines. `entity(crv_fr_eqn)` is the active equation record and `backup_ents(crv_fr_eqn)` is its separately identified backup record. Source-line order is significant. Lines beginning with `/*` are comments. Executable lines use `identifier = expression`; identifiers referenced on the right-hand side are expression dependencies. Numeric literals, previously assigned identifiers, parentheses, and `+`, `-`, `*`, `/` form the arithmetic subset. A right-hand reference to a uniquely assigned program identifier binds to that assignment independent of source order. Evaluate assignments in source order; an assignment remains symbolic when a dependency has not yet acquired a value.

The identifiers `r`, `theta`, and `z` define cylindrical curve coordinates over the normalized parameter `t` from zero through one. `theta` is in degrees. Constant positive `r` with affine `theta(t)` and affine `z(t)` is a circular helix: its angular travel divided by 360 is the signed revolution count, `z(1) - z(0)` is its signed axial rise, and `theta(0)` is its start angle. The owning curve-equation entity retains the native placement axis.

A curve-equation entity carries its placement in `local_sys f9 <dimensions> <count> <body>`. The scalar body is bounded by the following named field and uses the stateful local-system lane; it is part of the equation entity rather than a reference to a separate coordinate-system entity. For `f9 04 03`, twelve explicit slots have the same support-frame layout as a plane local system: slots 0 through 2 are the first radial direction, slots 3 through 5 are the zero rank marker, slots 6 through 8 are the second radial direction, and slots 9 through 11 are the origin. The explicit slot language includes the `18 e5` basis-vector triple and the standalone-zero forms defined for plane local systems. Orthogonal equal-scale nonzero radial directions define the unit axis by their normalized cross product. The cylindrical coordinates map through this frame as `origin + u*r*cos(theta) + v*r*sin(theta) + axis*z`.

A `protextrude` or `protrevolve` operation references its sweep axis through `gsec3d_ptr` placement fields rather than an inline axis vector. The `srf_array` row `feat_id` binds each materialized carrier to the generating feature. Extruding a section line yields a plane, extruding an arc yields a cylinder, and extruding an interpolation spline yields a degree-one ruled NURBS surface that retains the spline's degree, knot vector, control points, and weights along the directrix parameter. The feature's cap-plane offsets bound the translation parameter, including symmetric and two-sided spans. A closed profile yields cap planes. Each solved carrier in an `ent_tab` profile or a closed point-incidence fallback profile defines an unbounded surface of revolution independently of the operation's angular trim. A line parallel, angled, or perpendicular to the axis yields a cylinder, circular cone, or plane. A circular arc or complete circle with center on or off the axis yields a sphere or torus. An interpolation spline yields a full-turn tensor-product NURBS carrier. Saved analytic entities use their `order_table` source identity and same-feature generated-surface entry exactly as saved splines do. The projected carrier-to-axis vector defines the zero-azimuth direction; construction segments outside the resolved profile do not generate surfaces.

A section with a resolved `gsec3d_ptr` placement is an ordered planar sketch
history node owning the placed sketch geometry. When the section transform has
a generating feature identifier, that feature depends on the sketch history
node. The sketch node precedes its profile consumer in construction order.

`FamilyInf.Sld_FamilyInfo.drv_tbl_ptr` is the configuration driver-table
pointer. The configuration-root identity is
`creo:family_info:driver_table#root`. `e1` is an explicit null pointer; `f7
<canonical-reference-id>` identifies a present driver table.
The pointer is a configuration-root record even when it is null. A referenced
form retains the canonical entity identifier; interpreting the referenced
driver-table rows requires their table grammar.

A null pointer establishes that the part has no family-table configurations.
It is a complete configuration state, not an undecoded table.

Unix-compress streams with header `1f 9d 10` grow code width from 9 to 16 bits. Code 256 is a literal dictionary entry rather than a clear code.
