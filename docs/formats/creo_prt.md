# Creo Parametric `.prt` (PSB): Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

This specification covers the Creo Parametric and Pro/ENGINEER `.prt` variant. Creo files use the PSB (Pro/E Session Binary) container.

Record offsets, field widths, and endianness are also maintained as a machine-checked table in [`docs/layouts/creo.md`](../layouts/creo.md), generated from `docs/layouts/creo.toml`. That table is the canonical source for the numbers; the prose below carries the semantics. `cargo test -p cadmpeg --test layout_tables` proves the two agree.

## 1. Container

A PSB file begins with an ASCII UGC header. The legacy persistence generation
uses an ASCII `P_OBJECT` body, either monolithic or followed by named sections.
Later generations use a table of contents followed by named binary sections.

```text
#UGC:2 P ...
...
#-END_OF_UGC_HEADER\n
#UGC_TOC ...
#END_OF_TOC_HEADER\n
#<SectionName>\n <payload>
```

The header record `#- CMNM <hhh><name>` stores the native model filename. The
three ASCII hexadecimal digits give the filename byte length. Trailing ASCII
spaces pad the counted field. A unique nonempty `.prt` filename supplies the
current relation model name after removing that padding and suffix.
`hhh` is a three-digit ASCII hexadecimal byte count for `name`; padding after
those bytes is not part of the name. A repeated or malformed `CMNM` record does
not establish identity.

Binary model-data sections can store the same identity in the named field
`e0 0a model_name\0`. Its value is a NUL-terminated UTF-8 string. One leading
`f1` framing byte may precede the string and is not part of the value. The
single-byte `e1` value is null and does not establish identity. When no unique
`CMNM` record establishes identity, the first nonempty, non-control-valued
`model_name` field in section order is the root model name. A missing valid
value leaves model identity undefined. This field already contains the relation
model name; the `CMNM` filename form supplies it after its `.prt` suffix is
removed.

Legacy ASCII sections can repeat `model_name` in separate scopes. A nonempty
value other than the `NULL` placeholder in source order is the root source
identity. Other scoped values remain separate model references. Relation
evaluation uses the stricter unique-root resolver and does not use this
fallback when those references conflict.

In the legacy ASCII layout, the byte immediately after
`#-END_OF_UGC_HEADER\n` begins `#P_OBJECT <schema>\n`. `schema` is one or more
ASCII decimal digits. The object ends with `#END_OF_P_OBJECT`, followed
immediately by `\n#Pro/ENGINEER`. These header-adjacent start, end, and banner
markers together select the legacy ASCII layout. The same marker bytes later
in a payload do not select the layout. The banner's `Version <release>`,
`Release <release>`, and `Release<release>` forms store the product release.
A banner without one of these forms has no product release token. Legacy ASCII
data uses `@<name>` field declarations and ASCII value rows. It can continue as
one object or use an undecorated named-section directory.

The named-section form stores its directory after the product banner:

```text
@Toc <toc-id> 0
0 <toc-id> ->
@entry <entry-id> 10
1 <entry-id> [<capacity>]
2 <entry-id> <name> <offset-hex> <stored-length-hex> 0 <version>###...
```

`#` bytes pad each entry row. A row containing only padding is unused. Each
section offset is relative to the first byte of the `#Pro/ENGINEER` banner.
The field before `version` is zero, and `version` is ASCII decimal. The stored
length includes the `#<name>\n` header. A populated entry is valid only when
its computed offset contains that exact header and its stored extent is inside
the file. Valid directory entries are authoritative and ordered by their
computed offsets. A monolithic legacy body has no named sections; its outer
`END_OF_P_OBJECT` and `END_OF_UGC` markers are framing, not sections.

The outer object and each named ASCII attribute section define independent
attribute-ID scopes. An ASCII attribute scope contains these line records:

```text
@<name> <attribute-id> <type-code>
<depth> <attribute-id> <payload>
$<continued-payload>
```

The declaration binds its decimal attribute identifier to a name and decimal
type code in the current scope. A value row stores a decimal object-tree depth,
a locally declared attribute identifier, and the remaining bytes of the line
as its payload. A `$` row continues the immediately preceding value row; a
`$`-prefixed line in any other context is not a continuation record. Attribute
identifiers can be reused in another scope. Named sections with a byte payload
that does not begin with an attribute declaration do not use this line grammar.

Type 0 stores object nodes. `->` and an empty payload are distinct non-null
object forms, and `NULL` is the null form. A positive dimension header stores an
object array. Its direct elements are the following rows at depth one greater
with the same attribute identifier; element subtrees can contain rows at still
greater depths. The direct-element count must equal the product of the extents
for a complete array. A header without direct element rows stores no default
objects.

Within one attribute scope, a row at depth `d > 0` is owned by the most recent
preceding type-0 row at depth `d - 1`. A row at depth zero has no parent. A new
row at a depth closes the prior node at that depth and all of its deeper
descendants. This parent relation applies independently of the row's value
type.

A type-1 scalar payload is a signed decimal 32-bit integer. A type-1 array uses
the positive decimal extent header, continuation rows, comma separators,
terminal-comma rule, and `n*value` run-length form defined below for type 2,
with signed decimal integers in place of compact reals. A one-element `[1]`
array can instead store its integer in the immediately following value row at
depth one greater and with the same attribute identifier. The sum of run counts
must equal the product of the extents. A type-1 array header without element
rows stores no integer elements; its declared extents do not supply default
values.

A type-2 scalar payload is one through sixteen uppercase hexadecimal digits.
The digits are the most-significant nibbles of an IEEE-754 binary64 bit word.
Missing low nibbles are zero. A terminal `R` instead repeats the last written
nibble through the low end of the word. Thus `3FF` is `3FF0000000000000` and
`40396R` is `4039666666666666`. Only finite decoded values are semantic reals.

A type-2 array header is one or more positive decimal extents written as
`[d0][d1]...`. Immediately following `$` rows store its linear element sequence
as comma-separated compact-real tokens. A token `n*H` repeats compact real `H`
`n` times. A terminal comma before the line break adds no element. The sum of
run counts must equal the product of the extents. A one-element `[1]` array can
instead store its compact-real token in the immediately following value row at
depth one greater and with the same attribute identifier. An incomplete array
does not produce a typed value.

Type 3 stores a nullable byte-string scalar. The exact token `NULL` is null;
all other payloads, including an empty payload, are stored byte strings. Type 4
stores its complete scalar payload as a byte string; `NULL` has no special
meaning for type 4. Neither type uses continuation rows.

Type 6 uses the type-2 compact-real scalar and array grammar. Its array run
count must equal the product of the declared extents.

Types 5, 7, 9, and 11 store unsigned decimal 32-bit scalars. Their arrays use
the positive decimal extent headers, `$` continuation rows, comma separators,
terminal-comma rule, and `n*value` run-length form defined for type 1. The sum
of run counts must equal the product of the extents. A one-element `[1]` array
can instead store its unsigned decimal value in the immediately following row
at depth one greater and with the same attribute identifier. A header without
element rows stores no default values.

A type-10 scalar stores the remaining line payload as a byte string. The exact
token `NULL` is a null string, while an empty payload is a stored zero-length
string. The payload has no in-band character-set selector or normalization
marker; its byte sequence is authoritative.

A type-10 array header is one or more positive decimal extents. The first
extent gives the number of direct string elements. Later extents do not
multiply the element-row count. The direct elements are the following rows at
depth one greater with the same attribute identifier. Each direct row stores
one scalar type-10 value. An incomplete array retains its declared dimensions
and present elements; missing rows do not supply default strings.

In legacy ASCII geometry persistence, `Sld_VisGeom.active_geom.srf_array` is
the visible surface-row namespace and `Sld_NonVisGeom.inactive_geom.srf_array`
is the non-visible namespace. The direct elements of each complete array are
the rows. A complete row has one scalar child for each of `geom_type`,
`geom_id`, `feat_id`, `boundary_type`, `next_geom_ptr`, and `orient`. The
`geom_type` values `0x22`, `0x24`, `0x25`, `0x26`, `0x28`, `0x29`, `0x2a`, and
`0x2c` select plane, cylinder, cone, torus-or-sphere, spline, fillet, and
linear-extrusion families, respectively; `0x2a` and `0x2c` select the same
linear-extrusion family. A row is complete only when `boundary_type` is a
defined surface-boundary value and `orient` is `1` or `-1`.

A visible plane or cylinder row has one direct `srf_prim_ptr(plane)` or
`srf_prim_ptr(cylinder)` child of the matching family. Its complete `local_sys`
type-2 array has dimensions `[4][3]` and twelve scalar slots in row-major
order. Columns zero, one, and two are the first radial direction, second
radial direction, and normal or axis; slots nine through eleven are the model
origin. The three directions form a right-handed orthonormal frame. A
cylinder also has one positive finite scalar `radius`. A missing, repeated,
incomplete, non-finite, non-positive, or conflicting field leaves the bounded
row and prototype native.

A visible legacy ASCII cone row has one direct `srf_prim_ptr(cone)` child. Its
complete `local_sys` type-2 array has dimensions `[4][3]` and twelve scalar
slots in row-major order. Columns zero, one, and two are the reference
direction, transverse support direction, and signed axis direction; slots nine
through eleven are the apex. The three directions form a right-handed
orthonormal frame. Its unique scalar `half_angle` is finite, nonzero, and has
absolute value in `(0, pi/2)`. A positive angle selects column two as the axis;
a negative angle selects its negation. The carrier is circular with zero apex
radius, unit radial ratio, and the absolute half-angle. For a negative angle,
the source `v` parameter is negated when it is mapped into that positive-angle
frame; the source `u` parameter is unchanged. A missing, repeated,
incomplete, non-finite, zero, out-of-range, or conflicting field leaves the
bounded row and prototype native.

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
enables block mode. In block mode, code 256 clears the dictionary. Without
block mode (for example `1f 9d 10`), code 256 is a literal dictionary entry.
The initial dictionary contains the 256 one-byte values in slots `0` through
`255`. The initial code width is nine bits. The next free slot is `256` in a
non-block stream and `257` in a block-mode stream because block mode reserves
`256` for the clear code. A decoded code adds one dictionary entry after the
first code in a block; the reader increases the width from `w` to `w + 1`
before the first code that requires a value above `(2^w)-1`. Codes are packed
least significant bit first in code-width-sized byte blocks. Block alignment
resets when the code width increases or, in block mode, a clear code resets it
to nine. Expansion is valid only when the output length equals the TOC
expanded-length field. For model-data sections, the expanded payload begins
directly with its PSB named record. `THMB_IMG_MAIN` is an auxiliary exception:
its expanded payload contains the JPEG payload and is identified by the
`FF D8 FF` marker.

PSB does not use the Parasolid neutral-binary encoding. Parasolid terminology may describe some geometric concepts, but it does not define PSB byte semantics.

### 1.1 Layout families

| Layout       |            Section count | Geometry representation                                               |
| ------------ | -----------------------: | --------------------------------------------------------------------- |
| Legacy ASCII |            0 or multiple | ASCII attribute persistence, optionally partitioned into named sections. |
| ND           | approximately 40 or more | Dense PSB rows in `VisibGeom`, including `srf_array` and `crv_array`. |
| DEPDB        |         approximately 12 | Sparse PSB views and feature/section records.                         |

The outer layout discriminator is the first record in `DEPDB_DATA`. A
`DEPDB_DATA` payload that begins with `e0 00 p_dep_db\0 e3` is a DEPDB layout.
Names of embedded records may carry an `ND:` decoration; that decoration does
not change the outer layout. An `ND:` decoration on an outer section identifies
an ND layout. The complete header-adjacent `P_OBJECT` framing identifies the
legacy ASCII layout. A file with none of these discriminators is an unknown
layout. Section cardinality is descriptive and does not select a layout.

### 1.2 Section map

| Section                          | Contents                                                                                                             |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `VisibGeom`                      | Visible PSB geometry. ND files store dense geometry rows here.                                                       |
| `NovisGeom`                      | Invisible and construction PSB geometry.                                                                             |
| `AllFeatur`                      | Feature rows, generated-entity tables, affected-geometry identifiers, feature references, and DEPDB section recipes. |
| `FeatDefs`                       | Feature definitions, section recipes, placement records, outlines, dimensions, and saved section entities.           |
| `Geomlists`                      | Body-count and quilt-discriminator fields.                                                                           |
| `ActDatums`                      | Active datum-plane and datum-cylinder geometry under `act_datum_geoms → srf_array`.                                  |
| `DEPDB_DATA`                     | Persistence data used by DEPDB-layout parts, including embedded geometry namespaces and feature-definition records.  |
| `FamilyInf`                      | Family-table driver pointer for configurations.                                                                      |
| `MdlRefInfo`                     | Model-space reference entities, including finite line endpoints.                                                     |
| `NeuPrtSld` and display sections | Material, appearance, display, and tessellation data.                                                                |
| `THMB_IMG_MAIN`                  | JPEG thumbnail. Its raw payload begins with `FF D8 FF` or uses Unix-compress framing; after expansion, bytes from the `FF D8 FF` marker through the payload end are the JPEG payload. It does not contain model geometry. |

### 1.3 Units

`_principal_sys_units_id` identifies the active coordinate unit system.

| Value | Unit system                         |
| ----: | ----------------------------------- |
|  `51` | millimeter-Newton-Second (`mmNs`)   |
|  `54` | inch-pound-mass-second (`inLbmS`)   |
|  `55` | millimeter-Kilogram-Second (`mmKs`) |

Binary selector `54` stores model lengths in inches. The neutral model
converts every stored length, coordinate, distance, radius, linear tolerance,
length-bearing Boolean-operation tolerance, and length-bearing parameter to
millimeters by multiplying it by `25.4`.
Surface and curve parameter coordinates use the same conversion on each
length-valued parameter axis; angular and dimensionless parameters are
unchanged.
This includes model-space origins and distances carried by feature motions,
explicit pattern centers, procedural construction vectors and origins, and
procedural cache fit tolerances; unit vectors, angles, scale factors, and
parameter intervals are unchanged.

In legacy ASCII persistence, the unique type-10 `principal_sys_units` scalar
identifies the active system. `millimeter Newton Second (mmNs)` stores lengths
in millimeters. `Inch lbm Second (Pro/E Default)` stores lengths in inches, so
lengths are multiplied by `25.4` for canonical millimeters. An absent,
repeated, differently typed, continued, or unrecognized scalar does not select
a coordinate unit system.

When that scalar is absent, one complete `unit_arr` object array can select the
legacy length unit. Its first direct element is the active length record. That
record has one `unit_type` scalar equal to `0`, one nonempty UTF-8 `name`, and
one finite positive `factor` scalar. `factor` is the number of inches in one
stored length unit; the canonical length scale is `25.4 * factor` millimeters.
Every direct element must be a complete `unit_arr` object, and the array and
all three selected fields must be unique. A missing, repeated, incomplete,
non-finite, or conflicting record does not select a coordinate unit system.

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
`7e→3F F3`/`c6→BF F3`, `7f→3F F4`/`c7→BF F4`,
`80→3F F5`/`c8→BF F5`, and
`97→40 0C`/`dd→C0 0C`. Each prefix is followed by the remaining six IEEE
bytes. Prefix `51` maps to `3F C6`. The defined negative DICT members are
`a7..ac→BF D3..D8`, `ae→BF DA`, `b3→BF E0`, `bd→BF EA`, `c3→BF F0`,
`c6..ce→BF F3..FB`,
`d0→BF FE`, `d2→C0 00`, `d4→C0 02`, `d6→C0 04`, `d8→C0 06`, and
`da→C0 08`. Prefix `d5` instead
uses the lane-specific negative sub-unit form and reconstructs
`BF <tail6> 00`. The positive sub-unit form `4f <tail6>` reconstructs
`3F <tail6> 00`. Its eight-byte world-coordinate form `2d <tail7>`
reconstructs `40 <tail7>`; the same form is positive in the saved-section
coordinate lane. Its `19 <tail7>`, `32 <tail7>`, `37 <tail7>`, and
`41 <tail7>` forms reconstruct `3F <tail7>`. The unresolved `00 XX YY` and
`34 XX YY` forms occupy three bytes; the unresolved `01 XX YY ZZ` form
occupies four.

Lane-specific seven-byte forms include `6a <tail6>` for positive IEEE with leading byte `40` and implicit trailing `00`; `9e <tail6>` and `a3 <tail6>` for positive and negative forms paired with the section-local `46` cache; `b9`, `d1`, `d3`, `de`, and `df` for negative sub-unit forms with leading byte `BF`; and `41`, `4b`, `66`, `67`, `68`, `77`, and `82..8f` for positive sub-unit forms with leading byte `3F`. A paired form finds exactly one distinct `46 <byte1> <tail6>` token with the same six-byte tail and reconstructs `40 <byte1> <tail6>` for `9e` or `C0 <byte1> <tail6>` for `a3`. Duplicate copies of one token do not add a candidate; differing `<byte1>` values leave the paired scalar unresolved.

In positional surface and curve row lanes, `71 <tail6>` is a seven-byte
sub-unit form reconstructed as `3F <tail6> 00`. In named scalar lanes, `71`
occupies eight source bytes and reconstructs as `3F <tail7>`.
In a positional surface row, `a0 <tail6>` is the negative DICT form
`C0 15 <tail6>`.
The positional surface-row lane defines the same-tail sign pair
`73→3F E8` and `bb→BF E8`. Its `a7 <tail6>` form reconstructs
`BF D3 <tail6>`.
The positional surface-row lane maps `d1`, `d3`, `de`, and `df` to IEEE
prefixes `3F FF`, `40 01`, `40 10`, and `40 11`, respectively.
In that lane, `92 <signed-i48>` and `da <signed-i48>` store an exact signed
six-byte big-endian integer and convert it directly to a finite scalar.

Each record grammar defines the DICT lane for its scalar slots. A decoder must not apply DICT sign rules across unrelated record grammars.

#### World-coordinate tokens

World-coordinate tokens normally occupy eight bytes. Their final seven bytes hold the IEEE mantissa and low exponent. In the positional-outline/world lane, `46` denotes a positive token and `2d` denotes a negative token; `2d` consumes the complete eight-byte token in that lane. A field-specific compact world lane stores a negative coordinate as `2d <tail6>`, reconstructed as `C0 <tail6> 00`. The enclosing field frame distinguishes the seven-byte and eight-byte forms; the surface family does not.

#### Constants and cache references

`0d` encodes negative one, `0f` and `e6` encode zero, and `e4` encodes one. In row and `f9` scalar lanes, `e8 00` encodes standalone `1.0`; other contexts use a different selector grammar. `18 <index>` indexes a raw section-local `46` cache. Build that cache by scanning the raw section bytes, including `46` values that occur within other token tails. The standalone-zero test is local to the enclosing scalar lane: `18` followed by the first byte of a scalar form defined in that lane encodes zero, and the following byte begins a new token. A prefix defined only by another lane remains a compact cache index. The positional row lane additionally defines `0e` as negative one half; the positional surface-row lane additionally defines `73`, `92`, `a0`, `bb`, and `da`. In a saved-line coordinate row, `18` immediately before the row close or trailing entity reference is a standalone zero. At the byte-bounded end of a positional scalar-slot array, terminal `18` is a standalone zero.

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
| Boundary bytes        | `00`, `01`, `06`, `08`, `f6`                                                         |

A counted surface-array frame ends at the next `srf_array`, `crv_array`,
`lo_array`, or `qlt_array` label. Header-shaped bytes outside that frame do not
belong to it. A byte range owned by a bounded named prototype parameter cannot
start a sibling surface row. The count is the frame's slot extent; a slot may
have no materialized fixed-prefix row header. Every unique validated row inside
the frame is retained when the validated-row count does not exceed the stored
count. More validated headers than slots makes the frame invalid and withholds
all rows from it. A frame is complete only when its number of unique validated
rows equals the stored count. Datum and prototype joins require a complete
frame and do not use rows from an incomplete frame.

A positional surface parameter body ends at its compound close, the next validated surface-row header, or a named-record header. A named-record boundary has `e0`, a field-type byte in `00..24`, a nonempty ASCII identifier beginning with a letter, and a null terminator. An `e0` byte inside an opaque numeric or pointer token is not a boundary.

Row bodies end at a valid row-close marker, named-record header, or a following positional row header that matches the row schema. Scalar-token length takes precedence over structural-byte interpretation, so an `e3` byte inside a complete scalar does not close the row. The first row after `srf_array\0` can be a named-record row with the fields `geom_id`, `geom_type`, `feat_id`, `orient`, `boundary_type`, `next_geom_ptr`, `envlp`, `outline`, and `local_sys`.
Named-record rows require one defined `orient` byte and one defined `boundary_type` byte from the row header value sets. An absent or undefined discriminator does not define a surface row.
`geom_id` is unique within one selected namespace. Multiple header-shaped byte
sequences carrying the same identifier are ambiguous and are not surface rows.
A nonzero `next_geom_ptr` may reference a rowless face use, so materialization of
its target is not a row-acceptance condition.
Plane envelope and post-envelope local-system bodies use the same grammar for
each defined boundary byte; `boundary_type` does not select their scalar layout.
The family-aware positional parameter body owns complete scalar tokens. Its
compound close bounds the plane envelope when a header-shaped byte inside a
complete parameter scalar interrupts the structural envelope walk. The
following compound-close-bounded body is the local system.
A positional row may then carry a complete contour chain after the local-system
close. Its grammar is:

```
contour          := <curve_hdr_ptr_ci2> <trv> <envlp0> <envlp1> <envlp2> <envlp3> <close>
chain            := contour_terminal
                  | contour_intermediate ([<f7> <reference_ci>] contour_intermediate)*
                    [<f7> <reference_ci>] contour_terminal
curve_hdr_ptr_ci2 := canonical two-byte compact integer (`80..bf` lead)
trv              := `00` | `01` | `02` | `03` | `f6`
contour_intermediate := contour with `close = e3`
contour_terminal := contour with `close = e1`
```

Each `envlp` slot uses the positional surface-row scalar lane. The four slots
remain in source order. `34 <byte> <byte>` occupies one unresolved three-byte
slot and has no numeric value. An `e3` contour close may be followed by the
optional separator reference `f7 <reference_ci>` before the next contour. The
terminal `e1` closes the chain; row-tail data follows it. A chain requires its
terminal marker and all four envelope slots for every entry.
A `geom_type = 22`, `boundary_type = 01`, `next_geom_ptr = 0` row is an
unbounded feature plane. When it is the unique plane row carrying its
`feat_id`, its placed carrier is the datum-plane definition for that feature.

### 3.1 Surface families

| `geom_type` | Surface family                               |
| ----------- | -------------------------------------------- |
| `22`        | Plane                                        |
| `24`        | Cylinder                                     |
| `25`        | Cone                                         |
| `26`        | Torus or sphere representation               |
| `28`        | Spline surface                               |
| `29`        | Fillet surface                               |
| `2a`        | Linear-extrusion family, `ruled_srf` variant |
| `2c`        | Linear-extrusion family, `tab_cyl` variant   |

A decoder must not infer the kind of a row without a materialized parameter row from adjacent rows or topology.

### 3.2 Surface prototypes

`srf_prim_ptr` records contain the surface prototype fields. The prototype block closes with `f1 f7 <entity_ref> e3`. A scalar field ending with bare `18` before that structural close stores zero.
In a named analytic surface-radius field, prefix `28` stores a positive
eight-byte scalar: `3f` is the implicit first IEEE-754 byte and the seven bytes
after the prefix are bytes one through seven.
Within `5b` through `a3`, prefixes not assigned a generic named-scalar form
store a positive DICT scalar in seven bytes. The first two IEEE-754 bytes equal
the big-endian integer `3f75 + prefix`; the six bytes after the prefix are
bytes two through seven. Generic named-scalar forms retain precedence.
The `par_v_0` and `par_v_1` fillet parameter bounds use this positive DICT
lane and the generic named-scalar forms, but not the radius-only `28` form.
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

In legacy ASCII persistence, a `geom_type = 0x28` (decimal 40) row has a model-space spline
carrier only when its direct `srf_prim_ptr(splsrf)` child has one complete type-2 array for
each of `i_points`, `u_params`, `v_params`, `u_tangts`, `v_tangts`, and `uv_deriv`.
`i_points` contains `U * V` model-space interpolation triples in u-major order. The
three derivative fields also contain `U * V` triples in that order. The carrier
construction selects the lower-u and upper-u rows of `u_tangts`, the lower-v and
upper-v columns of `v_tangts`, and the four `uv_deriv` corner entries in
`[lower-u/lower-v, upper-u/lower-v, lower-u/upper-v, upper-u/upper-v]` order.
The two parameter arrays are strictly increasing and finite, with at least two
values each. These fields define a non-rational, non-periodic clamped bicubic
NURBS surface with the source parameter ranges. A missing, duplicate,
non-array, dimensionally incomplete, or non-finite field does not define a
carrier. This carrier join does not by itself prove a trim curve, face
instance, or neutral B-rep. The interpolation arrays do not join the surface
to a trim or intersection curve; the spline surface therefore supplies no
pcurve endpoint witness or face-loop admission witness until that join is
present.

Named `i_pnts` and `c_pnts` fields inside a nested curve record following a
torus prototype belong to that curve, not to the analytic torus prototype.

A nested `curve(b_spline)` record uses compact integers for `id`, `type`,
`tan_cond`, and `degree`. Its `params` array and `c_pnts` reference array are
independent fields. A `c_pnts` body `f8 <count> f7 <start_id> fb` denotes the
contiguous entity-reference range `start_id .. start_id + count`. `flip f1
<value_ci>` stores one compact integer. `offset_type <value_ci> f1 f7
<class-id>` stores one compact integer followed by a nonzero canonical class
reference. Both fields retain their complete wrapper bytes. `dum_array`,
`data_dbls`, and `data_type` are separate named fields. A `tan_spline` field
with no bytes before the next named-record header has an empty body. A
`frst_cntr_crv_hdr_ptr` field and its following `trv` field each store one
compact integer; the `trv` header terminates the preceding pointer body. A
`frst_cntr_ptr` field and a `next_cntr_ptr` field each store one compact
integer. The `envlp`, `outline`, and `srf_flip_dat` fields retain their bounded
body bytes exactly; their bodies end at the next named-record header or the
prototype close. These fields are part of the prototype's contour-chain data
and do not by themselves materialize a surface row or a face.
count-prefixed compact-integer array is typed as such only when exactly the
declared number of compact integers consumes the entire bounded field body;
trailing bytes make the field opaque.
`parent_feats` may append the exact trailer
`f7 <class-id> <entity-id> [e1 [f6 f6]]` after its declared compact-integer
array. Both trailer identifiers use the canonical entity-reference grammar and
are nonzero. The trailer does not add parent-feature identifiers.
Each array identifier names a feature that consumes the prototype surface. The
consuming feature depends on the adjacent surface row's `feat_id`.
In a counted `params` scalar array, `e5` supplies two consecutive zero slots
and `e6` supplies three. The expanded slots must exactly match the declared
count and the scalar tokens must consume the complete bounded field body.
`18` is either one standalone zero slot or a section-cache reference. The
declared count and field boundary must select exactly one complete tokenization;
zero or multiple complete tokenizations leave the field opaque.

