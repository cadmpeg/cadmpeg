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

### BC-01. Other class-root layouts

**Question.** Which class-root record layouts and ownership relations select bodies outside the supported class-root chain form?

**Known.** `sldprt.md` §6 defines the supported class-root index prefix, chain heads, and the byte intervals used to associate entity records with a class-root body. Other class-root layouts remain unresolved.

**Need.** We must know the class-root grammar and body ownership rule to decode every body without assigning entities from one body to another.

**Note.** The closure evidence is limited to synthetic byte builders in `src/tests.rs:6889-6917` and `6921-6957`. `crates/cadmpeg-codec-sldprt/src/brep/entity.rs:258-271` accepts one distinct class-root vector, and `:473-558` assigns records by head-offset intervals. If a file has two valid heads with an unrelated face record between them, interval membership can bind that face to the wrong body; no stored relation is checked. The specification now states this interval grammar, but the closure did not establish it from an independent specimen or a native ownership field.

### BC-02. Deltas faces outside partition intervals

**Question.** Which class-root body owns a deltas face that is outside every selected partition head interval?

**Known.** `sldprt.md` §6 defines class-root interval ownership for selected partition records. A deltas face outside all selected intervals remains unresolved.

**Need.** We must know the ownership relation to retain faces that do not fall inside a partition interval.

**Note.** `crates/cadmpeg-codec-sldprt/src/brep/entity.rs:473-558` partitions entity records by class-root head offsets, and `crates/cadmpeg-codec-sldprt/src/brep/graph.rs:1439-1468` emits only faces claimed by the selected body relation. The closure test at `src/tests.rs:6921-6957` contains one controlled class-root chain and one deltas face. It does not establish ownership for a face outside all intervals. A multi-chain file can therefore withhold a valid deltas face or leave it attached to an arbitrary fallback body while the specification presents the interval rule as settled.

### BC-03. Superseded partition faces

**Question.** How does a deltas topology record replace a partition face when the replacement has a different bridge or owner identity?

**Known.** `sldprt.md` §6 states that partition topology has precedence for shared identities and that a complete deltas record can supersede a partition face when the native record identifies the replacement.

**Need.** We must know the replacement relation to avoid retaining a superseded partition face or emitting both faces.

**Note.** `crates/cadmpeg-codec-sldprt/src/brep/topology.rs:359-383` retains an existing partition bridge and merges only missing selected deltas bridges. `crates/cadmpeg-codec-sldprt/src/brep/graph.rs:775-809` orders partition streams before deltas. The test `partition_topology_wins_when_deltas_reuse_a_bridge_identity` covers only one reused identity; it does not prove replacement under a new attribute or owner. If a full deltas bridge is the replacement, the partition bridge remains because no replacement relation is decoded. The specification's supersession rule is therefore a promotion of an unverified precedence assumption.

### BC-04. Duplicate topology record identities

**Question.** Which occurrence is authoritative when one stream contains multiple valid topology records with the same attribute but different references or coordinates?

**Known.** Topology records are keyed by stream-local attribute. The specification does not define a duplicate-record rule or a native precedence field.

**Need.** We must select the authoritative topology record to avoid silently changing body connectivity or vertex positions when a duplicate identity is present.

**Note.** `crates/cadmpeg-codec-sldprt/src/brep/topology.rs:626-699` inserts bridges, vertex uses, and points by attribute, so a later valid occurrence replaces the earlier one. `:403-447` keeps the latest candidate occurrence for edge and coedge attributes. `crates/cadmpeg-codec-sldprt/src/brep/entity.rs:477-484` and `:565-573` likewise keep the record with the greatest sequence for duplicate entity attributes. If two complete records share an attribute but point to different loops or coordinates, the later or highest-sequence record controls the emitted topology without a decoded native conflict rule. Candidate evidence rejects some ambiguous edge and coedge shapes, but the other tables have no uniqueness gate.

### BC-05. Extended topology reference form

**Question.** Does a SolidWorks Parasolid topology stream encode a reference wider than `u16`?

**Known.** Typed topology records store `refs` as big-endian `u16` values at fixed strides in `brep/topology.rs`. The `u16` ceiling is real for every settled fixed-offset call site. Siemens NX encodes an extended XMT form as a negative signed remainder plus a quotient (`quotient * 32767 + remainder`).

