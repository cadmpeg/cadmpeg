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

A body-section header is `#<name>\n`. The first header follows the TOC's
newline. Later headers follow either the text delimiter `#\n` or the PSB
compound-close byte `f1`. An `f1 #<name>\n` boundary is a section boundary only
when the initial TOC lists `<name>` as a section entry. Section names are
complete printable runs. ND-layout section names may include an
`ND:0:<Name>:N` decoration or a `ModelView#N` suffix.

The ordered section directory stores each validated section's normalized name,
raw decorated name, semantic role, header offset, and byte length. It enumerates
decoded and opaque model data, auxiliary assets, and the thumbnail without
interpreting payload bytes as additional directory entries.

`#UGC_TOC 2 <count> <row-width> ...` is followed by `<count>` fixed-width ASCII
rows. An ordinary row begins with `<name> <offset-hex> <stored-length-hex>`.
Offsets are relative to the byte after `#-END_OF_UGC_HEADER\n`; stored lengths
include the `#<name>\n` section header. A `ModelView` row inserts its decimal
view identifier before the offset and has raw section name
`ModelView#<identifier>`. `NEXT_TOC_ENTRY` identifies another TOC block and is
not a body section. Every TOC-derived entry is valid only when its computed
offset contains the matching section header and its stored extent is inside the
file. Valid TOC entries are the authoritative section directory; delimiter
scanning is the fallback when no TOC entry validates.

A section payload beginning `1f 9d <flags>` uses Unix `compress` LZW framing.
The low five flag bits give the maximum code width from 9 through 16; bit 7
enables block mode and code 256 clears the dictionary. Codes are packed least
significant bit first in code-width-sized byte blocks. Block alignment resets
when the code width increases or a clear code resets it to nine. Expansion is
valid only when the output length equals the TOC expanded-length field. The
expanded payload begins directly with its PSB named record.

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
| `MdlRefInfo`                     | Model-space reference entities, including finite line endpoints.                                                       |
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

An expanded model scalar section stores `double_xar\0 f8 <count>` followed by exactly `count` ordered slots. `10` is the literal-one slot and `0b` is the literal-zero slot. The exact recursive placeholder images `e5 07 23 11 2e` and `e8 26 d6 95` each occupy one unresolved slot. Other slots use their defined scalar token widths. The final counted slot is `e0`, an explicit terminal null. Literal slots retain their decoded values; recursive placeholders retain their exact bytes.

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

In the ND layout, a complete plane or torus prototype `local_sys` and family parameters define the first instance carrier. Slots 0 through 2 contain the first support direction. In the rank-two compressed form slots 3 through 5 are zero and slots 6 through 8 contain the second support direction. A torus prototype can instead store its second support direction directly in slots 3 through 5. Slots 9 through 11 contain the origin in either complete form. Each positional plane origin slot uses its row-lane scalar form when the prefix defines one. Other slot-9 prefixes use the signed first-coordinate lane defined for tabulated-cylinder directrix points; other slot-10 and slot-11 prefixes use the corresponding second-coordinate lane. The first-coordinate lane's `4a` form stores a negative coordinate in seven bytes: `c0` is the implicit first IEEE-754 byte, the six bytes after `4a` are bytes one through six, and the low byte is zero. The normalized cross product of the two orthogonal, equal-scale support directions is the analytic axis. A bare terminal `18` in the bounded `local_sys` body occupies one zero slot. A plane passes through the local-system origin, uses the analytic axis as its normal, and uses the first support direction as its parameter-space reference direction. A zero torus `radius1` and positive `radius2` define a sphere centered at the local-system origin. Positive `radius1` and `radius2` define a torus with respective major and minor radii centered at that origin.

A complete plane envelope whose two model-space diagonal corners have exactly
one byte-equal coordinate defines an axis-aligned plane through that held
coordinate. The other two coordinate pairs are byte-distinct. This defines
the plane equation independently of the positional `local_sys`; it does not
define the plane's parameter-space reference direction.

A complete positional cylinder body begins `11 18 13`, followed by axial
length and the first corner's three coordinates, then the second corner's first
two coordinates in the positional surface-row scalar lane. An opaque third
coordinate follows that prefix. The body then contains exactly one complete
twelve-slot positional `local_sys` and ends with one positive scalar radius.
For an X- or Y-axis carrier, exactly one stored corner-coordinate difference
equals the positive axial length and the other stored difference equals twice
the radius. Slots 9 through 11 of the local system contain the model-space
origin. Its axial coordinate equals exactly one axial corner coordinate. The
axis points from that endpoint toward the other endpoint. Slots 0 through 2
contain the reference direction; reversing the axis also reverses that
direction. These fields define the cylinder carrier, radius, and bounded axial
length.

The compact axis-aligned positional cylinder body also begins `11 18 13` and
contains exactly seven surface-row scalars through the body boundary: positive
axial length followed by two XYZ corners. Exactly one corner-coordinate span
equals the axial length. Of the other two spans, exactly one is twice the
other. The smaller span is the radius and the larger is the diameter. The
second corner supplies the axial endpoint and the center coordinate on the
radius-span axis; the midpoint of the diameter span supplies the remaining
center coordinate. The axis points from the second axial endpoint toward the
first. The reference direction points from the diameter midpoint toward the
first corner.

The directrix-lane compact axis-aligned body has the same `11 18 13` opener
and exactly seven scalars through the body boundary, but every scalar uses the
first tabulated-cylinder directrix-coordinate lane. The first scalar is
positive. The remaining six scalars are two XYZ corners. Exactly one pair of
coordinate spans has a unique two-to-one relation; those spans are the
diameter and radius. The remaining coordinate is axial, and its corner span is
the bounded axial length. Origin, axis, and reference direction follow the
same second-corner, midpoint, and first-corner rules as the surface-row compact
body when the seventh scalar ends the body. A terminal `f7 17` or `f7 19`
reverses the corner orientation: the first corner supplies the axial endpoint,
and the axis and reference direction point toward the second corner. The
radius-span center coordinate remains the second corner in both forms.

A zero-support positional cylinder uses the same six-scalar envelope prefix as
the complete local-system form. Immediately before its three-scalar origin it
stores the support suffix `0f 18 e6 10 18 0f 18`; all nine support slots are
zero. A bare `18` may occupy the bounded final origin slot before the terminal
positive radius. Exactly one of the two stored corner-coordinate differences
equals the axial length, and the other equals twice the radius. The origin's
radial coordinate equals the midpoint of the radial corner pair, and its axial
coordinate equals exactly one axial corner. The axis points from that endpoint
toward the other. The reference direction points from the radial midpoint
toward the second radial corner.

A referenced planar-envelope positional cylinder begins `11 18 13` and stores
positive axial length, first radial bound, first axial bound, one complete
`19` or `32` model-reference token, second radial bound, second axial bound,
and positive radius. All geometric fields use the first tabulated-cylinder
directrix-coordinate lane. The radial span equals twice the radius and the
axial span equals the stored length. The cylinder origin has zero third
coordinate, the radial midpoint as its first coordinate, and the second axial
bound as its second coordinate. Without a trailer, the axis points from the
first axial bound toward the second and the reference direction points from the
first radial bound toward the second. A terminal `f7 17` or `f7 19` reverses
both directions while retaining the second-bound origin. The model-reference
token does not contribute a geometric coordinate.

A held-axis positional cylinder begins `11 18 13` and stores one held
coordinate, first radial bound, the literal separator `10`, first axial
coordinate, second radial bound, one complete `19` model-reference token,
second axial coordinate, and the exact trailer `f7 17`. Coordinates use the
first tabulated-cylinder directrix-coordinate lane. The two axial coordinates
are equal and the radial bounds are distinct. In model XYZ order, the radial
midpoint, held coordinate, and common axial coordinate define the origin. The
axis is positive Z, the reference direction points from the first radial bound
toward the second, and half the radial span is the radius. This body defines an
unbounded analytic carrier and does not define an axial extent. The
model-reference token does not contribute a geometric coordinate.

An axial/radial positional cylinder begins `11 18 13` and stores positive
axial length, first axial coordinate, one radial sample, second axial
coordinate, one complete `19` model-reference token, radial center, and the
exact trailer `f7 17`. All numeric fields use the first tabulated-cylinder
directrix-coordinate lane. A literal `10` separator occurs either immediately
before the radial sample or immediately after it. The axial-coordinate span
equals the stored length, and the radial sample differs from the radial center.
The separator before the radial sample selects the first axial endpoint as the
origin and directs the X axis toward the second endpoint. The separator after
the sample selects the second endpoint and directs the X axis toward the first.
The model Y origin coordinate is zero; the radial center is the model Z origin
coordinate. The radius is the absolute radial sample-to-center distance. The
reference direction is model Z with the sign opposite the radial offset. The
model-reference token does not contribute a geometric coordinate.

Cylinder and cone prototype local systems are parameter templates. Their terminal
triples do not establish model-space origins. Cylinder and cone carriers require
their positional construction or a feature placement.

A positional cone suffix consists of exactly one complete nine-slot support
frame, one axis-coordinate apex scalar, one complete `19` or `32`
model-reference token, one three-byte station token, and a terminal positive
DICT half-angle. The support frame's first and third triples are orthogonal
unit directions. Their cross product defines the axis line. The only nonzero
apex coordinate lies on that axis; the axis sign points from the apex toward
model zero. Negating the support frame's third direction defines the
parameter-space reference direction. The apex, signed axis, zero apex radius,
unit radial ratio, and positive half-angle define the exact cone independently
of the station token's scalar meaning.

A planar-envelope positional cone has an axis parallel to model Y and a
reference direction along positive model X. It stores positive outer and inner
apex distances, symmetric negative and positive radial bounds, and the paired
outer and inner Y stations. Subtracting each apex distance from its paired Y
station produces the same apex Y coordinate. The half-angle is
`atan(positive radial bound / outer apex distance)`. The body beginning `15`
separates the two apex distances with `18`, separates the inner station from
the positive radial bound with `18`, repeats the positive radial bound after
the outer station, and ends there. The body beginning `17` separates the apex
distances with `15`, repeats the negative radial bound after the inner station,
and ends with one complete model-reference token followed by `f7 2c`. The
model-reference token does not contribute a geometric coordinate.

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
`i_pnts` and `i_points` are aliases for the interpolation-point scalar lane.
Within their bounded body, `f9 00` between coordinate tuples is a continuation
marker and occupies no coordinate slot. When that form leaves the final tuple
one coordinate short at the field boundary, the omitted terminal coordinate is
zero. A terminal `18` occupies one explicit zero slot.

The direction/directrix form of a `geom_type = 2c` positional body begins with
a three-scalar model-space sweep-direction frame followed by the bytes
`00 0c 9a`. The directrix construction begins after this marker. Replay-bound
rows carry a six-scalar frame after the marker; that frame does not contain two
straight-directrix endpoints. An optional terminal `f7` entity reference
follows the frame, and the following `e3` closes the positional body. Scalar
payload bytes inside the six declared slots do not close the body. In a row
without a cubic replay, the six-scalar frame stores
the start and end XYZ points of a straight directrix. A nonzero sweep direction
and nondegenerate straight directrix define an unbounded plane.
Frame slots using cache-indexed scalar forms resolve against the scalar cache of
the containing geometry section; the resolved values remain part of the
surface parameter record.

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
`_ 42 _ _ 18 _` layout requires zero intercepts and retains the stored
sweep-axis sign. Its first and fourth slots accept the complete first-coordinate
scalar lane; its third and sixth slots accept the complete second-coordinate
scalar lane. In the `_ 2d _ _ 2d _` layout, slots one and
four also use the first-coordinate lane. Its directrix charts select exactly
one of two forms: a zero-offset form retaining the sweep-axis sign, or a
first-axis intercept magnitude of 30 with a zero second-axis intercept and a
reflected sweep-axis sign. A missing or non-unique form leaves the frame opaque.
Other six-scalar sequences after the marker are not directrix envelopes.

