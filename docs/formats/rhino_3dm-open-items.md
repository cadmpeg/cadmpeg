# Rhino 3DM Open Items

This document lists the parts of the Rhino 3DM format that we do not know. The specification `rhino_3dm.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Third-party plug-ins

### PP-01. Plug-in class payloads

**Question.** What payload grammar and semantics does each class UUID that a third-party plug-in registers select?

**Known.** `rhino_3dm.md` §7 "An object record is:" through `rhino_3dm.md` §7 "The object type is a category bitfield, not a class identity. The UUID chunk" define the class wrapper, class UUID, bounded class-data payload, and checksum boundary. `rhino_3dm.md` §20.5 "The 32-byte" through `rhino_3dm.md` §20.5 "The 32-byte" define the opaque-record identity for a class that the built-in registry does not define.

**Need.** We must know the grammar and semantics to decode the class payload as typed geometry, topology, presentation, or document data.

### PP-02. Plug-in userdata payloads

**Question.** What payload grammar and semantics does each third-party userdata class UUID and item UUID pair select?

**Known.** `rhino_3dm.md` §7.2 "A class userdata chunk begins with a packed version byte." through `rhino_3dm.md` §7.2 "The header has the checksum selected by its typecode." define the userdata header, application identity, version fields, anonymous payload boundary, and legacy archive-version rule.

**Need.** We must know the grammar and semantics to decode the bounded userdata payload as typed data.

### PP-03. Plug-in dictionary entries

**Question.** What value grammar and semantics does each plug-in-defined dictionary entry select?

**Known.** `rhino_3dm.md` §6.3 "| dictionary |" through `rhino_3dm.md` §6.3 "| dictionary end |" define the dictionary chunk typecodes. `rhino_3dm.md` §7.2 "A class userdata chunk begins with a packed version byte." through `rhino_3dm.md` §7.2 "The header has the checksum selected by its typecode." define the containing userdata boundary and identity.

**Need.** We must know the value grammar and semantics to decode a plug-in dictionary without treating its entries as one opaque record.

### PP-04. Plug-in application records

**Question.** What payload grammar and semantics does each plug-in-defined application record select?

**Known.** `rhino_3dm.md` §7 "The normal V2+ table sequence is:" through `rhino_3dm.md` §7 "Optional tables may be absent. A table is a bounded table chunk containing" define bounded table records and user tables. `rhino_3dm.md` §20.5 "The 32-byte" through `rhino_3dm.md` §20.5 "The 32-byte" define the identity and byte boundary of a remaining opaque record.

**Need.** We must know the grammar and semantics to transfer the application record as typed document data.

### PP-05. Plug-in object-attribute items

**Question.** What width, payload grammar, and semantics does each plug-in-defined object-attribute item ID select?

**Known.** `rhino_3dm.md` §9.2 "The payload" through `rhino_3dm.md` §9.2 "minor 0: items 1..21" define the payload and version gate for each built-in object-attribute item ID through 41. The item stream has no general length field for an unknown item.

**Need.** We must know the width and grammar to find the next item boundary and to transfer the item as typed object state.

### PP-06. Plug-in layer-extension items

**Question.** What width, payload grammar, and semantics does each plug-in-defined layer-extension item ID select?

**Known.** `rhino_3dm.md` §8.3 "Gated fields" through `rhino_3dm.md` §8.3 "The extension stream is item byte, payload, next item byte, terminated by item" define the stream terminator, payload, and version gate for each built-in layer-extension item ID through 36. The item stream has no general length field for an unknown item.

**Need.** We must know the width and grammar to find the next item boundary and to transfer the item as typed layer state.

## 2. Later built-in versions

### FV-01. Unregistered built-in classes

**Question.** What payload grammar and semantics does each later built-in class UUID select?

**Known.** `rhino_3dm.md` §7 "An object record is:" through `rhino_3dm.md` §7 "The object type is a category bitfield, not a class identity. The UUID chunk" define a class wrapper independently of the class-data grammar. `rhino_3dm.md` §20.5 "The 32-byte" through `rhino_3dm.md` §20.5 "The 32-byte" require a complete unregistered class record to remain one named opaque record.

**Need.** We must know the grammar and semantics to add the class to the built-in registry and to transfer its typed data.

### FV-02. Later object-attribute items

**Question.** What width, payload grammar, version gate, and semantics does each later built-in object-attribute item ID select?

**Known.** `rhino_3dm.md` §9.2 "The payload" through `rhino_3dm.md` §9.2 "minor 0: items 1..21" define item IDs 1 through 41 and their introduction gates. The tagged stream has no general length field for a later item.

**Need.** We must know the width and grammar to find the next item boundary and to extend the built-in object-attribute model.

### FV-03. Later layer-extension items

**Question.** What width, payload grammar, version gate, and semantics does each later built-in layer-extension item ID select?

**Known.** `rhino_3dm.md` §8.3 "Gated fields" through `rhino_3dm.md` §8.3 "The extension stream is item byte, payload, next item byte, terminated by item" define item IDs 28 through 36 and their introduction gates. The extension stream has no general length field for a later item.

**Need.** We must know the width and grammar to find the next item boundary and to extend the built-in layer model.

### FV-04. Later major payload versions

**Question.** What complete payload grammar and semantics does each built-in major version that `rhino_3dm.md` does not define select?

**Known.** `rhino_3dm.md` §5 "A packed payload version is one byte:" through `rhino_3dm.md` §5 "These forms" define packed and anonymous payload-version fields. Each containing long or anonymous chunk supplies the complete payload boundary.

**Need.** We must know the grammar and semantics to decode the new major version as typed data.

### FV-05. Later minor-version suffixes

**Question.** What field grammar and semantics does each later built-in minor-version suffix select?

**Known.** `rhino_3dm.md` §5 "A packed payload version is one byte:" through `rhino_3dm.md` §5 "These forms" define packed and anonymous minor-version fields. A bounded payload fixes the end of the suffix but does not give its field boundaries.

**Need.** We must know the field grammar and semantics to decode the suffix and to distinguish it from malformed trailing bytes.

## 3. Reopened closures audited on 2026-08-10

The following items were removed by `b8c98b9c5` and were reopened by the QA pass. The commit changed the implementation and documentation, but did not establish the settled rule recorded in each item.

### LG-01. V1 geometry payloads

**Question.** What grammar and semantics does each V1 geometry payload use?

**Known.** The current specification and `crates/cadmpeg-codec-rhino/src/legacy.rs` define several V1 point, curve, face, surface, boundary, and mesh paths. They do not establish the complete V1 geometry family or every field and variant.

**Note.** Reopened. This is promotion to spec: partial decoder coverage was written as a complete V1 rule. Passing self-authored fixtures or matching the current decoder does not prove the missing V1 payload grammar.

**Need.** We must know the payload grammar and semantics to decode V1 geometry as typed neutral geometry.

### LG-02. V2 geometry payloads

**Question.** What grammar and semantics does each V2 geometry payload use?

**Known.** The current specification states that V2 class payloads use the same point, curve, surface, mesh, Brep, and annotation grammar as later archives. The class wrapper and CRC framing are defined, but the payload claim is not verified against openNURBS source or corpus files for every V2 class and version.

**Note.** Reopened. This is promotion to spec. The broad V2 statement is an assertion derived from the current decoder shape, not evidence for every V2 payload. Agreement with the branch's fixtures is consistency with the guess.

**Need.** We must know the payload grammar and semantics to decode V2 geometry as typed neutral geometry.

### ON-04. Strictness rules that openNURBS does not apply

**Question.** Which of the codec's framing refusals must stay fatal?

**Known.** `chunks.rs` now demotes negative long values, accepts an EOF body at least the file-size width, and keeps the stored EOF size informational. `container.rs` warns when a table has no end marker. The specification still says every table has a short end marker, so the decoder and specification do not state the same rule.

**Note.** Reopened. The four-way decision was not closed as a specification-plus-decoder contract. The missing-table-marker path is recoverable in code but still described as required in `rhino_3dm.md` §7.

**Need.** We must decide, for each rule, whether it stays fatal, becomes a warning with recovery, or is removed. The decision changes the specification and the decoder together.

### TE-01. Object transfer on Rhino-authored files

**Question.** Why does an object class fail on a Rhino-authored file where the committed fixture for the same class passes?

**Known.** The external witness runs the codec and openNURBS over the example corpus and checks archive-level object totals and supported-count floors. It does not identify the byte-level difference for each class that remains undecoded.

**Note.** Reopened. The aggregate witness does not answer the item question. Treating corpus agreement with the current decoder as verification would be the consistency-as-verification failure this item was intended to prevent.

**Need.** We must find, for each affected class, which byte-level difference separates a Rhino-authored record from the fixture.

### TE-02. Witness strategy and the support claim

**Question.** Which files give an uncorrelated witness that the codec reads and writes 3DM?

**Known.** The branch adds an external openNURBS transfer test over the example corpus and pins aggregate floors by archive version. It does not add the requested synthesized second fixture tier or remeasure the full support claim with a per-version transfer requirement.

**Note.** Reopened. A test over the same corpus is useful regression evidence, but it does not supply the independent synthesized fixtures and support-boundary measurement required by this item.

**Need.** We need a second fixture tier that mirrors the example-file structure, plus a per-archive-version transfer measurement that defines the support claim.

### NS-01. Brep mesh-side wrapper version byte

**Question.** What is the first byte of a Brep mesh-side wrapper body?

**Known.** `crates/cadmpeg-codec-rhino/src/brep.rs:733-780` now reads the first byte as face-zero presence, with no version field. `rhino_3dm.md` §19.4 "For Brep minor at least 1, each mesh-side wrapper" still documents a packed version byte `0x00` before the presence entries.

**Note.** Reopened. The decoder change is consistent with the openNURBS rule, but the settled specification still records the removed byte. The closure is therefore incomplete and would mislead the next implementation pass.

**Need.** The wrapper body starts at the presence byte of face 0. The decoder and `rhino_3dm.md` must state that rule, and cache degradation must remain a typed loss.

### RS-01. Trailing bytes in a bounded chunk

**Question.** What does a bounded chunk with unread trailing bytes mean?

**Known.** `brep.rs` skips trailing bytes in some anonymous helpers, but `brep.rs:384-388`, `history.rs`, `mesh.rs`, and `instances.rs` still reject unread bytes at other bounded payload and record boundaries. The behavior is not a consistent bounded-chunk rule.

**Note.** Reopened. The implementation only partially applies the openNURBS recovery behavior. A later suffix can still discard a complete geometry or instance record at the remaining fatal checks.

**Need.** We must decide whether a bounded chunk may carry unread bytes and apply that decision consistently at every bounded reader.

### RS-02. Exact minor-version equality

**Question.** Which version fields must a decode site compare exactly?

**Known.** The specification says major-1 array and element readers accept every nonnegative minor. Exact checks remain in `history.rs`, `mesh.rs`, `instances.rs`, `morph.rs`, `polyedge.rs`, and Brep nested readers. Later minor fields therefore still take incompatible paths at different sites.

**Note.** Reopened. The broad promotion in `rhino_3dm.md` §4.2 "A reader consumes the fields defined for the payload major version" is not true of the current decoder. The version policy and suffix policy need a site-by-site rule with evidence.

**Need.** A decode site must accept a minor version that is not less than the one whose fields it reads and then apply the trailing-byte rule of RS-01.

### RS-04. Non-canonical boolean bytes

**Question.** Which byte values does a stored `bool` field allow?

**Known.** `chunks.rs:304-307` normalizes every nonzero byte to true for every archive version. It has no pre- or post-2017 archive gate for the later strict `ReadBool` behavior, and the specification states the same unconditional rule.

**Note.** Reopened. The branch removed the old rejection but did not implement the source's archive-version distinction or distinguish raw character fields from strict Boolean fields.

**Need.** The archive-version condition must gate the refusal, and a field read as a raw character must not use the strict Boolean rule.

### RS-05. Enumeration values outside the known range

**Question.** What must a reader do with an enumeration value that it does not know?

**Known.** `objects.rs:1101-1123` warns and returns no color for an unknown selector. `brep.rs:366-375` normalizes an unknown `is_solid` value to unset, while `decode.rs:4251-4259` then derives a body kind. The specification calls value 3 `not-solid`, which the decoder does not preserve.

**Note.** Reopened. The branch still mixes fallback, unset, and silent normalization. It does not apply one source-backed clamp-or-retain rule or emit a typed loss for each unknown enumeration.

**Need.** An unknown enumeration value must clamp and record a typed loss, or stay as stored data. It must not discard the containing record.

### RS-06. Redundant count and index agreement

**Question.** Which stored counts and indices must agree before a record decodes?

**Known.** Some positional checks were removed and some SubD count mismatches are cleared, but the implementation has no typed loss for those repairs and still has exact count, index, and unit checks in several geometry and instance paths. A repaired or discarded field is not distinguished in the loss census.

**Note.** Reopened. Partial tolerance is not the requested rule. Each redundant field needs a source-backed repair or degradation policy and a typed loss where the IR no longer carries the stored value.

**Need.** Each redundant field must repair or degrade and record a typed loss. Discarding a record loses data that the file holds.

### IC-04. Quad triangulation diagonal

**Question.** Which diagonal splits a quadrilateral mesh face?

**Known.** `mesh.rs` now compares diagonal lengths and removes repeated vertices. `decode.rs:3026-3034` still marks an unscaled tessellation byte-exact, and `commit_mesh` records an n-gon loss but no loss for converting stored quadrilateral topology to triangles.

**Note.** Reopened. The geometric split rule is implemented, but the IR conversion remains falsely byte-exact and does not expose the topology loss required by the item.

**Need.** The split must match openNURBS, and the loss of quadrilateral topology must be recorded.

## 4. Hostile sweep findings recorded on 2026-08-10

### SW-01. Duplicate layer index resolution

**Question.** Which layer record owns an archive layer index when the index occurs more than once?

**Known.** `crates/cadmpeg-codec-rhino/src/settings.rs:1273-1317` retains every layer record and emits only a duplicate-index warning. `crates/cadmpeg-codec-rhino/src/objects.rs:1377-1390` builds a map with `layers.entry(layer.index).or_insert(layer)`, so the first record supplies object identity, color, visibility, and name.

**Note.** Duplicate indexes may be malformed, but the scanner already accepts and reports them; the resolver has no source-backed owner rule.

**Need.** Establish the duplicate-index behavior from the openNURBS reader source or from a corpus case, then define a deterministic owner rule with an ambiguity loss when the source does not identify one. If two layer records share an index and the later record carries the authoritative name or appearance, objects that reference the index resolve to the first record. Reordering the two records changes object identity without changing the reference. No ambiguity loss is emitted.

### SW-02. Duplicate singleton metadata selection

**Question.** Which metadata record owns a singleton property or setting when the file contains more than one?

**Known.** `crates/cadmpeg-codec-rhino/src/settings.rs:1273-1303` assigns `writer_version` on each matching property in table iteration order. `settings.rs:1357-1432` assigns `units`, current layer, current material, current color, font, and dimstyle each time a matching setting is read. There is no duplicate check or ambiguity loss for these fields.

**Note.** The normal table model treats these fields as singletons, but the decoder has no source-backed response to a duplicate and no diagnostic that identifies which value won.

**Need.** Establish the openNURBS reader behavior for duplicate singleton records, or settle a reject/first/last policy with a typed ambiguity diagnostic. If two unit records disagree, the later record silently changes coordinate scaling. If two writer-version records disagree, the later record changes version gates. Reordering the records changes the decoded document without a stated ownership rule.

### SW-03. Instance transform ownership inferred from topology

**Question.** Which decoded entities from one instance-definition member receive the instance transform?

**Known.** `crates/cadmpeg-codec-rhino/src/decode.rs:1940-1973` decodes one definition member and then calls `transform_new_entities`. At `decode.rs:1982-2107`, points referenced by new vertices, curves referenced by new edges, and surfaces referenced by new faces are classified as body-owned; other points, curves, and surfaces are transformed directly. Meshes and SubD entities are always transformed, and procedural curves and surfaces are omitted.

**Note.** The heuristic prevents double transformation for ordinary Brep topology, but the openNURBS membership rule has not been traced to establish that topology attachment is the ownership rule for every member class.

**Need.** Establish the membership rule from the openNURBS instance-definition source, with an instance fixture with mixed body and free member entities that identifies which entities move and which stay local. If one member emits a body plus an auxiliary curve or surface whose source ownership is not represented by topology, the topology heuristic decides whether it moves. A shared or cache-like entity can therefore be transformed as free geometry, left in body-local coordinates, or omitted based on the emitted IR shape rather than the source member identity. No ownership ambiguity loss is emitted.

### SW-04. V1 vertex deduplication by first nearby point

**Question.** Does V1 topology identify shared vertices by source references or by geometric proximity?

**Known.** `crates/cadmpeg-codec-rhino/src/legacy.rs:578-612` builds IR vertices from endpoint coordinates and reuses the first existing vertex for which `same_point` succeeds. The comparison uses the maximum of the source tolerances and a fixed floor; it does not use a source vertex identifier.

**Note.** The code is an explicit first-match plausibility choice. The broad V1 grammar item LG-01 did not record this topology-selection rule.

**Need.** We need a source-backed V1 vertex identity rule and a fixture with distinct nearby vertices to test whether tolerance permits merging or only validates coordinates. If two distinct V1 vertex records lie within the selected tolerance but are topologically separate, the first endpoint inserted absorbs the second. Changing trim or face order changes the chosen IR vertex and can collapse a narrow edge or face.

### SW-05. V1 seam-group curve selection

**Question.** Which model-space curve owns a V1 seam or mate group when more than one trim stores one?

**Known.** `crates/cadmpeg-codec-rhino/src/legacy.rs:527-539` unions seam and shell mates, then stores only the first explicit curve for each union root with `or_insert_with`. It uses that curve's endpoints for the shared edge.

**Note.** Seam and mate records may be required to carry equivalent curves, but the current code does not verify that rule and the V1 ledger did not record the first-wins selection.

**Need.** Establish the V1 seam/mate ownership rule from the openNURBS source, with an authored pair of records with different curve copies to establish whether one is authoritative or disagreement is malformed. If two trims in one union group contain different model-space curve copies, source order selects the edge geometry and endpoints. A stale or transformed second copy is silently discarded, with no consistency check or loss.

### SW-06. First-match selection of built-in userdata extensions

**Question.** Which built-in userdata extension owns a dimension or hatch when duplicate class UUIDs occur?

**Known.** `crates/cadmpeg-codec-rhino/src/dimensions.rs:945-985` selects the first matching angular or dimension extension with `.find`. `crates/cadmpeg-codec-rhino/src/hatch.rs:255-264` selects the first matching V5 hatch extension. Later matching records are ignored without a warning or loss.

**Note.** The extensions may be singleton by source convention, but no uniqueness rule is documented and the implementation does not detect duplicates.

**Need.** We need the source uniqueness or precedence rule for these built-in userdata classes, plus an ambiguity loss when a file supplies more than one conflicting extension. If a dimension or hatch contains two extension records with different offsets, arrow data, or base-point data, changing userdata order changes the decoded presentation while the discarded record leaves no trace.
