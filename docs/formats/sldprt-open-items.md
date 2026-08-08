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

### BC-01. Other class-root layouts

**Question.** What grammar binds a class-root vector after `index_map_offset` when the vector does not satisfy a defined body, shell, face-use, or cluster-key layout?

**Known.** `sldprt.md` §5 "Face records use these families:" through `sldprt.md` §6 "The disc22-disc12-face layout uses" define the disc-keyed ownership layouts and the class-number-independent cluster-key layout. Each complete layout assigns canonical faces to a stored body.

**Need.** We must know the other layouts to assign their faces to bodies.

### BC-02. Deltas faces outside partition intervals

**Question.** Which body owns a deltas-stream face that is outside all partition intervals of a multi-chain site?

**Known.** `sldprt.md` §6 "The disc04-root layout uses" defines interval ownership for the class-number-independent layout. A sole chain owns all canonical faces in its site.

**Need.** We must know the owner to construct the final body membership of a multi-chain site.

### BC-03. Superseded partition faces

**Question.** Which field or relation identifies the partition face that a deltas face supersedes?

**Known.** `sldprt.md` §4.2 "A deltas stream groups its records into change sets." states that change-roster membership does not identify persistence. `sldprt.md` §4.2 "A deltas change set can re-create a body's faces under new attributes." states that a full deltas bridge is part of the final state and can supersede a partition face.

**Need.** We must identify the superseded face to prevent duplicate faces in the final body.

### BC-04. Unclaimed face disposition

**Question.** Which body owns a canonical face when the site has body records and no body record claims that face?

**Known.** `sldprt.md` §6 "The disc04-root layout uses" defines interval ownership. `crates/cadmpeg-codec-sldprt/src/brep/graph.rs:901` deletes each face that no body record claims:

```rust
if !body_records.is_empty() {
    faces.retain(|face| bridge_group.contains_key(&face.bridge_attr));
}
```

The decoder records no loss for a deleted face. A site with no body records keeps all faces and reports `TopologyBodyHierarchyDerived`. One recognized body record changes the disposition from a reported loss to a silent deletion.

**Need.** We must know the owner to construct the final body membership. Until we know it, the decoder must report the unclaimed faces as a loss.

### BC-05. Head-to-component assignment order

**Question.** Which component does a schema-33103 head select when two unassigned components have equal face overlap in its section interval?

**Known.** `sldprt.md` §6 defines the maximum-overlap rule and the one-to-one assignment. `crates/cadmpeg-codec-sldprt/src/brep/entity.rs:2684` selects the component with `max_by_key` and refuses a zero overlap. It has no rule for equal overlap. `Vec::max_by_key` selects the last of the equal elements. The component order comes from `HashSet` iteration at `entity.rs:2640`, so two runs of the same binary can give different assignments.

**Need.** We must know the tie rule to bind each head to one component. The assignment must also be stable between runs.

### BC-06. Multi-region disc14 sites

**Question.** How many bodies does a disc14 partition with more than one `0x1a` region contain, and which region gives each body its identity?

**Known.** `sldprt.md` §6 defines the disc14 layout for one `0x1a` region and one reachable `0x16` shell. `sldprt.md` §6 states that multiple disc17 records represent distinct stored bodies. `crates/cadmpeg-codec-sldprt/src/brep/entity.rs:2548` collects all regions into one body and takes the body attribute and offset from `region_records[0]`. The region order comes from `HashMap` iteration, so the body identity can change between runs.

**Need.** We must know the body count to construct the stored bodies. We must know the identity rule to give each body a stable neutral identifier.

### BC-07. Two bridges with one owner

**Question.** What do two face-use bridges denote when both name the same owner entity?

**Known.** `sldprt.md` §4 defines `00 0e.ref0` as the owner/use discriminator. `sldprt.md` §4.2 "A deltas change set can re-create a body's faces under new attributes." states that a full deltas bridge denotes a face of the final state. `crates/cadmpeg-codec-sldprt/src/brep/graph.rs:557` sorts the faces by bridge attribute and keeps the first face for each owner:

```rust
faces.sort_by_key(|f| f.bridge_attr);
let mut face_owners = HashSet::new();
faces.retain(|face| {
    t.bridges.get(&face.bridge_attr).and_then(|bridge| bridge.owner)
        .is_none_or(|owner| face_owners.insert(owner))
});
```

