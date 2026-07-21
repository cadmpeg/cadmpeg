# Siemens NX `.prt` (SPLMSSTR + Parasolid): Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

---

## 1. Format overview

`.prt` is the native part format of Siemens NX. NX uses the **SPLMSSTR** (Siemens PLM Software Master Storage) hierarchical name-to-stream container. Geometry uses zlib-compressed Parasolid neutral-binary streams.

**Part and assembly storage.** A part stores geometry as Parasolid partition and deltas stream pairs. An assembly stores child part names and paths in `EXTREFSTREAM`.

**Byte order and units (global):**

- SPLMSSTR and UG_PART table fields are **little-endian**.
- Parasolid neutral-binary payload fields are **big-endian**.
- Parasolid geometric doubles are in **meters**; model geometry is conventionally millimeters (×1000).
- The Parasolid null reference value is `1`.
- Parasolid xmt indices are **stream-scoped**; the cross-stream merge key is `(stream_index, node_type, xmt_index)`.

---

## 2. SPLMSSTR container

```text
0x00..0x07   ASCII "SPLMSSTR"
0x08         version tag, constant 0x06
0x09..0x0b   file-specific uint24 LE (correlates with file complexity, not footer offset)
0x0c..0x0f   constant 0x00000000
0x10         constant 0x00
0x11..0x16   FOOTER offset, 48-bit LE (points into the FOOTER region near EOF)
0x19..0x1e   ASCII "HEADER"
0x1f..       variable-length directory entries
```

Directory entry grammar (HEADER and FOOTER identical): `name_len:u32 LE` + ASCII path (`/Root/...`) + payload. File entries carry `file_offset:u64 LE, size:u64 LE`; directory/non-file entries carry 16 opaque bytes.

FOOTER region at the 48-bit offset: ASCII `FOOTER`, then `entry_count:u32 LE`, then directory entries, then a 4-byte per-save fingerprint (unique per file version). The `/Root/` sentinel node carries UUID `611ec9b3-fa60-d111-8ad9-0800362fb302` across files.

NX XML streams contain one UTF-8 XML document and may carry one terminal `00`
byte after the document. An embedded `00` or multiple terminal `00` bytes
invalidate the stream atomically.

`/Root/part/arrangements` has an `Arrangements` root. Each `Arrangement` child
has a nonempty `Name` and a `Default` value of `YES` or `NO`. At most one child
is default, and child names are unique. Child order is configuration order. The canonical arrangements and
part-attribute paths each occur at most once; a repeated path rejects that typed
XML table atomically.

A unique part-owned `NX_Arrangement` string attribute names the active
configuration when its value equals the unique `Default="YES"` arrangement
name. The two declarations form one active-configuration identity relation.
Every body in the selected current B-rep belongs to that active configuration.
Other arrangements have no body membership without a distinct body-state relation.
An absent declaration, duplicate declaration, multiple defaults, type mismatch,
or unequal name does not form the relation.

The canonical `/Root/UG_PART/UG_PART` payload begins with a segment index of
12-byte little-endian rows:

```text
type_code:u32  subtype_code:u32  value:u32
```

Row ordinal 1 has `type_code = 1`, `subtype_code = 1`, and a `value` equal to
the payload-relative byte offset immediately after the index. Complete rows
occupy the declared region from offset zero; zero to eleven trailing bytes fill
the remainder when that offset is not divisible by twelve. Row order and all
three words are significant.

A segment-index word can point to a compressed-stream wrapper. Its first
`u32 LE` is `kind | extension_length`, where `extension_length` is the low 30
bits. Kind `0x80000000` places the zlib header at
`8 + extension_length`; kind `0xc0000000` places it at
`33 + extension_length`. The extension may contain a Parasolid text header.
The pointed stream is valid only when that exact computed position begins a
complete zlib payload accepted by the stream grammar. The containing row
ordinal and word position preserve the wrapper's segment order.

A partition or plain cached-body wrapper word begins a five-word segment tuple.
The following word is zero, the next two words are object-index aliases naming
the same body image, and the final word is the stream role. Either body alias
may occur in feature-history primary-body and Boolean operand fields. The tuple
can cross a 12-byte row boundary. The body-image binding is valid only when the
wrapper word resolves to the exact compressed stream position and both aliases
are non-zero.

A primary feature body field reuses a segment body image when its object index
equals either alias of exactly one partition or plain cached-body tuple. The
relation retains the primary field and segment binding identities; those
records retain the operation, stream classification and ordinal, object
indices, and token offsets. No relation transfers when the object index matches
zero or multiple segment bindings.

A deltas stream applies to the nearest preceding partition stream in segment
order with the same Parasolid schema token. Non-history compressed streams do
not break this relation. A later partition begins a distinct body-history unit;
a deltas stream does not cross an intervening equal-schema partition.

A segment-index word can also point directly to an OM section signature, or to
`c0 d1 f1 ed` followed immediately by that signature. The latter form has a
four-byte separator. The row ordinal and word position order the pointed OM
section relative to the compressed stream wrappers in the same segment index.

Linked OM registries define their schema role by exact declarations:
`UGS::Solid::Topol` marks the model store, `UGS::FEATURE_RECORD` marks feature
history, `UGS::EXP_expression` marks expressions, and
`UGS::OM::SaveAuditTrail` marks audit data when no preceding specialized marker
applies.

A size-framed OM section's schema trailer can contain a little-endian
section-relative record-area offset. The target begins with three `u32 LE`
control words followed by `04|05 01 text_length:u8 "NX " product_text 00`.
The pointed record area extends to the size-framed section boundary.
Feature-history sections are traversed in ascending physical section-offset
order. Multiple validated segment-index links to one section identify one
semantic feature-history section; the complete segment-link set remains raw
container linkage. Operations within a section retain record order.
Within a feature-history record area, an operation header is encoded as the
marker `80 cd 01 04 01 2f a4 7a e1 47 ae 14 7b ff ff`, four object-index
slots, then `03 length:u8 name 00`. An index below 128 is one byte. Values
through 4095 use `80..8f low:u8` and decode as `(prefix - 80) * 256 + low`.
Larger values use `90 value:u16 BE`; `ff` is null. The four slots retain their
decoded values and exact serialized tokens. `name` contains printable
ASCII bytes and `length = name_length + 2`.
Each non-null header slot addresses the zero-based entity-record ordinal in the
offset-only OM store. The addressed record retains its external index boundary
as the operation's ordered input block. The input binding retains the exact
object-index token and its offset. A slot binds only when exactly one offset-only
store contains that ordinal.
Input bindings from two or more distinct operation headers form an identity
group when they resolve to the same bounded data block. Group members retain
their input-binding identity, operation-label identity, header slot, and
object-index token offset in ascending token-offset order. Repeated slots from
only one operation do not form a group. The group assigns no direction or
semantic role between its operations.
All resolved bindings from one operation to one exact numeric expression form
one parameter-use relation. Binding identities and source offsets remain in
ascending source-offset order. Multiple input slots may witness the same use;
they do not create multiple operation-expression relations.
The fixed marker begins an operation record. A record extends through the byte
before the next validated operation marker; the final record extends through
the feature-history record-area boundary.

`UNITE`, `SUBTRACT`, and `INTERSECT` labels are followed by the fixed Boolean
header `31 00 00 01 00 14 2f a4 7a e1 47 ae 14 7b 03 00 00 e0 7f ff ff ff 01 01`,
then a target list and a tool list separated and terminated by `00`. Each list
is encoded as `01 count:u8 refs`, contains `count - 1` object indices using the
operation-header index encoding, and contains no null indices. Every reference
retains its exact object-index token and offset. The target list contains exactly
one reference. The tool list contains at least one reference and preserves tool
order.

Each of these labels projects as a neutral combine with join, cut, or intersect
semantics respectively. A complete Boolean header supplies the target and
ordered tool selections. Target and tool selections resolve atomically only
when every serialized participant maps to a nonempty neutral body set and the
participant sets are pairwise disjoint. An absent or invalid header, an
unresolved body binding, or alias overlap leaves both native
selections unresolved without discarding the operation family or Boolean kind.

A body-affecting operation record contains exactly one primary-body field
`01 02 10 body_object_index ff`. The object index uses the operation-header
encoding and retains its exact token and offset. Operations sharing the index
form one ordered body lineage. An operation depends on the preceding operation
in its primary-body lineage. A
Boolean additionally depends on the preceding operation in each tool-body
lineage, preserving tool order and omitting duplicate dependencies. When the
primary body object has a segment body-image binding, every surviving neutral
body from that image is an output of the operation. An unbound primary body
retains its object index but has no neutral output.
`DELETE` is a body-deletion operation only when its bounded record contains the
primary-body field. A `DELETE` record without that field does not identify a
body target and remains native.

An operation label equal to `SKETCH` denotes a sketch history operation. Its
position in the operation sequence is the sketch's history position. The
sketch record consists of that label, the operation record beginning at the
same header, and its uniquely resolved non-null input blocks in header-slot
order. A missing operation boundary prevents formation of the sketch record;
an unresolved input slot remains absent without reordering the other slots.

### 2.1 Stream inventory

| Stream                       | Role                                                                           |
| ---------------------------- | ------------------------------------------------------------------------------ |
| `/Root/UG_PART/UG_PART`      | canonical part payload: OM sections + Parasolid partition/deltas/plain streams |
| `/Root/FastLoad/RMFastLoad`  | fast-load object-id table → active-body membership (NX OM per-class form)      |
| `/Root/FastLoad/JT`          | preview/JT mesh and metadata                                                   |
| `/Root/UG_PART/DisplayJT`    | indexed embedded JT display-model documents                                   |
| `/Root/images/preview`       | JPEG preview image                                                            |
| `/Root/materialsTif/<name>`  | named TIFF material texture                                                    |
| `/Root/*/ExternalReferences` | `EXTREFSTREAM`; child-part names, filesystem paths, occurrence handles         |
| `/Root/part/attrs`           | `<UgAttributes>` UTF-8 XML key/value part metadata                             |
| `/Root/qafmetadata`          | UTF-8 XML catalog for stored and logical asset paths                           |
| `/Root/part/arrangements`    | (assemblies) UTF-8 XML arrangement config                                      |

`part/attrs` has an `UgAttributes` root. Each `Attribute` supplies `owner`,
`pdmBased`, `title`/`utf8title`, `value`/`utf8value`, `version`, and an XML schema
type. UTF-8 title and value fields take precedence over their compatibility
duplicates. JT and LWPA payloads are preview meshes.

`/Root/UG_PART/DisplayJT` begins with `version:u32 LE, count:u32 LE`, followed by `count` rows. Each row is `value:word_swapped_u64, header_offset:word_swapped_u64`. A word-swapped integer stores its high and low 32-bit words in significance order, with each word encoded little-endian: `integer = u32_le(first) << 32 | u32_le(second)`. The count and every row value are nonzero. Header offsets fit `u32`, are strictly increasing, follow the complete row table, remain within the stream, and address ASCII `Version ` JT headers. The index and rows retain their exact source offsets. The row value has no assigned byte-length, identity, topology, or presentation role. A missing row, zero value, oversized or nonmonotone offset, or missing JT header rejects the index atomically.

Each indexed JT document extends from its header offset to the next indexed header or the `DisplayJT` stream boundary. Its header is `version_field[80], byte_order:u8, reserved:u32, toc_offset:u32, lsg_segment_id[16]`. The version field begins with ASCII `Version ` and otherwise contains printable ASCII or whitespace. Byte order is zero and the reserved word is zero. At `toc_offset`, `count:u32 LE` is nonzero and is followed by `count` fixed 28-byte entries: `segment_id[16], segment_offset:u32 LE, segment_byte_len:u32 LE, attributes[4]`. Every segment length is nonzero and its document-relative range begins after the complete table of contents and ends within the bounded document. The document retains its outer-index row, exact version field, byte-order flag, logical scene-graph segment identifier, physical length, entries, exact attribute bytes, and source offsets. A truncated header or table, unsupported byte order, nonzero reserved word, empty table, or out-of-bounds segment rejects all documents from that outer index atomically.

Each TOC range contains `segment_id[16], segment_type:u32 LE, segment_byte_len:u32 LE, payload[]`. The segment identifier and byte length equal the owning TOC entry. The TOC attribute field is the segment type encoded as `u32 BE`. A payload beginning `compression_flag:u32 LE = 2, compressed_data_byte_len:u32 LE, algorithm:u8 = 2` contains one zlib member in all remaining bytes. `compressed_data_byte_len` equals one algorithm byte plus the physical zlib-member length. The member must consume the complete compressed range. Segments retain their document and TOC identities, identifier, type, physical length, payload hash, source offset, and validated compression lengths and inflated hash. Any header/TOC mismatch or incomplete compressed envelope rejects all segments atomically.

A type-7 shape-LOD payload is an ordered sequence of `element_byte_len:u32 LE, object_type_id[16], object_base_type:u8, object_id:u32 LE, body[]` elements. Every element has object base type 4. `element_byte_len` counts every byte after its own word, so `body` has length `element_byte_len - 21`. The sequence ends with `element_byte_len = 16, object_type_id = ff[16]`, followed by the exact six-byte segment tail `01 00 00 00 00 00`. Elements retain their segment, order, exact object-type identifier, object identifier, base-type discriminator, body length and hash, and source offset. An undersized or out-of-bounds element, wrong base type, absent end marker, or noncanonical segment tail rejects all shape-LOD elements atomically.

A JT 9 tri-strip shape-LOD element has object-type identifier `ab10dd10-c82a-d111-9b6b-0080c7bb5997`. Its body begins `base_version:u16 LE = 1, vertex_version:u16 LE = 1, vertex_bindings:u64 LE, topological_mesh_version:u16 LE, vertex_records_object_id:u32 LE, compressed_lod_version:u16 LE`, where both mesh versions are 1 or 2. The remaining bytes are the complete compressed mesh representation. The header retains both fixed versions, the exact binding mask, both mesh versions, vertex-record object identifier, remaining byte length and hash, and source offset. A truncated header or unsupported version rejects all tri-strip headers atomically.

The JT 9 topologically compressed representation begins with Int32 Compressed Data Packet Mk. 2 vectors in this order: eight face-degree contexts; vertex valences, vertex groups, and vertex flags; eight 30-bit face-attribute-mask contexts; the next 30 bits and upper four bits of context seven; one or more high-degree face-attribute-mask lanes; split-face symbols; and split-face positions. An absent high-degree collection is represented by one empty lane. Each packet begins `value_count:u32 LE`; a zero count ends the packet after four bytes. A nonempty packet continues with `codec:u8`. Bitlength and arithmetic packets contain `code_text_bit_len:u32 LE` and `ceil(code_text_bit_len / 32)` 32-bit code words. A chopper packet contains `chop_bits:u8`; zero chop bits are followed by one recursively framed packet, while nonzero chop bits are followed by `value_bias:i32 LE, value_span_bits:u8` and recursively framed MSB and LSB packets. Recursion depth does not exceed three and every nested packet has the outer value count. Every arithmetic packet ends with one recursively framed out-of-band packet whose value count equals the number of decoded escape symbols; zero escapes require the four-byte zero-count packet. Arithmetic probability contexts and out-of-band packets are included in their owning packet envelope. A `composite_hash:u32 LE` follows the outer vectors. The high-degree lane count is the unique framing for which two following split packets and the vertex-record header reproduce the shape-LOD vertex binding mask and valid quantization and nonnegative count fields; absence or ambiguity rejects the topology envelope.

The topologically compressed vertex-record block immediately follows the composite hash. Its header is `vertex_bindings:u64 LE, vertex_quantization_bits:u8, normal_quantization_factor:u8, texture_quantization_bits:u8, color_quantization_bits:u8, topological_vertex_count:i32 LE`, followed by `vertex_attribute_count:i32 LE` only when the topological vertex count is positive. Both serialized counts are nonnegative; an omitted attribute count has value zero. Vertex, texture, and color quantization do not exceed 24 bits; the normal factor does not exceed 13. The topology packet sequence retains every semantic lane name, value count, codec, complete byte length, digest, relative offset, composite hash, total topology length, and source offset. A decoded lane also retains its reconstructed primal values; vertex flags and split-face symbols use Lag1 prediction and all other topology lanes use null prediction. The vertex-record header retains its binding mask, quantization parameters, counts, remaining compressed-array length and digest, and source offset. Invalid packet framing, nested count disagreement, excessive recursion, invalid quantization, negative counts, or truncation rejects both arenas atomically.

When the topological vertex count is positive, the compressed coordinate array begins `unique_vertex_count:i32 LE, component_count:u8`, followed by three uniform quantizers. Each quantizer is `minimum:f32 LE, maximum:f32 LE, quantization_bits:u8`. The unique count equals the topological vertex count, the component count is three, every range endpoint is finite and ordered, and each component's quantization bits equal the vertex-record quantization value. The coordinate-array header retains the unique count, component count, three ranges, three bit counts, remaining compressed-component length and digest, and source offset. A zero topological vertex count omits the coordinate array. Count disagreement, an invalid component count, nonfinite or reversed ranges, bit counts above 32, bit disagreement, or truncation rejects all topology, vertex-record, and coordinate-array arenas atomically.

When all three coordinate quantization widths are zero, each XYZ component is encoded as an exponent-and-sign Int32 Compressed Data Packet Mk. 2 followed by a mantissa packet. Both vectors have the unique vertex count and use Lag1 prediction. The low nine bits of each reconstructed exponent-and-sign integer and the low 23 bits of its reconstructed mantissa integer form one IEEE 754 binary32 bit pattern. All reconstructed coordinates are finite. When the widths are nonzero, each XYZ component is one Int32 Compressed Data Packet Mk. 2 code vector with the unique vertex count and Lag1 prediction. For width `bits`, `maximum_code` is `2^bits - 1` below 32 bits and `0xffffffff` at 32 bits. Each reconstructed code is interpreted as unsigned, does not exceed `maximum_code`, and decodes to `minimum + (code - 0.5) * (maximum - minimum) / maximum_code`. A `coordinate_hash:u32 LE` follows the six lossless packets or three quantized packets. The decoded coordinate array retains the owning header, ordered XYZ points in metres, coordinate hash, complete encoded byte length, and source offset. A packet failure, count disagreement, out-of-range code, nonfinite coordinate, or missing hash rejects all decoded coordinate arrays atomically.

An Int32 Compressed Data Packet Mk. 2 begins with `value_count:u32 LE`; a zero count ends the packet. A nonempty packet continues with a codec byte. Codec 1 contains `code_bit_count:u32 LE` and `ceil(code_bit_count / 32)` little-endian code words consumed most-significant-bit first. Its first code bit selects fixed mode when zero and variable mode when one. Fixed mode contains `minimum_width:u6, maximum_width:u6`, signed minimum and maximum fields of those widths, then one unsigned range code per value using `ceil(log2(maximum - minimum + 1))` bits; adding the minimum produces each value. Variable mode contains `mean:i32, width_delta_bits:u3, run_length_bits:u3`, then blocks until the declared value count is reached. Each block changes the current signed-value width, initially zero, by one or more signed width deltas; the minimum and maximum delta values continue the delta sequence and any interior value ends it. The resulting width is zero through 32. A nonzero run length follows, then that many signed values of the current width, each added to the mean with wrapping 32-bit arithmetic. The code-text bit count is consumed exactly. Codec 3 contains the same code-text fields, followed by one probability context and a recursive out-of-band packet. The probability context begins with a big-endian entry count; its field widths, minimum value, and entries are a byte-aligned MSB-first bit stream. Zero-frequency entries occupy no arithmetic interval. Every escape symbol consumes one value from the out-of-band packet, including when the escape count is zero. Codec 4 begins with `chop_bits:u8`. A zero chop width continues with one complete recursively encoded packet having the outer value count. A nonzero chop width continues with `bias:i32 LE, span_bits:u8`, then recursive MSB and LSB packets; each nested packet has the outer value count and each reconstructed value is `(lsb | (msb << (span_bits - chop_bits))) + bias`. Recursive depth does not exceed three. An invalid codec, field width, recursion, count, padding bit, range, or packet bound rejects the packet.