**Need.** Corpus bytes that exercise a topology reference above `65535` before any encoding change. A confirmed extended form changes record length and turns the fixed-offset call sites into a running cursor.

**Note.** The NX extended form is a hypothesis for SolidWorks, not evidence. Do not invent the encoding without corpus bytes.

## 2. Geometry carriers

### GC-01. Non-isoparametric B-spline trim UV

**Question.** How do we derive the UV curve for a non-isoparametric trim on a B-spline face?

**Known.** `sldprt.md` §7.1 "00 TT [ff]?" through `sldprt.md` §7.3 "The chart is a solved cache" define exact pcurves for the supported analytic, boundary-isocurve, affine-axis interior-isocurve, polar-NURBS, ruled-surface, and complete intersection-cache cases. The affine-axis constructions apply symmetrically to the `u` and `v` axes. A complete width-4 intersection witness supplies co-parameterized solved UV caches for both support surfaces. The Parasolid stream does not store a general two-dimensional NURBS trim control array.

**Need.** We must know the convention to construct the trim in the surface parameter space.

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

**Note.** `crates/cadmpeg-codec-sldprt/src/brep/offset.rs:22-85` accepts the `00 3c` shape, discriminator bytes `V`, `I`, or `U`, a flag, a support attribute, and a finite distance. `crates/cadmpeg-codec-sldprt/src/brep/graph.rs:1507-1615` evaluates it as the same signed normal offset and uses it before blend and sweep fallbacks. The parser and nested-offset tests use synthetic records in `src/tests.rs:357-371`, `8549-8592`, and `8691-8715`; identical offset semantics for all discriminators and flags have not been verified against corpus records. A valid carrier with another meaning would be silently decoded as the same offset surface.

### GC-05. Variable-radius blend carriers

**Question.** Which native carrier stores the result and radius law of a variable-radius blend, and how does it bind to the feature history?

**Known.** `sldprt.md` §7.4 states that a variable-radius result uses a `00 7c` B-spline surface-use wrapper or NURBS carrier and that feature history carries its design intent.

**Need.** We must know the carrier and relation to reconstruct variable-radius blend geometry and its radius law.

**Note.** `crates/cadmpeg-codec-sldprt/src/brep/spline.rs:814-960` parses generic `00 7c` NURBS surface carriers, but does not identify a variable-radius blend. The variable-radius Keywords feature in `history.rs` carries history data only. The test `src/tests.rs:13308-13383` uses a synthetic Keywords feature and does not contain a native B-rep result carrier. If multiple generic `00 7c` surfaces exist, the decoder emits generic NURBS without a radius-law identity or a native history binding. The specification promotes an unverified association.

### GC-06. Surface-intersection surface carriers

**Question.** Which record carries surface-intersection surface geometry, and what is its payload grammar?

**Known.** `sldprt.md` §7.2 "Curve carriers: an edge's `00 10.refs[3]` can point to a `00 86` B-spline/list curve carrier," defines curve carriers for the intersection of two surfaces. It does not define a surface carrier for this geometry family.

**Need.** We must know the carrier to construct the exact surface.

### GC-07. Spline-on-surface carriers

**Question.** Which record carries spline-on-surface surface geometry, and what is its payload grammar?

**Known.** `sldprt.md` §7.1 "Stream-scope" defines B-spline surface and curve carriers. It does not define the relation that makes a spline a curve on a support surface.

**Need.** We must know the carrier and relation to construct the exact surface geometry.

### GC-08. Duplicate geometry carrier identities

**Question.** Which occurrence is authoritative when one stream contains multiple valid geometry carriers with the same stream-local attribute?

**Known.** Geometry carriers are indexed by stream-local attribute. The specification does not define a duplicate-identity rule or a precedence field.

**Need.** We must select the authoritative carrier without silently changing geometry when a duplicate record is present.

**Note.** `crates/cadmpeg-codec-sldprt/src/brep.rs:180-190` overwrites analytic carriers with `HashMap::insert`; `brep/blend.rs:135-155` keeps the first blend/support pair with `or_insert`; `brep/offset.rs:77-85` keeps the last offset carrier; and `brep/spline.rs:267-309`, `:732-747`, and `:814-960` keep the first descriptor or carrier. If two structurally valid records share an attribute but carry different geometry, the selected record depends on carrier family and byte order. The maps apply structural filters, but no native uniqueness or precedence relation is decoded.

