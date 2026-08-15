# FreeCAD `.FCStd`: Open Items

This document lists the parts of the FreeCAD `.FCStd` format that we do not know. The specification `freecad_fcstd.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## Decision queue

These items have a Conflict part and need a decision.

- AR-03. Typed geometry side-entry cardinality
- PT-02. Element-map position to neutral-occurrence order
- PT-03. Element-map carrier and owner selection
- XT-03. Non-manifold radial order
- DP-02. Sketch profile seed order
- DP-03. Sketch profile junction ambiguity and tolerance
- AT-01. Attachment frame carrier precedence

## 1. Auxiliary records

### AR-01. Application-specific side-entry framing

**Question.** What byte framing does each application-specific side-entry family use when no typed property grammar identifies the family?

**Known.** `freecad_fcstd.md` states that an entry gets semantic meaning from a typed reference in `Document.xml` or `GuiDocument.xml`. An unreferenced entry remains a named archive record. Application data without a neutral representation retains its owning object and property.

**Need.** We must know the framing to parse and validate record boundaries in these side entries.

**Note.** Commit `3d3bf58f4` added an opaque-retention policy and promoted it to the specification. This is a safe decoder policy, not evidence that no FreeCAD side-entry grammar exists. No FreeCAD producer source or independent saved witness settles the framing. Keep the unknown open and retain the opaque fallback.

### AR-02. Application-specific side-entry values

**Question.** What does each field in an application-specific side-entry family mean when no typed property grammar identifies the family?

**Known.** The native record retains the owning object, property, declared application type, links, source order, XML bytes, side-entry bytes, byte spans, lengths, and digests.

**Need.** We must know the field meanings to transfer the side entry to a typed native or neutral record.

**Note.** Commit `3d3bf58f4` converted the absence of a typed decoder into a specification claim that no application record family exists. Opaque retention prevents an unsafe interpretation but does not establish field semantics. No producer evidence was supplied.

### AR-03. Typed geometry side-entry cardinality

**Question.** How many side entries can one `PropertyMeshKernel` or `PropertyPointKernel` property reference, and which entry contains the geometry payload?

**Known.** The current specification defines one typed payload per property. Property records retain every side-entry request in source order.

**Conflict.** `crates/cadmpeg-codec-freecad/src/application_geometry.rs:31-38` rejects more than one side entry and otherwise reads the first entry. The rejection test only exercises synthetic malformed XML; it does not establish the producer cardinality or entry selection rule.

**Need.** We must establish the cardinality rule for both runtime types. The decoder must reject an invalid cardinality or identify the payload entry from the typed value grammar.

**Note.** Commit `2ceb8c2b0` turned the one-entry policy into settled format prose without a FreeCAD-saved witness or producer source for the cardinality.

### AR-04. Shared side-entry logical ownership

**Question.** How does the logical byte ledger represent one archive entry that is referenced by more than one property or typed payload?

**Known.** `EntryRecord.referenced_by` now retains multiple semantic references while the byte span has one archive-entry owner.

**Need.** We must know whether typed side entries can be shared. If sharing is valid, the ledger needs a separate many-owner relation that does not duplicate byte spans. If sharing is invalid for a typed family, decoding must reject the conflicting claims.

**Note.** Commit `a5882797a` fixed the internal representation but did not establish whether sharing is valid in FreeCAD output. An implementation choice is not evidence for the format rule.

## 2. GUI properties

### GP-01. Other GUI property grammars

**Question.** What value grammar does each GUI property runtime type use when `freecad_fcstd.md` does not define that type?

**Known.** Undefined GUI properties retain their owner, runtime type, status, ordered value elements, side-entry references, exact XML, and byte range.

**Need.** We must know each grammar to parse and validate the property as a typed presentation value.

**Note.** Commit `6d9430a69` established exact handling for selected material and color-list types but closed this broader item by retaining every other type. That fallback avoids guessing; it does not prove the grammar of the remaining GUI types. No complete runtime registry or producer witness was supplied.

### GP-02. Other GUI property semantics

**Question.** What presentation value does each GUI property runtime type represent when `freecad_fcstd.md` does not define that type?

**Known.** GUI records retain view-provider identity and each undefined property's runtime type and ordered values.

**Need.** We must know the value semantics to transfer the property to the correct neutral presentation field.

**Note.** The opaque/native fallback in `6d9430a69` is not semantic evidence. The item remains open for any unregistered GUI type; no FreeCAD source or independent files establish that the type has no neutral meaning.

## 3. Persistent topology identity

### PT-02. Element-map position to neutral-occurrence order

**Question.** What exact relation connects each final element-map name position to neutral topology occurrences, including repeated placed roots?

**Known.** Persistent names and source topology indices must bind to each placed neutral occurrence. Transient table indices do not constitute persistent identity.

**Conflict.** `crates/cadmpeg-codec-freecad/src/topology_transfer.rs:1535-1581` reconstructs a source index with a custom depth-first walk and `or_insert_with`. It does not read an element-map index or cite a FreeCAD/OCCT enumeration rule. Repeated or equal transformed occurrences can collapse to the first key and later traversal changes the assigned index.

**Need.** We must establish the B-rep indexed-map enumeration rule and carry that index through exact-topology transfer. Repeated placements must bind by placement plus source index, not by an inferred traversal.

**Note.** Commit `cfdcda41e` replaced the earlier modulo join and passed repeated-root synthetic tests. The tests verify the new internal walk, not the producer's indexed-map order. The specification now promotes that walk to settled behavior without independent evidence.

### PT-03. Element-map carrier and owner selection

**Question.** Which `Part`, `ElementMap2`, and property carrier belong to one persistent element map when a shape XML contains more than one candidate?

**Known.** Element maps are associated with a shape property and retain their source XML and map order.

**Conflict.** The decoder rejects more than one `Part` or `ElementMap2` carrier in one exact-shape property and rejects more than one enclosing property for a string table. The producer-defined cardinality and association rule for duplicate carriers is still not established, and no discriminator links multiple legal carriers.

**Need.** We must establish the exact element-map carrier cardinality and property association. Duplicate candidates must be rejected or linked by a producer-defined discriminator.

**Note.** No producer rule for duplicate carriers or shared map ownership was found. Conservative
rejection prevents a source-order choice but does not resolve the legal cardinality.

## 4. Exact-topology transfer

### XT-03. Non-manifold radial order

**Question.** What source order defines the radial cycle when more than two coedges use the same edge?

**Known.** Native topology retains ordered child uses and orientations. A neutral coedge has one `radial_next` relation.

**Conflict.** `crates/cadmpeg-codec-freecad/src/topology_transfer.rs:1661-1671` links one or two coedges, but leaves three or more self-radial. It therefore selects “no radial order” without showing that the source has no radial relation.

**Need.** We must establish whether the B-rep topology supplies a radial order for non-manifold uses. If it does not, the neutral model must retain unordered incidence or mark the radial order unresolved.

**Note.** Commit `63d07acec` changed the neutral fallback and stated that the source has no radial order. No producer source or independent non-manifold witness was cited.

## 5. Design projection

### DP-02. Sketch profile seed order

**Question.** Which non-construction entity starts each oriented sketch profile chain?

**Known.** Sketch entities retain persisted source order and native identity. Profile chains must be deterministic and attributable.

**Conflict.** `crates/cadmpeg-codec-freecad/src/design.rs:2260-2304` selects the smallest in-memory entity index. The current code no longer uses lexicographic decimal ids, but no producer evidence establishes that the first persisted entity is the profile seed when multiple disconnected chains exist.

**Need.** Profile construction must keep the persisted entity ordinal as data and use a producer-defined seed rule for each chain.

**Note.** Commit `cc7953ac4` fixed the decimal-string ordering defect and added synthetic profile tests. The tests do not establish the source rule for disconnected profiles, so the item remains open.

### DP-03. Sketch profile junction ambiguity and tolerance

**Question.** What endpoint tolerance connects two sketch entities, and what happens when more than one unused entity meets the current endpoint?

**Known.** Constraints and persisted geometry can produce coincident endpoints. A neutral profile chain asserts one ordered continuation and orientation at every junction.

**Conflict.** `crates/cadmpeg-codec-freecad/src/design.rs:2269-2349` uses coordinate proximity in addition to coincident constraints and takes the first remaining candidate during chain growth. `near` at `:2427-2434` uses `64 * f64::EPSILON * max_coordinate_scale`; no FreeCAD tolerance or admissible profile topology supports this value.

**Need.** We must establish the endpoint equivalence rule and the admissible profile topology. An ambiguous junction must use constraint identity, an explicit source order rule, or an attributable refusal instead of a first match.

**Note.** Commit `e024f02dd` added ambiguity handling and a scale formula, but the boundary still rests on an uncited constant. Exact and synthetic ambiguous cases do not verify the numeric boundary.

## 6. Attachment and assembly

### AT-01. Attachment frame carrier precedence

**Question.** How do `Placement` and `AttachmentOffset` combine when both are present, and which property/value is authoritative when repeated?

**Known.** Attachment records retain support, map mode, placement, offset, and an effective frame.

**Conflict.** `crates/cadmpeg-codec-freecad/src/attachment.rs:23-39` assigns `effective_frame = placement.or(offset)`, so `AttachmentOffset` is ignored whenever `Placement` exists. The property helper at `:23-27` and value helper at `:45-53` also take first matches. Two valid carriers can therefore produce a different neutral frame after source reordering.

**Need.** We must establish the FreeCAD attachment composition and property cardinality. The decoder must compose or reject conflicting carriers according to that rule.

**Note.** The precedence is explicit; no producer rule for the neutral effective frame was found.