Named prototype fields describe the first surface instance. Within a complete
counted `srf_array` frame, the immediately preceding row is the first instance
when it has the prototype's family. When the immediately preceding row has a
different family, or when the prototype occurs before the first row header, the
following same-family row is the first instance. Positional row bodies carry
the per-instance values for subsequent instances.

A later positional `geom_type = 0x28` row replays its `splsrf` prototype
positionally. The row and prototype must be in one complete counted `srf_array`
frame, the frame must contain exactly one spline prototype, and the later row
must have the first instance's `feat_id`. The prototype supplies the field
order and all array extents; the row carries no field names, array headers, or
counts. Its body contains the leading envelope and outline, closed by `e3`,
then the two bare `tan_cond` compact integers, followed by `U * V` u-major
interpolation-point triples, `2 * V` u-boundary derivative triples, `2 * U`
v-boundary derivative triples, four mixed-derivative triples, `U` u-parameter
scalars, and `V` v-parameter scalars. The row's trailing frames use the
existing positional-row grammar. Every scalar is consumed in the spline
prototype's corresponding scalar lane; no byte is skipped between fields. The
replay body must end at its second structural `e3`, and the echoed tangent
conditions must equal the prototype values. A missing, duplicate, incomplete,
ambiguous, or byte-inexact replay remains native. A complete replay feeds the
same non-rational, non-periodic clamped bicubic interpolation constructor as
the first instance. It does not by itself bind a pcurve, trim, face, or neutral
B-rep component.

In the ND layout, a complete plane, cylinder, or torus prototype `local_sys` and family parameters define the first instance carrier. Slots 0 through 2 contain the first support direction. Slots 6 through 8 contain the second support direction. A torus prototype also admits slots 3 through 5 as a candidate second support direction. Exactly one admitted candidate has the same scale as the first direction and is orthogonal to it. Slots 9 through 11 contain the origin. The bounded scalar body encodes its declared slots sequentially; no byte may be skipped between slot encodings. Each positional plane origin slot uses its row-lane scalar form when the prefix defines one. Other slot-9 prefixes use the signed first-coordinate lane defined for tabulated-cylinder directrix points; other slot-10 and slot-11 prefixes use the corresponding second-coordinate lane. The first-coordinate lane's `4a` form stores a negative coordinate in seven bytes: `c0` is the implicit first IEEE-754 byte, the six bytes after `4a` are bytes one through six, and the low byte is zero. The normalized cross product of the two orthogonal, equal-scale support directions is the analytic axis. A bare terminal `18` in the bounded `local_sys` body occupies one zero slot. Terminal `00 0c 98` in a positional plane support frame also occupies one zero origin slot. The same byte triple separates the two bound pairs in a cylinder outline; its meaning is fixed by the enclosing record grammar. A plane passes through the local-system origin, uses the analytic axis as its normal, and uses the first support direction as its parameter-space reference direction. A cylinder uses that axis and reference direction and requires one positive finite `radius`. A zero torus `radius1` and positive `radius2` define a sphere centered at the local-system origin. Positive `radius1` and `radius2` define a torus with respective major and minor radii centered at that origin. A complete tagged radius trailer on the first associated type-26 row overrides the prototype `radius1` and `radius2` for that instance; the prototype local system remains the placement source, and an overridden `radius1 = 0` selects a sphere.
Within a named prototype `local_sys`, `e7 <count>` advances over `count`
inherited scalar slots. The count is a positive compact integer, must not cross
the declared array extent, and does not assign values to those slots. Scalar
decoding resumes at the first slot after the inherited interval.

A prefixed orthogonal positional plane support frame begins with a zero-rank
triple followed by `a, 0, b, e4, 0, m` and three origin coordinates. The
support scalars satisfy
`a² + b² = 1` and `|m| = |a|`. The zero-rank triple occupies slots 3 through
5, `e4` copies `b`, and `m` supplies the magnitude of the negated first
component. The resulting support directions are `(a, 0, b)` and
`(b, 0, -a)`; the final three scalars occupy slots 9 through 11.
The compact axis form begins `18 0f 18 e5 0f e4 18 e4`; that prefix defines
support directions `(1, 0, 0)` and `(0, 0, -1)` with a zero middle rank. Its
three following scalars are the origin coordinates.
The prefixes `0f 18 e6 0f 18 10 18` and
`18 e4 10 e4 18 e5 0f 18` each define support directions `(0, 1, 0)` and
`(0, 0, 1)` with a zero middle rank. Their three following scalars are the
origin coordinates. The resulting plane normal is `(1, 0, 0)`.
A trailing-rank orthogonal form stores `a, 0, b, e4, 0, m`, a zero-rank
triple, and three origin coordinates. It has the same `a² + b² = 1`,
`|m| = |a|`, copied-`b`, and negated-`a` semantics as the prefixed form.
A reflected-component form stores `(a, 0, b)`, a zero-rank triple,
`(b, 0, a)`, and three origin coordinates. The final stored `a` is reflected
across zero, so the second support direction is `(b, 0, -a)`.
A trailing-rank reflected form stores `(0, a, b)`, `(0, b, a)`, the rank
triple `(0, 0, 1)`, and three origin coordinates. The first two stored triples
satisfy `a² + b² = 1`; reflecting the final stored `a` gives support directions
`(0, a, b)` and `(0, b, -a)`.
In plane-support slot 8, prefix `50` stores a negative component in seven
bytes. IEEE-754 bytes zero and one are implicit `bf c2`; the six bytes after
the prefix are bytes two through seven.
In plane-support slot 6, prefix `4e` stores a positive component in seven
bytes. IEEE-754 bytes zero and one are implicit `3f cf`; the six bytes after
the prefix are bytes two through seven.

Two five-coordinate type-26 rows for one zero-`radius1` prototype encode the
two hemispheres of one Z-axis sphere. Each row stores
`x_min, z_start, y_min, radial_max, z_end`. The two radial minima are equal,
`radial_max - x_min` is the sphere diameter, and each axial span is one radius.
The axial spans share only the sphere-center endpoint and their union is one
diameter. The X and Y center coordinates are the midpoint of the radial range;
the shared axial endpoint is the Z center coordinate.
The prototype association for the frame and feature is unique. A frame and
feature with multiple torus-prototype associations does not pair its rows.

A complete plane envelope whose two model-space diagonal corners have exactly
one byte-equal coordinate defines an axis-aligned plane through that held
coordinate. The other two coordinate pairs are byte-distinct. This defines
the plane equation independently of the positional `local_sys`; it does not
define the plane's parameter-space reference direction.

A terminal-corner positional plane body ends `f7 1f` and has exactly one
scalar frame ending immediately before that trailer. The frame contains six
through ten scalars. Its final six scalars are two model-space XYZ diagonal
corners; preceding frame scalars and prefix bytes do not contribute to the
plane equation. Exactly one corner-coordinate pair is equal. That held
coordinate defines the axis-aligned plane equation.

The split terminal-corner form also ends `f7 1f`. It has one leading frame of
one or two auxiliary scalars and one terminal frame of exactly eight scalars.
One complete opaque prefix precedes the leading frame, one complete opaque
control span separates the frames, and no other bytes precede the trailer. The
terminal frame's first two scalars are auxiliary and its final six scalars are
the two XYZ corners. Exactly one corner-coordinate pair is equal and defines
the axis-aligned plane equation.

A complete positional cylinder body begins `11 18 13`, followed by axial
length and the first corner's three coordinates, then the second corner's first
two coordinates in the positional surface-row scalar lane. An opaque third
coordinate follows that prefix. The body then contains exactly one complete
twelve-slot positional `local_sys` and ends with one positive scalar radius.
The terminal radius has exactly one byte-valid positive scalar start that
consumes the body remainder; zero or multiple starts leave the body unresolved.
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

A complete directrix-interval positional cylinder begins `18 e4 11`,
`18 e4 00 11 07`, or `00 11 07 18 13`. Seven scalars follow in the first
tabulated-cylinder directrix-coordinate lane: axial lower bound, first radial
bound, first transverse bound, signed axial length, second radial bound,
transverse center, and axial upper bound. The body then contains the exact
trailer `f7 17 e3`; later row operands can follow that close. The difference
of the axial bounds equals the signed length. Half the signed radial-bound
difference is nonzero, and the transverse center differs from the first
transverse bound by its absolute value. The radial midpoint, transverse
center, and axial lower bound define the origin. The sign of the axial length
defines the model-Z axis direction, the sign of the radial difference defines
the model-X reference direction, and the absolute half-difference is the
radius. The absolute signed length is the bounded axial length.

A signed axis-aligned positional cylinder begins `11`, one nonzero signed
axial length, and `13`, followed by one auxiliary scalar and two XYZ corners.
All seven fields after `13` use the positional surface-row scalar lane. The
auxiliary scalar has magnitude less than the axial length. Of the three corner
spans, exactly one equals the absolute axial length; of the remaining two,
exactly one is twice the other. The smaller radial span is the radius and the
larger is the diameter. Without a trailer, the second corner supplies the
axial endpoint and the axis points toward the first corner. A terminal
`f7 17` instead selects the first axial endpoint and points toward the second.
The diameter midpoint and the second corner's radius-span coordinate complete
the origin. The reference direction points along the diameter from its
midpoint toward the same corner as the axis.

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

A signed zero-support positional cylinder begins `11`, one nonzero signed
axial length, and `13`, followed by two three-coordinate corners in stored
`Z, X, Y` order. Immediately before its three-scalar model-space origin it
stores the support suffix `10 18 e6 0f 18 0f 18`; all nine support slots are
zero. A positive radius terminates the body. In XYZ order, one corner span
equals the absolute axial length, one equals twice the radius, and one equals
the radius. The origin lies at one axial endpoint, at the diameter-span
midpoint, and at one endpoint of the radius span. The axis points from the
origin endpoint toward the other endpoint. The sign of the stored length
selects the diameter-axis reference direction.

A signed radial-envelope positional cylinder begins `11`, one scalar, and
`13`, followed by seven surface-row scalars. The final six scalars store two
radial XY pairs, one axial sample, and one upper axial endpoint in the order
`x0, y0, z_sample, x1, y1, z1`. Exactly one radial span is twice the other;
the smaller span is the positive radius, the larger is the diameter, and the
second bound on the radius-span coordinate is the center coordinate. The axial
sample lies in the closed interval ending at `z1`. A body ending in `f7 19`
uses the negative scalar before `13` as its signed axial length and the first
scalar after `13` as an auxiliary bound. A body ending after the seventh scalar
uses the positive first scalar after `13` as its axial length and the scalar
before `13` as the auxiliary bound. The auxiliary bound has smaller magnitude
than the axial length. The negative form originates at `z1` and points toward
`z1 - abs(length)`; the positive form originates at that lower endpoint and
points toward `z1`. The diameter direction follows the axis sign.

A terminal-zero signed radial-envelope form stores a negative signed axial
length before `13`, an auxiliary scalar, five surface-row coordinates, and a
terminal bare `18` for `z1 = 0`. The auxiliary magnitude is smaller than the
length. The five preceding coordinates are `x0, y0, z_sample, x1, y1`; the
same radial-span and axial-sample invariants apply. The carrier originates at
`z1`, points in negative model Z, and uses the diameter direction selected by
that axis sense.

A precise center-to-edge positional cylinder begins with `18`, one opaque byte,
and one finite seven-byte body-local control scalar. A nonzero signed axial
extent and two XYZ samples follow in the surface-row scalar lane, then the
exact trailer `f7 19`. Exactly two sample-coordinate spans are equal and
nonzero; they are radial center-to-edge spans and their common magnitude is
the radius. The remaining span is axial and is greater than the radius. The
first sample supplies both radial center coordinates. Adding the signed extent
to the second sample's axial coordinate gives the precise origin coordinate;
the first sample's coarse axial coordinate lies between that origin and the
second sample. The axis points from the precise origin toward the second
sample. Of the two radial
model axes, the later XYZ coordinate is the parameter-space reference axis and
points from the first sample toward the second.

A precise held-center positional cylinder begins with `18`, two opaque bytes,
and one finite seven-byte body-local control scalar. It then stores a nonzero
signed axial extent, first model-X sample, one held radial center, literal `e4`,
second model-X sample, one radial edge, another literal `e4`, and the exact
trailer `f7 19`. Scalar fields use the surface-row lane; each `e4` is a unit
radius and the two radius values are equal. The held-center-to-edge distance
equals that radius. Subtracting the signed extent from the first X sample gives
the precise X origin. The second X sample lies between that origin and the
first sample and differs from the precise origin by at most one radius. The
cylinder's Y and Z origin coordinates both equal the held center. Its axis has
the sign of the signed extent on model X, and its reference direction is model
Z from the held center toward the radial edge.

A local-system-suffix positional cylinder ends with one complete twelve-slot
support frame and one positive scalar radius. Exactly one suffix before the
radius decodes as a cylinder local system whose first and second three-slot
support vectors are nonzero, equal-length, and orthogonal. Their normalized
cross product is the cylinder axis; the normalized first vector is the
parameter-space reference direction. Slots 9 through 11 are the model-space
origin and use the first tabulated-cylinder coordinate lane, including its
signed `46` form. The terminal scalar is the radius. This body defines an
unbounded carrier and no axial extent. Prefix bytes before the unique complete
suffix do not contribute carrier geometry.

An inline cylinder may use the `11 10 13` placement-witness prefix before the
local-system suffix. The prefix stores one zero auxiliary slot, transverse
bounds `b0`, `b1` separated by literal `10`, the other transverse center
coordinate `c`, one complete `19` or `32` model-reference token, the signed-half
marker `0e`, and one `f7 <reference>` replay trailer. The omitted axial center
coordinate is zero. The prefix placement is
`origin = (midpoint(b0, b1), 0, c)`, `axis = +Z`,
`ref_direction = sign(b0 - b1) X`, `radius = abs(b0 - b1) / 2`, and no bounded
axial extent. The `e3` after the replay trailer starts the local-system suffix.
The suffix must provide one complete finite orthonormal frame and one positive
radius. Its candidate placement must agree with the prefix origin, +Z axis,
and radius; the prefix then resolves any compact-image sign ambiguity. A
missing or conflicting witness retains the row as native data.

A compound-local-system positional cylinder stores one complete twelve-slot
support frame and one positive radius between two compound closes. Row-local
control bytes can follow the second close. In its reflected XY support form,
the stored triples are `(a, b, 0)`, `(b, a, 0)`, and `(1, 0, 0)`, with
`a² + b² = 1`. The second stored `a` is reflected across zero, producing the
orthogonal support pair `(a, b, 0)` and `(b, -a, 0)`. The normalized cross
product is the cylinder axis, the normalized first vector is the reference
direction, slots 9 through 11 are the origin, and the following scalar is the
radius. This body defines an unbounded carrier.

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

A signed-prefix axial/radial cylinder begins `11`, one nonzero signed axial
length, and `13`. It then stores one auxiliary scalar, first axial coordinate,
radial sample, literal `e4`, second axial coordinate, one complete `19`
model-reference token, radial center, and the exact trailer `f7 17`. Numeric
fields use the positional surface-row scalar lane. The auxiliary magnitude is
less than the axial length, and the axial-coordinate span equals the absolute
stored length. The second axial coordinate is the model X origin and the axis
points toward the first. Model Y is zero. The radial center is the model Z
origin; its distance from the radial sample is the radius, and the reference
direction has the opposite model Z sign. The model-reference token does not
contribute a geometric coordinate.

A repeated-diameter type-24 round body stores two scalar diameter endpoints
and two model-space XYZ extent endpoints. The body is either one contiguous
scalar frame after `15` or `00 15 1c`, or two scalar frames separated by the
literal byte `12`. In the compact-control form, one selector in `11..14`
precedes a one-scalar first-diameter frame, another selector in `11..14`
separates it from the seven-scalar second-diameter-and-extent frame. The
selectors do not contribute geometry. Three split-control forms use the same
one-scalar and seven-scalar frames: `14 <first> 00 13 1a <second-and-extents>`
and `00 11 13 <first> 14 <second-and-extents>`, plus `12 <first> 00 11 13
<second-and-extents>`. In the auxiliary-control form, selector
`19` or `32` precedes a two-scalar frame containing an auxiliary value and the
first diameter endpoint; literal `12` separates that frame from the
seven-scalar second-diameter-and-extent frame. The selector and auxiliary value
do not contribute geometry. The prefixed-control form begins with the five-byte
control field `eb ba <payload3>`, followed by the one-scalar first-diameter
frame, literal `12`, and the seven-scalar second-diameter-and-extent frame. The
control field does not contribute geometry. A replay body may append one
complete reference encoded as `f7 <reference-id>` after the last scalar frame;
that reference does not alter the envelope. The diameter endpoints are distinct. Exactly one
coordinate span between the extent endpoints equals their absolute difference.
That coordinate
is radial: its midpoint is the corresponding cylinder-origin
coordinate, its sign from the first endpoint to the second defines the
reference direction, and half its span is the radius. Removing that radial
component from the extent-endpoint displacement produces the nonzero cylinder
axis vector. Its magnitude is the bounded axial length, its normalized value
is the axis direction, and the first extent endpoint supplies the other two
origin coordinates.

A generated type-24 round-edge body has a control shell, two edge parameters,
two model-space endpoint triples, and an optional generated-entity reference.
This production applies only when the bounded row has no complete inline
non-plane envelope or local-system body. An inline body owns the carrier and
is not reinterpreted as a generated round-edge replay.
The control shell is one of `1b <control> 00`, `34 <control> 00`, `32
<scalar>`, `19 <scalar>`, `eb ba <payload3>`, `ec ba <payload3>`, `ed ba
<payload3>`, `5a b2`, or a bare `18`. The two edge parameters use the first
tabulated-cylinder directrix-coordinate lane. A positive DICT parameter with
prefix `p` in `4b..a3` reconstructs IEEE bytes two through seven with
`q = (p + 75) mod 256`; byte one is `3f` when `q >= 80` and `40` otherwise.
The signed directrix lattice handles all other parameter prefixes. The
separator after the first parameter is one byte in `11..14`, or three bytes
beginning with `00` or `34`; separator payload bytes are not scalar fields.
The second parameter follows the separator.

The six endpoint coordinates use the same first tabulated-cylinder
directrix-coordinate lane in XYZ order. A complete endpoint sequence may end
at the body boundary or at `f7 <reference-id>`; a compound close may follow
that sequence when another bounded body is present. The reference identifies
the generated entity and does not supply a coordinate. The two triples are
samples on the generated round-edge carrier, not a carrier by themselves. A
class-913 replay supplies the constant rolling radius. The generated-entity
and topology join supplies the two incident support planes. Normalize their
normals and form every intersection of the two planes offset by `+radius` or
`-radius`. Keep a candidate only when the endpoints bind one-to-one to the two
unoffset support planes and both endpoint distances from the candidate line
equal the replay radius. Coincident candidate lines are one carrier; admit the
row only when one line remains. For perpendicular support planes this is the
mixed transverse-coordinate construction. The stored parameter interval is an
arc interval and does not become a cylinder length; a geometric axial span may
be retained when the endpoint projection is positive. Missing, conflicting, or
non-unique radius, ownership, support, or line evidence retains the row native.

For perpendicular support planes, project the endpoint displacement onto each
support normal. The two absolute projections have the same positive magnitude;
that magnitude is the radius. Offset each support plane toward the cylinder by
the derived radius and intersect the offset planes. Both endpoints must lie at
the derived radius from the resulting line. This construction does not require
a separate replay radius. When a replay radius is present, both constructions
must define the same carrier.

The single-diameter type-24 form has one terminal frame of exactly eight
slots. Its first slot is auxiliary, its second slot is the positive diameter,
and its final six slots are two XYZ extent endpoints. Exactly one absolute
coordinate span equals the diameter. That coordinate is radial and follows
the same midpoint and reference-direction rules as the repeated-diameter
form. Removing it from the extent-endpoint displacement produces the nonzero
cylinder axis vector and bounded axial length.

In the terminal square-radial type-24 form, the final scalar frame has six
through nine slots and reaches the body end, one terminal control byte `00`,
`10`, or `18`, or one complete terminal entity reference. Its final six slots
are two opposite XYZ envelope corners;
preceding slots and frames are auxiliary. Exactly two absolute coordinate
spans are equal and nonzero. They are the radial diameters. A distinct nonzero
span defines the cylinder axis and finite axial length. The two radial
midpoints and the first axial coordinate define the origin. Half the common
radial span is the radius. When the distinct span is zero, the cylinder is
unbounded, its axis is the positive omitted model coordinate, and it has no
stored axial length. A finite body occupying a repeated-diameter frame and
control shell remains a repeated-diameter body and is not a square-radial form.

An exactly eight-slot terminal frame is a single-diameter form only when the
square-radial invariants do not also hold. If both the single-diameter and
square-radial forms satisfy their invariants, neither form defines the carrier.

Cylinder prototype local systems are parameter templates; their terminal
triples do not establish model-space origins. A named cone prototype whose
`local_sys` field contains the complete support-apex body and whose
`half_angle` field contains exactly one positive angle is different. Its
support directions, apex coordinate, axis sign, reference direction, and
half-angle have the same model-space semantics as the positional cone suffix
below. The named prototype therefore defines the carrier of its uniquely
associated first surface row. Other cylinder and cone prototypes require a
positional construction or feature placement.

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

A later positional cone can store this support-apex operand after a separate
envelope operand. Each operand ends with `e3`. The support-apex operand stores
the complete nine-slot support frame, apex scalar, model-reference token,
three-byte station token, and positive half-angle in that order. It defines
the carrier independently of the preceding envelope. Exactly one complete
support-apex operand must occur among the `e3`-delimited segments. Zero or
multiple distinct complete operands do not define a carrier.

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

A positional non-plane surface row with an inline carrier stores an axial
envelope, an outline, a twelve-slot local system, a family suffix, and the
contour chain in that order. The envelope is `<u> <v0> 12 <v1>` followed by
two model-space XYZ corners and `e3`; the `12` is the separator between the
two axial parameters. The local system has three direction triples followed
by one origin triple and is closed by `e3`. A cylinder suffix is one radius, a
cone suffix is one half-angle, and a type-26 suffix is `radius1 radius2`.
The suffix lane uses exact `18` for zero, `0f` for one, and `0e` for one half.
For type 26, a finite non-negative `radius1` and positive `radius2` define a
torus; exact `radius1 = 0` selects a sphere whose radius is `radius2`.

A local-system-suffix row may omit the axial envelope. Its body begins with
the local system and family suffix, or with an earlier row-local block closed
by `e3` followed by that local system and suffix. The suffix local system ends
at the next `e3`; its origin is already in model space and no axial extent is
defined. For a cone, a complete legacy planar-envelope block immediately
before the suffix is a witness for it: the family, dominant axis, and suffix
half-angle must agree across both operands. The witness supplies the
model-space placement when the suffix does not preserve a unique placement
interpretation. An incomplete or conflicting prefix does not supply a
carrier. A complete suffix is admitted only when its frame is finite and
orthonormal and its family suffix consumes the entire bounded body before the
terminal `e3`.

The local-system direction triples may use the explicit scalar lanes or one of
the compact axis images below. The images name the axis coordinate and expand
the nine direction slots; they do not determine model-space signs:

| Image | Axis coordinate |
| --- | --- |
| `A 18 e5 B 18 e5 C` | Z |
| `18 A 18 B 18 e6 C` | Z |
| `A 18 e6 B 18 C 18` | Y |
| `18 e4 0f 18 0f 18 10 18 e4` | X |
| `18 10 18 e5 10 0f 18 e4` | X |
| `18 0f 18 e5 0f e4 18 e4` | X |

Here `A`, `B`, and `C` are the image's single-byte signed unit tokens. The
image dictionary expands the support directions according to its slot lanes;
the stored origin and the envelope witness below select the model-space signs.
An explicit frame consumes the first five direction coordinates, then
`18 e5 0f` fills the remaining direction matrix coordinates with
`[0, 0, 0, 1]`. The reflected form `18 e5 10` fills them with
`[0, 0, 0, -1]`. In both forms the final three slots are the model-space
origin. The standalone `18 e5` image expands to `[0, 1, 0]`. The remaining
explicit slots use the scalar lanes already defined for tabulated-cylinder
coordinates and positional surface rows.

The carrier placement is admitted only when the envelope and outline provide a
unique witness. The outline coordinate whose span equals `abs(v1 - v0)` is the
axis coordinate. Solve
`{o + v0 C, o + v1 C} = {lo, hi}` for
`o in {+abs(s), -abs(s)}` and `C in {+1, -1}`, where `s` is the stored origin
component on that coordinate. A unique solution supplies the model-space axis
origin and direction. For each perpendicular coordinate, select the unique
sign of its stored component for which the outline lies inside
`[center - R, center + R]`. Use the cylinder radius for `R`; for a cone use
`max(abs(v0), abs(v1)) * tan(half_angle)`; for a torus or sphere use
`radius1 + radius2`. A failed or non-unique witness retains the complete row
as native data and does not create an analytic carrier.

The local-system-suffix form has no envelope witness. Its stored origin,
normalized first direction, normalized frame axis, and family suffix define
the carrier directly. If more than one complete frame interpretation is
geometrically valid, the row remains native.

The resulting equations are: a cylinder has an axis through the resolved
origin, the witnessed direction, and the suffix radius; a cone has its apex at
the resolved origin, the witnessed direction, and radius
`abs(v) * tan(half_angle)` at axial parameter `v`; a torus has center at the
resolved origin, the witnessed axis, and the two suffix radii; and a sphere
has center at the resolved origin and radius `radius2`. The stored first
support direction, projected perpendicular to the witnessed axis, is the
parameter-space reference direction. The witness must agree with every
complete inline interpretation of the bounded body.