### GC-09. Conflicting surface carrier families

**Question.** Which surface carrier family owns a face when analytic, offset, blend, or sweep candidates are all structurally valid for the same surface attribute?

**Known.** `sldprt.md` §7 defines analytic and procedural surface carriers, but does not define a conflict rule between valid carrier families.

**Need.** We must know the native family discriminator or ownership relation to select the exact face support.

**Note.** `crates/cadmpeg-codec-sldprt/src/brep/graph.rs:1507-1635` accepts an analytic surface first, then evaluates an offset, blend, and later sweep fallback. It does not prove that the rejected families are invalid when the first candidate is valid. If a face has valid carriers from two families, the branch order selects one without a native ownership field. Offset support and cycle checks reject some malformed candidates, but they do not settle the conflict when multiple candidates pass.

## 3. Container metadata

### CM-01. Cache-cell prefix

**Question.** What index-state information does the cache-cell prefix encode?

**Known.** `sldprt.md` §1.2 "The bytes before the first valid block and the long inter-block gaps hold a **fixed-cell" through `sldprt.md` §1.2 "A valid cache cell satisfies `two_L == 2L`, `half_L == L//2`, `0 < name_len < 500`, and has a" define the cache-cell grid, cell size, and section-index interpretation.

**Need.** We must know the prefix semantics to validate and write a cache cell.

### CM-02. Cache-cell fill

**Question.** What index-state information does the cache-cell fill encode?

**Known.** `sldprt.md` §1.2 "The bytes before the first valid block and the long inter-block gaps hold a **fixed-cell" through `sldprt.md` §1.2 "A valid cache cell satisfies `two_L == 2L`, `half_L == L//2`, `0 < name_len < 500`, and has a" define how nonzero cache-cell section indices address blocks.

**Need.** We must know the fill semantics to validate and write the grid.

### CM-03. Cache-cell `type_id` high half

**Question.** What does the high half of a cache-cell `type_id` encode?

**Known.** `sldprt.md` §1.2 "The bytes before the first valid block and the long inter-block gaps hold a **fixed-cell" through `sldprt.md` §1.2 "A valid cache cell satisfies `two_L == 2L`, `half_L == L//2`, `0 < name_len < 500`, and has a" define the cell `type_id` field and its relation to the addressed block.

**Need.** We must know the high-half semantics to validate and write the type identifier.

### CM-04. Tail-directory fill

**Question.** What index-state information does the variable-length fill after the final tail-directory entry encode?

**Known.** `sldprt.md` §1.3 "The file tail carries an **OPC package section directory**" through `sldprt.md` §1.3 "The file tail carries an **OPC package section directory**" define tail-directory entry framing and section lookup.

**Need.** We must know the fill semantics to validate and write the tail directory.

### CM-05. Other inline entity families

**Question.** What fixed slot count does each bare inline entity family outside the canonical face families use in each Parasolid schema?

**Known.** `sldprt.md` §5 "Top-level entity families include" defines the common entity header. `sldprt.md` §5 "Bare entity framing is defined for schema revisions" defines the supported schema revisions, disc families, and six-, seven-, and nine-slot forms. `sldprt.md` §10 "Bare inline `00 51` subrecords use a fixed slot count" defines bare record boundaries. A prefixed record is self-delimiting because its `[01][hi][lo]` slot run ends at the first `00` byte.

**Need.** We must know each bare slot count to find bare record boundaries without treating payload bytes as delimiters.

The decoder uses the schema revision and an explicit `(disc, flo) -> slot count` table for the bare entity families consumed by its body-layout recognizers. An unlisted schema revision or family remains unresolved. Prefixed records do not use the table because their zero terminator frames the reference run.

The attribute scanner accepts only the exact supported family names followed immediately by a `00 50` definition. It accepts only the list widths that the supported family grammars define: one, or five through seven. A name or list must fit in the remaining stream bytes. Name, definition, and list nodes must be nonsentinel. Duplicate definition and list identities must be byte-equivalent; the decoder withholds a conflicting identity. An instance becomes a relation only when its target survives as an emitted face or an explicit body.

