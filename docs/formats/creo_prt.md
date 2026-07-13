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

A body-section header has the byte sequence `#\n#<name>\n`. The preceding section ends at the `#` byte before that sequence. Section names are complete printable runs. A decoder must require both the preceding `#` terminator and a printable name when locating a section boundary. ND-layout section names may include an `ND:0:<Name>:N` decoration or a `ModelView#N` suffix.

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
| `DEPDB_DATA`                     | Persistence data used by DEPDB-layout parts.                                                                         |
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
byte0 = 0x3F when byte1 >= 0xE0, otherwise 0x40
```

Known prefixes include `71→3F E6`, `74→3F E9`, `81→3F F6`, `8b→40 00`, `90→40 05`, `91→40 06`, `a1→40 16`, `a2→40 17`, and `b7→3F E4`. In the `var_arr` coordinate lane, `d7` is the sign counterpart of `90` and maps to `C0 05 <tail6>`.

Lane-specific seven-byte forms include `6a <tail6>` for positive IEEE with leading byte `40` and implicit trailing `00`; `a3 <tail6>` for the negative form paired with the section-local `46` cache; `b9`, `d3`, and `df` for negative sub-unit forms with leading byte `BF`; and `41`, `4b`, `66`, `67`, `68`, `77`, and `82..8f` for positive sub-unit forms with leading byte `3F`.

Each record grammar defines the DICT lane for its scalar slots. A decoder must not apply DICT sign rules across unrelated record grammars.

#### World-coordinate tokens

World-coordinate tokens occupy eight bytes. Their final seven bytes hold the IEEE mantissa and low exponent. In the positional-outline/world lane, `46` denotes a positive token and `2d` denotes a negative token. `2d` consumes the complete eight-byte token in that lane.

#### Constants and cache references

`0f` and `e6` encode zero; `e4` encodes one. In row and `f9` scalar lanes, `e8 00` encodes standalone `1.0`; other contexts use a different selector grammar. `18 <index>` indexes a raw section-local `46` cache. Build that cache by scanning the raw section bytes, including `46` values that occur within other token tails. In a row or `f9` body, `18 <float-opener>` encodes a standalone zero and the following byte begins a new token.

## 3. Surface namespace: `srf_array`

`srf_array` provides surface and face-reference identifiers.

| Item                  | Rule                                                                                |
| --------------------- | ----------------------------------------------------------------------------------- |
| Count header          | `srf_array\0 f8 <count>`                                                            |
| ND count              | Count from the selected geometry payload.                                           |
| DEPDB count           | Sum `srf_array` counts across concatenated geometry subsections.                    |
| Positional row header | `<geom_id_ci> <geom_type> <feat_id_ci> <orient> <boundary_type> <next_geom_ptr_ci>` |
| Orientation bytes     | `01`, `f6`                                                                          |
| Boundary bytes        | `00`, `01`, `06`, `f6`                                                              |

Row bodies end at a valid row-close marker, named-record header, or a following positional row header that matches the row schema. The first row after `srf_array\0` can be a named-record row with the fields `geom_id`, `geom_type`, `feat_id`, `orient`, `boundary_type`, `next_geom_ptr`, `envlp`, `outline`, and `local_sys`.

### 3.1 Surface families

| `geom_type` | Surface family                                   |
| ----------- | ------------------------------------------------ |
| `22`        | Plane                                            |
| `24`        | Cylinder                                         |
| `25`        | Cone                                             |
| `26`        | Torus or sphere representation                   |
| `28`        | Spline surface                                   |
| `29`        | Spline or fillet surface family                  |
| `2a`, `2c`  | Linear-extrusion family (`surface_of_extrusion`) |

A decoder must not infer the kind of a row without a materialized parameter row from adjacent rows or topology.

### 3.2 Surface prototypes

`srf_prim_ptr` records contain the surface prototype fields. The prototype block closes with `f1 f7 <entity_ref> e3`.

| Prototype                                             | Named fields                                                    |
| ----------------------------------------------------- | --------------------------------------------------------------- |
| `srf_prim_ptr(plane)`                                 | `local_sys f9 04 03`, envelope, and domain fields               |
| `srf_prim_ptr(cylinder)`                              | `local_sys f9 04 03`, `radius`                                  |
| `srf_prim_ptr(cone)`                                  | `local_sys f9 04 03`, `half_angle`                              |
| `srf_prim_ptr(torus)`                                 | `local_sys f9 04 03`, `radius1`, `radius2`                      |
| `srf_prim_ptr(fillet_srf)`                            | Nested spline, tangent, flip, and `i_pnts` fields               |
| `srf_prim_ptr(tab_cyl)` and `srf_prim_ptr(ruled_srf)` | Local-system, curve/spline, parameter, and control-point fields |

Named prototype fields describe the first surface instance. Positional row bodies carry the per-instance values for subsequent instances.

Positional cylinder rows store cap-plane point data rather than a `local_sys` replay. Their per-instance radius does not inherit the prototype default; derive it from bound `fc 05` cap-circle geometry or from a byte-backed analytic construction.

Cone `half_angle` uses the positive DICT rule and is expressed in radians. Valid values lie in `(0, pi/2)`.

### 3.3 Torus and sphere representation

A `srf_prim_ptr(torus)` prototype stores `e1[3], e2[3], e3[3], origin[3], radius1, radius2`. A sphere uses `radius1 = 0` and radius `radius2`; a torus uses nonzero `radius1`. Per-instance row-body overrides use a separate grammar.

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

The guard requires orthogonal, equal-scale nonzero support directions. `outline f9 02 03` stores two XYZ corners. A coordinate held constant across both corners defines the corresponding model-space plane offset.

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

### 4.1 Pcurve endpoints

An eight-scalar curve body has this layout. Its values are parameter coordinates in the corresponding face spaces.

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

`fc 05` records can store cap-circle control points in the order `X`, `Z`, `t`, `Y`, where `X` and `Y` use eight-byte world-coordinate tokens and `Z` and `t` use DICT or standalone-zero scalar tokens. `fc 13` stores a control polyline rather than an analytic circle.

An `fc 05` cap pair belongs to one cylinder when each curve suffix binds one
side to the same `geom_type = 24` face and the other side to a `geom_type = 22`
face. The records must have equal radii and equal in-plane centers at distinct
constant cap ordinates. This binding establishes the cylinder radius and its
axis line in the owning feature's row frame. Model-space placement additionally
requires that feature's row-frame transform.

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
| `relat_ptr`   | Counted sketch-constraint relations. The `f8` allocation count includes two structural entries; exactly `count - 2` positional rows follow the schema close. Each row ends at `e2` and begins with `id`, `used`. |

An arc radius is the distance from its center to an endpoint in `var_arr`. A trim-vertex identifier is distinct from a `segtab` point identifier.

In a round-feature generated-entity table, a rowless face-use entry is a cylinder only when the table's following materialized `srf_array` entry is a cylinder. The table class token alone does not identify the surface kind.

## 6. Features and datums

`MdlStatus` names encode feature kinds as `<Kind> id <N>`. Known names include `Round`, `Chamfer`, `Protrusion`, `Extrude`, `Revolve`, `Hole`, and `Cut`.

`AllFeatur` edge-treatment rows are feature recipes. `strong_parents`, `geoms_affected`, `edgs_affected`, and `contours` contain compact-int identifiers for the current body; they are neither coordinate arrays nor global geometry counts. The first edge-treatment row supplies the labelled schema, and later round and chamfer rows replay that schema positionally.

`ActDatums` stores datum-plane geometry as `act_datum_geoms → srf_array` records. Each section includes one named datum row and can include positional `<gid> 22 ...` rows. For datum planes, `outline` stores two diagonal corners. Let `k = argmin_i |p0[i] - p1[i]|`; the plane equation is `x_k = p0[k]`. Datum names do not define their geometric orientation.

`FeatDefsDtm` `matrix` records are display or saved-view matrices under `View`, `viewattr`, `world_matrix`, and `model2world` records. They do not define datum-plane placement.

`gsec3d_ptr` binds a 2D section to its placement, saved-section data, plane references, reference planes, order table, and dimension tables. `plane_flip` negates the sketch normal and extrusion side when it is not `f6`.

In `gsec3d` placement, project the referenced datum normal into the sketch plane to obtain the in-plane sketch `u` direction, then derive `v` as `n × u`. The resulting section-to-model transform is a proper rigid transform and is not a stored global matrix.

When the sketch plane is a placed plane carrier or axis-aligned `ActDatums`
plane, the reference plane is a perpendicular axis-aligned `ActDatums` plane,
and the flip fields are clear, their section transform is:

```text
n      = sketch_plane.normal
u      = reference_plane.normal
v      = cross(n, u)
origin = sketch_plane.offset * n + reference_plane.offset * u
model([s, t, 0]) = origin + s*u + t*v
```

Parallel plane references and set flip fields do not use this transform case.

## 7. DEPDB layout

DEPDB `crv_array` rows are sparse topology views with one-sided `[0, X1, F1, 0]` suffixes. They do not encode final loops or trim topology. Reconstruct the final B-rep by evaluating the profile and its `protextrude` or `protrevolve` operation. Embedded `1f 9d 10` streams use Unix-compress LZW with header flag `10` and block mode `0`; they contain display, XML, color, and shader data.

## 8. Additional record semantics

### 8.1 Scalar and datum tokens

A `0x99` DICT prefix maps to IEEE prefix `40 0E` in positive reads and `C0 0E` in the mirrored saved-section lane.

In plane `local_sys` rows, `18 e5` encodes `[0, 1, 0]`. `18 10`, `18 e4`, `18 e6`, and bare `10` encode standalone zero values under their row-specific token rules.

Positional `ActDatums` plane rows contain flat `envlp(2x2)` and `outline(2x3)` scalar sequences without `f9` array openers. Their outlines use the held-coordinate plane rule of named rows. The datum-plane set includes the named datum row and positional `geom_type = 0x22` rows.

In a named datum outline, paired standalone-zero slots at positions `k` and `k+3` identify coordinate axis `k` and plane offset zero. Other outline slots do not affect this rule.

`ref_planes` stores an outer reference followed by a nested `plane_id`. The nested identifier is the geometric datum identifier and joins `ActDatums.srf_array.geom_id`. A referenced datum normal orients a sketch in-plane axis only when it is perpendicular to the sketch-plane normal.

### 8.2 Section topology

`order_table` entries are `ext_id`, `int_id`, and orientation-flag tuples. `ext_id` references a section entity and `int_id` is a one-byte generated-entity order index. In a feature-generated table, a line entity with `int_id = N` maps to table position `N - 1`. Arc entities map in `int_id` order to cylinder entries in generated-table order only when the feature's arc count equals its cylinder-entry count; `int_id - 1` does not index arc-generated cylinders.

A section arc bound this way supplies a cylinder radius from its `cntrid` and endpoint in `var_arr`; its axis direction is the resolved `gsec3d` extrude axis, and its axis point is the section arc center transformed into model space.

`ent_tab` membership identifies solved trimmed section entities. `segtab` entities outside `ent_tab` are construction or envelope entities.

`vert_tab` chains bind a solved trim-vertex identifier to two incident `segtab` external identifiers. This vertex namespace is the namespace used by `ent_tab.start_vtx` and `ent_tab.end_vtx`. A solved trim vertex is the intersection of its two defining `segtab` entities evaluated from `var_arr`; its identifier differs from a `segtab` point identifier.

`p_saved_result` contains evaluated section entities and does not define the authoritative solved trim topology.

The `segtab` positional replay stores `type`, three direction fields, two endpoint point identifiers, `cntrid`, `arcorient`, `verhor`, two radii, and `ext_id`. A raw `verhor` value of `f5` adds one field before `radius`.

### 8.3 DEPDB profiles and operations

A `point` record stores a first section coordinate as an IEEE-fill scalar, a point identifier, and a second coordinate as an `18 <index>` reference into the record-local `0x46` cache.

`i_pnts f9 <n> 03`, `end_tangts f9 02 03`, and `params f8 <n>` encode an interpolation-point spline with endpoint tangent angles and parameter values.

A `protextrude` or `protrevolve` operation references its sweep axis through `gsec3d_ptr` placement fields rather than an inline axis vector. Extruding a section line yields a plane, extruding an arc yields a cylinder, and a closed profile yields cap planes. Revolving a line parallel, angled, or perpendicular to the axis yields a cylinder, cone, or plane. An arc with center on or off the axis yields a sphere or torus.

Unix-compress streams with header `1f 9d 10` grow code width from 9 to 16 bits. Code 256 is a literal dictionary entry rather than a clear code.