The discarded face keeps no loss record. Its loops, coedges, and edges do not enter the graph.

**Need.** We must know what the second bridge denotes to keep or to discard it correctly. The lower bridge attribute is not a defined selector.

## 2. Geometry carriers

### GC-01. Non-isoparametric B-spline trim UV

**Question.** How do we derive the UV curve for a non-isoparametric trim on a B-spline face?

**Known.** `sldprt.md` §7.1 "00 TT  [ff]?" through `sldprt.md` §7.2 "00 2d  marker [ff?]  value_count u32 BE  attr u16 BE  f64[value_count] BE   ; poles /" define exact pcurves for the supported analytic, isoparametric, polar-NURBS, and ruled-surface cases. The Parasolid stream does not store a two-dimensional UV control array.

**Need.** We must know the convention to construct the trim in the surface parameter space.

### GC-02. Missing intersection witnesses

**Question.** Where are the chart, terminator, and support-UV witnesses for an intersection composite when its referenced witnesses are absent or inconsistent?

**Known.** `sldprt.md` §7.2 "Curve carriers: an edge's `00 10.refs[3]` can point to a `00 86` B-spline/list curve carrier," through `sldprt.md` §7.3 "An edge's `00 10.refs[3]` can point to either intersection carrier for a curve defined by the" define both intersection carriers and the three witness record families. The two support surfaces define the exact intersection only when the carrier selects a usable branch.

**Need.** We must find the witnesses to construct the bounded intersection curve.

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

### GC-08. Stored edge direction anchor

**Question.** Which coedge anchors the stored edge direction in a prefixed deltas edge-use record?

**Known.** `sldprt.md` §4.1 "canonical coedge = same-site coedge with attr == 00 10.refs[0]" defines the anchor. `sldprt.md` §4.1 "The `00 10.refs[0]` coedge anchors the stored edge direction." states the rule. `crates/cadmpeg-codec-sldprt/src/brep/topology.rs:156` fills all six references for the bare form. The prefixed deltas form at `topology.rs:185` fills `refs[3]` only, so `refs[0]` is not available for that form.

**Conflict.** `crates/cadmpeg-codec-sldprt/src/brep/graph.rs:571` does not use `refs[0]` for the bare form either. It sorts the faces by bridge attribute and takes the direction from the coedge that the walk reaches first:

```rust
edge_ends.entry(edge_attr).or_insert((start_vuse, end_vuse, curve_attr));
```

The same statement reads `refs[3]` from the edge-use record for the curve carrier, so the anchor is available and unused. `crates/cadmpeg-ir/src/validate/topology.rs` has no vertex-continuity check for a loop, so a reversed edge passes validation.

**Need.** We must use the anchor for the bare form. We must know the anchor for the prefixed deltas form.

### GC-09. Sphere seam pole selection

**Question.** Which pole does a spherical face's degenerate seam use, and what identifies it?

**Known.** `crates/cadmpeg-codec-sldprt/src/brep/graph.rs:3086` selects the nearer of the two poles for an edge with a resolved endpoint. It selects `+axis` when the edge has no resolved endpoint:

```rust
.map_or(north, |endpoint| {
    if squared_distance(*endpoint, north) <= squared_distance(*endpoint, south) { north } else { south }
})
```

`graph.rs:3159` uses `+axis` with no test. When no loop vertex is within the distance limit of that pole, `graph.rs:3203` creates a new point and vertex there. `graph.rs:3246` gives the paired pcurve the fixed origin `Point2::new(0.0, FRAC_PI_2)`. The entry gate counts loops, coedges, and circular edges. It does not test which pole the face reaches.

**Need.** We must know the pole to construct the seam. A face that reaches the `-axis` pole must not receive a vertex at `+axis`.

### GC-10. Isoparametric trim line parameter

**Question.** Which field fixes the `v` parameter of an isoparametric trim line on a ruled B-spline surface?

**Known.** `sldprt.md` §7.1 defines the ruled-surface carrier. `crates/cadmpeg-codec-sldprt/src/brep/graph.rs:2681` searches each knot span with a golden-section minimization and accepts the parameter with the smallest residual:

```rust
let (v, error) = best?;
if error.sqrt() > 0.01 { return None; }
```

The limit `0.01` millimetres is not a stated bound. The decoder does not compare the best parameter with the second-best parameter. The two boundary cases above it use the stored boundary control rows and are exact.

**Need.** We must know the field to construct the trim without a search. Two rulings inside the limit must not select by residual order.

### GC-11. Cylindrical pcurve parameter range

**Question.** Which field fixes the start and end parameters of a NURBS edge on a cylindrical face?

**Known.** `crates/cadmpeg-codec-sldprt/src/brep/graph.rs:1949` searches the knot spans for the parameter nearest each vertex point and accepts it below `0.01`:

```rust
let (parameter, squared_distance) = best?;
(squared_distance.sqrt() <= 0.01).then_some(parameter)
```

The decoder does not test that one parameter only is inside the limit. `derive_cylindrical_pcurves` uses the two results as the pcurve `parameter_range`.

**Need.** We must know the field to bound the trim. A curve that approaches its own start must not collapse to a sliver.

### GC-12. Compact carrier marker position

**Question.** What fixes the position of the `0x2b` or `0x2d` marker in a tripled deltas analytic carrier?

**Known.** `sldprt.md` §7.1 defines the compact analytic carriers. `crates/cadmpeg-codec-sldprt/src/brep.rs:251` uses a fixed position when the form gives one. For the deltas form it accepts the first position in a 56-byte window:

```rust
(hdr + 8..(hdr + 64).min(body.len())).find(|at| {
    matches!(body.get(*at), Some(0x2b | 0x2d)) && body.get(at.saturating_sub(1)) == Some(&1)
})?
```

`brep.rs:263` then refuses the record when a value is not finite or its magnitude is more than `1e6`. `sldprt.md` states no coordinate magnitude limit. The frame invariants at `brep.rs:102` are exact and come from `sldprt.md` §7.1.

**Need.** We must know the position to find the values without a window. We must know the magnitude limit, or remove it, so that a large part keeps its carriers.

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

**Question.** What fixed slot count does each inline entity family outside the canonical face families use in each Parasolid schema?

**Known.** `sldprt.md` §5 "Top-level entity families:" defines the common entity header. `sldprt.md` §10 "Inline `00 51` subrecords use a fixed slot count" defines inline record boundaries when the schema-specific slot count is known.

**Need.** We must know each slot count to find record boundaries without treating payload bytes as delimiters.

**Note.** `crates/cadmpeg-codec-sldprt/src/brep/entity.rs:70` gives three families an explicit slot count and gives every other family the count `6`:

```rust
_ => 6,
```

The table has no schema dimension. The count is load-bearing: `refs()` uses it to select between the prefixed-triple form and the bare u16 form, and to bound the read. A wrong count changes every reference the layout recognizers chain through. The decoder has no branch that refuses an unlisted family.

`crates/cadmpeg-codec-sldprt/src/brep/attrib.rs:78` and `attrib.rs:126` accept a `00 4f` name length and a `00 52` list count in the range `1..64`. `sldprt.md` states no bound for either. Both modules scan every byte offset for the tags, which is the practice this item names.

### CM-06. Partition and deltas precedence

**Question.** Which record takes precedence when partition and deltas streams contain records with the same site, attribute, and sequence?

**Known.** `sldprt.md` §3.2 "An attribute id is **not** globally unique." defines the shared site namespace. `sldprt.md` §4.2 "A deltas stream groups its records into change sets." through `sldprt.md` §4.2 "A deltas change set can re-create a body's faces under new attributes." define deltas change sets and final-state faces, but do not define equal-key precedence.

**Need.** We must know the precedence to select one final record.

**Note.** The decoder answers this question. `crates/cadmpeg-codec-sldprt/src/decode.rs:1869` sorts the partition stream before the deltas stream. `crates/cadmpeg-codec-sldprt/src/brep/graph.rs:494` then keeps the first non-empty body set, and `brep.rs:206` merges only the carriers that the partition index does not have. The partition record wins.

`crates/cadmpeg-codec-sldprt/src/brep.rs:377` states a source for that choice:

> the first (partition-order) wins, matching the "weak deltas must not overwrite a stronger partition record" rule ([spec §4.2](...))

`sldprt.md` §4.2 does not contain that rule. The quoted sentence is not in the specification. Remove the citation or replace it with the decided rule.

The same comment is also wrong about the code below it. `CarrierIndex::insert` at `brep.rs:166` uses `HashMap::insert`, so inside one stream body the carrier at the higher offset wins, not the first.

`sldprt.md` §4.2 "A deltas change set can re-create a body's faces under new attributes." states the opposite direction for bridges: a full deltas bridge denotes a face of the final state, and the partition faces it supersedes do not persist.

### CM-07. `moTransRefPlaneData_c` gap

**Question.** What does the byte run between the `moTransRefPlaneData_c` class token and the first of its nine f64 values encode, and what fixes its length?

**Known.** `sldprt.md` §8 "**Materials / metadata**" gives the field offsets of each document metadata record from the end of its class token. Every other record in that table starts its fields at token end +0. This one starts them after a gap. The decoder finds the value block by the first offset in `0..64` at which nine finite f64 values satisfy the extent constraints.

Observed gap:

| gap length | bytes | record that follows |
| --- | --- | --- |
| 8 | `ff ff ff ff ff ff ff ff` | plane center xyz |

**Need.** We must know the gap to write the record back. A writer that omits it moves every later record in the SW Objects payload, which moves the byte offset each `sldprt:metadata:` identifier carries, so a rewrite that changes nothing still renames those attributes.

**Note.** The Known statement "Every other record in that table starts its fields at token end +0" is not correct for the decoder. See CM-11: the decoder reads `moLengthUserUnits_c` with a 200-byte forward search, not at token end +0. Either that record also starts after a gap, and this statement is wrong, or the search is unnecessary latitude. Settle CM-11 and this statement together.

### CM-08. Active configuration partition binding

**Question.** Which field binds a Keywords configuration to its `Config-N-Partition` section?

**Known.** `sldprt.md` §2 "A Keywords configuration's decimal `id` attribute is the slot identity for" states that the configuration `id` is the slot identity for `Config-N-ResolvedFeatures` and that it is independent of `Config-N-Partition`.

**Conflict.** `crates/cadmpeg-codec-sldprt/src/container.rs:767` binds them by list position. It takes the position of the active `Configuration` element among its siblings and uses that position to index the sorted, deduplicated partition slot numbers:

```rust
if partitions.len() == configuration_count {
    return partitions.get(position).copied();
}
partitions.contains(&position).then_some(position)
```

The `SourceIndex` attribute at `container.rs:790` is the only other path. `SourceIndex` occurs one time in the repository, at that read. No writer sets it and `sldprt.md` does not name it, so the position rule is the only live path.

The result adds `1_000_000` to the score in `select_active_parasolid`, so it selects the active block. That block gives the active body set at `decode.rs:3518` and is the block the writer patches at `writer.rs:397` and `writer_patch.rs:46`.

**Need.** We must know the field to select the active configuration's geometry. Element order is not a defined selector, and the specification states that the two identities are independent.

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

### CM-11. `moLengthUserUnits_c` name position

**Question.** What fixes the position of the unit-name string in a `moLengthUserUnits_c` record?

**Known.** `sldprt.md` §8 "**Materials / metadata**" gives the record fields at token end +0: `bytes[3]` = `ff fe ff`, then a `u8` code-unit count, then the UTF-16LE name.

`crates/cadmpeg-codec-sldprt/src/metadata.rs:99` does not use that position. It accepts the first `ff fe ff` in the next 200 bytes:

```rust
let limit = search.saturating_add(200).min(payload.len());
let Some(relative) = payload[search..limit]
    .windows(STRING_MARKER.len())
    .position(|bytes| bytes == STRING_MARKER)
```

The tests that follow are non-empty, an even byte count, and non-blank text. No test holds the marker at +0 and no test rejects a marker at another offset. The SW Objects payload holds many other `ff fe ff` strings, so a record with an absent or empty name can take a neighbouring string.

**Need.** We must know the position to read the correct string. See the Note on CM-07: this record and `moTransRefPlaneData_c` are the two records that the decoder does not read at token end +0.

### CM-12. Site identity