### CM-06. Partition and deltas precedence

**Question.** Which topology record is authoritative when partition and deltas streams describe the same site with different valid records?

**Known.** `sldprt.md` §6 states that partition topology has precedence for shared identities and that deltas records fill missing topology.

**Need.** We must know the native precedence relation to merge partition and deltas topology without retaining stale or duplicate faces.

**Note.** `crates/cadmpeg-codec-sldprt/src/brep/graph.rs:775-809` sorts streams by whether their description contains `deltas`, and `brep/topology.rs:359-383` preserves the partition bridge while merging missing deltas records. The only closure test, `partition_topology_wins_when_deltas_reuse_a_bridge_identity`, uses the same bridge identity. It does not establish precedence for equal-site records with different valid topology. The stream description and iteration order are not evidence of a native precedence field.

### CM-07. `moTransRefPlaneData_c` gap

**Question.** What does the byte run between the `moTransRefPlaneData_c` class token and the first of its nine f64 values encode, and what fixes its length?

**Known.** The decoder requires an eight-byte `ff` prefix and reads nine little-endian f64 values after it. The values are the plane center, two in-plane directions, and their normal. A valid transformed reference plane record follows the plane center xyz.

**Need.** We must know the gap to write the record back without moving or inventing undecoded bytes.

**Note.** `crates/cadmpeg-codec-sldprt/src/metadata.rs:43-89` still validates the f64 block by finite-value and extent checks after the fixed prefix. Commit `058e16cbf` changed the scan from the first plausible offset to the `ff` prefix and added the synthetic rejection test `src/tests.rs:22953-22972`; the generated fixture supplies the observed bytes. The prefix-and-nine-values rule has not been verified against corpus records or against complete record framing. Another token occurrence with the same prefix and bounded numbers can be emitted as a plane, while a valid revision with a different gap is skipped.

### CM-09. Active body stream selection

**Question.** Which field identifies the active B-rep stream when no configuration record selects one?

**Known.** The active configuration's `SourceIndex=N` selects `Config-N-Partition`. `Config-N-Deltas`, `Config-N-GhostPartition`, and `Config-N-ResolvedFeatures` do not substitute for that partition. Without an explicit active source index, a sole non-ghost partition is active. Multiple partitions leave active geometry identity unresolved. Stream size and container order do not select one.

**Need.** We must know whether another stored field selects one partition when multiple partitions exist and no configuration record supplies `SourceIndex`.

**Note.** `crates/cadmpeg-codec-sldprt/src/decode.rs:1918-1947` selects `max_by_key((faces, bodies, points, reverse(index)))` when `container::select_active_parasolid` returns no unique candidate. If a file has two non-ghost partition sites and no `SourceIndex`, the richer site is selected without a native identity field. The container selector itself requires an unambiguous candidate, but the decode fallback bypasses that unresolved state.

### CM-10. Parasolid stream boundary

**Question.** What fixes the end of a Parasolid stream inside one block payload?

**Known.** `sldprt.md` §3.1 gives the stream header: the `PS 00 00` signature, `desc_len u16 BE`, the description, the padding, `schema_len u8`, and the schema. The header has no total-length field. `sldprt.md` §3.2 states that a block can hold a partition stream and a deltas stream, and gives no delimiter between them.

The decoder treats a later `PS\0\0` as a boundary only when the bytes from that position contain a complete stream header with a bounded description and schema token. An unframed signature in coordinate or string data remains part of the current stream.

**Need.** We must know the authoritative boundary to distinguish a real following stream from payload bytes that happen to contain a complete header-shaped sequence.

### CM-14. Configuration body membership

**Question.** Which field binds an inactive configuration without `SourceIndex` to its bodies?

**Known.** Decimal `SourceIndex=N` binds a Keywords configuration to `Config-N-Partition`. The Keywords decimal `id` selects `Config-N-ResolvedFeatures` and is independent of the partition identity. Element order and regeneration ordinal do not bind a partition. A uniquely named active configuration can use the active geometry partition. `crates/cadmpeg-ir/src/features.rs:57` defines `ConfigurationBodies::Unresolved` for every remaining configuration whose body membership is not established.

