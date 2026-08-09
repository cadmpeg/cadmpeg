# SolidWorks `.sldprt`: Open Items

This document lists the parts of the SolidWorks `.sldprt` format that we do not know. The specification `sldprt.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Body classification

## 2. Geometry carriers

### GC-01. Non-isoparametric B-spline trim UV

**Question.** How do we derive the UV curve for a non-isoparametric trim on a B-spline face?

**Known.** `sldprt.md` §7.1 "00 TT  [ff]?" through `sldprt.md` §7.3 "The chart is a solved cache" define exact pcurves for the supported analytic, boundary-isocurve, affine-axis interior-isocurve, polar-NURBS, ruled-surface, and complete intersection-cache cases. The affine-axis constructions apply symmetrically to the `u` and `v` axes. A complete width-4 intersection witness supplies co-parameterized solved UV caches for both support surfaces. The Parasolid stream does not store a general two-dimensional NURBS trim control array.

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

**Question.** Which record carries offset-surface geometry, and what is its payload grammar?

**Known.** `sldprt.md` §6 "The disc1e-disc14-face layout uses one `0x1e/flo2` region with a slot-1 sentinel and the chain" through `sldprt.md` §7.3 "**`00 28` chart** — the solved point cache:" define compact analytic, B-spline, intersection, and constant-radius rolling-ball surface carriers.

**Need.** We must know the carrier to construct an exact offset surface.

### GC-05. Variable-radius blend carriers

**Question.** Which record carries non-constant-radius blend geometry, and what is its payload grammar?

**Known.** `sldprt.md` §7.4 "A `00 38` surface carrier defines a circular rolling-ball blend between two support surfaces:" defines the `00 38` constant-radius rolling-ball surface. Its two offsets have one common nonzero magnitude.

**Need.** We must know the other carrier to construct a variable-radius blend surface.

### GC-06. Surface-intersection surface carriers

**Question.** Which record carries surface-intersection surface geometry, and what is its payload grammar?

**Known.** `sldprt.md` §7.2 "Curve carriers: an edge's `00 10.refs[3]` can point to a `00 86` B-spline/list curve carrier," defines curve carriers for the intersection of two surfaces. It does not define a surface carrier for this geometry family.

**Need.** We must know the carrier to construct the exact surface.

### GC-07. Spline-on-surface carriers

**Question.** Which record carries spline-on-surface surface geometry, and what is its payload grammar?

**Known.** `sldprt.md` §7.1 "Stream-scope" defines B-spline surface and curve carriers. It does not define the relation that makes a spline a curve on a support surface.

**Need.** We must know the carrier and relation to construct the exact surface geometry.

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

**Known.** `sldprt.md` §5 "Top-level entity families:" defines the common entity header. `sldprt.md` §10 "Bare inline `00 51` subrecords use a fixed slot count" defines bare record boundaries. A prefixed record is self-delimiting because its `[01][hi][lo]` slot run ends at the first `00` byte.

**Need.** We must know each bare slot count to find bare record boundaries without treating payload bytes as delimiters.

**Note.** `crates/cadmpeg-codec-sldprt/src/brep/entity.rs:78` gives three families an explicit slot count and gives every other family the count `6`:

```rust
_ => 6,
```

The table has no schema dimension. The count bounds bare u16 references only. A wrong bare count changes every reference the layout recognizers chain through. The decoder has no branch that refuses an unlisted bare family.

`crates/cadmpeg-codec-sldprt/src/brep/attrib.rs:78` and `attrib.rs:126` accept a `00 4f` name length and a `00 52` list count in the range `1..64`. `sldprt.md` states no bound for either. Both modules scan every byte offset for the tags, which is the practice this item names.

### CM-07. `moTransRefPlaneData_c` gap

**Question.** What does the byte run between the `moTransRefPlaneData_c` class token and the first of its nine f64 values encode, and what fixes its length?

**Known.** `sldprt.md` §8 "**Materials / metadata**" gives the field offsets of each document metadata record from the end of its class token. Every other record in that table starts its fields at token end +0. This one starts them after a gap. The decoder finds the value block by the first offset in `0..64` at which nine finite f64 values satisfy the extent constraints.

Observed gap:

| gap length | bytes | record that follows |
| --- | --- | --- |
| 8 | `ff ff ff ff ff ff ff ff` | plane center xyz |

**Need.** We must know the gap to write the record back. A writer that omits it moves every later record in the SW Objects payload, which moves the byte offset each `sldprt:metadata:` identifier carries, so a rewrite that changes nothing still renames those attributes.

### CM-09. Active body stream selection

**Question.** Which field identifies the active B-rep stream when no configuration record selects one?

**Known.** `sldprt.md` §1.2 names the `Config-N-Partition`, `Config-N-Deltas`, and `Config-N-GhostPartition` families. It gives no rank between them and no rule that selects one as the active B-rep.

`crates/cadmpeg-codec-sldprt/src/container.rs:716` selects with a weighted score:

```rust
let mut score = (ps.len() / 64) as i64;
... score -= 1_000_000;   // ghost
... score -= 1_000_000;   // resolvedfeatures
... score += 100_000;     // partition
... score += 1_000_000;   // matches the active configuration
... score += 50_000;      // deltas
```

The values `64`, `50_000`, `100_000`, and `1_000_000` are not in `sldprt.md` or in `docs/layouts/sldprt.toml`. The comparison uses `>`, so the earlier block wins an equal score. The function has no branch that withholds.

`crates/cadmpeg-codec-sldprt/src/container.rs:691` uses a second rule for the compound envelope. It takes the largest body stream with `max_by_key` and gives partition and deltas equal rank.

**Need.** We must know the field to select the active stream. Stream size is not a defined selector.

**Note.** The test for `partition` uses the section name only. The test for `deltas` uses the section name or the stream description. `crates/cadmpeg-codec-sldprt/src/parasolid.rs:188` shows that the description carries both words. A block with an empty or unprintable preamble therefore gets no partition score while a sibling deltas block still gets `50_000` from its description. The writer then patches the deltas lane. Correct this asymmetry with the selection rule.

### CM-10. Parasolid stream boundary

**Question.** What fixes the end of a Parasolid stream inside one block payload?

**Known.** `sldprt.md` §3.1 gives the stream header: the `PS 00 00` signature, `desc_len u16 BE`, the description, the padding, `schema_len u8`, and the schema. The header has no total-length field. `sldprt.md` §3.2 states that a block can hold a partition stream and a deltas stream, and gives no delimiter between them.

`crates/cadmpeg-codec-sldprt/src/parasolid.rs:32` ends each stream at the next `PS\0\0` signature in the payload. `parasolid.rs:95` repeats the rule. `stream_header` reads the front of each candidate only, so a stream that a signature inside its own payload cuts short keeps a valid header and is accepted. The decoder records no loss.

**Need.** We must know the boundary to read a complete stream. A `PS\0\0` byte sequence in coordinate or string data must not end a stream.

### CM-13. Primary site selection

**Question.** Which field identifies the primary B-rep site when a file has more than one decodable site?

**Known.** `sldprt.md` §3.2 defines a site. It gives no rule that ranks sites.

`crates/cadmpeg-codec-sldprt/src/decode.rs:1910` selects the site with the most faces, then the most bodies, then the most points:

```rust
let score = (decoded.faces.len(), decoded.bodies.len(), decoded.points.len());
```

The other sites are merged into the model. Each untyped surface and curve retains the outer block or compound-stream identity of its own site.

The decoder has a second and different idea of the active stream, `container::select_active_parasolid`, which uses `swConfigurationName`. `decode.rs:2637` uses that one for the `active_parasolid_block` attribute. The two are not reconciled.

**Need.** We must know the field to select the primary site.

### CM-14. Configuration body membership

**Question.** Which field binds an inactive configuration without `SourceIndex` to its bodies?

**Known.** Decimal `SourceIndex=N` binds a Keywords configuration to `Config-N-Partition`. The Keywords decimal `id` selects `Config-N-ResolvedFeatures` and is independent of the partition identity. Element order and regeneration ordinal do not bind a partition. A uniquely named active configuration can use the active geometry partition. `crates/cadmpeg-ir/src/features.rs:57` defines `ConfigurationBodies::Unresolved` for every remaining configuration whose body membership is not established.

**Need.** We must know whether another stored field binds an inactive source-less configuration. The decoder keeps this body membership `Unresolved` and reports the loss.

## 4. Auxiliary lanes

### AL-01. DisplayLists face ranges

**Question.** How does a B-rep face attribute select its triangle range in a DisplayLists block?

**Known.** `sldprt.md` §7.3 "**`00 28` chart**" defines the DisplayLists descriptor table, strip lengths, and triangle-count relations. `sldprt.md` §5 "Face records use these families:" defines B-rep face identities.

**Need.** We must know the mapping to attach each tessellated triangle to its face.

### AL-02. DisplayLists descriptor table position

**Question.** What fixes the position of the descriptor table after a `uoTempFaceTessData_c` class token?

**Known.** `sldprt.md` §7.3 "**`00 28` chart**" gives the descriptor relations `C = sum(ListA)`, `ListC[i] = 2*ListA[i] - 2`, and `TriCount = C - 2*N`. It gives no offset for the table. `docs/layouts/sldprt.toml` records the offset as `not_applicable` and states that the parser asserts fixed offsets that the specification does not give.

`crates/cadmpeg-codec-sldprt/src/tessellation.rs:257` tries two offsets in order and takes the first that frames:

```rust
for relative in [8usize, 40] {
    if let Some((mesh, mut at)) = parse_table(payload, end + relative) {
```

`parse_table` does check the strip totals against the vertex count, so a distant offset is refused. The decoder does not test that the `+8` reading is invalid before it accepts it, and it does not withhold when both offsets frame.

**Need.** We must know the offset to find the table. Two offsets that both frame must not select by order.

### AL-03. Color record framing

**Question.** What frames a `00 53` color record, and what does a second record for one attribute denote?

**Known.** `sldprt.md` §5 names `00 53` as a color, property, or helper record. It gives no field grammar. `sldprt.md` §5 states that top-level tag bytes inside slots are data and not record delimiters.

`crates/cadmpeg-codec-sldprt/src/brep/entity.rs:141` accepts a record at any byte offset when the tag is `00 53`, the low byte of the flags is `3`, the attribute is more than `1`, and three big-endian f64 values are finite and inside `0.0..=1.0`:

```rust
if attr <= 1 || ![r, g, b].iter().all(|value| value.is_finite() && (0.0..=1.0).contains(value)) {
    return None;
}
```

The range test is the acceptance rule, not a decoded invariant. `entity.rs:182` inserts each hit into a map keyed by attribute, so a second hit for one attribute replaces the first. The decoder records no loss.

The disc-`0x0014` path reads its color at a computed offset and does not use this scan.

**Need.** We must know the framing to find the records without a scan of every offset. A normalized knot vector or a unit direction after the same two tag bytes must not become a color.

## 5. Design intent

### DI-01. Other optional feature-manager node identities

**Question.** What native field identifies the role of each optional classless feature-manager node that does not satisfy a defined layout-scoped identity?

**Known.** `sldprt.md` §2 "An `moCurvePattern_c` feature-input object is immediately preceded by its seed feature object" through `sldprt.md` §2 "An `moLPattern_c` interval without a line-reference record carries each displayed translation" identify the annotations container, principal planes, model origin, lights-and-cameras container, ambient and directional lights, sheet-metal node, and exploded-views container. The identities depend on a complete native-class roster. Other source identifiers are allocation positions and are not role codes. `sldprt.md` §2 "A classless Keywords `Feature` whose `Type` token is `EquationDriven` is the equation" identifies the equation container by its operation-family token, which is a role code and needs neither a native class nor a reserved source identifier.

**Need.** We must distinguish the remaining binders, comments, body folders, materials, notes, sensors, favorites, history, selection sets, and markups.

**Note.** The decoder resolves the light part of this question without a role code. `crates/cadmpeg-codec-sldprt/src/history.rs:301` groups the payload-free classless nodes by kind token and gives a group a scene light class when the group size equals the count of anonymous instances of that class:

```rust
let candidates = groups.values().filter(|indices| indices.len() == *count).collect::<Vec<_>>();
let [indices] = candidates.as_slice() else { return None; };
```

Equal cardinality is the only link between the group and the class. `sldprt.md` §2 "An `moLPattern_c` interval without a line-reference record carries each displayed translation" binds classless light nodes by reserved source identifier and by kind token, not by count. A group that takes a light class loses its `reserved_feature_tree_node_role` path, because that path needs `input_class.is_none()`.

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

**Note.** A second narrowing stage resolves the case this item records as unresolved. `crates/cadmpeg-codec-sldprt/src/resolved_features/transforms.rs:254` keeps only the candidates whose axis swap and axis signs equal those of the placement frame:

```rust
let oriented = candidates.iter().copied().filter(|candidate| {
    candidate.swap == frame.swap && candidate.u_sign == frame.u_sign && candidate.v_sign == frame.v_sign
}).collect::<Vec<_>>();
if oriented.is_empty() { candidates.to_vec() } else { oriented }
```

`sldprt.md` §2 states that placement supplies the transform only when the anchors do not determine one, and that a transform-dependent marker stays unresolved. Frame orientation is not a listed precedence rule. This stage runs after the cascade this item names, and also on the surface-derived path at `resolved_features/dimensions.rs:379`.

### DI-11. Ambiguous linked reference markers

**Question.** Which profile entity does a reference marker select when its linked loci do not identify one entity?

**Known.** `sldprt.md` §2 "A Keywords configuration's decimal `id` attribute is the slot identity for" and `sldprt.md` §2 "Keywords length literals use the suffixes `uin`, `mil`, `mm`, `cm`, `in`, `ft`, `nm`, `um`, `µm`" define unique linked-handle and shared-entity resolution.

**Need.** We must select one entity to bind the reference operand.

### DI-13. Marker-only profile placement

**Question.** How is a marker-only profile placed in model space when it has no local, contextual, or unique lane-wide compact reference-plane record?

**Known.** `sldprt.md` §2 "Point-reference cells address feature-local object indices within the owning feature object" defines model-space recovery from a planar sketch placement. `sldprt.md` §2 "An `moLPattern_c` feature-input object is immediately preceded by its seed feature object. That" through `sldprt.md` §2 "Among classless, parameterless, propertyless history records, `Feature` source ID `1` is the" define the supported reference-plane frames.

**Need.** We must know the placement to transfer the profile from planar coordinates to model coordinates.

### DI-14. Other Keywords operation families

**Question.** What neutral operation does each Keywords operation family outside the typed neutral feature set represent?

**Known.** `sldprt.md` §2 "A detailed curve record is immediately followed by a curve-detail marker of the same generation:" through `sldprt.md` §2 "A bare `0` or `1` Keywords dimension bound to a unique driving distance" define the native records and operands of the supported history operations.

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

**Question.** What Boolean operation does each inline operation byte other than `moExtrusion_c` byte `00` and `moICE_c` byte `02` represent?

**Known.** `sldprt.md` §2 "Feature-tree" defines those two operation bytes and states that `moICE_c` byte `00` does not carry an operation.

**Need.** We must know the other byte values to construct the extrusion Boolean operation.

### DI-22. Other extrusion form codes

**Question.** What distinguishes a joining `moICE_c` form-`11` object from a subtracting form-`11` object, and what operation and record shape does each sparse extrusion object outside the defined form codes use?

**Known.** `sldprt.md` §2 "Keywords element order is serialization order, not regeneration order. Neutral regeneration" through `sldprt.md` §2 "Feature-tree" define direct, repeated-class, sentinel, terminated-trailer, and sparse-trailer objects. Most form-`11` objects subtract, but a minority join.

**Need.** We must know the discriminator and the other form codes to parse the object and construct its Boolean operation.

**Conflict.** `sldprt.md` §2 "An extrusion object-name record is followed by four zero bytes, a little-endian u16 family word," states the answer as fact: "Form code `3` joins and form code `11` subtracts for either class." This item states that most form-`11` objects subtract and a minority join. One of the two documents is wrong.

`crates/cadmpeg-codec-sldprt/src/resolved_features/operations.rs:89` follows the specification sentence and always gives `Cut`:

```rust
(Some("moICE_c"), 0 | 1 | 2 | 5 | 7 | 10 | 14 | 15 | 22_993 | u32::MAX) | (_, 11) => {
    Some(BooleanOp::Cut)
}
```

There is no unresolved branch for code `11` and no loss. A joining form-`11` object subtracts in the neutral model. Decide which document is correct before the code changes.

### DI-23. Combine-body reconciliation

**Question.** How does a body selected by a compact `moCombineBodies_c` target or tool path map to a body in the final B-rep?

**Known.** The compact paths identify generated feature-local bodies. `sldprt.md` §4.2 "00 01 00 01" through `sldprt.md` §6 "The disc22-disc12-face layout uses one `0x22/flo2` region with a slot-1 sentinel and the chain" define final B-rep body identities and ownership. `ATOM_ID_2001` carries face identities and does not bind a feature-local body identity.

**Need.** We must know the mapping to bind the Boolean target and tool bodies.

### DI-24. Delete-body reconciliation

**Question.** How does an `moDeleteBody_c` regeneration-input-local body identity map to a body in the final B-rep?

**Known.** The compact record retains the regeneration-input-local identity. `sldprt.md` §4.2 "00 01 00 01" through `sldprt.md` §6 "The disc22-disc12-face layout uses one `0x22/flo2` region with a slot-1 sentinel and the chain" define final B-rep body identities and ownership.

**Need.** We must know the mapping to delete the selected final body.

### DI-25. Component-edge path reconciliation

**Question.** How does an edge selected by an entry-form `moCompEdge_c` path map to an edge in the final B-rep?

**Known.** The entry path identifies a generated feature-local edge. `sldprt.md` §3.2 "An attribute" through `sldprt.md` §4 "**Bridge `00 0e`:** `refs[2]` = owning loop-head, `refs[4]` = primary surface carrier (compact" define final B-rep edge identity and direction. `ATOM_ID_2001` carries no edge identity. The `EDGE_PERM_ID_2_2003` attribute grammar is not defined.

**Need.** We must know the mapping to bind the selected operation edge.

### DI-26. Compact edge-identity vectors

**Question.** What grammar and identity namespace does a compact edge vector use?

**Known.** Entry-form `moCompEdge_c` paths have a defined component-path grammar. Compact vectors remain separate native records.

**Need.** We must know the grammar and namespace to bind their selected edges.

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

## 6. Write-path evidence

### EV-01. Unpinned edit validators

**Question.** Which edit shape does each write-path validator refuse?

**Known.** `crates/cadmpeg-codec-sldprt/src/history.rs` guards `sync_neutral_features` with five validators, called from two places in the same function: `validate_compact_body_selection_edits`, `validate_compact_edge_selection_edits`, `validate_compact_surface_selection_edits`, `validate_surface_sweep_profile_edits`, and `validate_embedded_helix_edits`. A kill test made all five return `Ok` for every input and ran the complete sldprt suite. One test failed, and it covers the compact body-selection validator. The other four have no test that reaches their refusal.

Each of the five opens with a guard that returns `Ok` when the document carries no native graph, so a neutral-only document passes all five without a check.

**Need.** We need one negative test for each of the four unpinned validators. The test must build the edit shape that the validator refuses and must assert the error through the encode path, so that removing the validator fails the suite.

**Note.** `crates/cadmpeg-codec-sldprt/src/history.rs:16494` `dependency_residual` is a sixth guard with the same defect. For a `Pattern` feature it returns `Vec::new()` for both the expected and the projected side, so the consistency gate at `history.rs:16407` is always true. For extrude, revolve, sweep, loft, and rib it removes every sketch-typed dependency from both sides, so a changed profile dependency also passes. Give this guard a negative test with the other five.

### EV-03. Regenerated `SWObjects` record content

**Question.** What do the undecoded bytes of a regenerated `SWObjects` metadata record hold?

**Known.** `crates/cadmpeg-codec-sldprt/src/writer.rs:806` drops every source section whose name contains `swobjects`, and `writer.rs:126` writes a regenerated payload in its place. There is no replay path.

The decoder reads three fields of the configuration-manager record (`metadata.rs:268`: `+66`, `+107`, `+117`), two fields of `moPart_c` (`metadata.rs:238`), and the colour and name of `moVisualProperties_c` (`appearance.rs:27`). The writer emits a 125-byte record with every other byte zero (`writer.rs:1234`), a 13-byte `moPart_c` record (`writer.rs:1212`), and the constant `0x00c0_c0c0` inside each material record (`writer.rs:1801`). That constant is not in `sldprt.md`. The two record lengths are not fixed by any field or specification sentence.

The metadata attributes carry `Exactness::ByteExact` (`metadata.rs:311`), which the round trip does not hold to.

**Need.** We must know what the undecoded bytes hold before the writer regenerates the record. A decode and re-encode with no edit must not replace them.

### EV-04. Tail-directory entry trailer

**Question.** Which value does a tail-directory entry's 14-byte descriptor and 6-byte trailer hold?

**Known.** `sldprt.md` §1.3 "The file tail carries an **OPC package section directory**" states that the 6-byte trailer "has one value for all entries in a file, for example `e5 4b 57 5b 00 00`", and that its first four bytes are the directory separator.

`crates/cadmpeg-codec-sldprt/src/container.rs:542` never reads the descriptor or the trailer, so `DirectoryEntry` cannot carry them. `crates/cadmpeg-codec-sldprt/src/writer.rs:2827` emits 14 zero bytes and the specification's example trailer for every regenerated entry. `writer.rs:342` replays a source entry verbatim when the name, `type_id`, and size all match, so a file whose separator differs and one of whose sections changed size gets two different separators in one directory.

**Need.** We must read and retain both fields. A file must keep one separator for all entries.