Family-26 and family-29 rows may omit the inline envelope and local-system
body. When the header is followed immediately by a contour chain, their
carriers come from the owning class-913 round replay and generated-entity
binding. A family-29 row that carries a body instead uses the named
`fillet_srf` prototype roster (`srf_prim_ptr`, `pnt_spline`, `id`, `type`,
`gen_info`, `flip`, `tan_cond`, and `i_pnts`); its spline fields are not an
inline analytic carrier.

The next valid named field or the enclosing `e3` compound close terminates a named prototype field, whichever occurs first. A named-field header has a field type no greater than `24` and a nonempty identifier made from ASCII letters, digits, underscores, or parentheses. An `e0` byte inside a scalar token does not terminate the field. Bytes after the structural close belong to subsequent instance or namespace records.
A parenthesized `srf_prim_ptr(<family>)` record also ends at the next legacy
`srf_prim_ptr\0` record. Fields owned by that sibling prototype do not belong
to the parenthesized record. A following top-level `entity_ptr(<family>)`
record also ends the prototype; its named fields belong to that peer entity.

`radius`, `radius1`, `radius2`, and `half_angle` are scalar-typed fields. A body that does not complete a scalar token remains opaque and is not reinterpreted as a compact integer.
A named surface-prototype schema field occurs no more than once. Duplicate field
names make the prototype ambiguous; the bounded records remain retained, but no
field value is defined for that identifier.

In a geometry section containing exactly one `torus` prototype with a positive
finite `radius2`, a type-`26` positional row can replay that minor radius as
the final scalar of its terminal scalar frame. The scalar must end at the
row-body boundary,
must equal the prototype `radius2` exactly, and the row must not contain the
tagged radius-override form. This replay defines only the instance minor
radius. It does not define the major radius, center, axis, or parameter-space
reference direction.

Positional cylinder rows store cap-plane point data rather than a `local_sys` replay. Their per-instance radius does not inherit the prototype default; derive it from bound `fc 05` cap-circle geometry or from a byte-backed analytic construction.

Every complete positional-cylinder body is checked against each defined row
grammar. The carrier is admitted only when all matching grammars produce the
same origin, axis, reference direction, radius, and stored axial length. A
body with conflicting complete interpretations remains native.

For a class-911 row whose complete counterbore dimension tuple is established,
the positional radius must equal the bore radius or the counterbore radius.
Another radius remains native. An absent or ambiguous counterbore tuple does
not supply this gate.

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

The `tab_cyl` prototype's `local_sys f9 04 03` supplies a chart-origin vector.
In the complete form, all twelve slots are finite and slots 9 through 11 are
the vector. In the compact chart form, slots 0 through 6 are finite zero,
slots 7 through 9 are finite vector components, and slots 10 and 11 are
omitted or inherited; slots 7 through 9 are the vector. A replay joins its
prototype only when both records are in the same section and the prototype's
`c_pnts f8 04 f7 <start> fb` expands to the replay's four control-point IDs in
the same order. A missing or non-unique join leaves the prototype chart origin
undefined.

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
separators, and the reference-bearing terminal trailer. The byte-exact replay
body begins at `curve_id_ci` and ends after the terminal `f6 e3`. A replay
belongs to the nearest preceding `geom_type = 2c` surface row after the
previous replay signature. Intervening rows from other surface families do not
consume it.
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
frame-axis spans equal the first-to-last control-point spans of the two
directrix coordinates, those axes define the directrix chart. Interior control
points do not widen these spans. Each directrix axis is a signed unit-slope
affine map. A layout whose second and fifth scalar prefixes are `46` uses the
magnitude of the joined prototype chart-origin component on the first
directrix axis and zero on the second axis. The `_ 42 _ _ 18 _` layout uses zero
intercepts on both axes. Every other complete frame selects exactly one of a
zero-intercept chart, which retains the stored sweep-axis sign, and a
prototype-origin chart, which reflects the sweep-axis sign; the latter exists
only when a joined prototype supplies a nonzero first-axis component. The
prototype-origin intercept magnitude is finite and can have any model-unit
value; frame sign and reversal select its signed affine map.
The selected map and frame-axis assignment must be unique; otherwise the frame
is opaque. The remaining axis defines the extrusion vector. The four placed
points form a non-rational clamped cubic B-spline with knot vector
`[0,0,0,0,1,1,1,1]`.
For a replay-bound row, this unique ordered frame-axis construction defines the
extrusion vector independently of whether the bytes before `00 0c 9a` form a
three-scalar sweep-direction frame.

When a tabulated-extrusion NURBS surface is adjacent to a plane, one complete
surface control edge defines their intersection curve when the surface is
nonperiodic transverse to that edge, every control point on the edge lies in
the plane, and every other control point lies strictly on one side of the
plane. Exactly one of the four control edges must satisfy the rule. A
constant-U edge retains the surface's V degree, knots, control points, weights,
and periodicity; a constant-V edge retains the corresponding U data.

An adjacent plane that contains the extrusion vector intersects a clamped cubic
tabulated-extrusion surface along one generator when the four generator vectors
are equal, each generator's two rational weights are equal, and the weighted
cubic directrix has exactly one plane root in its parameter domain. Evaluating
the directrix at that root supplies the two generator control points. The
generator retains the surface's V degree, knots, rationality, and periodicity.

Two adjacent tabulated-extrusion NURBS surfaces share an intersection generator
when exactly one pair of their control edges encodes the same nonperiodic
degree-one NURBS curve. The control points are equal in forward or reverse
order, the knot vectors are equal after affine domain normalization in the same
orientation, and rational weights are equal up to one positive common scale. A
plane through that generator strictly separates every other control point of
the two surfaces, and neither surface is periodic transverse to the edge. The
shared curve retains one boundary's degree, knots, control points, weights, and
periodicity.

The fifth-slot `18` is a one-byte zero bound and does not consume bytes from the
sixth slot. Its first and fourth slots accept the complete first-coordinate
scalar lane; its third and sixth slots accept the second-coordinate scalar
lane. In the `_ 2d _ _ 2d _` layout, slots one and four also use the
first-coordinate lane. Scalar prefixes select the encoding of each coordinate;
they do not otherwise constrain chart selection. A missing or non-unique form
leaves the frame opaque.
Each endpoint bound carries its own stored sign; resolving a chart may negate
the two bounds independently. The resulting unit-slope affine map remains
unique.

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

In slot 6, `41 b1 b2 b3 b4 b5 b6 b7` stores the negative fixed-width
coordinate whose IEEE-754 binary64 image is `bf b1 b2 b3 b4 b5 b6 b7`.
The `41` form in the other slots stores the positive image beginning with
`3f`.

Compact token `0e` encodes positive `0.5` in a named prototype local-system
coordinate slot. Its negative positional-row meaning does not apply.

In the named prototype local-system coordinate lane, `5d <tail6>` reconstructs
the negative IEEE-754 image `BF D2 <tail6>`.

In a named prototype local-system body, `18` immediately before a defined
coordinate-lane opener occupies one zero slot. The coordinate token begins the
next slot.

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

A type-26 body ending in `f7 1c` can store five terminal coordinates before
that close. The coordinates either occupy one contiguous five-scalar frame or
the final three scalars of one frame followed by a nonempty control payload and
a terminal two-scalar frame. Scalars preceding the final three-coordinate
suffix are body-local controls and do not occupy coordinate slots.

The untagged torus-envelope prefix begins after eight body-local bytes with
`18 94 3f 02 70 16 be fc 00 12 20`. Its direct form stores five contiguous
coordinates followed by `21`. Its split form stores two coordinates, `3a`, a
six-byte body-local control payload, and two more coordinates at the bounded
body end. The control payload does not occupy a coordinate slot.

A placement-complete direct torus replay continues after `21` with the control
bytes `b1 48 0a e3`, a twelve-slot local system, and two terminal radius
scalars. Local-system support slots 0 through 8 use the first-coordinate lane;
in slot 6, `28 b1 b2 b3 b4 b5 b6 b7` stores the negative IEEE-754 image `bf b1
b2 b3 b4 b5 b6 b7`. Origin slots 9 through 11 use the positional row lane.
Slots 0 through 2 and 6 through 8 are equal-scale orthogonal support
directions; their normalized cross product is the torus axis. The origin is
the torus center. The first terminal scalar is a positive major radius. The
second is a nonzero signed minor radius; its magnitude is the analytic minor
radius. The five-coordinate envelope independently satisfies the two-radius
equation below. The local system and both radius scalars consume the remainder
of the bounded body.

A compact type-24 cylinder envelope has a model-space Y axis. Its direct form
is `14 <y0> <scalar> <y1> <x-center> <y0> <z0> <x-edge> <y1> <z1>`.
Its split form is `12 <y0> 14 <y1> <x-edge> <y0> <z0> <x-center> <y1>
<z1>`. The repeated axial bounds agree, `abs(x-edge - x-center)` equals half
`abs(z1 - z0)`, and both spans are nonzero. The cylinder origin is
`(x-center, y0, midpoint(z0, z1))`; its axis points from `y0` to `y1`, its
reference direction points from `x-center` to `x-edge`, its radius is half the
Z span, and its finite length is the Y span.

An `ActDatums` type-24 cylinder row with boundary type `00` or `01` can instead
carry a terminal seven-slot envelope frame. Its first slot is a nonzero signed axial
span, or the unique signed span is in an earlier scalar frame for a split
form. The remaining six slots are two opposite model-space corners. Exactly
one corner-coordinate span equals the absolute axial span. Of the other two
spans, one is exactly twice the other; the larger span is the diameter and the
smaller span is the radius. Zero, nonfinite, ambiguous, or otherwise
inconsistent spans do not define a carrier. The origin uses the diameter
midpoint, the axial coordinate of the second corner, and the held coordinate
of the first corner. The axis points from the second corner to the first. The
reference direction points from the first diameter coordinate to the second,
reversed for a negative signed span or a reversed row orientation. This
carrier remains in the `ActDatums` surface namespace; its numeric identifier
does not join a `VisibGeom` surface with the same number.

An XZ-axis type-24 cylinder body has the form `20 10 00 <z0-local> <aux>
<z1-local> <x0> <y0> <z0> <x1> <y1> <z1>`. The first-corner `z0` slot can use
the exact three-byte zero form `34 f0 00`; all other coordinates use the
positional row lane. The local and model Z deltas agree. The cylinder origin
is `(x0, midpoint(y0,y1), z0)`, its axis points along `(x1-x0, 0, z1-z0)`, its
reference direction points from `y0` to `y1`, its radius is half the nonzero Y
span, and its finite length is the XZ span. The auxiliary magnitude is less
than that length, and the body contains no trailing bytes.

A symmetric-revolution type-24 cylinder body begins with `15 <y0> 18 <y1>`
or `17 <y0> 15 <y1>`. Four geometric scalars follow: `<r0> <y1-opposite> <r1>
<y0-opposite>`. The `15` form has a zero byte before `r1`, repeats `r1`, and
then ends with `f7 19`. The `17` form repeats `r0` before `r1`, stores one
model-reference scalar after `y0-opposite`, and then ends with `f7 19`. The
repeated radial value agrees with its first occurrence. The two axial pairs
have one midpoint, the second pair extends beyond the first pair, and the
radial midpoint is zero. The cylinder origin is `(0, axial-midpoint, 0)`, its
axis points from `y0-opposite` to `y0`, its reference direction points from
`r1` to `r0`, its radius is half the radial span, and its finite length is the
first axial span. The model-reference scalar does not contribute geometry.

An axial-endpoint radial-sample type-24 cylinder body has two seven-byte
leading scalars separated by `18`, followed by `0e`, then `<x-radial> <y0>
<aux-radial> <radius> <y1> <z-radial> f7 19`. The leading scalars and auxiliary
radial coordinate are finite. The radius and Y span are nonzero, the auxiliary
radial magnitude does not exceed the radius, and `(x-radial,z-radial)` lies on
the stored circle. The cylinder origin is `(0,y0,0)`, its axis points from `y0`
to `y1`, its reference direction points opposite the X-radial sign, its radius
is the stored radius, and its finite length is the Y span.

A held-coordinate type-24 round envelope has three contiguous scalar frames
with slot counts two, two, and five. The first frame starts at the body with a
zero slot and ends before control bytes `78 ac`; the second starts immediately
after those bytes and ends before `24 00`; the five-coordinate frame occupies
the remainder of the bounded body. The replay form has frame slot counts two,
one, and six. Its second frame also begins after `78 ac`, two control bytes
separate it from the six-slot frame, the first slot of that frame is auxiliary,
and `f7 18` may follow the frame. The controls and auxiliary slot do not
contribute cylinder geometry. In both forms the five geometric coordinates are
`x0, y0, z, x1, y1`. The omitted second Z coordinate equals `z`. The cylinder
origin is `(x0, midpoint(y0, y1), z)`, its axis points from `x0` to `x1`, its
reference direction points from `y0` to `y1`, its radius is half the Y span,
and its finite length is the X span. Both spans are nonzero.

A bounded type-24 round envelope stores two diameter endpoints and two
three-coordinate extent endpoints. The diameter endpoints occur around a held
coordinate after `15` or `00 15 1c`, or across the single-byte `12` separator
between a two-scalar leading frame and a seven-scalar trailing frame. A split
zero-coordinate form has frame slot counts two, three, and three. Its leading
frame ends before `12`; the middle frame stores the second diameter endpoint
and the first two coordinates; the exact token `34 f0 00` supplies the third
coordinate as zero; and the terminal frame stores the second endpoint. At
least one corresponding extent-coordinate delta repeats the positive diameter.
Half that repeated diameter is the rolling radius; it is independent of the
generated cylinder carrier radius.

When all three extent-coordinate deltas equal the diameter, the envelope does
not select a cylinder axis. Two circular `MdlRefInfo` entities owned by the
same feature select an axis when their normals are parallel to that candidate
axis, they occupy its opposite extent-coordinate values, and each joins the
same pair of opposite radial-envelope corners projected onto its plane. The
circular pair may use either radial diagonal. The cylinder origin is the
radial midpoint on the first cap, the axis points toward the second cap, the
radius is half the diameter, and the cap separation is the finite length.
Exactly one candidate axis must satisfy both cap records.

The first-coordinate bounded round form is 50 bytes. It begins with `4c b7`,
stores the first diameter endpoint at offset 7, `12` at offset 15, the second
diameter endpoint at offset 16, and five contiguous first-coordinate-lane
extent scalars at offset 24. Terminal `18` at offset 49 is the zero-valued
sixth extent coordinate. The two diameter endpoints and five extent scalars
use the tabulated-cylinder first-coordinate lane, including its positive
eight-byte `2d` form. The common bounded-round diameter and unique radial-span
invariants apply to the resulting two three-coordinate extent endpoints.

The segmented first-coordinate bounded round form is 56 bytes. Byte zero is
`18`; the first diameter endpoint occupies bytes 1 through 8; bytes 9 through
15 are `70 bf e3 4f 05 11 10`; the second diameter endpoint occupies bytes 16
through 23; and six contiguous extent coordinates occupy bytes 24 through 53.
The body ends with `f7 19`. Both diameter endpoints and all six extent
coordinates use the tabulated-cylinder first-coordinate lane. The common
bounded-round diameter and unique radial-span invariants apply.

A type-24 surface row generated by a round feature may terminate with its
positive rolling radius in a seven-byte positive-DICT scalar. The scalar ends
the row body directly or is followed only by `f7 17`. Every type-24 row
generated by the feature must carry the same terminal radius before it defines
the feature's constant radius. A terminal eight-byte coordinate-lane scalar is
not a radius.

When a feature generates exactly one type-24 row and its entity tables select
exactly two reference circles with explicitly stored centers, equal radii,
parallel axes, and distinct coaxial centers, the circles place that cylinder.
The center displacement defines the cylinder axis line and length. The first
circle's stored axis, center, radius, and start radial define the oriented
cylinder axis, origin, radius, and parameter reference direction.

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
spans covering every byte outside those slots. Defined type-26 contiguous and
control-split coordinate envelopes retain their ordered coordinates and
body-relative first coordinate offset in that arena. Scalar frames are the maximal
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
searched for nested scalar openers. The complete declared scalar span owns
header-shaped and compound-close bytes inside its scalar tokens; those bytes do
not terminate the field. `i_points` uses eight-byte `28` and `41`
positive sub-unit forms in addition to eight-byte `2d`/`46` world coordinates,
the positive DICT lattice, and the `b3`/`b9` negative forms. `end_v_tangts`
uses the signed coordinate DICT lattice defined for the second directrix
coordinate lane. `u_params` and the seven-byte `v_params` forms use the
positive DICT lattice. `v_params` also uses the eight-byte `28` positive
sub-unit form.

In a `fillet_srf` prototype, `i_pnts` and `i_points` use the positive DICT
lattice and the `a4..df` negative members of the signed second
directrix-coordinate lane. `tangts` uses the complete signed second
directrix-coordinate lane.

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

Plane row bodies contain envelope/domain data, `local_sys f9 04 03`, and a row/topology tail.
The next `srf_array` row of any surface family bounds the plane row. Compound
closes after that row do not terminate the plane envelope or local-system body.

A standard positional envelope is exactly ten contiguous scalar slots: four
two-dimensional domain bounds followed by two model-space corner triples. A
leading-compact envelope is `0e` followed by exactly nine contiguous scalar
slots: three prefix values followed by the two corner triples. Each layout
consumes its complete compound-bounded body. A compact envelope can instead be
the unique terminal nine-slot scalar frame after a nonempty structural prefix.
Bytes outside these layouts do not form a plane envelope.

`local_sys` has twelve scalar slots. Plane rows use three storage forms.
Compact and specialized plane-support bodies expand their direction values into
three support triples. A generic direct-normal body stores a parameter
direction, an exact zero-rank triple, and a stored plane normal. A generic
coordinate-first body starts with a coordinate scalar, not one of the control
openers `0e`, `0f`, `10`, or `18`, and contains an `18` zero-slot prefix at a
scalar-token boundary. Its slots 0 through 8 store the first three rows of a
3x3 matrix in row-major order. In that form, columns zero, one, and two are
the parameter direction, zero rank, and stored plane normal. All three forms
use the same origin slots.

```text
support-triple form:
slots 0..2    first support direction
slots 3..5    second support direction or [0, 0, 0]
slots 6..8    third support direction

direct-normal form:
slots 0..2    parameter direction
slots 3..5    [0, 0, 0]
slots 6..8    stored plane normal

coordinate-first matrix form:
slots 0, 3, 6    parameter direction column
slots 1, 4, 7    zero-rank column
slots 2, 5, 8    plane-normal column

both forms:
slots 9..11   support-frame origin
```

In a coordinate-first matrix body, the zero-rank column is exactly zero. The
parameter and normal columns are finite, nonzero, equal-scale, and
orthogonal. Normalize those two stored columns to obtain the plane chart. A
body that does not meet these conditions does not establish a matrix chart.

In a direct-normal body, the zero-rank triple is exactly zero. The parameter
direction and stored normal are finite, nonzero, equal-scale, and orthogonal.
Normalize both triples to obtain the plane chart. Use the stored normal as the
plane normal; do not replace it with a cross product.
When the stored frame and its z-mirrored parameter-direction and normal branch
define distinct planes, complete direct, prototype, and two-chart pcurve
endpoint pairs are equivalent branch witnesses. Admit the unique branch whose
chart endpoints lie on the adjacent face carrier. No compatible or multiple
compatible branches leave the stored frame unresolved.

In a support-triple body, the slots are:

```text
slots 0..2    support direction or [0, 0, 0]
slots 3..5    support direction or [0, 0, 0]
slots 6..8    support direction or [0, 0, 0]
slots 9..11   support-frame origin
```

Within slots 0 through 8, slots whose ordinal is divisible by three use the
signed first-coordinate lane and the other slots use the signed
second-coordinate lane. These component lanes take precedence over the generic
positional-row scalar lane. In support-triple storage, the first lane is the
first component of each support triple. In coordinate-first matrix storage, it
is the first matrix column. `18` immediately before a complete coordinate
token occupies one zero slot; the coordinate begins the next slot.

The twelve-slot macro language must consume the complete local-system body. A
terminal `e1` after a complete frame is a null row-tail marker and is not a
scalar slot. If any other bytes remain, none of the twelve slot positions is
assigned a numeric value.

In the coordinate-first matrix form, an `18` at a scalar-token boundary before
the next decodable coordinate occupies one zero slot. A terminal `18` occupies
one zero slot. The `18 e5` direction macro and the other compact support
macros are support-triple forms, not matrix rows.

The seven-byte compact image `A 18 e5 B 18 e5 C`, where each of `A`, `B`,
and `C` is `0f` or `10`, names a model-Z plane-normal frame. In a plane row,
it expands to a direct-normal frame with the model-X parameter direction, a
zero middle triple, and the model-Z normal. `A = 0f` selects positive model X
and `A = 10` selects negative model X. The `B` and `C` tokens retain the
orthogonal compact-image signs; they do not select a different normal
coordinate.

The rank-two body `18 e4 0f e4 18 e5 0f 18 e6` expands to support triples
`[0, 1, 0]`, `[0, 0, 0]`, and `[1, 0, 0]`, followed by origin `[0, 0, 0]`.
This image has the same expansion in every twelve-slot local-system field.

When the support-frame guard holds, derive the support-triple normal as:

```text
first, second = the unique equal-scale orthogonal pair in stored order
normal = normalize(cross(first, second))
```

The remaining support triple can be zero or nonzero. A second equal-scale
orthogonal pair makes the frame ambiguous. A residual magnitude between
`1e-9` and `1e-6` is not a zero triple and leaves the frame unresolved.
`outline f9 02 03` stores two XYZ corners. In these positional scalar lanes,
`73` and `bb` each begin a seven-byte scalar token. Repeated identical tokens
denote equal stored values; tokens with different prefixes denote distinct
values. Token equality remains defined when the scalar magnitude is not
decoded.

When the outline independently holds exactly one model coordinate, a complete
support frame may instead store one nonzero triple parallel to that held axis,
one nonzero triple perpendicular to it, and one zero triple. The parallel
triple confirms the plane normal role. The perpendicular triple is the
parameter-space reference direction. The frame origin must lie on the held
plane. A support triple that is neither parallel nor perpendicular leaves the
chart unresolved.

In the frame-bound held-coordinate outline form, the support frame establishes
the normal and parameter direction. The matching held outline coordinate,
including its shortened terminal form, establishes the plane offset. The plane
chart projects local-system slots 9 through 11 onto that plane, replacing only
their normal component with the held coordinate. Equality on another outline
axis collapses the nominal outline to a line; it does not define a competing
plane equation because the support-frame normal has already selected the plane
axis.

When exactly one coordinate is held constant across both corners, its axis is the positive basis normal and its value is the model-space plane offset. The other two coordinate pairs need only be known to be distinct; their magnitudes are not required. In the absence of a complete local-system chart, the first positive basis direction perpendicular to the normal is the neutral parameter reference direction. A complete local-system chart takes precedence. Zero or multiple held coordinates do not establish a plane equation from the outline.
The held coordinate establishes only the plane equation. It does not establish
the parameter-chart origin or either parameter direction.

A compound-close positional plane body can carry the two model-space outline
corners as one contiguous six-scalar frame immediately after `00 0c 9a`, even
when structural bytes separate earlier scalar frames. Slots zero through two are the first XYZ corner and
slots three through five are the second. Exactly one equal coordinate defines
the held axis and offset under the same plane rule. Zero or multiple equal
coordinates leave the plane unresolved.

An auxiliary-corner positional plane body has a three-byte prefix, one
seven-byte scalar, an eight-byte control payload, and a terminal frame of seven
contiguous scalars. The first terminal scalar is auxiliary. The remaining six
are two XYZ corners and use the same unique-held-coordinate plane rule.
An `f7 0c`-terminated auxiliary-corner form stores a final contiguous frame of
seven through ten scalars immediately before that terminator. The terminal
seven slots consist of one auxiliary scalar followed by two XYZ corners.
Earlier slots and control fields do not participate in the corner coordinates.

A first-coordinate-lane positional plane body stores two XYZ corners as six
contiguous scalars immediately after `00 0c 9a`; `a0` can immediately precede
the marker. The frame reaches the bounded body end or is followed only by
`f7 0c`. The first coordinate of each corner uses a negative token from the
tabulated-cylinder first-coordinate lane; the two slots can independently use
that lane's seven- and eight-byte forms. Negating each stored value gives its
model-space X coordinate. The other four slots use the positional surface-row
scalar lane and give the two YZ coordinate pairs. The resulting corners use
the unique-held-coordinate plane rule.

For a generated section plane selected through the parent-datum rule, multiple
held envelope coordinates are filtered against the orientation plane. The
unique perpendicular held axis defines the section plane.

For an axis-aligned plane, the held-coordinate outline defines the placed plane
equation. An axis-aligned `local_sys` support frame without that outline does not
establish the model-space offset outside its generating feature.
When an axis-aligned `local_sys` normal selects an outline coordinate whose two
stored tokens are equal, that coordinate supplies the plane offset. The other
outline coordinate pairs may be equal or unresolved because the support frame
already fixes the plane orientation.
A shortened standard outline can store the four bound scalars and first XYZ
corner followed by one terminal scalar token. The terminal token occupies the
coordinate selected by the axis-aligned support-frame normal. It establishes
the held coordinate when its exact token image equals that coordinate's token
in the first corner; the other two coordinates of the second corner are absent.

A uniquely identified plane surface row is placed by its native face topology
when a uniquely identified boundary circle or ellipse has a model-space
carrier, when two distinct model-space boundary lines determine a plane, or
when at least three distinct solved boundary vertices are non-collinear. The
conic's center and axis, the line directions and origins, or the vertices'
cross product define the plane. Every independently defined boundary plane,
every boundary line, and every solved boundary vertex of that face must agree.
Duplicate surface or curve identities, a line set with no distinct pair,
collinear vertices, or conflicting boundary evidence leave the carrier
unresolved.

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

### 3.5 Loop namespace: `lo_array`

`lo_array` is a native loop-roster namespace. Its records are retained as
native data; their semantic joins to faces, contours, and curve topology are
not defined by this namespace.