Predictor reconstruction operates on decoded Int32 packet values. The first four values are unmodified primers. Each later Lag1 or Lag2 value is its residual plus the preceding primal value at distance one or two. Stride1 predicts `v1 + (v1 - v2)`; Stride2 predicts `v2 + (v2 - v4)`. StripIndex predicts `v2 + (v2 - v4)` when that stride is strictly between -8 and 8, and `v2 + 2` otherwise. Ramp predicts the zero-based value index. Xor1 and Xor2 combine the residual with the Lag1 or Lag2 prediction by bitwise XOR instead of addition. Null prediction leaves every value unchanged. Integer addition and subtraction use wrapping 32-bit two's-complement arithmetic.

The first topological-mesh packet is the context-zero face-degree symbol vector. Its record retains every decoded symbol in topology-coder visit order, the complete packet byte length and hash, and its source offset. Failure to decode or bound this packet rejects all initial face-degree records atomically.

The topology vectors encode the dual vertex-facet mesh. Each vertex-valence symbol creates one dual vertex and consumes the corresponding group and flag symbols. Each nonzero face-degree symbol creates a dual face with that many ordered vertex slots; zero denotes a split reference. The face-degree context is determined from the current dual vertex valence and the count and total degree of its already known incident faces: valence three selects contexts zero through two relative to degree six, valence four selects contexts three through five relative to degree four, valence five selects context six, and every other valence selects context seven. Newly created faces enter an active list. The next face is the member with the fewest empty slots among the last sixteen active entries, preferring the newest on a tie; completed entries encountered in that window are removed. A split-face symbol is a one-based offset from the end of this active list and its paired position is the target face's vertex slot. All degree, valence, group, flag, split-face, and split-position symbols are consumed exactly.

Each newly created dual face consumes one attribute mask from context `min(7, max(0, degree - 2))`. Contexts zero through six contain the low 30 mask bits. Context seven combines its low 30-bit lane, following 30-bit lane, and upper four-bit lane into a 64-bit mask. Faces above degree 64 consume `ceil(degree / 32)` consecutive high-degree words. Bits beyond the face degree are zero. The number of set bits allocates that many consecutive global vertex-attribute identifiers. At a face vertex, attribute selection starts with the face's last identifier and advances cyclically at every set mask bit through that vertex slot. A face without set bits has no attribute identifier. All mask words are consumed exactly.

Every completed dual vertex becomes one output polygon. Its cyclic incident-face identifiers are coordinate-array indices, while its group and flag values become the polygon group and flags. Dual faces may have degree one or two at non-manifold display seams, but output polygons have nonzero valence. Every polygon index is below the coordinate array's unique vertex count. A missing symbol, invalid context, zero valence, invalid degree, conflicting slot assignment, out-of-range split, incomplete dual ring, unconsumed symbol, or out-of-range coordinate index rejects all polygon meshes atomically. The polygon-mesh record retains the topology sequence, coordinate header, ordered index rings, groups, flags, and source offset.

A polygon with a nonnegative group is a rendered triangle; negative-group polygons are structural dual-mesh records and do not enter the rendered triangle list. A canonical tessellation requires one exact property-table binding from the owning logical shape node to the type-7 segment and payload object containing the coordinate and topology records. The logical node resolves through its common node element in the binding's type-1 scene segment. Every group-derived node has common node data followed by `version:u16 LE = 1, child_count:u32 LE, child_object_id:u32 LE[child_count]`; bytes after this common group prefix belong to the concrete node family. These ordered child lists supply parent links for group, metadata, part, LOD, range-LOD, and partition nodes within the segment. Base-node flag bit zero prunes every root-to-shape path through that node. An instance node has common node data followed by `version:u16 LE = 1, child_object_id:u32 LE`; it contributes one parent link from its own object identifier to the referenced child. Transform attributes accumulate from each root-to-shape path. An ignored transform has no effect; a final transform suppresses later attributes unless their force bit is set. A local matrix premultiplies the inherited matrix. Each acyclic root-to-shape path produces one tessellation and retains the ordered instance-node identities on that path. The matrix transforms serialized metre coordinates before conversion to document millimetres. Normals use the normalized inverse transpose of its linear part. When normals, colors, texture coordinates, or vertex flags are bound, triangle corners are deindexed so each output vertex has the corner's coordinate and mask-selected attribute record; this preserves attribute seams at a shared coordinate. Normals become the parallel canonical normal vector. RGBA binary32 values use codec-owned channel kind `0x4e580001`, item size 16, and the two-bit color binding as flags. Vertex flags use codec-owned kind `0x4e580002`, item size four, flags zero, and little-endian `u32` values. Texture channel ordinal `n` uses codec-owned kind `0x4e580100 + n`, item size `4 * component_count`, and flags `n | (binding_nibble << 8)`. Channel payload counts equal the deindexed vertex count. The canonical tessellation retains the shape-node source identity, root-to-shape node path, ordered instance-node path, ordered coordinates, rendered triangles, normals, and channels; it has no body association until a separate NX body-to-scene-node relation is present. A missing or ambiguous scene binding, invalid group prefix, cyclic or incomplete scene path, non-triangular rendered polygon, missing coordinate array, out-of-range attribute index, incomplete bound attribute lane, inconsistent texture component count, or empty rendered triangle set rejects all JT tessellations atomically.

When vertex binding bit three is set and the vertex-attribute count is positive, a compressed normal array follows the coordinate array and begins `normal_count:i32 LE, component_count:u8 = 3, quantization_bits:u8`. The normal count equals the vertex-attribute count and the quantization width equals the vertex-record normal factor. Width zero encodes each XYZ component as exponent-and-sign and mantissa Int32 Compressed Data Packet Mk. 2 vectors without prediction; their low nine and 23 bits form finite binary32 values. A nonzero width from one through 13 encodes four unpredicted packet vectors: sextant, octant, theta, and psi. Sextants are zero through five, octants zero through seven, and angular codes are below `2^bits`. Deering reconstruction uses a 13-bit angular grid with maximum psi `0.615479709`, applies the sextant coordinate permutation, then negates each component whose corresponding octant bit `4,2,1` is zero. A `normal_hash:u32 LE` follows the packets. The normal array retains its vertex-record header, ordered vectors, hash, complete byte length, and source offset. Invalid counts, components, widths, codes, packets, nonfinite results, or missing hash reject all normal arrays atomically.

A compressed texture-coordinate array begins `texture_coordinate_count:i32 LE, component_count:u8, quantization_bits:u8`; its count equals the vertex-attribute count, its component count is one through four, and its width is zero through 24. Width zero stores one unpredicted exponent-and-sign and mantissa packet pair per component. Nonzero width stores one uniform quantizer `minimum:f32 LE, maximum:f32 LE, bits:u8` per component followed by one Lag1-predicted code packet per component. Every quantizer width equals the array width and codes use the uniform dequantization rule. Component-major values are transposed into attribute-record order. A `texture_coordinate_hash:u32 LE` follows every representation. Invalid counts, components, quantizers, packets, codes, nonfinite values, or missing hash reject the array atomically.

Color binding bits four and five select one compressed color array after the optional normal array and before all texture-coordinate arrays. The array begins `color_count:i32 LE, component_count:u8, quantization_bits:u8`; its count equals the vertex-attribute count, its component count is three or four, and its width is zero through eight. Width zero stores one Lag1-predicted exponent-and-sign and mantissa packet pair per component; three-component colors receive alpha one. A nonzero width stores `hsv_flag:u8` followed by four component quantizers and four Lag1-predicted code packets in hue/red, saturation/green, value/blue, alpha order. RGB quantizers are four complete `minimum:f32 LE, maximum:f32 LE, bits:u8` records. HSV quantizers are four bit widths with fixed ranges `[0,6]`, `[0,1]`, `[0,1]`, and `[0,1]`. HSV values convert to RGBA by the six-sector hue model. A `color_hash:u32 LE` follows every representation. The decoder retains ordered RGBA values, the hash, complete byte length, and source offset. Invalid counts, component counts, widths, flags, ranges, packets, codes, nonfinite results, or missing hash reject the array atomically.

Compressed vertex arrays occur in binding order: coordinates, normals, colors, texture-coordinate channels zero through seven, then vertex flags. Each present array starts at the exact end of its predecessor. Binding bit six selects the vertex-flag array `flag_count:i32 LE, flags:Int32CDP2`. Its count equals the vertex-attribute count and every decoded value is zero or one. The retained array stores the owning vertex-record header, ordered values, exact byte length, and source offset. Count disagreement, packet failure, or a nonbinary value rejects all vertex-flag arrays atomically.

The inflated content of a compressed segment uses the same element-length, object-type, object-identifier, base-type, and end-object-marker framing. Bytes after the end-object marker are a segment-type-specific tail and remain exact rather than being interpreted as another element. A compressed sequence retains the owning segment and type, ordered element identities, framed byte length, exact tail bytes and hash, and physical envelope offset. Each element retains its inflated offset. An invalid element length or absent end-object marker rejects all compressed element sequences atomically.

Every element in a type-31 segment is a string property atom with object-type identifier `6e10dd10-c82a-d111-9b6b-0080c7bb5997`, base type 5, and body `01 00 00 00 00 40 01 00, code_unit_count:u32 LE, value[code_unit_count]:u16 LE`. The body ends after the declared UTF-16 code units. Each atom retains its compressed-element identity, object identifier, exact code units, decoded string, and physical envelope offset. A different object type or base type, invalid UTF-16, count mismatch, or trailing body byte rejects all type-31 string property atoms atomically.

The JT major and minor version are the unsigned decimal components after ASCII `Version ` and before the following whitespace in the 80-byte version field. A missing component, nondecimal component, or component exceeding `u16` rejects all documents from the outer index atomically.

Every graph-node element in a type-1 logical scene-graph segment has base type 0, 1, or 2 and begins with common node data. JT major versions below 10 encode `version:u16 LE, flags:u32 LE, attribute_count:i32 LE, attribute_object_ids[attribute_count]:i32 LE`; major versions 10 and later encode the version as `u8` and retain the remaining fields in the same order. The count is nonnegative. Family-specific node data begins immediately after the ordered attribute identifiers. Object identifiers retain their exact 32-bit bit patterns as unsigned values. The common node record retains its compressed-element identity, exact object-type identifier, node object identifier, version, flags, ordered attribute references, and the family-data byte length and hash. A negative or truncated count, truncated reference lane, or non-graph-node base type rejects all type-1 common node records atomically.

A JT 9 geometric-transform attribute has object-type identifier `8310dd10-c82a-d111-9b6b-0080c7bb5997`, base type 3, and body `base_version:u16 LE = 1, state_flags:u8, field_inhibit_flags:u32 LE = 0, transform_version:u16 LE = 1, stored_values_mask:u16 LE, values[]:f32 LE`. State bits outside `0x0f` are zero. Matrix elements use row-major order from bit 15 through bit zero; a clear bit supplies the corresponding identity-matrix value and a set bit consumes one finite value. The matrix is affine: its final column is `(0,0,0,1)`. Its three linear rows are nonzero and mutually orthogonal within binary32 arithmetic tolerance, admitting rotation and positive or negative nonuniform scale while excluding shear, projection, and singular scale. The retained attribute stores its logical element, object identifier, accumulation state, mask, complete matrix, and physical envelope offset. Invalid versions, flags, inhibition, value count, nonfinite values, trailing bytes, shear, projection, or singular scale reject all transform attributes atomically. Base-type 3 elements are attributes and do not enter the common-node arena.

A JT 9 tri-strip shape node has object-type identifier `7710dd10-c82a-d111-9b6b-0080c7bb5997` and base type 2. Its family data is `shape_version:u16 LE = 1, reserved_bounds[6]:f32 LE, untransformed_bounds[6]:f32 LE, area:f32 LE, vertex_count_range[2]:i32 LE, node_count_range[2]:i32 LE, polygon_count_range[2]:i32 LE, memory_byte_len:u32 LE, compression_level:f32 LE, vertex_version:u16 LE, vertex_bindings:u64 LE, vertex_quantization_bits:u8, normal_quantization_factor:u8, texture_quantization_bits:u8, color_quantization_bits:u8`. Vertex version 1 ends after the quantization bytes and occupies 100 bytes. Vertex version 2 appends `version_2_vertex_bindings:u64 LE` and occupies 108 bytes. Each bounding-box minimum is no greater than its maximum. Area is finite and nonnegative. Count minima are nonnegative and do not exceed their maxima. Compression level is finite and in the inclusive range zero through one. Vertex version is 1 or 2. Vertex, texture, and color quantization do not exceed 24 bits; the normal factor does not exceed 13. The node retains its common node, object identifier, bounds, area, ranges, memory size, compression level, vertex version, binding masks, quantization parameters, and physical envelope offset. Invalid values, truncation, or trailing bytes reject all JT 9 tri-strip shape nodes atomically.

After the logical scene-graph element end marker, a type-1 payload contains a second element sequence of property atoms followed by a property table. A late-loaded property atom has object-type identifier `e55bb0e0-bdfb-d111-a3a7-00aa00d10954`, base type 8, and body `base_version:u16 LE = 1, state_flags:u32 LE, property_version:u16 LE, segment_id[16], segment_type:u32 LE, payload_object_id:u32 LE, reserved_value:u32 LE`; `reserved_value` is nonzero. A string property atom uses the type-31 string-atom body and base type 5. The property table is `version:u16 LE, element_count:u32 LE`, followed by `element_object_id:u32 LE` and zero or more `key_object_id:u32 LE, value_object_id:u32 LE` pairs for each element; a zero key terminates each element's pairs. The key `JT_LLPROP_SHAPEIMPL` names a late-loaded value whose segment identifier and type resolve exactly one type-7 segment in the same document. The resulting binding retains the table version, shape-node, key and value object identifiers, property flags and version, resolved segment, payload object identifier, reserved value, and source offset. Truncation, trailing table bytes, an invalid recognized atom, or an unresolved or ambiguous shape implementation rejects all shape-LOD bindings atomically.

In JT major versions below 10, a partition node has object-type identifier `3e10dd10-c82a-d111-9b6b-0080c7bb5997`. After common node data it contains `group_version:u16 LE, child_count:i32 LE, child_object_ids[child_count]:i32 LE, partition_flags:u32 LE, file_name_count:i32 LE, file_name[file_name_count]:u16 LE`. Only partition flag bit zero is defined. Six finite `f32 LE` values form each axis-aligned bounding box as minimum XYZ then maximum XYZ, with each minimum no greater than its maximum. If flag bit zero is set, the filename is followed by transformed bounds; otherwise it is followed by reserved bounds and then transformed bounds. The remaining fields are `area:f32 LE, vertex_count_range[2]:i32 LE, node_count_range[2]:i32 LE, polygon_count_range[2]:i32 LE`, followed by untransformed bounds only when flag bit zero is set. Area is finite and nonnegative. Each count minimum is nonnegative; its maximum is `-1` or at least the minimum. The record ends after these fields. Partition nodes retain their common node, ordered children, exact filename code units, decoded filename, flags, bounds, area, count ranges, and physical envelope offset. Invalid UTF-16, control characters, unknown flags, invalid bounds or ranges, truncation, or trailing bytes reject all JT 9 partition nodes atomically.

JT major versions below 10 encode group data as `group_version:u16 LE, child_count:i32 LE, child_object_ids[child_count]:i32 LE`. A range-LOD node has object-type identifier `4c10dd10-c82a-d111-9b6b-0080c7bb5997`; after group data it contains `lod_version:u16 LE, reserved_count:i32 LE, reserved_values[reserved_count]:f32 LE, reserved_value:i32 LE, range_version:u16 LE, range_count:i32 LE, range_limits[range_count]:f32 LE, center[3]:f32 LE`. Both counts are nonnegative and all floating-point values are finite. Range limits are nonnegative and strictly increasing. The record ends after the centre. Range-LOD nodes retain their common node, ordered alternate-representation children, all versions, reserved fields, range limits, centre, and physical envelope offset. Invalid counts, unordered limits, truncation, nonfinite values, or trailing bytes reject all JT 9 range-LOD nodes atomically.

`/Root/images/preview` is a JPEG marker stream beginning with SOI `ff d8`.
The first SOF segment supplies sample precision, non-zero big-endian height and
width, and a non-zero component count. Its payload length is exactly
`6 + 3 * component_count`. The bounded entry length and SHA-256 identify the
preview asset; malformed marker lengths or a scan/EOI marker before SOF do not
produce preview metadata.

Each `/Root/materialsTif/<name>` file entry contains one TIFF stream. The first
eight bytes are byte-order magic `II`, version `42:u16 LE`, and a little-endian
first-IFD offset, or byte-order magic `MM`, version `42:u16 BE`, and a big-endian
first-IFD offset. The first IFD begins at an offset of at least eight and before
the bounded entry end. Entry offset, entry length, byte order, version,
first-IFD offset, and SHA-256 identify the texture asset. The path suffix is the
stored stream name; it is not a material display name when the suffix is an
`unmap$` alias. An invalid TIFF header or out-of-bounds first IFD does not
produce an asset.

`/Root/qafmetadata` has a `folderContents` root and ordered `folderProperties`
children. Each child carries `location` and `unmappedLocation` attributes,
followed by one `createTime` and one `modifyTime` element. A material-texture
catalog entry exists when `location` begins `materialsTif/` and exactly equals
the path of a decoded `/Root/materialsTif/<name>` asset after removing
`/Root/`. `unmappedLocation` begins `materialsTif/` and its nonempty suffix is
the logical material-texture name. The relation retains both complete relative
paths and both time strings. Duplicate asset mappings, a missing texture asset,
an unexpected child element, or incomplete time children invalidate the
catalog atomically.

`EXTREFSTREAM` contains `EXTREFSTREAM` magic, `version:u32 LE (3)`, `payload_size:u32 LE`, a record region, and a trailing string table: `01` + `count:u32 LE` + `count × (len:u16 LE + control-free UTF-8)`. The string table contains child `.prt` names and paths.

Assembly `.prt` files contain no inline Parasolid partition, deltas, or plain cached-body streams. Their component geometry resides in the external child `.prt` files named by `EXTREFSTREAM`. Occurrence placement binds each external component instance.

---

## 3. Parasolid stream extraction

Text-wrapped envelope:

```text
**PARASOLID ... **END_OF_HEADER <zlib payload>
```

