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

- XT-03. Non-manifold radial order
- DP-02. Sketch profile seed order
- DP-03. Sketch profile junction ambiguity and tolerance

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

## 3. Exact-topology transfer

### XT-03. Non-manifold radial order

**Question.** What source order defines the radial cycle when more than two coedges use the same edge?

**Known.** Native topology retains ordered child uses and orientations. A neutral coedge has one `radial_next` relation.

**Conflict.** `crates/cadmpeg-codec-freecad/src/topology_transfer.rs:1661-1671` links one or two coedges, but leaves three or more self-radial. It therefore selects “no radial order” without showing that the source has no radial relation.

**Need.** We must establish whether the B-rep topology supplies a radial order for non-manifold uses. If it does not, the neutral model must retain unordered incidence or mark the radial order unresolved.

**Note.** Commit `63d07acec` changed the neutral fallback and stated that the source has no radial order. No producer source or independent non-manifold witness was cited.

## 4. Design projection

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