| Item                  | Rule                                                            |
| --------------------- | --------------------------------------------------------------- |
| ND frame header       | `lo_array\0 f3 f8 <count> f7 <class> fb e3`                    |
| DEPDB frame header    | `lo_array\0 f2 f8 <count> f7 <class> fb e3`                    |
| Bare frame header     | `lo_array\0 f8 <count> f7 <class> fb e3`                       |
| Named prototype       | Required fields `lo_id`, `lo_type`, `lo_subtype`, `feat_id`, `attributes`, `direction`, `next_lo_ptr`, and `object_data`; close `f1 f7 <class> e3` |
| Positional row prefix | `<lo_id_ci> <lo_type_ci> <lo_subtype_ci> <feat_id_ci> <attributes> <direction_ci> <next_lo_ptr_ci>` |
| Positional row body   | The bounded bytes after the prefix through the row-close `e3`. |

`<count>` is the frame slot extent. The frame ends at the next `crv_array`,
`lo_array`, `qlt_array`, or `srf_array` label. A complete positional row must
have all seven prefix fields and a row-close `e3`. Rows are retained only
while their count does not exceed the frame extent. An unresolvable row
boundary or an overfull frame withholds the affected positional rows. The row
body is retained as exact native bytes and has no neutral loop meaning.

## 4. Curve namespace: `crv_array`

`crv_array` provides edge identifiers, half-edge topology, type bytes, and pcurve records.

| Item                   | Rule                                                           |
| ---------------------- | -------------------------------------------------------------- |
| ND count               | `crv_array\0 [f3] f8 <count>`                                  |
| DEPDB count            | `crv_array\0 f2 f8 <count>`                                    |
| Positional row header  | `<crv_id_ci> <type_byte> <feat_id_ci> <dir0_flag> <dir1_flag>` |
| Standard suffix        | `[F0, F1, E0, E1, R0, R1] e3`                                  |
| DEPDB one-sided suffix | `[0, X1, F1, 0]`; `127` terminates `X1`                        |
| Row terminators        | `e1 e3` or `e1 f5 05 f6 e3`                                    |

The curve namespace ends at the next `crv_array`, `lo_array`, `qlt_array`, or
`srf_array` label. The final positional row may use that next-array boundary
when its prefix and complete topology suffix are present but no row terminator
precedes the boundary. A segment without a complete prefix and suffix is not a
curve row.

The positional row's `feat_id` is the identifier of the modeling feature that
generated the curve. Surface-row and curve-row generator identifiers belong to
the same feature namespace. A nonzero generator identifier establishes that
feature identity even when no operation-state or feature-definition row is
stored for it.

The `crv_id` is a source reference and is not an occurrence identity. Within
one curve namespace, a native parameter, topology, or cross-section curve-row
record uses its family key with `<crv_id>` when that identifier occurs once.
When it occurs more than once, the native key is `<crv_id>-<source_offset>`
with the source offset rendered as a 20-digit zero-padded decimal value. The
repeated-row key does not alter any geometry or topology join: a semantic join
still requires one uniquely identified row.

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

The two half-edge sides of one curve have opposite endpoint order. Their start
vertices therefore define the curve's oriented endpoint pair when either
side's closed loop supplies an end vertex. If both sides supply end vertices,
each must equal the opposite side's start vertex. A missing successor on one
face does not erase the endpoint relation proved by the other closed face.

Every edge represented in a topological vertex orbit contributes both of its
non-null face carriers to that vertex. The orbit stores outgoing half-edges;
carrier incidence is not limited to the stored side of each edge.

The raw `type_byte` does not by itself identify a curve family.

The parameter body is the byte range after the two direction flags and before
the six-reference suffix. `F0`, `F1`, `E0`, and `E1` use the canonical
reference-identifier lane. `R0` and `R1` replay the named prototype fields
`ref_geom[0]` and `ref_geom[1]` through the generic compact-integer lane. The
common `R0 = R1 = 0` form is the exact tail `00 00 e3`; nonzero reference
geometry values replace the corresponding zero compact integer. These fields
are references, not curve parameters.

After `e3`, a row can carry array-item linkage before its row terminator. The
linkage consists, in order, of an optional `f7 <compact-link>`, an optional
`f8 <count> <count compact-links>`, and zero through four terminal compact
links. A final row can then carry `e1 e0 00` or
`e1 f5 05 f6 e0 00` before the next namespace boundary. Array-item linkage
does not extend the parameter body.

A suffix candidate is a sequence of four canonical topology reference
identifiers followed by two generic compact reference-geometry values that
reaches `e3`. A unique candidate is valid
without namespace qualification. When multiple candidates exist, retain only
candidates whose `F0` and `F1` are zero or identify rows in the enclosing
`srf_array`. A face identifier absent from that array may qualify when it occurs
as an `F0` or `F1` role in a uniquely delimited topology row in the same
namespace. The enclosing `srf_array` qualification has precedence: when it
selects exactly one candidate, namespace evidence cannot replace or invalidate
that candidate. Namespace evidence is a fallback only when no candidate passes
the enclosing `srf_array` qualification. Exactly one candidate after the
applicable qualification is valid. Zero or multiple candidates withhold the
complete row from typed parameter records. The same rule frames the topology
row. Its scalar walk retains each decoded token with body-relative offset,
length, and exact bytes.
Canonical `f7` entity references retain the same span data. Maximal bytes
claimed by neither class form opaque spans, so the three span sets partition
the complete body.

### 4.1 Pcurve endpoints

A direct curve body consisting of exactly eight scalar slots and no references
has this layout. A scalar token occupies one slot. A standalone `12` occupies
one zero-valued slot. No other unclaimed byte is permitted. All eight values
are finite parameter coordinates in the corresponding face spaces. The
parameter row and its uniquely identified topology row have the same raw
`type_byte`; a same-identifier row of another type does not bind the body.

| Slots  | Meaning                            |
| ------ | ---------------------------------- |
| `0..1` | Endpoint A in face `F0` parameters |
| `2..3` | Endpoint A in face `F1` parameters |
| `4..5` | Endpoint B in face `F0` parameters |
| `6..7` | Endpoint B in face `F1` parameters |

A bare terminal `18` supplies the final zero slot when seven preceding scalar
slots are present. A direct `crv_pnt_arr f9 02 04` body stores the same layout
and occurs once in its labeled prototype. Each of
`crv_hdr_geom_ptr[0]`, `crv_hdr_geom_ptr[1]`, `next_crv_hdr_ptr[0]`, and
`next_crv_hdr_ptr[1]` occurs once in the same prototype; repeated endpoint or
topology fields make the prototype ambiguous.

The named prototype's `crv_pnt_dir` array stores the two half-edge direction
flags. A unique named prototype topology record supplies a rowless edge when a
positional or prototype topology suffix references its `crv_id` as `E0` or
`E1`. The decoder promotes that record to the half-edge graph only when the
prototype, its topology record, its direction array, and both face references
are unique in the enclosing model namespace. A named prototype that is not
referenced as a successor remains schema data.

### 4.1.1 Legacy ASCII curve topology and endpoints

In legacy ASCII persistence, the model curve namespace is the unique complete
`crv_array` object array directly below the unique
`Sld_VisGeom.active_geom` object. Each direct element is one `crv_array` curve
object. A curve object is a complete topology row only when it has one scalar
`crv_id`, `type`, and `feat_id`, one dimension-`[2]` type-1 `crv_pnt_dir`
array, and one scalar field for each of
`crv_hdr_geom_ptr[0]`, `crv_hdr_geom_ptr[1]`, `next_crv_hdr_ptr[0]`, and
`next_crv_hdr_ptr[1]`. The two `crv_hdr_geom_ptr` values are the `F0` and `F1`
surface identifiers. The two `next_crv_hdr_ptr` values are the `E0` and `E1`
successor curve identifiers. A missing, duplicate, dimension-incomplete, or
non-signed direction field withholds the typed row.

The legacy `crv_pnt_dir` values `1` and `-1` encode the `01` and `f6`
endpoint directions respectively. A complete `crv_pnt_arr` type-2 array has
dimensions `[N, 4]` with `N >= 2` and exactly `4N` finite values in row-major
order. Each sample row stores the two adjacent face-chart coordinates. The
first row is endpoint A and the last row is endpoint B:

| Slots | Meaning |
| ----- | ------- |
| `0..1` | Endpoint in face `F0` parameter space |
| `2..3` | Endpoint in face `F1` parameter space |

Intermediate rows are bounded pcurve samples. Their endpoint pairs bind to the
same `F0`/`F1` faces as the topology row. A complete legacy curve namespace
uses the same half-edge successor, vertex-orbit, face-loop, and carrier
admission rules as the binary `crv_array` namespace.

A binary canonical two-chart body opens `fc <count>`, where `<count>` is a
compact integer of at least two. Exactly `count` sample rows follow. Each row
contains four finite scalars in this order:

| Slots  | Meaning                                  |
| ------ | ---------------------------------------- |
| `0..1` | Sample coordinates in face `F0`'s chart |
| `2..3` | Sample coordinates in face `F1`'s chart |

The first and last rows are the edge endpoints. Intermediate rows are ordered,
pointwise-corresponding pcurve samples. A later row in the same `crv_array`
namespace with the same nonzero `feat_id` and raw `type_byte` replays the unique
canonical sample count without the `fc <count>` prefix. The canonical or
replay body is complete only when exactly `4 * count` scalar slots consume its
bounded parameter body. It has no trailing parameter-bound scalars. Multiple
canonical counts for one feature and raw type
make an unprefixed replay ambiguous.

Each chart's first coordinate uses the first tabulated-cylinder directrix
coordinate lane. Its second coordinate uses the second directrix coordinate
lane. A bare `18` occupies one zero slot in either coordinate. The chart pair
for a spline face is the interpolation surface's `(u, v)` pair. A plane uses
its stored affine `(u, v)` frame. An extrusion uses profile parameter followed
by sweep coordinate. Other surface families use their stored surface chart.

Every available face chart is evaluated at every sample. When both face
surfaces are available, corresponding model-space points must agree. A missing
face surface does not erase a complete path in the available face chart; that
path supplies one-sided, non-authoritative endpoint evidence. Two agreeing
complete paths supply authoritative endpoint evidence. A nonperiodic spline
sample marginally outside its stored parameter interval is evaluated with the
polynomial of the adjacent boundary knot span. It is not clamped or rejected.

### 4.1.2 Pcurve operand join

An endpoint path in one face parameter space maps to model space through that
face's stored surface chart. The path is a boundary witness only when its
mapped points satisfy both placed incident surface carriers. For two plane
carriers, their normals must be nonparallel so that the operand join has one
intersection line. A complete linear path uses its two endpoints and the
interpolated points between them for this carrier test.

One face path can establish a one-sided endpoint witness when the other face
path is incomplete or fails the carrier join. A failed path does not veto a
different path that has a complete carrier proof. Every carrier-proven path
for one curve must agree on the ordered model-space endpoint pair. A path that
conflicts with an already solved endpoint vertex is not evidence. When the
incident carriers do not provide a unique supported join, retain the complete
face paths and require their mapped endpoint pairs to agree.

### 4.2 `fc` curve bodies

An `fc` prefix is resolved by exact body grammar. A complete two-chart body
uses the byte after `fc` as its sample count. Other complete forms use it as a
body-family subtype. A partial token match does not select either meaning.

| Subtype | Body family                              |
| ------- | ---------------------------------------- |
| `fc 02` | Short pcurve-style endpoint record       |
| `fc 05` | Cap-circle arc record family             |
| `fc 08` | World-coordinate control-polyline family |
| `fc 13` | Held-cap-ordinate control polyline       |

The complete short `fc 02` endpoint body has the form
`fc 02 | u0 | v0 | 0 | 1 | u1 | v1 | 2 | T`, where each `u`/`v` slot is a
finite scalar. The literal marker byte images are `18`, `e4`, and `29 ff ff`
for `0`, `1`, and `2`, respectively; the last image decodes as the largest
binary64 value below `2`. `T` is a three-byte terminal operand whose first
byte is `34`. The `u`/`v` pairs are endpoint A and B in the first topology
face's stored parameter chart. The terminal operand remains opaque. The
second topology face supplies the carrier join; it does not supply a second
parameter path. A decoder may transfer this path only after mapping both
endpoints through the first face chart and applying the ordinary endpoint and
carrier admission rules.

`fc 05` records store cap-circle control points in the order `A`, `B`, `t`, `C`, where `A` and `C` use eight-byte world-coordinate tokens and `B` and `t` use DICT or standalone-zero scalar tokens. `C` is the owning cylinder's axis-placement ordinate. The adjacent plane supplies the cap circle's axial coordinate. `t` is the angular curve parameter in radians. The signed relation between successive polar angles and `t` determines curve sense; subtracting the signed stored parameter from a point's polar angle determines the parameter-zero radial direction. For a model-X axis, `(A, B, C)` maps to `(Z, Y, X)`; for a model-Y axis it maps to `(X, Z, Y)`; for a model-Z axis it maps to `(Y, X, Z)`. The row-frame radial vector `(A, B)` maps to `(0, B, A)`, `(A, 0, B)`, or `(B, A, 0)`, respectively. `fc 13` stores a control polyline rather than an analytic circle.

In an `fc 14` body for a circle shared by an axis-aligned coaxial circular cone
and cylinder, every `2d` world-coordinate token is the same exact token image.
Its value is the circle center's coordinate on the common axis. The value
selects one of the two algebraic cone-cylinder intersection circles only when
exactly one candidate has that axial coordinate.

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

One `fc 05` curve bound to one cylinder face and one resolved axis-normal cap
plane independently defines both its model-space circle and the cylinder
carrier. The cap plane supplies the model-space axial coordinate. The fitted
center and radius define the axis line and cylinder radius. When every stored
parameter agrees with one signed polar-angle progression, that sign defines
the cylinder-axis sense and the extrapolated parameter-zero radial direction
defines the circle and cylinder reference direction. Otherwise, the cap-plane
normal supplies the neutral axis sense and the radial direction from the fitted
center to the first stored sample supplies a neutral reference direction. The
cylinder axis passes through the cap-circle center. The neutral chart changes
neither carrier equation and does not assign native parameter semantics.

## 5. Topology and section records

Build the B-rep half-edge graph from the `crv_array` suffixes. A single-loop face has an outer boundary by topology. A two-edge loop is admissible only when its distinct edge carriers have typed non-linear geometry, complete native pcurve endpoint paths close in traversal order, and the resulting boundary is otherwise ordered by the same face rules. For multiple two-edge circular loops on a plane, complete circle carriers with a common center and distinct radii provide the outer-to-inner order. Two-edge line-only loops and ambiguous circular loop sets remain native. Multi-loop faces otherwise require parameter-space containment to distinguish outer from inner loops. When no placed surface chart is available, complete solved boundary vertices must prove one common plane before the same containment rule is applied. Native body components follow connected components of nonzero face references. In a legacy ASCII layout, a component is eligible for neutral admission when it contains at least one face whose visible surface carrier, boundary loops, edge carriers, and solved vertices all pass the admission gates. Only those eligible faces and edges enter the neutral body; references in the same component that do not pass the gates remain native. A component with no eligible visible face remains native. Neutral shells follow connected components of admitted face topology through shared edges or vertices.

Use the following order to select a body count:

1. A positive `Geomlists.n_bodies` value.
2. `Geomlists.first_quilt_ptr == 0` as a single-body discriminator.
3. Face-reference adjacency component count when it is the only byte-backed source.

Emit neutral body and region ownership only when the selected
count equals the number of native components and every component has an
admitted face or a solved edge. In legacy ASCII, a component with no eligible
face is not admitted even when it has solved edges. When a legacy ASCII part has no body count
or single-quilt discriminator, a multi-component admitted set has unresolved
body ownership and remains native. Group admitted faces into shells by shared
edges or vertices. A solved edge not used by an admitted face loop is a shell
wire edge; a component containing wire edges is a `General` body. An admitted
face group with no shared topology uses its own shell in the same region. A
component with no admitted face or solved edge, or a body-count mismatch,
leaves native topology records available without neutral body assignment.

ND layouts share `var_arr`, `segtab`, `order_table`, `ent_tab`, and `vert_tab`, joined by `ext_id`.

`feat_outl_info.outline f9 02 03` stores six sequential feature-local scalar
slots. `post_roll_back` and `post_regen` records store the same six-slot body
after `e3 f7 <class> f5 96 92 <selector>`. Each slot is independently bounded
by its scalar encoding; an undefined prefix does not remove that slot. A named
record beginning before slot six terminates the body; the remaining slots are
absent and own no bytes.

| Table         | Semantics                                                                                                                                                                                                                                                                                                                                                                                 |
| ------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `var_arr`     | Solver-variable table keyed by `key`; `type=1` is point `u`, `type=2` is point `v`, and `type=3` is radius; `value` is solved, `guess` is the pre-solve estimate, and `known`, `homogeneity`, and `uvar_id` retain solver state.                                                                                                                                                          |
| `segtab`      | Two-dimensional segments; `type=2` is LINE, `type=3` is ARC, and `type=10` is CIRCLE. A line uses `f6` as its null `cntrid`; an arc and circle use a center `pointid`.                                                                                                                                                                                                                    |
| `order_table` | Generated-entity ordering table.                                                                                                                                                                                                                                                                                                                                                          |
| `ent_tab`     | Trimmed profile entity chain.                                                                                                                                                                                                                                                                                                                                                             |
| `vert_tab`    | Trim vertices and their two incident `segtab` entities.                                                                                                                                                                                                                                                                                                                                   |
| `eqtn_arr`    | Solver-equation table. The header is an optional `f2`, `f8 <declared_count>`, optional `f7 <entity_ref>`, `fb e2`; a named prototype follows and closes with `f1 f7 <class> e2`. Positional rows store solver `equation_id` and `function_id`, followed by either an explicit `f8 <argument_count>` or an uncounted argument body, then `f6` auxiliary data and an `e2` row terminator. The final row may terminate directly at the following `f2 f7` table separator; the separator is not part of the row.                                                                                                                                 |
| `relat_ptr`   | Counted sketch-constraint relations. An `f8` allocation count of one is the empty table form. Larger counts include two structural entries; exactly `count - 2` positional rows follow the schema close. Zero is invalid. Each row ends at `e2` and stores `id`, `used`, three four-slot operand vectors `a`, `b`, `c`, then `sign`, dimension selector, and relation-type discriminator. |
| `skamp_ptr`   | Counted solver-incidence rows. Each row stores `id`, `type`, `flags`, `status`, and a counted ordered array of section-entity `ent_id`/`sense` pairs. The named prototype may close its one-item schema directly with the outer `f3 f7 <table-class> e2` trailer; instantiated rows retain the same item class and row delimiters.                                                                                                                                 |
| `triples_ptr` | Counted joins from relation and equation identifiers to `skamp_ptr` incidence identifiers. Each of the three fields independently admits the `f6` null sentinel.                                                                                                                                                                                                                          |

Within an `eqtn_arr` argument body, `e4` occupies one slot with value one,
`e5` occupies two slots with value zero, and `e6` occupies three slots with
value zero. In the explicit-count form, `f6` occupies one null argument slot
when it occurs before the counted slot total; the following `f6` is the
auxiliary field. In the uncounted form, the first `f6` terminates the argument
body and is the auxiliary field. The equation table ends before the following
`scale`, `scales`, or `guesses` named record. The declared count and decoded row
count are independent native values.

Within each `var_arr` row, `value` and `guess` are independent scalar lanes.
The nine-byte `ed <tail8>` form is a dimension-driven sentinel in either lane;
it does not encode an inline scalar. Each lane retains its own sentinel state
and exact nine-byte body.

A type-10 circle row stores direction slots `[0, 0, 0]`, point slots
`[f6, 1]`, a non-null center identifier, zero arc-orientation and
vertical/horizontal slots, a non-null radius reference, and an `f6` secondary
radius reference.
A type-1 point row stores direction slots `[0, 0, 0]`, point slots `[f6, 1]`,
a non-null point identifier in the center field, zero arc-orientation and
vertical/horizontal slots, and `f6` in both radius-reference fields.

The `skamp_ptr` and `triples_ptr` array headers retain their declared counts,
table-class references, and source offsets independently of the number of rows
whose bodies decode.
The `ent_tab` and `vert_tab` headers likewise retain their declared counts,
table-class references, and row-class references independently of validated
trim rows.

For a complete `eqtn_arr`, the declared count is one greater than the number of
replayed positional rows because the count includes the named prototype. A
non-null equation argument is a zero-based ordinal into the containing
`var_arr` row table. It is not a `uvar_id`, point key, relation identifier, or
dimension identifier. Function `2` has exactly two argument slots and asserts
equality of the two referenced scalar rows. When both rows have type `1` or
both rows have type `2`, the assertion equates the corresponding coordinate of
their point keys. Function `3` has exactly three argument slots. Its first two
rows have the same type, either `1` or `2`; its third row has type `0`. The
third row key is a zero-based ordinal into a complete `dimtab_ptr` row table.
For dimension types `1` through `5`, the type-0 scalar and the selected
dimension magnitude are equal non-negative millimetre values, and the equation
asserts that the absolute difference of the first two coordinates equals that
magnitude. An inline type-0 scalar, or a value resolved for that row through a
scalar-equality component, must equal the selected dimension magnitude. A
type-0 row with the nine-byte dimension-driven sentinel receives its resolved
value from the magnitude of the selected complete dimension row. When a
function-2 row pairs one type-3 row with one type-0 row, the type-0 key is a
zero-based ordinal into a complete `dimtab_ptr` row of dimension type `3`.
The type-0 scalar, selected dimension value, and type-3 radius scalar are
equal positive millimetre values;
each scalar may be resolved through its scalar-equality component, and the
type-3 row key is the radius identity. The same dimension-driven sentinel rule
applies to the type-0 scalar in this function-2 form.
Function `0` has exactly six argument slots. The first two rows are the type-1 and
type-2 coordinates of a first point. The next two rows are the type-1 and type-2
coordinates of a second point. The fifth row has type `0` or `3` and is the
non-negative radial distance. The sixth row has type `4` or `6` and is an angle
in radians. The second point equals the first point plus the radial distance
multiplied by `[cos(angle), sin(angle)]`. A zero radial distance produces a zero
coordinate difference and does not constrain the angle. A missing radial
distance or angle is resolved only when the complete point coordinates determine
a unique value, and a stored value must agree with those coordinates before the
equation is transferred.
Function `6` has five argument slots. The first two rows are the type-1 and
type-2 coordinates of a first point. The next two rows are the type-1 and
type-2 coordinates of a distinct second point. The final row has type `3` and
is their positive Euclidean distance. A missing type-3 scalar is derived when
both points are complete; a stored scalar must agree with that distance. An
incomplete point pair or a non-positive or conflicting scalar leaves the
equation native.
Function `35` has nine argument slots. The first two rows are the type-1 and
type-2 coordinates of a target point. The next four rows are the type-1 and
type-2 coordinates of two distinct reference points. The seventh row has type
`4`; the final two rows have type `5` and resolved scalar value zero. The target
point lies on the infinite line through the two reference points. A complete line
equation is required before a missing target coordinate is solved.
Function `13` has three argument slots. The first two rows have the same
coordinate type `2` and identify distinct point keys. The final row has type
`7` and resolved scalar value zero. The equation asserts equality of the selected
ordinates. An incomplete auxiliary row, a nonzero resolved auxiliary value, a mixed
coordinate type, or an ambiguous point key leaves the equation native.
Function `5` has a direct scalar-equality form with two type-6 rows followed by
a type-5 selector row whose resolved scalar value is zero. The first two scalar
values are equal. A finite value on either type-6 row supplies a missing value on the
other row. Conflicting finite values, a missing selector, a nonzero resolved
selector, another type sequence, or an inactive solver incidence leaves this
form native.
Function `33` has nine argument slots. The first eight slots are four type-1 and
type-2 coordinate pairs. The first two pairs identify one endpoint pair and the
next two pairs identify the other. The final row has type `7` and resolved scalar
value zero. The equation asserts equality of the squared Euclidean lengths of
the two endpoint pairs. A missing coordinate is solved only when the resulting
quadratic has one finite root that satisfies the complete equation system; two
roots, no root, an incomplete auxiliary row, or an ambiguous point key leaves
the equation native.

Function `42` has three argument slots. The first two rows have the same
coordinate type, either `1` or `2`, and identify two point coordinates. The
third row has type `6`. The type-6 scalar equals the arithmetic mean of the two
selected coordinates. A missing coordinate is solved from a finite type-6
scalar and the other coordinate; a missing type-6 scalar is derived from two
finite coordinates. A stored scalar or coordinate that conflicts with the
equation leaves that row native.
Function `31` has four argument slots. The first two rows are the type-1 and
type-2 coordinates of one point key. The final two rows are distinct type-6
scalars and bind the point's `u` and `v` coordinates, respectively. A missing
coordinate is solved from its finite type-6 scalar, and a missing type-6 scalar
is derived from its finite bound coordinate. A conflicting stored value leaves
the equation native.
Function `43` has eight argument slots. The first four rows are the type-1 and
type-2 coordinates of two distinct point keys. The fifth and sixth rows have
type `4` or `5`; the seventh row has type `0`; and the final row has type `5`.
The type-0 row is a non-negative axis distance between the two points. Its
value equals the absolute difference of exactly one selected coordinate. A
missing type-0 scalar is derived only when exactly one coordinate difference is
non-zero. A stored scalar must agree with exactly one coordinate difference.
Non-zero resolved type-5 auxiliary values, ambiguous coordinate matches, and
incomplete point pairs leave the equation native.
Function `16` has a direct four-slot form with two type-4 angle rows, a type-0
result row, and a type-5 selector row. When the selector's resolved scalar value
is zero and the first angle is not less than the second, the type-0 result is the
non-negative first-minus-second angle difference in radians, bounded by π. A
missing result is derived from the two finite angles; a stored result must be finite,
non-negative, and equal to that difference. A missing selector, a reversed or
over-π difference, or a conflicting result leaves the equation native.
Function `10` has a seven-slot axis-alignment form. Slots 0, 1, and 2 are
same-type type-1 or type-2 coordinates for distinct first, second, and target
point keys. Slots 4 and 5 are the opposite-type coordinates for the first and
second point keys. Slots 3 and 6 are type-7 auxiliaries. The first and second
selected coordinates must be distinct, their opposite coordinates must agree,
both auxiliary values must resolve to zero, and the target must have a finite
selected coordinate with its opposite coordinate missing. With complete,
finite, unambiguous point rows, the equation asserts that the target's missing
opposite coordinate equals the shared opposite coordinate of the first and
second points. Any other function-10 argument shape, auxiliary value, point
identity, coordinate completeness, or coordinate relationship remains native.

