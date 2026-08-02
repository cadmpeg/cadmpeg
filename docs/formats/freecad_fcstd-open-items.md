# FreeCAD `.FCStd`: Open Items

This document lists the parts of the FreeCAD `.FCStd` format that we do not know. The specification `freecad_fcstd.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Legacy persistence

### LP-01. Schema 2 object grammar

**Question.** What object grammar does `Document.xml` use when `SchemaVersion=2`?

**Known.** `freecad_fcstd.md` §1 "Schema versions 2 and 3" identifies schema 2 as a legacy envelope. `freecad_fcstd.md` §2 "`Document.xml` is the authoritative application object and property graph." states that `Document.xml` is the authoritative application object and property graph.

**Need.** We must know the grammar to decode and validate each schema 2 object boundary and value.

### LP-02. Schema 2 property grammar

**Question.** What property grammar does `Document.xml` use when `SchemaVersion=2`?

**Known.** `freecad_fcstd.md` §1 "Schema versions 2 and 3" states that earlier property encodings belong to separate legacy envelopes. `freecad_fcstd.md` §3 "`ProgramVersion` is metadata." states that property type and value tag select parsing dispatch.

**Need.** We must know the grammar to decode and validate each schema 2 property boundary and value.

### LP-03. Schema 3 object grammar

**Question.** What object grammar does `Document.xml` use when `SchemaVersion=3`?

**Known.** `freecad_fcstd.md` §1 "Schema versions 2 and 3" identifies schema 3 as a legacy envelope. `freecad_fcstd.md` §2 "`Document.xml` is the authoritative application object and property graph." states that `Document.xml` is the authoritative application object and property graph.

**Need.** We must know the grammar to decode and validate each schema 3 object boundary and value.

### LP-04. Schema 3 property grammar

**Question.** What property grammar does `Document.xml` use when `SchemaVersion=3`?

**Known.** `freecad_fcstd.md` §1 "Schema versions 2 and 3" states that earlier property encodings belong to separate legacy envelopes. `freecad_fcstd.md` §3 "`ProgramVersion` is metadata." states that property type and value tag select parsing dispatch.

**Need.** We must know the grammar to decode and validate each schema 3 property boundary and value.

### LP-05. Legacy object-layout dispatch

**Question.** Which version fields and type fields select each pre-schema-4 object layout?

**Known.** `freecad_fcstd.md` §1 "Schema versions 2 and 3" states that a decoder must identify the governing version before it refuses an unsupported layout. `freecad_fcstd.md` §3 "`ProgramVersion` is metadata." lists the structural attributes that select parsing dispatch.

**Need.** We must know the selection rule to choose the correct object grammar before object decoding starts.

### LP-06. Legacy property-encoding dispatch

**Question.** Which version fields, property types, and value tags select each property encoding before schema 4?

**Known.** `freecad_fcstd.md` §3 "`ProgramVersion` is metadata." states that document schema, file version, property type, and value tag select parsing dispatch.

**Need.** We must know the selection rule to choose the correct property grammar before property decoding starts.

## 2. Auxiliary records

### AR-01. Application-specific side-entry framing

**Question.** What byte framing does each application-specific side-entry family use when no typed property grammar identifies the family?

**Known.** `freecad_fcstd.md` §2 "`Document.xml` is the authoritative application object and property graph." states that an entry gets semantic meaning from a typed reference in `Document.xml` or `GuiDocument.xml`. An unreferenced entry remains a named archive record. `freecad_fcstd.md` §11 "Application data without a neutral representation retains its owning object and property" defines exact retention for application data without a neutral representation.

**Need.** We must know the framing to parse and validate record boundaries in these side entries.

### AR-02. Application-specific side-entry values

**Question.** What does each field in an application-specific side-entry family mean when no typed property grammar identifies the family?

**Known.** `freecad_fcstd.md` §11 "Application data without a neutral representation retains its owning object and property" requires retention of the owning object, property, declared application type, links, source order, XML bytes, side-entry bytes, byte spans, lengths, and digests.

**Need.** We must know the field meanings to transfer the side entry to a typed native or neutral record.

## 3. GUI properties

### GP-01. Other GUI property grammars

**Question.** What value grammar does each GUI property runtime type use when `freecad_fcstd.md` does not define that type?

**Known.** `freecad_fcstd.md` §11 "Format-neutral document and view presentation arenas represent GUI state." through `freecad_fcstd.md` §11 "For shape-bearing objects, the view provider's shape color" define document presentation, view-provider state, object appearance, topology color arrays, and their precedence. Each other GUI property retains its owner, runtime type, status, ordered value elements, side-entry references, exact XML, and byte range.

**Need.** We must know each grammar to parse and validate the property as a typed presentation value.

### GP-02. Other GUI property semantics

**Question.** What presentation value does each GUI property runtime type represent when `freecad_fcstd.md` does not define that type?

**Known.** `freecad_fcstd.md` §11 "GUI records retain view-provider identity separately from application-object identity." states that GUI records keep presentation data linked to its owner. Each undefined GUI property retains its runtime type and ordered value elements.

**Need.** We must know the value semantics to transfer the property to the correct neutral presentation field.