**Need.** We must know whether another stored field binds an inactive source-less configuration. The decoder keeps this body membership `Unresolved` and reports the loss.

## 4. Auxiliary lanes

### AL-01. DisplayLists face ranges

**Question.** How does a B-rep face attribute select its triangle range in a DisplayLists block?

**Known.** `sldprt.md` §7.3 "**`00 28` chart**" defines the DisplayLists descriptor table, strip lengths, and triangle-count relations. `sldprt.md` §5 "Face records use these families:" defines B-rep face identities. `sldprt.md` §8 defines unambiguous ownership from incidence with one analytic face support and from complete convex planar trims with circular inner loops. Other coincident trims and non-analytic supports do not supply the stored face-range mapping.

**Need.** We must know the stored mapping to attach tables whose support is ambiguous or non-analytic.

### AL-03. DisplayLists extended table-header token

**Question.** What does the nonzero extended-form token encode?

**Known.** `sldprt.md` §8 defines the common triangle-count and strip-count cells and the exact descriptor-table position of the compact and extended `uoTempFaceTessData_c` forms. The common cells do not select the form. The fixed extension cells and its nonzero token select the extended form.

**Need.** We must know the token semantics to regenerate the extended form without a retained source record.

### AL-04. Mesh polyline candidate selection

**Question.** Which native field identifies the mesh polyline used as a helix input when a stream contains more than one finite `00 22` point array?

**Known.** `sldprt.md` §2 defines the supported helix input as a counted XYZ polyline. It does not define a native rule that ranks multiple candidate arrays by point count.

**Need.** We must know the polyline identity to bind the helix to the correct input geometry.

**Note.** `crates/cadmpeg-codec-sldprt/src/parasolid.rs:183-243` scans every `00 22` array, retains finite XYZ candidates, sorts by scalar count, and selects the largest; equal-size candidates are rejected. If a feature-input payload contains a mesh array and an unrelated finite point array, the largest array is selected even when no native field binds it to the helix. The finite/count shape gate and tie rejection are counter-evidence against arbitrary byte acceptance, but they do not establish the largest-array rule.

### AL-05. Appearance ownership beyond DisplayLists

**Question.** Which records bind appearances to a part, configuration, display state, or B-rep-only feature output?

**Known.** `sldprt.md` §8 defines DisplayLists body defaults, feature assignments through framed persistent surface references, face-local assignments, and their precedence. A `moVisualProperties_c` definition alone does not establish ownership. The opaque six-value `ATOM_ID_2001` layout does not establish a DisplayLists feature binding.

**Need.** Part ownership, configuration and display-state selection, missing or conflicting persistent surface references, and feature appearance propagation to B-rep-only geometry remain unresolved.

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

### DI-05. Offset-edge relation invariant

**Question.** What neutral geometric invariant does an offset-edge marker relation impose?

**Known.** `sldprt.md` §2 "A `Config-N-ResolvedFeatures` lane supplies the evaluated parameter state for configuration slot" through `sldprt.md` §2 "A primary line-or-circle geometry handle on a transformed line segment identifies that line" define operand resolution and the neutral invariants of the supported dimensional and geometric relations.

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

**Known.** The native numeric taxonomy defines identities through code `85`. `sldprt.md` §2 "A `Config-N-ResolvedFeatures` lane supplies the evaluated parameter state for configuration slot" through `sldprt.md` §2 "A primary line-or-circle geometry handle on a transformed line segment identifies that line" define the neutral semantics of the supported codes.

**Need.** We must know each invariant and operand family to project the remaining codes.

### DI-09. Relation codes above `85`

**Question.** What native relation does each code above `85` identify?

**Known.** The defined native numeric taxonomy ends at code `85`.

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

**Known.** The exact compact legacy and extended radial layouts, their native code and role fields, and the `sgSlot` relation identify the supported dimensioned-circle roster. The selected profile stream can omit construction circles.

**Need.** We must know the discriminator to prevent an omitted construction circle from becoming profile geometry.