Cone `half_angle` uses the positive DICT rule and is expressed in radians. Valid values lie in `(0, pi/2)`.

A positional `geom_type = 25` body can terminate with one positive-DICT
`half_angle` scalar immediately followed by the structural body-close byte.
The scalar has precedence over scalar candidates beginning inside its payload;
the following close byte is not part of the scalar. The bounded body transfers
the value and source offset as `cone_half_angle_override`.

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

A `geom_type = 26` positional body trailer has the form `01 12 50
<selector_ci> <outline[2][3]>`. The selector is a compact integer. The outline
is six contiguous positional-row scalars and ends at the bounded body end. The
trailer transfers as `torus_outline_frame`; it does not assign radius or local
frame roles.

An untagged type-26 body can have the complete form `18 18 01 11 <scalar>
<coordinate[5]> 18`. The leading scalar is body-local and does not occupy a
coordinate slot. The five coordinates are contiguous positional-row scalars;
the terminal `18` closes the envelope and is not a sixth coordinate.

The untagged torus-envelope prefix begins after eight body-local bytes with
`18 94 3f 02 70 16 be fc 00 12 20`. Its direct form stores five contiguous
coordinates followed by `21`. Its split form stores two coordinates, `3a`, a
six-byte body-local control payload, and two more coordinates at the bounded
body end. The control payload does not occupy a coordinate slot.