The partition zlib stream is preceded by `c0 d1 f1 ed`. Small zlib streams use repeating `<u32 BE count> 0x02000002` marker pairs. The wrapper-header counts are segment or record counts.

Inflated prologue text classifies each stream:

| Prologue bytes                                      | Stream kind         |
| --------------------------------------------------- | ------------------- |
| contains `(partition)`                              | partition           |
| contains `(deltas)`                                 | deltas              |
| contains `TRANSMIT FILE created by` without subtype | plain (cached body) |
| otherwise                                           | stream              |

### 3.1 Neutral-binary encoding

Inflated streams begin `PS 00 00`; the prologue contains a schema token `SCH_<version>` (for example, `SCH_3501171_35102_13006`). The third component (`13006`) is an NX-embedding constant.

XMT index encoding:

| Form        | Encoding                                                                                         |
| ----------- | ------------------------------------------------------------------------------------------------ |
| Small index | `uint16` BE, 2 bytes                                                                             |
| Large index | negative `int16` remainder + `uint16` quotient, 4 bytes; `raw = quotient*32767 + abs(remainder)` |

**Record shift rules.** At logical offset `+2`, `0xff` can encode an envelope escape or begin a large-index xmt with a remainder beginning `ff`. Any xmt pointer slot can consume four bytes instead of two and shifts later fixed fields in the record. Effective record length is `fixed_length + escape_shift + record_start_large_index_shift`. Pointer-field large-index shifts change field positions without changing the record start length, except in families with a compact tail.

### 3.2 Schema self-description

The neutral-binary streams are partially self-describing. After `SCH_` the head carries a field dictionary for the stream-root wrapper class (the `00 ce` record). Node types absent from the base schema carry an inline class definition at first use:

```text
<type:u16 BE> <sig_len:u8> <signature> <name_len:u8> <name>
```

Signature alphabet: `C` = component/pointer (xmt ref), `I` = int, `D` = double, `A` = array ref, `Z` = terminator/compound. Inline definitions include type 38 `intersection_data` (`CCCCCCCCCCCA`), type 80 `legal_owners` (`CCCCCDI`), and type 100 `precision` (`CCCCCCCCCA`).

The wrapper `00 ce` instance owns the stream BODY (`child`), attribute-definition list (`attdef_list`), preview-mesh references (`mesh`/`polyline`/`lattice`), and index-map arrays (`index_map`, `node_id_index_map`, `schema_embedding_map`).

### 3.3 NX object-model framing

An indexed object-model section carries an entity-boundary array followed by an object count and object-ID array. Boundary slot zero is zero. Subsequent values are monotonic offsets relative to the section base. Object IDs in slots `1..count` pair with entity spans bounded by adjacent boundary values. The first entity begins with `(04|05) 01 text_length:u8 "NX "`.

An offset-only object-model store instead carries an absolute boundary array,
then a record count. Boundary slot zero bounds the store root/control block;
slots `1..count+1` bound column-storage blocks. These blocks have no individual
object identity. A block may split a string, fixed array, or field lane across
adjacent boundaries, so marker-shaped bytes inside one block do not define an
entity string or reference. Concatenating the column-storage blocks in boundary
order reconstructs the exact logical storage region; block boundaries add no
separator or padding. Block ordinals include the root/control block when it is
present: that block is ordinal zero and column blocks begin at one. A store
without a distinct root/control block numbers its first column block zero.

Each indexed store contains exactly one self-framed product/version header in
the combined control and first-data-block span:
`(04|05) 01 text_length:u8 "NX " version_text 00`. `text_length` equals the
printable text length plus two. Store metadata may precede the header inside
either bounded block.

Class definitions before the boundary array use `declared_length:u8 + "UGS::" name bytes + trailing_code:u8`, where `declared_length` includes the trailing code. Bytes between the trailing code and the next class declaration form that declaration's registry suffix; an empty suffix is valid. An 11–14-byte suffix consists of a 2–5-byte layout prefix, an eight-byte schema fingerprint, and one terminal layout byte. Member definitions in the same indexed schema use the same framing with an `m_` name. Declaration order supplies section-local class and member identity.

Class and member declaration ordinals are local to one OM section. The containing
section base plus the declaration ordinal forms their identity; equal ordinals in
distinct sections do not identify the same class or member. Entity-record
ordinals are likewise local to the indexed section whose base governs the
external boundary array.

A compact-index lane is a concatenation of entries. Bytes `00..7f` encode their
unsigned value directly. A byte in `80..fe` followed by `low:u8` encodes
`(prefix - 0x80) * 256 + low`. Byte `ff` encodes a null entry and consumes no
following byte. A two-byte prefix without its low byte does not form a complete
lane.

A numeric expression table contains a `hostglobalvariables` root entity. Each expression entity contains:

```text
<handle:u8> 04 text_length:u8
"(Number [" unit "]) " name ": " expression "; "
00
```

`text_length` includes the leading marker byte and trailing zero, so it equals the ASCII text length plus two. The nearest preceding `hostglobalvariables` marker starts the expression table; a later marker starts a distinct table even within the same directory entry. Defined units are `mm` and `degrees`. Parameter names use `p<decimal-index>` or `p<decimal-index>_<qualifier>`. The qualifier is nonempty and contains ASCII alphanumerics or underscores. The complete name is bounded on both sides by non-name bytes. The qualifier remains part of the parameter name; equal decimal indices with distinct qualifiers are distinct parameters. A context-free arithmetic expression over finite decimal scalars, parentheses, unary signs, `^`, `*`, `/`, `+`, and `-` supplies its evaluated value. Parameter references are atomic operands. Exponentiation precedes unary signs and associates right; multiplication and division precede addition and subtraction. Formula text retains ordered exact parameter-name dependencies; repeated references denote one dependency at its first occurrence. A dependency resolves only when its exact name identifies one parameter in the same expression table. Acyclic formulas evaluate after same-unit dependencies have values. Equal names in distinct tables do not conflict or cross-resolve. Ambiguous names within one table, cycles, cross-unit references, unknown names, calls, and noncanonical parameter tokens remain unevaluated.

---

## 4. Record framing

### 4.1 Fixed record families

Lengths are logical, before escape/large-index shifts. Each code is a Parasolid XT node type.

| Type | Name    | Length | Type | Name          | Length     |
| ---: | ------- | -----: | ---: | ------------- | ---------- |
|   12 | BODY    |     24 |   50 | PLANE         | 91         |
|   13 | SHELL   |     24 |   51 | CYLINDER      | 99         |
|   14 | FACE    |     39 |   52 | CONE          | 115        |
|   15 | LOOP    |     16 |   53 | SPHERE        | 99         |
|   16 | EDGE    |     32 |   54 | TORUS         | 107        |
|   17 | FIN     |     23 |   56 | BLEND_SURF    | 66 + shift |
|   18 | VERTEX  |     28 |   60 | OFFSET_SURF   | 31         |
|   19 | REGION  |     16 |  124 | B_SURFACE     | 23         |
|   29 | POINT   |     40 |  133 | TRIMMED_CURVE | 85 + shift |
|   30 | LINE    |     67 |  134 | B_CURVE       | 23         |
|   31 | CIRCLE  |     99 |  137 | SP_CURVE      | 33 + shift |
|   32 | ELLIPSE |    107 |      |               |            |

Types carrying `node_id:u32` place it at record offset `+4` (after shifts). FIN has no `node_id`. EDGE candidates with denormal tolerance (`abs(tol) < 1e-100`) are payload coincidences, not records.

Type 38 is the XT `INTERSECTION` node. Delta-stream `0x5a` records use the `intersection_data` layout.

### 4.2 Deltas-stream framing

A deltas stream is a schema-framed incremental edit log paired with a partition. Both declare the same schema token. Records are not length-prefixed; they self-delimit by typed decode (valid record ends on a plausible next-record tag). Two record forms:

Status-framed fixed records are normalized by removing each reference status byte before fixed-record graph decoding. An unpaired deltas stream uses the same normalization as a deltas stream that contributes a complete replacement to a partition; its non-fixed procedural residual remains a separate semantic lane.

**Full record:**

```text
type:u16 BE
xmt:encoded_index
node_id:u32 BE                   0-based delta-stream ordinal
<type signature fields>          reference slot = encoded_xmt + status:u8
```

FIN omits `node_id` and begins its nine signature references immediately after `xmt`. The status byte is `0x01` and frames each reference. The record form carries the merge operation.

**Tombstone:** a compact 6-byte deletion `type:u16 BE  xmt:u16  00 01`. A whole-record tombstone has this complete form. In a full record, `xmt 01` is a reference and status byte. Tombstone xmts are plain high-range `u16` values (48300+).

Tombstones form descending contiguous xmt runs that can span topology, geometry, and attribute record types. Partition topology remains authoritative. A tombstone does not remove a point, curve, or surface carrier still referenced by a surviving vertex, fin, edge, or face unless a later full deltas record replaces that carrier. Unreferenced exact-key records follow the last full-record or tombstone event.

---

## 5. Topology

### 5.1 Ownership graph

```text
body → shell → [region] → face → loop → fin → edge → vertex → point
                                    ↑ face → surface, edge → curve
```

**Common header** for analytic curve/surface types 30–32, 50–54: `attributes +8`, `owner +10`, `next +12`, `previous +14`, `group +16`, `sense +18`.

Any fixed record may place an envelope escape byte `ff` between its type and xmt fields. The xmt begins one byte later and all logical payload offsets shift by one. When the first xmt byte is also `ff`, both the escaped and unescaped large-index forms are structurally possible; the complete family field grammar disambiguates them.

Topology node layouts (logical offsets, pre-shift):

| Type        | Fields                                                                                                                                                                                                                           |
| ----------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| BODY (12)   | `node_id +4`; owner of shells/faces/edges/vertices                                                                                                                                                                               |
| SHELL (13)  | `node_id +4`, `attributes +8` (=1), `body_ref +10`, `next_shell +12` (=1), `first_face +14`, sentinels `+16/+18` (=1), `region_ref +20`, `face_anchor +22` (`1` or `first_face`)                                                         |
| FACE (14)   | `attributes +8`, `tolerance:f64 +10`, `next_face +18`, `prev_face +20`, `loop +22`, `shell +24`, `surface +26`, `sense +28`, `next_on_surface +29`, `prev_on_surface +31`, `next_front +33`, `prev_front +35`, `front_shell +37` |
| LOOP (15)   | `attributes +8`, `fin +10`, `face +12`, `next_loop +14`                                                                                                                                                                          |
| EDGE (16)   | `attributes +8`, `tolerance:f64 +10`, `fin +18`, `prev_edge +20`, `next_edge +22`, `curve +24`, `next_on_curve +26`, `prev_on_curve +28`, `owner +30`                                                                            |
| FIN (17)    | `attributes +4`, `loop +6`, `forward_fin +8`, `backward_fin +10`, `vertex +12`, `other_fin +14`, `edge +16`, `curve +18`, `next_at_vertex +20`, `sense +22`                                                                      |
| VERTEX (18) | `attributes +8`, `fin +10`, `prev_vertex +12`, `next_vertex +14`, `point +16`, `tolerance:f64 +18`, `owner +26`                                                                                                                  |
| POINT (29)  | `attributes +8`, `owner +10`, `next +12`, `prev +14`, `xyz:3×f64 +16` (meters)                                                                                                                                                   |
| REGION (19) | `node_id +4`; referenced by SHELL                                                                                                                                                                                                |

A **body-shape SHELL** requires the invariant fields `attributes`, `next_shell`, and `+16/+18` to equal `1`, non-null `body_ref` and `region_ref`, and a resolvable `first_face`. With null `face_anchor`, `FACE.next_face` defines a finite ownership chain whose members back-reference the SHELL. With non-null `face_anchor == first_face`, every FACE that back-references the SHELL belongs to it. The body and region references remain ownership identities when the stream omits the corresponding BODY or REGION record. FACE and EDGE `tolerance` decode as the sentinel `-3.14158e13` (`c2 bc 92 8f 99 6e 00 00`) or a positive finite meter value whose converted millimeter value is finite. The format imposes no magnitude bound on a tolerance. `FIN.curve` is non-null only on tolerant edges (tolerant-edge trims use TRIMMED_CURVE→SP_CURVE).

For SHELL, FACE, LOOP, FIN, EDGE, and VERTEX, a non-null `attributes`
reference identifies the stream-local attribute list owned by that exact topology
record. The topology type and xmt together identify the owner. Attribute-list
identity does not assign a class, value, or presentation meaning until the
referenced list and its instances resolve.

### 5.2 Reference domains

- Ordinary BREP references (`FACE.surface`, `EDGE.curve`, `FIN.curve`, `VERTEX.point`, BLEND_SURF/INTERSECTION support refs) resolve within the same stream.
- SHELL ownership records may resolve in `{partition, paired_deltas}`. A SHELL's non-null BODY and REGION references remain ownership identities when either referenced record is not serialized.

### 5.3 Topology assembly

| Entity   | Rule                                                                  |
| -------- | --------------------------------------------------------------------- |
| vertices | FIN-referenced VERTEX nodes; coordinates from same-stream POINT nodes |
| edges    | one per EDGE node; native endpoint incidence is `EDGE.fin → FIN.vertex` and `FIN.other_fin → FIN.vertex`, with null `other_fin` falling back to `FIN.forward_fin → FIN.vertex`; canonical start/end order follows increasing curve parameter; the carrier resolves through non-null `EDGE.curve`, otherwise through the owning `FIN.curve` |
| loops    | walked from `FACE.loop` through the null-terminated LOOP chain; each FIN ring closes at its first FIN with reciprocal forward/backward links; non-null partner FINs reciprocally reference one another and carry the same EDGE |
| faces    | one per FACE node, with resolved surface when available               |
| bodies   | one per validated body-shape SHELL                                    |

POINT is a geometric carrier. It becomes a topological vertex only through a validated `FIN.vertex → VERTEX.point` path. An unreferenced POINT is not a free vertex of an existing body.
Distinct fixed analytic carrier records retain physical record order within each point, surface, or curve family. A graph-bounded fixed analytic carrier and a scanner candidate at the same type-tag offset are one carrier; graph framing and XMT identity are authoritative.
An EDGE belongs to the assembled B-rep only when a FIN in a fully resolved owned LOOP references it.
An unresolved carrier placeholder belongs to the transferred model only when an
emitted FACE or EDGE references it. Fixed-record scanner candidates outside the
resolved body closure do not create free unknown carriers.
An edge's two serialized trim limits are an unordered interval. Canonical start/end order follows evaluation at the ascending limits. A periodic interval is then normalized by reducing its start modulo `2π` and preserving its nonnegative sweep; a seam-crossing interval therefore ends above `2π`.
The interval binds to the referenced typed curve only when evaluating its two limits reaches the edge vertices within the edge and vertex tolerances. A failed interval binding omits the parameter range but does not replace or discard the referenced curve carrier.
For a procedural carrier without a solved evaluator, the ascending native trim
interval remains authoritative and FIN incidence supplies endpoint order. Lack
of an evaluator does not replace an exact procedural construction with an
unknown carrier.

An EDGE may carry null curve reference `1` with a finite tolerance. With a null
owning `FIN.curve`, this is a tolerant intersection edge: its carrier is the
intersection relation between the two distinct surfaces reached through its
radial FIN pair, bounded by the EDGE vertices, within the serialized edge
tolerance. Transfer represents the relation as a procedural intersection
carrier with the two face surfaces; it does not synthesize a line between the
vertices. A null EDGE and FIN curve without exactly two distinct adjacent
support surfaces remains carrierless.
A null `EDGE.curve` may instead have a non-null owning `FIN.curve`. The FIN
reference is the carrier path. When it resolves through
`TRIMMED_CURVE → SP_CURVE` whose original 3D curve is null, the SP_CURVE's
surface and pcurve define a procedural parametric surface curve. Its finite
domain is the trim interval, or the solved NURBS pcurve knot domain when the FIN
references the SP_CURVE directly.
A FIN pcurve attaches to a coedge only when evaluation through that face's
surface reaches both edge vertices within the larger of the edge, vertex, and
pcurve fit tolerances. A pcurve carried on a different support remains part of
the procedural curve construction but is not attached to that face.
A body is solid when every assembled EDGE has exactly two FIN uses in that body. A body with faces and any edge-use count other than two is a sheet body.

BODY, REGION, and SHELL records contain no placement reference. POINT coordinates and the origins and axes stored by curve and surface carriers are part-model coordinates. An inline Parasolid body's part placement is therefore the identity transform.

Body-shape SHELL validation: invariant/ref predicate passes; `body_ref` and `region_ref` are non-null; `first_face`→FACE in the SHELL's stream. A null `face_anchor` requires the `FACE.next` walk to close at null with visited faces back-referencing the SHELL. A non-null `face_anchor` equals `first_face` and selects all FACE records that back-reference the SHELL.

**Periodic faces / closed edges.** Parasolid stores a periodic surface as one face. A full-circle/ellipse edge stores no trim interval or wrap-count field and references the bare CIRCLE/ELLIPSE. Its one-FIN loop has `forward_fin == backward_fin == self`. The FIN vertex is either a VERTEX shared by both edge ends or the null reference; the null form's canonical topological point is the analytic curve point at parameter zero. The full revolution has parameter identity `[0, 2π]`. An EDGE with `curve == 1` has no curve record and is the surface-intersection locus of its incident faces.

---

## 6. Geometry carriers

All geometric doubles are finite binary64 values in meters → ×1000 for mm, and their converted millimeter values are finite. The format imposes no model-magnitude bound. Directions and axes are unit vectors (not scaled); angular parameters are radians; linear curve parameters are meters of arc length.

### 6.1 Analytic curves and surfaces

Payload offsets are relative to the record's type tag, after the common header (§5.1).

| Type          | Payload                                                                              |
| ------------- | ------------------------------------------------------------------------------------ |
| LINE (30)     | point `+19`, direction `+43`                                                         |
| CIRCLE (31)   | center `+19`, normal `+43`, x_axis `+67`, radius `+91`                               |
| ELLIPSE (32)  | center `+19`, normal `+43`, x_axis `+67`, major `+91`, minor `+99`                   |
| PLANE (50)    | origin `+19`, normal `+43`, x_axis `+67`                                             |
| CYLINDER (51) | origin `+19`, axis `+43`, radius `+67`, x_axis `+75`                                 |
| CONE (52)     | origin `+19`, axis `+43`, radius `+67`, sin_half `+75`, cos_half `+83`, x_axis `+91` |
| SPHERE (53)   | center `+19`, radius `+43`, axis `+51`, x_axis `+75`                                 |
| TORUS (54)    | center `+19`, axis `+43`, major `+67`, minor `+75`, x_axis `+83`                     |

Every analytic normal or axis and its x-axis are finite unit vectors with an absolute dot product below `1e-6`. A non-unit or non-orthogonal frame rejects the analytic carrier.

An extended leading reference shifts the analytic payload and record end by the same decoded byte count. Bytes before that shifted end remain owned by the record and cannot open another analytic carrier.

Validity gates: CIRCLE, ELLIPSE, CYLINDER, SPHERE, and TORUS radii are positive. ELLIPSE has `major >= minor`. CONE reference radius is nonnegative and has finite nonzero `sin_half` and `cos_half` satisfying `sin_half² + cos_half² ≈ 1`; SPHERE has a unit axis; a horn torus has `major == minor`.

