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