A tagged `geom_type = 26` radius trailer begins with `18 0d`, followed by one
positive radial scalar, zero or one selector byte, and `0e`. Zero or one
selector byte after `0e` precedes the terminal positive `radius1` scalar. The
`radius1` scalar ends at the bounded body end. The separator `00 0e 01`
identifies the relative form: the first scalar is the outer ring radius
`radius1 + radius2`, so `radius2` is its positive difference from `radius1`.
Every other defined separator stores `radius2` directly. `radius1 = 0` selects
a sphere; a positive `radius1` selects a torus.

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
fields. Both extents in `f9 <dimensions_ci> <count_ci>` use compact integers.
The field declares exactly
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
slots 3..5    second in-plane support direction or [0, 0, 0]
slots 6..8    [0, 0, 0] or second in-plane support direction
slots 9..11   support-frame origin
```

The twelve-slot macro language must consume the complete local-system body. A
terminal `e1` after a complete frame is a null row-tail marker and is not a
scalar slot. If any other bytes remain, none of the twelve slot positions is
assigned a numeric value.

When the support-frame guard holds, derive the normal as:

```text
second = the nonzero triple among slots 3..5 and slots 6..8
normal = normalize(cross(slots[0..2], second))
```

Exactly one of slots 3..5 and slots 6..8 is the zero triple. The guard requires
orthogonal, equal-scale nonzero support directions. `outline f9 02 03` stores two XYZ corners. In these positional scalar lanes, `73` and `bb` each begin a seven-byte scalar token. Repeated identical tokens denote equal stored values; tokens with different prefixes denote distinct values. Token equality remains defined when the scalar magnitude is not decoded.

When exactly one coordinate is held constant across both corners, its axis is the positive basis normal and its value is the model-space plane offset. The other two coordinate pairs need only be known to be distinct; their magnitudes are not required. Zero or multiple held coordinates do not establish a plane equation from the outline.
The held coordinate establishes only the plane equation. It does not establish
the parameter-chart origin or either parameter direction.

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

When a plane is parallel to a placed cylinder axis and cuts the cylinder
strictly inside its radius, their intersection is two generator lines parallel
to the axis. The edge's paired half-edge incidences bind its two endpoint
vertex orbits. If both orbits have unique placed coordinates and exactly one
generator contains both coordinates, that generator is the edge carrier. Zero
or two matching generators do not select a carrier.

A topological vertex orbit with three linearly independent placed incident
planes is their unique intersection point. Additional incident placed planes
must contain the same point; otherwise the orbit has no placed vertex.
A tangent plane and sphere determine their single contact point. Two externally
or non-concentrically internally tangent spheres likewise determine their
single contact point. These two-carrier contacts define a topological vertex
without requiring a third carrier. Every additional incident carrier must
contain the same point.

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

When the byte following either row terminator begins a valid positional prefix,
that boundary prefix is authoritative; prefix-like byte sequences inside its
bounded parameter body do not introduce competing row starts. A segment that
contains a named preamble instead uses its unique valid prefix before the
terminal topology suffix.

A DEPDB cross-section curve count includes one labeled prototype followed by
`count - 1` positional rows. Each positional row has one fixed prefix and one
uniquely bounded `[0, X1, F1, 0]` suffix. The bytes between them are the row's
parameter body. The final positional row can end at the `e1` immediately
before the next `e0` named-record header. These one-sided rows remain in the
cross-section namespace and do not define model half-edge topology. Parameter
bodies use the positional curve scalar and canonical-reference token lanes;
unclaimed spans remain exact opaque bytes.

`F0` and `F1` reference faces in the `srf_array` namespace. `E0` and `E1`
reference the next edge for the two half-edge sides. When `previous(h)` is
unique, the equivalence relation `h ~ twin(previous(h))` defines topological
vertex orbits. The relation is symmetric and transitive; source identifier
order does not partition an orbit. The suffix graph defines half-edges, loops,
coedges, shells, and vertex orbits when both sides are present. `crv_pnt_dir` is
a per-side orientation-flag array, not a tangent vector. For pcurve endpoint
pairs, `01` traverses endpoint A to endpoint B and `f6` traverses endpoint B to
endpoint A. The two half-edge sides store complementary flags.

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

An unrecognized parameter token inside an otherwise complete point group does
not alter the point coordinates or held ordinate. The following eight-byte
world-coordinate opener bounds that token within at most eight bytes. Such a
record can establish its exact center and radius from the point equation, but
does not establish parameter sense or the parameter-zero radial direction.

Recognized eight-byte `46` and `2d` world-coordinate tokens in an `fc` body
retain their decoded millimeter value, exact bytes, body-relative offset, and
token length. Bytes between recognized tokens remain owned by the enclosing
curve parameter body as maximal opaque spans. The coordinate-token and opaque
span sets partition the complete retained body. Scalar order does not assign
point or parameter roles.

Within the `fc 05` scalar lane, the positive DICT prefixes `71`, `74`, `76`,
`81`, `8b`, `90`, `91`, `a1`, `a2`, `a3`, and `b7` each consume six payload bytes
and reconstruct the two high IEEE-754 bytes from the prefix. In particular,
`8b <tail6>` reconstructs `40 00 <tail6>` and `71 <tail6>` reconstructs `3f e6
<tail6>`. These lane-specific interpretations take precedence over wider
context-independent forms of the same prefix.

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
| `var_arr`     | Solver-variable table keyed by `key`; `type=1` is point `u`, `type=2` is point `v`, and `type=3` is radius; `value` is solved, `guess` is the pre-solve estimate, and `known`, `homogeneity`, and `uvar_id` retain solver state. |
| `segtab`      | Two-dimensional segments; `type=2` is LINE and `type=3` is ARC. A line uses `f6` as its null `cntrid`; an arc uses a center `pointid`. |
| `order_table` | Generated-entity ordering table.                                                                                                       |
| `ent_tab`     | Trimmed profile entity chain.                                                                                                          |
| `vert_tab`    | Trim vertices and their two incident `segtab` entities.                                                                                |
| `relat_ptr`   | Counted sketch-constraint relations. The `f8` allocation count includes two structural entries; exactly `count - 2` positional rows follow the schema close. Each row ends at `e2` and stores `id`, `used`, three four-slot operand vectors `a`, `b`, `c`, then `sign`, dimension selector, and relation-type discriminator. |
| `skamp_ptr`   | Counted solver-incidence rows. Each row stores `id`, `type`, `flags`, `status`, and a counted ordered array of section-entity `ent_id`/`sense` pairs. |
| `triples_ptr` | Counted joins from relation and equation identifiers to `skamp_ptr` incidence identifiers. Each of the three fields independently admits the `f6` null sentinel. |

The `skamp_ptr` and `triples_ptr` array headers retain their declared counts,
table-class references, and source offsets independently of the number of rows
whose bodies decode.
The `ent_tab` and `vert_tab` headers likewise retain their declared counts,
table-class references, and row-class references independently of validated
trim rows.
Complete native `ent_tab` rows are retained independently of whether `segtab`
is present, complete, or contains the same external identifiers. Cross-table
agreement is required only when deriving solved section topology.
The `dimtab_ptr` header retains its declared count and table-class reference
when no dimension row body validates.
Dimension rows form an ordinal parameter table only when the number of decoded
rows equals the declared count. An incomplete table contributes no neutral
parameters and does not resolve a relation's dimension selector.
The `var_arr` header retains its declared count and table-class reference when
no variable row body validates; its derived point set is then empty.
The `segtab_ptr` header retains its declared count and table-class reference
when no segment row body validates.
The `order_table` header retains its declared count and table-class reference
when its prototype or positional identity rows do not validate.
The `relat_ptr` header and its independent `skamp_ptr` and `triples_ptr` tables
remain present when a relation row body does not validate; preceding complete
relation rows remain ordered.
Within `skamp_ptr` and `triples_ptr`, a malformed later row does not invalidate
preceding complete rows or the declared table extent.
Derived equations and neutral constraints use a relation, incidence, or join
table only when all rows declared by that table decode. Complete prefix rows in
an incomplete table remain native records but cannot establish unique solver
identities.

The first `var_arr` row is the named field prototype between the table header
and schema close. It is a data row and contributes to the declared count;
positional replay rows follow the close.
The `f8` count is the exact total row count; bytes following that many rows do
not belong to `var_arr`.
An incomplete `var_arr` contributes no solved section coordinates. Complete
rows in an incomplete `segtab_ptr` remain independent section entities when
their `ext_id` values are unique among every decoded typed and opaque row. Such
rows supply standalone sketch geometry and solver-incidence loci, but the
incomplete table does not establish a complete profile, profile ordering, or
whole-table topology. Both table headers remain present with their complete row
prefixes.

`skamp_ptr` accepts the table wrappers `f1`, `f3`, and `f4 05`. Its named row
is the first counted row. Positional rows repeat the nested item schema for the
first item, then store additional `ent_id`/`sense` pairs directly; `e2`
separates direct items when the item count exceeds two. The row trailer is
`f3` plus the table entity reference plus `e2`; a one-item row instead ends at
its item `e2`, and the final row may end at the following named record. Solver
integer fields extend the compact-integer lattice with `c0..df XX YY`, equal
to `((head-c0)<<16)|(XX<<8)|YY`.

For a two-item type-zero incidence, sense `2` selects the native first endpoint
and sense `3` selects the native second endpoint. Sense `4` selects an arc or
circle center. The two selected point loci coincide and map to a neutral
coincident-loci constraint. When both loci are arc or circle centers, the same
incidence maps to the neutral concentric constraint for those two circular
entities.
When exactly one `segtab` row owns each referenced external identifier, this
incidence equates the corresponding stored `pointid` coordinates. A solved
coordinate on either endpoint therefore supplies the missing coordinate on the
other endpoint; conflicting solved coordinates remain distinct.
For an arc or circle operand, sense `4` selects its center. A type-14 incidence
stores a symmetry axis as a sense-zero line followed by two point loci selected
with senses `2`, `3`, or `4`. A type-3 incidence between a sense-zero line and
a selected point locus makes the locus coincident with the line and maps to a
neutral point-on-object constraint. A type-3 incidence between a sense-zero
`segtab` point and a selected point locus equates the point's `pointid`
coordinate with the selected endpoint or arc-center `pointid` coordinate and
maps to a neutral coincident-loci constraint. Solved coordinates propagate
across that equality under the same unique-row and conflict rules as type zero.
A two-item type-9 incidence with sense zero on one line and one point makes the
point coincident with the line and maps to a neutral point-on-object
constraint. Operand order does not change the line and point roles.
A two-item sense-zero line incidence makes the lines perpendicular for type 5,
parallel for type 7, and equal in length for type 8.
A two-item type-6 incidence with sense zero on two arcs or circles makes their
radii equal. A solved positive radius propagates through the connected radius
component. A solved arc center and endpoint supply their Euclidean distance as
the radius. A positive saved-arc or saved-circle radius anchors a connected
`segtab` radius component. Conflicting solved radii leave the component unresolved.
For an `arcorient = 0` arc these map to the neutral end and start loci,
respectively, because the analytic arc orientation is reversed. A two-item
type-four incidence makes the referenced entities tangent at their selected
endpoint loci.
A two-item type-three incidence has one sense-zero point entity and one
endpoint-selected entity; the point and endpoint loci map to a neutral
coincident-loci constraint. The separate sense-zero-line form maps to
point-on-object as defined above.
A one-item type-one incidence with sense zero makes the referenced line
horizontal. A one-item type-two incidence with sense zero makes the referenced
line vertical. Other senses select a locus and do not define an entity-level
orientation constraint.
Stored horizontal/vertical selectors and unique type-one/type-two incidences
define the line's held `v`/`u` coordinate, respectively. For type three or type
nine, a selected point on such a line inherits that held coordinate from either
line endpoint. The equality propagates in either direction and does not
overwrite conflicting solved coordinates.
Type-five and type-seven line incidences propagate perpendicular and parallel
orientation, respectively, through their connected line component. A
contradictory incidence cycle or conflicting stored or unary orientation leaves
the component orientation unresolved.
When trim vertices bound a line whose stored endpoint coordinates are
incomplete, this resolved component orientation validates the trimmed line
carrier against the corresponding equal section coordinate. An unresolved or
disagreeing orientation does not define that carrier.
Stored point ordinates, held-coordinate line equations, signed linear
dimensions, coincidence, point-on-line, same-coordinate, and axis-symmetry
incidences form affine equation components independently for `u` and `v` except
where one symmetry equation joins three ordinates. A consistent component
supplies every uniquely determined ordinate, including values that require
simultaneous equations rather than one-way propagation. A contradictory
component supplies no derived ordinate; byte-stored non-conflicting ordinates
retain their values.
A three-item type-fourteen incidence stores a sense-zero line followed by two
endpoint-selected loci. The loci are symmetric about the line, in stored order.
When the axis is uniquely horizontal or vertical and its held coordinate is
solved, one solved locus determines the other by copying the coordinate along
the axis and reflecting the perpendicular coordinate through the axis. A
complete saved endpoint or center supplies a solved locus without introducing a
section-point identity.
A three-item type-fourteen incidence whose first item is a sense-zero type-5
point instead makes the following two selected loci centrally symmetric about
that point entity. Senses `2`, `3`, and `4` select the same endpoint and center
loci as other solver incidences. For each section coordinate, the two selected
locus ordinates sum to twice the point entity ordinate.
A two-item type-seventeen incidence stores two endpoint or center loci that
share one sketch coordinate. Flag `1` selects the section `u` coordinate and
flag `2` selects the section `v` coordinate. This discriminator defines the
neutral same-coordinate axis without requiring solved locus coordinates. When
both loci are solved, their selected coordinates must agree. Other flag values
and contradictory solved coordinates retain the native incidence.
An incidence item may reference a complete saved-section entity through its
`order_table.ext_id`. When its type/sense pattern has no neutral constraint
mapping, retain the incidence type, ordered entity identifiers, and sense values
as one native sketch constraint; the absence of a typed locus interpretation
does not remove the solver relation. `relat_ptr`, `skamp_ptr`, `triples_ptr`,
`order_table`, and saved-section entities remain valid when `segtab_ptr` is
absent; segment-dependent refinement is withheld without dropping those design
records.
`skamp_ptr.id` is the incidence identity. A typed incidence requires exactly
one row with that identifier. Rows sharing an identifier remain separate native
constraints identified by their byte offsets.
Distinct `verhor`, `relat_ptr`, and `skamp_ptr` source records remain distinct
neutral constraints when they express equivalent equations; semantic
equivalence does not merge their source identities.
For an ordered saved line, senses `2` and `3` select its first and second stored
endpoints. For an ordered saved arc they select the neutral end and start loci,
respectively, because saved-arc evaluation reverses the stored endpoint order.
Sense-zero saved lines participate in type-one horizontal, type-two vertical,
type-five perpendicular, type-seven parallel, type-eight equal-length, and
type-fourteen symmetry-axis incidences through their `order_table` external
identifier under the same arity rules as `segtab` lines.
A complete saved line whose two endpoints share exactly one section coordinate
supplies that fixed-coordinate orientation to its connected type-five/type-seven
line component.
For type-three and type-nine point-on-line incidences, that same complete saved
line coordinate supplies the missing coordinate of the selected `segtab` point.
For type-zero and type-three coincidence incidences, complete saved endpoint or
center coordinates supply missing coordinates of the coincident `segtab` point.
For type-fourteen symmetry incidences, it supplies the reflection coordinate
without introducing a section-point identity for the saved axis.

The first `triples_ptr` row is named and contributes to its declared count.
Positional rows contain `rel_id`, `eqn_id`, and `skamp_id` followed by `e2`;
the last row may terminate directly at the next structural or named record.
A typed relation requires exactly one `relat_ptr` row with its `rel_id`.
Rows sharing a `rel_id` do not inherit `triples_ptr` joins and remain separate
native constraints identified by their byte offsets.
A relation joined to exactly one incidence through `rel_id` and `skamp_id`
inherits that incidence's ordered section-entity references and locus senses.
When the incidence contains exactly two items whose senses resolve to section
loci, those loci define the measured endpoints in stored order. This join is
independent of whether the relation discriminator has a neutral typed mapping.
A type-zero relation with sign zero, one, or `f6`, a defined `dimtab_ptr`
selector whose dimension type is linear, and a two-locus joined incidence is
the Euclidean distance between the joined loci. A nonempty incidence without
exactly two resolved loci remains an entity-level distance. A non-linear or
schema-defined selected dimension does not define a neutral distance. The more
specific operand-vector and `verhor` forms below refine that distance to
horizontal or vertical endpoint loci; incomplete operand vectors do not discard
the incidence-backed distance.

Within the three four-slot `relat_ptr` operand vectors, `e5` expands to two
zero slots and `e6` expands to three zero slots. `e4` is the integer value one,
and `f6` is a null operand. Expansion is bounded independently at four slots
for each of `a`, `b`, and `c`.

Within a positional `dimtab_ptr` row, `53` begins a seven-byte opaque value
field. The following byte is the `direct` field; the opaque value bytes do not
alter the alignment of `aux_value` and `ext_id` or terminate the dimension
table.

For a type-zero linear-distance relation with operand-vector forms
`a = [point0, point1, null, 1]`, `b = [0, 0, 0, 0]` or
`b = [1, 1, 0, 1]`, and
`c = [15, 16, 15, 1]`, the referenced dimension supplies the distance between
the two points along the measured horizontal or vertical segment. Sign `1`
adds the dimension and sign `f6` subtracts it. Sign zero selects the segment
direction: first-direction `1` adds the dimension, while the null
first-direction selector subtracts it.
Equivalent rows define one coordinate equation. Rows that assign different
signed differences to the same unordered point pair and coordinate define no
solved coordinate for that equation.

A type-zero relation with vectors `a=[first_point,second_point,null,1]`,
`b=[0,0,0,0]` or `b=[1,1,0,1]`, and `c=[15,16,15,1]` is a
segment-aligned linear dimension.
Its dimension selector is a zero-based index into `dimtab_ptr`. `verhor=1`
selects the section `u` difference and `verhor=0` selects the section `v`
difference. Sign `1` defines `second-first=+value`; sign `f6` defines
`second-first=-value`; sign zero stores only the unsigned magnitude.
Only a linear selected dimension contributes this section-coordinate equation;
an angular or schema-defined dimension does not supply a length ordinate.
The two point identifiers denote endpoint loci shared by every incident
`segtab` entity. Exactly one segment spanning the pair is required to refine the
relation to a horizontal or vertical distance constraint. The selected
`dimtab_ptr` row is the driving parameter independently of whether both endpoint
coordinates are evaluated.
The segment's unique orientation component selects the neutral distance axis.
A directly stored `verhor` selector and an orientation established through
type-one, type-two, type-five, or type-seven incidences have the same effect;
conflicting or unresolved orientation does not select an axis-specific neutral
constraint.

A type-one relation whose selected dimension is angular and whose first
operand vector is `a=[first_entity,second_entity,null,1]` measures the angle
between two line entities. The first two values are internal identifiers in
`order_ptr`; the complete order table must map each uniquely to a distinct
`segtab` line. Their stored order supplies the two neutral angle operands. The
remaining operand vectors and the relation sign retain the native
angle-direction selectors.

A type-five relation with
`a=[first_point,0,second_point,0]`, `b=[center_point,10,0,1]`,
`c=[16,15,0,0]`, and sign `1` binds the selected linear dimension to the
unique arc whose endpoint pair, center, and `radius` dimension index match
those stored operands. Endpoint order does not affect the radius. The selected
dimension is the neutral radius constraint parameter.

A type-14 relation with `a=[radius_id,0,0,0]`, `b=[0,0,0,0]`,
`c=[15,0,0,0]`, and sign `1` binds the selected dimension value to the
type-three `var_arr` radius with that key. An arc's `radius` field selects the
same radius key. The solved center point and positive radius define its
unbounded circular carrier before both arc endpoints are available.
Only a linear selected dimension contributes a solved radius.
The selected dimension is the neutral radius constraint parameter when exactly
one arc's `radius` field names that key and the selected dimension type is linear.
A non-linear or schema-defined selected dimension does not define a neutral
radius constraint.

The named `segtab` row before its schema close is likewise a data row. Its `type`, `dir`, `pointid`, `cntrid`, `arcorient`, `verhor`, radius, and `ext_id` fields contribute one segment to the declared table count.
Positional rows may insert the two-byte `c0 80` or `c1 00` wrapper before
`type`. The wrapper does not change the following field layout. A compact
`ext_id` value of zero is an identifier; the `f6` control sentinel represents
an absent value.
The `c0 80` wrapper may also precede the named row's scalar `type`. Segment
families other than types `2`, `3`, and `5` retain the same fields and count
toward table completeness, but do not define line, arc, or point geometry.
`ext_id` is the neutral section-entity identity when exactly one `segtab` row
stores that value. Rows sharing an `ext_id` remain independent construction
entities identified by their row offsets and do not participate in profile,
trim, generated-carrier, or solver-incidence joins through that value.
Only uniquely identified segments propagate solved section coordinates.
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
within the owning feature definition. A neutral parameter and any constraint
that selects it require exactly one `dimtab_ptr` row with that `ext_id`.
Every row is a neutral parameter. An undecoded value leaves its expression and
typed value unresolved without removing its identity. Repeated local identifiers use
occurrence-qualified parameter identities and names in source order, but no
constraint binds through that ambiguous identifier. Neutral parameter identity
includes the owning sketch-snapshot identity and `ext_id`; different snapshots
may reuse the same local `ext_id`. The parameter is owned by that snapshot's
sketch history feature. Repeated stored feature-definition identifiers use
source-offset-qualified native definition and sketch identities; repeated
parameter rows within one snapshot use occurrence-qualified identities in source
order. In positional dimension rows, a bare
`18` in the `aux_value` slot encodes zero and does not consume the following
compact `ext_id`.
The positional `value` lane uses the positive DICT lattice `5b..a3`; the first
two IEEE bytes are `3F75 + prefix` and the following six bytes complete the
value; `ad` is an alias for leading bytes `3F D9`. A bare `18` value is zero. Unresolved `00 XX YY`, `01 XX YY ZZ`, and
`53 <tail6>` value forms occupy three, four, and seven bytes respectively, so
the following one-byte `direct`, `aux_value`, and compact `ext_id` fields remain
aligned.
Type `0x03` has radius display semantics.

A `segtab` line whose two endpoint identifiers each have complete type-1 and
type-2 `var_arr` values is the bounded segment between those two `[u, v]`
points. A neutral ordinate requires exactly one `var_arr` row with the point
key and coordinate type, or repeated rows whose defined values agree.
Complementary coordinate rows combine by point key. Conflicting values leave
the point identity unresolved. Type-3 radius keys do not define section-point
identities. It is construction geometry when its `ext_id` is
absent from `ent_tab`.
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

For a native planar face with multiple closed loops, exactly one loop must
strictly contain every other loop. That containing loop is the outer boundary;
every contained loop is an inner boundary. A planar face with one closed loop,
and a non-planar face admitted under the one-loop rule, has one outer boundary.

In a round-feature generated-entity table, a rowless face-use entry is a cylinder only when the table's following materialized `srf_array` entry is a cylinder. The two entries are angular sectors of one oriented cylinder; the rowless face use inherits the materialized sibling's carrier and orientation. The table class token alone does not identify the surface kind.

Two parallel circular cylinders in strict secant position intersect in two
generator lines parallel to their common axis. Intersecting their transverse
circles gives the two line origins. The edge's paired solved endpoint orbits
select one generator when exactly one candidate contains both endpoints.

A circular cylinder whose axis contains a sphere center intersects the sphere
in two circles when the cylinder radius is strictly less than the sphere
radius. The circles have the cylinder radius and lie at signed axial offsets
`±sqrt(Rs² - Rc²)` from the sphere center. The edge's paired solved endpoint
orbits select one circle when exactly one candidate contains both endpoints.
Equal radii produce the single equatorial circle.
Intersecting every candidate circle with an additional incident plane supplies
a topological vertex only when all carrier intersections reduce to one point.
That unique model-space intersection is a neutral point independently of
whether every edge and face in its native B-rep component is evaluable. It is
also a neutral topological vertex only when an emitted edge uses its half-edge
orbit.

For a native edge on a derived intersection-line carrier, the oriented start
vertex is the carrier origin and the unit vector from start to end is its
direction. The edge interval is `[0, length]`. Exact source parameterizations
are not replaced by this construction. For an exact line with origin `O` and
direction `D`, each solved endpoint `P` has native parameter
`dot(P - O, D) / dot(D, D)`; the edge interval is the ordered pair of those
parameters. Periodic carriers require an independent arc-selection rule and do
not acquire an interval from endpoint positions alone. For a circular or
elliptical edge, the midpoint of a complete straight face pcurve maps through
the face surface to the interior of exactly one of the two conic arcs between
the solved edge endpoints. Ellipse parameters normalize coordinates by the
major and minor radii before applying `atan2`. The selected arc supplies the
ordered angular interval. Coincident endpoints select a full-turn interval
when the mapped midpoint is antipodal to the endpoint. Every endpoint-matching
pcurve on an evaluable adjacent face must select the same interval.
When every transferred use of a periodic conic edge is a one-half-edge closed
native loop, its half-edge orbit binds the same solved vertex at both ends, and
no native pcurve candidate is present, the loop defines one full carrier
period. The seam vertex parameter `t` defines the increasing interval
`[t, t + 2π]`. A multi-edge loop or any native pcurve candidate requires the
independent arc-selection rule above.

For a parabola with vertex `O`, focal distance `f`, major direction `X`, and
transverse direction `Y = axis × X`, the native parameter of point `P` is
`dot(P - O, Y) / (2f)` and its major coordinate is `f t²`. For a hyperbola
with center `O`, major radius `a`, minor radius `b`, major direction `X`, and
transverse direction `Y`, the positive-`X` branch parameter is
`asinh(dot(P - O, Y) / b)` and its major coordinate is `a cosh(t)`. Negating
both in-plane directions represents the opposite branch. Paired solved edge
endpoints must belong to exactly one hyperbola branch. A nonperiodic conic edge
interval is the ordered pair of its endpoint parameters.

A plane normal to a torus axis at axial offset `z` intersects the torus in circles of radii `R ± sqrt(r² - z²)`. At `|z| = r` the two roots coincide in one contact circle. At `|z| < r` the edge's paired solved endpoint orbits select one circle when exactly one positive-radius candidate contains both endpoints. A zero-radius horn-torus root is a point and does not define a curve.

A plane containing a torus axis intersects the torus in its two meridian
circles. Their centers are `C ± R radial`, where
`radial = normalize(plane_normal × torus_axis)`; each circle has radius `r`,
lies in the plane, and contains the torus axis direction. The edge's paired
solved endpoints select one meridian circle when exactly one candidate contains
both endpoints. A parallel plane not containing the torus center does not use
this construction.

A cylinder coaxial with a torus intersects it in one tangent circle when the cylinder radius equals the torus outer radius `R + r` or its positive inner radius `|R - r|`. The circle lies in the torus central plane, has the common axis, and has the cylinder radius. A cylinder radius strictly between the torus radial extrema produces two circles at axial offsets `±sqrt(r² - (Rc - R)²)` from the torus center. The edge's paired solved endpoint orbits select one circle when exactly one candidate contains both endpoints. Radii outside the torus radial interval do not intersect it.

A sphere whose center lies on a torus axis reduces their intersection to two circles in the axial meridian plane: one centered on the axis with the sphere radius and one centered at the torus major radius with the tube radius. External tangency or non-concentric internal tangency of those meridian circles produces one point with positive radial coordinate and therefore one model-space circle about the torus axis. A strict secant produces two meridian points and therefore two model-space circles. The edge's paired solved endpoint orbits select one circle when exactly one candidate contains both endpoints.

Two externally or non-concentrically internally tangent spheres have one common
point on their center line. That point is a unique topological vertex when it
also lies on every other incident carrier; it is not a zero-radius curve.
A plane tangent to a sphere likewise contributes its projected contact point
to vertex incidence without creating a zero-radius circle.

Two coaxial tori reduce their intersection to their tube circles in a shared axial meridian plane. External tangency or non-concentric internal tangency of the tube circles produces one point with positive radial coordinate and therefore one model-space circle about the common axis. A strict secant produces two meridian points and therefore two model-space circles. The edge's paired solved endpoint orbits select one circle when exactly one candidate contains both endpoints.

A circular cone and a coaxial sphere intersect in one circle when substitution of the cone radial function into the sphere equation produces one repeated axial root. For cone radius `r0`, slope `k = tan(a)`, and sphere center at axial coordinate `c` from the cone origin, the axial equation is `(1 + k²)t² + 2(r0 k - c)t + r0² + c² - Rs² = 0`. A zero discriminant gives the single tangent circle at axial coordinate `t`; its radius is `|r0 + kt|`. A positive discriminant gives two circles. The edge's paired solved endpoint orbits select one circle when exactly one candidate contains both endpoints.

A circular cone and a coaxial cylinder of radius `Rc` intersect in two axis-normal circles. For cone radius `r0` and slope `k = tan(a)`, their axial coordinates are `(Rc - r0) / k` and `(-Rc - r0) / k`; both circles have radius `Rc`. The edge's paired solved endpoints select one circle when exactly one candidate contains both endpoints.

Two coaxial cones whose positive transverse quadratic forms are proportional reduce their intersection to equality between scaled signed linear radial functions. This includes equal ratios with aligned principal frames and reciprocal ratios with exchanged principal frames. With the first cone's axial coordinate `t`, the second cone's axis alignment `d ∈ {-1, 1}`, its origin at first-axis coordinate `c`, and positive metric scale `m` defined by `M2 = m² M1`, the radial functions are `q1(t) = r1 + k1t` and `q2(t) = r2 + dk2(t - c)`. Each equation `m q1(t) = s q2(t)` for `s ∈ {-1, 1}` contributes one axis-normal section with first-frame radii `|q1(t)|` and `ratio1 * |q1(t)|` when its linear coefficient is nonzero and the radius is positive. Ratio one produces a circle; every other positive ratio produces an ellipse. An identity for either sign means the cone surfaces coincide and does not define an intersection curve. The edge's paired solved endpoints select one section when exactly one candidate contains both endpoints.

A circular cone and a coaxial torus reduce their intersection to the two signed cone lines and the torus tube circle in a shared axial meridian plane. For cone axial coordinate `t`, signed radial sense `s ∈ {-1, 1}`, torus major radius `R`, minor radius `r`, and torus-center axial coordinate `c` from the cone origin, each branch satisfies `(s(r0 + kt) - R)² + (t - c)² = r²` and contributes only roots where `s(r0 + kt) > 0`. Each retained root defines an axis-normal circle of radius `|r0 + kt|`. Repeated roots define tangent circles. The edge's paired solved endpoints select one circle when exactly one candidate contains both endpoints.

An analytic carrier pair transfers its sole intersection-curve candidate without requiring solved edge endpoints. When the pair produces multiple curve candidates, transfer requires paired solved endpoints contained by exactly one candidate. Solved endpoints must lie on the selected candidate even when the candidate set contains one curve.

A plane with any two cylinder, cone, or sphere carriers restricts both carrier
quadrics to conics in an orthonormal plane chart. The determinant of their
quadratic Sylvester matrix is a polynomial of degree at most four in one chart
coordinate. Every real resultant root is paired with the common real roots in
the other coordinate and refined against both conic equations. A topology
vertex is emitted only when exactly one resulting point satisfies every
incident carrier. Proportional coaxial cones use their exact section reduction
before this general resultant path.

Two independent planes define a model-space line. Substitution of that line
into any cylinder, positive-ratio cone, or sphere quadric gives a polynomial of
degree at most two. Its real roots are the complete candidate set, including a
single linear root when the quadratic term vanishes. A topology vertex is
emitted only when one candidate satisfies every incident carrier.

A plane normal to a circular cone axis intersects it in one circle away from the apex. Substitution of an oblique plane basis into the cone equation yields a diagonal quadratic whose signs distinguish ellipse, parabola, and hyperbola carriers. Completing the square gives the conic center or vertex, in-plane principal direction, radii, and parabola focal distance.

A positive-ratio elliptical cone uses local frame coordinates
`x² + (y / ratio)² = (radius + axial * tan(half_angle))²`. A plane normal to
its axis intersects it in an ellipse with major-frame radius equal to the
absolute local radius and minor-frame radius equal to that radius times the
ratio. Intersecting two independent planes produces a model-space line; direct
substitution into this equation yields a quadratic. One retained root defines
a topological vertex, while two roots remain ambiguous without another
selector. Substituting an arbitrary plane chart into the cone equation produces
a symmetric two-variable quadratic. Orthogonal diagonalization gives its
principal directions; the eigenvalue signs and completed-square constant
define an ellipse, parabola, or hyperbola with exact model-space frame and
radii or focal distance. For a plane through the cone apex, the constant and
linear terms vanish. The determinant of the remaining homogeneous quadratic
distinguishes no generator, one tangent generator, and two secant generators.
The edge's paired solved endpoint orbits select a generator when exactly one
of two lines contains both endpoints. Coaxial-surface and
surface-of-revolution reductions require `ratio = 1`.

## 6. Features and datums

`MdlStatus` names encode feature kinds as `<Kind> id <N>`. Defined names include
`Annotation Feature`, `Cross Section`, `Datum Plane`, `Round`, `Chamfer`,
`Protrusion`, `Extrude`, `Revolve`, `Hole`, `Cut`, `Draft`, `Mirror`, and
`Surface`. Reference-backed `Thicken <decimal-ordinal>` and
`Fill <decimal-ordinal>` names identify thicken and filled-surface operations.
`Merge <decimal-ordinal>` identifies a surface-merge operation.
Root feature-definition class `946` identifies the same surface-merge family
when the current-state record omits its display name. The class value does not
encode face selection or merge operands.
`Extrude <decimal-ordinal>` identifies an extrusion operation.
`Boundary Blend <decimal-ordinal>` identifies a boundary-surface operation.
`Protrusion` identifies a linear extrusion operation; absent section operands
leave its profile, direction, and extent unresolved without changing its family.
The German operation-family names `Bezugsebene`, `Rundung`,
and `Schräge` denote the same datum-plane, round, and draft families as
`Datum Plane`, `Round`, and `Draft`, respectively. `Annotation Feature` is a
non-modeling annotation container.
`Cross Section` and its German operation-family name `Querschnitt` are
non-modeling cross-section definitions. `Mirror` identifies a reflection
operation.

Operation names end in ` id <N>` or ` ID <N>`; the stored case follows the
name's localization. An ASCII `o`, `x`, `y`, or `z` byte immediately preceding
an uppercase operation-family name is a stored-name prefix, not part of the
family name and not a current-state selector. Multiple operation names with the same feature identifier are ordered
stored states; the last occurrence is the current state. Decoding the current
state does not discard the preceding state records. State ordinals are local to
one feature identifier and increase in byte order from zero. A stored state
retains the prefix-inclusive name bytes, the `id`/`ID` spelling, and the offset
of the optional prefix; a recipe-only state has no stored operation name.

`MdlRefInfo` feature-reference entries encode
`f7 0x71 <own-ref-id> <reference-type> <feature-id> <name> 00 <own-ref-id> <own-ref-id>`.
The three identifiers before the name and the two closing identifiers are
compact integers. The repeated closing identifiers delimit the name entry and
must equal its opening `own-ref-id`. The feature identifier joins the stored
name to the corresponding model-history feature when `MdlStatus` has no
identifier-bearing display name. Multiple names for one feature define a
display name only when their bytes agree.

The current-state record's root schema class selects the operation definition.
Feature rows supply a schema class only when the current-state record does not
carry one and all rows for that feature agree on one class. Row order does not
override the current-state class. The current state's recipe and parent
identifier likewise define the neutral operation family, Boolean effect,
source tag, parent, and dependency. A differing recipe or parent in an earlier
stored state remains history and does not veto the current projection.

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

A definition identifier selects geometry, placement, and operation semantics
only when exactly one bounded feature-definition record carries that identifier.
Repeated identifiers are ambiguous and do not join to a section transform or
feature operation.

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
owner. When the definition is contained by a class-926 row, `F` depends on
that saved-section history feature.

In `DEPDB_DATA`, `gsec2d_ptr 00 e0 0a name 00 S2D<digits> 00` begins a
labelled section definition. Its labelled table records define the positional
table classes used by following unlabeled `S2D` definitions. The next labelled
`gsec2d_ptr`, unlabeled `S2D`, or feature-definition record ends its body.

The same labelled section-definition form may occur inside a class-926
`AllFeatur` feature row. The containing row identifies the saved-section
history node. It does not replace the section-definition identifier or identify
the modeling operation that consumes the section. The definition body is
bounded by the end of that feature row; nested section tables and saved-result
records remain members of the definition.

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

Repeated named affected-ID arrays for one feature and namespace are distinct
stored states. They define a neutral edge selection, parent set, generated
output set, or round support set only when their ordered identifier arrays are
identical. Conflicting arrays remain native operation parameters.
An agreed `edgs_affected` identifier selects the B-rep edge with the same
`crv_array` curve identifier when that edge is present in the transferred body.
The bodies containing those selected edges are the feature's modified outputs.
Positional replay geometry and edge arrays use the same agreement rule,
including empty arrays; an empty and a nonempty state conflict.

For a class-913 cylindrical slot fillet, the first two `geoms_affected`
identifiers are the axial cap planes. The remaining identifiers are tangent
support faces. The constant fillet radius is half the perpendicular gap between
parallel support planes. Multiple parallel support pairs define one constant
radius only when all nonzero gaps have the same magnitude. When every generated
cylinder carrier is placed, their common positive radius independently defines
the constant fillet radius; differing radii define no constant-radius result.
When every surface row generated by the round is type `26`, every row must
carry a complete tagged radius trailer. Their normalized `radius2` values are
the rolling-ball radii of the toroidal patches and define one constant fillet
radius only when all values agree.
When those rows have no tagged radius trailers, a uniquely associated named
torus prototype supplies the rolling-ball-radius candidate from `radius2`.
Every generated row must carry a complete terminal outline, and exactly one of
the three corresponding endpoint-coordinate deltas in each outline must equal
that candidate. The candidate then defines the constant fillet radius.
The untagged five-coordinate envelope is an independent radius proof. With
coordinates `[a1,a2,b0,b1,b2]`, it requires `a1 = b0`; the two remaining
endpoint deltas, under exactly one coordinate ordering, must equal
`2*(radius1+radius2)` and `radius2`. The split four-coordinate form applies the
same two-delta rule to its leading and trailing coordinate pairs. Every
generated row must satisfy one of these envelope forms against the same
prototype radii.
Two linearly independent parallel support pairs with the same gap locate the
cylinder axis at the intersection of their midplanes. Intersecting those
midplanes with either axial cap plane fixes the carrier origin. Every support
plane must be parallel to the axis and tangent at the common radius. The
construction transfers a carrier only when the feature has exactly one
unplaced materialized cylinder row and every support plane satisfies these
constraints.

The fixed prefix of an `AllFeatur` feature row contains `f6 <class> e1`. The compact integer is the root `FeatDefs` schema class for that feature. This class dispatches the row to its operation-definition grammar. Class 916 is a subtractive section-sweep definition and class 917 is an additive section-sweep definition; their recipes discriminate linear extrusion from rotation. Class 911 is a hole definition, class 913 is a round definition, class 914 is a chamfer definition, class 923 is a datum-plane definition, and class 926 is a saved section. In a DEPDB recipe prefix, the root schema class performs the same dispatch.

A class-926 row containing one section definition is the history node for that
planar sketch. The contained definition identifier selects the neutral sketch
and the row identifier remains the history feature identifier. The section's
modeling owner remains independent. A definition without this unique
containment join uses a definition-scoped sketch history node.

Every byte-bounded `AllFeatur` row denotes a history feature independently of
whether the feature owns a materialized surface row. A recognized root schema
class selects its neutral operation type. Other root schema classes retain a
native operation with the schema class as a typed source property. Rows sharing
one feature identifier but carrying conflicting root schema classes still
define one native history feature; the conflicting classes remain source
properties and do not select a neutral operation family.

Each `DEPDB_DATA` recipe row ends with its canonical `f7` recipe binding. Its
body begins at the section boundary or immediately after the preceding recipe
binding. Multiple bindings in one persistence section define independent
feature rows.

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
an independent body has new-body semantics. Prior material exists only after
an unsuppressed feature has a body output or an unsuppressed earlier sweep has
new-body semantics. A hole,
round, chamfer, or joining sweep without a body output does not establish a
body for subsequent Boolean classification.

In a class-916 or class-917 positional feature row, feature form `2` selects a
rotational section sweep. Its `param_choice_ptr` body begins after
`83 df f6 e3` and stores the choices in the labelled prototype order. The
choice sequence
`00 00 ea 44 00 00 f6 f6 f6 00 00 00 00` places
`ea 44 00 00` in `angle_choice` and defines a complete 360-degree revolution.
The preceding zero is the inactive `depth_choice`; it is not a zero angular
extent. The same complete `83 df ...` choice sequence inside the bounded
section definition applies to its owning DEPDB rotational recipe. Repeated
identical sequences are distinct stored regeneration states with the same
full-turn extent. A neutral angular extent exists only when every decoded
termination state for the feature selects the same extent; state order does not
select one termination over another.

When a class-911 hole owns exactly two complete outline-backed plane rows, their
stored order is the entry and termination order. The planes are parallel.
Projecting the second origin minus the first origin onto the first unit normal
gives the signed blind depth; its magnitude is the hole depth and its sign
orients the hole axis from the entry plane toward the termination plane. The
first plane row is the hole's native placement-face selection.
When that surface is a transferred B-rep face, the surface identifier selects
the face with the same native identifier.

A class-911 simple-hole generated table has four entries in the order entry
plane, termination plane, first cylinder use, and second cylinder use. Both
plane outlines store diagonal corners of the same axis-normal square. The
midpoint of either square is on the hole axis; half either in-plane span is the
hole radius. The two squares have equal nonzero in-plane spans and equal radial
midpoints. Both cylinder uses share this carrier. Layouts with additional
entries do not use this simple-hole rule. The midpoint of the entry square is
the neutral hole position, twice the square half-span is its diameter, and the
four-entry form is a simple cylindrical hole.
The termination plane is the flat blind bottom of that simple hole.
A compact class-911 simple-hole table contains class-204 and class-203 topology
entries followed by two class-200 generated-geometry entries. The first
class-200 entry has source section entity zero and no surface row; it is the
rowless bottom. The second has no source section entity and uniquely names an
owned cylinder row; it is the hole side. This structure establishes the simple
cylindrical form independently of whether the cylinder parameters are
evaluable.

A class-917 circular section sweep uses the same four-entry order: first cap
plane, second cap plane, first cylinder use, and second cylinder use. The cap
planes are distinct and parallel. A complete cap outline whose two in-plane
spans are equal and nonzero is the circle's axis-normal bounding square. Its
midpoint lies on the cylinder axis and half either span is the radius. When both
cap outlines are complete, their radial midpoints and radii agree. One complete
cap outline is sufficient because the second placed cap plane fixes the sweep
direction and axial span independently. Both cylinder uses share this carrier.
The owning feature definition selects the emitted section sketch when that
sketch has a resolved profile chain and otherwise retains the native circular
profile reference. The ordered cap planes define the neutral extrusion direction and blind extent. A
`Protrusion` has join semantics when an earlier modeling feature establishes a
body and new-body semantics when its evaluated topology forms an independent
body.

A blind class-917 circular section sweep instead has four entries with classes
`204, 203, 200, 200`: a rowless cap use, one materialized cap plane, the
source-profile entity, and one cylinder use. The source-profile entry carries
its section entity identifier; the cylinder entry does not. The materialized
cap plane's complete square outline fixes the cylinder axis, radial center, and
radius. A type-20127 zero-offset placement instruction fixes the section at the
parallel standard datum; the materialized cap then fixes the blind trimming
extent. The resolved section profile, section normal, and cap offset define the
same neutral blind extrusion operation as the two-cap form.

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
`18 e4`, `18 e6`, bare `10`, and terminal bare `18` each occupy one zero slot.
A frame is numeric only when this language consumes the complete bounded body
as twelve slots.
When four slots precede `18 e5`, the token expands to `[0, 0, 1, 0, 0]`. This
rank-two form completes the zero local-y triple and supplies the local-z unit
direction.
The four consecutive triples are the local x axis, local y axis, local z axis,
and origin. When a definition contains exactly one complete `local_sys`, its
local z axis and origin define the section-plane equation. A zero-length local
z axis does not define a plane. Perpendicular nonzero local-x and local-z axes
also define the section's in-plane reference equation through the stored
origin. This complete local frame supplies section orientation when the
section's referenced plane entities do not reduce to one orientation plane.

A class-923 feature with exactly one resolved plane carrier defines that datum plane by the carrier's model-space origin, normal, and in-plane reference direction.

For a linear section sweep, generated plane carriers parallel to the section normal bound the sweep axially. Their signed offsets are measured from the section origin along the section normal. The extreme nonzero offset on one side defines a blind extrusion from offset zero to that offset; its sign determines the sweep direction. Extreme offsets on opposite sides define a two-sided extrusion. Equal magnitudes select the symmetric form with total length equal to the sum of the magnitudes. Interior axis-normal planes do not shorten the sweep. The section-definition identifier is the profile reference; it denotes a neutral sketch profile only when the sketch contains a resolved profile chain. The first resolved section sweep in feature-definition order forms the base body. A later sweep requires its Boolean operation before it can be committed as an independent body. A section-sweep definition is solid when its evaluated closed-profile topology produces a solid body. An absent evaluated body does not define a nonsolid sweep.
A class-916 or class-917 section sweep with one complete section transform and
parallel generated cap-plane equations is a linear extrusion even when its
current feature-state record omits the recipe discriminator. A stored
rotational recipe excludes this classification.
Without complete placement or cap equations, the same non-rotational class
remains a linear extrusion with unresolved profile, direction, and extent.
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

Evaluating one closed line/arc profile through a full turn produces one face
per oriented profile entity. A profile vertex off the revolution axis produces
one closed circular edge with one seam vertex; the preceding and following
faces form its two radial uses. A profile vertex on the axis collapses and
produces no edge. Each face has one singleton loop for each off-axis endpoint.
Planar, cylindrical, conical, spherical, and toroidal faces use their analytic
parameterizations. Boundary pcurves traverse one full azimuth at constant
axial, polar, or tube parameter; a planar boundary is an exact rational
quadratic circle. A spindle-torus boundary retains the signed ring branch, so
a negative ring shifts azimuth by π instead of reflecting the trim. Face sense
is the analytic carrier normal aligned to the outward side of the oriented
section profile.

A complete positional pcurve row stores endpoint A and endpoint B in each of
the two adjacent face parameter frames. A uniquely identified labeled
`crv_pnt_arr` prototype joined to one labeled curve-topology record provides
the same two endpoint pairs and adjacent face identities. The endpoint pair
belonging to one face forms a straight pcurve when mapping the pair through
that face surface yields the coedge endpoints in exactly one order. That order
is the pcurve direction and its parameter interval is `[0, 1]`. Agreeing
positional and labeled forms define one pcurve. Distinct matching paths, or a
pair that matches neither endpoint order or both orders, do not define a
pcurve.
Mapping a straight pcurve through a planar face chart defines an exact
model-space line carrier. Every positional and labeled path for that curve
which maps through a placed planar chart must produce the same ordered
model-space endpoint pair.
Mapping endpoint A and endpoint B through every evaluable adjacent face chart
must produce the same ordered model-space pair. For one topological vertex
orbit, the common point among the unordered mapped endpoint pairs of at least
two incident curves is its model-space point when exactly one point remains.
A unique orbit point selects the opposite endpoint of every incident
pcurve-backed edge and propagates through the connected endpoint component.
A candidate point must also lie on every independently placed analytic curve
carrier incident to that vertex orbit.
A carrier-derived point for the same orbit must agree with that point. An
empty endpoint domain withholds every dependent point in the component.
An edge transfers independently when both endpoint vertex orbits are solved;
face and loop transfer still requires every edge of the complete boundary.

When a native edge has no pcurve candidate on a solved planar face, an exact
line, circle, ellipse, parabola, hyperbola, or NURBS carrier lying in that plane
projects into the plane chart. For plane origin `O`, unit `u` axis `U`, unit
normal `N`, and `V = N × U`, model point `P` maps to
`(dot(P - O, U), dot(P - O, V))`. Directions use the same two dot products.
This affine projection preserves analytic parameters and NURBS degree, knots,
weights, periodicity, and edge parameter interval. Every analytic carrier
frame and every NURBS control point must lie in the plane. A present native
pcurve candidate remains authoritative; failure to reconcile it does not fall
back to a derived projection.

When a native circular or elliptical edge is a constant-`v` parallel of a
solved cylinder, cone, sphere, or torus, has the surface's local ring radii,
and has no native pcurve candidate, its pcurve is affine in the edge's angle
parameter. Cylinders, spheres, and tori require equal conic radii. A cone
parallel's major radius is the absolute local cone radius and its minor radius
is that radius times the positive cone ratio. The pcurve `u` origin is the
signed phase from the surface reference direction to the conic reference
direction, and its `u` direction is `+1` or `-1` according to the two frames'
handedness. Cylinder and cone `v` is the conic center's axial displacement from
the surface origin. Sphere `v` is the canonical polar angle
`atan2(axial_displacement, conic_radius)`. A torus parallel requires exactly
one signed ring-radius solution and uses its tube polar angle. A negative cone
or torus ring radius adds a half-turn phase and reverses the surface's
azimuthal tangent before handedness is applied. The pcurve retains the edge
parameter interval. Off-axis centers, unequal local radii, apex or pole points,
nonpositive cone ratios, ambiguous torus branches, and misaligned frames do not
define this pcurve.

When a native circular edge with no native pcurve candidate is a sphere or
torus meridian, its plane contains the surface axis. A sphere meridian is a
great circle centered at the sphere center. Its oriented plane normal and the
sphere axis fix the constant-`u` radial direction. A torus meridian is centered
one major radius from the torus center in the equatorial plane and has the
minor radius; its center fixes the constant-`u` radial direction. The signed
phase from that radial direction toward the surface axis fixes the pcurve `v`
origin, and circle-frame handedness fixes a `v` direction of `+1` or `-1`.
This affine pcurve retains the circle's native angle parameter and the edge
parameter interval, including a full sphere meridian through both poles. A
displaced center, unequal radius, or misaligned meridian plane does not define
this pcurve.

When a native line with no native pcurve candidate is a constant-`u`
generator of a solved cylinder or positive-ratio cone, its line origin fixes
the surface azimuth and axial `v`. Cone azimuth is recovered by dividing the
two radial frame components by the signed local major and minor radii; the
normalized components must lie on the unit circle. Its direction must be a
nonzero scalar multiple of the surface derivative
`axis + tan(half_angle) * (cos(u) * x_axis + ratio * sin(u) * y_axis)`; the
cylinder derivative uses zero radial slope and unit ratio. The scalar multiple
is the pcurve `v` direction, so the affine pcurve preserves the 3D line
parameter and edge parameter interval. Lines off the surface or skew to the
generator derivative do not define this pcurve.

A NURBS curve has intrinsic domain
`[knots[degree], knots[control_point_count]]`. A native edge on a nonperiodic
higher-degree curve uses that complete domain when its two solved vertices
uniquely match the curve evaluations at the two domain bounds. Each nonzero
knot span of a degree-one NURBS with positive weights is a rational line
segment. For geometric segment fraction `a`, endpoint weights `w0` and `w1`,
and local knot fraction `l`, inversion is
`l = a w0 / (w1(1 - a) + a w0)`. A solved vertex defines a bounded degree-one
edge parameter only when this inversion and curve reevaluation produce exactly
one parameter across all spans. A matching constant span or repeated model
point is ambiguous. The two unique endpoint parameters define the increasing
edge interval. A positive-weight periodic NURBS used only by one-edge closed
native loops uses its complete intrinsic domain when both domain bounds
evaluate to the seam vertex and no native pcurve candidate is present. Other
periodic carriers and nonmatching endpoint pairs do not establish an edge
interval by these rules.

Evaluating one closed linear-sweep profile produces one side face per oriented profile entity. A line produces a planar side face and an arc produces a cylindrical side face. Each profile vertex produces an edge parallel to the sweep direction. The exact signed area is the sum of line chord terms and circular-arc sector terms. Its sign selects the cap and side face senses. The two cap loops use the profile edges in opposite directions, and every cap or longitudinal edge has exactly two face uses. Cap-face pcurves are the section entities in the cap plane's `(u,v)` frame: lines remain lines and arcs become exact rational quadratic arcs. A planar side face uses profile distance and sweep offset as its parameters. A cylindrical side face uses profile angle and sweep offset. Its cap-edge pcurves hold the sweep offset constant and its longitudinal-edge pcurves hold the profile parameter constant. A multi-profile solid sweep has one outer profile that strictly contains every hole profile. Hole profiles are pairwise disjoint, unnested, and oriented opposite the outer profile.

The cap loops produced from the outer profile are outer boundaries, cap loops
produced from hole profiles are inner boundaries, and every single-loop side
face has an outer boundary.

Evaluating a one-circle linear-sweep profile produces two planar caps and one
cylindrical side face. Each cap circle is one closed edge with one seam vertex.
The cap and side coedges form a two-use radial pair. The side face has one
closed loop at each axial bound. Cap pcurves retain the circle's section-space
center and increasing full-turn parameterization; side pcurves run from zero
through `2π` at constant sweep offset.
Each cap's sole loop is its outer boundary.

A feature owns each mixed generated-entity table bounded by its `AllFeatur` row. The array's compact-integer count is not limited to a one-byte or 64-entry range. Each declared entry has an optional `f7 1e` prefix, a canonical entity-reference identifier, a compact entry class, a positional body, and an `e3` close within the bounded feature row. A class `200` entry carries its source section entity's external identifier immediately after the class when that lane is populated; a structural marker in that position leaves the source absent. The record close follows these typed compact lanes; an `e3` byte can be the low byte of their canonical two-byte form. A table surface identifier denotes geometry generated or modified by that feature. When that surface is the carrier of a connected face, the face's owning body is an output of the feature.

Generated carrier lookup spans every mixed generated-entity table owned by the
feature. A source section entity binds a neutral carrier only when exactly one
owned table entry carries that source identifier and its leading entity is a
materialized surface in that table. Multiple owned tables are not ambiguous by
themselves; duplicate source bindings across them are ambiguous.

A table-class-100 entry references a generated entity. When exactly one other
feature owns a class-200 entry for that entity identifier, the referencing
feature depends on that generating feature. A self-reference does not add a
history dependency. Competing generating owners leave the dependency
unresolved.

`edg_id_tab_ptr`, `lo_id_tab_ptr`, `bnd_type`, `used_bodies`, `geom_lists`,
and `dtm_id_tab` declare feature-owned geometry tables. Each table retains its
declared compact count and the entity-class identifier following its `f7`
marker. The label selects the edge, loop, boundary, body, geometry-list, or
datum identifier namespace independently of that class identifier.

The implicit `AllFeatur` entity table begins at section-body offset zero with
`e0 00 Sld_Features 00`. A section body without this root does not carry the
walker-order table.

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

Within one `AllFeatur` stream, the named `dtm_id_tab` establishes the table
class for following positional feature rows. A positional table begins
`f8 <count> f7 <class> fb e2`. Its first entry begins
`f7 <class + 1> <dtm_id> <dim_id>`. Each additional entry begins
`[f1|f2] f7 <class> e2 <dtm_id> <dim_id>`. The datum and dimension identifiers
use canonical reference-id encoding; `f6` is a null dimension identifier.
Exactly `count` datum identifiers belong to the owning positional feature row.
Table-class state does not cross an `AllFeatur` stream boundary.

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

`place_instruction_ptrs` declares an entity-reference class. Each instantiated
positional row begins `f1 f7 <declared-class> e3`, followed by instruction
type, scalar offset, nullable dimension, nullable reference, nullable first and
second geometry operands, and two membership selectors. `f6` is null in an
identifier lane. Instruction type 20127 with exact zero offset, null dimension
and reference, the `gsec3d` reference datum as its first geometry operand, null
second geometry operand, and zero membership selectors places the section at
zero offset from the standard datum parallel to the generated cap. Repeated
identical rows are identical regeneration states of one placement.

In `gsec3d` placement, project the referenced datum normal into the sketch
plane to obtain the in-plane type-2 direction `v`, then derive the type-1
direction as `u = v × n`. The resulting section-to-model transform is a proper
right-handed rigid transform and is not a stored global matrix.

When the sketch plane resolves to a placed plane carrier or axis-aligned
`ActDatums` plane and the reference plane is perpendicular, their section
transform is:

```text
n      = sketch_plane.normal
v      = reference_plane.normal
u      = cross(v, n)
origin = sketch_plane.offset * n + reference_plane.offset * v
model([s, t, 0]) = origin + s*u + t*v
```

A set `plane_flip` or section `flip` negates `n` and its plane offset. A set
reference `flip_flag` negates `u` and its plane offset. Apply the two sketch
normal flips independently before deriving `v`.

For the blind class-917 `204, 203, 200, 200` layout, the type-20127 placement
selects the unique construction datum parallel to the materialized cap and
perpendicular to the referenced orientation datum. The cap must have nonzero
separation from that datum. Its complete square outline supplies the generated
cylinder center. Translating the section origin within its plane so the saved
circle center maps to that cylinder center preserves the stored sketch
coordinates and fixes the model-space profile placement.

Parallel plane references and set flip fields do not use this transform case.

## 7. DEPDB layout

DEPDB `crv_array` rows are sparse topology views with one-sided `[0, X1, F1, 0]` suffixes. They do not encode final loops or trim topology. Reconstruct the final B-rep by evaluating the profile and its `protextrude` or `protrevolve` operation. Embedded `1f 9d 10` streams use Unix-compress LZW with header flag `10` and block mode `0`; they contain display, XML, color, and shader data.
`DEPDB_DATA` carries the same fixed-prefix `srf_array` rows and bounded surface
parameter records as visible-geometry namespaces. Row acceptance uses the
stored family, feature, orientation, boundary, and next-surface fields; the
DEPDB section boundary supplies the namespace bound.

The DEPDB `Xsections` section contains an independent
`Sld_Xsections > xsec_geom > srf_array` namespace. Its rows use the same fixed
prefix. Each named prototype row has boundary type `00`; every positional
replay has boundary type `06`. Other boundary types inside the counted frame
belong to row bodies. Cross-section identifiers do not join the material
model-face namespace. Their bounded positional parameter bodies use the same
scalar-token and row-boundary rules and remain in the cross-section namespace.
Plane rows use the standard or compact envelope layouts and the following
bounded local-system chunk without changing namespace ownership. A complete
held-coordinate outline or complete non-axis local frame defines a
model-coordinate cross-section plane carrier; it is not a material model face.

## 8. Additional record semantics

### 8.1 Scalar and datum tokens

A `0x99` DICT prefix maps to IEEE prefix `40 0E` in positive reads and `C0 0E` in the mirrored saved-section lane.
Model-reference coordinate rows encode `ed <bytes8>` as the big-endian
IEEE-754 value `<bytes8>`.
Their `19 <bytes7>` and `32 <bytes7>` forms encode the big-endian IEEE-754 value
`3f <bytes7>`.
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
model-space corner triples. The scalar lane resolves `18 <index>` through the
section-local dictionary of distinct `46` tokens. A complete outline with
exactly one equal coordinate pair defines the corresponding axis-aligned plane
and offset.

In the positional datum scalar lane, `a5` and `9f` each occupy seven bytes.
Their numeric values are not required by the held-coordinate rule: identical
raw tokens compare equal and distinct raw tokens compare unequal.

In a named datum outline, paired standalone-zero slots at positions `k` and
`k+3` identify coordinate axis `k` and plane offset zero.
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
The `S2D<N>` header, complete placement fields, and complete reference rows
remain present when a later field or row is incomplete.
The in-plane orientation is the unique referenced plane not parallel to the
resolved sketch plane. Its normal projected into the sketch plane defines the
section `u` axis, and the intersection of the two plane equations defines the
section origin. Parallel support planes and non-plane references do not define
the section axis.

`order_table` entries are `ext_id`, `int_id`, and orientation-flag tuples. `ext_id` references a section entity and `int_id` is the section's internal ordering index. The declared count includes one structural prototype followed by exactly `count - 1` stored rows. Named tables encode the prototype as named `ext_id`, `int_id`, and `bitmask` fields; positional tables encode the same three fields positionally. An incomplete table retains its complete row prefix but establishes no semantic joins. A semantic join requires exactly one row for the selected `ext_id` and exactly one row for the selected `int_id`; duplicate keys do not select a first row. A class-200 feature-generated-table entry stores the same `ext_id` as its source identifier and stores the generated surface identifier as its leading entity identifier. This explicit equality joins line, arc, and spline section entities to their generated carriers; table position and family order do not define the join.

A saved entity with a unique internal identifier takes the corresponding unique
`order_table.ext_id` as its section-entity identity even when no `segtab` row
has that external identifier. More than one saved entity with the internal
identifier, or more than one `segtab` row with the external identifier, makes
the join ambiguous.
When both `var_arr` and the joined saved entity define complete line or arc
geometry, their ordered endpoints and carrier equations must agree. Conflicting
complete forms leave the section entity unresolved.

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
Every bucket index from zero through `bucket_count - 1` is stored explicitly in
ascending order. Populated and empty buckets both contribute an index; a
missing, repeated, or out-of-order index makes the bucket frame incomplete.
Each populated bucket stores an array opener whose count is the number of
entries in that bucket. Empty buckets store no entry array and have an entry
count of zero. The named first bucket stores its entry count in `bucket_xar`;
later populated buckets store the count immediately after their bucket index.
The named schema prototype is one entry in the first bucket. A bucket is
complete only when its decoded prototype and positional entry bodies equal its
declared entry count exactly; missing and extra bodies both make it incomplete.

`vert_tab` chains bind a solved trim-vertex identifier to its incident `segtab` external identifiers. This vertex namespace is the namespace used by `ent_tab.start_vtx` and `ent_tab.end_vtx`. A trim vertex with exactly two incident carriers can be solved as their intersection evaluated from `var_arr` or the joined saved-section geometry; its identifier differs from a `segtab` point identifier. A neutral sketch line uses its `ent_tab` start and end intersections, not the untrimmed carrier endpoints.
Both intersections must lie on an independently solved line carrier. A neutral
sketch arc likewise uses its `ent_tab` intersections as endpoints; both must
lie on the independently solved `var_arr` or saved-section circle carrier.
Native `vert_tab` rows are retained from their own complete entry bodies. Their
retention does not depend on whether either incident entity is present in the
decoded `ent_tab` subset.
The `ent_ids` array count is the number of incident entity identifiers and is
not fixed at two. The vertex identifier follows those entity identifiers and a
zero terminates the entry. Collision-chain entries may omit the `ent_ids` array
opener; in that form, every identifier before the final vertex identifier is an
incident entity. Geometric intersection coordinates are derived only for rows
whose incident identifiers are distinct and whose every carrier pair has one
intersection at the same section coordinate. This includes junctions with more
than two incident entities. An unsupported pair, a non-unique pair, or
disagreeing pairwise coordinates leaves the vertex unresolved. Repeated vertex
identifiers are semantically ambiguous even when each stored entry body is
complete. When complete `ent_tab` and `vert_tab` tables are both present, their
incident entity sets must agree after entity-to-segment identity resolution.
All stored, saved-section, and propagated coordinates for one trim-vertex
identifier must agree. Conflicting candidates leave that vertex unresolved.
When the two incident `segtab` rows have exactly one common endpoint
`pointid`, that point's complete `var_arr` coordinate is the trim-vertex
coordinate. This join applies to line-line, line-arc, and arc-arc incidences.
Without a unique common point, independently evaluated carriers must have one
unique intersection before a coordinate is assigned. Two circular carriers
define a trim coordinate at internal or external tangency. Secant circular
carriers have two roots and remain unresolved without an independent root
selector. A bounded line and circle define a trim coordinate only when exactly
one algebraic line-circle root has line parameter in the closed segment
interval. Two in-segment roots remain unresolved; roots on the infinite line
outside the segment do not participate.

The positional `vert_tab.chains` opener uses the same bucket-count framing.
Each populated entry begins with `f7 <entry_class>` and stores two incident
`ent_tab.ext_id` values, one trim-vertex identifier, and a terminal zero.

`p_saved_result` contains evaluated section entities and does not define the
authoritative solved trim topology. Its named table remains present when no
entity row is complete. Saved line rows may contain `f0 f7 <ref>`,
`f1 f7 <ref>`, or bare `f7 <ref>` references between their identity, attribute,
and coordinate fields. A saved line retains its identity, references,
attributes, and ordered coordinate prefix when a structural boundary occurs
before all six endpoint-coordinate slots.
Named saved arcs and circles retain their identity and each decoded scalar
field when later center, radius, endpoint, or parameter fields are absent.
Positional saved arcs retain their uniquely joined identity and ordered
12-slot scalar prefix at a structural row boundary.
The line prototype can close with `f1 e3`; positional line rows follow that
close. Within saved-section three-scalar coordinate fields, `18 e5` expands to
the coordinate triple `[0, 1, 0]`. In a saved-line coordinate row, `41` occupies
eight bytes, and `74` and `75` are positive DICT prefixes. Entity references may
also follow the sixth coordinate before the row-closing `e3`. Consecutive
`18 18` bytes are two standalone zero scalar slots; the first `18` does not
consume the second as a dictionary index.

`save_entity_ptr(spline)` carries `i_pnts f9 <count> 03` followed by exactly
`count` section-space XYZ triples. Every coordinate is a scalar-lane value.
The spline identity, declared point count, and complete point prefix remain
present when the point body is incomplete. Neutral spline geometry requires the
complete declared point count.
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

A saved entity identifier is an `order_table.int_id`; joining through that row's `ext_id` binds its evaluated geometry to the corresponding `segtab` entity. A join requires a complete order table and a row whose internal and external identifiers each occur exactly once. The internal identifier must occur on exactly one saved entity before this join applies. Saved rows sharing an internal identifier remain independent construction entities identified by their row offsets. A saved line with two complete section-space XY endpoints supplies that entity's line geometry when its `var_arr` endpoints are relation-backed. The saved-entity and solved-`segtab` sets are one-to-one by entity family. After explicit `order_table` joins, exactly one unmatched saved entity and one unmatched solved entity of the same family bind as the unique remaining pair; multiple unmatched pairs remain unresolved.

When a unique decoded `segtab` row and a unique `order_table` join bind a
complete saved line, arc, circle, or spline to an opaque segment family, the
saved entity supplies the standalone neutral geometry for that external sketch
entity. The opaque row retains the entity's solver identity and does not replace
the complete saved geometry. A complete `segtab` table and a compatible
same-feature generated surface binding make that entity profile geometry.

A saved line, arc, or circle with complete section-space geometry and an
`order_table` join defines a neutral sketch entity under that row's `ext_id`.
Every saved-section row remains a sketch design entity when its analytic
coordinates are incomplete. Its decoded family and unique internal or joined
external identity select native sketch geometry; incomplete coordinates do not
remove the entity or constraints that reference it.
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

The `segtab` positional replay stores `type`, three direction fields, two endpoint point identifiers, `cntrid`, `arcorient`, `verhor`, two radii, and `ext_id`. Within each fixed-width field group, `e4` expands to one, `e5` expands to two zero values, `e6` expands to three zero values, and `f6` expands to one absent value. Expansion must end exactly at the field-group width. A raw `verhor` value of `f5` adds one field before `radius`.

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

Every assignment is a distinct neutral design parameter. A source identifier
assigned once is its parameter name. Repeated assignments use the parameter
names `<identifier>#1`, `<identifier>#2`, and so on in source order and retain
the unqualified identifier as `source_name`. A reference to a repeated
identifier is an ambiguous dependency and does not bind to one occurrence.
Parameter dependencies precede their consumers when the unique dependency
graph is acyclic. A cyclic edge remains source metadata instead of forming an
invalid neutral dependency order.