**Question.** What identifies a site?

**Known.** `sldprt.md` §3.2 "An attribute id is **not** globally unique." states that a site is one validated outer block, identified by its marker offset, and that streams in different outer blocks are distinct sites.

**Conflict.** `crates/cadmpeg-codec-sldprt/src/decode.rs:2008` identifies a site by the section name with the `partition` or `deltas` suffix removed:

```rust
for suffix in ["partition", "deltas"] {
    if let Some(at) = key.rfind(suffix) { key.truncate(at); break; }
}
```

The block offset is available on `BodyStream` through `BodyOrigin::Block` and is not used. A `Config-0-Partition` stream in one block and a `Config-0-Deltas` stream in another block become one site, so the decoder binds attribute `7` of the first to attribute `7` of the second.

**Need.** We must know which identity is correct. The existence of this function is evidence that a configuration's streams occur in separate blocks; the specification states that such streams are distinct sites.

### CM-13. Primary site selection

**Question.** Which field identifies the primary B-rep site when a file has more than one decodable site?

**Known.** `sldprt.md` §3.2 defines a site. It gives no rule that ranks sites.

`crates/cadmpeg-codec-sldprt/src/decode.rs:1910` selects the site with the most faces, then the most bodies, then the most points:

```rust
let score = (decoded.faces.len(), decoded.bodies.len(), decoded.points.len());
```

The other sites are merged in and are not refused. `crates/cadmpeg-codec-sldprt/src/decode.rs:2561` then writes the selected site's block identifier onto every untyped surface and curve in the merged model, including the untyped records of the other sites. That identifier is the only route back to the defining bytes.

The decoder has a second and different idea of the active stream, `container::select_active_parasolid`, which uses `swConfigurationName`. `decode.rs:2637` uses that one for the `active_parasolid_block` attribute. The two are not reconciled.

**Need.** We must know the field to select the primary site. An untyped carrier must point at the block that holds its bytes.

### CM-14. Configuration body membership

**Question.** Which field binds a configuration without a stored source index to its bodies?

**Known.** `sldprt.md` §2 "A Keywords configuration's decimal `id` attribute is the slot identity for" states that the configuration `id` is independent of `Config-N-Partition`. `crates/cadmpeg-ir/src/features.rs:57` defines `ConfigurationBodies::Unresolved` for a configuration whose body membership is not established.

`crates/cadmpeg-codec-sldprt/src/decode.rs:3550` uses the configuration ordinal as the partition index. `crates/cadmpeg-codec-sldprt/src/decode.rs:3586` then replaces every remaining `Unresolved` value with an empty resolved list:

```rust
if configuration.bodies.is_unresolved() {
    configuration.bodies = cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new());
}
```

An empty resolved list states that the configuration holds no bodies. It is not distinct from a configuration that holds none. The `unresolved_configuration_bodies` counter at `decode.rs:427` therefore cannot be nonzero for a file with geometry, so `ConfigIncoherentBodyRefs` cannot report this state.

**Need.** We must know the field to bind the bodies. Until we know it, the decoder must keep `Unresolved` and report the loss.

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

**Question.** Which native field marks dimensioned circular geometry as construction geometry when it is absent from the selected profile stream?

**Known.** `sldprt.md` §2 "A compact-legacy kind `2` bounded curve with locus `05 00 01 00` and the compact indexed" through `sldprt.md` §2 "An extended-prefix kind-`1` profile circle uses the same equal-index 104-byte or terminal" define ordinary and construction full-circle layouts. `sldprt.md` §2 "An `sgSlot_c` declaration may immediately precede a current-, legacy-, or extended-prefix slot record with" distinguishes aggregate slot descriptors from independent curve geometry.

**Need.** We must know the discriminator to prevent omitted construction circles from becoming profile geometry.

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

### DI-31. Last-body-modifying feature identity

**Question.** What identifier space does the `LAST_BODY_MODIFYING_FEATURE_ID` body attribute use?

**Known.** Its values are not native feature object identifiers.

**Need.** We must know the identifier space to bind a body to its last history feature.

### DI-32. Compact line-reference width

**Question.** What distinguishes the eight-scalar compact line-reference form from the nine-scalar form when both final-triple interpretations are unit vectors?

