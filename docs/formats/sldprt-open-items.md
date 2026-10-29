# SolidWorks `.sldprt`: Open Items

This document lists the parts of the SolidWorks `.sldprt` format that we do not know. The specification `sldprt.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Body classification

### BC-04. Duplicate topology record identities

**Question.** Which occurrence is authoritative when one stream contains multiple valid topology records with the same attribute but different references or coordinates?

**Known.** Topology records are keyed by stream-local attribute. A complete short-index FACE includes five pointers after its sense. Complete EDGE and VERTEX bodies include their trailing owner pointers. POINT has exactly four pointers before its coordinate vector. Equal projected fields do not establish equality of complete record bytes. The specification does not define a duplicate-record rule or a native precedence field.

**Need.** We must select the authoritative topology record to avoid silently changing body connectivity or vertex positions when a duplicate identity is present.

### BC-08. Complete identity in single-face component selections

**Question.** Which single-face component-selection field, if any, supplies the complete ordered tail of a persistent face identity?

**Known.** Complete `FACE_ID_2001` and DisplayFace identities include tail length and ordered values. A source/local pair can name several distinct complete identities. A single-face selector carrying only that pair requires one distinct target. Generated-hole surface references expand the pair to a set and do not select one persistent face. A component selection has an ordered path of type signatures and optional local identifiers; path length and earlier local identifiers do not establish a persistent-identity tail.

**Need.** An explicit native field relation must distinguish a complete selector from a prefix-only selector before a colliding source/local pair can select one face.

### BC-09. Dummy-FIN partner corroboration

**Question.** Which native field corroborates the ring-order endpoints of an unwalked boundary-dummy canonical FIN before the dummy joins a face loop?

**Known.** `decode.rs` withholds an edge through `TopologyFinPartnerUnresolved` when an unwalked boundary-dummy canonical FIN lacks reciprocal partner corroboration of both ring-order endpoints, or when a mutual twin FIN contradicts the ring-order endpoint; no dummy enters a public face loop (#517).

**Need.** We must know the corroborating field to admit a dummy FIN instead of withholding its edge.

### BC-10. FIN-link withholding

**Question.** Which native field distinguishes a FIN backward link that names no reciprocal same-loop forward link from a valid ring closure?

**Known.** `decode.rs` withholds a ring through `TopologyFinLinkUnresolved` when a FIN backward link names no reciprocal same-loop forward link; the ring is not walked on doubt (#517).

**Need.** We must know the link grammar to walk such a ring instead of withholding it.

### BC-11. Vertexless ring-edge admission

**Question.** Which native record admits a vertexless ring edge onto its carrier?

**Known.** `decode.rs` admits a vertexless ring edge only with reciprocal opposite FINs on the same unbounded circle carrier, a face-owned single-FIN loop, and at most one sheet dummy; every other vertexless ring edge is withheld through `TopologyRingEdgeWithheld` and no vertex is invented (#517).

**Need.** We must know the native admission record to place such an edge instead of withholding it.

### BC-12. Stored face-sense against coedge parity

**Question.** Which native field decides the face sense when the stored sense contradicts the shared-edge coedge parity?

**Known.** The decoder keeps each face's stored sense and reports the disagreement through `TopologyFaceSenseConflict` (#504). `sldprt.md` §7.1 gives the three-reversal neutral sense composition and states that coedge markers do not constrain face senses, but names no native rule for the disagreement and no prose for the report gate.

**Need.** We must know the native rule to resolve the conflict instead of reporting it.

## 2. Geometry carriers

### GC-02. Missing intersection witnesses

**Question.** Where are the authoritative chart and terminator replacements when an intersection composite references an absent witness record?

**Known.** `sldprt.md` §7.2 "Curve carriers: an edge's `00 10.refs[3]` can point to a `00 86` B-spline/list curve carrier," through `sldprt.md` §7.3 "The chart is a solved cache" define both intersection carriers and the three witness record families. The referenced terminators select the unique chart stride with the least endpoint displacement and replace the approximate chart endpoints. A complete width-4 support-UV record supplies the two solved pcurve caches. An absent or structurally inconsistent optional support-UV record does not invalidate the curve. Tolerance-bounded inversion constructs its pcurves on analytic and positive-weight NURBS supports. Consecutive inverses select one continuous parameter branch, the terminators supply the exact endpoint parameters, and analytic derivative bounds or rational Bézier residual control hulls certify the complete mapped segments against the chart tolerance.

**Need.** We must locate an authoritative replacement when the referenced chart or terminator record is absent.

### GC-03. Surface-owned edge curve attributes

**Question.** What grammar and role do untyped edge curve attributes have when a surface record owns them instead of a curve carrier?

**Known.** `sldprt.md` §7.1 "Stream-scope" defines `00 86` as a B-spline or list curve carrier. `sldprt.md` §7.2 "Curve carriers: an edge's `00 10.refs[3]` can point to a `00 86` B-spline/list curve carrier," defines the intersection curve carriers. An edge selects its support curve through `00 10.refs[3]`.

**Need.** We must know the grammar to construct the edge curve.

### GC-04. Offset-surface carriers

**Question.** Which carrier and field semantics identify an offset surface, including the discriminator and flag in its native record?

**Known.** `sldprt.md` §7.4 defines the supported offset carrier and its signed normal-offset construction over an analytic, B-spline, or procedural support surface.

**Need.** We must know the carrier grammar and field semantics to construct the exact offset surface and preserve its orientation.

### GC-05. Variable-radius blend carriers

**Question.** Which native carrier stores the result and radius law of a variable-radius blend, and how does it bind to the feature history?

**Known.** `sldprt.md` §7.4 defines the generic `00 7c` B-spline surface-use wrapper and NURBS carrier. Feature history separately carries variable-radius design intent.

**Need.** We must define the native relation that binds one variable-radius history feature and its radius law to one result carrier.

### GC-08. Duplicate geometry carrier identities

**Question.** Which occurrence is authoritative when one stream contains multiple valid geometry carriers with the same stream-local attribute?

**Known.** Geometry carriers are indexed by stream-local attribute. The specification does not define a duplicate-identity rule or a precedence field.

**Need.** We must select the authoritative carrier without silently changing geometry when a duplicate record is present.

### GC-09. Conflicting surface carrier families

**Question.** Which surface carrier family owns a face when analytic, offset, blend, or sweep candidates are all structurally valid for the same surface attribute?

**Known.** `sldprt.md` §7 defines analytic and procedural surface carriers, but does not define a conflict rule between valid carrier families.

**Need.** We must define the native family discriminator or ownership relation that selects one exact face support when several carrier families are structurally valid.

### GC-10. Normal sense of non-compact surface carriers

**Question.** Which field orients the surface normal against the natural normal of the neutral surface for a `00 7c` B-spline surface-use wrapper, a `00 43` or `00 44` swept or spun surface, a rolling-ball blend, and an offset surface?

**Known.** `sldprt.md` §7.1 gives the orientation marker of a compact analytic surface record (`00 32` through `00 36`) and composes it with the bridge marker of §4. The `00 43`, `00 44`, blend, and offset records carry a `0x2b` or `0x2d` byte in the same compact-header position. `sldprt.md` does not give its meaning for these families.

**Need.** We must know the surface normal sense to give a face on one of these carriers its outward normal.

### GC-11. Compact curve marker exceptions

**Question.** Which field sets the edge direction when a compact circle with marker `0x2b` carries an edge whose stored direction runs along the curve parameter?

**Known.** `sldprt.md` §7.1 "The marker of a compact line, circle, or ellipse gives the direction" defines the marker rule. A circle edge between a cylinder face and a planar face can have marker `0x2b` while its display tessellation runs along the curve parameter. The specification defines no second field that overrides the marker.

**Conflict.** The decoder applies the marker rule to that edge and reports no loss, so the neutral edge trims the circle through the complementary arc of the tessellated arc.

**Need.** We must find the overriding field, or the defect in the marker rule, so that this edge does not trim the circle through the complementary arc.

### GC-12. Bounded-curve wrapper ranges that miss the edge vertices

**Question.** Which direction does an edge on a bounded-curve wrapper have when the wrapper range endpoints match the edge vertices in neither order?

**Known.** `sldprt.md` §7.1 "The marker of a compact line, circle, or ellipse gives the direction" states that a bounded-curve wrapper range sets the edge direction instead of the marker. The range start and end points match the edge vertices in order or in reverse order and so give the direction. The specification defines no direction for a range whose endpoints match the vertices in neither order. The wrapper carries its own orientation marker byte.

**Need.** We must define whether the wrapper marker, the basis curve marker, or another field sets the direction of such an edge.

### GC-13. `00 28` chart field offsets

**Question.** Which byte offsets hold the seventh f64, the two absent-value sentinels, and the point block of a `00 28` chart?

**Known.** `sldprt.md` §7.3 lists the ordered fields `count u32 BE, attr u16 BE, base_parameter f64 BE, base_scale f64 BE, chart_count u32 BE, chordal_error f64 BE`, one further f64, then the sentinels at body +36 and +44, then `count` point entries at body +52. The unnamed seventh f64 occupies body +34 through +41 and overlaps the first sentinel at body +36. `docs/layouts/sldprt.toml` records the overlap as a `[[record.discrepancy]]`; `brep/intersection.rs` reads the sentinels at `preamble + 36` and `preamble + 44` and the point block at `preamble + 52` with `preamble = body + 6`. §4 uses body-relative offsets elsewhere, so the two conventions collide in one document.

**Need.** We must know the native offsets to emit the record. Without them, a writer cannot place the seventh f64, the sentinels, or the point block.

**Conflict.** The specification's stated offsets and the decoder's read offsets disagree, and no open item tracked the native truth.

## 3. Container metadata

### CM-01. Cache-cell prefix

**Question.** Which producer rule generates a new cache cell's opaque prefix values when no source bytes are available?

**Known.** The prefix occupies the `N=L/S` bytes between an independently anchored cell start and its marker. Equal section names, displacements, complete fixed headers, and unique target blocks do not determine equal prefix values. Retained cells preserve these bytes without assigning a producer meaning.

**Need.** A producer-side relation must establish a regeneration rule before a source-less writer can synthesize this field.

### CM-02. Cache-cell fill

**Question.** Which producer rule generates a new cache cell's opaque trailing values when no source bytes are available?

**Known.** The trailing field extends from the encoded name's end to `cell_start+S`. Equal section names, displacements, complete fixed headers, and unique target blocks do not determine equal trailing values. Zero and `ff` are opaque byte values, not structural rejection conditions. Retained cells preserve the complete range.

**Need.** A producer-side relation must establish a regeneration rule before a source-less writer can synthesize this field.

### CM-04. Opaque tail residual generation

**Question.** How does the producer generate the opaque prefix and residual bytes in the named tail-padding and anonymous footer cells?

**Known.** `sldprt.md` §1.4 defines the named-grid scales, footer scales, exact cell extents, EOCD count, and both central-directory locator equations. Bytes outside those fields remain opaque and are retained exactly; they do not select directory members.

**Need.** An independent producer oracle that explains or generates those residual bytes for newly constructed cells.

**Conflict.** None.

**Note.** Parsed grid geometry and EOCD fields do not establish a regeneration rule for opaque residuals.

### CM-07. Transformed enclosure opaque prologue

**Question.** How does the producer generate the 24 opaque bytes in a `moTransRefPlaneData_c` serializer prologue?

**Known.** `sldprt.md` §8 defines its fixed 25-byte prologue, presence byte, and optional ten-value enclosure. The stored center, corners, and radius do not define the opaque bytes.

**Need.** An independent producer rule or controlled native edit must determine these bytes for a newly authored record.

**Conflict.** None.

**Note.** Retained prologue bytes can be preserved without assigning a generation rule.

### CM-10. Parasolid stream boundary

**Question.** What fixes the end of a Parasolid stream inside one block payload?

**Known.** `sldprt.md` §3.1 gives the stream header: the `PS 00 00` signature, `desc_len u16 BE`, the description, the padding, `schema_len u8`, and the schema. The header has no total-length field. `sldprt.md` §3.2 states that a block can hold a partition stream and a deltas stream, and gives no delimiter between them.

A later `PS\0\0` is a boundary only when the bytes from that position contain a complete stream header with a bounded description and schema token. An unframed signature in coordinate or string data remains part of the current stream.

**Need.** We must know the authoritative boundary to distinguish a real following stream from payload bytes that happen to contain a complete header-shaped sequence.

### CM-14. Configuration body membership

**Question.** Which field binds an inactive configuration without `SourceIndex` to its bodies?

**Known.** Decimal `SourceIndex=N` binds a Keywords configuration to `Config-N-Partition`. The Keywords decimal `id` selects `Config-N-ResolvedFeatures` and is independent of the partition identity. Element order and regeneration ordinal do not bind a partition. A uniquely named active configuration can use the active geometry partition. `decode.rs` counts configurations with no body selection and reports them through `ConfigIncoherentBodyRefs`; `BodySelection::Unresolved` (`cadmpeg-ir/src/features.rs`) marks every remaining configuration whose body membership is not established.

**Need.** We must know whether another stored field binds an inactive source-less configuration. The body membership stays `Unresolved`.

### CM-15. Outer version word

**Question.** Which values can the outer header version word take?

**Known.** `sldprt.md` §1.1 states the word (u32 big-endian at offset 4) as the value `0x00000004`. `dialect.rs` never compares the word: `scan` hard-sets it to `0` on the compound-file branch and reads it only into the `outer_version` attribute as provenance.

**Need.** We must know the observed values to state whether the word is fixed. Without them, a reader cannot reject or accept another value.

**Conflict.** The specification states the value definitionally while the decoder never compares it.

## 4. Auxiliary lanes

### AL-01. DisplayLists face ranges

**Question.** How does a B-rep face attribute select its triangle range in a DisplayLists block?

**Known.** `sldprt.md` §7.3 "**`00 28` chart**" defines the DisplayLists descriptor table, strip lengths, and triangle-count relations. `sldprt.md` §4 "Primary ownership chain:" defines the B-rep face topology. `sldprt.md` §8 defines the complete persistent identity path carried by matching B-rep attributes and DisplayLists references, as well as unambiguous ownership from incidence with one analytic or NURBS face support and from complete bounded planar, cylindrical, and conical trims. Procedural, polygonal, opaque, and non-exact NURBS supports, other coincident trims, and tables without a complete matching identity do not supply the stored face-range mapping.

**Need.** We must know the stored mapping to attach tables whose support is ambiguous or unsupported, including non-exact NURBS caches, and carry no complete matching persistent identity.

### AL-03. DisplayLists renderer flag meanings

**Question.** Which renderer states do the stored `B` and `C` flags identify?

**Known.** `sldprt.md` §8 defines the four supported extended header words, their Boolean metadata triples, the exact stored-normal classification, and the optional scalar/unit-vector tuple. The independent metadata word does not contribute to the header word.

**Need.** We must know the renderer meanings of `B` and `C` and the rule that selects their values when no retained metadata supplies them.

### AL-05. Retained and direct face appearance paths

**Question.** Which complete persistent-path relation selects retained feature faces and direct face appearances?

**Known.** `sldprt.md` §8 defines native display-state selection, three complete part-default class chains, body identity through `ATOM_ID_2001`, and generated-face feature scope through current `FACE_ID_2001` source and timestamp values. Four-word face values identify a feature-scoped owner set without a local face selector. A `moCompFace_c` object can carry a complete multi-component path. A terminal source, timestamp, and local triple can match more than one current face or omit another affected current face. A bare definition supplies no ownership. Unresolved higher-rank paths do not supply lower-rank appearance fallbacks.

**Need.** The complete path-to-current-face binding for retained or modified feature faces and direct face assignments, and a complete holderless part-default owner relation. A unique terminal triple in one current identity set does not establish a general path-truncation rule. A DisplayLists display-face table that carries conflicting reused class tags withholds its persistent join through `TessellationReusedTagAmbiguous` (`decode.rs`, #506, 78e93f783b); `sldprt.md` §8 gives no prose for reused class tags.

### AL-06. Decal mapping fields

**Question.** What do the two u32 values that begin a `moDecalProperties_c` body select, and what unit do the decal locations, width, and height use?

**Known.** `sldprt.md` §8 "A `moDecalProperties_c` instance is an 84-byte body" defines the five f64 fields. The u32 LE at `+0` is `0` or `1`, and the u32 LE at `+4` is `0` or `4`. Rotation takes the values `0`, `90`, `180`, and `270`. Width and height are metre-scale in most bodies; a width or height of exactly `500` occurs only with the pair `(1, 4)`, and a metre-scale size occurs with that pair too.

**Need.** We must know the mapping mode and the size unit before a decal can be placed on its faces.

### AL-07. Decal face lists in other DisplayLists forms

**Question.** Where does a DisplayLists decal record store its face list when the count is not at `+315`?

**Known.** `sldprt.md` §8 "A DisplayLists decal record stores its framed image path" defines one complete form. Other records store the same framed image path, and the bytes at `+6`, `+14`, and `+315` do not satisfy that grammar. A part can also declare `moDecalProperties_c` instances and store no DisplayLists decal record.

**Need.** We must know the other record forms, and the binding of a decal that has no DisplayLists record, to join every decal to its faces.

### AL-08. Visual-property payload fields

**Question.** What do the unnamed fields of a visual-property payload store?

**Known.** `sldprt.md` §8 "A visual-property payload begins at its packed colour" defines the names, the texture flag, and the image paths. The six optical scalars, the 104 bytes and 22 bytes of the texture block, the state cells, and the bytes around the appearance file path have no assigned meaning. The inline instance tag is resolved: `sldprt.md` §8 "The tag's low 15 bits are the class's index in the stream's archive map" states how its value follows from the stream.

**Need.** We must know the meaning of the texture scale and orientation fields, and of the six optical scalars.

## 5. Design intent

### DI-01. Other optional feature-manager node identities

**Question.** What native field identifies the role of each optional classless feature-manager node that does not satisfy a defined layout-scoped identity?

**Known.** `sldprt.md` §2 "An `moCurvePattern_c` feature-input object is immediately preceded by its seed feature object" through `sldprt.md` §2 "An `moLPattern_c` interval without a line-reference record carries each displayed translation" identify the annotations container, principal planes, model origin, lights-and-cameras container, ambient and directional lights, sheet-metal node, and exploded-views container. The identities depend on a complete native-class roster. Other source identifiers are allocation positions and are not role codes. `sldprt.md` §2 "A classless Keywords `Feature` whose `Type` token is `EquationDriven` is the equation" identifies the equation container by its operation-family token, which is a role code and needs neither a native class nor a reserved source identifier. `sldprt.md` §8 defines the explicit DisplayLists scene-object source binding. Anonymous scene-object counts do not identify Keywords records.

**Need.** We must distinguish the remaining binders, comments, body folders, materials, notes, sensors, favorites, history, selection sets, and markups.

### DI-02. Equation angular-unit mode

**Question.** Which native field stores the equation manager's angular-unit mode?

**Known.** `sldprt.md` §2 "A bare integer Keywords dimension bound to a unique driving angular scalar denotes milliradians" through `sldprt.md` §2 "A nonempty Keywords parameter value with no scalar literal, operator, grouping delimiter," define expression identifiers and unit-bearing literals. An explicit angular unit determines the interpretation of a trigonometric operand.

**Need.** We must know the document mode to evaluate a bare numeric trigonometric operand.

### DI-03. Default document properties

**Question.** Which native carrier stores the default document-property namespace used by equations?

**Known.** `sldprt.md` §2 "When exactly one line-distance operand identifies a profile line, the other operand identifies" through `sldprt.md` §2 "A bare `0` or `1` Keywords dimension bound to a unique driving distance" define configuration records, configuration-local properties, and equation evaluation. They do not bind a default document-property carrier.

**Need.** We must find the carrier to resolve an equation that references a default file property.

### DI-04. Configuration property lookup

**Question.** How does a `property@configuration@part` operand select its property namespace?

**Known.** `sldprt.md` §2 "When exactly one line-distance operand identifies a profile line, the other operand identifies" through `sldprt.md` §2 "A bare `0` or `1` Keywords dimension bound to a unique driving distance" define independently evaluated configuration snapshots and configuration-local properties.

**Need.** We must know the lookup rule to evaluate a configuration-qualified property operand.

### DI-05. Offset-edge relation invariant with an external operand

**Question.** What neutral geometric invariant does an offset-edge relation impose when one operand is an external-reference record?

**Known.** `sldprt.md` §2 "The relation table of a sketch block follows the last record of the block" gives the invariant for two local operands: two parallel lines or two concentric arcs. An external-reference record stores backed-up points in the frame of its source sketch, so a comparison in the frame of the owning sketch does not apply.

**Need.** We must know the invariant to project the native relation as a neutral sketch constraint.

### DI-06. Arc-cardinal relation invariants

**Question.** What neutral geometric invariant does each top, bottom, left, and right arc-cardinal marker relation impose?

**Known.** `sldprt.md` §2 "A `Config-N-ResolvedFeatures` lane supplies the evaluated parameter state for configuration slot" through `sldprt.md` §2 "A primary line-or-circle geometry handle on a transformed line segment identifies that line" define operand resolution and the neutral invariants of the supported relation families.

**Need.** We must know the invariants to project these native relations as neutral sketch constraints.

### DI-07. Ambiguous relation-locus ownership

**Question.** Which profile locus does a relation operand select when its marker graph identifies a handle but does not identify one geometric locus?

**Known.** `sldprt.md` §2 "A Keywords configuration's decimal `id` attribute is the slot identity for" through `sldprt.md` §2 "Feature-input geometry-handle coordinates and the nested Parasolid profile differ by a signed" define handle chains, coordinate transforms, canonical shared-coordinate loci, and relation operand selection. Ambiguous chains and transform-dependent markers remain unresolved.

**Need.** We must select one locus to construct the neutral constraint.

### DI-08. Relation codes `29..32`, `36..41`, and `43..85`

**Question.** What neutral invariant and operand roles does each relation code in `29..32`, `36..41`, and `43..85` have?

**Known.** The native numeric taxonomy defines identities through code `85`. `sldprt.md` §2 "The relation table of a sketch block follows the last record of the block" gives the row layout and operand kinds. Relation tables use codes `32`, `40`, `50`, `53`, `55`, `56`, `57`, `62`, `64`, `65`, `66`, `67`, `75`, `80`, and `85` of this range. A patterned row (`57`) has two congruent operands, but the IR has no patterned constraint. A use-edge row (`32`) pairs an external-reference record with a local curve. An at-pierce row (`40`) has two point operands.

**Need.** We must know each invariant and operand role to project the remaining codes.

### DI-09. Relation codes above `85`

**Question.** What native relation does each code above `85` identify?

**Known.** The defined native numeric taxonomy ends at code `85`. No known relation-table row or sketch marker has a code above `85`.

**Need.** We must know the identities before we can define their operands and neutral invariants.

### DI-10. Ambiguous marker transforms

**Question.** Which marker-to-profile transform applies when coordinate sets permit more than one signed-axis transform and the transforms select different loci?

**Known.** `sldprt.md` §2 "Keywords length literals use the suffixes `uin`, `mil`, `mm`, `cm`, `in`, `ft`, `nm`, `um`, `µm`" through `sldprt.md` §2 "Operand tags" define transform selection, placement fallback, and the case in which all valid transforms give the same locus set.

**Need.** We must select one transform to bind each remaining marker to profile geometry.

### DI-11. Ambiguous linked reference markers

**Question.** Which profile entity does a reference marker select when its linked loci do not identify one entity?

**Known.** `sldprt.md` §2 "A Keywords configuration's decimal `id` attribute is the slot identity for" and `sldprt.md` §2 "Keywords length literals use the suffixes `uin`, `mil`, `mm`, `cm`, `in`, `ft`, `nm`, `um`, `µm`" define unique linked-handle and shared-entity resolution.

**Need.** We must select one entity to bind the reference operand.

### DI-12. Omitted dimensioned circles

**Question.** Which native field marks dimensioned circular geometry as construction geometry when absent from the selected profile stream?

**Known.** The exact compact legacy and extended radial layouts, their native code and role fields, the `sgSlot` relation, and the current-prefix geometry-locus `sgArcHandle` point carrier identify supported dimensioned-circle records. A declared `sgEntHandle` operand that references the exact point carrier supplies its center even when no radial record is present. Tags `d4 80` and `d5 80` directly carry a finite same-feature point or constrained-point center whose local identifier equals the operand address; their explicit identity takes precedence over pair and radius searches. Other native `sgCircleDim` tags use that same explicit point identity only after no declared pair, child, geometry-locus, or unique circular-witness carrier resolves. The reference must resolve in the unique declaring `sgEntHandle` lane to a finite same-feature point or constrained-point marker whose local identifier equals the operand address. A point that is the radial member of a declared pair cannot supply this fallback center; an invalid or ambiguous identity remains native. A declared `sgEntHandle` operand also accepts an adjacent center/radial point pair when the center local identifier equals the radial object index; the radial local identifier is either absent or zero in the ordinary form, or nonzero in the indexed point form. Tag `6e 83` uses an unreferenced lane-local `sgEntHandle` to select an indexed adjacent point pair from an even coordinate-bearing roster; every pair must carry the center-local-to-radial-object join and a nonzero radial local identifier. A declared `sgEntHandle` operand with an explicit `sgSlot_c` marker accepts one unique scoped `sgSlotHandle` class with exactly two current `e7 88` cells: the first cell identifies the slot and the second identifies one of the slot's selected center points. That form carries a construction circle centered at the selected point. A coordinate-less `line_or_circle` marker before the operand with exactly two identical links to one earlier same-feature `Arc` marker is also an arc carrier when both local identifiers match and the arc passes its endpoint or unique-later-radial-witness validation. Multiple, mismatched, or invalid linked carriers remain native. The selected profile stream can omit construction circles.

**Need.** We must know the discriminator for dimensioned-circle operands that have no radial record, do not reference the exact geometry-locus `sgArcHandle` carrier, and do not use the settled point, pair, slot, or duplicate-link arc identities.

### DI-13. Marker-only profile placement

**Question.** How is a marker-only profile placed in model space when it has no local, contextual, or unique lane-wide compact reference-plane record?

**Known.** `sldprt.md` §2 "Point-reference cells address feature-local object indices within the owning feature object" defines model-space recovery from a planar sketch placement. `sldprt.md` §2 "An `moLPattern_c` feature-input object is immediately preceded by its seed feature object. That" through `sldprt.md` §2 "Among classless, parameterless, propertyless history records, `Feature` source ID `1` is the" define the supported reference-plane frames.

**Need.** We must know the placement to transfer the profile from planar coordinates to model coordinates.

### DI-14. Other Keywords operation families

**Question.** What neutral operation does each Keywords operation family outside the typed neutral feature set represent?

**Known.** `sldprt.md` §2 "A detailed curve record is immediately followed by a curve-detail marker of the same generation:" through `sldprt.md` §2 "A bare `0` or `1` Keywords dimension bound to a unique driving distance" define the native records and operands of the supported history operations. Projected split-line operations are defined by `sldprt.md` §2 "An `moPLine_c` feature-input object containing exactly one `moPLineProject_c` class" and are outside this open item.

**Need.** We must know the operation semantics to transfer the remaining design history.

### DI-15. Up-to-vertex endpoint selector

**Question.** Which endpoint does the u32 selector in an up-to-vertex code-`3` edge-endpoint reference select?

**Known.** The point-reference form retains the edge and endpoint selector. The little-endian u32 at four bytes before the duplicated selection-vector marker is the `endpoint_selector`. `sldprt.md` §2 "A `Config-N-ResolvedFeatures` lane supplies the evaluated parameter state for configuration slot" states how ordinary point-reference object indices resolve.

**Need.** We must know the selector values to terminate the extrusion at the correct edge endpoint.

### DI-16. Point-reference codes above `9`

**Question.** What point-reference form does each code above `9` identify?

**Known.** Point-reference codes through `9` have defined native forms and neutral projections.

**Need.** We must know the forms to project their selected geometry.

### DI-17. Other second-direction termination codes

**Question.** What termination does each second-direction end-spec code other than `0` and `1` represent?

**Known.** `sldprt.md` §2 "A named feature-input object bound to a classless history `Sketch` record with a nonzero source" defines second-direction code `0` as absent and code `1` as through-all.

**Need.** We must know the other codes to construct the second extrusion extent.

### DI-18. Other first- and second-direction combinations

**Question.** What semantics apply when a nonzero second-direction code occurs with a first-direction code other than `0`, `1`, or `9`?

**Known.** `sldprt.md` §2 "A named feature-input object bound to a classless history `Sketch` record with a nonzero source" defines the supported one-direction and two-direction combinations.

**Need.** We must know the combination semantics to construct both extrusion extents.

### DI-19. Reference-bearing end-spec shapes

**Question.** What record uses the end-spec shape when the word at `+18` is a reference child instead of a termination code?

**Known.** `sldprt.md` §2 "A named feature-input object bound to a classless history `Sketch` record with a nonzero source" defines the end-spec anchor and the termination words at `+18` and `+22` for valid end-spec children.

**Need.** We must distinguish the other record family from an extrusion end specification.

### DI-20. Feature-local face reconciliation

**Question.** What is the disposition of a feature-local face identity when no surviving face carries that identity?

**Known.** `sldprt.md` §2 "An extrusion object-name record is followed by four zero bytes, a little-endian u16 family word," through `sldprt.md` §2 "When operation objects and their dimension children form separate ordered groups, a blind" define the `moSingleFaceRef_w` path forms and their feature-local face selection. `sldprt.md` §4.1 "The fourth FACE reference is the owning SHELL." defines the typed face relation, and §5 defines how the terminal owner and feature-local face identity select the surviving face through `FACE_ID_2001`.

**Need.** We must distinguish a face consumed by a later operation from a face whose owner or identity attribute was not decoded.

### DI-21. Class-scoped inline extrusion operations

**Question.** What Boolean operation does the inline family `0x0140` with operation byte `00` represent for `moCutThin_c`, and what operation does each other unsupported family-word and operation-byte combination represent?

**Known.** `sldprt.md` §2 "Feature-tree" defines the supported ordinary extrusion combinations. The ordinary `0x0140`/`00` join mapping does not establish the operation of a `moCutThin_c` object. Family `0x01ca` with byte `00` or `02` establishes cut. A normalized Keywords `CutExtrudeThin` type also establishes cut independently. A complete configuration-local operation carrier is distinct from the base configuration's operation state.

**Need.** We must identify an independent, class-scoped Boolean discriminator for the `moCutThin_c` `0x0140`/`00` form and define the remaining family-word and operation-byte combinations.

### DI-22. Other sparse extrusion form codes

**Question.** What operation and record shape does each sparse extrusion object outside the defined form codes use?

**Known.** `sldprt.md` §2 "Keywords element order is serialization order, not regeneration order. Neutral regeneration" through `sldprt.md` §2 "Feature-tree" define direct, repeated-class, sentinel, terminated-trailer, and sparse-trailer objects. Form code `11` identifies an object form and does not determine its Boolean operation. A Keywords operation token or a complete inline operation trailer supplies the operation. Without either carrier, the operation remains unresolved.

**Need.** We must know the remaining form codes to parse each object and construct its Boolean operation.

### DI-23. Combine-body reconciliation

**Question.** How does a body selected by a compact `moCombineBodies_c` target or tool path map to a body in the final B-rep?

**Known.** The compact paths identify generated feature-local bodies. A complete target or tool path projects to one generated body reference whose feature is the terminal path producer, whose local identity is the ordered path-local sequence, and whose dependencies include every uniquely identified traversed history feature. `sldprt.md` §4.3 "00 01 00 01" through `sldprt.md` §6 "Body membership is the typed XT ownership graph." define final B-rep body identities and ownership. `FACE_ID_2001` carries face identities and does not bind a feature-local body identity.

**Need.** We must know the mapping to bind the Boolean target and tool bodies.

### DI-24. Delete-body reconciliation

**Question.** How does an `moDeleteBody_c` regeneration-input-local body identity map to a body in the final B-rep?

**Known.** The compact record retains the regeneration-input-local identity. `sldprt.md` §4.3 "00 01 00 01" through `sldprt.md` §6 "Body membership is the typed XT ownership graph." define final B-rep body identities and ownership.

**Need.** We must know the mapping to delete the selected final body.

### DI-25. Component-edge path reconciliation

**Question.** How does an edge selected by an entry-form `moCompEdge_c` path map to an edge in the final B-rep?

**Known.** The entry path identifies a generated feature-local edge. `sldprt.md` §3.2 "An attribute" through `sldprt.md` §4 "**Bridge `00 0e`:** `attr` is u16 BE at body +0" define final B-rep edge identity and direction. `FACE_ID_2001` carries no edge identity. The `EDGE_PERM_ID_2_2003` attribute grammar is not defined.

**Need.** We must know the mapping to bind the selected operation edge.

### DI-26. Compact edge-vector B-rep reconciliation

**Question.** How does a compact edge vector map its feature-local edge path to a final B-rep edge?

**Known.** `sldprt.md` §2 defines the duplicated-marker framing, count, lane-local selector, typed and reference-list entry forms, separators, terminal slots, and lane-local type signatures. The ordered compact vector and its producer-feature paths are retained. The terminal component identifies a feature-local generated edge, but it does not by itself identify the final B-rep edge.

**Need.** We must define the topology join from the retained compact edge identity to the final B-rep edge before projecting the selected edge.

### DI-28. Unbound general-curve references

**Question.** How does a general-curve-reference form select geometry when it has no component-profile source record and no immediately preceding unique profile feature?

**Known.** `sldprt.md` §2 "An extrusion object without `Profile` or `DissectableChildren` has an unresolved profile unless" defines ownership for a unique enclosed sweep profile. The supported general-curve-reference forms bind through a component-profile source or an adjacent profile feature.

**Need.** We must know the other binding rule to select sketch or B-rep geometry.

### DI-29. Unbound composite sweep profiles

**Question.** How does a composite sweep-profile form select its profile when it has no unique enclosed planar profile stream and no immediately following unique profile feature?

**Known.** `sldprt.md` §2 "An extrusion object without `Profile` or `DissectableChildren` has an unresolved profile unless" defines the enclosed planar profile rule. The supported composite form can also bind to an adjacent profile feature.

**Need.** We must know the other binding rule to construct the sweep profile.

### DI-30. Other compact sweep Boolean codes

**Question.** What Boolean operation does each compact sweep code other than join code `15` represent?

**Known.** Compact sweep code `15` joins the sweep result.

**Need.** We must know the other codes to construct the sweep result operation.

### DI-31. Native neutral-plane Draft outward flag

**Question.** Which native field identifies the outward flag of a neutral-plane Draft when Keywords omits it?

**Known.** `sldprt.md` §2 "A Draft feature-input interval uses the lane-scoped `moPlaneRef_w` token" defines neutral-plane Draft operands. `sldprt.md` §2 "A compact parting-line Draft uses direct duplicated component-vector records" defines the parting-line form, which has no outward operand. Keywords independently retains an explicit `Outward` Boolean for a neutral-plane Draft.

**Need.** We must recover the native outward flag of a neutral-plane Draft when Keywords omits it.

### DI-32. Compact line-reference width

**Question.** Which compact line-reference width and trailer layout are authoritative when more than one valid direction triple occurs in a record?

**Known.** `sldprt.md` §2 defines the supported compact line-reference layouts and requires an exact trailer for each layout. A final ambiguous layout with distinct valid directions remains unresolved.

**Need.** We must know the record width and direction field to decode each line reference without choosing a candidate by plausibility.

### DI-34. SWIFT implicit nominal construction

**Question.** Which nominal-geometry rule applies to each remaining feature-size annotation whose `Nominal` field is zero and whose `Dimension` field is absent or zero?

**Known.** `sldprt.md` §2.1 defines zero as an omitted nominal sentinel when `Dimension` is absent or zero. It defines diameter and depth nominal recovery from the rendered literal, declared decimal places, pattern or compound-feature traversal, and cylindrical or spherical nominal geometry. The rendered literal, not the unrounded geometry, supplies the labeled value. It defines width and length from named slot-geometry fields and radius from the named radius field of fillet, cylindrical, or spherical geometry. It defines counterbore diameter from the direct nominal cylinder and countersink diameter and angle from the direct nominal cone. An empty applied-feature graph and empty CAD identifiers supply no direct geometry and do not bind an unrelated rendered literal; an empty `GdtPattern` can additionally use the exact `Hole PatternN` to `LPatternN` history join when one seed has exactly one later consuming Hole with a positive diameter. Directional distance annotations recover plane, axis, compound-hole, and closed-slot feature-of-size locations from their named geometry.

**Need.** The empty-pattern history join is settled by `sldprt.md` §2.1. We must still define nonidentity `NominalTransform` semantics and the other `ComputeAnswerBy`, `Direction`, and `NormalTo` modes before those forms can supply a nominal.

### DI-35. Endpoint-less variable-fillet edge groups

**Question.** How does a selected-edge reference-list vector without `8083` endpoint references select its radius profile when the same `VarFillet_c` object contains more than one ordered endpoint-radius pair?

**Known.** `sldprt.md` §2 defines the reference-list grammar, including count-framed adjacent references, the variable-fillet input-edge boundary, the vertex-control join, and grouping of endpoint-bearing selected edges by their ordered endpoint radii. A legacy three-edge control vector with `D0`/`D1` supplies one feature-wide ordered pair, which applies to endpoint-less selected-edge vectors. A single ordered pair is likewise the feature-wide profile when all endpoint-bearing selected edges agree.

**Need.** We must identify the native association between an endpoint-less selected-edge vector and one of several distinct radius profiles.

### DI-36. SurfaceCut discarded side

**Question.** Which native field in an `moSurfCut_c` interval selects the retained side of a surface cut?

**Known.** `sldprt.md` §2 defines the target-body reference-list vector and the cutting-surface component vector. The target projects as a body selection and the cutting surface projects as a face selection. The neutral `CutWithSurface.reverse` value remains optional; an absent native Boolean remains unresolved.

**Need.** Distinguish the wrapper selector byte and the tail words between the surface-body identity pair and the termination sentinels with labeled parts cut to opposite sides before assigning the reverse value.

### DI-39. `moTempAxisRef_w` carrier end and middle triple

**Question.** What fixes the end of a `moTempAxisRef_w` nine-scalar carrier, and what do its scalars at `+263`, `+271`, and `+279` hold?

**Known.** `sldprt.md` §2 "An `moCirPattern_c` interval retains its repeated seed" gives the carrier at offsets `+0` through `+311`, the axis origin at `+239`, the axis direction at `+287`, and zero padding of at most 24 bytes before the next class declaration. The specification gives no stored field that fixes the padding length or binds the three scalars between the origin and the direction.

**Need.** We must know the field that ends the carrier, so the record does not need a tuned search window, and the middle triple, so the complete axis frame is decoded.

### DI-40. Extrusion profile identity without an explicit profile relation

**Question.** Which native relation binds the profile of a classless
`Extrusion` or `DissectableRoot` extrusion when `Profile` and
`DissectableChildren` do not identify a sketch?

**Known.** An explicit `Profile` attribute or a single dissected sketch child
binds the profile. Source identifiers order feature records, but ordering alone
does not establish that an earlier sketch is the extrusion's regeneration
input.

**Need.** We must decode the profile dependency before projecting the extrusion
with a neutral sketch profile.

**Conflict.** `project_extrude` in `src/history/project/solid.rs` selects the
non-origin sketch with the greatest source identifier smaller than the
extrusion's source identifier for the supported profile-less forms. An
unrelated intervening sketch can therefore be projected as the extrusion
profile solely because it is nearest in source order.

### DI-41. Pattern seed identity without a resolved seed relation

**Question.** Which native relation binds the seed of a linear or mirror pattern
when its component vectors and persistent generated-surface identities do not
resolve a seed feature?

**Known.** Complete component paths and generated-surface identities can bind a
pattern seed directly. A derived cosmetic-thread output identifies a generated
child, not by itself the input feature that the pattern repeats. Feature order
does not establish seed ownership.

**Need.** We must decode the seed relation before assigning a preceding history
feature as the repeated neutral feature.

**Conflict.** `bind_pattern_inputs` in `src/resolved_features/bindings.rs`
uses the nearest preceding sibling `moCosmeticThread_c` feature when a pattern
has a derived cosmetic-thread output and no resolved seed. Another fallback
uses the immediately preceding feature-input object. Either rule can assign an
unrelated preceding feature as the pattern seed.

### DI-42. A configuration stream that holds the active configuration's values

**Question.** Does a `Contents/Config-N` stream show that its equation-manager values are not its configuration's current values?

**Known.** `sldprt.md` §2 states that an inactive configuration's values come from its own stream. A stream can hold, line for line, the values of the active configuration's stream while the configuration's own body shows other values. The specification gives no field that separates such a stream from one whose configuration has the active configuration's values.

**Need.** We must know this to report the configuration's values as not established. Without it, the decode keeps the active values and reports them as the configuration's.

### DI-43. Where the document stores its Angular Equation Units

**Question.** Which field of a SLDPRT file stores the Equations dialog's Angular Equation Units setting, degrees or radians, which sets the unit the equation manager's trigonometric functions take and give?

**Known.** SolidWorks documents the setting in the Equations dialog; its default is degrees. In the only part sampled with trigonometric lines, an `atn` that gives radians reproduces the value a later line stores as its input in every configuration, and one that gives degrees reproduces none, while the same lines read their angle dimensions in degrees. After the last relation's lowercase name and value table, where a section holds the relation count minus one as a u32 followed by `00 00 ff ff ff ff`, the u32 before it is 0 in that part and in 31 others without trigonometric lines, and 1 in the parametric biped's parts, whose equations come from an external file, and in the two FRC rail parts (5 sections); 129 of 222 manager sections have no such anchor. No other field was compared. `int()` truncation was the other half of this item: it truncates the manager's value, and a value a few ulps short of a whole number is that whole number (`sldprt.md` §2). Whether it truncates or floors a negative argument is not established: none occurs in the dataset.

**Need.** We must know the field to evaluate a line that calls a trigonometric function. Without it, the decode gives such a line no value, and its parameter keeps its literal.

### DI-44. Sign of an integer scalar value

**Question.** Is the four-byte value of an `moIntegerParameter_c` named scalar signed or unsigned?

**Known.** `sldprt.md` reads it as a little-endian i32. Across the 1 040 integer cells of the 1 581-part dataset, values are 0 through 1327, so no value distinguishes i32 from u32.

**Need.** We must know the sign before a writer can store a negative value. Without it, the writer refuses one.

### DI-45. Relation-table row flag

**Question.** What does the u16 row flag `1` of a relation-table row mean?

**Known.** `sldprt.md` §2 "The relation table of a sketch block follows the last record of the block" gives the flag position. The solved geometry satisfies a flag-`0` row. A flag-`1` row has no established invariant.

**Need.** We must know the meaning to type or to write flag-`1` rows.

### DI-46. Point records with a curve or relation code

**Question.** Which field identifies a point record whose u32 at marker +17 is `2` or a relation code?

**Known.** Such a record stores the coordinate tag `1e 00` at marker +64 and holds a slot in the point array of its block. Its u32 at marker +17 is `2` or a relation code.

**Need.** We must draw these points to type the relations that name them.


### DI-47. Relation-table framing

**Question.** Which fields frame a sketch block's relation table: the row count, the start of the table, and the length of each row's trailer?

**Known.** `sldprt.md` §2 "The relation table of a sketch block follows the last record of the block" gives the row header, the handle cells, and the trailer forms of the typed codes. The u32 thirty bytes before a table's first row is not always its row count. The handle cells are new archive objects, so a row added or removed changes the archive's object indices after it.

**Need.** We must know the framing to add or remove a relation row. Without it, the writer refuses both.

## 6. Container record semantics

### EV-03. `SWObjects` record content

**Question.** What fields and relations occupy the unresolved bytes of each `SWObjects` metadata record?

**Known.** Configuration-manager values occur at offsets `+66`, `+107`, and `+117`; `moPart_c` has two defined fields; and `moVisualProperties_c` defines colour and name fields. The remaining bytes, the complete record lengths, and the role of `0x00c0_c0c0` in material records are undefined.

**Need.** We must know what the undecoded bytes hold before the writer can edit a retained record or claim that a generated record is complete.
### EV-05. Leading word of a compound-file `ResolvedFeatures` stream

**Question.** What does the u16 that opens `Contents/Config-N-ResolvedFeatures` in a compound-file part hold, and does that stream continue the class map of `Contents/Config-N`?

**Known.** `sldprt.md` §2.2: in a block-container part the stream opens with the u32 map count at which `Config-N` ended; in a compound-file part it opens with a u16 and then a class declaration.

**Need.** Without the map count, a reader cannot check its `Config-N` class map against the stream, and cannot resolve a `ResolvedFeatures` tag that names a `Config-N` class.

### EV-06. Named-object header and feature-object block fields

**Question.** What do the named-object header's u32 at +0 and +12, the feature-object block's unnamed u32 fields, its +41 f64, its two version-and-date pairs and its FILETIME hold? What are the bytes that `moEnvFolder_c` and `moDetailCabinet_c` write before the block's +55 string?

**Known.** `sldprt.md` §2.3 gives the offsets and widths of the named-object header and the feature-object block. The +29 and +37 version numbers include values that are `swVersion` values, such as 17000, and values that are not, such as 1195. The +29 version is not consistently smaller or larger than the +37 version.

**Need.** A writer has to fill these fields for a new or changed object, and a reader cannot state an object's provenance without them.

### EV-07. Favorites and selection-set folder bodies

**Question.** What do `moFavoriteFolder_c` and `moSelectionSetFolder_c` write after the feature-object block when they own items?

**Known.** With no items, `moFavoriteFolder_c` writes `00 00 00 00 01 00 00 00` and `moSelectionSetFolder_c` writes `00 00 01 00 00 00`. A folder that owns one item writes `01 00` and then a `moFavoriteFeatItemData_c` or `moSelectionSetData_c` object.

**Need.** A writer that adds a favorite or a selection set has to write the folder's count and the bytes that follow its items.

### EV-08. Reference-plane body fields

**Question.** What do the `moRefPlane_c` u32 at +64 and +136 and the f64 at +112, +120 and +128 hold?

**Known.** `sldprt.md` §2.3 gives their offsets. The +112 value is negative and the +120 value positive. The +128 value is zero or of magnitude below 1e-17. The +136 value is 0, 1, 4 or 5. The +64 value is zero in most planes, and `cd cd cd cd` in some.

**Need.** A writer that creates a reference plane has to fill these fields.

### EV-09. Bytes after a directional light's embedded block

**Question.** What does a `moDirectionLight_c` object write after the feature-object block embedded at its name string +60?

**Known.** `sldprt.md` §2.3 gives the light's bytes through that block. After it come eight zero bytes in files whose `swVersion` is 15000 to 19000, and twelve zero bytes from 11000 to 14000. Six zero bytes, or six zero bytes, a class tag and two zero bytes, also occur at 17000 and 19000.

**Need.** A writer that creates or changes a directional light has to write these bytes.

### EV-10. Feature body head fields

**Question.** What do the fields *h*, *f*, *t*, *g*, *e*, *x* and the *m* u32 values of the feature body head (`sldprt.md` §2.3) mean, and what do the `moPerBodyChooserData_c`, `moCompSolidBody_c` and `moPMarkRecord_c` objects that follow the head store?

**Known.** After *x* `1` comes a u16 equal to *m* in all but two heads, then a `moPerBodyChooserData_c` object. After *g* `1` come `moCompSolidBody_c` objects in 687 of 850 heads; 153 others open with a class tag the corpus walk does not name, and whether those are solid bodies is open. Every `moRevolution_c`, `moCut_c` and `moRevCut_c` object with *g* `0`, *e* `0` and *x* `1` writes u16 `0`, a reference, u16 `1`, u32 `1`, the same reference and further bytes. `moCombineBodies_c`, `moScale_c` and `moMoveFace_c` write u16 `1` and an object in place after *x*. The *m* values equal the feature-object block +20 value of other objects, which several folders share, so they are not identifiers. *t* is `3` in body-changing features and `0` in reference features, patterns and stored bodies, with `2` in ten objects.

**Need.** A writer that creates or changes a feature has to write these fields.

### EV-11. The bytes around a sketch feature's sketch object

**Question.** What does a `moProfileFeature_c` object write after its `sgSketch` object, and how is an in-place `sgSketch` body laid out up to its first sketch entity record?

**Known.** `sldprt.md` §2.3 gives the ten zero bytes and the `sgSketch` object. An in-place `sgSketch` usually has §2's sketch entity record marker `ff ff 1f 00 03` at its class tag +14, and some have no such marker there. A referenced `sgSketch` is usually followed by `01 00 00 00 01 00 00 00`.

**Need.** A writer has to emit the sketch object's framing and the profile's trailing bytes around the entity records.

### EV-12. Other fillet and chamfer forms

**Question.** What are the layouts of the `Fillet_c` and `Chamfer_c` objects that do not follow `sldprt.md` §2.3, and what do their fields mean?

**Known.** Some fillets write a long body of repeated records after the +54 u32, with `0e 01 00 00` and a class tag in each record. Some write a body shorter than 68 bytes. Some have other values in the first 54 bytes. In a fillet the two u32 at second-table +30 and +64 usually differ by one; in a chamfer the +30 value is usually zero.

**Need.** A writer that creates or changes a fillet or chamfer, including multi-edge and variable forms, has to write these bytes.

### EV-13. Feature folder bytes

**Question.** What does a `moFtrFolder_c` feature folder write after its eight zero bytes and u32 `1`, `1`, `0`, `1` when it does not end with a reference to itself, and how does it store the features it contains?

**Known.** 176 folders written with a class declaration, 175 of them after a `moNotesAreaFtrFolder_c` folder, write u32 `1` after those values and then further bytes; the 188 such bodies that end at a declaration are 177 to 187 bytes long. 23 folders do not start with the reference and u32 values of `sldprt.md` §2.3, and 11 written with a class tag end with a reference to another object. The folder's contents are not among these bytes.

**Need.** A reader that shows a feature tree's folders, and a writer that groups features into them, have to read and write these bytes.

### EV-14. Persistent identity fields

**Question.** What do the fields of the identity-representation objects, the `moFR_c` u32 before its time stamp, and the reference header's selector and record identifier mean, and what are the `moExtObject_c` forms whose u16 after the two string pointers is not `2`?

**Known.** The `moFR_c` u32 is small and repeats across hops of one feature, so it may number the producing feature. In `moFromSktEnt3IntSurfIdRep_c` the second u32 is usually `ffffffff`. In 173 `moExtObject_c` objects the bytes between the third string and the configuration name are 17, not 18. Hop lists whose class index the corpus walk cannot name, and identity-representation classes not in the table of `sldprt.md` §2.3, remain unparsed.

**Need.** A reader that resolves a selected face or edge to geometry, and a writer that creates a selection, have to read and write these fields.

### EV-15. Feature-object block inputs

**Question.** What are the inputs of each feature class's feature-object block, and what are the bytes that follow the inputs of a `moDetailCabinet_c` cabinet?

**Known.** An extrusion's one input is a reference to the sketch feature it extrudes. A cabinet's inputs are its notes folders. In some files the cabinet's inputs are followed by a back-reference to the first input, eight zero bytes, a run of u32 `0` or `1` values and a back-reference to the last input, the same shape as the bytes a `moFtrFolder_c` folder writes of its own (§2.3); in others the last input is a `moFtrFolder_c` folder that writes them. Fourteen zero bytes instead of twelve then end the list in some files.

**Need.** A writer that creates a feature has to write its inputs.

### EV-16. Sketch object fields

**Question.** What do the unnamed fields of the sketch record head, point and curve records, the `sgSketch` settings block and its 61 trailing bytes mean, what are the eight relation groups, and how are the compact `ff ff 07 00 01` records laid out?

**Known.** In some sketches (about 17 streams; `Intake Final Drive Hex`, `105t HTD Belt…`) the dimension and relation groups hold one u16 count fewer than the usual three plus eight, with the same entries otherwise: a polygon that is the last group's entry elsewhere appears one group earlier. The header fields do not tell the two apart, but no group count has bit 15 set, so a reader detects the shorter form by the class tag that stands where the eighth count would be; which group is dropped is not known. The relation groups observed hold one-operand relations (types `4`, `5`, `17`) in the first group and two-operand relations (types `9`, `12`) in later groups. Some sketches write four zero bytes after the eight groups before the first `suObList` pointer. The zero bytes between the last curve record and the dimension count number 36 when the last curve list holds lines (106 of 115 sketches examined) and 22 when it holds arcs (187 of 220); 34 and 24 occur in the rest. The compact generation occurs in files whose sketches open their records with `ff ff 07 00 01`.

**Need.** A reader that recovers sketch geometry and constraints, and a writer that creates a sketch, have to read and write these fields.

### EV-17. Extrusion feature fields

**Question.** Where is the item count of a feature dimension's placement block, what are the 76 bytes before an extrusion's `moFromEndSpec_c`, what does the 84-byte bounding-box form's 20-byte tail hold, what do extrusion and ICE objects write after their end specification in two-direction and up-to-surface forms, what are the `moBBoxCenterData_c` forms other than the 70- and 72-byte ones, and how is an ICE whose second body chooser and mark record are written without a class tag (by value) laid out?

**Known.** After the end specification come the dimension display objects of the feature's dimensions, an up-to-surface face selection when the end type is `4`, and a `moFromEndSpec_c` object. Some bounding boxes write twelve more bytes before the next object. Some ICE objects write a body chooser's count and face references, and 64 zero bytes in place of a mark record, with no class tag. Chamfers have at least three forms: the one of `swVersion` 15000–16000 is read; in files of 12000 the dimension display is followed by u32 `1`, four empty `CString` values and f64 values, and in files of 17000 and 19000 an object follows the first `suObList` at once. A `moSketchBlockInst_c` writes, after its block definition, a 16-byte identifier, u16 `0x68`, u16 `11`, eight zero bytes, a `moDetailFolder_c`, transforms and `moAbsolutePoint_c` objects (13 files). A silhouette edge's backed-up data (u32 `9`, f64 π, …, the handle block, a `moArcBackedUpData_c` and the name `Silhouette Edge`) is not read.

**Need.** A reader that recovers an extrusion's depth, direction and bodies, and a writer that creates an extrusion, have to read and write these fields.

### EV-18. In-context reference fields

**Question.** What do the 64 zero bytes before an in-context reference's context component, the ten bytes a face group writes after its compressed bodies, the six f64 of a group chain, the u16 group count of the sketch in-context block, the ten zero bytes and four f64 of a face support, and the frame data after a `moFaceRefPlnData_c` face reference hold?

**Known.** The context component's two forms are distinguished by whether its identifier is repeated; form A carries sixteen `ff` bytes and form B two transform blocks. Each compressed body inflates to a Parasolid transmit file, the backed-up geometry of the support. A `moFaceRefPlnData_c` face reference is followed by `00 00`, u16 `1`, a context component, twelve zero bytes, i32 `-1` and a run of little-endian f64 values whose layout has not been decoded.

The face geometry after a `moFaceRefPlnData_c` in an in-context part has two observed forms. In one, an 80-byte head (six f64, an f64, three f64), a transform block at +80, a flag-`0` triple, 22 bytes, a second transform block and a 65-byte tail ending `04 00 00 00 ff ff ff ff 00 00 00 00`, a u32 identifier, a u32 and eight zero bytes (402 bytes in 55 samples). In the other it opens with the 58-byte handle block's fields (three i32 `-12345`, the version), twelve zero bytes, i32 `-1` and four f64 (`-1`, `1`, `1`, `-1`) (370 bytes in 21 samples). Lengths of 210, 298, 364, 392, 436, 442 and 474 also occur.

**Need.** A reader that keeps a part's in-context references, and a writer that preserves them, have to read and write these fields.

### EV-19. Configuration object stream

**Question.** What is the grammar of a `moRelMgr_c` object's detailing settings after its ten dimension styles, how are `moCThreadRefMgr_c` records and the untagged thread data laid out, how are `moRelation_c` objects and their equation sides read inside `Config-N`, and what are the `Config-N` layouts of files below `swVersion` 14000?

**Known.** A per-version template of gap lengths and string positions reads the detailing settings in 892 streams. Thread data with records writes 17-byte entries (u32, u32, a byte, f64), then f64 geometry of varying length, two `CString` values and three matrix rows; 124 streams stop there. 34 stop at equations, and none of the 37 streams of 10000–12000 reaches its end, for want of templates for those versions.

**Need.** A writer that creates a part has to write the configuration stream, and a reader that keeps detailing settings, threads and equations has to read it.

### EV-20. Favourite handle tail

**Question.** Which field of a `moFavoriteHandle_c` decides the length of the bytes after its last `CString`?

**Known.** The tail is a u32 flag word and 50 bytes in most handles; 85 bytes after the flag when bit 16 is set, with u32 `0` and a `CString` more when bit 0 is also set; 52 bytes and a `CString` and 18 bytes more when bit 7 is set. It is absent after a pattern-count dimension's handle and when an object follows at once, and 28 bytes before a reference object in one sample. Before an extrusion's 76-byte block it is zero bytes: 54 in 309 handles, 50 in all 4 of `swVersion` 14000, and 52 in 5 of `swVersion` 16000 and 19000. No byte of the handle's first 382 bytes separates the 52 from the 54 (314 handles of `swVersion` 15000 and above compared), so a reader finds the 76-byte block after a run of zero bytes. The 122 or 194 bytes of f64 values seen after handles with flag bits 11 and 15 set are the handle's own view block (resolved 2026-10-21; §2.3). Before a sketch's in-context block the tail is 52 bytes in four parts, told by the list that follows: a chain list, or a group header whose group data a reader would otherwise take for a chain count.

**Need.** A writer has to write the tail a handle's reader expects; a wrong length shifts the object index count for the rest of the stream.

### EV-21. Thread feature fields

**Question.** What do the unnamed fields of a `moSweepThread_c` mean: the two f64 after the edge's handle block, the 40 bytes before u32 `1`, u32 `1`, and the f64 groups of the cylinder and surface references?

**Known.** 10 of the 33 streams that hold a thread parse to their end; the start edge's point, axis and length, the pitch, the size and the library path are identified. The cylinder geometry is eleven f64 and the surface geometry thirteen f64 and a u32 in all samples read. A face-reference slot holding a `moCircleRef_w` (one file) and a 40-byte gap holding non-zero values (one file) are not read; in one file of `swVersion` 17000 the 74 bytes before the closing objects hold u32 `1`, `0`, `1` at +8 in place of u32 `1`, `1` at +40, read by length.

**Need.** A writer that creates a cosmetic or modelled thread has to write these fields; a reader that keeps thread data has to read them.

### EV-22. Sketch text fields and in-context plane frames

**Question.** What do the unnamed fields of sketch text mean (the character format's other slots and four f64, the u32 pair before the character count, the six f64 of text on a path), and what do the two frames of a plane in an assembly's context, and the 47 bytes between them, distinguish?

**Known.** Text records, their character format and outlines are read to their end in all 7 streams that hold them (3 designs; §2.3), including text on a path. The outlines are the rendered glyphs, so a writer that changes the text has to regenerate them from the font. An in-context plane's two frames were equal in the 6 parts read, and the bytes between them were 31 zero bytes, `01` and 15 zero bytes in all.

**Need.** A writer that keeps a sketch's text, or a sketch placed on a face of another part of an assembly, has to write these records.

### EV-23. Sheet-metal bends and single-design forms

**Question.** What is the layout of a sheet-metal bend (`moOneBend_c`, the children of a `moEdgeFlange_c`), and of the forms seen in one design each: a polygon relation entry whose head differs from the documented one; a contour chooser whose tail selects sketch entities (a count, handles and identifiers `100000` + *k*); a spline tangent-magnitude dimension whose operands are data; the six f64 and u32 `0` that end one circular pattern at its stream's end; the 168 bytes that end a `moFlatPattern_c`; and the rest of a mirrored part (`moMirrorStock_c`: a `moBodyRef_w` whose `moCompBody_c` holds a context unit component, fields, a mirror frame and the mirrored body's compressed data, then further references and the mirror plane) and of an add-in's custom feature (`moMacroFeature_c`); the geometry of a `moCompRefAxis_c` (105 bytes, framed); the object index that 15 McMaster-Carr bolts count after the first `moSweepSideSurfIdRep_c` hop (a reader's count falls one short there, and nothing in the bytes shows the extra object); and a sketch's `sgBlock` (its layout is stated in §2.3; open is one block of 720 triples whose sequence after the triples differs)?

**Known.** A bend is a named feature, then 122 bytes, its radius display, a `moCompFeature_c`, 33 bytes, its `moCompEdge_c` and its display objects, then a block. One block form is 215 bytes in six of the seven streams and 287 in the seventh. It opens with u32 `0` (the last display's flag word), u32 `8`, u32 `0x01000000`, u32 `0`, u16 `0x0100`, eight bytes that repeat in its middle (`20 5d 67 ce ce 72 ef bc` in the samples) and a frame of 3, 6 or 12 f64; its last 67 bytes are identical in all seven: f64 `1`, u8 `0`, eight `ff` bytes, u32 `1`, u32 `1`, thirty zero bytes and twelve `ff` bytes. Six streams write 34 further bytes before those: f64 `1`, three f64 `0` and u16 `0`. A reader frames that form by the next named object: a class tag, the name object's class tag and a Unicode `CString`. The other form opens `00 00 80 00 08 00 00 00`; it is 608 bytes and ends at the next object (a `moScalerDim_c` in the samples). A sheet-metal edge flange (`moEdgeFlange_c`) writes its counted children, then a display object, then a block that a reader frames by the next named object (a bend in the samples). A display dimension parsed at top level writes a view block after its favourite handle, opening u32 `0`, u32 `8`, u32 `0x01000000`, u32 `0`, u16 `0x0100` and the repeated eight-byte identifier; it ends at the next display object (472 bytes in the sample). The same block after a display inside a feature belongs to that feature. A top-level display dimension without that view block writes placement data before the next object: an empty Unicode `CString`, u32 values and zero bytes, all within 64 bytes of its end; without a `CString`, f64 and u32 values end at the next display object within 128 bytes.

**Need.** A writer that creates sheet-metal flanges has to write bends; a reader that keeps them has to read them.

## 7. Assembly documents

### AS-01. Mate-list record layout

**Question.** What is the layout of the `moMate*` records in `Contents/Config-N-MatesList` (`sldprt.md` §11.3), and how does each operand bind to its component instance and face?

**Known.** The section holds mate class tokens, mate feature names, instance names `<instance name>@<assembly name>`, the referenced documents' paths, and `SurfIdRep` face references. No field offsets are known.

**Need.** We must know the layout to transfer a mate as an assembly joint. Without it, the mates transfer only as occurrence placements.

### AS-02. Component placements of a compound-file assembly

**Question.** Where does a compound-file assembly (dialect `sw-version-pre-12000`) store its component instances and their placements?

**Known.** The compound file has `Contents/Config-0-MatesList` and no `swXmlContents` sections, so no `COMPINSTANCETREE`. `Contents/Config-0` is the candidate.

**Need.** We must know the records to transfer the occurrence tree of such an assembly. Without them, the occurrence structure does not transfer.

### AS-03. Stored placement of path-driven components

**Question.** Is the stored `swTransform` of a component that a path or chain mate drives the solved position of that component?

**Known.** In an assembly with three chain sub-assemblies, every one of their 264 leaf instances is 0.69 mm (median) to 85 mm from the nearest instance of the STEP export of the same assembly, while the other sub-assemblies agree to 3e-11 mm. In another assembly, four instances under one sub-assembly agree with neither the parent-relative nor the root-relative composition of §11.2. The export state of these STEP files is not known.

**Need.** We must know whether such placements need a solve before a consumer can use them as positions.

### AS-04. Parasolid streams of an assembly document

**Question.** What do the Parasolid streams in the `Contents/Config-N-ResolvedFeatures`, `Contents/Config-N-MatesList`, and `Contents/Config-N-GhostPartition` sections of an assembly document hold?

**Known.** The part decode of such a document gives no body. The streams are not in embedded documents (`sldprt.md` §11.4). Assembly features (cuts, holes) and the cached geometry of mate faces are the candidates.

**Need.** We must know their content to transfer it or to state that it is not model geometry. Without it, the document keeps the part losses that report an untransferred Parasolid model.

### EV-24. Bytes after the last top-level object

**Question.** What owns the four zero bytes some `ResolvedFeatures` streams write after their last top-level object, and the two zero bytes an in-context part writes after a top-level `moProfileFeature_c` whose sketch plane is written in place?

**Known.** `sldprt.md` §2.2 gives the stream framing. The trailing bytes lie outside every top-level object. The specification gives no field that names them.

**Need.** A writer that reemits the stream has to know whether to write them.