The identifiers `r`, `theta`, and `z` define cylindrical curve coordinates over the normalized parameter `t` from zero through one. `theta` is in degrees. Constant positive `r` with affine `theta(t)` and affine `z(t)` is a circular helix: its angular travel divided by 360 is the signed revolution count, `z(1) - z(0)` is its signed axial rise, and `theta(0)` is its start angle. The owning curve-equation entity retains the native placement axis.

A curve-equation entity carries its placement in `local_sys f9 <dimensions> <count> <body>`. The scalar body is bounded by the following named field and uses the stateful local-system lane; it is part of the equation entity rather than a reference to a separate coordinate-system entity. For `f9 04 03`, twelve explicit slots have the same support-frame layout as a plane local system: slots 0 through 2 are the first radial direction, slots 3 through 5 are the zero rank marker, slots 6 through 8 are the second radial direction, and slots 9 through 11 are the origin. The explicit slot language includes the `18 e5` basis-vector triple and the standalone-zero forms defined for plane local systems. Orthogonal equal-scale nonzero radial directions define the unit axis by their normalized cross product. The cylindrical coordinates map through this frame as `origin + u*r*cos(theta) + v*r*sin(theta) + axis*z`.

The curve-equation rank-two body `18 e4 0f e4 18 e5 0f 18 e6` expands to first
radial direction `[0, 1, 0]`, zero rank marker `[0, 0, 0]`, second radial
direction `[1, 0, 0]`, and origin `[0, 0, 0]`.