**Note.** `crates/cadmpeg-codec-sldprt/src/resolved_features/dimensions.rs` now joins each omitted dimension center to the native radial-circle roster and propagates the radial record's role-2 construction state through dimension-carrier, point-witness, and circle-only-carrier projections. A non-empty lane with no unique radial role or with conflicting role matches remains native instead of becoming profile geometry. The remaining unknown is the native discriminator for a dimension carrier that has no recoverable radial record at all.

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

**Known.** The point-reference form retains the edge and endpoint selector. `sldprt.md` §2 "A `Config-N-ResolvedFeatures` lane supplies the evaluated parameter state for configuration slot" states how ordinary point-reference object indices resolve.

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

**Known.** `sldprt.md` §2 "An extrusion object-name record is followed by four zero bytes, a little-endian u16 family word," through `sldprt.md` §2 "When operation objects and their dimension children form separate ordered groups, a blind" define the `moSingleFaceRef_w` path forms and their feature-local face selection. `sldprt.md` §5 "Face records" defines how the terminal owner and feature-local face identity select the surviving face through `ATOM_ID_2001`.

**Need.** We must distinguish a face consumed by a later operation from a face whose owner or identity attribute was not decoded.

### DI-21. Other inline extrusion operation bytes

**Question.** What Boolean operation does each inline family-word and operation-byte combination outside family `0x0140` byte `00` and family `0x01ca` byte `00` or `02` represent?

**Known.** `sldprt.md` §2 "Feature-tree" defines the three supported combinations. The family word establishes join or cut independently of a shared class token; family `0x01ca` byte `00` derives subtraction from its family rather than from the zero byte alone.

**Need.** We must know the other byte values to construct the extrusion Boolean operation.

### DI-22. Other sparse extrusion form codes

**Question.** What operation and record shape does each sparse extrusion object outside the defined form codes use?

**Known.** `sldprt.md` §2 "Keywords element order is serialization order, not regeneration order. Neutral regeneration" through `sldprt.md` §2 "Feature-tree" define direct, repeated-class, sentinel, terminated-trailer, and sparse-trailer objects. Form code `11` identifies an object form and does not determine its Boolean operation. A Keywords operation token or a complete inline operation trailer supplies the operation. Without either carrier, the operation remains unresolved and the decoder reports a design loss.

**Need.** We must know the remaining form codes to parse each object and construct its Boolean operation.

### DI-23. Combine-body reconciliation

**Question.** How does a body selected by a compact `moCombineBodies_c` target or tool path map to a body in the final B-rep?

**Known.** The compact paths identify generated feature-local bodies. A complete target or tool path projects to one generated body reference whose feature is the terminal path producer, whose local identity is the ordered path-local sequence, and whose dependencies include every uniquely identified traversed history feature. `sldprt.md` §4.2 "00 01 00 01" through `sldprt.md` §6 "The disc22-disc12-face layout uses one `0x22/flo2` region with a slot-1 sentinel and the chain" define final B-rep body identities and ownership. `ATOM_ID_2001` carries face identities and does not bind a feature-local body identity.

**Need.** We must know the mapping to bind the Boolean target and tool bodies.

### DI-24. Delete-body reconciliation

**Question.** How does an `moDeleteBody_c` regeneration-input-local body identity map to a body in the final B-rep?

**Known.** The compact record retains the regeneration-input-local identity. `sldprt.md` §4.2 "00 01 00 01" through `sldprt.md` §6 "The disc22-disc12-face layout uses one `0x22/flo2` region with a slot-1 sentinel and the chain" define final B-rep body identities and ownership.

**Need.** We must know the mapping to delete the selected final body.

### DI-25. Component-edge path reconciliation

**Question.** How does an edge selected by an entry-form `moCompEdge_c` path map to an edge in the final B-rep?

**Known.** The entry path identifies a generated feature-local edge. `sldprt.md` §3.2 "An attribute" through `sldprt.md` §4 "**Bridge `00 0e`:** `refs[2]` = owning loop-head, `refs[4]` = primary surface carrier (compact" define final B-rep edge identity and direction. `ATOM_ID_2001` carries no edge identity. The `EDGE_PERM_ID_2_2003` attribute grammar is not defined.

**Need.** We must know the mapping to bind the selected operation edge.

### DI-26. Compact edge-vector B-rep reconciliation

**Question.** How does a compact edge vector map its feature-local edge path to a final B-rep edge?