**OFFSET_SURF (60):** discriminator byte `+19` (`V`/`I`/`U`), `true_offset:u8 +20` (`0`/`1`), base surface ref `+21`, finite `offset_distance:f64 +23` (meters). Surface `P = base(u,v) + offset_distance · unit_normal(u,v)`. The format imposes no magnitude bound on the finite distance. There is no scale field at `+31` (that position lands in the next record). For a B_SURFACE base, the unit normal comes from the rational quotient rule:

```text
Pu = (Au·W − A·Wu)/W²,  Pv = (Av·W − A·Wv)/W²,  normal = normalize(Pu × Pv)
```

An OFFSET_SURF used by a FACE transfers as a procedural surface carrier. The carrier and offset construction reference each other and the carrier references a complete typed source record. The source record retains the discriminator, true-offset flag, base-surface reference, signed millimeter offset, and inflated-stream offset. Model evaluation follows the base reference recursively, computes the normalized parameter-tangent cross product, and applies the signed offset; cyclic base graphs do not evaluate.

### 6.2 B-spline carriers (B_SURFACE 124 / B_CURVE 134)

B_SURFACE / B_CURVE are compact: header through sense `+18`, then `nurbs` ref `+19` and `data` ref `+21` (both large-index capable). The full NURBS resolves through support records:

| Type | Tag    | Role                                                                                                                                                        |
| ---: | ------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
|  125 | `007d` | B-surface control-grid payload (`double_count` near `+91`, then values)                                                                                     |
|  126 | `007e` | B-surface descriptor: `u_degree +6`, `v_degree +8`, `u_pole_count +12`, `v_pole_count +16`, forms `+18/+19`, distinct-knot counts `+20/+24`, mult/knot refs |
|  127 | `007f` | multiplicity arrays (`alloc`, ref, `alloc × u16`)                                                                                                           |
|  128 | `0080` | knot arrays (`alloc`, ref, `alloc × f64`)                                                                                                                   |
|  135 | `0087` | B-curve control payload                                                                                                                                     |
|  136 | `0088` | B-curve descriptor: `degree +4`, `pole_count +8`, `dimension +10` (2=UV, 3=XYZ), distinct-knot `+14`, form `+16`, mult/knot refs `+23/+25`                  |

Types 135 and 136 may place an `ff` envelope escape before their xmt. This
shifts every subsequent logical field by one byte. Type 135 may place a second
`ff` escape before its control-value count; the count and control-reference tail
shift by one additional byte. Multiplicity and knot references in type 136 are
sequential encoded xmts, so an extended multiplicity reference shifts the knot
reference.

Control-grid stride = `double_count / (u_pole_count · v_pole_count)`; `3` = non-rational xyz and `4` = rational xyzw. Each support-record XMT is unique within its stream. In each direction, degree is less than pole count and multiplicities satisfy `sum(mults) = n_poles + degree + 1`. Pole-grid ordering is u-major.

### 6.3 Procedural intersection curves (type 38 / `0x5a`)

NX stores freeform edges and blend rails as construction relations with branch witnesses. A type-38 record has a compact header through sense `+18` and six support xmt references at `+19,+21,+23,+25,+27,+29`.

The neutral curve carrier references a complete typed source record. The source record retains the five common-header references, orientation sense, six construction references, record form, and inflated-stream offset. The record form distinguishes type 38 from the single-byte `0x5a` delta twin.

CHART_s, term-use, support-UV, and blend-bound XMT identities are unique within one physical stream. A duplicate identity rejects that auxiliary relation. A second chart record may supply native parameters and support-UV lanes only when its point count and model-space points agree with the geometric chart within the larger chordal tolerance. When deltas history is merged into its paired partition, a later complete CHART_s record replaces an earlier record with the same XMT before intersection validation.

The chart/start-term/end-term witness slots `ref[2:5]` are atomic: all three are null reference `1`, or all three are non-null. Mixed null and non-null witness slots do not form a type-38 or `intersection_data` construction. Type-38 common-header `attributes` is null reference `1`. Deltas type-38 records append status byte `01` to every reference; transfer removes those status bytes before applying the partition-style construction grammar.

| Ref | Role                                                                                       |
| --- | ------------------------------------------------------------------------------------------ |
| 0/1 | primary support surface + type-59 second-support bridge (order set by the `0x00cc` marker) |
| 2   | `0x28` CHART_s seed/control polyline                                                       |
| 3/4 | `0x29` term_use start / end endpoint                                                       |
| 5   | `0x00cc` values-array (support UV parameters)                                              |

For the `0x5a` delta twin the layout is fixed (primary = ref[0], bridge = ref[1]); for type-38 the primary/bridge assignment follows the `0x00cc` marker (marker-2 → primary ref[0]; marker-3 → primary ref[1]).

When exactly one serialized support is null, an edge using the construction
supplies the missing support if its incident face surfaces contain exactly one
surface not already named by the construction. That unique complement occupies
the null support slot. Two null slots or more than one unmatched incident
surface do not establish support order and remain unresolved.

After support completion, an incident FIN supplies a missing support-UV chart
when its coedge has a validated pcurve, its owning face uses that exact support
surface, and no second distinct pcurve satisfies the same curve/surface pair.
The pcurve geometry transfers into the matching construction side.

**CHART_s (`0x28`):** branch selector and native-parameter certificate:

```text
00 28 [ff] count:u32 BE  xmt
base_parameter:f64  base_scale:f64  chart_count:u32  chordal_error:f64  angular_error:f64
parameter_error[2]:f64   (sentinel pair -31415800000000.0)
count × Hvec              (Hvec block always starts at pre+52, pre = end of count+xmt)
```

Hvec form depends on the stream: partition streams use **`xyz3`** (`x,y,z` meters); deltas streams use **`ext11`** (`x,y,z, p3,p4,p5,p6, tx,ty,tz, t`), with a unit tangent and strictly increasing native `t`. The two ext11 surface-parameter lanes are `(p3,p5)` and `(p4,p6)`. A non-sentinel lane belongs to a support when evaluating every lane pair on that support reproduces the corresponding chart point within `chordal_error` and no other support satisfies the same lane. Two assigned lanes address distinct supports. When exactly one of two complete lanes has a unique support assignment, the other lane belongs to the other distinct support. A sentinel-bearing, multiply matching, non-evaluating, or conflicting lane otherwise supplies no support chart. The chart parameter is meter-scale: `t_{k+1} = t_k + chord_k · f_k`, with `t_0 = base_parameter` and chords in meters. `chordal_error` defines the verification tolerance for chart-hosted carriers. Intersection charts use `(base_parameter, base_scale) = (0.0, 1.0)`. Procedural-spine charts have `chart_count == count`, sentinel `parameter_error`, and finite non-zero `base_scale`. When `xyz3` and `ext11` records have the same xmt, count, and point sequence within the larger chordal error, the `ext11` native `t` sequence and surface-parameter lanes govern the shared chart carrier.

Each physical direct or escaped CHART_s record remains a distinct typed source record, including duplicate XMT records. It retains both counts, all preamble scalars, the sentinel pair, ordered points, ext11 parameter and UV lanes, Hvec layout, framing, and inflated-stream offset. Carrier construction merges compatible duplicate XMT records only after retaining them separately.

**term_use (`0x29`)** records are hard trim endpoints (`ref[3]` = start vertex point, `ref[4]` = end vertex point, meters). Each record contains a leading count, XMT, two-byte endpoint-form discriminator, and one model-space point. Count `1` uses form `L?`; count `2` uses `TF` or `TS`. The typed source record retains these fields, the point in millimetres, framing form, and inflated-stream offset. Each endpoint lies within the CHART_s `chordal_error` of the corresponding first or last chart point. When either term reference does not resolve, exactly one EDGE must carry the intersection curve, both edge vertices must resolve through VERTEX and POINT records, and exactly one ordering of the two edge points must match the two chart endpoints within `chordal_error`. The matching topology point replaces only the missing term witness. Three record forms occur for `0x28`/`0x29`/`0x00cc`: direct tagged, `0xff`-escaped, and descriptor-inline (payload follows the ASCII schema keyword + a fixed field-schema tail).

Within one physical stream a duplicate term-use XMT invalidates that endpoint identity. When deltas history is merged into its paired partition, later complete term-use records replace earlier records with the same XMT before endpoint validation.

**`0x00cc` values-array** packs support UV samples by marker byte:

| Marker | Packing | Meaning                                                       |
| -----: | ------- | ------------------------------------------------------------- |
|    2/3 | `2·n`   | `(u,v)` on support 0                                          |
|      4 | `4·n`   | `(u0,v0,u1,v1)`: first pair on support 0, second on support 1 |

The typed source record retains the scalar count, marker, ordered scalar lane, direct/escaped/descriptor-inline framing, and inflated-stream offset. Tuple grouping derives only from the marker and never reorders the serialized lane.

The value `-31415800000000.0` is a missing-parameter sentinel. Preserve the tuple position. Support-0 `(u,v)` values evaluate on the analytic surface to the curve's 3D points.

CHART_s and its serialized or uniquely topology-backed endpoint witnesses define the bounded 3D carrier independently of the values-array. A null, sentinel-bearing, or count-mismatched values-array omits the corresponding pcurve; it does not invalidate the 3D chart carrier. A missing endpoint without a unique topology-backed match rejects the carrier atomically.

When independent support inversion still leaves a procedural support lane incomplete and that lane retains a finite non-sentinel first parameter pair, a transverse support pair continues the chart-selected intersection branch in the combined parameter space `(u0,v0,u1,v1)`. The retained pair seeds the unresolved support and the first chart point seeds the other inverse parameterization. Each later chart point is inversely mapped on both supports using the preceding corrected pair as the branch seed. Periodic analytic coordinates are lifted by integral periods to the representative nearest the preceding corrected coordinate. A periodic B_SURFACE uses the difference between its active knot-domain endpoints as its period; evaluation and first derivatives reduce an unwrapped coordinate modulo that period. OFFSET_SURF inherits the periods of its recursive support. The null vector of the three-row surface-equality Jacobian supplies the parameter-space tangent, and its mapped model-space direction must agree with the chart chord. When a tangential support pair makes that null vector non-unique, the chart chord independently maps into each support tangent plane; both normalized mapped tangents must reproduce the chord direction within `1e-8`, and identical support identity is rejected. Correction solves the three surface-equality equations plus the predictor's tangent-normal hyperplane. Every corrected support pair evaluates to one common model point and that point lies within `chordal_error` of the corresponding chart point. A rank-deficient predictor that does not already satisfy surface equality, reversed or stationary progress, non-finite steps, non-convergence, or a chart-tolerance failure rejects the coupled derived completion atomically.

Each CHART_s point first maps independently through one support carrier's inverse parameterization. Analytic planes, cylinders, cones, spheres, and tori use their closed-form inverse. A NURBS surface supplies the closest solution over its bounded knot domain; each point after the first uses the preceding solution only to select between equal-distance solutions. Undefined coarse-grid evaluations and failed refinement starts reject only those candidates; at least one complete finite candidate is required. A finite non-sentinel serialized point at the same ordinal is the preferred inverse seed. OFFSET_SURF preserves its support's parameterization. Inversion uses the support inverse before an offset-surface domain grid and refines against the offset surface; refinement terminates as soon as the mapped point lies within `chordal_error` of the CHART_s point. The complete derived lane atomically replaces an absent or sentinel-bearing lane only when forward evaluation reproduces every chart point within `chordal_error`. Consecutive analytic azimuth parameters unwrap by integral turns to preserve chart continuity. A complete serialized lane that already has a unique support assignment takes precedence.

Two support carriers have identical parameterization when their typed surface geometries are equal, or when both are OFFSET_SURF constructions with bit-identical signed offsets, equal U/V senses and extension flags, and recursively identical support parameterizations. Distinct carriers with identical parameterization use the same complete support-UV lane. A cyclic comparison does not establish identity.

When a FIN carries the intersection and its owning FACE uses one of the intersection supports, that support's UV chart is the FIN pcurve. Transfer requires both pcurve endpoints, mapped through the FACE surface, to coincide with the EDGE vertices within the stored edge, vertex, face, or chart-fit tolerance. A chart that fails this incidence relation remains construction data and is not attached to the coedge.

```text
cylinder: P = O_mm + (v·1000)·A + r_mm·(cos u · X + sin u · (A×X))
plane:    P = O_mm + (u·1000)·X + (v·1000)·(N×X)
torus:    Y = A×X;  P = C_mm + (R + r·cos v)·(cos u · X + sin u · Y) + r·sin v · A
```

**UV validation.** The first and last evaluated UV samples reproduce the term endpoints within `1e-6` mm.

**Type-59 BLEND_BOUND (`0x003b`)** contains the XMT, five common-header references, sense, `boundary_index` (0/1), and `blend_surface_ref` to a BLEND_SURF construction surface. The optional `0xff` after the tag is an envelope escape. The typed source record retains every field, the escape form, and the inflated-stream offset. For participating support `A`, `B.support_refs[1 - boundary_index] == A`. `B.support_refs[boundary_index]` identifies the support that closes the blend rolling-ball law at the cap.

### 6.4 TRIMMED_CURVE (133) and SP_CURVE (137)

**TRIMMED_CURVE (133):** basis_curve ref `+19` (large-index capable → shifts later fields +2), `point_1 +21`, `point_2 +45`, `parm_1:f64 +69`, `parm_2:f64 +77`. The curve is `basis(t)` restricted to `[parm_1, parm_2]`; parameters are in the basis's native units: LINE uses meters of arc length from the stored point (×1000 for mm), CIRCLE uses radians, and B_CURVE uses knot units. Unscaled meter spans on a LINE basis place the trim interval 1000× too small.

The typed source record retains the basis reference, both stored model-space points in millimetres, both parameters in basis units, and the inflated-stream offset. Topological transfer aliases the wrapper to its terminal basis carrier and applies the validated parameter range to the owning edge.

TRIMMED_CURVE and SP_CURVE references form an XMT graph, not a record-order stack. A wrapper may reference another wrapper serialized later; resolve wrapper chains to a terminal curve carrier independent of record order.

**SP_CURVE (137):** surface ref `+19`, b_curve ref `+21`, original ref `+23`, `tolerance_to_original:f64 +25` (after ref shifts). It represents a curve-on-surface: a 2D B-curve in the surface parameter space. `tolerance_to_original` uses the same missing-tolerance sentinel as FACE and EDGE; the sentinel transfers as no tolerance.

The typed source record retains the support-surface, parameter-space B-curve, and nullable original-curve references, the serialized meter tolerance including its sentinel form, and the inflated-stream offset.

A B_CURVE descriptor with `dimension = 2` stores `(u,v)` control points rather than model-space coordinates. Rational payloads store homogeneous `(u·w,v·w,w)` triples. The coordinates use the supporting surface's native parameter units. Transfer to canonical IR multiplies both plane parameters by 1000 and multiplies the axial parameter of cylinders and cones by 1000; angular parameters and NURBS knot-space parameters remain unchanged. The SP_CURVE tolerance is a model-space distance in meters and transfers to millimeters.

### 6.5 Rolling-ball blend surface (BLEND_SURF 56)

A BLEND_SURF FACE is a procedural canal or envelope surface. Record layout:

```text
compact header through sense +18
subtype byte +19            (`0x52` / `R` = rolling-ball)
support refs +20,+22,+24    (large-index capable): support 0, support 1, spine
4 × f64                     values = (range[0], range[1], thumb_weight[0], thumb_weight[1])
4 × xmt tail refs           `1` (null references)
```

A BLEND_SURF used by a FACE transfers as a procedural surface carrier. The
carrier references the complete typed source record. The carrier and blend
construction reference each other; oriented supports, spine, radius law, and
cross-section remain in the construction. The source record retains both
signed support offsets and both dimensionless thumb weights in serialized
support order.

`values[0:2]` are nonzero signed support offsets `range[2]` in meters. Their magnitudes are equal within `1e-9` meter and give the rolling-ball radius `r = |range|`. The neutral constant-radius law retains the sign of `range[0]`; each oriented support independently retains its own range sign. `values[2:4]` are dimensionless `thumb_weight[2]`. Support reference 2 identifies the ball-centre spine. Spine families include:

- **Offset-intersection spine:** a type-38 whose two supports are both OFFSET_SURF, with base refs and offsets mirroring the blend's supports and `range` (`O_i = base_i + range_i · oriented_normal_i`). Freeform (NURBS-offset) bases.
- **Direct-supports spine:** a type-38 on the original analytic supports directly.
- **Fixed-curve spine:** an ELLIPSE (type 32); ellipse non-circularity encodes plane draft angle (`major/minor = 1/cos(draft)`).
- **Tool-body delta spine:** a `0x5a` INTERSECTION_DATA record with a real (non-sentinel) `geometric_owner`.

**Canal law** `B(t,s)` uses the two supports, signed range, and the spine marker-4 UV chart:

```text
B(t,s) = C(t) + r · Rot_about_T(t)( s·α(t) ) · E0(t)
  C(t)   = ball-centre spine = S0(u0,v0) + σ0·r·N0 = S1(u1,v1) + σ1·r·N1
  Q_i(t) = contact rail on support i = S_i(u_i(t), v_i(t))
  E_i(t) = (Q_i(t) − C(t)) / r        (unit; |Q_i − C| = r exactly)
  T(t)   = unit spine tangent C'(t)/|C'(t)|
  α(t)   = atan2((E0×E1)·T, E0·E1)    signed ball-arc angle, varying along the spine
  rails:  B(t,0) = Q0(t),  B(t,1) = Q1(t)
  normal: n(t,s) = (B(t,s) − C(t)) / r   (radial from ball centre; envelope-of-spheres, no differentiation)
```

`σ0, σ1 ∈ {+1,−1}` are the `range` signs, with `|range| = r`. The spine identity is `S0+σ0·r·N0 == S1+σ1·r·N1`. Rail incidence is `B(t,0)=Q0(t)`. At each rail, the canal normal equals the support surface normal.