A `protextrude` or `protrevolve` operation references its sweep axis through `gsec3d_ptr` placement fields rather than an inline axis vector. The `srf_array` row `feat_id` binds each materialized carrier to the generating feature. Extruding a section line yields a plane, extruding an arc yields a cylinder, and extruding an interpolation spline yields a degree-one ruled NURBS surface that retains the spline's degree, knot vector, control points, and weights along the directrix parameter. The feature's cap-plane offsets bound the translation parameter, including symmetric and two-sided spans. A closed profile yields cap planes. Each solved carrier in an `ent_tab` profile or a closed point-incidence fallback profile defines an unbounded surface of revolution independently of the operation's angular trim. A line parallel, angled, or perpendicular to the axis yields a cylinder, circular cone, or plane. A circular arc or complete circle with center on or off the axis yields a sphere or torus. An interpolation spline yields a full-turn tensor-product NURBS carrier. Saved analytic entities use their `order_table` source identity and same-feature generated-surface entry exactly as saved splines do. The projected carrier-to-axis vector defines the zero-azimuth direction; construction segments outside the resolved profile do not generate surfaces.

Each closed-profile vertex of a linear sweep defines a line carrier through its
placed section position in the normalized section-normal direction. The
feature's linear extent trims the carrier.

Each closed-profile vertex outside the axis defines a circular orbit carrier.
Its center is the orthogonal projection of the placed vertex onto the
revolution axis, its radius is the projection distance, and the placed radial
vector defines zero azimuth. The operation's angular extent trims the carrier.
A profile vertex on the axis is a rotational singularity and does not define a
circle.