Scalar-equality reconciliation is source-independent. It compares finite stored
`var_arr` row values with finite candidates from dimensions, relations, equation
forms, and coordinate solving; no source has precedence. A non-finite source or
any disagreement within a complete scalar-equality component leaves every member
of that component without a resolved scalar. A derived candidate cannot fill a
member of a conflicting component. The original stored rows remain retained in
their native variable record.

Complete native `ent_tab` rows are retained independently of whether `segtab`
is present, complete, or contains the same external identifiers. Cross-table
agreement is required only when deriving solved section topology.
The `dimtab_ptr` header retains its declared count and table-class reference
when no dimension row body validates.
Every decoded dimension row transfers as a neutral parameter with identity
scoped by its feature definition, external identifier, and repeated-row
occurrence. Dimension rows form an ordinal relation target only when the
number of decoded rows equals the declared count. An incomplete table does not
resolve a relation's dimension selector.
The `var_arr` header retains its declared count and table-class reference when
no variable row body validates; its derived point set is then empty.
The `segtab_ptr` header retains its declared count and table-class reference
when no segment row body validates.
After the positional table prototype closes, each replay row begins either
immediately or after a compound-close-terminated structural trailer. A replay
row is admitted only when its complete fixed field sequence and final
compound-close decode within the table extent.
A type-10 `segtab` row is a full circle. It has no endpoint identifiers;
the second point slot is the structural value one, `cntrid` selects the center
point, and `radius` selects the ordinal radius or diameter dimension. A
type-three selected dimension stores the radius. A type-four selected dimension
stores the diameter, so half its positive value is the solved geometric radius.
The dimension join requires a complete dimension table and a unique type-10
external identifier; it does not require every declared `segtab` row to decode.
When the type-10 external identifier is not unique, the circular-size relation
remains native.
The unique type-10 circle and selected dimension transfer as a neutral radius
constraint for type three and a neutral diameter constraint for type four.
Other dimension types do not define the circle size or a circular-size
constraint.
A type-one `segtab` row is a construction point. Its first point slot is null,
its second point slot is the structural value one, and `cntrid` selects the
point key in `var_arr`. Complete point coordinates define the neutral sketch
point. Sense zero selects the whole point in solver incidences. Construction
points do not participate in `ent_tab` profile chains. Sense four selects the
same point as a point locus.
A type-47 `segtab` row is a centered construction line when `dir=[0,0,0]`,
the point slots are `[null,1]`, `cntrid` is present, `arcorient=0`, `verhor=0`, the
primary radius-reference slot is one, and the secondary radius-reference slot
is null. Point keys zero and one are the line endpoints; `cntrid` selects their
stored center point. Complete coordinates define the bounded neutral line only
when the stored center equals the endpoint midpoint and the endpoints are
distinct. Sense zero selects the line, senses two and three select its start
and end, and sense four selects its center. Other type-47 layouts remain
opaque.
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
is the first counted row. A one-item named prototype may omit the inner item
trailer and use the outer table trailer to close its item schema. Positional
rows repeat the nested item schema for the first item, then store additional `ent_id`/`sense` pairs directly; `e2`
separates direct items when the item count exceeds two. The row trailer is
`f3` plus the table entity reference plus `e2`; a one-item row instead ends at
its item `e2`. The final row may end at the following named record, at a
following positional table header `f8 <count> f7 <class> fb e2 f7 <row-class>`,
or at a positional table wrapper `f4 04|05 f7 <class>` followed by either a
complete array header `f8 <count> f7 <class> fb e2` or the next structural
body marker. Solver
integer fields extend the compact-integer lattice with `c0..df XX YY`, equal
to `((head-c0)<<16)|(XX<<8)|YY`, and `ea XX YY ZZ`, equal to the unsigned
little-endian value `XX|(YY<<8)|(ZZ<<16)`.
The least-significant `status` bit is the constraint enable state: zero denies
the constraint and one enables it. Higher status bits are independent solver
state and remain in the native row. A disabled incidence does not supply point
equations, line orientation, radius equality, relation-operand binding, or
native-geometry role evidence. It remains an inactive neutral constraint and a
complete native incidence row. Defined incidence type, flag, and locus-sense
patterns retain their neutral constraint kind when disabled; saved coordinates
and unresolved carrier geometry are not required to satisfy an inactive
equation.
Every neutral definition, active or inactive, requires each selected locus to
match the emitted entity family. An incompatible candidate remains a native
incidence record.

For a two-item type-zero incidence, sense `2` selects the native first endpoint
and sense `3` selects the native second endpoint. Sense `4` selects an arc or
circle center. Sense zero on a type-1 construction-point `segtab` row selects
that point as a whole-point locus. The two selected point loci coincide and map
to a neutral coincident-loci constraint. When both loci are arc or circle
centers, the same incidence maps to the neutral concentric constraint for those
two circular entities. Senses `2` and `3` establish an endpoint-bearing native
curve family when the underlying line, arc, or spline family is not otherwise
known. Sense `4` establishes a native circular family and retains its center
meaning without requiring solved center coordinates. Combined endpoint and
circular evidence establishes the native arc family. A generic native entity
without these incidence roles does not establish an endpoint or center locus.
When exactly one `segtab` row owns each referenced external identifier, this
incidence equates the corresponding stored `pointid` coordinates. A solved
coordinate on either endpoint therefore supplies the missing coordinate on the
other endpoint; conflicting solved coordinates remain distinct.
For an arc or circle operand, sense `4` selects its center. A type-14 incidence
stores a symmetry axis as a sense-zero line followed by two point loci selected
with senses `2`, `3`, or `4`. A type-3 incidence between a sense-zero line,
arc, circle, or spline and a selected point locus makes the locus coincident
with the curve and maps to a neutral point-on-object constraint. A type-3
disabled incidence retains that mapping when the carrier and selected circular
center have defined native geometry families. The disabled equation does not
require an evaluated carrier or center coordinate. A type-3
incidence between a sense-zero `segtab` point and a selected point locus
equates the point's `pointid` coordinate with the selected endpoint or
arc-center `pointid` coordinate and
maps to a neutral coincident-loci constraint. Solved coordinates propagate
across that equality under the same unique-row and conflict rules as type zero.
A two-item type-9 incidence with sense zero on one line and one point makes the
point coincident with the line and maps to a neutral point-on-object
constraint. Operand order does not change the line and point roles.
A two-item sense-zero curve incidence makes the curves perpendicular for type 5. When both operands are lines, type 7 makes them parallel and type 8 makes
them equal in length.
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
coincident-loci constraint. Two sense-zero point entities also map to neutral
coincident loci. The separate sense-zero-curve form maps to point-on-object as
defined above.
A two-item type-four incidence with sense zero on both curve entities maps to
an entity-level tangent constraint. Endpoint-selected operands map to the
explicit tangent-loci form. When exactly one operand has sense zero and the
other selects an endpoint, the selected endpoint's section-point identifier
selects the unique matching endpoint of the sense-zero line or arc. The two
matched endpoints map to the explicit tangent-loci form. No match or two
matches retain the native incidence. A disabled endpoint-selected incidence
retains the tangent-loci form when the endpoint carriers remain native
geometry; carrier evaluation is not required to satisfy the disabled tangent
equation.
A two-item type-nine incidence with sense zero on two lines makes the lines
collinear. The line-and-point form maps to point-on-object as defined above.
A one-item type-ten incidence with sense zero on an arc fixes its positive
sweep to 90 degrees. The corresponding type-eleven incidence fixes the
positive sweep to 180 degrees. These map to neutral fixed arc-angle
constraints. A nonzero sense or a non-arc entity does not satisfy either form.
A one-item type-twelve incidence with sense zero on an arc makes the arc's
start and end loci horizontally aligned. The corresponding type-thirteen
incidence makes those loci vertically aligned. These map to neutral
horizontal- and vertical-loci constraints. Solver activity controls whether
the alignment participates in the solved section; it does not change the
stored endpoint roles. In the affine section solver, an active type-twelve
form equates the two endpoint `v` ordinates and an active type-thirteen form
equates their `u` ordinates. A nonzero sense or a non-arc entity does not
satisfy either form.
A one-item type-thirty-three incidence with `flags = 34`, `sense = 10`, and a
unique bounded-curve operand maps to a fixed-entity sketch constraint for the
complete bounded curve; sense `10` does not select an endpoint or center. Other
one-item type-thirty-three incidences retain their type, flags, status, entity
identity, and item sense.
A one-item type-one incidence with sense zero makes the referenced line
horizontal. A one-item type-two incidence with sense zero makes the referenced
line vertical. The unary incidence establishes the referenced entity's line
family independently of its solver activity. A separate sense-`2` or sense-`3`
solver operand or a sense-zero type-35 target whose other operand resolves to a
point locus also establishes the line role when that entity's geometry remains
native. Other senses select a locus and do not define an entity-level
orientation constraint. Disabling the unary equation does not change the
referenced entity family.
A `segtab` row with coincident endpoint identifiers is an axis line when its
`verhor` selector agrees with its unary type-one or type-two incidence
and the row is the sense-zero axis operand of a type-fourteen symmetry
incidence. The coincident section point anchors the infinite axis; it is not a
point entity or a bounded line endpoint pair.
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
A disabled point-symmetry incidence retains that neutral form when the point
center and both selected loci have defined native geometry families. Solved
coordinates are not required to satisfy the disabled symmetry equation.
A two-item type-fifteen or type-seventeen incidence stores two endpoint or
center loci that share one sketch coordinate. Flag `1` selects the section `u`
coordinate and flag `2` selects the section `v` coordinate. This discriminator
defines the neutral same-coordinate axis without requiring solved locus
coordinates. A disabled incidence retains endpoint loci established by its
sense fields even when an emitted solver-only carrier has no section-point
identity. When both loci are solved, their selected coordinates must agree.
Other flag values and contradictory solved coordinates retain the native
incidence.
Types 30 and 31 store the same two-locus relation with a fixed coordinate:
type 30 selects section `v`, and type 31 selects section `u`. Their `flags`
field does not select the coordinate.
A two-item type-35 incidence whose operands resolve as one point locus and one
bounded line or arc places that point at the entity midpoint. The target entity
has sense zero. A centered type-47 construction line instead uses sense `4` on
the target line; its stored center point is the line midpoint. This form
establishes the other sense-zero solver-only entity as a point independently of
incidence activity. The point operand is either a sense-zero point entity, the
center of a sense-zero arc or circle, or an endpoint or center locus selected
by sense `2`, `3`, or `4`. Exactly one operand pairing must supply a bounded
target and a point locus; zero or two pairings retain the native incidence.
Operand order does not change these roles. A circle is not a bounded midpoint
target.
An unresolved centered type-47 construction line remains a native line carrier,
but its sense-four center is a valid midpoint locus. This center role does not
establish line coordinates or any other line geometry.
A two-item type-37 incidence with two sense-zero operands binds a projected
reference entity to the regular profile entity copied from it. The reference
entity identifier immediately precedes the profile entity identifier. The
profile entity occurs exactly once in the trim table. The relation retains the
ordered source and result identities independently of solver activity. Other
type-37 operand shapes remain native. A neutral projected-copy constraint is
valid only when both materialized source and result geometries are identical;
when both are materialized and differ, retain the native incidence.
An incidence item may reference a complete saved-section entity through its
`order_table.ext_id`. When its type/sense pattern has no neutral constraint
mapping, retain the incidence type, ordered entity identifiers, and sense values
as one native sketch constraint; the absence of a typed locus interpretation
does not remove the solver relation. `relat_ptr`, `skamp_ptr`, `triples_ptr`,
`order_table`, and saved-section entities remain valid when `segtab_ptr` is
absent; segment-dependent refinement is withheld without dropping those design
records.
A solver incidence entity identifier with no `segtab_ptr` or ordered
saved-section definition is a solver-only section entity. It retains one
construction-entity identity shared by every incidence in the sketch; its
geometry remains native. A unique non-conflicting line role from a two-line
type five, seven, or eight incidence retains the native line family. This line
evidence applies only to solver-only entities and does not replace the family
of a decoded `segtab` row. Sense `4` retains the native circular family
independently of solver activity; a
two-circle type-six incidence supplies the same role while active. Sense `2` or
`3` retains the native endpoint-bearing curve family; independent line evidence
narrows that family to line, while circular evidence narrows it to arc.
In a type-zero coincidence, a sense-zero solver-only entity paired with an
endpoint or center locus of a uniquely established carrier family retains the
native point family.
Conflicting family roles retain the generic solver-only family.
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
A `segtab` line whose stored selector, unary type-one/type-two incidence, or
consistent type-five/type-seven component uniquely fixes one coordinate is an
unbounded axis-parallel line when both decoded endpoint values for that
coordinate agree. The other endpoint coordinate may remain unresolved and
does not define a finite extent.
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
It also inherits the incidence activity state: an odd `status` is active and an
even `status` is inactive. Activity transfers independently of whether the
relation has a neutral typed mapping. An absent or ambiguous incidence join
leaves relation activity unspecified.
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
When the selected dimension is schema-defined with stored value zero, the same
type-zero linear operand vectors instead replay an orientation incidence only
when the relation joins exactly one active unary type-one or type-two incidence
on the same line, the vector point pair is that line's endpoint pair, and the
line's independent orientation selectors agree with the incidence kind. The
relation transfers as the joined horizontal or vertical line constraint without
a dimensional parameter. A nonzero schema-defined value remains native.
The same locus and entity mappings apply when the joined incidence is disabled;
the resulting distance is inactive and does not require its equation to be
satisfied by resolved geometry.

Within the three four-slot `relat_ptr` operand vectors, `e5` expands to two
zero slots and `e6` expands to three zero slots. `e4` is the integer value one,
and `f6` is a null operand. Expansion is bounded independently at four slots
for each of `a`, `b`, and `c`.

For a type-zero linear-distance relation with operand-vector forms
`a = [point0, point1, null, 1]`, `b = [0, 0, 0, 0]` or
`b = [1, 1, 0, 1]`, and
`c = [15, 16, 15, 1]`, the referenced dimension supplies the distance between
the two points along the measured horizontal or vertical segment. Sign `1`
adds the dimension and sign `f6` subtracts it. Sign zero stores only the
unsigned magnitude: it supplies the invariant
`abs(second-first)=value`, but does not select an orientation. A coordinate
solver may test both orientations only after the dimension table is complete
and the measured coordinate is established. It admits a derived coordinate
only when every consistent orientation gives the same value; an unresolved
orientation supplies no coordinate. Rows from a dimension table missing any
declared dimension do not supply coordinates. Equivalent rows define one
coordinate invariant. Rows that assign different signed differences to the
same unordered point pair and coordinate define no solved coordinate for that
equation.

A type-zero relation with vectors `a=[first_point,second_point,null,1]`,
`b=[0,0,0,0]` or `b=[1,1,0,1]`, and `c=[15,16,15,1]` is a
segment-aligned linear dimension.
Its dimension selector is a zero-based index into `dimtab_ptr`. `verhor=1`
selects the section `u` difference and `verhor=0` selects the section `v`
difference. Sign `1` defines `second-first=+value`; sign `f6` defines
`second-first=-value`; sign zero defines only the unsigned magnitude invariant
above and leaves its orientation to the complete section solve.
Only a linear selected dimension contributes this section-coordinate equation;
an angular or schema-defined dimension does not supply a length ordinate.
The two point identifiers denote endpoint or center loci owned by unique
`segtab` entities. An arc or circle center is a valid locus when its carrier
external identifier is unique. A segment spanning the pair is not required
when each point has an incident unique entity and the two solved points agree
on exactly one coordinate. When a point key has more than one incident unique
carrier, its locus is ambiguous and the affected relation remains native.
Equal `u` selects a vertical distance and equal
`v` selects a horizontal distance. The selected `dimtab_ptr` row is the
driving parameter independently of whether both point coordinates are
evaluated.
A spanning segment's unique orientation component otherwise selects the neutral
distance axis.
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
dimension is the neutral radius constraint parameter, except that a type-four
dimension produces a diameter constraint because its stored value is the full
diameter.

A type-six relation with sign `1`, complete vectors
`a=[first_point,second_point,0,1]`, `b=[center_point,0,0,0]`, and a complete
four-slot `c` selector binds the selected linear dimension to the unique arc
whose endpoint pair, center, and `radius` dimension index match those stored
operands. Endpoint order does not affect the radius. The `c` selector must be
complete but does not select a different arc in this form. The selected
dimension is the neutral radius constraint parameter, except that a type-four
dimension produces a diameter constraint because its stored value is the full
diameter. An incomplete selector, incomplete dimension or relation table, or
ambiguous arc identity remains a native relation.

A type-14 relation with `a=[radius_id,0,0,0]`, `b=[0,0,0,0]`,
`c=[15,0,0,0]`, and sign `1` binds the selected dimension value to the
type-three `var_arr` radius with that key. An arc's `radius` field selects the
same radius key. The solved center point and positive radius define its
unbounded circular carrier before both arc endpoints are available.
Only a linear selected dimension contributes a solved radius.
For a type-four diameter dimension, the propagated radius is half the selected
dimension value.
The selected dimension is the neutral circular-size constraint parameter when
exactly one arc's `radius` field names that key and the selected dimension type
is linear. Type four produces a diameter constraint; other linear types produce
a radius constraint.
A non-linear or schema-defined selected dimension does not define a neutral
circular-size constraint.

The named `segtab` row before its schema close is likewise a data row. Its `type`, `dir`, `pointid`, `cntrid`, `arcorient`, `verhor`, radius, and `ext_id` fields contribute one segment to the declared table count.
In a positional replay, `f2 f7 <table-class> e2` after the array header closes
the inherited prototype without repeating its fields. That elided prototype
contributes one entry to the declared count but does not create another segment
entity. A positional segment table is complete when the elided prototype plus
its complete replay rows equals the declared count.
Positional rows may insert the two-byte `c0 80` or `c1 00` wrapper before
`type`. The wrapper does not change the following field layout. A compact
`ext_id` value of zero is an identifier; the `f6` control sentinel represents
an absent value.
The `c0 80` wrapper may also precede the named row's scalar `type`. Segment
Segment layouts outside the defined families retain the same fields and count
toward table completeness, but do not define neutral geometry.
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
The named dimension prototype may carry a nested `dim_ref` table. Its header is
`f8 <count> f7 <entity-ref> fb e2`; the count includes the named prototype.
The named row stores `item_id`, `sense`, and a two-slot `point` array. A
positional replay row stores those fields in the same order. `item_id` and
`sense` are nullable compact integers. Each point slot is nullable; `e4`
encodes one and `e5` and `e6` expand to two and three zero slots respectively.
The first replay row follows `f1 f7 <entity-ref> e2`; later replay rows use
`f3 f7 <entity-ref> e2` separators. The nested table returns to its enclosing
dimension row at `f3 f7 <dimtab-entity-ref> e2`.
`value` and `aux_value` are independently bounded scalar bodies. The encoding
and decoded numeric value of either field do not constrain the other field.
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
The positional `value` lane uses the positive DICT lattice `53..a3`; the first
two IEEE bytes are `3F75 + prefix` and the following six bytes complete the
value; `ad` is an alias for leading bytes `3F D9`. The seven-byte
`31 <tail6>` form reconstructs `[40, tail6, 00]`. A bare `18` value is zero. Unresolved `00 XX YY` and `01 XX YY ZZ`
value forms occupy three and four bytes respectively. Compact `0e` is `-0.5`, so
the following one-byte `direct`, `aux_value`, and compact `ext_id` fields remain
aligned. Each unresolved form is a bounded token distinct from a scalar value
or expression.
Type `0x03` has radius display semantics.

A `segtab` line whose two endpoint identifiers each have complete type-1 and
type-2 `var_arr` values is the bounded segment between those two `[u, v]`
points when their normalized separation exceeds `1e-12`. Coincident endpoint
coordinates do not define a bounded line. A neutral ordinate requires exactly
one `var_arr` row with the point key and coordinate type, or repeated rows
whose defined values agree.
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

An analytic carrier pair transfers its sole intersection-curve candidate when edge endpoints are unresolved. When solved edge endpoints exist, they must lie on the candidate. When the pair produces multiple curve candidates, transfer requires paired solved endpoints contained by exactly one candidate.

Every uniquely identified transferred analytic surface is available to the native topology solver as its model-space carrier. This includes planes derived from feature geometry even when the plane has no independently complete row-local placement frame.

A transferred NURBS boundary curve supplies a face plane when its complete knot
and control-point record has no weight lane or a finite positive weight lane
and the control net contains three non-collinear coplanar points. A non-periodic degree-one NURBS
boundary supplies a boundary line when every control point is collinear with
its first and last points and it has no weight lane or a finite positive weight
lane. Invalid,
degenerate, non-coplanar, or non-collinear control nets do not supply a plane
or line.

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
A class-`942` `Surface` operation with an `Extrude <decimal-ordinal>` reference
identifies a sheet extrusion. The operation creates an independent quilt
rather than performing a solid boolean.
The fill operation creates a planar quilt from one closed sketch boundary. It
has no adjacent support faces, imposes positional continuity at the boundary,
and does not merge the result into an existing quilt.
Exactly one section transform bound to a Fill feature selects its section
definition by definition identifier and section offset. If the Fill has no
bound section transform, exactly one definition owned by the Fill supplies the
sketch boundary. Multiple bound transforms, an unmatched bound transform, or
competing owned definitions leave the boundary unresolved. The selected sketch
identity transfers independently of sketch placement and profile resolution.
`Merge <decimal-ordinal>` identifies a surface-merge operation.
Root feature-definition class `946` identifies the same surface-merge family
when the current-state record omits its display name. The class value does not
encode face selection or merge operands.
The operation merges coincident boundary entities of its input quilts and
retains the result in the quilt namespace. It does not create a solid body.
For a surface-merge feature, each entry in a class-`100` generated-entity table
names the base input entity. The input is established only when exactly one
preceding feature-generated class-`200` entry has the same entity identifier.
The `qlts_affected` array is the ordered roster of every quilt participating in
the merge, including the base input. Each quilt identifier occupies the
feature-generated entity namespace and joins to its generating feature through
the equal identifier of a class-`200` entry.
For each joined quilt, the generating feature's unique class-`100` entry with
the same entity identifier supplies the corresponding table surface identifier.
That surface identifier must resolve to exactly one surface row owned by the
generating feature; otherwise the quilt remains a native selection.
In a compact class-`946` replay row, an `f7 150` anchor precedes the counted
removed-entity array and its `01 e3` close. The affected-geometry and
affected-edge arrays follow, then `f0 f7 153`, the affected-quilt array, and a
suffix that repeats the replay-row identifier. Each affected-array position
inherits its count from the preceding class-`946` row in the same feature
stream when its `f8 <count>` opener is omitted. A named row supplies the same
state through `geoms_affected`, `edgs_affected`, and `qlts_affected`.
`Extrude <decimal-ordinal>` identifies an extrusion operation.
`Intersect <decimal-ordinal>` with exactly one feature-owned class-`29` entity
table containing materialized surfaces identifies a section-shape operation
that creates curves at the intersections of two selected shape sets.
`Boundary Blend <decimal-ordinal>` identifies a boundary-surface operation.
`Protrusion` identifies a linear extrusion operation; absent section operands
leave its profile, direction, and extent unresolved without changing its family.
For `Protrusion`, `Cut`, `Extrude`, `Revolve`, and numbered `Extrude` or
`Revolve` operations, exactly one feature-bound section transform selects its
matching section definition.
Without a feature-bound transform, exactly one definition owned by the feature
selects the section. The selected section supplies the native profile identity
independently of whether the sweep direction or axis and termination resolve.
Multiple transforms or competing owned definitions leave the profile unresolved.
The German operation-family names `Bezugsebene`, `Rundung`,
and `Schräge` denote the same datum-plane, round, and draft families as
`Datum Plane`, `Round`, and `Draft`, respectively. `Annotation Feature` is a
non-modeling annotation container.

For a root schema class `927` Draft feature, the sole class-`209` entry across
the feature-owned generated-entity tables supplies the neutral plane when its
entity identifier is in that table's materialized-surface roster, resolves to
exactly one `srf_array` row, that row is a plane, and its `feat_id` equals the
Draft feature identifier. The enclosing table class is not part of this rule.
The neutral-plane selection uses the native
`creo:visibgeom:surface#<surface-id>` identity. Missing, duplicate,
unmaterialized, foreign-owned, non-plane, or otherwise ambiguous class-`209`
carriers leave the neutral plane unresolved. Class-`224` and class-`230`
entries do not by themselves define the drafted-face selection.

`Cross Section` and its German operation-family name `Querschnitt` are
non-modeling cross-section definitions. A current-state `Body` or `Körper`
record with no recipe, root feature-definition class, or feature reference name
is a solid-body model-tree node. A `Surface` record under the same conditions
is a surface-body model-tree node. A recipe, root class, or reference name
takes precedence and identifies the corresponding modeling operation. `Mirror`
identifies a reflection operation.

Operation names end in ` id <N>` or ` ID <N>`; the stored case follows the
name's localization. An ASCII `o`, `x`, `y`, or `z` byte immediately preceding
an uppercase operation-family name is a stored-name prefix, not part of the
family name. Multiple operation names with the same feature identifier are
ordered stored-state candidates. No candidate is the current state without a
state selector. State ordinals are local to one feature identifier and increase
in byte order from zero. Each candidate retains the prefix-inclusive name
bytes, the `id`/`ID` spelling, and the offset of the optional prefix. The
neutral projection retains only operation fields on which all candidates
agree. A recipe-only state has no stored operation name.

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
source tag, parent, and dependency. Multiple recipe bindings for one feature
must agree on the recipe, root schema class, and parent identifier.

Within one current-state record, `protextrude` identifies an additive linear
section sweep, `cutextrude` identifies a subtractive linear section sweep,
`protrevolve` identifies an additive rotational section sweep, and
`cutrevolve` identifies a subtractive rotational section sweep. The recipe
name precedes the `<Kind> id <N>` operation name and applies to that feature
state when it is the sole complete recipe name in the bounded record. Multiple
DEPDB bindings for one feature apply only when their recipe, schema class, and
parent identifier agree. Conflicting recipe candidates leave the recipe,
recipe-bound schema class, parent, operation family, and Boolean effect
unresolved.
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