For an offset-intersection spine, each complete spine-side pcurve maps the spine parameter directly to the corresponding blend-support parameters. The spine side and blend support correspond when their recursive OFFSET_SURF lineages have the same base carrier and the magnitude of their accumulated signed-offset difference equals `r` within binary64 arithmetic tolerance. Separately serialized planes have the same offset lineage when their normals and u-axes are equal and the origin displacement is parallel to the normal; the signed normal projection is the lineage offset. Separately serialized cylinders have the same offset lineage when their origins, axes, and reference directions are equal; their signed-radius difference is the lineage offset. Separately serialized spheres require equal centers, axes, reference directions, and radius signs; their lineage offset is the radius difference multiplied by the support-radius sign. Opposite radius signs do not establish lineage across the singular center. Separately serialized circular cones have the same offset lineage when their axes are collinear, their axis and reference directions and half-angles are equal, and both elliptic ratios are exactly one. For cone-origin displacement `Δo`, radius difference `Δr`, common unit axis `a`, and half-angle `α`, parameter-preserving lineage additionally requires `Δr sin(α) + dot(Δo,a) cos(α) = 0` within binary64 arithmetic tolerance; the signed lineage offset is `Δr cos(α) - dot(Δo,a) sin(α)`. Separately serialized tori require equal centers, axes, reference directions, and major radii; both major radii exceed the absolute minor radii, and the minor-radius signs are equal. Their lineage offset is the minor-radius difference multiplied by the support minor-radius sign. Opposite minor-radius signs and horn or spindle tori do not establish one global lineage. Two circular constant-radius blends have the same offset lineage when they use the same spine, exactly one support correspondence preserves both support senses, every corresponding support pair has offset magnitude equal to the blend-radius difference, and the blend-radius difference is the lineage offset. Offset sign varies with support ordinal and surface sense and does not select the support. The mapping is used only when exactly one same-lineage spine side has the required magnitude; zero or multiple candidates reject the mapping. Degree-1 non-rational pcurves invert piecewise linearly in support-parameter space. Higher-degree and rational NURBS pcurves invert over their complete active knot domain. Circular and elliptical blend spines use their angular parameterization; periodic inverses retain the branch nearest the preceding support-control parameter. A blend-boundary inverse transfers only when exactly one boundary reproduces the model-space point within the intersection `chordal_error`. Reconstructed support charts become available to dependent blend spines in monotone dependency order; reconstruction terminates when a complete pass adds no support lane. When exactly one intersection side has a complete chart, the opposite chart is its inverse image on the other support. The solved 3D intersection carrier supplies the shared model point when the complete side is an unevaluable procedural boundary. For a direct blend support, the complete support pcurve inverts the blend spine's same-lineage contact pcurve and fixes the blend boundary ordinal. When the contact pcurve is unavailable, the nearest blend-spine parameter fixes the same boundary only when the solved point lies at radius `r` from the spine and its normalized radial vector is perpendicular to the spine tangent. Radius error is bounded by the effective chart tolerance; angular error is bounded by `max(chordal_error / r, 1e-8)`. The inverse chart subdivides in the shared curve parameter until its piecewise-linear UV evaluation reproduces the complete side in model space or satisfies those canal-boundary invariants within the curve's cache-fit or edge tolerance; failure at the subdivision bound rejects the entire chart. A tolerant edge with two absent charts uses a NURBS support boundary only when both edge endpoints invert to exactly one constant-parameter domain boundary and vary in the other parameter. The second support chart transfers from that boundary, or two independently identified boundary charts transfer together, only when the complete parameter range evaluates coincidently within the edge tolerance.

For an intersection CHART_s point on a constant-radius BLEND_SURF, the nearest resolved spine point over the complete active knot domain supplies an initial `t`. The normalized ball-centre-to-chart vector supplies the initial section direction. Its signed angle from `E0` about `T`, divided by `α`, supplies an initial `s`, including integral-turn alternatives. When this pair does not reproduce the chart within `chordal_error`, coupled least-squares refinement minimizes the three-dimensional canal residual in `(t,s)`. Each accepted refinement step decreases squared model-space distance; backtracking rejects an increasing step, and a finite NURBS spine domain bounds `t`. The derived `(t,s)` lane transfers only when the spine is an evaluable line or NURBS carrier, both support contact points resolve, and forward canal evaluation reproduces every CHART_s point within `chordal_error`. Continuation seeds select between equal-distance spine branches after the first chart point; they do not restrict the closest-point domain.

For a finite NURBS spine, failure of the nearest-centre initial pair invokes a bounded canonical-patch search over the complete active spine domain and section interval `s ∈ [0,1]`. A `9 × 5` grid includes both domain boundaries. Undefined canal evaluations reject only their grid positions. The closest finite grid position seeds the same decreasing-residual refinement, and a refined fallback leaving the closed section interval is rejected. Forward evaluation within `chordal_error` remains mandatory.

When a BLEND_SURF spine is itself a procedural curve represented by a cached carrier, forward support-chart evaluation uses the sum of the chart's `chordal_error` and the spine carrier's cache-fit tolerance. A completed support lane raises the dependent intersection curve's cache-fit tolerance to at least that sum. An exact line or NURBS spine contributes no additional tolerance. This propagation preserves the independently serialized radius and bounds the accumulated deviation of nested cached constructions.

**Chained blend-on-blend** recurses into the support blend canal. A cyclic support graph does not define a canal and is invalid. Offsetting a constant-radius canal along its normal gives a canal with radius `r+δ`: `B(t,s; r+δ) = B(t,s; r) + δ·n(t,s)`. A spine uses one branch pair `(i0,i1)` for each polyline point.

**Primitive reduction.** A constant-radius blend with a circular spine has torus parameters `major = circle radius`, `minor = r`. A line spine has cylinder radius `r`. Reduction requires `|range[0]| == |range[1]|` and a circular or linear spine with at least five points.

---

## 7. Metadata, history, and body composition

### 7.1 NX object model (OM)

UG_PART begins with a 12-byte row table of LE u32 triples pointing at OM sections and Parasolid wrapper headers. An OM section starts at signature `ff ff ff ff`, optionally preceded by `c0 d1 f1 ed`, and stores `payload_size:u32 BE` at `+8` with `section_end = signature_offset + 16 + payload_size`. Bytes `+12..+14` are `OM`. The section decomposes into preamble, type registry, field registry, object-id table, and entity records.

**Externalized record boundaries.** Every OM section with an id-table carries, immediately before its `object_id_table`, a `(count+1)`-entry monotone `u32 LE` **entity_index** with `index[0] == 0`. OM entity records have no inline length prefix; lengths live in the entity_index:

```text
oid_end = object_id_table_off + 4 + count*4       # first entity record start
base    = oid_end − entity_index[1]               # self-anchoring
record i = bytes[base + index[i], base + index[i+1])
object_id(i) = object_id_table[i]
```

Each bounded record retains the decoded object ID, exact source offset of its four-byte object-ID table word, exact payload boundary, and payload byte identity.

The first record at `oid_end` begins `04 01, declared_len:u8, version_text[declared_len-2], 00`. `version_text` is printable ASCII beginning with `NX ` and may end in a space. A **type registry** declaration is `declared_len:u8, name[declared_len-1], trailing_code:u8`; `name` is printable ASCII beginning with `UGS::`. The zero-based declaration ordinal is the class identity. A **field registry** declaration has the same core framing with a printable name beginning `m_`. The bytes from its trailing code through the next length-framed `m_` declaration form that field's registry suffix. The final declaration has no next-declaration boundary and therefore no bounded suffix.

The primary UG_PART section uses an offset-only index. A trailing `record_count:u32 LE` follows `record_count+2` monotone offsets. Offsets are relative to the UG_PART payload start. `index[0]` starts identity metadata, `index[1]` starts the first entity, and the remaining entries bound `record_count` entities:

```text
identity_metadata = bytes[index[0], index[1])
record i = bytes[index[i+1], index[i+2])   # 0 <= i < record_count
```

The offset-only form does not assign one fixed-width object ID to every record. A bounded record has entity identity only when it contains a persistent handle.

A zero-prefixed offset-only store control-array form is an atomic array of four-byte words. Each word is `00, value:u24 LE`; the array is nonempty and its byte length is divisible by four. Values retain their zero-based word order and byte offsets. A nonzero prefix byte or incomplete final word means the control block uses another form and does not produce this array.

A product-terminated control-array form has zero to three leading zero bytes, followed by a nonempty aligned array of `value:u32 LE`, followed immediately by the unique self-framed `04|05 01 ... "NX " ... 00` product record in the control block. The leading-zero count aligns the value array to its own four-byte boundary. A value smaller than the same section's total control-plus-column block count addresses the block at that ordinal; other values remain unbound. Multiple product records, a nonzero alignment prefix, or a partial value invalidates the complete array.

Independently of the control-block form, complete `e0, handle:u32 BE` and four-byte high-nibble-`c` tagged-reference tokens are retained in byte order within the bounded control block. Record-ordinal tokens are not defined for offset-only control storage and are excluded.

A maximal run of exactly two adjacent persistent-handle tokens forms a control handle pair: `e0, first:u32 BE, e0, second:u32 BE`. The pair retains both reference occurrences and values. A single token or a maximal run of three or more tokens does not form a pair.

An offset-store block may carry a counted block-index lane `01, declared_count:u8, anchor, member[declared_count-2], 01 11`, with `declared_count >= 3`. The anchor and members are non-null compact indices: `00..7f` are direct, `80..fe, low:u8` decode as `(marker-80)*256+low`, and `ff` is null. Every index addresses the same offset-only store's control-plus-column block ordinal. The lane is retained only when its count is complete, its terminator is exact, and every addressed block exists. It retains decoded indices, exact serialized tokens, and source offsets. Anchor and member order remain distinct; no semantic role is assigned by the lane framing.

Contiguous offset-store column storage may carry an `ABR` reference lane `11, slot[16], 02 11 41 42 52 ff 03`. Each ordered slot is a nullable compact block index: `ff` is null and non-null values use the direct and extended forms. Every non-null value addresses the same offset-only store's control-plus-column block ordinal. The lane is retained only when all sixteen slots and the complete literal terminator are present and every non-null target exists. It retains decoded slots, exact serialized tokens, and source offsets. Physical data-block boundaries do not constrain the lane.

Contiguous offset-store column storage may also carry a self-framed index row `2d 02 0b, first:compact_index, 93 8a, flag:u8, index[4], 00 47 04 04 01 c0 44 04 00`, where `flag` is `03` or `07` and every compact index is non-null. Each member of the four-index lane addresses the same offset-only store's control-plus-column block ordinal. The row retains the unresolved leading index, ordered block lane, exact serialized index tokens, flag, token offsets, block containing its opening byte, and opening offset within that block without assigning operand, object, class, or block-ownership roles. A null or out-of-range block index, other lane arity, other flag, changed discriminator, or incomplete terminator rejects the candidate atomically. Physical data-block boundaries do not constrain the row.

A second self-framed row is `02 0b, first:compact_index, 93 8c, discriminator:u8, target:compact_index, ff ff 90 fe, index[3], 00 47, flag:u8, mode:u8, 01 c0 44 04 00`. The discriminator is `16`, `17`, or `18`; the flag is `03` or `07`; the mode is `04` or `07`; and `target` plus all three trailing indices are non-null in-range block ordinals in the same offset-only store. The unresolved leading index, discriminator, ordered four-block lane, exact serialized index tokens, flag, mode, token offsets, block containing its opening byte, and opening offset within that block are retained. The opening block does not own the logical row because the row may cross physical block boundaries. A changed marker, other discriminator, flag, or mode, null or out-of-range block index, or incomplete row rejects the candidate atomically.

A target-index row is `02 01 01 01 16, target:compact_index, ff ff 90 fe, index[3], 00 47 03, mode:u8, 01 c0 44 04 00`. The mode is `04` or `07`; the target and all three trailing indices are non-null in-range block ordinals in the same offset-only store. The row retains its ordered four-block lane, exact serialized index tokens, mode, token offsets, block containing its opening byte, and opening offset within that block. Physical block boundaries do not delimit the row or assign ownership. A changed marker, other mode, null or out-of-range block index, other lane arity, or incomplete row rejects the candidate atomically.

A complete column-index table begins with one mode-`07` linked row, continues with one or more target-index rows, and ends with one or more mode-`04` linked rows. Every target block ordinal across the three portions decreases by exactly one in source order. Every target-index row except the last has mode `07`; the last has mode `04`. All linked suffix rows have mode `04`. The complete table is retained atomically with its opening row, ordered target rows, ordered linked suffix rows, inclusive target interval, and opening source offset. A missing portion, other mode sequence, source-order reversal, target gap, store boundary, or entry mismatch rejects the table without assigning a semantic role to the target interval.

When an operation-header input and a slot in any of the three row grammars resolve to the same bounded data block, one common exact reuse relation retains the operation, header slot, row grammar and identity, row slot, shared block, and row-token offset. In the linked and target grammars the target is row slot zero and the three post-marker indices are slots one through three. A row belonging to exactly one complete composite table also retains that table identity; an ordinary index row or ambiguously repeated row has no table identity. Repeated row slots remain distinct uses. The relation does not assign a semantic operand or class role.

A feature input has a column target when exactly one linked or target-index row in a complete composite table addresses the input block in row slot zero. The relation retains the input, operation and header slot, row grammar and identity, composite table, target block and token offset, row mode, and all three post-marker field indices, resolved blocks, and token offsets. For a linked row it also retains the leading compact value and offset, discriminator, and flag; these fields are absent for a target-index row. Zero or multiple complete-table target rows, or a missing or duplicate grammar-specific row, leave the input without a column target. Trailing-lane and ordinary index-row reuse cannot establish a column target.

An offset-store object frame is `object_id:compact_index, 00 72 01 c0 20 02 01 c0 45 04 00 80 86 02 01 02 80 a4`. The compact index is non-null and uses the same direct and extended forms. Its value is a persistent object ID. The frame and discriminator lie within one bounded data block; non-overlapping frame order, exact compact-index token, and compact-index byte offset are retained.

A zero-prefixed offset-store control block begins with an ordered class-selection lane. Each word is `00, class_ordinal:u24 LE`; every ordinal indexes the store-local class registry and occurs once. The lane ends at the first out-of-range word, and every remaining control word is out of range. An empty lane, duplicate ordinal, or later in-range word rejects the class-selection lane atomically. Each retained ordinal resolves to its exact registered class definition and name.

A printable OM string value is framed as `66 32 03, declared_len:u8, text[declared_len-2], 00`. The text is non-empty printable ASCII. The marker, declared length, text, and null terminator lie within one externally bounded record.

A feature-history operation record begins at the fixed operation-header marker and ends at the next validated operation header or the record-area boundary. Its label is `03, declared_len:u8, printable_name[declared_len-2], 00`. The operation payload begins immediately after that null terminator and extends through the operation-record boundary. Payload strings use `04, declared_len:u8, utf8_text[declared_len-2], 00`; the text is non-empty valid UTF-8 and contains no control characters.

A `SKETCH` operation carries one ordered counted-reference field beginning `01 00, nonempty:u8`. When `nonempty` is one, `declared_count:u8` follows and is nonzero, followed by `declared_count - 1` contiguous indices. When `nonempty` is zero, the declared count is zero and no leading indices follow. The field then contains `00 00`, one terminal index, and `01 00 00 00`. Each index uses a canonical width marker: `f0, value:u8` represents `0..255`, while `f1, value:u16 BE` represents `256..65535`. Each reference retains the exact two- or three-byte index token and its width-marker offset. The indices address offset-only OM data blocks; resolution is retained only when one indexed store contains the addressed block.

A complete sketch construction-input record requires exactly one joined sketch operation record, a consistent declared count, contiguous reference ordinals, exactly `max(declared_count-1, 0)` leading member references, one final terminal reference, and unique data-block resolution for every reference. It retains the leading member lane and separated terminal reference as distinct ordered fields. Any missing, duplicated, inconsistent, noncontiguous, multiply terminal, or unresolved field is rejected atomically.

The logical sketch construction payload is the bytewise concatenation of the resolved leading member blocks followed by the resolved terminal block. Block boundaries do not delimit values or named-record boundaries. The payload retains its exact concatenated byte length and hash, ordered source-block identities, each block's payload offset and byte length, and each block's absolute source offset.

A sketch payload scalar field is `50 59 66, field_code:u8, 00, shifted_f64`. The shifted binary64 uses the extrusion shifted-IEEE transform. Each complete finite field retains its discriminator, decoded value, exact eight-byte encoding, payload-relative marker offset, and absolute source offset. The field frame does not assign a geometric or constraint role to the value.

A sketch fixed pair is `04 e0 48 0e 02 03 80 84, 30, first:i56 BE, 00, 30, second:i56 BE`. Each signed atom has value `i56 / 2^55`. The pair retains both decoded dimensionless values, both exact seven-byte two's-complement payloads, payload-relative offsets, and absolute source offsets. The frame does not assign a point, vector, plane, entity, or constraint role to either value.

A datum-coordinate-system fixed pair is `0b 02 03 01 03 01 c0 45 04 00 80 86 02 00 03, 30, first:i56 BE, 00, 30, second:i56 BE`. Each signed atom has value `i56 / 2^55`. The pair retains the datum operation and payload identities, exact discriminator, both decoded dimensionless values, both exact seven-byte two's-complement payloads, payload-relative offsets, and absolute source offsets. The frame does not assign an origin, axis, orientation, sketch coordinate, or constraint role to either value.

A sketch payload name field is `66, compact_type, 03, declared_len:u8, text[declared_len-2], 00`. The compact type is non-null; its decoded value, exact serialized token, payload offset, and absolute source offset are retained. At reconstructed payload offset zero, the type-free form is `03, declared_len:u8, text[declared_len-2], 00`; it has no compact type. In both forms text is nonempty printable ASCII. A complete name field opens a named payload interval ending exclusively at the next complete name field or the reconstructed payload boundary. Framed shifted-binary64 scalars and fixed pairs within that interval are retained independently in payload order. Bytes preceding the first complete name field remain outside named intervals.

A named payload interval whose name is exactly `Point` followed by a positive decimal ordinal is a sketch point when the interval contains exactly two framed scalar fields. The scalar order is the point's native two-dimensional coordinate order. The coordinate unit and model-space frame are not assigned by this record. A zero ordinal, nondecimal suffix, missing scalar, or additional scalar rejects the typed point atomically.

A named payload interval whose name is exactly `Point` followed by a positive decimal ordinal is a fixed-point record when it contains no shifted-binary64 scalar and exactly one fixed pair. The fixed pair supplies two ordered dimensionless values. The record retains the name, pair identity, decoded values, and source offset without assigning a sketch-coordinate, model-coordinate, annotation, or constraint role. A shifted scalar, zero or multiple fixed pairs, or an invalid point suffix rejects this fixed-point form atomically.

All same-name sketch points in one `SKETCH` operation form one point group when their two coordinate values are bit-identical in order. The group retains every point record in payload order and the common coordinates. Any coordinate conflict rejects the group atomically without rejecting the individual point records.

An offset-store named point object begins at a bounded data block whose offset zero carries the type-free `Point<positive decimal>` name frame. Its extent is the minimal consecutive-block span containing exactly two complete framed scalars and no second complete name. Zero or one scalar extends the span; a second name or a third scalar rejects the object. The object retains every block identity in the span, scalar order and values, exact shifted-binary64 encodings, and exact source offsets. The record assigns no sketch ownership, coordinate unit, or model-space frame.

A sketch named-point block use exists when one resolved reference in the sketch's counted field addresses a block in a typed named-point span. It retains the sketch reference and ordinal, named-point identity, shared block, and block position within the point span. The relation assigns no ownership when the reference field does not address the point span.

A sketch preceding-named-point use exists when one typed named-point span ends at block ordinal `N` and one sketch operation's complete reference lane begins at block ordinal `N+1` in the same offset store. Every reference resolves, all references carry the lane length as their declared count, ordinals are contiguous from zero, and only the final reference carries the terminal marker. Exactly one named-point span must satisfy the predecessor relation. The use retains the complete point block span, first sketch reference, and following construction block. A block gap, store mismatch, incomplete lane, or multiple predecessor points rejects the relation atomically. This relation binds the named point to the construction lane without assigning a sketch plane or geometric role.

