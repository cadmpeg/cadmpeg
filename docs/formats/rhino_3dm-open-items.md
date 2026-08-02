# Rhino 3DM Open Items

This document lists the parts of the Rhino 3DM format that we do not know. The specification `rhino_3dm.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

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

**Known.** `rhino_3dm.md` §6.3 "| dictionary                |" through `rhino_3dm.md` §6.3 "| dictionary                |" define the dictionary chunk typecodes. `rhino_3dm.md` §7.2 "A class userdata chunk begins with a packed version byte." through `rhino_3dm.md` §7.2 "The header has the checksum selected by its typecode." define the containing userdata boundary and identity.

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

## 3. Legacy geometry

### LG-01. V1 geometry payloads

**Question.** What grammar and semantics does each V1 geometry payload use?

**Known.** `rhino_3dm.md` §1 "Rhino 3DM is" through `rhino_3dm.md` §1 "V1 uses a flat-chunk grammar and may omit the end marker. V2 and later use the" define V1 as a flat chunk stream. `rhino_3dm.md` §4.1 "For V1, CRC16 is selected by the legacy chunk cases: legacy geometry chunks," through `rhino_3dm.md` §4.1 "The stored CRC16 is little-endian. Test vectors are:" define the V1 geometry checksum rule. The specification does not define the geometry fields inside these chunks.

**Need.** We must know the payload grammar and semantics to decode V1 geometry as typed neutral geometry.

### LG-02. V2 geometry payloads

**Question.** What grammar and semantics does each V2 geometry payload use?

**Known.** `rhino_3dm.md` §1 "Rhino 3DM is" through `rhino_3dm.md` §1 "V1 uses a flat-chunk grammar and may omit the end marker. V2 and later use the" define V2 as a table sequence with four-byte chunk values. `rhino_3dm.md` §4.1 "For V2 and later, a long chunk with `TCODE_CRC` set ends with a four-byte" through `rhino_3dm.md` §4.1 "For V2 and later, a long chunk with `TCODE_CRC` set ends with a four-byte" define its CRC32 rule. The specification does not define the geometry fields inside these records.

**Need.** We must know the payload grammar and semantics to decode V2 geometry as typed neutral geometry.