Every bounded feature definition containing section design records is an
ordered planar sketch history node, including definitions containing dimensions
or constraints without geometry. Its sketch, entity, constraint, profile, and
standalone history-feature identities share the definition identity: the
numeric feature-definition identifier when unique, otherwise the bounded
record's source-offset-qualified identifier. A section with exactly one
resolved `gsec3d_ptr` placement owns placed sketch geometry. Other section
snapshots retain unresolved placement and do not generate model-space curves.
When the section transform has a generating feature identifier, that feature
depends on the sketch history node. The sketch node precedes its profile
consumer in construction order. Duplicate transforms remain native placement
records. When the transform names a generating feature, it also requires
exactly one transform for that feature; two definitions claiming the same
feature do not select a profile snapshot.

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

### 8.4 Expanded primitive scalar arrays

`SolidPrimdata` is a PSB compound stream. The named fields `p1`, `p2`, `pts`,
`mv_p_xyz`, and `mv_p_NxNyNzxyz` use `f8 <count>` arrays whose count is the number of
scalar values, not the number of points. `p1` and `p2` contain XYZ endpoints.
`pts` and `mv_p_xyz` contain consecutive XYZ points. `mv_p_NxNyNzxyz` contains consecutive
six-scalar tuples in normal-X, normal-Y, normal-Z, position-X, position-Y,
position-Z order.