One or more sketch named-point block uses and one reconstructed sketch-point group identify the same solved two-dimensional point when they belong to the same sketch operation and named-point object, their `Point<positive decimal>` names are identical, and the group's two scalar values are bit-identical to the named-point values in order. One identity relation retains the point group, the independently framed named point, and every block use and sketch reference in reference order. No identity is assigned when the reconstructed payload has no matching conflict-free point group.

A later `DATUM_CSYS` construction depends on a `SKETCH` operation when exactly one sketch-point identity addresses a named-point span related to the coordinate-system construction. The block relation is either one exact block shared by the point span and construction or a complete point span whose final block immediately precedes the construction's first block in the same offset store. The dependency retains the point-identity witness and the typed shared or consecutive block relation. The sketch must precede the coordinate-system operation. No dependency is assigned for zero, multiple, or later sketch candidates.

An `EXTRUDE` operation carries an ordered profile-reference field `01 02 16 01, count:u8, reference[count-1], 01 03 79`, with `count >= 2`. The payload may repeat the identical ordered encoded references as `01, count, reference[count-1], 00 00`; an exact unique repetition is retained as an independent witness of the list. Profile indices use the same canonical `f0` and `f1` widths, retain their exact tokens and offsets, and resolve against offset-only OM data blocks under the same uniqueness rule.

The extrusion payload begins `0f 00 00 01 00` followed by two shifted-IEEE scalars. A shifted-IEEE scalar occupies eight bytes: adding `0x10` to its first byte and retaining the following seven bytes verbatim produces one big-endian IEEE-754 binary64 value. The header retains both finite decoded values and both exact eight-byte encodings. Overflow of the first-byte addition and non-finite reconstructed values invalidate the scalar header atomically.

The extrusion payload terminal discriminator lane is `01 01 02, type_index[2], 01 03 02 01, flag[4], 00 00 00 29 29, trailing_index[n], 00`. Type and trailing indices use non-null compact-index encoding. The lane extends through the payload terminator and retains decoded indices, exact serialized index tokens, token offsets, modes, and flags only when exactly one complete lane exists. A null or truncated index, missing fixed marker, absent terminal zero, or second complete candidate rejects the lane atomically.

The three-scalar extrusion branch places `11` and three self-delimiting scalar atoms after its unique body-reference field. `00` is exact zero. Markers `20..3f` and `a0..bf` begin eight-byte binary64 atoms decoded by adding `0x10` to the marker. Markers `40..5f` and `c0..df` begin four-byte binary32 atoms decoded by subtracting `0x10` from the marker; the finite binary32 value is widened exactly to binary64. The three atoms retain their ordered values, width forms, exact serialized bytes, and source offsets.

The same three-scalar clause framing applies independently to every complete body-reference occurrence in any operation record: the body-reference terminator is followed by a one-byte branch discriminator and three self-delimiting scalar atoms. Each complete clause retains its body-reference occurrence order, body object index, discriminator, scalar values, width forms, exact scalar bytes, and source offsets. A body occurrence without three complete scalar atoms does not produce a scalar clause.

A branch-`11` body clause may continue with a wrapped member lane `01, count:u8, (2e, compact_index, 00)[count-1]`, where `count >= 2` and compact indices use the non-null compact-index form. The lane is atomic and retains body-reference occurrence order, member order, decoded index, exact compact-index token, and source offset.

For `TRIM BODY`, the branch-`11` member lane is followed by `01, 02, compact_index, 00, 00, 01, object_index, 00, 00`. The compact index and terminal object index are non-null. The continuation is atomic and retains the anchoring body index, continuation index, terminal object index, both exact index tokens, and their source offsets.

A branch-`11` or branch-`1c` body clause may continue after its three scalars with an unwrapped reference lane `01, count:u8, reference[count-1], 00, 00, 0b, 00`, where `count >= 2`. Every reference in one lane uses either non-null compact-index encoding or `f0`/`f1` payload object-index encoding; encodings are not mixed. The indices address offset-only OM data blocks under the unique-resolution rule used by construction references. The lane is atomic and retains the body-reference occurrence, branch discriminator, encoding, ordered decoded indices, exact encoded index tokens, ordered resolved targets, and source offsets. A wrapped branch-`11` member lane begins with `2e` after its count and is disjoint from this form.

An `EXTRUDE` construction profile is complete when its witnessed profile-reference field has contiguous zero-based ordinals and it and one branch-`11` payload-object reference lane contain the same non-empty ordered object-index sequence and independently resolve to the same ordered offset-bounded data blocks. The construction profile retains the anchoring body index, ordered object indices, resolved blocks, and source offsets from both encodings. Missing, duplicated, gapped, ambiguous, differently ordered, differently resolved, or unresolved inputs reject the complete profile atomically.

A wrapped operation-body member is a body operand when its compact index differs from the anchoring body index and equals an object index present in an operation body-reference field or validated segment body-binding tuple. The operand retains its body clause, member order, serialized identity, matching segment bindings, and source offset. Other wrapped members retain only their native member representation.

A `SEW` operation projects as a neutral body-sew feature. Without a unique
primary-body field or without body operands, the body selection is unresolved.
With both fields, the primary body is the first participant and the remaining
participant order is the wrapped-member order. Every participant resolves to
neutral bodies only when all object identities have surviving segment body
bindings and their body sets are pairwise disjoint; otherwise the ordered
native object-index selection is retained
atomically. The operation record does not assign a gap tolerance, so the
neutral tolerance remains absent.

A `TRIM BODY` operation projects as a neutral body-trim feature. Without one
unambiguous primary body and one or more body operands, the target and tool-body
selections are unresolved. With those fields, the primary body is the target;
wrapped-member order defines the ordered tool-body selection. Target and tools
resolve atomically through surviving segment body bindings only when their
participant body sets are pairwise disjoint, and otherwise retain their native
object-index selections. The body clauses do not assign which side
is retained, so the neutral retained side is unresolved.

A `DELETE` operation with a primary-body field projects as a neutral
delete-selected-bodies feature. The field identifies the selected body, not an
output. A uniquely bound body image resolves the selection; an ambiguous body
binding retains the native selection. Without the primary-body field, the
record does not identify a body target and remains a native operation.

A `TEXT` operation with exactly two payload strings projects the first string as a text semantic annotation and the second as its font family. The owning ordered feature is a non-modeling annotation node and retains the operation record as its native identity.

Bodies named by validated segment binding tuples exist at the start of retained feature history. A `DELETE` primary-body field consumes the selected body image and never establishes a writer. A `SEW` or `TRIM BODY` body operand consumes that body image when the body's latest decoded writer precedes the operation. Boolean tool operands follow the same ordering rule. A later writer supersedes earlier consumption. Terminal body selection is applied only when every emitted partition has one unambiguous terminal status and at least one, but not every, emitted body remains terminal.

An `OFFSET` operation projects as a neutral surface-offset feature. Exactly one
segment-bound output image establishes a native support selection when its
region, shell, and face ownership graph contains at least one OFFSET_SURF and
every owned OFFSET_SURF construction carries the same bit-exact signed
distance. The distinct base-surface identities form that selection. Each base
surface resolves to a neutral face only when exactly one distinct face owns it.
The neutral distance is the carrier distance when every resolved face has
forward sense and its negation when every resolved face has reversed sense. A
missing or multiply owned base surface, or mixed resolved face senses, leaves
the neutral distance unresolved; missing or multiply owned base surfaces retain
only the native selection. Procedural surfaces in other bodies of the same
stream do not participate.

A `BLEND` operation with exactly one segment-bound output image projects when that body's region, shell, and face ownership graph contains at least one BLEND_SURF and every owned BLEND_SURF has a circular cross-section. Procedural surfaces in other bodies of the same stream do not participate. The output body's BLEND_SURF identities define the result set. When every construction has two distinct resolved supports, their unordered support graph is complete bipartite, and every support surface owns one unique distinct face, the operation projects as a face blend over the two deterministic graph partitions. Each connected component places its lowest surface identity in the first partition; a missing cross-pair, odd cycle, unresolved support, duplicate face, or non-unique surface-to-face relation leaves the input-edge selection unresolved and projects the operation as a fillet. When every construction has a finite nonzero constant radius with one common absolute bit pattern, that magnitude is the blend radius. Multiple constant magnitudes retain constant-law form without assigning one radius. Exclusively linear or curve-driven laws retain variable-law form. Mixed laws retain an unresolved radius form. An unbound output, an incomplete ownership graph, a body without BLEND_SURF, or any owned conic or polynomial blend cross-section leaves the operation native.

The `FACE_BLEND` operation label uses the same owned circular BLEND_SURF projection as `BLEND`. A complete bipartite support graph resolves its two face selections and radius law. When the owned result carriers resolve but their support graph does not, the operation remains a face blend with unresolved face selections and the resolved radius form. `BLEND` uses an unresolved edge-selected fillet for the same fallback.

The structured extrusion branch begins `32 00 00` after its unique body-reference field, followed by one shifted-IEEE binary64 scalar whose finite value and exact eight-byte encoding are retained. A counted fixed-width lane follows as `01, count:u8, (3d, extended_compact_index, 00)[count-1]`, where `count >= 2`. Each wrapped index uses exactly `80..fe, low:u8` and decodes as `(marker-80)*256+low`; direct and null forms are invalid in this lane. The complete fixed-width atoms and their exact source offsets are retained. Two counted compact-index lanes follow, each framed `01, count:u8, index[count-1]` with `count >= 2`. Compact indices use `00..7f` as direct values, `80..fe, low:u8` as `(marker-80)*256+low`, and `ff` as null; null is invalid in these lanes. Both lanes retain their decoded indices, exact tokens, and exact source offsets. Indices in all three lanes address offset-only OM data blocks under the unique-resolution rule used by profile references. The branch ends `00 01, object_index, 00 00`, using the feature object-index form. The terminal object index retains its exact token and offset and equals the body object index anchoring the branch.

A complete structured-`32` extrusion construction requires exactly one self-witnessed structured branch, one non-empty profile-reference field with contiguous ordinals, and unique data-block resolution for every profile reference and every member of the branch's three index lanes. It retains the branch, body identity, ordered profile references, and the four resolved block lanes without assigning unresolved semantic roles to the three branch lanes. A duplicate branch rejects the construction atomically.

An `EXTRUDE` operation projects as a neutral extrusion. Exactly one complete construction profile supplies its native profile identity; no complete profile or simultaneous branch-`11` and structured-`32` profiles leave the profile unresolved. When the primary body has no preceding writer, resolves to exactly one transferred solid or sheet output, and the extrusion is its first writer, the Boolean mode is new-body. Wire, mixed-dimensional, multiple, absent, and later writer states leave the Boolean mode unresolved. The scalar lanes do not assign direction, termination, or draft, so those fields remain unresolved.

A `BLOCK` payload begins `control:u8, 00 00 01 00 00`, eighteen contiguous canonical payload references, `01`, one terminal canonical payload reference, eleven `ff` bytes, and four zero bytes. A canonical payload reference is `f0, value:u8` for `0..255` or `f1, value:u16 BE` for `256..65535`; noncanonical widths invalidate the complete field. The nineteen ordered references retain their exact tokens and offsets and address offset-only OM data blocks under the uniqueness rule used by sketch and extrusion profile references. The control byte is retained independently of the ordered reference lane.

The logical `BLOCK` construction payload is the bytewise concatenation of all eighteen resolved member blocks followed by the resolved terminal block. Fields may cross source-block boundaries. The reconstructed payload retains its exact length and hash, ordered block identities, payload-relative block starts, exact block lengths, and absolute source offsets.

Within the reconstructed `BLOCK` construction payload, shifted-binary64 scalar fields and compact-code name fields use the sketch payload field frames, including exact scalar encodings. A complete name field opens an interval ending at the next complete name or the payload boundary, and retains every complete scalar field in that interval in payload order. A `Point<positive decimal>` interval containing exactly two scalar fields is a typed construction point with the two values in native coordinate order. The record assigns no placement frame or geometric role to the coordinates. Any other scalar count rejects the typed point atomically without rejecting the underlying name or scalar fields.

All same-name construction points in one `BLOCK` operation form one point group when their two coordinate values are bit-identical in order. The group retains every point record in payload order and the common coordinates. Any coordinate conflict rejects the group atomically without rejecting the individual point records.

A `BLOCK` operation parameter binding selects the first declaration of its dimension run. The run consists of exactly three consecutive, unqualified declarations `pN`, `p(N+1)`, and `p(N+2)` in one OM section's expression-record order. Each declaration resolves uniquely to one finite millimeter expression in one shared expression table in that section. The typed dimension set retains every anchor binding, the three ordered declarations and expression records, and the three values in model millimeters. A section or expression-table boundary, nonconsecutive name or index, ambiguity, non-length unit, or unevaluated value rejects the complete dimension set.

The owning `BLOCK` feature links the complete typed dimension set and construction independently. Dimension order is native parameter order. Placement requires that complete ordered dimension set. The placement body is the feature's exactly one transferred solid output, or, when the feature has no transferred output relation, the document's exactly one connected solid body. Its complete ownership graph contains exactly one region and one shell. The shell's planar surfaces form exactly three mutually orthogonal normal bands, every band has two distinct extrema, and exactly one permutation of the three extrema separations matches the ordered dimensions within the document linear tolerance. Owned non-planar faces do not participate; this preserves the primitive frame after later curved operations modify the terminal body. Faces outside the placement body do not participate. Each normal is initially oriented so its first non-zero model-space component is positive. After dimension ordering, the third axis and its offsets are reversed when required for a right-handed frame. The three resulting minimum plane offsets define the origin. These ordered axes and origin form the local-to-model transform. Missing dimensions, multiple explicit outputs, multiple candidate fallback bodies, a non-solid body, multiple regions or shells, an incomplete ownership graph, additional owned plane directions, non-orthogonal bands, missing extrema, zero or multiple dimension-to-band permutations, or a separation outside the document linear tolerance leave placement absent.

A `BLOCK` operation projects as a neutral rectangular block. A complete typed
dimension set supplies its ordered local x, y, and z dimensions. The feature
source content references all three exact expression parameters once in
dimension order after any independently positioned payload content; an
expression already present through a direct input binding is not duplicated.
Each parameter names the dimension-set identity and owning block feature as a
consumer. The block depends on every equations feature owning those dimensions
even when no direct operation-input binding repeats the expression. Placement
transfers only when the planar extents uniquely map to that ordered dimension
set; absent placement does not imply the identity transform.

A complete block construction requires nineteen contiguous reference ordinals, one uniform control byte, exactly eighteen nonterminal members, one final terminal reference, and unique data-block resolution for every reference. It retains the member lane and terminal reference as distinct fields. Missing, reordered, differently controlled, incorrectly terminated, or unresolved inputs reject the construction atomically.

A body-reference field is `01 02 10, object_index, ff`. `object_index` uses the feature object-index form: `00..7f` is direct, `80..8f` contributes the high index byte and is followed by one low byte, `90` is followed by a big-endian `u16`, and `ff` is null. Every complete non-null field in a bounded operation record is retained in byte order. Exactly one field identifies an unambiguous primary-body writer; records containing zero or multiple fields do not establish that writer role.

An object-ID-bounded record in a section declaring `UGS::EXP_expression` declares a parameter name as `04, declared_len:u8, name[declared_len-2], 00`. `name` is `p`, one or more decimal digits, and an optional underscore-prefixed qualifier composed of ASCII letters, digits, and underscores. A declaration record contains exactly one such name frame. The parameter index is the decimal integer after `p`. The record may contain one additional frame with the same framing whose text is a context-free constant numeric expression; this is the declaration-local literal. Multiple numeric-expression frames make the declaration literal ambiguous without invalidating the parameter declaration. An exact unique name match binds the declaration to the value record carrying `(Number [mm|degrees]) name: expression; `.

An offset-only OM data block references a persistent OM object as `04 00, object_index, 02 0b`, using the same object-index form as feature operation headers. Complete fields retain their decoded object ID, exact serialized token, and block byte order. An object ID resolves to a target record or parameter declaration only when exactly one record with that ID occurs in the same directory entry.

An operation input slot depends on every uniquely resolved parameter declaration referenced by its target data block. Binding order is operation-header slot order followed by reference byte order within each block. When exactly one numeric-expression record names the declaration, the consumption edge also identifies that expression record. The binding establishes parameter consumption but does not assign a dimensional role to the parameter.

All binding occurrences for one operation and one expression form one parameter
use. Its occurrences retain ascending source-offset order, including repeated
slots. A feature's ordered source content merges its payload strings and every
parameter-use occurrence by absolute source offset; repeated consumption emits
the same parameter identity at each serialized position.
Each expression table is one equations feature; equations features follow their first physical expression-record offsets and precede operation features. Within an acyclic table, neutral parameter order is a stable dependency order with physical record order as its tie-breaker. A cyclic table retains exact formula text without neutral dependency edges. An operation depends once on each equations feature owning a parameter in its final ordered source content, in first-content-use order. Parameter consumer metadata follows the first physical use occurrence.

A source-native feature definition retains every resolved parameter use as `parameter_name -> exact_expression_text` when all referenced expression records exist and their names are unique within the operation. A missing expression record or duplicate parameter name rejects the definition-level parameter map atomically without rejecting the exact parameter-use relations. Typed neutral feature definitions retain their operation-specific parameter fields instead of this source-native map.

The `Container` label identifies an object-model grouping record, not a modeling operation. Its bounded native record and payload remain retained, and it does not create a neutral feature. Later feature ordinals retain their physical label positions.

The `SIMPLE HOLE` payload template is the unique payload string beginning `Hole_` and is underscore-delimited. `Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer` identifies a general simple hole extending through all material, with chamfer treatments at its entry and exit. The six tokens form one atomic template; a missing or additional `Hole_` string or missing, reordered, or unknown token does not produce a typed hole template. Neutral projection retains through-all extent and the typed native template retains both chamfer tokens.

The operation labels `HOLE PACKAGE`, `RIB`, `CHAMFER`, and `THICKEN_SHEET` identify their corresponding construction families. Neutral projection preserves the family as a hole, rib, edge chamfer, or face-thickening operation. Undeclared operands, sidedness, draft, Boolean state, and dimensions outside the rules below remain unresolved.

A `THICKEN_SHEET` operation with exactly one solid output body has a resolved thickness when that body's complete topology owns one or more `OFFSET_SURF` carriers in one of two forms. In the one-sided form every owned carrier has the same bit-exact finite nonzero signed distance; the absolute distance is the thickness. For resolved support faces, positive distance is forward on a forward-sense face and reverse on a reversed-sense face; negative distance reverses that mapping. The neutral side resolves only when every selected face produces the same result. In the symmetric form the owned carriers divide into positive and negative distances with one common absolute bit pattern and identical nonempty sets of support surfaces; twice the absolute distance is the thickness and the side is both. Other distinct signed distances or unequal positive and negative support sets leave the thickness unresolved. The selected support surfaces retain their native identities. They resolve to neutral faces only under the same one-support-to-one-distinct-face ownership rule as `OFFSET`; otherwise the selection remains native.

The operation labels `Pattern Feature`, `Pattern Geometry`, and `Geometry Instance` identify repetition constructions. Neutral projection preserves an unresolved pattern with an unresolved seed selection until its input records supply the pattern form, transform, and seeds.