A definition instance selects geometry, placement, and operation semantics by
its bounded record identity. The `feat_defs_<id>` value alone identifies an
instance only when exactly one bounded definition carries it. When the schema
identifier repeats, the absolute `gsec3d_ptr` offset qualifies the instance and
joins its section transform; an identifier without that offset remains
ambiguous.

An instantiated positional definition begins at
`e0 01 feat_id 00 <canonical-reference> e0 00 ref_model_info 00`. The reference
is its owning modeling feature identifier. This boundary ends the preceding
labelled template or positional instance.

An unlabeled positional definition begins at `e3 S2D<digits> 00`. The next
such boundary ends the instance. A uniquely keyed `ent_tab` selects the unique
unclaimed feature whose nonempty class-200 source-entity identifier set exactly
equals its `ext_id` set. When no exact candidate exists, the source-entity set
must be contained in the instance's `order_table.ext_id` set. In either form the
feature must select exactly one unlabeled instance. Definitions without this
reciprocal unique join have no owner. They remain section definitions and
retain their complete bounded body. Replay order does not define feature
identity.

An unowned instantiated saved section joins the unique unclaimed feature whose
nonempty class-200 source-entity identifier set exactly equals the section's
uniquely keyed `ent_tab.ext_id` set, provided that feature selects exactly one
such section. This join assigns the canonical feature owner and preserves the
stored `feat_defs_<id>` schema identifier. A partial, competing, or reused set
does not assign an owner.

`DEPDB_DATA` and each complete bounded `AllFeatur` feature row store an
internal sketch-datum chain. A procedural recipe feature
`F` immediately followed in feature-state order by a non-recipe feature
`F + 1` owns the unique section definition whose `gsec3d_ptr.sketch_plane`
entity is `F + 2`. The intermediate feature is the section datum. When more
than one definition selects the same sketch-plane entity, the chain does not
select a regeneration snapshot and none of those definitions acquires the
owner. A definition is eligible for this join only while its byte offset lies
inside the complete source range that contains it. When the definition is
contained by an `AllFeatur` row, `F` depends on that row's saved-section
history feature; the row context does not replace the section-definition
identifier or select the modeling operation.

In `DEPDB_DATA`, `gsec2d_ptr 00 e0 0a name 00 S2D<digits> 00` begins a
labelled section definition. Its labelled table records define the positional
table classes used by following unlabeled `S2D` definitions. The next labelled
`gsec2d_ptr`, unlabeled `S2D`, or feature-definition record ends its body.

The same labelled section-definition form may occur inside a complete bounded
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

Classes 913 and 914 store `geoms_affected` and `edgs_affected` as the first and second
affected-array schema positions. Each position has independent extent state
within one `AllFeatur` stream and schema class. `f8 <count>` replaces that position's current
extent; omission of `f8` reuses its preceding extent. Exactly that many compact
identifiers belong to the position before the next position begins. The first
row can carry the field labels; positional rows omit them without changing the
two positions. The positional pair begins after `f1 f7 42 <variant> 80 01 e3`,
where `<variant>` is `c8` or `d8`. Before an explicit second-position `f8`,
`f7 <canonical-reference>` identifies the replayed schema position and does
not belong to either identifier array. An omitted second-position extent also
omits that reference. The unanchored positional form ends the pair immediately
before `e1 e1 <row-id> e3 <suffix> <selector> <row-id> 00 e1 00 <tail>`.
`<suffix>` is either `e3` or `f7 <canonical-reference> e3`. The repeated compact
`row-id` values must agree. `<tail>` is either the `e3` compound close or an
`e1` null row-tail marker. The pair begins immediately after a compound
close, and its two stateful extents must consume the bytes up to that suffix
exactly.
An unanchored row can instead terminate two explicit affected arrays after
generated-surface and generated-edge arrays. In that form the final two
`f8 <count> <ids...>` arrays before the repeated-row suffix are
`geoms_affected` and `edgs_affected`. The arrays are adjacent, separated by
`f7 <reference>`, separated by `f0 f7 <reference>`, or separated by
`f1 f7 <reference> 01 e3 [f7 <reference>]`. The second array is followed
immediately by the repeated-row suffix, by `f1 f7 <reference>` and that
suffix, or by `f5 96 92 00` and that suffix. Earlier arrays in the row remain
generated-output tables. The exact trailing explicit-array form takes
precedence over inherited-extent probing for the same suffix. More than one
exact pair leaves the row opaque.

Repeated named affected-ID arrays for one feature and namespace are distinct
stored states. They define a neutral edge selection, parent set, generated
output set, or round support set only when their ordered identifier arrays are
identical. Conflicting arrays remain native operation parameters.

A class-914 equal-distance chamfer is represented by circular generated cone
surfaces with zero apex radius and a half-angle of pi/4. Each cone axis points
from its apex toward the affected support plane. The chamfer setback is the
smallest positive axial distance from each apex to an affected plane whose
normal is parallel to the cone axis. Every generated cone yields the same
setback. Only affected identifiers that resolve to model surface-plane
carriers participate in the support-plane set; other affected geometry
identifiers do not select support planes. Every recognized affected model
plane must have one unambiguous placement.
An agreed `edgs_affected` identifier selects the B-rep edge with the same
`crv_array` curve identifier when that edge is present in the transferred body.
When that global edge is absent, the unique `crv_array` topology row with the
same identifier selects the feature-local edge in the regenerated result of
the row's `feat_id` feature.
The bodies containing those selected edges are the feature's modified outputs.
In the legacy ASCII feature graph, the unique Sld_Features root owns direct
first_feat_ptr and next_feat_ptr feature nodes. A feature node has one scalar
id and one feat_type_ptr child with one scalar type. Type 913 selects a round
feature. The unique Sld_FullData root owns one complete dim_array. Each direct
dimension element with type = 8, dim_type = 3, and feat_id equal to the round
feature id owns one dim_dat_ptr child. Its unique scalar value real is the
design radius. A constant radius is admitted only when every matching value is
finite, positive, and bit-identical. A missing dimension row supplies no
radius witness; a malformed, non-positive, or differing matching set supplies
an unresolved radius.
The complete visible crv_array rows whose feat_id equals the round feature id
are its result-edge identities only when every crv_id is unique. An absent or
ambiguous row set does not select edges.
Positional replay geometry and edge arrays use the same agreement rule,
including empty arrays; an empty and a nonempty state conflict.

For a class-913 cylindrical slot fillet, the first two `geoms_affected`
identifiers are the axial cap planes. The remaining identifiers are tangent
support faces. The constant fillet radius is half the perpendicular gap between
parallel support planes. Multiple parallel support pairs define one constant
radius only when all nonzero gaps have the same magnitude. When every generated
cylinder carrier is placed, their common positive radius independently defines
the constant fillet radius. Differing positive radii across that complete
placed cylinder set identify the variable-radius form and define no
constant-radius result.
An all-cylinder generated set whose rows each carry a complete type-24 round
envelope, or an all-type-26 set whose rows each carry a complete tagged radius
trailer, identifies the variable-radius form when its positive rolling radii
differ. The radius samples remain unresolved until their edge-chain positions
are decoded.
Two or more independently decoded generated rolling-radius samples that differ
also identify the variable-radius form when other generated rows have
unresolved radius bodies. An unresolved sibling row cannot make the observed
unequal radii constant.
When every surface row generated by the round is type `26`, every row must
carry a complete tagged radius trailer. Their normalized `radius2` values are
the rolling-ball radii of the toroidal patches and define one constant fillet
radius only when all values agree.
When those rows have no tagged radius trailers, a uniquely associated named
torus prototype supplies the rolling-ball-radius candidate from `radius2`.
Each generated row can prove that candidate by replaying it exactly as the
final scalar in its terminal scalar frame. A complete terminal outline also
proves it when exactly one of the three corresponding endpoint-coordinate
deltas equals the candidate.
The untagged five-coordinate envelope is an independent radius proof. With
coordinates `[a1,a2,b0,b1,b2]`, it requires `a1 = b0`; the two remaining
endpoint deltas, under exactly one coordinate ordering, must equal
`2*(radius1+radius2)` and `radius2`. The split four-coordinate form applies the
same two-delta rule to its leading and trailing coordinate pairs. Every
generated row must satisfy the exact replay, outline, or envelope proof against
the same prototype radii. The candidate then defines the constant fillet
radius.
An owned round set may mix type-24 cylinder rows and type-26 torus rows. When
every row carries its complete family-specific radius form, the type-24 rolling
radius and type-26 `radius2` values form one common radius set. The round is
constant-radius only when every value is positive and equal.
Two linearly independent parallel support pairs with the same gap locate the
cylinder axis at the intersection of their midplanes. Intersecting those
midplanes with either axial cap plane fixes the carrier origin. Every support
plane must be parallel to the axis and tangent at the common radius. The
construction transfers a carrier only when the feature has exactly one
unplaced materialized cylinder row and every support plane satisfies these
constraints.

An `AllFeatur` feature row starts at section-body offset zero, immediately
after the section's `#<name>\n` header, or immediately after an `e3` compound
close. Its leading canonical compact feature identifier is followed by a
two-byte row header. The row-header bytes are retained but are not a fixed
allowlist for row discovery. Within the first 16 bytes after the feature
identifier, after that row header, the fixed prefix contains
`e3 f6 <compact-class> e1`. The compact integer is the root `FeatDefs`
schema class for that feature. A candidate without this complete prefix is
not a row. A row ends immediately before the next candidate satisfying the
same boundary and prefix rules with a different feature-identifier/schema-
class pair, or at the section end. Within one stream, a feature identifier
and root schema class pair identifies one row; a later candidate with the
same pair does not create a separate boundary. A feature identifier and
row-shaped bytes inside an existing row do not create a boundary.

The root schema class dispatches the row to its operation-definition grammar.
Class 916 is a subtractive section-sweep definition and class 917 is an
additive section-sweep definition; their recipes discriminate linear
extrusion from rotation. Class 911 is a hole definition, class 913 is a round definition,
class 914 is a chamfer definition, class 923 is a datum-plane definition, and
class 926 is a saved section. In a DEPDB recipe prefix, the root schema class
performs the same dispatch. Class 979 with the exact model-reference name
`PRT_CSYS_DEF` is the default part coordinate-system feature. A uniquely owned
definition with one complete `local_sys` stores the coordinate-system x, y,
and z axes in its first, second, and third triples and the model-space origin
in its final triple. The three normalized axes must be pairwise orthogonal and
right-handed. Other class-979 frames remain unresolved.

The labelled feature-row schema stores the ten procedural choice fields in
order from `blend_choice` through `misc_choice`. Each field ends at the next
choice header. `misc_choice` ends at the following `assoc_type` named-record
header; `assoc_type` and later row fields are not part of the misc-choice
payload.

A class-926 row containing one section definition is the history node for that
planar sketch. The contained definition identifier selects the neutral sketch
and the row identifier remains the history feature identifier. The section's
modeling owner remains independent. A definition without this unique
containment join uses a definition-scoped sketch history node.

Every byte-bounded `AllFeatur` row denotes a history feature independently of
whether the feature owns a materialized surface row. A recognized root schema
class selects its neutral operation type. Other root schema classes retain a
native operation with the schema class as a typed source property unless an
independent stored operation name selects a defined family. Rows sharing one
feature identifier but carrying conflicting root schema classes retain the
conflicting classes as source properties. Those classes do not select a
neutral operation family; an independent stored operation name can still do
so.

The row's leading entity-reference identifier occupies a row-local numeric
namespace that can collide with model-feature identifiers. A materialized
surface whose `feat_id` equals the row identifier establishes ownership.
An identifier in `parent_feats` establishes ownership because that table uses
model-feature identifiers. Without either structural join, a `MdlStatus` or
`DEPDB_DATA` operation state establishes ownership only when its root class or
defined operation family agrees with the row's root class, or when the row
class is outside the defined operation-class set. An `MdlRefInfo`
feature-name entry establishes
ownership for a section row, or for a datum-plane row when the stored name is
`Datum Plane id <feature-id>`, `Bezugsebene ID <feature-id>`, or
`DTM<decimal-ordinal>`. The exact `PRT_CSYS_DEF` name establishes ownership of
a class-979 coordinate-system row. Numeric equality alone does not establish
ownership.

Each `DEPDB_DATA` recipe row ends with its canonical `f7` recipe binding. Its
body begins at the section boundary or immediately after the preceding recipe
binding. Multiple bindings in one persistence section define independent
feature rows.

A mixed generated-entity table opens as
`f8 <count> f7 <table-class> fb e3`. The first entry can begin with
`f7 <entry-class>`; table and entry schema-class identifiers vary by schema
stream. A first counted prototype stores that prefixed class, its identifier,
and its body without repeating the class after the identifier. Positional
entries store their identifier and repeated class. Exactly `count` entries
follow. An entry normally ends at `e3`. A final class-200 entry with one-byte
body `00` or `01` can end immediately before the `f2 f7` separator that opens
the following table's inherited-class prefix.

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
For a complete full-turn revolution, every `srf_array` row whose `feat_id`
selects the feature must transfer to one carrier before generated carriers can
define the revolution axis. Cylinder, cone, and torus carriers contribute their
axis lines; plane carriers contribute their normal directions; sphere carriers
contribute their centers but no direction. All contributed lines must be
coaxial, all plane normals must be parallel to them, and every sphere center
must lie on the common line. The common unoriented line is the revolution axis.
Its origin is the point on the line nearest the model origin, and its direction
is sign-canonicalized by the first nonzero component. A partial angular extent
does not use this unoriented-axis reconstruction.

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

In the paired-replay form of a class-911 table-class-29 generated table, an
adjacent class-204 and class-203 pair opens a replay run of the contiguous
class-200 entries that follow it. The next class-204 entry or the next
non-class-200 entry closes the run. Exactly two runs contain materialized
surfaces. These two materialization runs have the same nonzero source roster
and pair entries by source identifier. Additional replay runs contain only
rowless topology uses. Reuse of a source identifier in those topology runs
does not add an entry to its materialization pair. An optional source-zero
entry is rowless and occurs at most once. An entry without a source identifier
is also rowless. Neither form participates in the paired source roster.

A cylindrical stepped entry has two source section entities whose paired
materialization entries are both cylinder rows and one other source whose pair
contains one materialized plane row and one rowless face use. The paired
cylinder rows are the two patches of each cylindrical step. The plane is an
axis-normal step support. A complete local-system frame on that plane stores a
point on the hole axis as its origin, and its normal supplies the unoriented
hole axis. The frame alone does not assign the entry position, drilling
direction, or step depth. When the feature generates no conical surface, this
structure selects counterbore form independently of whether both cylinder
carriers and the counterbore dimensions are evaluable.

A split-patch class-29 counterbore table has exactly five unique materialized
surface rows owned by the feature: four cylinders and one plane. Its
materialized class-200 entries cover that complete surface set exactly. The
cylinders form exactly two groups of two by nonzero source section entity;
the plane has a distinct nonzero source section entity and one rowless
class-200 companion for that source. At least one adjacent class-204 and
class-203 pair is wholly rowless. This layout selects the same counterbore
form as the paired-replay layout; dimensions, carriers, entry position, and
direction still require their independent proofs.

A class-911 table-class-29 simple-drilled recipe has one paired source that
materializes two cone rows, one paired source that materializes two cylinder
rows, and either two or three other paired sources that each contain two
rowless face uses. Every source in the materialization roster has one of these
forms. A complete three-row class-911 external-ID-2 dimension table assigns
external ID `0` to the bore radius, ID `1` to the included drill-point angle,
and ID `2` to the blind depth. IDs `0` and `2` have
dimension type `2` and millimetre units. ID `1` has dimension type `10` and
radian units. The bore radius is positive, the depth is nonzero, and the
included angle is strictly between zero and π. The neutral bore diameter is
twice the stored radius. The depth magnitude is the blind length; its sign is
an orientation state and does not change that length. Only a table with these
exact three row signatures participates in the external-ID-2 family. Other
three-row layouts are independent template families.

The recipe with exactly two rowless source pairs selects the external-ID-2
depth family. The recipe with exactly three rowless source pairs selects the
external-ID-4 depth family. The external-ID-4 family assigns ID `0` to the bore
radius, ID `1` to the included drill-point angle, and ID `4` to the blind
depth. Its types, units, value invariants, and neutral conversions are the same
as the external-ID-2 family. A different rowless-pair count does not define a
simple-drilled recipe.

Each materialized cylinder row in this recipe has a type-24 compound-close
parameter record. The last six scalar slots in the final frame are two
three-coordinate envelope corners. An entity-reference suffix or exact
`f7 17` compound-close suffix terminates this frame. Normalize the two endpoint
values on each axis independently. When the two cylinder rows have equal
normalized intervals on an axis, their common nonzero interval length is a
candidate span. When one normalized interval ends where the other starts, the
nonzero length of their union is a candidate span. When the intervals share
exactly one lower or upper bound, the nonzero difference between their other
bounds is a candidate span. A dimension tuple matches the generated cylinders
when its bore diameter and blind-depth magnitude match candidate spans on two
distinct axes.

When the blind depth is the unique common span on one axis, the two remaining
axes define the bore cross-section only if one has a common diameter span and
the other has two adjacent intervals whose union has the same diameter. The
two raw corner pairs have the same signed depth delta. Their first axial
coordinate is the hole entry coordinate, the radial union midpoint is the hole
axis position, and the sign of the depth delta is the hole direction.

A one-sided cylinder-patch pair shares exactly one normalized bound on a radial
axis. Its two non-shared bounds differ by the bore diameter. The midpoint of
those non-shared bounds is the hole-axis coordinate on that radial axis. The
other radial axis has one common span equal to the bore diameter and supplies
its midpoint. The blind depth remains the unique common axial span, and the two
raw corner pairs have the same signed depth delta. Their first axial coordinate
and signed delta supply the hole entry coordinate and direction as in the
complementary form. Pairs whose non-shared bounds do not differ by the bore
diameter do not supply placement.

A clipped radial cylinder-patch pair has the blind depth as its unique common
axial span, one adjacent-union radial span equal to the bore diameter, and one
common nonzero radial span that is not the bore diameter. Its two corresponding
cone rows each have a compound-close parameter body with exactly seven scalar
tokens. In generated order, the final three cone tokens equal the corresponding
cylinder envelope's second radial coordinates and first axial coordinate. The
cone coordinate on the clipped radial axis must be equal in both rows. That
coordinate is the missing hole-axis coordinate. The adjacent-union midpoint is
the other radial coordinate. The first axial coordinate and the common signed
axial delta supply the entry coordinate and direction. This cross-record form
does not make other cone terminal triples model-space origins.

When the cylinder envelope is available, complete three-row class-911 tables
whose diameter and depth do not match it do not participate in template
selection. All participating tables must supply one equal tuple. When the
envelope is unavailable, every complete three-row class-911 table in the
recipe-selected dimension family must supply one equal tuple. The recipe
transfers as a simple drilled hole with that diameter, angle, and blind depth.
Each complete positional cylinder frame on the paired cylinder rows must have
the dimension-assigned bore radius. The available frames must define one
coaxial line; origins may differ only along that line and axis signs may differ.
One available frame is sufficient because the recipe binds both rows to one
cylinder source. This carrier supplies an unoriented hole-axis placement. It
does not supply the hole entry position or drilling direction.
The dimension tuple alone does not assign the hole axis, entry position,
placement face, or depth-to-tip state.
Unsourced class-200 entries are admitted only when they are rowless
non-surface entities; they do not create source section entity groups.
When a feature owns multiple table-class-29 tables, exactly one table must have
the simple-drilled recipe. Zero or multiple matching tables do not select it.

An instantiated class-911 positional definition inherits schema identifier
`911` from its preceding `feat_defs_911` template. Its complete four-row
dimension table assigns external ID `0` to the bore radius, ID `1` to the
placement distance, ID `2` to the counterbore depth, and ID `3` to the
counterbore radius. IDs `0`, `1`, and `3` have dimension type `2`; ID `2` has
dimension type `1`. Bore and counterbore diameters are twice their stored
radii. A replay supplies neutral hole dimensions only when its ID-3 radius
equals a generated larger-cylinder radius for that hole and all matching
replays agree.

A complete five-row envelope-bound counterbore table assigns external ID `0`
to the counterbore depth, ID `1` to the bore radius, ID `2` to the included
drill-point angle, ID `3` to the counterbore radius, and ID `4` to the placement
distance. A four-row table either omits ID `2` and retains IDs `0`, `1`, `3`,
and `4`, or shifts the last two fields down so that ID `2` is the counterbore
radius and ID `3` is the placement distance. Linear rows have millimetre units.
The depth has dimension type `1`; radii and placement distances have dimension
type `2`. The included angle has dimension type `10` and radian units. The
depth is nonzero. Its magnitude is the counterbore depth, and its sign is an
orientation state. Both radii are positive, the counterbore radius is larger
than the bore radius, and a present included angle is strictly between zero and
π.

Each of the two cylinder source groups has two type-24 terminal corner
envelopes. One group has the bore diameter as a candidate span on exactly two
axes. The other has the counterbore diameter on exactly two axes and the
counterbore depth on the remaining axis. Candidate spans are common spans,
adjacent-union spans, and one-sided non-shared-bound differences. When both
groups have complete envelopes, the source assignment must be unique. When
exactly one group has complete envelopes, it must match exactly one of these
two roles. Exactly one class-29 entity table contains materialized source-bound
cylinders. Additional class-29 tables without materialized source-bound
cylinders do not participate. Tables whose dimensions do not match the
available patch spans do not participate, and all participating tables supply
one equal diameter and depth tuple. Placement distance does not participate in
this envelope binding.

When neither cylinder source group supplies complete terminal corner envelopes,
every complete four-row or five-row class-911 table must have this exact
dimension signature and must supply one equal diameter and depth tuple. A
different complete four-row or five-row layout prevents this unbound replay
selection. This fallback applies only after the paired generated-surface recipe
identifies counterbore form.

The same terminal envelopes supply directed placement when each assigned
source has its diameter on exactly two axes through common or adjacent-union
spans. The remaining axis has one common nonzero interval in each source. The
counterbore interval length equals the counterbore depth. Both sources have the
same axial axis and equal radial centers, and their axial intervals are exactly
adjacent. The outer counterbore bound is the entry position. The direction is
from that bound through the counterbore interval into the bore interval. The
union of the two axial intervals is the full blind extent. This envelope form
does not identify a placement face.

When both source groups provide complete terminal corner envelopes, the unique
dimension and placement join also constructs the four source cylinder carriers
when no source geometry has already been admitted for those rows. The source
group assigned to the bore receives half the bore diameter as its radius. The
other group receives half the counterbore diameter. Every carrier uses the
counterbore entry point as its origin and the directed envelope axis as its
axis. Its reference direction is the positive model-coordinate direction of
the first radial axis in the validated envelope layout. A partial, conflicting,
or ambiguous source observation does not use this construction.

A counterbore-form hole with a complete bound dimensional tuple has a resolved
counterbore entry; otherwise the identified counterbore form remains
unresolved. The two source-entity cylinder pairs are coaxial. The pair whose
materialized carrier radius equals the dimension-assigned counterbore radius
uses the counterbore cylinder; the other pair uses the same origin, axis, and
reference direction with the dimension-assigned bore radius. When both patches
of the counterbore cylinder have the same complete carrier, that carrier
supplies the hole's unoriented axis placement. This carrier derivation does not
assign an axial trim, entry position, or hole direction.
When both patches of each cylinder pair have one type-0 circular
boundary on the same axis-normal plane, the two equal counterbore-radius circles
define the counterbore entry and the two equal bore-radius circles define the
bore exit. The counterbore circle center is the entry position. The normalized
vector from the counterbore center to the bore center is the hole direction,
and the center distance is the full blind span. The circles must have their
dimension-assigned radii, their axes must be parallel to the span, and the
counterbore depth must not exceed the full span.

A cylinder patch may end with two scalar coordinate pairs separated by
`00 0c 98`, followed by orientation scalar `-1`. The pairs are opposite
corners of an axis-normal rectangle. Two cylinder rows from the same feature
that each meet the same plane through a type-0 topology edge define one
carrier when their rectangles share one complete span, meet exactly on the
other span, and their union is a nonzero square. The plane normal is the
cylinder axis, the plane origin fixes its axial coordinate, the square
midpoint fixes its radial center, and half the square span is the radius. The
two rows are complementary patches of that carrier.

A compact class-911 simple-hole table has class `29`. Its exact four-entry form
has class-204 and class-203 topology entries, a rowless class-200 entry whose
source section entity is zero, and a materialized class-200 entry with no source
section entity. The class-200 entries are the bottom and hole side,
respectively. The side uniquely names an owned cylinder row. The topology pair
is either wholly rowless or has exactly one entry that names an owned plane row.
An extended form has the same adjacent class-204, class-203 topology pair and
can retain other non-materialized regeneration states. Exactly one topology
pair in the extended form contains one owned plane row and one rowless entry.
The cylinder and optional plane are the only materialized surfaces, and the
bottom and side are the unique class-200 entries with their respective source
states. In both forms, the ordered table identifiers equal the entry
identifiers. This structure establishes the simple cylindrical form
independently of whether the cylinder parameters are evaluable. A complete
positional cylinder frame supplies the stored hole-axis position, axis, blind
length, and diameter. The table does not identify a placement face.

A class-917 circular section sweep uses the same four-entry order: first cap
plane, second cap plane, first cylinder use, and second cylinder use. The cap
planes are distinct and parallel. A complete cap outline whose two in-plane
spans are equal and nonzero is the circle's axis-normal bounding square. Its
midpoint lies on the cylinder axis and half either span is the radius. When both
cap outlines are complete, their radial midpoints and radii agree. One complete
cap outline is sufficient because the second placed cap plane fixes the sweep
direction and axial span independently. Both cylinder uses share this carrier.
The two-cap table has entry classes `204, 203, 200, 200`; the two cap entries
and the source-less cylinder entry are materialized table surfaces, while the
source-bearing profile entry is non-surface. Each cap plane has one
unambiguous placed equation from its outline or positional frame; conflicting
equations do not establish the cap. The materialized cylinder entry is the
single neutral carrier for both generated cylinder uses.
The owning feature definition selects the emitted section sketch when that
sketch has a resolved profile chain and otherwise retains the native circular
profile reference. When the feature definition is absent or does not match the
table's section identifier, the profile remains unresolved without discarding
the independently defined direction and blind extent. The ordered cap planes
define the neutral extrusion direction and blind extent. A
`Protrusion` has join semantics when an earlier modeling feature establishes a
body and new-body semantics when its evaluated topology forms an independent
body.