**Known.** `sldprt.md` §2 defines the duplicated-marker framing, count, lane-local selector, typed and reference-list entry forms, separators, terminal slots, and lane-local type signatures. The decoder retains the ordered compact vector and its producer-feature paths. The terminal component identifies a feature-local generated edge, but it does not by itself identify the final B-rep edge.

**Need.** We must define the topology join from the retained compact edge identity to the final B-rep edge before projecting the selected edge.

### DI-27. Component-surface-body reconciliation

**Question.** How does a face selected by an `moCompSurfaceBody_c` path map to a face in the final B-rep?

**Known.** The path identifies a generated feature-local face. `sldprt.md` §4.2 "Deltas streams re-encode records in prefixed/tripled forms (each ref stored as a `[hi][lo][01]`" defines final B-rep face identities.

**Need.** We must know the mapping to bind the selected surface-body faces.

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

**Note.** `crates/cadmpeg-codec-sldprt/src/resolved_features/axes.rs` collects every structurally valid direction candidate for an addressed width and rejects the record when distinct candidates disagree. The documented specific-layout precedence remains only when the candidates agree; the native authoritative width and trailer semantics remain unknown.

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

### DI-37. Circular-pattern axis carrier

**Question.** Which native record supplies the axis origin and direction for an `moCirPattern_c` interval after its seed identity resolves?

**Known.** `sldprt.md` defines the `moCirPatternSurfIdRep_c` generated-surface identity path and its seed join. The interval retains the circular count and angle parameters. The identity path does not contain a complete axis frame, so the neutral circular-pattern axis remains unresolved.

**Need.** We must recover the axis frame to construct `PatternKind::Circular` and replay the pattern.

### DI-33. SWIFT feature-to-topology identity

**Question.** How does each `GdtAnalysis.CadRef.CadIdentifier` select a Parasolid body, face, edge, or vertex identity?

**Known.** `sldprt.md` §2.1 defines the SWIFT annotation-to-feature graph. Each semantic feature can own a `CadReferences` collection. Each `CadRef` stores a `CadIdentifier`. An empty identifier supplies no topology identity. The annotation target remains the stable GDT-analysis feature identity when that identifier does not resolve to one neutral topology object.

**Need.** We must know the identity conversion to attach semantic PMI to the exact neutral topology object.

## 6. Write-path evidence

### EV-03. Regenerated `SWObjects` record content

**Question.** What do the undecoded bytes of a regenerated `SWObjects` metadata record hold?

**Known.** The semantic writer computes machine-local digests over the decoded metadata attributes, their source identities, and the material state represented by `SWObjects`. When that state is unchanged, it replays every retained `SWObjects` section byte-for-byte. It patches decoded fixed-width metadata fields in place while preserving every other byte. It refuses material edits, record-set changes, and variable-width edits because replacing or shifting a retained record would discard or relocate undecoded bytes. A source-less document uses the generated record forms.

The decoder reads configuration-manager fields at `+66`, `+107`, and `+117`, two fields of `moPart_c`, and the colour and name of `moVisualProperties_c`. The source-less writer emits a 125-byte configuration-manager record with every other byte zero, a 13-byte `moPart_c` record, and the constant `0x00c0_c0c0` inside each material record. That constant is not in `sldprt.md`. The two record lengths are not fixed by any field or specification sentence.

The metadata attributes carry `Exactness::ByteExact`. Retained-section replay satisfies that contract for an unchanged decoded state.

**Need.** We must know what the undecoded bytes hold before the writer can edit a retained record or claim that a generated record is complete.

### EV-04. Tail-directory descriptor semantics

**Question.** Which value does a tail-directory entry's 14-byte descriptor hold?

**Known.** `sldprt.md` §1.3 "The file tail carries an **OPC package section directory**" states that the 6-byte trailer "has one value for all entries in a file, for example `e5 4b 57 5b 00 00`", and that its first four bytes are the directory separator.

The decoder retains each descriptor and trailer. A rewritten existing entry retains its descriptor. Every generated entry uses the source directory's unique trailer, so one directory does not mix separators.

**Need.** We must know the descriptor semantics before a new entry can derive a nonzero descriptor without a same-name source entry.