`Pattern Feature` and `Pattern Geometry` payloads contain at most one ten-slot construction-reference graph. Its framing is `61, reference[0], ff 00 ff 01, reference[1], reference[2], 61, reference[3], ff 00 ff 01, reference[4], reference[5], ff 62, reference[6], reference[7], ff 00 00 01, reference[8], nullable-reference[9], ff ff 01`. A null terminal slot is one `ff` byte. `Geometry Instance` payloads contain at most one reference field framed as `00 ff ff, reference, 01 02 00 ff ff ff ff ff ff ff ff ff 00 00 00 01 02`. Non-null references use the canonical object-index encoding and unique-store resolution rule and retain their exact tokens and offsets. Missing fixed bytes, null required slots, noncanonical references, or multiple complete fields reject the entire field without rejecting the bounded operation record.

When the nine required graph references and optional non-null terminal reference resolve to one offset store, bytewise concatenation of their blocks in slot order is the logical pattern construction payload. Physical block boundaries do not delimit its fields. The payload retains its exact operation family, length and hash, ordered non-null references and block identities, payload-relative block starts, exact block lengths, and absolute source offsets. An incomplete, out-of-order, unresolved, or cross-store graph remains unreconstructed. Complete canonical `66 32 03` printable-string frames in the logical payload retain their values, payload-relative offsets, and exact absolute source offsets across block boundaries.

The logical pattern construction payload also uses the signed Q1.55 lane defined for draft construction graphs. Each complete lane retains its ordered decoded values, exact atom markers and raw values, payload-relative discriminator and atom offsets, and absolute source offsets across block boundaries. The framing does not assign transform, coordinate, or dimensional roles to these values.

`Pattern Feature` and `Pattern Geometry` payloads contain at most one counted transform lane `01, count, row[count-1], 5f 00 00 01`, where `count >= 2`. A `Pattern Feature` row is `60 01 00 00, shifted-f32, 00 01 00 00 00 00 01 00 00 00 00 01 01 03, selector, 01, ordinal, 00 00 ff 00 00`. A `Pattern Geometry` row is `60 01 00 00 00 00 01 00, shifted-f64, 00 00 01 00 00 00 00 01 01 03, selector, 01, ordinal, 00 00 ff 00 00`. Each selector is a non-null compact index and row ordinals are exactly `1..count-1`. All row scalars are finite and use one width selected by the operation family. The lane retains its count, encoding, exact scalar bytes and offsets, decoded values, selectors, exact selector tokens and offsets, and source offset. A malformed count, mixed or non-finite scalar encoding, null selector, incorrect ordinal, incomplete row or terminator, or multiple complete lanes rejects the lane atomically. Scalar and selector roles are not assigned by this framing.

The `POINT` operation payload begins with one construction header. Its exact framing is `72 00 00 01 00 00 00, reference, 00 ff ff ff ff ff ff ff ff ff 00 00 00 00 00 00 00 00 00 00 00 00 0d 01 02 01 00 00 00 89 02 01 01 01 00 a5 57 95 01 00 00 ff, mode, c0 1f ff fd 01 00 00 01 01 01 03 02 01 01 01 00 00 00 00 00`, where `mode` is `02` or `03`. The reference uses the canonical object-index encoding and unique-store resolution rule and retains its exact token and offset. A wrong operation family, malformed fixed byte, unsupported mode, null or noncanonical reference, or incomplete suffix rejects the header atomically without rejecting the bounded operation record. Bytes following the header belong to separate point-construction fields. The operation label identifies a datum-point family whose model-space position remains unresolved.

The header reference selects a six-scalar lane ending in its addressed offset-store block. The lane begins with the final three bytes of the immediately preceding block, continues through the first 45 bytes of the target block, and contains six consecutive shifted binary64 scalars. The target-block suffix at offsets `45..64` is `00 25 25 41 00 04 01 07 01 c0 45 10 00 80 86 02 00 01 00`. Both block identities, all six finite values, their exact eight-byte encodings, and their exact source offsets are retained. A missing predecessor, malformed suffix, truncated lane, non-finite scalar, or non-unique target store rejects the lane atomically.

The `DRAFT` operation payload begins `67 00 00 01 00 2f a4 7a e1 47 ae 14 7b 03`, eight `ff` bytes, and a counted compact-index lane. The lane is `01, declared_count:u8, compact_index[declared_count-1]`, where `declared_count >= 2`; every index is non-null and the following bytes are `01 02`. The count, ordered values, exact compact-index tokens, and exact token offsets are retained. When exactly one offset store contains every leading index and all four references in the construction graph, the leading indices resolve atomically to blocks in that store. No common store, multiple common stores, or an out-of-range leading index leaves the complete block lane unresolved without rejecting the numeric lane. A malformed count, null or truncated index, or missing terminator rejects the numeric lane atomically.

The bytewise concatenation of the resolved leading blocks in lane order is the logical draft construction payload. Physical block boundaries do not delimit its fields. The payload retains its exact length and hash, ordered block identities, payload-relative block starts, exact block lengths, and absolute source offsets.

A complete draft construction identity frame is either `41, compact_index, f0, compact_index, branch, 01, identity, 3f`, where the first index is non-null and `branch` is `02` or `03`, or `41 f0, compact_index, ff 02 01, identity, 3f`. The identity is one or more lowercase hexadecimal digits. Frames are decoded across physical block boundaries and retain their typed prefix form and index values, exact prefix bytes, identity, payload-relative offsets, and absolute source offsets. An incomplete prefix, null first index, unsupported branch, empty or non-lowercase-hexadecimal identity, or missing terminator does not form a frame.

The same payload contains exactly one four-reference construction graph. The graph is `01 02, reference[0], 01 02, reference[1], 68 2f 70 62 4d d2 f1 a9 fc 03 50 44 00 00 01 46 8a 2a 01 a3 60 10 01 01 01 04 02 01 02 01 00 00 00 00 01, reference[2], ff 00 00 00, reference[3], ff`. References use the canonical object-index encoding and unique-store resolution rule and retain their exact tokens and offsets. Missing fixed bytes, a null or noncanonical reference, an incomplete graph, or multiple complete graphs reject the field atomically without rejecting the bounded operation record. The operation label identifies the draft family, while its construction operands and angle remain unresolved in neutral projection.

When the counted leading lane and all four graph references select one offset store, bytewise concatenation of the four referenced blocks in graph order is the logical draft construction graph payload. Physical block boundaries do not delimit its fields. The graph payload retains its exact length and hash, ordered construction references and block identities, payload-relative block starts, exact block lengths, and absolute source offsets. An incomplete, out-of-order, or cross-store graph remains unreconstructed.

The draft construction graph payload contains zero or more signed Q1.55 lanes. A lane is `25 25 41 00 04 01 07 01 c0 45 10 00 80 86 02 00 01 00, atom+, 00`; each atom is `marker, raw[7]`, where `marker` is `30` or `b0` and `raw` is a signed two's-complement Q1.55 value. The lane retains ordered decoded values, exact markers and raw values, payload-relative offsets, and absolute source offsets. An empty lane, unsupported marker, truncated atom, or missing zero terminator rejects that lane atomically.

The same graph payload contains zero or more shifted-IEEE binary32 lanes. A lane begins with either `90 18 45 01 04 01 04 01 c0 45 04 04 80 86 02 00 03 00` or `90 18 45 01 04 01 03 01 c0 45 04 00 80 86 02 00 03 00`, followed by one or more shifted-binary32 scalar atoms and a zero terminator. The lane retains its exact `04` or `03` branch and discriminator, ordered finite values and raw encodings, payload-relative offsets, and absolute source offsets. An empty lane, unsupported or nonfinite scalar encoding, truncated scalar, or missing zero terminator rejects that lane atomically.

Canonical `66 32 03, declared_length, printable_text, 00` string frames are decoded over the complete draft construction graph payload. Each string retains its exact value, payload order and offset, graph-payload ownership, and absolute source offset. Physical block boundaries do not delimit the string scan.

The payload ends with one terminal lane: `extended_compact_index[0], extended_compact_index[1], 01 03 02 01 02 01 01 01 00 00 00, tail[3], 00`. Each index uses the two-byte `80..fe, low:u8` form. The ordered indices, exact two-byte tokens, their exact offsets, and the three uninterpreted tail bytes are retained. A direct, null, or truncated index, malformed fixed byte, missing terminal zero, trailing byte, or multiple complete end-anchored parses rejects the lane atomically.

The operation labels `CPROJ` and `CPROJ_CMB` identify projected-curve constructions. Neutral projection retains unresolved source-path, target-face, direction, and directionality fields.

A `CPROJ` payload contains at most one construction-reference field framed as `01 02, reference, reference, 80 57 00 02 01, reference, ff 01 02 02 7d`. Each reference uses the canonical `f0 + u8` or `f1 + u16 BE` object-index form, retains its exact token and offset, and resolves only when exactly one offset-only store contains the addressed block. The three references retain serialized order. A missing suffix, null or noncanonical reference, or multiple complete fields rejects the field atomically without rejecting the bounded operation record.

A `CPROJ_CMB` payload contains at most one construction-reference graph framed as `3c 32 01 02 32 01 04 36 01 33, reference[0], 33, reference[1], 00, reference[2], 00 00 00 00 00 00, reference[3]`, followed by two branch lanes and one tail. Branch lane `i` is `16 01 02, reference[i], 01 02 00 00 00 00 00 ff 01 02, reference[4+i], 00 81 5c`; its repeated reference must equal anchor `i`. The tail is `ff 01 ff 01, reference[6], reference[7], 04 02`. The eight non-repeated references retain graph order and use the same canonical encoding and unique-store resolution rule as `CPROJ`. A mismatched repeated anchor, missing fixed byte, null or noncanonical reference, or multiple complete graphs rejects the graph atomically without rejecting the bounded operation record.

When every non-repeated reference resolves in one offset store, bytewise concatenation of the three `CPROJ` blocks or eight `CPROJ_CMB` blocks in reference order is the logical projected-curve construction payload. Physical block boundaries do not delimit payload fields. The payload retains its exact length and hash, operation family, ordered construction references and block identities, payload-relative block starts, exact block lengths, and absolute source offsets. An incomplete, out-of-order, unresolved, or cross-store field remains unreconstructed.

Canonical `66 32 03, declared_length, printable_text, 00` string frames are decoded over the complete projected-curve construction payload. Each string retains its exact value, payload order and offset, construction-payload ownership, and absolute source offset. Physical block boundaries do not delimit the string scan.

The `TRIMMED_SH` operation label identifies a surface-trim construction. Its
selected input faces, trim path, and retained region remain unresolved. Output
face ownership does not identify the pre-operation face selection.

The `EXTEND_SHEET` operation label identifies a surface-extension construction.
Its selected input faces, distance, and continuation law remain unresolved.
Output face ownership does not identify the pre-operation face selection.

The `EXTRACT_BODY` operation label identifies a body-extraction construction. Neutral projection retains an unresolved source-body selection independently of the operation's output bodies.

The `SKIN` operation label identifies a loft-family skin construction. Its section identities and result controls remain unresolved until the corresponding parameter fields are assigned.

`SKIN` and `Studio Surface` payloads share one exact common construction-reference envelope. Its header is `discriminator, 00 00 01 00, reference[0..2], 01 09, header[8], 01 09, reference[3..10]`; `SKIN` discriminators are `3e` and `3f`, while the `Studio Surface` discriminator is `14`. The same bounded payload contains exactly one trailing lane framed as `03 03 2f a4 7a e1 47 ae 14 7b, reference[11..13], 01 01 ff ff ff ff ff ff ff ff ff 00 00 00 00 01 02`. All references use the canonical object-index encoding and unique-store resolution rule and retain their exact tokens and offsets. An invalid discriminator, malformed header, absent or repeated trailing lane, null or noncanonical reference, or incomplete suffix rejects the common envelope atomically without rejecting the bounded operation record.

When all fourteen common-envelope references resolve to one offset store, bytewise concatenation of their blocks in reference order is the logical surface construction payload. Physical block boundaries do not delimit its fields. The payload retains its exact length and hash, ordered construction references and block identities, payload-relative block starts, exact block lengths, and absolute source offsets. An incomplete, out-of-order, unresolved, or cross-store graph remains unreconstructed.

The logical surface construction payload uses the common object-payload scalar-pair frame `08 02 03 01, branch, c0 45 04 00 80 86 02 00 03, shifted-f64, 00, shifted-f64`, where `branch` is `03 01` or `81 02 01`. Each complete occurrence is retained in payload order. Both values are finite. The typed frame preserves its owning logical payload, exact discriminator including the branch, both exact scalar encodings, payload-relative discriminator and scalar offsets, and their exact absolute source offsets across source-block boundaries. An incomplete or non-finite frame is rejected atomically.

A surface-referenced printable string frame is `66 1b 03, byte-length, UTF-8[byte-length], 00`. The value is nonempty and contains no control characters. An incomplete frame, invalid UTF-8, empty value, control character, or absent terminator rejects the frame atomically.

The payload contains at most one counted branch group beginning `a0 5a, family, header-code, 01, branch-count`. Supported family bytes are `14` and `50`. Each branch is `mode, 01, count, member-reference[count-1], count-witness?, zero-lane, ff 01 02, terminal-reference, 00, suffix[1..5]`, where `mode` is `16` or `40` and `count >= 2`. Member and terminal references retain their exact canonical object-index tokens and offsets. A count witness is `01, count`; its zero lane contains `count + 3` bytes, while an unwitnessed branch contains five zero bytes. The suffix is retained byte-exact and is bounded by the uniquely parsed next branch. The final suffix is followed by `00 00 00 01 03 00 00 00 ff ff 01` for `SKIN` or `00 00 00 00 00 ff ff 01` for `Studio Surface`. The declared branch count must produce exactly one complete parse; ambiguity, a malformed count or witness, a null or noncanonical reference, or a missing family terminator rejects the group atomically.

The `Studio Surface` operation label identifies a freeform-surface construction. Its control geometry and construction controls remain unresolved until the corresponding parameter fields are assigned.

A `TEXT` operation whose payload contains exactly two ordered strings projects one semantic text annotation. The first string is the visible text and the second string is the font family. The annotation object and native reference are the owning neutral feature and operation label. Any other payload cardinality remains native-only.

One complete construction-identity group projects hole diameters when its operation set equals the complete typed through-hole template set. Without a construction group, the complete set of distinct typed through-hole operations uses the same rule. Every operation either resolves through its primary-body field to exactly one transferred output body, or every operation lacks that relation and the document contains exactly one connected solid body. Mixed resolved and unresolved ownership or more than one candidate connected solid rejects the projection. Operations are partitioned by equal output-body identity. Each output body is solid and owns exactly one region containing exactly one shell. For each partition, that shell's complete face ownership graph contains exactly one reversed two-loop cylindrical face per operation. Every coedge in both loops resolves to a circular carrier whose radius equals the cylinder radius within the document linear tolerance, whose axis is parallel to the cylinder axis within the angular tolerance, and whose center lies on the cylinder axis within the linear tolerance. All carriers in one loop occupy one axial station within the linear tolerance, and the two loop stations differ by more than that tolerance. Every cylinder radius in the partition is finite, positive, and bitwise equal. Every operation in the partition receives twice that radius. Distinct output bodies may carry distinct diameters. This is permutation-invariant for operations sharing one output image. Multiple construction groups, unmatched templates or owned cylinders, a non-through template, duplicate operation identity, missing or ambiguous output ownership, a sheet or structurally disconnected output, an incomplete topology ownership graph, an empty, noncircular, off-axis, coincident, or differently bounded cylinder loop, or differing radii within one output partition leave every diameter absent atomically.

The same complete operation-to-through-bore cardinality and body partition supplies a model-space drilling direction when every cylinder axis in one output-body partition is mutually parallel within the document angular tolerance. Cylinder radii may differ because the common direction is invariant under every operation-to-bore correspondence even when diameter ownership is ambiguous. Each axis is normalized and sign-canonicalized so its first component exceeding the angular tolerance is positive. Every operation in the partition receives that common direction; distinct output bodies may carry distinct directions. A missing, non-unitizable, or nonparallel axis leaves every direction absent atomically. The canonical sign is a geometric gauge for a through-all operation and does not assign an entry face or an operation-to-bore position correspondence.

A body partition containing exactly one hole operation and exactly one validated through-bore cylinder supplies the hole's model-space axis position. The position is the point on the cylinder axis closest to the model origin: for normalized axis `a` and serialized cylinder origin `o`, `p = o - dot(o, a) a`. This gauge is invariant under an arbitrary axial shift of `o`. A partition containing multiple operations or multiple bores has no operation-to-axis position correspondence and supplies no positions. Position projection does not assign an entry face or drilling-direction sign.

The complete set of `HOLE PACKAGE` operations uses the same output-body partition and through-bore bijection rule without requiring a simple-hole payload template. Every package operation receives the diameter of its body partition. A duplicate operation identity or any incomplete partition leaves all package diameters unresolved atomically.

Entry and exit chamfer dimensions project when every typed through-hole operation requests both chamfers and has the same explicit-output or unique-connected-solid body resolution used for bore diameters. Operations are partitioned by output-body identity. In each partition, every owned matched bore cylinder has exactly two reversed coaxial conical faces and no additional owned candidate cone remains. Cones outside those output-body ownership graphs do not participate. Each conical face has two circular boundary carriers: the inner radius equals the partition's common bore radius bitwise and the outer radius is larger. All outer radii in one partition agree within the document linear tolerance and all cone half-angles in that partition agree within the document angular tolerance. Their arithmetic means define that partition's canonical outer radius and included angle. The neutral chamfer diameter is twice the canonical outer radius and its included angle is twice the canonical cone half-angle. Distinct output bodies may carry distinct treatments. Missing or ambiguous output ownership, an incomplete topology ownership graph or boundary, ambiguous bore axis, unequal treatment within one output partition, noncircular boundary, or unmatched owned cone rejects every chamfer dimension atomically.

Before its unique `Hole_` template string, a `SIMPLE HOLE` payload may carry an even nonzero count of marker-`30` shifted-binary64 scalars. The second half must be byte-identical to the first half in order. The payload retains the first half as one ordered scalar lane with finite decoded values and exact eight-byte encodings, and retains the absolute offset of every scalar in both witnesses. An odd or zero count or any unequal aligned encoding rejects the repeated lane atomically. No unit, coordinate frame, or geometric role is assigned to these values.

Each complete scalar-lane witness is followed immediately by two tagged object indices. `f0,lo` encodes an ordinal below 256. `f1,hi,lo` encodes a big-endian ordinal of at least 256. Both reference pairs address blocks by direct ordinal in the offset store that owns the operation-header input blocks. The four indices resolve atomically: the operation inputs must select one store and every addressed ordinal must exist in that store. The first and repeated reference pairs retain their order independently.

Two or more `SIMPLE HOLE` operations belong to one construction-identity group when their resolved first-witness block pair and repeated-witness block pair are equal in order. The group retains the shared four-block identity and aligns operation labels, scalar lanes, and block-reference lanes in feature-history order. Each operation contributes exactly one scalar lane and one block-reference lane; a duplicate operation identity or lane rejects the group. A different block in any position prevents the join. The equality assigns no parent, dependency, placement, or scalar role.