These fields use a primitive float32 lane. `00` encodes zero. The three-byte
vector macro `00 28 00` expands to `[0, 1, 0]`. A four-byte positive value beginning `46..4d` maps to
an IEEE-754 binary32 value by subtracting seven from the leading byte. A
four-byte negative value beginning `36..3d` maps by adding `89` hexadecimal
to the leading byte. The remaining three bytes are the unchanged IEEE-754
fraction/exponent tail. A scalar array is complete only when exactly its
declared count can be decoded.

Within `value(prim_tristripsetwithatt)`, `p_accum_set_size f8 <count>`
contains monotonically increasing cumulative vertex counts. Consecutive
differences are triangle-strip lengths and each is at least three.
`mv_p_xyz` supplies exactly the final cumulative count of XYZ positions. An
`mv_p_NxNyNzxyz` array supplies the same position count through complete
normal-position tuples and transfers its first three tuple values as vertex
normals.
Strip triangles alternate winding: `[i,i+1,i+2]`, then `[i,i+2,i+1]`.

### 8.5 Model reference geometry

`MdlRefInfo` stores finite model-space reference lines under an
`ent_list(line)` prototype. The prototype declares `end1 f8 03` and `end2 f8
03`; each following `entity(line)` positional row carries six scalar slots as
`end1.xyz` followed by `end2.xyz`. Intermediate rows end at `e3`; the terminal
row ends at the following named entity record. The row prefix and display
attributes precede this six-slot suffix. The suffix uses the section-local
scalar cache and the signed coordinate DICT lane. `18` immediately before a
complete coordinate token is a standalone zero slot. A positional row defines
a line only when exactly six finite scalars consume the complete suffix. The two
endpoint positions are model coordinates in the active principal length unit.