A blind class-917 circular section sweep instead has four entries with classes
`204, 203, 200, 200`. The first two entries are the cap uses. Exactly one cap
use is rowless and exactly one is materialized; the materialized cap is a plane
and the rowless cap is its non-surface counterpart. The class order does not
select which cap is materialized. The third entry is the source-profile entity
and the fourth is one cylinder use. The source-profile entry carries its
section entity identifier; the cylinder entry does not. The materialized cap
plane's complete square outline fixes the cylinder axis, radial center, and
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

A class-923 feature with exactly one owned plane row defines that datum plane
when the row's neutral carrier has a resolved model-space origin, normal, and
in-plane reference direction. Multiple owned plane rows leave the datum
unresolved even when only one carrier is currently transferable.
When both the placed neutral carrier and the unique transferred model-space
plane carrier exist for that row, their plane equations must agree. A duplicate
transferred carrier, a transferred non-plane carrier, or a disagreement leaves
the datum unresolved.
A class-923 feature with no owned plane row instead uses its uniquely owned
definition's unique complete `local_sys` when the stored local x and z axes are
nonzero and perpendicular. The local z axis is the datum normal, the local x
axis is its in-plane reference direction, and the stored origin is the datum
origin. Incomplete sibling `local_sys` fields do not compete with the complete
frame.

For a linear section sweep, generated plane carriers parallel to the section normal bound the sweep axially. Their signed offsets are measured from the section origin along the section normal. The extreme nonzero offset on one side defines a blind extrusion from offset zero to that offset; its sign determines the sweep direction. Extreme offsets on opposite sides define a two-sided extrusion. Equal magnitudes select the symmetric form with total length equal to the sum of the magnitudes. Interior axis-normal planes do not shorten the sweep. When no complete section transform or ordered cap equations are available, a rectilinear plane-family carrier chart uses the uniquely owned `gsec3d_ptr` record with a resolved sketch-plane identifier. Its start-cap `orient` reversal is the parity of set `plane_flip` and set section `flip`; each field independently negates the sketch normal. `flip_flag` and the feature Boolean operation do not select cap polarity. Without that section flag witness, the direction and extent remain unresolved. The section-definition identifier is the profile reference; it denotes a neutral sketch profile only when the sketch contains a resolved profile chain. The first resolved section sweep in feature-definition order forms the base body. Feature-definition order is increasing absolute byte offset of the bounded definition record, not current operation-state order. A material feature enters this resolved sweep order only when exactly one section transform joins exactly one bounded definition; a duplicate or missing join cannot select the base body. A later sweep requires its Boolean operation before it can be committed as an independent body. A section-sweep definition is solid when its evaluated closed-profile topology produces a solid body. An absent evaluated body does not define a nonsolid sweep.
One or more uniquely decoded positional cylinder frames joined from section arcs
to same-feature generated cylinder rows independently define a blind extent when
their axes, positive lengths, and section-plane origins agree. Generated arc
rows without a decoded frame do not compete with those direct witnesses.
Duplicate parameter rows for any joined cylinder reject the construction.
A class-916 or class-917 section sweep with one complete section transform and
parallel generated cap-plane equations is a linear extrusion even when its
current feature-state record omits the recipe discriminator. A stored
rotational recipe excludes this classification.
In a table-class `29` linear section sweep, a source-less class-`204` entry is
the start cap and a source-less class-`203` entry is the end cap when each is
unique and every remaining entry is class `200` with a populated source section
entity. Both cap identifiers must be materialized by the table and resolve to
unique same-feature plane rows. Parallel placed cap equations define a blind
extent: their absolute normal separation is the length, and their ordered
signed separation is the direction.
Without complete placement, ordered cap equations, or the rectilinear section
flag witness, the same non-rotational class
remains a linear extrusion with unresolved direction and extent. Its uniquely
owned section definition still supplies the native profile reference. That
reference resolves to the neutral sketch when the sketch contains a resolved
profile chain; competing definitions leave the profile unresolved.
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

Evaluating one closed line/arc/interpolation-spline profile through a full turn
produces one face per oriented profile entity. A profile vertex off the
revolution axis produces
one closed circular edge with one seam vertex; the preceding and following
faces form its two radial uses. A profile vertex on the axis collapses and
produces no edge. Each face has one singleton loop for each off-axis endpoint.
Planar, cylindrical, conical, spherical, and toroidal faces use their analytic
parameterizations. Boundary pcurves traverse one full azimuth at constant
axial, polar, or tube parameter; a planar boundary is an exact rational
quadratic circle. A spindle-torus boundary retains the signed ring branch, so
a negative ring shifts azimuth by π instead of reflecting the trim. Face sense
is the analytic carrier normal aligned to the outward side of the oriented
section profile. An interpolation-spline profile entity produces a NURBS side
face. Its directrix direction retains the oriented spline degree, intrinsic
knot domain, poles, weights, and periodic flag; its angular direction is the
exact full-turn quadratic construction defined above. Each spline endpoint
boundary is a constant-`u` pcurve from `v = 0` through `v = 2π`, with `u`
equal to that endpoint's intrinsic spline parameter. Its face sense uses the
evaluated directrix tangent and the oriented profile area.

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
Mapping a linear pcurve through a planar face chart defines an exact model-space
line carrier. A linear pcurve with constant `u` through a cylindrical or
conical face chart defines an exact generator line. Every positional and
labeled path for that curve which maps through a placed face chart must produce
the same ordered model-space endpoint pair and the same analytic carrier.
A constant-`v` cylindrical path defines a circle. A constant-`v` conical path
defines a circle for equal radial scales and an ellipse for unequal radial
scales. Constant-`u` spherical paths define meridian circles and constant-`v`
paths define latitude circles. Constant-`u` toroidal paths define tube circles
and constant-`v` paths define ring circles; a negative ring radius reverses the
reference direction. If any evaluable adjacent face path is not one of these
analytic forms, the pcurve does not define an analytic model-space carrier.
Mapping endpoint A and endpoint B through every evaluable adjacent face chart
must produce the same ordered model-space pair. For one topological vertex
orbit, the common point among the unordered mapped endpoint pairs of at least
two incident curves is its model-space point when exactly one point remains.
A unique orbit point selects the opposite endpoint of every incident
pcurve-backed edge and propagates through the connected endpoint component.
A candidate point must also lie on every independently placed analytic curve
carrier incident to that vertex orbit.
A pair of nonparallel incident model-space line carriers also defines a vertex
candidate when their closest points coincide. Every intersecting line pair in
the orbit must produce that same point, and the point must lie on every other
incident analytic carrier.
An incident line and analytic conic contribute their finite model-space
intersection set. A tangent contributes one candidate and a secant contributes
two. Two analytic conics in transverse planes contribute the candidates on
their common plane-intersection line. Two coplanar analytic conics contribute
their common real roots, up to four candidates. Coincident conics do not define
a finite domain. The orbit transfers only when the incident-carrier and
mapped-pcurve constraints reduce every candidate domain to one agreeing point.
A carrier-derived point for the same orbit must agree with that point. An
empty endpoint domain withholds every dependent point in the component.
An edge transfers independently when both endpoint vertex orbits are solved;
face and loop transfer still requires every edge of the complete boundary.

An exact non-periodic NURBS curve produced by a complete tabulated-extrusion
boundary or shared-generator join supplies an unordered endpoint pair by
evaluating its intrinsic parameter-domain limits. Those points constrain the
two endpoint orbits of its topology edge. Periodic NURBS and NURBS carriers
from other constructions supply no endpoint pair through this rule.

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

Evaluating one closed linear-sweep profile produces one side face per oriented profile entity. A line produces a planar side face, an arc produces a cylindrical side face, and an interpolation spline produces a ruled NURBS side face. Each profile vertex produces an edge parallel to the sweep direction. The exact signed area is the sum of line chord terms and circular-arc sector terms; a NURBS profile contribution is the signed line integral of its evaluated carrier over every nonzero knot span. Its sign selects the cap and side face senses. The two cap loops use the profile edges in opposite directions, and every cap or longitudinal edge has exactly two face uses. Cap-face pcurves are the section entities in the cap plane's `(u,v)` frame: lines remain lines, arcs become exact rational quadratic arcs, and interpolation splines retain their degree, intrinsic knot vector, control points, weights, and orientation. A planar side face uses profile distance and sweep offset as its parameters. A cylindrical side face uses profile angle and sweep offset. A ruled NURBS side face uses the profile's intrinsic parameter as `u` and normalized translation fraction `[0,1]` as `v`; its `u` degree, knot vector, control points, weights, and periodic flag are the profile's, its `v` degree is one with knot vector `[0,0,1,1]`, and each directrix pole is duplicated at the lower and upper cap translations with its weight duplicated. Its cap-edge pcurves hold the sweep offset constant and its longitudinal-edge pcurves hold the profile parameter constant. A multi-profile solid sweep has one outer profile that strictly contains every hole profile. Hole profiles are pairwise disjoint, unnested, and oriented opposite the outer profile.

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

A feature owns each mixed generated-entity table bounded by its `AllFeatur` row. The array's compact-integer count is not limited to a one-byte or 64-entry range. A positional entry has a canonical entity-reference identifier, a compact entry class, and a positional body. The first counted entry can instead be a prototype whose `f7 <entry-class>` prefix supplies the class omitted after its identifier. A class `200` entry carries its source section entity's external identifier immediately after the class when that lane is populated; a structural marker in that position leaves the source absent. Classes `210`, `219`, and `2017` carry a related canonical entity identifier immediately after the class. Class `214` has a related-entity form with the same lane; its other positional bodies do not define that relationship. Classes `210`, related-form `214`, and `219` follow the related identifier with state byte `00`; class `2017` follows it with state byte `00` or `01`. These related identifiers occupy a separate namespace from the class-200 source section entity. An entry normally closes with `e3`; a final related-form entry can terminate at the following `f2 f7` table separator after its state byte. An `e3` byte inside a canonical two-byte typed integer is not a record close. A table surface identifier denotes geometry generated or modified by that feature. When that surface is the carrier of a connected face, the face's owning body is an output of the feature. A complete class-210 surface transition resolves through one related-form class-214 entry in the same table: the class-210 related identifier names the class-214 non-surface entity, and the class-214 related identifier names the preceding materialized surface. The class-210 entity names the resulting materialized surface. Every transition in one complete table has distinct preceding, intermediate, and resulting identifiers. For a thicken operation, each placed planar source/result pair is parallel, and the common nonzero absolute normal separation is the operation thickness.
The source row's reversal flag orients its plane normal. When every placed
planar transition has the same signed separation along that oriented normal,
positive separation selects the forward side and negative separation selects
the reverse side. A result row has the opposite reversal state from its source.
Mixed signed separations do not define a one-sided thicken operation.
Each preceding surface identifier in a complete thicken transition names a
face in the regenerated result of the feature identified by that surface
row's `feat_id`. The ordered source roster is the thicken face selection.

In a mixed generated-entity table whose leading run has entry class `254`,
that run is the ordered visible-surface sequence. Entry-class `214` rows after
the visible run are nonvisible replay surfaces. A contiguous class-214 window
is one replay of the visible sequence only when it has the same length, every
identifier resolves uniquely in `NovisGeom`, every visible identifier resolves
uniquely in `VisibGeom`, all rows belong to the table's owning feature, and the
surface families agree position by position. Nonmatching class-214 entries
between complete windows are independent construction surfaces.

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

For each feature, every materialized surface identifier in its owned
generated-entity tables declares a regenerated-result face identity when that
identifier has exactly one surface row and the row's `feat_id` is the owning
feature. This includes materialized cap and transition entries as well as
class-`200` entries. The result state names that face as
`surface#<geom_id>`. Duplicate identifiers, missing rows, and rows owned by
another feature invalidate the feature's complete face-result state. A
generated face selection is valid only when its producer feature and
`surface#<geom_id>` identity occur in that producer's result state.

A regenerated-result edge identity is declared for each unique `crv_array`
topology row whose `feat_id` is the producing feature. The topology row is
already a complete materialized edge record: its two face-side identifiers and
two successor-edge identifiers are bounded by the row grammar. If the same
curve identifier occurs more than once in the complete curve namespace, that
feature's edge roster is unresolved. The result state names a declared edge as
`curve#<crv_id>`. An affected edge that is absent from the current B-rep binds
to this result identity only when its unique topology row and producing
feature result state both declare `curve#<crv_id>`.

`edg_id_tab_ptr`, `lo_id_tab_ptr`, `bnd_type`, `used_bodies`, `geom_lists`,
and `dtm_id_tab` declare feature-owned geometry tables. Each table retains its
declared compact count and the entity-class identifier following its `f7`
marker. The label selects the edge, loop, boundary, body, geometry-list, or
datum identifier namespace independently of that class identifier.

A named `lo_id_tab_ptr` table can be followed in the same feature row by
`e0 01 lo_hist 00 f8 06`. The value `6` is the stored loop-history record
width. Exactly the table's declared count of loop-history records follows.
Each record begins with the feature-local loop identifier and four
self-delimiting PSB fields. Its sixth slot is the terminator `e3` or
`f1|f2 f7 <reference> e3`. The final record can instead end directly at the
following named-record header or contain one additional self-delimiting field
before that header. Record order is the loop roster order. An incomplete field,
early terminator, or nonfinal header boundary defines no loop roster.

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

`ActDatums` stores datum-plane geometry as `act_datum_geoms → srf_array` records. Each section includes one named datum row and can include positional `<gid> 22 ...` rows. For positional datum rows, `outline` stores two diagonal corners. Exactly one corresponding coordinate pair must compare equal; that pair supplies the positive plane normal and equation `x_k = p0[k]`. Zero or multiple equal pairs leave the positional plane unresolved. Datum names do not define their geometric orientation.

Within the counted `ActDatums` `srf_array` frame, a positional datum row uses
the complete compact-integer `geom_id` and `feat_id` fields, `geom_type = 22`,
an orientation byte, `boundary_type = 01`, and `next_geom_ptr = 0`. The frame
count bounds the rows; bytes outside the frame do not start datum rows. The
row body contains four environment scalar slots followed by the six outline
corner slots and ends before the next validated row or the containing frame
boundary.

The datum surface row's `feat_id` is the owning modeling feature identifier.
The row's `geom_id` remains the separate datum-geometry identifier used by
`gsec3d` plane references.

Plane lookup keeps the datum-geometry and model-surface namespaces separate. A
reference is eligible only when one namespace supplies one complete plane
equation. If one numeric identifier has both a unique complete `ActDatums`
datum plane and a unique complete model-space plane carrier, the reference is
ambiguous; neither namespace takes precedence and the section placement remains
unresolved. A unique complete candidate in one namespace remains eligible.

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
section-local dictionary of distinct `46` tokens. The six slot encodings are
contiguous and consume the bounded field body. A complete outline with
exactly one equal coordinate pair defines the corresponding axis-aligned plane
and offset.

Named and positional datum outlines use the same bounded model-coordinate
lane. In this lane, `73 <tail6>` reconstructs
`[3f e8 <tail6>]`, `bb <tail6>` reconstructs `[bf e8 <tail6>]`,
`a5 <tail6>` reconstructs `[bf d0 <tail6>]`, and `9f <tail6>` reconstructs
`[40 14 <tail6>]`.

In a named datum outline, exactly one pair of standalone-zero slots at
positions `k` and `k+3` identifies coordinate axis `k` and plane offset zero.
Zero pairs or multiple pairs do not define a plane equation.
Datum-outline nonzero coordinates use the bounded model-coordinate DICT lane. `5e..a3`
set the two-byte IEEE prefix to `0x3f75 + prefix`; `a4..a6`,
`a7..b1`, `b2..cf`, `d0..dc`, `dd`, and `de..df` set it to
`0xbf2b + prefix`, `0xbf2c + prefix`, `0xbf2d + prefix`, `0xbf2e + prefix`,
`0xbf2f + prefix`, and `0xbf32 + prefix`, respectively. Each prefix is
followed by `tail6`.
The `28` and `41` forms reconstruct `[3f, tail7]`; the fixed `2c`, `4c..4d`,
`50`, and `54` forms reconstruct
`[3f, tail6, 00]`. The `46` and `2d` forms retain their eight-byte
`[40, tail7]` and `[c0, tail7]` forms. Each complete token consumes one
coordinate slot; a missing or incomplete token leaves the outline unresolved.
The `41` scalar form therefore occupies eight bytes: the prefix followed by
seven payload bytes. The seven-byte `45` and `5c` tokens consume one slot but
retain an unresolved numeric value.

`ref_planes` stores an outer reference followed by a nested `plane_id`. The nested identifier is the geometric datum identifier and joins `ActDatums.srf_array.geom_id`. A referenced datum normal orients a sketch in-plane axis only when it is perpendicular to the sketch-plane normal.

### 8.2 Section topology

DEPDB stores a section directly below `gsec2d_ptr` when it is not nested in a
`feat_defs_<id>` record. Its `name` value `S2D<N>` supplies the section
identifier. When the namespace contains one section and one procedural-recipe
record, the recipe record's feature identifier owns the section. The section
retains the same `segtab_ptr`, `dimtab_ptr`, `relat_ptr`, `var_arr`,
`gsec3d_ptr`, and `p_saved_result` grammars as a nested feature definition.

A named `gsec3d_ptr` placement span begins at its record header and ends at the
first following `p_saved_result` record. If that close is absent, the span ends
at the next `gsec3d_ptr` header or at the enclosing definition boundary. Named
fields outside this span do not belong to the record. The placement's plain
`plane_id` field supplies the sketch-plane entity; an `e0 01 plane_id` field
inside `ref_planes` supplies the nested datum-geometry identifier.

Positional `segtab_ptr` replay ends at the first following section-table label,
including `dimtab_ptr`, `relat_ptr`, `var_arr`, `gsec3d_ptr`, `order_ptr`, or
`p_saved_result`, or at the next sibling `S2D<N>` record. Bytes in later tables
or sibling section records are not segment rows.

In an instantiated positional definition, the `S2D<N>` name terminator is
followed immediately by the unlabeled `segtab_ptr` array body. Its `f8` extent
bounds the section-entry table. Its first declared entry is the inherited
prototype closed by `f2 f7 <table-class> e2`; subsequent entries are replay
rows. Decoded line, arc, and point rows are the entries with segment type `2`,
`3`, and `5`. A positional replay row body begins with its optional `c1 00`
type wrapper or its segment-type field and includes the terminal `e2` row
close. Type `12` is a bounded section curve. Both point fields are
non-null endpoint references and define its start and end loci, but the type
does not by itself define an analytic carrier. Its direction, center,
arc-orientation, vertical/horizontal, and radius fields retain their stored
state. A type-12 row with a null endpoint remains opaque. Type `25` is a
section-reference line. Its two point fields are
nullable endpoint references; its center, radius, and secondary-radius fields
are null, and its arc-orientation field is zero. Its direction triple and
vertical/horizontal field retain the stored reference-line state. When both
point references resolve to distinct section coordinates, they are the ordered
endpoints of the reference-line carrier. A type-25 row participates as a line
in unary horizontal or vertical incidences and in perpendicular or parallel
line components. When its stored vertical/horizontal selector or one consistent
incidence component uniquely fixes a coordinate and both referenced endpoint
ordinates for that coordinate are present and agree, the row defines an
unbounded axis-parallel reference line. The other endpoint ordinates do not
define a finite extent. Other complete fixed-field segment families remain
opaque segment rows. The entity-reference header and segment rows use the same
framing and field order as the labelled `segtab_ptr` table.

The positional dimension table repeats the labelled template's `dimtab_ptr`
table-class reference in an unlabeled `f8 <count> f7 <table-class> fb e2`
header. The following entity reference selects the dimension-row class. The
first row follows that reference; later rows follow
`f3 f7 <table-class> e2`. All rows use the labelled dimension field order. A
table with at least two rows is self-identifying without a decoded labelled
template when the declared count is complete, every row has a defined linear
or angular dimension type, and exactly one array in the positional definition
satisfies this grammar. A one-row array does not establish its table family.

The positional variable table repeats the labelled template's `var_arr`
table-class reference in the same unlabeled array header and then stores its
variable-row class reference. The first row ends with
`f1 f7 <table-class> e2`; later rows are separated by `e2`. Its `f8` extent is
the number of variable rows. Each row replays `type`, `key`, `value`, `guess`,
`known`, `homogeneity`, and `uvar_id` in that order. A bare `18` in the
`guess` slot is exact zero when exactly three compact solver-state fields
follow before the row separator or next table boundary.

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
An incidence row can store an auxiliary frame between `status` and the nested
item array. The first such frame uses the labelled `aux` counted form; later
rows replay the auxiliary body positionally. The nested item array retains the
same item-table and item-row classes across these rows. A repeated item-table
class reference can immediately precede the array opener. The auxiliary frame
does not replace or reorder `id`, `type`, `flags`, `status`, or the incidence
items. Exactly one matching nested item array must occur before the incidence
row separator. A second matching array makes the positional row invalid; no
selector chooses one array. The final row may terminate at the end of the bounded
definition, at a named record, at a structurally complete following positional
table header `f8 <count> f7 <class> fb e2 f7 <row-class>`, or at a positional
table wrapper `f4 04|05 f7 <class>` followed by a complete array header `f8
<count> f7 <class> fb e2` or the next structural body marker. A following
table header with the nested item-table class is not a boundary; it remains an
ambiguous nested item array.

The positional relation-join table repeats the labelled `triples_ptr` table
class and stores exactly its `f8` count of `rel_id`, `eqn_id`, and `skamp_id`
triples. Each field independently uses `f6` for null.
`f1 f7 <table-class> e2` separates the prototype from the following triples;
bare `e2` separates later triples.
A positional feature definition may instead retain labelled `skamp_ptr` or
`triples_ptr` tables alongside its positional `relat_ptr` table. Each labelled
subtable uses its own named grammar and takes precedence over replay through
the preceding template class.

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
the section axis. The selected reference row supplies its own `ref_type`,
`seg_id`, and `flip_flag`; fields from another row do not orient the section.
A positional reference-plane selection requires exactly one row with the
selected `plane_id`; a duplicate or missing row leaves the section placement
unresolved.

A linear section frame is also complete when at least two distinct solved arc
centers bind through same-feature class-200 entries to complete positional
cylinders. The cylinders have one directed axis, each cylinder origin is the
model-space image of its source arc center, and every pair preserves the
section-space center distance. The directed cylinder axis is the section
normal. The unique right-handed rigid map from all center correspondences
defines the section origin and axes. Coincident centers, nonparallel or
oppositely directed cylinder axes, distance disagreement, or more than one
rigid map leaves the frame unresolved.

`order_table` entries are `ext_id`, `int_id`, and orientation-flag tuples. `ext_id` references a section entity and `int_id` is the section's internal ordering index. The declared count includes one structural prototype followed by exactly `count - 1` stored rows. Named tables encode the prototype as named `ext_id`, `int_id`, and `bitmask` fields; positional tables encode the same three fields positionally. An incomplete table retains its complete row prefix but establishes no semantic joins. A semantic join requires exactly one row for the selected `ext_id` and exactly one row for the selected `int_id`; duplicate keys do not select a first row. A class-200 feature-generated-table entry stores the same `ext_id` as its source identifier and stores the generated surface identifier as its leading entity identifier. This explicit equality joins line, arc, and spline section entities to their generated carriers; table position and family order do not define the join.

A saved entity with a unique internal identifier takes the corresponding unique
`order_table.ext_id` as its section-entity identity even when no `segtab` row
has that external identifier. More than one saved entity with the internal
identifier, or more than one `segtab` row with the external identifier, makes
the join ambiguous.
For a joined line, saved `end1` and `end2` are the section coordinates of the
first and second `segtab.pointid` fields. For a joined arc, saved `end1` and
`end2` likewise supply the first and second `segtab.pointid` coordinates;
`arcorient = 0` renders the second point as the angular start and the first
point as the angular end. The saved arc center supplies the coordinate of its
`segtab.cntrid`. Complete saved endpoints and centers participate in the
section coordinate equations and must agree with stored and constraint-derived
values. A joined saved circle supplies its center to the type-10
`segtab.cntrid` coordinate and its radius to that row's radius-reference
variable. Equal-radius solver incidences propagate that saved radius only when
all values in the connected radius component agree.
When both `var_arr` and the joined saved entity define complete line or arc
geometry, their ordered endpoints and carrier equations must agree. Conflicting
complete forms leave the section entity unresolved.

For a linear section sweep with a resolved model-space section frame, a complete
saved line joined through this chain generates a plane parallel to the sweep
direction, and a complete saved arc or circle generates a cylinder whose axis
is the sweep direction. The generated surface row must belong to the sweep
feature and have the matching plane or cylinder family.

Source-bound positional cylinders also define a blind linear-sweep extent when
every such cylinder owned by the feature starts in the resolved section plane,
has the same directed axis parallel to the section normal, and stores the same
positive finite length. The stored cylinder length takes precedence over
unbound same-feature plane offsets. A missing length or disagreement in start
plane, direction, or length leaves the extent unresolved.

The complete set of same-feature generated planes and positional cylinders
defines a blind linear sweep when every cylinder row transfers uniquely, has
one complete positional frame, and agrees on its directed axis, start station,
and positive finite length. The transferred cylinder carrier must agree with
its positional frame. A section transform, when present, must place every
cylinder start in the section plane and have a normal parallel to the cylinder
axis. A transferred generated plane normal to the axis is a cap and must pass
through a cylinder end; two distinct transferred caps must span the common
cylinder length. A transferred generated plane parallel to the axis is a side
carrier. Oblique transferred planes reject the construction. Generated plane
rows need not transfer because the bounded cylinder frames define both axial
ends independently. The directed cylinder axis is the extrusion direction and
the common length is the blind extent. Duplicate rows, duplicate cylinder
parameter records, other generated surface families, missing cylinder
transfers, or inconsistent carriers leave direction and extent unresolved.
For a class-29 cap table, each source-less cap identifier must resolve to one
placed or transferred plane carrier. When both carriers exist, their plane
equations must agree; a missing, duplicate, non-plane, or conflicting carrier
leaves the cap extent unresolved.

