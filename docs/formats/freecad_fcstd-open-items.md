# FreeCAD .FCStd: Open Items

This document records unresolved FreeCAD .FCStd format questions. The specification records
settled byte semantics and invariants.

Each item has an identifier and these fields:

- Question
- Known
- Need
- Conflict
- Note

## 1. Application-specific side entries

### AR-01. Application-specific side-entry framing

**Question.** What concrete `SaveDocFile` payload framing does each application-specific side-entry
writer use when its runtime property type is not registered by this codec?

**Known.** `Writer::addFile` stores one persistence writer and one unique member name for each
request, and the ZIP writer invokes that object's `SaveDocFile` for the complete member. The
generic archive boundary has no application-wide payload header. The producer source contains
concrete core and module side-entry writers, but the codec does not yet enumerate every remaining
runtime family that can reach this path.

**Need.** Enumerate the remaining unregistered runtime families and document the payload boundary
and framing emitted by each concrete writer.

**Note.** The full producer writer path settles the generic request and member boundary, not the
internal grammar of every family. This item remains narrowed; opaque retention is not a semantic
answer.

### AR-02. Application-specific side-entry values

**Question.** What does each field in an application-specific side-entry payload mean after its
concrete persistence writer has been identified?

**Known.** The exact persistence object writes the complete payload selected by `SaveDocFile`; core
serializers define typed payloads for several file-backed properties, including raw file bytes,
vector and placement lists, float/color/material lists, string tables, and element maps. The native
record retains the owning object, property, declared application type, XML bytes, side-entry bytes,
byte spans, lengths, and digests.

**Need.** Read the remaining concrete serializers and establish their field semantics before
transferring an unregistered side entry to a typed native or neutral record.

**Note.** The producer source now supplies a concrete writer lineage for this subset. Remaining
unregistered family fields still need their own writer evidence; native retention is not meaning
evidence.

## 2. GUI properties

### GP-01. Other GUI property grammars

**Question.** What value grammar remains for each GUI property runtime type not yet covered by the
specification?

**Known.** `ViewProvider` persistence reaches the same `TransactionalObject`,
`ExtensionContainer`, and `PropertyContainer` serializer used by application objects. The base
registry and an authored GUI witness establish these forms: Font/String, StringList/String,
IntegerList/I, Map/Item, Matrix `a11`-`a44`, Position/Direction `PropertyVector`, Quantity/Float,
and Rotation/PropertyRotation. The authored Sketcher witness also establishes a custom
`VisualLayerList` root with ordered `VisualLayer` records.

**Need.** Establish the complete remaining module-owned and dynamic GUI runtime registry, including
custom serializers and side-entry use, and validate those values without dropping the native
record.

**Note.** The authored headless GUI witness establishes the settled subset. The complete
module-owned and dynamic registry is settled by further authored witnesses and by the FreeCAD
property-editor registration source.

### GP-02. Other GUI property semantics

**Question.** What presentation semantics remain for each GUI property runtime type after the
settled core and Sketcher visual-layer subset?

**Known.** GUI properties use the application property's semantic type; GUI persistence does not
introduce a second interpretation. The authored Sketcher witness and its source class establish
that ordered `VisualLayer` records represent per-layer visibility, line pattern, and line width.
GUI records retain view-provider identity and each remaining undefined property's runtime type and
ordered values.

**Need.** Read the defining FreeCAD source and authored witness uses for each remaining
module-owned or dynamic runtime type before transferring it to a neutral presentation field.

**Note.** Core value semantics and the Sketcher visual-layer subset are now source-backed. The
remaining provider-specific presentation mapping is open; native retention is not semantic
evidence.

## 3. Persistent topology identity

## 4. Exact-topology transfer

## 5. Design projection

## 6. Product structure

## 7. Assembly joints

## 8. Attachment and assembly

## 9. Persistent graph admission