An `ent_list(line3d)` positional row repeats its canonical entity identifier
on both sides of `e3`, followed by its compact type and `e2` body opener. The
body fields include `end1.xyz`, `end2.xyz`, and `orig_len` as seven consecutive
scalars. A complete spatial line has a nonzero endpoint distance equal to the
absolute stored `orig_len`. The scalar run precedes the remaining positional
fields. Entity references and display fields before or after that run do not
contribute coordinates.

An `ent_list(arc_z)` positional row uses the same repeated-identifier and
`e2` body framing. Its explicit scalar form stores `center.xyz`, positive
`radius`, `end1.xyz`, and `end2.xyz` consecutively after the fixed row prefix.
Both endpoints lie at the stored radius. For non-antipodal endpoints, their
ordered radial vectors define the circle-plane normal by their cross product.
A compressed diameter form omits the explicit center; its endpoint distance is
twice the radius, their midpoint is the center, and their shared model Z value
selects the model-Z plane. The first endpoint defines the reference direction.
The later parameter fields do not alter this carrier equation.

The named entity in `ent_list(conic)` declares compact `id`, `type`, and
`flip` fields; model-coordinate arrays `end1 f8 03` and `end2 f8 03`; scalar
fields `t0`, `t1`, `c1`, and `c2`; and a twelve-slot
`local_sys f9 04 03` body. The endpoint arrays use the model-reference
coordinate lane. Within the local-system body, `4a` is the positive seven-byte
frame-coordinate form, and `18 e5` expands to the three
slots `[0, 1, 0]`; other slots use the same coordinate lane, including an `18`
standalone-zero slot before another complete coordinate; a terminal `18` is
also a zero local-system slot. The conic record retains its coefficients and
parameter fields without assigning ellipse semantics until its frame and
carrier invariants are complete.

A positional conic row repeats its canonical entity identifier on both sides
of the preceding `e3`, then stores `<id> <type> e2`. Its body begins
`02 48 10 00 eb 10 00 00 00 00 <flip>` and replays `end1.xyz`, `end2.xyz`,
`t0`, `t1`, `c1`, `c2`, and the twelve local-system slots in that order. The
compact `11` parameter form remains an opaque one-byte parameter while leaving
the following coefficient and local-system positions aligned. A complete row
consumes all twelve local-system slots before its trailing compound record.

A conic record defines a complete ellipse carrier without interpreting its
parameter tokens when the first two local-system triples are finite orthogonal
unit vectors, the final triple is a finite center, `|c1|` and `|c2|` are
positive, and the two stored endpoints are antipodal about that center at
exactly one of those radii. Their common plane normal is the normalized cross
product of the frame vectors. The larger coefficient magnitude is the
semi-major radius. An endpoint at the semi-major radius supplies the major
direction; an endpoint at the semi-minor radius supplies its in-plane
perpendicular. Records that fail any invariant do not define an ellipse
carrier.