A transferred same-feature linear-extrusion surface also defines a bounded
carrier when exactly one NURBS parameter direction is nonperiodic degree one
with two poles and a clamped four-knot vector. Corresponding poles and rational
weights across that direction pair, and every pole pair has the same nonzero
finite displacement. The poles at the lower parameter bound lie in one plane
normal to the displacement. Every transferred linear-extrusion row must satisfy
this form and all such rows must agree on their directed displacement and start
plane. A section transform and transferred generated planes satisfy the same
start-plane, direction, side-carrier, and cap conditions as bounded cylinders.
Untransferred generated rows do not compete with the complete carriers. If both
NURBS parameter directions satisfy the linear two-pole form, the sweep
direction is ambiguous. The common pole displacement is the extrusion vector;
its magnitude is the blind extent.

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
The saved line body begins at the row's first reference, attribute, or entity
identifier and ends after its final owned token. The following `e3` row
separator or named-record opener is not part of the body. The body preserves
the stored scalar and compact coordinate encodings independently of their
decoded endpoint values.
Named saved arcs and circles retain their identity and each decoded scalar
field when later center, radius, endpoint, or parameter fields are absent.
Positional saved arcs retain their uniquely joined identity and ordered
12-slot scalar prefix at a structural row boundary.
Named saved circle, conic, and dummy bodies begin after their entity labels and
end before the following entity label. A named saved arc body ends before its
first positional replay-row separator or the following entity label, whichever
comes first. Positional saved arc bodies begin at the row identifier and end
before the row-closing `e3`. These bodies preserve the exact stored fields
independently of decoded geometry.
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
The retained `i_pnts` value body begins at `f9` and ends after the last complete
point triple. A complete `end_tangts` value body retains its `f9 02 03` wrapper
and six tangent scalars. A complete `params` value body retains its `f8
<count>` wrapper and all parameter scalars. Incomplete tangent and parameter
fields have no decoded value body.
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

When both `segtab` and `order_table` declare an elided prototype, the saved
record at the saved-result table origin is that prototype if a later saved row
has the same internal identifier. The prototype does not participate in the
entity join. A table-origin row without a later same-identifier instance
remains an ordinary saved entity.

When a unique decoded `segtab` row and a unique `order_table` join bind a
complete saved line, arc, circle, or spline to an opaque segment family, the
saved entity supplies the standalone neutral geometry for that external sketch
entity. The opaque row retains the entity's solver identity and does not replace
the complete saved geometry. A complete `segtab` table and a compatible
same-feature generated surface binding make that entity profile geometry.
A solved type-10 `segtab` circle with a unique external identity is likewise a
closed one-entity profile when a same-feature generated cylinder binds that
identity. Without that generated-cylinder binding, it remains construction
geometry.

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

A named `entity(conic)` record with `type = 58` stores `end1(3)`, `end2(3)`,
`t0`, `t1`, `c1`, `c2`, and `local_sys` fields. `c1` and `c2` are the two
positive ellipse radii. `local_sys` is an `f9 4 3` frame: its first two rows
are the ordered in-plane unit axes, its compact third row is `(0, 0, 1)`, and
its fourth row is the section-space center. The frame must be finite,
right-handed, and orthonormal. The larger coefficient selects the major axis;
when `c2 > c1`, the second frame row is the major axis and the parameter
origin shifts by negative one quarter-turn. Equal complete `end1` and `end2`
triples with `t0 = 0` and an omitted `t1` denote a full ellipse. Otherwise,
finite increasing `t0` and `t1` delimit an elliptical arc.

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

A type-58 `segtab` row identifies a saved conic. Its direction triple is
zero, its point fields are absent and one, its center field is present,
`arcorient = 0`, and `verhor = 2`. The two radius fields are references to
the saved conic's first and second coefficients, not a circle radius and a
secondary radius. `ext_id` joins the row through `order_table` to the
type-58 `entity(conic)` record.

### 8.3 DEPDB profiles and operations

A `point` record stores a first section coordinate as an IEEE-fill scalar, a point identifier, and a second coordinate as an `18 <index>` reference into the record-local `0x46` cache.

`i_pnts f9 <n> 03`, `end_tangts f9 02 03`, and `params f8 <n>` encode an interpolation-point spline with endpoint tangent angles and parameter values.
When its saved entity identifier joins `order_table.int_id`, the corresponding
`order_table.ext_id` is the spline's section-entity identity. A generated
class-200 entry with that source identifier binds the spline into the owning
sweep profile and to its generated spline surface. Clamped spline profile
connectivity uses the first and last evaluated control points.

A curve-from-equation entity stores `expression f8 <count>` followed by exactly `count` NUL-terminated UTF-8 source lines. `entity(crv_fr_eqn)` is the active equation record and `backup_ents(crv_fr_eqn)` is its separately identified backup record. Source-line order is significant. Lines beginning with `/*` are comments. Executable lines use `identifier = expression`; identifiers referenced on the right-hand side are expression dependencies. Identifier binding is ASCII case-insensitive while source spelling is retained. A dependency symbol may carry one or more colon-delimited alphanumeric or underscore scope segments; the complete scoped symbol is one dependency. Numeric literals, quoted UTF-8 string literals, previously assigned identifiers, the reserved immutable geometric constant `PI`, and parentheses form expressions. Literal contents are not dependencies. `PI` has the value π and is not a dependency. Operator precedence from highest to lowest is grouping and function calls, right-associative exponentiation `^`, unary `+`, `-`, `!`, and `~`, multiplication and division, addition and subtraction, one comparison, logical AND `&`, and logical OR `|`. Numeric `+` adds; string `+` concatenates. Comparisons are `==`, `>`, `>=`, `<`, `<=`, and the equivalent not-equal forms `!=`, `<>`, and `~=`. Strings admit equality and inequality comparisons. Comparisons and logical operators return numeric one or zero; zero is false and every nonzero scalar is true. Function names followed by an argument list are operators rather than dependencies. The scalar function set is `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `sinh`, `cosh`, `tanh`, `sign`, `mod`, `if`, `bound`, `dead`, `near`, `min`, `max`, `log`, `ln`, `exp`, `pow`, `sqrt`, `abs`, `ceil`, `floor`, and `dbl_in_tol`. Deterministic string functions are `itos`, `rtos`, `search`, `extract`, `string_length`, `string_starts`, `string_ends`, `string_match`, and `string_pattern`; function names are case-insensitive. `itos` rounds its numeric argument to an integer. `rtos(real)` uses the real's decimal representation, `rtos(real, decimals)` emits the requested fixed number of decimal places, and a true third argument selects scientific notation. Scientific exponents contain at least two digits and omit a leading plus sign. Both conversion functions return an empty string for zero. `search` returns the one-based character position of the first substring occurrence or zero. `extract` uses a one-based character position and character count. `string_match` tests exact whole-string equality. `string_pattern` applies its regular-expression pattern to the complete string; an invalid or resource-exceeding pattern remains unresolved. `rel_model_name()` returns the current model name from the unique counted part filename; `rel_model_type()` returns `part` for a part document. `exists(string)` returns true when the case-insensitive string names an identifier declared by any assignment in the same complete program, independent of source order or conditional activation, or a decoded section dimension identified by `d<external_id>`. A decoded section dimension initializes `d<external_id>` with its value when every occurrence of that identifier has one equal decoded value. Linear and schema-defined values retain their stored scalar; angular values convert from stored radians to relation degrees. An unresolved or conflicting occurrence leaves the value unresolved while preserving existence. A name absent from those decoded namespaces remains unresolved. Circular trigonometric arguments and inverse-trigonometric results use degrees. `log` is base ten and `ln` is natural logarithm. A right-hand reference to a uniquely assigned program identifier binds to that assignment independent of source order. Evaluate assignments in source order; an assignment remains symbolic when a dependency has not yet acquired a value or an operation is outside its domain. Reassigning an identifier in any letter case replaces its preceding value; an unresolved reassignment leaves the identifier unresolved for following lines.

Tangent is undefined at angles congruent to 90 degrees modulo 180 degrees. `atan2` is undefined when both arguments are zero.
Hyperbolic functions use their real domains; a finite input is evaluated, and a result that is not finite remains unresolved.

`SOLVE` opens a simultaneous-equation block and `FOR` followed by one or more
comma- or whitespace-separated identifiers terminates it and declares the
ordered unknowns. An intervening top-level single-equals statement involving a
declared unknown is an equation with arbitrary expressions on both sides.
Equation dependencies are the identifiers in first-appearance order across the
left side and then the right side. A valid assignment that does not involve a
declared unknown is an auxiliary one-way relation in the solve block. An
auxiliary relation retains ordinary assignment-target semantics, participates
in the program identifier namespace, and evaluates when its dependencies have
values. An equation does not declare a parameter. A complete block retains its
ordered equations, auxiliary relations, unknowns, and aligned solution values.
At the terminating `FOR` line, a declared unknown takes the physical dimension
of its preceding value when that value is defined. An unknown without a
preceding value receives dimensions from unit-qualified operands, dimensioned
reserved constants, known quantities, and dimension-equality constraints imposed
by the equation expressions. Every exponent of every previously untyped
unknown must have one integral solution in the five canonical axes: length,
mass, time, angle, and temperature. An inconsistent or underdetermined
dimension system remains unresolved. A dimensionally valid affine equality is
then evaluated after known dependencies and block-local auxiliary relations are
applied. When the complete affine system has one finite consistent solution,
the solution in canonical relation units replaces the unknown values at `FOR`
and supplies following assignments. Unknowns may have different physical
dimensions; multiplication or division by a known dimensioned value supplies
the corresponding affine coefficient. A nonlinear system requires one
preceding finite numeric value of the resolved physical dimension for each
unknown. Those values initialize the solve. The system uses the same dimension
checks and replaces the unknown values only when its numeric residual equations
have one finite root with a full-rank local Jacobian. Other diagnostic starting
points can reject competing roots but cannot supply the accepted root. A root
is not a solution when any residual is nonnumeric,
dimensionally inconsistent, outside a function domain, or non-finite. Multiple
roots, a rank-deficient root, an underdetermined system, an inconsistent system,
or an unresolved dependency retains absent solution values. Non-smooth,
piecewise, and discrete function forms retain absent solution values unless the
affine reduction above resolves them without a numeric nonlinear solve.
An equality or ordering comparison between two affine forms is independent of
the unknowns when subtraction cancels every unknown coefficient. Such a
comparison has its constant Boolean value during the solve. `if` selects an
affine branch when its condition is constant, and `min` or `max` selects an
affine operand when the difference between its operands is constant. `bound`
and `dead` likewise reduce when the value and both bounds have pairwise
constant differences. `near` and `dbl_in_tol` reduce when their operand
difference and nonnegative tolerance are constant.
Entering the block invalidates any preceding
value of each declared unknown; that value does not supply following
assignments when the block remains unsolved. A malformed block is bounded from
its `SOLVE` line through its `FOR` line, or through the end of the program when
unterminated; no bounded line is interpreted as a sequential assignment.

`itos` and `rtos` accept every non-string scalar in canonical relation units.
The `rtos` decimal count and scientific selector are dimensionless.

The context-dependent function names `cable_len`, `cable_thick`,
`cbl_logical_file`, `eang`, `elen`, `edistk`, `ecoordx`, `ecoordy`,
`evalgraph`, and `trajpar_of_pnt` are operators rather than parameter
dependencies. Their arguments retain ordinary string-literal and parameter
dependency semantics. Their results resolve through the referenced cabling,
case-study, graph, or trajectory namespace.

The context-dependent function names `massprop_param`, `material_param`,
`mp_mass`, `mp_assigned_mass`, `mp_surf_area`, `mp_volume`, `mp_cg_x`,
`mp_cg_y`, and `mp_cg_z` are operators rather than parameter dependencies.
Their arguments retain ordinary string-literal and parameter dependency
semantics. Their results resolve through model mass properties, material
assignments, model paths, and coordinate systems.

On the right side of an assignment, the context-dependent function names
`has_value`, `match_value`, `average`, `value_by_argument`, `weighted_average`,
`value`, and `count_rows` query series or list parameters. They are operators
rather than parameter dependencies. Their arguments retain ordinary parameter
dependency semantics. Unary `min` and `max` likewise query a series or list
parameter; their two-argument forms compare ordinary scalar expressions. Query
results resolve through the referenced table-valued parameter.
On the left side of an assignment, `value(parameter,row)` and
`value(parameter,row,column)` select a cell of a list or series parameter.
`parameter` identifies the table-valued parameter; `row` and the optional
`column` are selector expressions. A table-cell target mutates that cell and
does not declare a scalar parameter named `value` or `parameter`. The table
identifier and selector-expression identifiers precede right-hand identifiers
in the assignment dependency order.

`rel_model_name:<session_id>()` selects the model identified by the decimal
session identifier. The scoped call is a model-context query, not a parameter
reference. Its result resolves through the referenced model namespace.

An assignment target may be a complete colon-scoped dimension or parameter
identifier such as `d7:0` or `width:fid_25:cid_12`. The assignment drives that
scoped item and does not declare an unscoped local parameter. A scoped target
cannot carry a new-parameter unit declaration. Its evaluated value becomes the
current value of the complete scoped identifier for following source lines.
Unscoped system targets use the prefixes `d` for a model dimension, `sd` for a
section dimension, `rd` for a reference dimension, `rsd` for a section
reference dimension, `kd` for a known parent dimension, `ad` for a driven
dimension, `p` for a pattern instance count, and `tpm`, `tp`, or `tm` for a
tolerance component, followed by one or more decimal digits. A system target
drives that system item, does not declare a user parameter, cannot carry a
new-parameter unit declaration, and supplies the complete identifier's current
value to following source lines.
A registered relation function may be a write target as
`function(argument,...) = expression`. The function identifier selects the
registered write callback; each argument is an ordinary expression. Target
argument dependencies precede right-hand dependencies in source order. A
function-write target does not declare a parameter and does not supply a scalar
value to the local relation namespace.

A `crv_fr_eqn` program containing calls to `abs`, `ceil`, `floor`, `extract`, `if`,
`itos`, or `search`, or containing `IF`, `ELSE`, or `ENDIF` control lines, is
not an evaluable datum-curve equation. Its source, assignments, and dependencies
remain native design data, but none of its assignments supplies a value or
derived curve.

`G` is the reserved acceleration 9.8 meters per square second and is not a
dependency.

`min(x,y)` selects `x` only when `x < y`; `max(x,y)` selects `x` only when
`x > y`. Both functions select `y` when the operands are equal.

Square brackets following a numeric literal or parameter expression contain a
unit expression. Identifiers inside the brackets are unit symbols, not relation
dependencies. Length symbols `mm`, `cm`, `m`, `in`, `inch`, `ft`, `foot`, and
`micron` convert to millimeters. `sq_mm`, `sq_cm`, `sq_m`, `sq_in`, and `sq_ft`
are area units; `cu_mm`, `cu_cm`, `cu_m`, `cu_in`, and `cu_ft` are volume units.
Mass symbols `kg`, `g`, `mg`, `lb`, `lbm`, `slug`, and `tonne` convert to
kilograms. Time symbols `s`, `sec`, `second`, `Msec`, `min`, `minute`, `hr`,
`hour`, and `day` convert to seconds. Force symbols `N`, `newton`, `kN`, `dyne`,
`lbf`, and metric ton-force `ton` convert to kilogram-millimeters per square
second. `erg` and `joule` are energy units; `kW` and `MW` are power units;
`Pa`, `MPa`, `GPa`, `psi`, and `ksi` are pressure units. Angle symbols `deg`,
`degree`, `rad`, and `radian`
convert to relation degrees. Temperature symbols `K`, `C`, `F`, and `R`
convert Kelvin, Celsius, Fahrenheit, and Rankine to canonical kelvin values.
Unit symbols are ASCII case-insensitive. Unit multiplication, division,
parentheses, and signed integer powers form compound dimensions. Affine
Celsius and Fahrenheit units cannot form compound units; Kelvin and Rankine
can. Addition,
subtraction, and comparison require equal dimensions. Multiplication and
division add and subtract base-dimension powers. An integer power multiplies
the powers, and `sqrt` divides even powers by two. `abs`, `min`, `max`, `near`,
`dbl_in_tol`, `pow`, `if`, `sign`, `mod`, `bound`, `dead`, `ceil`, and `floor`
preserve or validate dimensions. Circular trigonometric functions accept
angular quantities. Inverse circular trigonometric functions produce angular
quantities; the two arguments of `atan2` require equal dimensions. Evaluated
assignments retain their physical dimensions.
An assignment target may append a bracketed unit expression only when that
assignment creates the parameter. A dimensionless right-hand value is
interpreted in the declared unit; an explicitly dimensioned right-hand value
must have the same dimension. The parameter identity excludes the bracketed
declaration.

`ceil(value)` and `floor(value)` round to an integer after applying their
defined numeric tolerance. Their optional second argument selects a decimal
position after truncation to an integer. Zero rounds to an integer, a positive
value rounds digits after the decimal point, and a negative value rounds digits
before the decimal point. A value above eight leaves the first argument
unchanged.

`IF <condition>`, optional `ELSE`, and `ENDIF` occupy separate source lines and
may nest. `TRUE` and `YES` are numeric true; `FALSE` and `NO` are numeric
false. A resolved condition executes exactly one branch. An inactive assignment
does not change scalar state. When a condition cannot be evaluated, every
assignment it may execute is conditional and invalidates the preceding scalar
state for that identifier. An unbalanced conditional program does not evaluate
any assignment. Assignment activation transfers as `active`, `inactive`, or
`conditional` while every source assignment retains its identity.

Every local-parameter assignment is a distinct neutral design parameter. A
local source identifier assigned once is its parameter name. Repeated local
assignments use the parameter names `<identifier>#1`, `<identifier>#2`, and so
on in source order and retain
the unqualified identifier as `source_name`. A reference to multiple executing
or conditional assignments of one identifier is ambiguous and does not bind to
one occurrence. An unscoped `d<external_id>` dependency binds to its transferred
section-dimension parameter only when exactly one such parameter exists in the
model. Repeated dimension identities remain external source metadata even when
their equal values permit expression evaluation. Inactive assignments do not
define the current dependency.
Parameter dependencies precede their consumers when the unique dependency
graph is acyclic. A cyclic edge remains source metadata instead of forming an
invalid neutral dependency order.

The identifiers `r`, `theta`, and `z` define cylindrical curve coordinates over the normalized parameter `t` from zero through one. `theta` is in degrees. Constant positive `r` with affine `theta(t)` and affine `z(t)` is a circular helix: its angular travel divided by 360 is the signed revolution count, `z(1) - z(0)` is its signed axial rise, and `theta(0)` is its start angle. The source curve-equation entity retains its native placement axis as source data.

A curve-equation entity carries its placement in `local_sys f9 <dimensions> <count> <body>`. The scalar body is bounded by the following named field and uses the stateful local-system lane; it is part of the equation entity rather than a reference to a separate coordinate-system entity. For `f9 04 03`, twelve explicit slots have the same support-frame layout as a plane local system: slots 0 through 2 are the first radial direction, slots 3 through 5 are the zero rank marker, slots 6 through 8 are the second radial direction, and slots 9 through 11 are the origin. The explicit slot language includes the `18 e5` basis-vector triple and the standalone-zero forms defined for plane local systems. Orthogonal equal-scale nonzero radial directions define the unit axis by their normalized cross product. The cylindrical coordinates map through this frame as `origin + u*r*cos(theta) + v*r*sin(theta) + axis*z`. When this frame is complete and the program is a circular helix, the curve-equation feature transfers as a neutral `Helix` with the frame-derived axis origin and direction, source radius, signed axial rise per revolution, revolution count, start angle, and clockwise sense. An incomplete frame retains `HelixNativeAxis` and its native axis reference.

Curve-equation rows use the shared rank-two local-system image defined for
plane rows.

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
A filled-surface feature with one owned section definition and that definition's
unique generating-feature transform consumes the corresponding sketch as its
boundary path. A missing or ambiguous definition, transform, or generated
sketch leaves the boundary selection unresolved.

`FamilyInf.Sld_FamilyInfo.drv_tbl_ptr` is the configuration driver-table
pointer. The configuration-root identity is
`creo:family_info:driver_table#root`. `e1` is an explicit null pointer; `f7
<canonical-reference-id>` identifies a present driver table.
The pointer is a configuration-root record even when it is null. A referenced
form retains the canonical entity identifier. A binary null pointer establishes
that the binary configuration root has no family-table configurations.

Legacy ASCII persistence stores a family table as an object graph. The root is
the unique direct `drv_tbl_ptr` object whose parent object is named `Solid` or
`Sld_FamilyInfo` and whose payload is `->`. A `drv_tbl_ptr` nested below an
`instances` row is that instance's model target, not another root. A null root,
zero roots, or multiple direct roots does not select a legacy family table.

A selected root has one complete one-dimensional `items` object array and one
complete one-dimensional `instances` object array. Each array element is a
direct child of its array object and remains in source order. An item element
has an inline object payload and exactly one scalar field of each name `id`,
`type`, `invisible`, and `name`; the first three use type-1 signed integers and
`name` uses a type-10 string scalar. Item identifiers and names are not unique;
their zero-based array ordinal is the column key.

An instance element has an arrow object payload, a non-empty UTF-8 `name`, one
type-1 signed-integer `attributes` scalar, one arrow `drv_tbl_ptr` model target,
and one complete one-dimensional `values` object array. Instance names are
unique. The values array has exactly the item-array length, and its inline
elements join the item columns by ordinal. Each value element has one type-1
signed-integer `type` scalar and exactly one typed scalar: type `50` selects
type-2 `value(d_val)`, type `51` selects type-10 `value(s_val)`, and type `52`
selects type-1 `value(i_val)`. A missing, duplicate, mismatched, or incomplete
field withholds the table join and retains the source object graph.

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
normals. When a record contains multiple complete position representations,
their position sequences must be equal. Multiple complete normal-position
arrays must also have equal normal sequences. A disagreement invalidates the
triangle-strip record; source order does not select one representation.
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
a line only when exactly six finite scalars consume the complete suffix and
exactly one byte offset starts that suffix. Zero or multiple qualifying starts
leave the row unresolved. The two endpoint positions are model coordinates in
the active principal length unit.

An `ent_list(line3d)` positional row repeats its canonical entity identifier
on both sides of `e3`, followed by its compact type and `e2` body opener. The
body fields include `end1.xyz`, `end2.xyz`, and `orig_len` as seven consecutive
scalars. A complete spatial line has a nonzero endpoint distance equal to the
absolute stored `orig_len`. The scalar run precedes the remaining positional
fields. Entity references and display fields before or after that run do not
contribute coordinates. The row body ends at the next validated row header or
the `ent_list(line3d)` block end. Exactly one seven-scalar run within that body
may satisfy the endpoint distance and stored-length invariant.

An `ent_list(arc_z)` positional row uses the same repeated-identifier and
`e2` body framing. Its explicit scalar form stores `center.xyz`, positive
`radius`, `end1.xyz`, and `end2.xyz` consecutively after the fixed row prefix.
Its coordinate suffix uses the tabulated-cylinder first-coordinate scalar
lane. That lane has precedence when a prefix has a different model-reference
mapping; a token without a first-coordinate form falls back to the
model-reference lane.
Both endpoints lie at the stored radius. For non-antipodal endpoints, their
ordered radial vectors define the circle-plane normal by their cross product.
A compressed diameter form omits the explicit center; its endpoint distance is
twice the radius, their midpoint is the center, and their shared model Z value
selects the model-Z plane. The first endpoint defines the reference direction.
The later parameter fields do not alter this carrier equation. Exactly one
explicit or compressed scalar run may satisfy the corresponding circle
invariant. The row body ends at the next validated row header or the
`ent_list(arc_z)` block end.

The named entity in `ent_list(conic)` declares compact `id`, `type`, and
`flip` fields; model-coordinate arrays `end1 f8 03` and `end2 f8 03`; scalar
fields `t0`, `t1`, `c1`, and `c2`; and a twelve-slot
`local_sys f9 04 03` body. The endpoint arrays use the model-reference
coordinate lane. Fields occur in the declared order, with `t0` and `t1`
optional. A decoded scalar owns its complete byte extent, including bytes that
match a later field header. No schema field occurs more than once; duplicate or
out-of-order identifiers, endpoints, parameters, coefficients, or local systems
make the named conic ambiguous. A `t1` body consisting of the single compact byte `11` stores
`t0 + pi`; it has no independent scalar payload and requires a decoded `t0`.
Within the local-system body, `4a` is the positive seven-byte
frame-coordinate form, and `18 e5` expands to the three
slots `[0, 1, 0]`; other slots use the same coordinate lane, including an `18`
standalone-zero slot before another complete coordinate; a terminal `18` is
also a zero local-system slot. The following `f2 f7` sequence bounds the body.
An `f2 f7` image inside a complete frame coordinate belongs to that coordinate;
when several images occur, only the unique image following a complete
twelve-slot frame is the field boundary. Decoded endpoints, parameters, and
coefficients are finite. The conic record retains its coefficients and parameter
fields without assigning ellipse semantics until its frame and carrier
invariants are complete.

A positional conic row repeats its canonical entity identifier on both sides
of the preceding `e3`, then stores `<id> <type> e2`. Its body begins
`02 48 10 00 eb 10 00 00 00 00 <flip>` and replays `end1.xyz`, `end2.xyz`,
`t0`, `t1`, `c1`, `c2`, and the twelve local-system slots in that order. The
compact `11` `t1` form stores `t0 + pi` while leaving the following coefficient
and local-system positions aligned. Decoded endpoints, parameters, and
coefficients are finite. Exactly one complete twelve-slot local-system prefix
must be immediately followed by the trailing compound record. Zero or multiple
qualifying prefixes leave the positional conic unresolved.

A type-30 conic record defines a complete ellipse carrier without interpreting
its parameter tokens when the first two local-system triples are finite
orthogonal unit vectors, the final triple is a finite center, and `|c1|` and
`|c2|` are positive. Their common plane normal is the normalized cross product
of the frame vectors. The larger coefficient magnitude is the semi-major
radius. Antipodal endpoints at exactly one coefficient radius establish the
corresponding principal direction: a major-radius endpoint supplies the major
direction, while a minor-radius endpoint supplies its in-plane perpendicular.
For non-antipodal endpoints, assigning `|c1|` and `|c2|` to the two frame
directions must produce exactly one mapping under which both endpoints are in
the frame plane and satisfy `(x/r1)^2 + (y/r2)^2 = 1`. The frame direction
assigned the larger radius is the major direction, oriented toward the first
endpoint with a nonzero major-axis projection. Equal coefficient radii have
equivalent mappings and use the first frame direction. Records that satisfy
neither proof, or admit two distinct unequal-radius mappings, do not define an
ellipse carrier.