A `DATUM_CSYS` payload begins `control:u8, 00 00 01 00 00 01 01 00 01 00 00 00 00`, followed by exactly eight canonical `f0`/`f1` object indices and `01 01 00 01 00 00 00 00`. The control byte is retained independently. The eight indices retain their exact tokens and offsets and resolve atomically to blocks in the single offset store selected by the operation-header inputs. Their serialized order is retained. A missing, noncanonical, unresolved, differently stored, or incorrectly terminated reference rejects the complete coordinate-system construction lane.

The logical payload formed by the first two resolved `DATUM_CSYS` blocks uses the common `50 59 66, field_code, 00, shifted_f64` scalar frame. Complete finite fields retain their discriminator, value, exact eight-byte encoding, payload-relative offset, and exact file offset across the two source blocks. Their coordinate or dimensional role is not assigned by the frame.

The first two resolved datum-coordinate-system blocks form one logical object payload in serialized lane order. Their bytewise concatenation is authoritative: fields may cross the source-block boundary. The reconstructed payload retains its exact length and hash, both block identities, payload-relative block starts, exact block lengths, and absolute source offsets. The other six construction lanes remain independently bounded records.

The `DATUM_CSYS` operation label establishes a coordinate-system feature family. Its neutral construction remains unresolved until the payload fields establish a complete model-space frame.

An object-payload scalar-pair frame is `08 02 03 01, branch, c0 45 04 00 80 86 02 00 03, shifted-f64, 00, shifted-f64`, where `branch` is `03 01` or `81 02 01`. Each complete occurrence in a reconstructed datum-coordinate-system or sketch payload is retained in payload order. Both values are finite. The typed frame preserves its owning logical payload, exact discriminator including the branch, both exact scalar encodings, payload-relative discriminator and scalar offsets, and their exact absolute source offsets across source-block boundaries. A preceding `6d 00 f0` prefix belongs to the containing record and does not create a second pair.

Each sketch feature links its ordered typed coordinate-pair records by payload ordinal. Source-block boundaries do not delimit sketch entities and cannot assign coordinate ownership; a coordinate frame crossing a block boundary remains one field in the owning logical sketch payload.

Each of datum-coordinate-system construction lanes 5–7 is an independently bounded descriptor block. A typed block contains exactly one maximal run of 30–32 lowercase hexadecimal digits. Bytes before and after the identity remain exact prefix and suffix fields. The descriptor retains its construction lane, resolved block, identity, exact prefix and suffix, block offset, and identity offset. A block with no qualifying run or multiple qualifying runs remains untyped.

Equal typed descriptor identities join datum-plane and datum-coordinate-system constructions. The relation retains both typed descriptors, both operations, the shared identity, and the coordinate-system lane ordinal. Feature dependency follows serialized operation order: the later operation depends on the earlier operation. Identity equality does not impose a fixed plane-to-coordinate-system ownership direction.

Each resolved coordinate-system block is joined to every operation-header input addressing the identical store block. The relation retains the coordinate-system construction, reference ordinal, shared block, input binding, consuming operation, and input slot. Equal numeric indices in different stores do not join. No origin, axis, input, or output role follows from block equality alone.
When the coordinate-system construction precedes the consuming operation in the same ordered feature area, the consuming feature depends on the coordinate-system feature.

A `DATUM_PLANE` payload begins `control:u8, 00 00 01 00 01, declared_count:u8, branch_tag:u8, 01 02`, with `declared_count >= 2`. The control, count, and branch tag are retained independently. The branch tag selects the following construction grammar; the common header assigns no reference, plane-kind, origin, or normal role to branch bytes.

For branch tag `1b` or `23` with declared count two, the header is followed by one non-null compact descriptor index, `01`, one canonical `f0`/`f1` object index, and `00 14 02 00 01 00 00 00 00 ff ff 00`. The descriptor and object indices remain separate ordered fields, and both exact index tokens and offsets are retained. Both indices resolve atomically in the single offset store selected by the operation-header inputs; a missing, ambiguous, or differently stored target leaves both unresolved. The branch does not assign a plane-kind, origin, normal, or dependency role to either index.

Branch tag `29` carries two canonical object indices. With declared count two they are separated by `01 01 18 03 00 01 00 00 00 00 ff` and followed by `01`, nine `ff` bytes, twelve zero bytes, and `0d`. With declared count three they are separated by `01 01 3a 01 02` and followed by `01 17 02 00 01 00 00 00 00 ff ff 00`, nine `ff` bytes, twelve zero bytes, and `0d`. Both indices retain their exact tokens and offsets and resolve atomically under the same operation-selected-store rule. Their serialized order is retained without assigning plane-frame or dependency roles.

Branch tag `28` with declared count three carries one non-null compact descriptor index, `01 29 01 02`, one canonical object index, `01 01 07 02 00 00 00 00 00 00 ff ff 00`, nine `ff` bytes, twelve zero bytes, and `0d`. Its two indices use the same separate ordered lanes, exact descriptor-token retention, and atomic same-store resolution as the tag-`1b`/`23` form.

Each resolved datum-plane descriptor or object block is joined to every operation-header input addressing the identical store block. The relation retains the construction operation, lane kind, lane ordinal, shared block, consuming operation, and input slot. Equality across different offset stores does not join and the relation alone assigns no plane-frame role. When the datum-plane construction precedes the consuming operation in the same ordered feature area, the consuming feature depends on the datum-plane feature.

The logical datum-plane object payload is the bytewise concatenation of its resolved object blocks in serialized lane order. Block boundaries do not delimit fields. The payload retains its exact length and hash, ordered block identities, each block's payload offset and byte length, and each block's absolute source offset.

A terminal datum-plane object-index lane is `01, declared_count:u8, compact_index[declared_count-1], 00, trailer:u32 BE`, with `declared_count >= 2`. Every compact index is non-null and the trailer ends at the reconstructed payload boundary. A unique complete lane retains its payload offset, count, ordered values, exact serialized index tokens, value offsets, and trailer word. Truncation, null indices, trailing bytes, or multiple complete candidates leave the typed lane absent.

A datum-plane object scalar-pair frame is `6d 00 f0 08 02 03 01 03 01 c0 45 04 00 80 86 02 00 03, shifted-f64, 00, shifted-f64`. Each occurrence in the reconstructed logical payload is independent and ordered by payload offset. Both scalars are finite. The native record retains the frame offset, both scalar offsets, decoded values and exact encodings, and their exact absolute source offsets across source-block boundaries.

A datum-plane descriptor block is exactly 40 bytes: `lowercase_hex_identity, 3f 41, compact_schema_index, ff 02 01, printable_label`, where the identity and label are nonempty and the compact index is non-null. Descriptor references resolve within the operation-selected offset store. The typed descriptor retains its owning plane header, descriptor-lane ordinal, resolved block, identity, exact delimiter-prefixed suffix, schema index, label, and absolute block offset. Malformed framing or a non-40-byte block leaves the descriptor untyped.

The `DATUM_PLANE` operation label establishes a reference-plane feature family. Its neutral construction remains unresolved until the payload fields establish a complete model-space plane frame.

**Persistent-handle identity.** `e0 + handle:u32 BE` values are persistent handles forming a cross-stream bridge (RMFastLoad ↔ UG_PART OM ↔ EXTREFSTREAM). Equal handle values group their ordered distinct bounded OM records, offset-store control blocks, and indexed EXTREFSTREAM records under one native handle identity. A second family is a four-byte big-endian word whose high nibble is `0xC` and low 28 bits are the reference value. Both tokens remain within one externally bounded record and occur as `(e0-handle, c-ref)` pairs.

Each persistent-handle identity separately counts serialized occurrences in OM/control storage and EXTREFSTREAM storage. The external count includes every normalized handle-set prefix occurrence, its omitted closing duplicate when present, and every exact tail-pair occurrence.

**Same-section record references.** A counted reference run is `01, count:u8, (count - 1) × (90, record_ordinal:u16 BE)`, with `count >= 2`. Every ordinal addresses an entity record in the same external entity-index directory. The containing record depends on the addressed records; the addressed records have the containing record as a dependent. The complete run lies within one bounded record; any out-of-range ordinal invalidates the run atomically. Token order is operand order, and inverse dependent order follows containing-record ordinal.

### 7.2 Partition and deltas merge

A complex part contains current body images and historical or tool bodies, each with its own partition/deltas pair and stream-local xmt namespace, plus optional plain cached tool bodies. `RMFastLoad` object-ID membership identifies the current body images. Multiple decisively represented images are distinct current bodies. When membership does not distinguish current images from historical or tool bodies, the final body set requires the operand bindings and order encoded by NX OM feature-history records.

`/Root/part/attrs` is a versioned XML attribute table. Each `Attr` element
contains its owner token, UTF-8 title and value, schema type, PDM-ownership flag,
and record version. These part-level values transfer as document attributes;
the native record retains the remaining ownership and schema fields.

```text
live = partition ∪ delta_full − tombstones
```

- A full record with `xmt ∈ partition` replaces that partition record. Paired streams share one xmt namespace.
- A full record with `xmt ∉ partition` (high range) adds a new entity.
- The deltas stream adds entities through explicit high-range records.

BODY (`00 0c`, xmt=3) records delimit body revisions. `node_id` is a monotonic per-body revision counter. A partition containing a validated body-shape SHELL is the authoritative current topology image. BODY through REGION records in its paired deltas stream are revision history and do not replace or delete that topology image.

`RMFastLoad` stores the active object-id set alongside the partition and deltas body records. The membership table is a little-endian `count:u32` followed by exactly `count` ordered `object_id:u32` words. FACE, EDGE, and VERTEX `node_id` values share this identity space. Membership assigns each represented body image independently; the set may select more than one body. A body image without active membership is retained unless another image has a decisive membership assignment.

Across the ordered feature-history sections, the last non-`DELETE` operation carrying a primary-body field is that body object's latest writer. A segment-bound image exists before the retained operations when it has no decoded writer. The two body-object indices in a segment tuple are aliases for one body image and are interchangeable in writer and operand fields. Alias equality closes transitively across tuples that share either index; the smallest index canonically identifies the connected alias component. A `DELETE` consumes its primary body. A later Boolean consumes each tool image; a later `SEW` or `TRIM BODY` consumes each typed body operand. Consumption applies only when the image's latest writer precedes the consuming operation, and a still later writer supersedes it. Every segment binding receives one terminal or consumed lineage status when the complete ordered history resolves atomically. Terminal selection requires one status for every emitted partition image and retains at least one but fewer than all images; otherwise every emitted image remains retained.

A compact deltas tombstone is `type:u16 BE, xmt:u16 BE, 00 01`. Outside the authoritative partition topology families, a matching key deletes the partition record and a full record replaces it. Repeated events are chronological; the last full record or tombstone for one key is current. A deltas topology image is assembled only when its partition has no validated body-shape SHELL.

---

## 8. Units and tolerances

- Geometric doubles are meters; multiply by 1000 for mm. Applies to point coordinates, radii, offsets, tolerances, chart chords, TRIMMED_CURVE LINE parameters.
- Do not scale unit axes/directions/normals, `thumb_weight`, angular parameters (radians), UV surface parameters, knot values, or ratios.
- `chordal_error` defines the verification tolerance for chart-hosted procedural carriers.
- Exactness certificates for procedural geometry are floor bounds `max(1e-12, 128·eps·scale)` mm; the relations are zero in exact arithmetic (S0==S1 spine identity, envelope-of-spheres normal).

## 9. Additional record semantics

### 9.1 `EXTREFSTREAM`

An `EXTREFSTREAM` record region begins with `0x00`, followed by little-endian `(record_id, record_offset)` pairs terminated by a single `record_id == 0`. Record IDs are nonzero and unique, offsets are strictly increasing, the directory ends no later than the first record, and the last record ends at the trailing string table. Every indexed record transfers with its ID, exact byte boundary and digest. An empty indexed record is exactly `01 00 00 00 00 01`, optionally followed by a closing `01`; the form retains whether that marker is present. A handle-set record at `record_offset` begins `01 00 00 00`, then `n:u16 BE`, `01`, four `u32 LE` ID slots, `01`, `count:u8`, `count - 1` occurrences of `e0 + handle:u32 BE`, and a closing byte equal to `count`. Handles are strictly ascending except that the final occurrence may repeat the preceding handle; transfer records whether that closing duplicate is present and omits it from the normalized handle list. Within the remaining bounded tail, an adjacent pair `e0 + handle:u32 BE + tagged:u32 BE` is retained when the high nibble of `tagged` is `0xC`; the normalized tagged reference is its low 28 bits. Unpaired tokens and other tail structure remain opaque. Other indexed record layouts remain opaque. The trailing string table is `01 + count:u32 LE + count × (len:u16 LE + nonempty control-free UTF-8)`. The final string ends at the stream boundary. The nominal `16 + payload_size` boundary can fall inside a string record. Each string transfers with its table ordinal and absolute byte offset.

Each of the four ID slots indexes the trailing string table in the same `EXTREFSTREAM`. A complete record-to-string relation retains the record, slot, serialized index and byte offset, and target string identity. If any slot is out of range or its `(stream, ordinal)` target is not unique, none of the record's four relations transfer.

Slot zero names the child `.prt`, slot one is the reference code, slot two is the child directory, and slot three is the load-mode token. A record with exactly one complete ordered four-slot relation and a `.prt` child name binds its handle-set record to the slot-zero child name and slot-two directory. The code-token values remain uninterpreted.

### 9.2 Stream and deltas framing

The `00 ce` stream-root schema declares `index_map`, `node_id_index_map`, and `schema_embedding_map`; each serializes as a null or empty array and supplies no tombstone bridge.

A deltas-stream BODY record with type `00 0c` and xmt `3` delimits a body snapshot. Its `node_id` is a monotonic revision counter within that body sequence, and a reset begins another interleaved body sequence. Deltas streams encode null-node deletions as descending contiguous xmt runs that can span topology, geometry, and attribute record types.

### 9.3 B-spline payloads

A type-125 B-surface control payload stores a parameter-range block, a marker byte, a sense byte, `double_count:u32`, a large-index-capable `first_index`, and `double_count` doubles. An optional envelope escape before `double_count` shifts the remaining fields by one byte.

A type-126 B-surface descriptor stores U and V degrees, pole counts, form codes, distinct-knot counts, multiplicity references, knot references, and a control-payload reference. It has short and large-index layouts.

A type-135 B-curve control payload stores `double_count:u32`, `first_index`, and `double_count` doubles. Type 136 stores degree, pole count, dimension, distinct-knot count, form, control-data index, multiplicity reference, and knot reference.

The B-spline form code does not determine whether a control grid is rational. The control-grid stride determines the representation: stride 3 stores xyz and stride 4 stores xyzw.

### 9.4 Attributes and expressions

Parasolid attribute definitions use a two-record catalog entry. `00 4f [ff] name_len:u32 BE, class_xmt:u16 BE, name[name_len]` declares a non-empty printable ASCII class name; `ff` is the optional record-envelope escape. The field record follows immediately as `00 50, field_count:u32 BE, field_xmt:u16 BE, reference[2]:u16 BE, header_word[2]:u16 BE, payload`. Both XMT identities and the ordered references are stream-local. The header words are retained verbatim; their second value includes `2328`, `1f67`, and `1f44`. The header is followed by an exact 26-byte descriptor prefix and `field_count` one-byte field codes. This gives the type-80 record a self-contained length of `42 + field_count` bytes. A truncated descriptor or field-code lane invalidates the declaration pair atomically. Type code `0x05` in the descriptor prefix denotes a component/reference or string field, `0x06` a double field, and `0x00` a void or flag field. The per-field code lane remains ordered independently of the descriptor type code. The primary storage kind is typed only when descriptor bytes four and five are `03 00`, `03 05`, or `03 06`; another marker or type code leaves the storage kind absent without discarding the exact descriptor prefix.

A type-81 entity/attribute-list record is `00 51 [ff], flags:u32 BE,
xmt, sequence:u32 BE, discriminator:u16 BE, references`. XMT fields use the
compact or extended XMT encoding. `xmt` is non-null, `sequence` is nonzero, and
the low flags byte is in `1..=0x20`. The reference count is seven for
`(discriminator, low_flags) = (001d|001e, 02)`, nine for
`(0020|0024|0027, 04)`, and six otherwise, including
`(0018|0020|0025, 01)`. References are either consecutive XMT values or
individually `01`-prefixed XMT values followed by `00`; the two forms are
atomic. A topology attribute-list identity resolves only when exactly one
type-81 record in the same stream has that xmt.

The type-81 discriminator selects an attribute class when its value plus one
equals the XMT of exactly one type-79 attribute definition in the same stream.
Every matched instance retains the serialized discriminator, matched
definition XMT, type-81 instance, and type-79 definition independently of
topology ownership. A topology-owned matched instance additionally retains its
topology ownership relation. A missing, overflowing, or multiply declared
definition XMT leaves the class unresolved. Definition declaration order and
type-81 reference values do not participate in class selection.

A printable type-84 value record is `00 54 [ff], length:u32 BE, xmt,
text[length], 00`. The length is nonzero, xmt is non-null, and every text byte
is printable ASCII. The terminator is outside the declared text length. A
type-81 reference slot addresses the value only when exactly one type-84 record
in the same stream has the referenced xmt; reference order is retained.

A type-82 unsigned-integer value record is `00 52 [ff], count:u32 BE, xmt,
value[count]:u32 BE`. A type-83 numeric value record is `00 53 [ff], count:u32
BE, xmt, value[count]:f64 BE`. Counts are nonzero, XMT identities are non-null,
and every type-83 value is finite. Each record ends after its final value and
has no terminator. A type-81 reference slot resolves a numeric value record only
when exactly one type-82 or type-83 record in the same stream has the referenced
xmt; reference order and the value lane order are retained.

A shell, face, loop, edge, FIN, or vertex topology record with one uniquely resolved
attribute-list identity owns every uniquely resolved type-82, type-83, and
type-84 value referenced by that type-81 record. Each value record transfers as
one topology-targeted source attribute. Its name contains the value-record
family and zero-based type-81 reference ordinal; its values retain serialized
lane order. The independently resolved class relation identifies the owning
attribute definition, but a value receives a semantic field name only through
the class-specific field-value serialization.

When the class relation resolves, each neutral source-attribute name is the
exact class name followed by its type-82, type-83, or type-84 family and
zero-based type-81 reference ordinal. Without a resolved class it retains only
the family and ordinal. This qualification assigns class ownership without
assigning a declared field role.

`hostglobalvariables` stores numeric expressions as independently length-framed ASCII records:

```text
handle:u8  04  length:u8  "(Number [units]) name: expression; "  00
```

`length - 2` is the ASCII text length. `units` is `mm` or `degrees`; `name` contains ASCII alphanumerics and underscores. `expression` is a finite decimal scalar or formula. Context-free arithmetic uses parentheses, unary signs, `^`, `*`, `/`, `+`, and `-`. Formula parameter references use `p<decimal-index>` tokens. The nearest preceding `hostglobalvariables` marker supplies table identity. The record framing is independent of the OM entity-index and object-ID arrays. An enclosing indexed entity supplies persistent object identity when present; otherwise the record's entry-relative byte offset supplies identity.