**Known.** Both forms contain scalar triples that can satisfy the unit-vector invariant.

**Need.** We must know the discriminator to parse the reference without choosing a width from geometric plausibility.

### DI-33. Bounded arc sweep direction

**Question.** Which field gives the sweep direction of a bounded arc?

**Known.** A centre and two distinct endpoints define two arcs. Their sweeps add to 2π.

`crates/cadmpeg-codec-sldprt/src/resolved_features/endpoints.rs:4418` selects the arc with the sweep that is not more than π. It exchanges the two endpoints when the stored order gives the other arc:

```rust
let sweep = (end_angle - start_angle).rem_euclid(std::f64::consts::TAU);
let (start_angle, end_angle) = if sweep <= std::f64::consts::PI + tolerance {
    (start_angle, end_angle)
} else {
    (end_angle, start_angle)
};
```

`crates/cadmpeg-codec-sldprt/src/resolved_features/curves.rs:352` repeats the same selection in `tangent_bounded_curve`.

`crates/cadmpeg-codec-sldprt/src/resolved_features/dimensions.rs:249` repeats it a third time in `transformed_dimensioned_arc`. That site also exchanges the two entries of `endpoint_refs`, so the entity's endpoint identities change with the geometry. The test at `dimensions.rs:263` then asserts `sweep <= PI + quantum`, which the exchange above it has already made true. That test cannot fail.

Every sldprt sketch-arc constructor uses one of those two functions, except `curves.rs:203`. The codec therefore does not emit a sketch arc with a sweep of more than π. `cadmpeg-ir` sets no such limit: `crates/cadmpeg-ir/src/validate/sketches.rs` tests only that the angles are finite and different, and `crates/cadmpeg-codec-freecad/src/design.rs:1797` passes stored start and end angles through with no change.

**Conflict.** `sldprt.md` §2 "A detailed curve record is immediately followed by a curve-detail marker of the same generation:" gives the detail record a unit 2D start tangent at detail +64 and +72, states that the tangent and the endpoints determine one circle, and then states: "The bounded arc is the minor sweep between those endpoints."

A start point and a start tangent give the direction of travel, so they give the sweep. `tangent_bounded_curve` uses the tangent to place the centre at `start + normal * scale` and then discards it for the ≤π test. The specification and the decoder both read the witness and then do not use it.

`sldprt.md` §2 states the same limit in three more places: "The angle order represents the minor arc between the endpoints.", "distinct endpoint indices define the minor arc.", and "every ordered endpoint pair has a positive counterclockwise sweep no greater than π".

**Need.** We must know the field to construct an arc with a sweep of more than π. A 270° arc must not become its 90° complement.

**Note.** `curves.rs:203` `set_arc` is the one constructor that does not apply the limit. It serves the slot end cap, which sweeps π exactly. It orders its endpoints by the sign of their projection on the perpendicular of the centre-to-centre axis, which is a second geometric rule and not a stored witness. That special case exists because the general rule does not hold.

The comparison `sweep <= PI + tolerance` tests an angle in radians against the length tolerance that the sketch resolvers thread through. Give the angular test its own limit.

### DI-34. Endpoint index base and roster

**Question.** Which field selects the index base and the roster for the endpoint fields of a compact indexed curve record?

**Known.** `sldprt.md` §2 "The current, compact legacy, and extended marker prefixes have a solved curve or arc with role u16 `1` at marker +27 that uses a compact indexed record." gives the endpoint fields at marker +56 and +58 for several record widths. The same paragraph gives an ordered rule for the extended-prefix widths:

> An extended-prefix 84-byte, 104-byte, or terminal 102-byte profile-locus record first adds one to each endpoint index and resolves the resulting point object indices. If that pair does not resolve, each raw field is a direct point object identifier ... If that pair also does not resolve, both fields are direct ordinals in the complete feature-local sketch-marker roster in marker order.

`crates/cadmpeg-codec-sldprt/src/resolved_features/endpoints.rs:1196` implements that shape. It reads the same two u16 fields under a second roster and a second index base and takes whichever resolves:

```rust
resolve(complete_entity_roster, one_based)
    .or_else(|| { ... .then(|| resolve(false, false)).flatten() })
    .unwrap_or_default()
```

