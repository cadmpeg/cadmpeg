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
