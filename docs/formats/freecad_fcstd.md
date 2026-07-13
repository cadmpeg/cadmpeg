# FreeCAD `.FCStd`

## Support envelope

The primary envelope is a ZIP archive containing `Document.xml` with document `SchemaVersion=4`
and `FileVersion=1`. The application graph may contain core App, Part, PartDesign, Sketcher,
Spreadsheet, Assembly, TechDraw, and GUI persistence records. Exact shapes may use text or binary
B-rep side entries. GUI state, thumbnails, persistent element maps, and string-hasher tables are
independently optional.

Schema versions 2 and 3, pre-schema-4 object layouts, and earlier property encodings are separate
legacy envelopes. A decoder must identify their governing version before refusing a layout it does
not support.

Recovery directories, unpacked project trees, backups, and unrelated ZIP archives are not FCStd
documents.

## Container identity

An FCStd document is identified by ZIP framing plus a root `Document.xml` entry whose XML document
element and version attributes identify the persistence document. A ZIP signature alone is not an
FCStd identity marker.

Entry names are unique, relative paths. Absolute paths, parent traversal, encrypted entries, and
names whose normalized form aliases another entry are invalid. Logical entry size, total expanded
size, entry count, nesting depth, and expansion ratio are bounded before allocation or
decompression.

`Document.xml` is the authoritative application object and property graph. `GuiDocument.xml` is a
presentation graph. Other entries acquire meaning only from typed references in either graph;
unreferenced entries remain named archive records.

## Version dispatch

`ProgramVersion` is metadata. Parsing dispatch is selected by container layout, document schema and
file version, object type, property type, value tag, and side-entry form. Unsupported combinations
are reported using those structural attributes.

## Identity and retention

Every document object has a stable identity composed from the document identity and its persisted
object identity. Every property identity includes its owner and persisted property name. Source
order is significant for declarations, properties, links, and side-entry requests.

Unknown object and property types retain their type name, owner, persisted name, status and dynamic
metadata, recoverable links, raw XML span, referenced entry bytes, and source order. Unknown
application records remain named records rather than being merged into one document-wide payload.

Serialized Python and extension payloads are inert bytes. Reading, inspecting, validating,
diffing, and exporting never executes or imports them.

## Measurement semantics

Native scalar text and native quantity values are retained exactly. Neutral model-space lengths
are millimetres. Angles retain whether the native value is radians or degrees. Parameter domains,
placements, orientation, tolerance values, and display-unit settings are distinct fields; display
units do not rescale model geometry.

## Byte accounting

The physical archive and every decompressed logical entry each have an independent byte ledger.
Ledger spans are ordered, non-overlapping, and cover the complete stream.

Physical ZIP spans classify local headers, names, extra fields, compressed payloads, data
descriptors, central-directory records, end records, archive comments, and legal padding.
Compressed bytes and the corresponding logical entry bytes belong to different ledgers.

Logical XML spans classify declarations, delimiters, comments, whitespace, and escaping as
structural bytes. Typed values own their exact lexical spans. A retained record owns one named
opaque span with its declared length and digest. No byte may be both typed and opaque.

## Exact shapes

Part shape properties reference text or binary B-rep entries. Shape records retain native table
indices, locations, geometry carriers, topology, tolerances, flags, parameter ranges, and pcurves.
Transient table indices do not constitute persistent element identity. Persistent element names
exist only when an element-map record supplies them.

The native location chain is applied exactly once at the owning topology level. Display
tessellation is presentation data and does not replace an available exact shape.

## Presentation and application records

GUI records retain view-provider identity separately from application-object identity. Visibility,
display modes, materials, colors, line and point styles, cameras, view state, tree state, clipping,
thumbnail references, and display assets remain presentation records linked to their owners.

Application data without a neutral representation retains its owning object and property,
declared application type, links, source order, XML bytes, referenced side-entry bytes, byte spans,
lengths, and digests.