A stored field has one interpretation. `sldprt.md` §2 contains nine sentences of the form "if that pair does not resolve", each of which gives an order of attempts and not a discriminator.

**Need.** We must know the field that selects the base and the roster. A first interpretation that resolves two markers that are not the endpoints gives a line between two unrelated points, and the later tiers never run.

### DI-35. PMI and Keywords dimension precedence

**Question.** Which value holds when a `PMISemanticDataDB` record and a Keywords dimension give different values for one parameter?

**Known.** `sldprt.md` §8 states the rule: PMI values "supply history dimensions when the Keywords record omits them; an explicit Keywords dimension has precedence."

**Conflict.** `crates/cadmpeg-codec-sldprt/src/pmi.rs:473` overwrites the parameter with the PMI value and has no test for an existing value:

```rust
if let Some(parameter) = existing_parameter.map(|index| &mut parameters[index]) {
    parameter.expression = expression;
    parameter.display = display;
    parameter.value = value;
```

`decode.rs:2074` and `decode.rs:2910` call it after `project_design_history` has set the Keywords value, so the PMI value wins. The sibling function `enrich_history_parameters_with_features` at `pmi.rs:242` honours the rule with `.entry(name).or_insert_with(...)`.

A `PmiDimensionSubtype::Native` record formats its value as a bare decimal in metres at `pmi.rs:462`, where the typed arms format millimetres with a unit suffix. Such a record replaces a Keywords `12mm` with `0.012`.

**Need.** We must apply the stated precedence. The decoder must not replace an explicit Keywords dimension.

### DI-36. Repeated PMI dimension records

**Question.** Which record holds when two `PMISemanticDataDB` records give the same owner and dimension name but different values?

**Known.** `sldprt.md` §8 states the binding is valid "when the feature name is unique and all records for the same owner and dimension name encode the same value."

`crates/cadmpeg-codec-sldprt/src/pmi.rs:411` implements the owner-uniqueness half. The value-agreement half is not implemented. `pmi.rs:607` sorts the records by GUID and `pmi.rs:473` overwrites, so the last GUID in lexicographic order wins. `pmi.rs:236` shows the correct shape in the same file: sort, dedup, and withhold when more than one value remains.

The parameter keeps the identifier of the first record and the `native_ref` of the last, so `patch_payload` at `pmi.rs:272` can no longer match the first record and leaves its bytes stale.

**Need.** We must apply the stated agreement test. Records that disagree must leave the parameter unbound.

### DI-37. Multi-item `dimItems` arrays

**Question.** What does each element after the first denote in a `PMISemanticDataDB` `dimItems` array?

**Known.** `sldprt.md` §8 describes `PMISemanticDataDB` through `cadText`. It gives no cardinality for `dimItems`.

`crates/cadmpeg-codec-sldprt/src/pmi.rs:535` takes the first element and refuses the record when its class is not `DimSemData`:

```rust
let Some(Value::Map(item)) = items.first() else { continue; };
if string_field(item, "class") != Some("DimSemData") { continue; }
```

There is no test of the element count. `field_marker` at `pmi.rs:611` takes the first occurrence of each MessagePack key in the record window, so the write-path offsets also address the first element.

**Need.** We must know what the other elements denote. A dual-unit annotation must not lose its second value while the record reports as fully bound.

## 6. Write-path evidence

### EV-01. Unpinned edit validators

**Question.** Which edit shape does each write-path validator refuse?

**Known.** `crates/cadmpeg-codec-sldprt/src/history.rs` guards `sync_neutral_features` with five validators, called from two places in the same function: `validate_compact_body_selection_edits`, `validate_compact_edge_selection_edits`, `validate_compact_surface_selection_edits`, `validate_surface_sweep_profile_edits`, and `validate_embedded_helix_edits`. A kill test made all five return `Ok` for every input and ran the complete sldprt suite. One test failed, and it covers the compact body-selection validator. The other four have no test that reaches their refusal.

Each of the five opens with a guard that returns `Ok` when the document carries no native graph, so a neutral-only document passes all five without a check.

**Need.** We need one negative test for each of the four unpinned validators. The test must build the edit shape that the validator refuses and must assert the error through the encode path, so that removing the validator fails the suite.
