# STEP Part 21 clear-text format

Part 21 is a clear-text exchange grammar. [`docs/layouts/step.md`](../layouts/step.md)
records the binary literal's fixed nibble rule. The source table is
[`docs/layouts/step.toml`](../layouts/step.toml). STEP `inspect` runs the
semantic decode path; the concluded disposition is
[step-inspect.md](step-inspect.md).

## 1. Envelope

A Part 21 exchange structure uses `FILE_SCHEMA` to identify the EXPRESS
schemas that specify its DATA entity instances. AP203, AP214, and AP242 are
supported CADIR application-protocol targets. CADIR decision: the inspect
report derives a known AP242 edition only from the first identifier; later
identifiers remain metadata and do not override that report. AP203, AP214, and
AP242 documents carry exchanged product shape and product structure. Product
occurrence relationships carry identity and placement.

Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use
separate encodings.

ISO 10303-28:2007 defines an XML representation of EXPRESS schemas and data
using XML Schema. It does not define one fixed AP203, AP214, or AP242 element
vocabulary. A Part 28 XML grammar is the combination of the Part 28 document
mapping, a configuration, and the XML Schema generated from the selected AP
EXPRESS schema and edition. The configuration selects the XML namespace,
target namespace, serialized unit-of-serialization element, and mapping
options such as attribute-content and tagless encoding. The generated schema
selects entity element names, attribute names, types, cardinalities,
OPTIONAL values, SELECT values, aggregate representation, and reference
attributes.

In the terse configuration, the UOS element is commonly
`iso_10303_28_terse`. The AP namespace identifies the EXPRESS schema mapping;
the Part 28 namespace carries configuration elements such as `exp:header`;
and the `schema` attribute carries the EXPRESS schema name. An entity instance
is an XML element with an XML `id`; an entity reference is an IDREF-valued
attribute or content value defined by the generated schema. The XML `id` is
local to this population and is not a Part 21 `#` identifier. The XML graph
therefore maps one EXPRESS entity instance to one graph node and one reference
value to one graph edge. EXPRESS SET, LIST, ARRAY, BAG, and aggregation bounds
remain the governing cardinality and ordering rules; XML serialization order
does not add order to a SET or BAG. The configuration and generated schema
also govern complex entity encoding and unset OPTIONAL attributes.

ISO 10303-28:2003 is withdrawn. The current Part 28 edition is ISO
10303-28:2007, edition 1. AP242 BO-Model XML uses ISO/TS 10303-3001 schemas,
not the Part 28 mapping, and is a separate encoding.

CADIR decision: the STEP codec admits Part 21 clear text and its ZIP container
only. XML with the Part 28 XML declaration and Part 28 document marker is
recognized as an alternate encoding and is refused with
`NotImplemented("STEP Part 28 XML encoding")` before Part 21 parsing. The
codec does not infer an AP schema or edition from an XML prefix, filename,
UOS-local name, or namespace alone; it does not implement a generic
schema-driven XML adapter and does not join XML data to a Part 21 file by
filename or identifier. A caller that needs Part 28 must provide the exact
Part 28 configuration, generated AP XML Schema, EXPRESS schema edition, and a
separate graph-binding policy.

ISO/TS 10303-26:2011 defines a binary representation of EXPRESS-driven data
using HDF5 version 5. The mapping is schema-driven. A conforming population
uses a schema encoding group directly below the HDF5 root, named from the
schema with the `_encoding` suffix and carrying `iso_10303_26_schema`. Each
population group carries `iso_10303-26_data` with the schema identifier and
`iso_10303_26_data_set_names` with the population's entity dataset names.
Optional population metadata does not change the graph mapping. One HDF5 file
may contain several populations of the same schema.

For each EXPRESS entity, the schema group defines a named HDF5 compound type.
Its relative name is the schema-group name followed by the EXPRESS entity
name; a complex entity joins its leaf entity names with `+` in alphabetical
order. The first compound member is `set_unset_bitmap`; the second is
`Entity-Instance-Identifier`. The remaining members represent explicit
attributes, including inherited attributes, with the HDF5 types selected by
the EXPRESS mapping. Derived and inverse attributes, EXPRESS WHERE and UNIQUE
rules, and EXPRESS executable declarations are not stored as entity members.
Primitive EXPRESS values map to the Part 26 HDF5 integer, floating-point,
boolean/logical, string, binary, enumeration, and aggregate encodings.

Entity instances are rows in datasets under the entity's HDF5 object group,
using its compound type. An entity reference is a pair of HDF5 dataset and
instance indexes; an aggregate is embedded or stored in an aggregate dataset
according to the Part 26 storage rules and contains the corresponding element
or reference values. The `Entity-Instance-Identifier` identifies an instance
within its entity
population; it is not a Part 21 `#` identifier. One HDF5 row maps to one
EXPRESS entity node, compound members map to explicit attributes, bitmap bits
map to OPTIONAL presence, and dataset/instance pairs map to graph edges.
EXPRESS SET and BAG order has no semantic meaning; LIST and ARRAY order is
preserved by their aggregate representation.

ISO/TS 10303-26:2011 is edition 1 and remains the current confirmed edition.
An HDF5 signature identifies an HDF5 file, not a valid Part 26 population or
an AP203, AP214, or AP242 schema. The schema identifier, mapping version,
population markers, compound types, and dataset conventions are all required
to decode the graph.

CADIR decision: the STEP codec refuses an input beginning with the HDF5
signature `89 48 44 46 0d 0a 1a 0a` with
`NotImplemented("STEP Part 26 binary/HDF5 encoding")` before Part 21 parsing.
It does not treat arbitrary HDF5 as STEP, infer an AP schema or mapping from
the signature, implement a schema-bound HDF5 adapter, or compose the HDF5
graph with a Part 21 file. A caller that needs Part 26 must provide the exact
EXPRESS schema, Part 26 mapping edition, and a separate graph-binding policy.

An AP242 BO-Model XML exchange uses the XML Schema for its selected AP242 BO
Model edition. AP242 Edition 1 (2014) uses the ISO/TS 10303-3001 edition-1
schema; its 2016 technical corrigendum uses the edition-2 schema. The
published AP242 Edition 2 and later XML material is the AP242 Domain Model,
not the BO Model, and has a different schema and namespace. An AP242 edition
number and an XML-schema edition number are not interchangeable.

For the AP242 BO Model edition-2 schema, the document element is `Uos` in the
namespace
`http://standards.iso.org/iso/ts/10303/-3001/-ed-2/tech/xml-schema/bo_model`.
The namespace URI, not the prefix, identifies the schema. The common schema
defines `Uos` as an ordered sequence of one `Header` followed by one or more
`DataContainer` elements. `Header` has these optional children in this order:
`Name`, `TimeStamp`, `Author`, `Organization`, `PreprocessorVersion`,
`OriginatingSystem`, `Authorization`, and `Documentation`. The AP242 schema
provides the concrete `AP242DataContainer` type, selected with `xsi:type`.
The corresponding edition-1 schema uses its edition-1 namespace and its own
schema document. A supplied `xsi:schemaLocation` names the namespace and the
schema document; it does not replace namespace identity. The `.stpx` and
`.stpxZ` suffixes are CAx-IF recommended filename conventions, not XML
grammar tokens.

Part 21 does not define a sidecar filename, a same-stem pairing, an XML root,
or an association from `FILE_NAME` to a BO-Model XML document. The CAx-IF
AP242 BO-Model XML recommended practices model a referenced Part 21 or nested
BO-Model file as a `DigitalFile` with an `ExternalItem`; the target filename is
in `ExternalItem.Id`, and an optional `ExternalItem.Source` supplies path or
location information. This is an application-level external-file reference.
It is not a Part 21 sidecar key and does not import the referenced file into
the XML `Uos`.

CADIR decision: the STEP codec classifies a document by its published BO-Model
namespace and refuses it as an alternate encoding. It does not discover a
same-stem XML file or infer an association from `FILE_NAME`, filename suffix,
XML prefix, or `Uos` alone. A caller must explicitly bind an XML resource,
declare its AP242 XML schema and edition, and pass that binding to the separate
composition policy. XML-to-Part-21 identity, value precedence, and conflict
handling remain the BM-02 composition decision.

The AP242 BO-Model XML schema is ISO/TS 10303-3001. The common envelope is
defined by its common XML schema. CAx-IF Recommended Practices for AP242
BO-Model XML Assembly Structure §§4.1.5, 4.1.6, 4.1.7, 4.1.8, 4.1.9, 9.1,
and 9.3 define the recommended header population, namespaces, file suffixes,
and external-file relationship.

The BO-Model XML identity system is local to the XML document. `uid` has XML
Schema type `ID`, and `uidRef` has type `IDREF`; they identify XML elements and
references in that XML document. `Id.id` and `Identifier.id` are business
identifier strings with the role and context fields defined by the BO Model.
They are not Part 21 occurrence names. A Part 21 `#` or `@` occurrence is
local to its exchange structure, so equal numeric values, equal text values,
equal filenames, or equal business identifiers do not establish one object.

The BO-Model recommended practices can carry a basic external-file reference
from a `DigitalFile` or `ExternalGeometricModel` to a Part 21 file. Nested
references can carry a target `Part.id`, `PartVersion.id`, and `PartView.id`
with the component-file reference. These fields identify a target file and
business object for navigation. They do not map an XML `uid` or business
identifier to a Part 21 `#` occurrence, copy attributes, or define value
precedence. The CAx-IF External (Element) References recommended practices
explicitly leave Part 28 XML EER out of scope; their Part 21 EER rules use an
explicit external source and target anchors, and state that EER adds
information rather than replacing existing data.

CADIR decision: there is no generic BO-Model-to-Part-21 composition in the
STEP codec. It does not discover a sidecar, join by filename, stem, numeric
occurrence, `uid`, `Id`, text value, or serialization order, and it does not
let XML values override Part 21 values. A caller that composes the documents
must supply an explicit resource binding, the declared XML schema edition,
and an identity mapping for the domain fields it chooses to connect. CADIR
retains both source graphs and source identities. No source representation has
default precedence. A neutral value is projected only when the caller's
mapping identifies one semantic field and the source values agree; a missing
mapping or conflicting values retains both source values, emits a composition
conflict, and selects no neutral value. The caller may apply a separate
domain-specific policy after this binding and conflict result.

A Part 21 ZIP container uses PKZIP 2.04g compression. It admits stored and
Deflate entries. PKZIP 2.04g excludes encryption, Unicode filename support,
and Deflate64. The archive may contain multiple exchange files, directories,
nested ZIP archives, and ancillary data. Its root member is named exactly
`ISO-10303.p21` and is at the archive root. Every other member is a
subsidiary. The root member contains the Part 21 exchange structure. It is the
only member that a URI from outside the archive may address. A root
`REFERENCE` entry or a root `ANCHOR` forwarding a resource may address a
subsidiary. An internal relative address is resolved from the directory of its
referencing member and cannot address a file outside the archive. Archive
member paths use `/`; the reader
rejects an unsafe path, a duplicate name, an encrypted or Unicode-name entry,
an unsupported compression method, or a root member with a size or CRC
mismatch. For each member it retains the central-directory name, compression,
CRC-32, compressed and uncompressed sizes, and local-header, payload, and
central-directory offsets.

## 2. Byte repertoire and exchange framing

A clear-text exchange structure uses this outer grammar:

```text
exchange = "ISO-10303-21;" header anchor? reference? data*
           "END-ISO-10303-21;" signature*
header   = "HEADER;" header_entity* "ENDSEC;"
anchor   = "ANCHOR;" anchor_entry* "ENDSEC;"
reference= "REFERENCE;" reference_entry* "ENDSEC;"
data     = "DATA" data_parameters? ";" entity_instance* "ENDSEC;"
data_parameters = "(" string "," "(" string ")" ")"
signature= "SIGNATURE;" base64 "ENDSEC;"
anchor_entry    = anchor_name "=" anchor_item anchor_tag* ";"
anchor_item     = omitted | integer | real | enumeration | string | binary
                  | rhs_occurrence_name | resource | anchor_item_list
anchor_item_list = "(" anchor_item* ")"
rhs_occurrence_name = entity_instance_name | value_instance_name
                      | constant_entity_name | constant_value_name
anchor_tag      = "{" tag_name ":" anchor_item "}"
reference_entry = (entity_instance_name | value_instance_name) "=" resource ";"
anchor_name     = "<" uri_fragment_identifier ">"
```

The edition of Part 21 fixes the direct character encoding; the
`implementation_level` value identifies that edition and its syntactical
conformance class. ISO 10303-21:2002 §5.2 defines the older basic alphabet as
ISO 8859-1 positions `G(02/00)` through `G(07/14)`, represented by octets
`0x20..=0x7e`. Characters outside that alphabet use the string control
directives below. ISO 10303-21:2016 §§4.3 and 5.2 define the edition-3 basic
alphabet as Unicode code points `U+0020..U+007E` and `U+0080..U+10FFFF`,
represented by UTF-8. Thus a legacy level does not authorize direct high
octets, and a level-4 file does not select an ISO 8859 code page.

Edition 3 uses implementation levels `4;1`, `4;2`, and `4;3`. Class 1
(`4;1`) forbids ANCHOR, REFERENCE, SCHEMA_POPULATION, and SIGNATURE sections.
Class 2 (`4;2`) permits those sections but forbids value instances and
EXPRESS constants. Class 3 (`4;3`) permits all edition-3 occurrence forms.
Historical levels `1`, `2`, `2;1`, and `2;2` require one unparameterized DATA
section and no FILE_POPULATION, SECTION_LANGUAGE, or SECTION_CONTEXT header
entity. Levels `3;1` and `3;2` require at least one DATA section and forbid
ANCHOR, REFERENCE, SCHEMA_POPULATION, and SIGNATURE sections, value instances,
EXPRESS constants, and resource values. ISO 10303-21:2016 §8.2.2 permits
`3;1` and `2;1` as compatibility declarations only under the listed legacy
restrictions, including use of `\X2\` and `\X4\` for non-ASCII string
characters. Every UTF-8 sequence uses the shortest form, encodes one Unicode
scalar value, and excludes surrogate code points.

Space, the explicit `\N\` and `\F\` print-control directives, and comments
separate tokens. The `/*` delimiter starts a comment, and `*/` ends it.
Comment delimiters form non-nesting pairs. ASCII control octets are ignored
when processing the exchange structure, including when they occur inside a
token. Detection skips leading ASCII control octets, spaces, print-control
directives, and complete comments, then matches the opening
`ISO-10303-21;` token while ignoring ASCII control octets inside that token.
An incomplete leading comment or a leading byte-order mark does not identify
an exchange structure; a byte-order mark is not Part 21 whitespace and is
invalid. The print-control directives are ignored in effective string and
binary contents and are forbidden in resources, ANCHOR sections, and
REFERENCE sections. String and binary literals retain the other source bytes
needed for escape decoding.

Byte accounting assigns each consumed byte to structural syntax, whitespace,
comments, a typed record, or an opaque record. An unclassified byte raises a
parse error.

## 3. Tokens

```text
entity_instance_name = "#" digit+
value_instance_name  = "@" digit+
constant_entity_name = "#" upper (upper | digit)*
constant_value_name  = "@" upper (upper | digit)*
standard_name = letter (letter | digit | "_" | "-")*
user_name     = "!" standard_name
resource      = "<" resource_character* ">"
integer       = sign? digit+
real          = sign? ((digit+ "." digit* exponent?)
                       | ("." digit+ exponent?)
                       | (digit+ exponent ".")
                       | (digit+ exponent))
exponent      = ("E" | "e" | "D" | "d") sign? digit+
enumeration   = "." (letter | "_") (letter | digit | "_")* "."
string        = "'" string_item* "'"
binary        = '"' indicator hex_digit* '"'
indicator     = "0" | "1" | "2" | "3"
omitted       = "$"
derived       = "*"
sign          = "+" | "-"
tag_name      = (letter | "_") (letter | digit | "_")*
```

Keywords and entity names use ASCII letters, digits, underscore, and hyphen.
User-defined names begin with `!` where the grammar admits them. Canonical
keyword spelling uses uppercase. CADIR decision: the reader matches keywords
and the opening token without ASCII case, as a recovery tolerance. Anchor tag
names preserve source case and use letters, digits, and underscore; a tag name
cannot begin with a digit.

Enumeration names begin with an ASCII letter or underscore and continue with
ASCII letters, digits, or underscore. The reader converts ASCII lowercase to
uppercase. A hyphen is not an enumeration character.

Numeric `#` and `@` occurrences require at least one nonzero digit. Leading
zeroes are accepted and removed from the stored integer. Entity and value
occurrence integers share one namespace: an integer used by one prefix cannot
be used by the other prefix in the same exchange. Named occurrences begin with
an ASCII letter or underscore, use only ASCII letters, digits, and underscore,
and are canonicalized to uppercase. A numeric `#` occurrence is a DATA entity
reference. A numeric `@` occurrence is a value reference declared by a
`REFERENCE` entry. Named occurrences are EXPRESS entity or value constants
from the first schema in `FILE_SCHEMA`. An anchor name is a nonempty URI
fragment identifier with at least one non-digit character. A reference
left-hand side is a numeric entity or value occurrence name.

`1.`, `0.E+000`, exponent-form values without a decimal point, exponent-form
values with a trailing decimal point such as `6E-16.`, and Fortran `D`
exponents are real values. A binary literal starts with one indicator nibble
and continues with hexadecimal payload digits.
The indicator gives the number of unused low-order bits in the final payload
digit. Its value is `0..=3`, and each unused bit is zero. Payload digits pack
most-significant nibble first. The decoded bit length is four times the payload
digit count minus the indicator. The empty bit sequence is written `"0"`.

Comma, equals sign, parentheses, braces, colon, and semicolon are individual
punctuation tokens. A resource token contains a UTF-8 byte sequence between
`<` and `>`. The sequence excludes `>` and print-control directives.

## 4. Strings

Two consecutive apostrophes encode one apostrophe. Two consecutive reverse
solidus bytes encode one reverse solidus. Direct bytes in `0x20..=0x7e`, with
apostrophe and reverse solidus handled by the preceding rules, encode
themselves in every edition. In the 1994 and 2002 editions, this is the full
direct string repertoire. In the 2016 edition, direct bytes above `0x7e` are
the UTF-8 encoding of Unicode code points.

The escape `\S\c` adds 128 to the seven-bit code of `c`. Selectors `\PA\`
through `\PI\` choose the ISO 8859 part used by later `\S\` escapes. A
selector contains its letter directly between the two reverse solidus bytes.

The escape `\X\hh` encodes one byte with two hexadecimal digits. The form
`\X2\hhhh...\X0\` encodes four-hex-digit UTF-16 code units. A valid surrogate
pair combines into one scalar value. An isolated surrogate is invalid. The
form `\X4\hhhhhhhh...\X0\` encodes eight-hex-digit Unicode scalar values.
Hexadecimal digits ignore case. Direct ASCII, `\X2\`, and `\X4\` forms denote
the same scalar values where their repertoires overlap.
The print-control directives `\N\` and `\F\` do not contribute to effective
string contents. A string occupies at most 32,769 source octets, including
its opening and closing apostrophes. ISO 10303-21:2016 §6.4.3 retains the
`\S\`, `\P\`, `\X2\`, and `\X4\` directives for compatibility with older
editions; they do not change the direct encoding selected by the edition.
CADIR decision: when a legacy-level source contains a direct octet above
`0x7e`, the reader decodes that malformed source byte as an ISO-8859-1 scalar
to salvage metadata and entity strings. This recovery does not make the
source syntactically conforming. Edition-3 direct bytes that are not a valid
UTF-8 sequence are rejected by header validation; in a semantic field the
reader omits the affected value and records the applicable
`metadata.string-invalid` or `attribute.string-invalid` loss.

## 5. Values and records

A parameter is an entity reference, value reference, named entity constant,
named value constant, integer, real, enumeration, string, binary literal,
resource, omitted value, derived value, list, or typed parameter. A list is a
parenthesized comma-separated sequence. A typed parameter is a name followed
by one parenthesized parameter. ISO 10303-21:2016 §5.5 Table 3 defines
`TYPED_PARAMETER` as `KEYWORD ( PARAMETER )`; §7.1 permits it wherever a
parameter occurs to represent a select value. Section §6.3 defines a keyword
beginning with `!` as a user-defined keyword for a named entity or defined
type, with its meaning agreed between the exchange partners. The wrapper and
its wrapped parameter therefore have no universal value semantics from the
`!` name alone. Empty lists are valid. Numeric value references
and named constants are values, not local DATA entity identifiers.

CADIR decision: the parser retains a user-defined typed parameter as its exact
`!` name and recursively retained parameter value. Opaque-record retention
recursively collects references inside lists and typed parameters. The reader
does not promote a user-defined wrapper or its wrapped value to a neutral or
typed native value without the agreed schema semantics.

A simple entity instance is:

```text
#id = ENTITY_NAME(parameter, ...);
```

A complex entity instance is:

```text
#id = (LEAF_A(...) LEAF_B(...) ...);
```

Complex-instance partial records appear in ascending entity-name order. Each
partial record supplies the attributes introduced by its leaf in external
mapping. `*` marks an inherited attribute supplied by a sibling leaf. The
merged instance retains every leaf name and parameter sequence. Schema
accessors resolve inherited attributes against that representation.

A complex instance whose partial records are not in ascending entity-name order
does not conform to Part 21. CADIR salvage retains the observed partial order,
resolves attributes by partial name, and reports `NoncanonicalSourceSyntax`
with the containing record's byte offset. Strict decode rejects the record.

Part 21 simple-entity mapping supplies one parameter for each explicit
attribute, in declaration order. Internal complex mapping supplies inherited
explicit attributes before the attributes introduced by the leaf. External
complex mapping supplies every partial's explicit attributes. `$` represents
an absent OPTIONAL explicit attribute; no mapping removes an inherited
attribute from its required position. A record that shifts a leaf parameter
into an omitted inherited `name` position is invalid Part 21 source.

Entity instance names share one namespace across all DATA sections. Forward
and backward references resolve after all DATA sections are read. A reference
to an absent local instance is a structural reference error. An entity or
value occurrence declared by a REFERENCE entry is external and is not required
in the local DATA graph. A value occurrence cannot resolve to a DATA entity
instance. An unknown standard or user-defined entity name produces a named
opaque record that retains its complete token span, byte span, and links to
resolved source records.

ISO 10303-21:2016 §11.2 defines a user-defined entity instance as an entity
that is not part of the EXPRESS schema named by the header. It uses the same
entity-instance syntax as a schema entity, with `USER_DEFINED_KEYWORD` in its
simple record. The meaning of the instance, including the number, data types,
and meanings of its attributes, is an agreement between the exchange partners.
Part 21 recommends defining an EXPRESS schema for such information and
encoding it in a separate DATA section when a shared schema is available.
CADIR decision: a `!` entity name and its attributes select no neutral or
source-native semantics without that agreement. The reader retains the full
record as a named opaque record, preserves links to every resolved referenced
source record, and does not create a neutral or typed native entity from the
name alone.

## 6. Header

The header contains `FILE_DESCRIPTION`, `FILE_NAME`, and `FILE_SCHEMA` in that
order. `FILE_DESCRIPTION` supplies description strings and implementation
level. `implementation_level` identifies the Part 21 edition and the
syntactical conformance class; it is not a free-standing character-set
selector. The edition-1 and edition-2 compatibility levels use the historical
direct ASCII range and escape directives. The edition-3 levels `4;1`, `4;2`,
and `4;3` use direct UTF-8 for high Unicode code points. The reader uses the
legacy ISO-8859-1 interpretation only for the malformed-source salvage
decision in §4.
`FILE_NAME` supplies name, timestamp, authors, organizations,
preprocessor version, originating system, and authorization. `FILE_SCHEMA`
supplies one or more unique schema identifier strings. The first schema is the
governing schema for schema-population conformance, EXPRESS constant entity
names, and EXPRESS constant value names. Each parameterized DATA section can
name any schema in the list.
`FILE_DESCRIPTION` strings and every `FILE_NAME` string attribute have an
effective length of at most 256 characters. A non-empty `FILE_NAME` timestamp
uses the complete extended calendar-date and time-of-day form
`YYYY-MM-DDTHH:MM:SS`, with an optional fractional second and an optional `Z`
or signed `HH:MM` time-zone offset. Each `FILE_SCHEMA` identifier has an
effective length of at most 1024 characters. Its schema name is an EXPRESS
`simple_id`: an ASCII letter followed by ASCII letters, digits, or underscores.
ASCII lowercase schema-name letters are converted to uppercase for schema
identity. Its optional object identifier is an ISO/IEC 8824-1 object identifier
enclosed in braces with at least two space-delimited components. A component is
a non-negative decimal number, an ASN.1 identifier, or an identifier followed
by parentheses containing a non-negative decimal number or identifier. A
numeric component has no leading zero. An ASN.1 identifier starts with a
lowercase ASCII letter, uses ASCII letters, digits, and hyphens, and has no
trailing or consecutive hyphens. Numeric root components are `0`, `1`, or `2`;
when the first numeric root is `0` or `1`, a numeric second component is in
`0..=39`. CADIR decision: leading and trailing whitespace around an identifier
is ignored for validation, matching, and uniqueness.
The schema name in a parameterized DATA section compares with the
identifier's schema-name portion when the identifier has an object identifier.
The writer's supported schema identifiers are:

| Identifier                                                                                                   | Protocol and edition |
| ------------------------------------------------------------------------------------------------------------ | -------------------- |
| `CONFIG_CONTROL_DESIGN`                                                                                      | AP203 edition 1      |
| `AP203_CONFIGURATION_CONTROLLED_3D_DESIGN_OF_MECHANICAL_PARTS_AND_ASSEMBLIES_MIM_LF { 1 0 10303 403 2 1 2 }` | AP203 edition 2      |
| `AUTOMOTIVE_DESIGN` or `AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }`                                         | AP214                |
| `AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }`                                    | AP242 edition 1      |
| `AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 3 1 4 }`                                    | AP242 edition 2      |
| `AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 4 1 4 }`                                    | AP242 edition 3      |

For the CADIR AP242 edition report, the first identifier must have the exact
long-form name and one of the exact numeric object identifiers in the table.
An AP242 identifier with no object identifier, an ASN.1 named object
identifier, or another numeric object identifier reports an unspecified
edition. ASCII case differences compare equal. Later identifiers do not
override the first identifier's report.

After `FILE_SCHEMA`, the header may contain at most one `SCHEMA_POPULATION`,
zero or more `FILE_POPULATION` entities, and `SECTION_LANGUAGE` and
`SECTION_CONTEXT` entities with unique section selectors. A
`SCHEMA_POPULATION` contains one or more triples of address string, optional
timestamp string, and optional non-empty Base64 digest string. A timestamp in
this triple uses the same complete timestamp form as `FILE_NAME`. A
`FILE_POPULATION`
contains a governing schema name, a determination-method string, and either
`$` or a nonempty set of DATA section names. `SECTION_LANGUAGE` contains an
optional DATA section name and a three-letter language code. `SECTION_CONTEXT`
contains an optional DATA section name and a nonempty list of context strings.
Built-in header entities precede user-defined `!` header entities. Named
section selectors identify DATA sections. Implementation level `2;1` forbids
FILE_POPULATION, SECTION_LANGUAGE, and SECTION_CONTEXT; level `3;1` forbids
SCHEMA_POPULATION.

In a `SCHEMA_POPULATION` triple, the address is the URI of the external
exchange structure, the timestamp records when the resource was last visited,
and the digest validates the referenced file's bytes. If present, the
timestamp can establish that the referenced file has not changed relative to
its `FILE_NAME` timestamp. A digest is permitted only when the referenced
exchange structure has a signature section. These fields provide freshness and
integrity assertions; Part 21 does not define URI normalization, cache keys,
content negotiation, validators, or representation equivalence. ISO
10303-21:2016 §8.2.5 and Annex F.4.3 define these meanings.

The Part 21 schema population is separate from the decoded instance graph. It
contains every entity in the local DATA sections. If `SCHEMA_POPULATION` is
present, it also contains the schema population of every listed exchange
structure. A `REFERENCE` section adds the schema populations of its referenced
exchange structures. The inclusion is transitive, and an entity occurs at most
once. A resource that does not resolve to an exchange structure contributes no
entities. This population is the conformance population; it does not import a
referenced file's numeric DATA names into the local exchange.
ISO 10303-21:2016 §§8.2.5, 10.2, 11.2, and Annex J define this population and
the distributed exchange meaning.

## 7. Edition 3 sections

ANCHOR entries bind a resource name to an in-file parameter value and may carry
ordered `{tag:value}` metadata tags. Anchor and tag values retain their source
values after resource references resolve. Anchor names are unique. Resource
values that name anchors resolve recursively before schema decoding and before
omitted inherited `name` attributes are repaired. A cycle is a structural
error. Resource references in tag values use the same recursive resolution
rules as anchor values.

CADIR decision: after ANCHOR values and local REFERENCE occurrences resolve,
the reader salvages a single-partial record only when its entity name is in the
reader's named-carrier set and it has no first parameter or its first resolved
parameter is neither a string nor `$`. It inserts an empty string at parameter
position zero. It does not shift complex records, non-carrier entities, or
records whose first parameter is already a string or `$`. The reader reports
this source repair as
`NoncanonicalSourceSyntax` with the containing record's byte offset; salvage
decode retains the recovered record and strict decode rejects the loss.

REFERENCE entries bind an external entity or value occurrence name to a resource URI. Resource names and URIs are delimited by `<` and `>`; external names use `#id` or `@id`.
Entity and value occurrence integers are unique across both prefixes, and
neither may collide with a local DATA entity instance. A URI without a
fragment resolves to `$`. A fragment-only URI whose fragment is not a UUID
resolves to the same-named local ANCHOR; a missing local anchor resolves to
`$`. A fragment-only UUID requires a resource locator or registry. A URI with
a resource path is resolved against that resource; its fragment must identify
an ANCHOR that supplies an entity for a `#id` occurrence or a value for an
`@id` occurrence. If an ANCHOR forwards another URI, resolution repeats, and a
failed or cyclic resolution produces `$`. A resource path or UUID that cannot
be obtained remains an external dependency until the caller supplies resource
access. External occurrence names do not create local DATA entity identities.
ISO 10303-21:2016 §6.5.2 requires the resource token to contain an IETF URI;
§10.2.1 makes a URI without a fragment resolve to `$`, and §§10.2.2–10.2.7
define UUID, local-anchor, and legacy numeric-fragment handling.

For a resolved external resource, the referenced `ANCHOR_ITEM` supplies one
entity or value at the local occurrence. URI forwarding repeats this lookup.
It does not copy the resource's DATA records, numeric instance names, schema
sections, units, or other records into the local exchange. Numeric instance
names are unique within their own exchange structure; the URI fragment and
anchor name identify the cross-resource target. A target that is not an entity
or value in the current `FILE_SCHEMA`, or that cannot be resolved, has the
format result `$`. ISO 10303-21:2016 §§10.2.5–10.2.7 and Annex J define this
single-target substitution across distributed exchange structures.

CADIR decision: a standalone clear-text input has no implicit transport base
URI. The codec does not derive one from `FILE_NAME.name`, any other header
field, an application `DOCUMENT_REFERENCE` or `EXTERNAL_SOURCE` string, or
the process working directory. A fragment-only non-UUID is resolved against
the current exchange's ANCHOR table. A URI with a nonempty path, query, or
scheme is retained as the exact external dependency until an external resource
resolver supplies it; the codec does not open a local path or normalize that
URI against the host filesystem. Application document-reference source fields
are schema strings and remain source metadata, not Part 21 resource tokens.

For a ZIP container, the root exchange has the archive member
`ISO-10303.p21` as its base directory entry. Part 21:2016 Annex A.4 requires
relative addresses to be interpreted against the directory of the referencing
member and forbids an address outside the archive root. The codec normalizes
`.` components, processes `..` only while a parent member remains, rejects an
absolute path, an empty path component, or traversal above the root, and treats
a URI scheme or network-path reference as external. It checks the resulting
member name against the archive central directory. Root ANCHOR forwarding is
resolved before this member check; subsidiary members remain resources and are
not read into the root graph.

CADIR decision: external resource access is an admission boundary outside the
STEP codec. The codec performs no network, filesystem, archive-download, or
registry request for an `http`, `https`, `file`, `urn`, or UUID-only URI. It
retains the exact URI and external occurrence and reports the dependency. A
caller may resolve a dependency with an explicit resolver, but that resolver
must apply its own scheme allowlist, redirect, size and time limits,
authentication, TLS and certificate rules, authorization scope, and optional
message-digest or signature checks before it supplies resource bytes to a
separate composition step. No resolver result enters the decoded STEP graph
implicitly; a missing or refused access result remains an unresolved external
dependency.
For an edition-3 ZIP, the root exchange has the schema-population and
REFERENCE rules above. A root ANCHOR whose value is a URI can forward a root
reference to an entity or value in a subsidiary member.

CADIR decision: the STEP codec admits only the root exchange graph. It keeps
each external occurrence bound to its exact resource URI and anchor fragment;
it does not assign a subsidiary numeric instance to a root identity and does
not merge subsidiary DATA, schemas, units, or coordinate contexts. A caller
that chooses composition must provide a resolver-qualified resource binding,
verify the supplied target and its schema, and apply its trust, digest or
signature, unit, and coordinate-context policies before connecting that target
to a local occurrence. The codec has no implicit cross-resource composition
step. A missing, refused, or unverified target remains an external dependency.

CADIR decision: the STEP codec has no external-resource cache and does not
canonicalize URI spellings. For a caller cache, the retrieved representation
key is the exact URI before its `#` fragment plus the caller's representation
and request policy; the fragment selects an anchor after retrieval and is part
of the composed target identity, not the fetched-byte key. A validated
message digest may identify reusable bytes across resource bindings. A
timestamp alone does not establish byte identity. Different validated
digests for one URI are a conflict, not a merge. No cached bytes or cache
equivalence result enters the decoded STEP graph implicitly.

ISO 10303-21:2016 §14.1 defines each signature as CMS for external content
and requires the CMS structure to be encoded as Base64. RFC 5652 §5 places
the digest algorithm identifiers in `SignedData` and the per-signer digest and
signature algorithm identifiers in `SignerInfo`; Part 21 supplies no separate
method or parameter field. CADIR decision: the parser validates the CMS
envelope and retains its decoded bytes. It does not infer an algorithm or
verification parameter from the Part 21 section delimiters or Base64 text;
verification uses the identifiers and parameters carried by CMS and a
caller-supplied trust policy.

CADIR decision: the STEP codec admits only the root member's exchange graph
into CADIR. It checks that each root REFERENCE binding that resolves to an
internal member, including a binding forwarded through a root ANCHOR, names an
archive member. It records the binding as an external dependency and
decode-report note. It does not open subsidiary members or merge their DATA
namespaces, schemas, units, or identities into the root graph. Subsidiary bytes
remain archive resources.
Each SIGNATURE section follows the exchange terminator. Its content is a
detached CMS `SignedData` object as defined by RFC 5652, encoded as one RFC
4648 Base64 token. RFC 5652 encodes CMS values with BER; signed attributes,
when present, use DER even when the surrounding CMS value uses BER. The
detached `SignedData.encapContentInfo` carries its content-type identifier and
omits `eContent`. `SignerInfo.signature` is an OCTET STRING; signer identity,
algorithm identifiers and parameters, signed or unsigned attributes, and
optional certificates and revocation information are CMS fields. They are not
additional Part 21 fields. The section has one `SIGNATURE;` token, one
Base64 content token, and one `ENDSEC;` token. Space, print-control directives,
and comments may separate these tokens. ASCII control octets are ignored,
including inside the Base64 token. The section boundary is the first
token-boundary `ENDSEC;` after its `SIGNATURE;` token. `ENDSEC;` text inside a
comment or inside the Base64 token is content, not a section boundary.
Multiple sections are retained in source order and may be adjacent. ISO
10303-21:2016 §14.1 defines the CMS message-digest input as the source
characters in Table 1's alphabet. The signed byte range starts at the `I` in
`ISO-10303-21;` and ends immediately before the `S` in that section's
`SIGNATURE;` token. ASCII control octets are omitted from the range because
§5.2 requires them to be ignored; spaces, comments, and print-control
directives remain because their graphic characters are in the Table 1
alphabet. A later section therefore also authenticates every earlier complete
signature section and the alphabet bytes between it and the later
`SIGNATURE;` token. The reader retains both the complete source span and the
decoded CMS payload.

For one signature, `valid` requires all of the following: the verifier computes
the digest over the exact Table 1 alphabet projection of the preceding source;
when `SignerInfo.signedAttrs` is present, it verifies the DER signed-attribute
input, the `messageDigest` attribute equals that content digest, and the
`content-type` attribute equals `encapContentInfo.eContentType`; it verifies the
CMS `signature` OCTET STRING with the `signatureAlgorithm` identifier and the
selected signer public key; and the caller's trust policy accepts the signer,
certificate path, key usage, validity time, revocation evidence, and required
authorization. `invalid` means that a cryptographic check or an explicitly
required policy check fails. A source or CMS value that fails Part 21 or CMS
structural admission is structurally invalid and is refused by the parser
before a signature result is emitted. `indeterminate` means that the CMS is
structurally admitted but required content, key, certificate, trust, time,
revocation, or authorization evidence is unavailable. RFC 5652 §5.6 leaves
public-key selection, certification-path validation, and other external
context to the recipient; therefore the absence of a caller-supplied verifier
or trust policy is indeterminate, never valid. CADIR decision: the STEP codec
performs structural admission only, retains the source span and decoded CMS,
and emits no cryptographic result; a downstream verifier reports the result
for each signature section.

DATA sections are optional in edition 3. One unnamed DATA section requires one
FILE_SCHEMA identifier. If a DATA section has parameters, they contain a
decoded unique section name and one governing schema name listed in
FILE_SCHEMA. Multiple DATA sections require parameters on every section. All
DATA sections share the entity-instance namespace.

## 8. Entity-layer invariants

All STEP aggregate indices are one-based. Entity references preserve identity,
and CADIR keeps one carrier for each referenced entity. `$` denotes an omitted
optional value. `*` denotes a derived attribute. An empty aggregate uses an
empty list. Select and typed-parameter wrappers remain available to schema
accessors.

Length values convert to millimetres. Plane-angle values convert to radians.
`PLANE_ANGLE_MEASURE_WITH_UNIT` requires a `PLANE_ANGLE_UNIT`, and all
dimensional exponents of `PLANE_ANGLE_UNIT` are zero. An `SI_UNIT` has an
optional `prefix`; the prefix is the ratio to its `name`, and omitted prefix
has ratio one. A plane-angle SI unit names `RADIAN`. A
`CONVERSION_BASED_UNIT` multiplies its conversion-factor value by the
recursively resolved unit component, so a plane-angle conversion chain ends
at the radian SI unit and includes its SI prefix and every conversion factor.
Conversion-based units form an acyclic chain that ends in a dimensional base
unit. ISO 10303-43
defines uncertainty at three scopes. `GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT`
applies to representations that share its context,
`UNCERTAINTY_ASSIGNED_REPRESENTATION` applies to the items collected by one
representation, and `qualified_representation_item` from ISO 10303-45 applies
to one item. The precedence is item uncertainty, then representation
uncertainty, then global-context uncertainty. An
`UNCERTAINTY_MEASURE_WITH_UNIT` applies only to items using the measure type
of its value component, and its numeric value is positive. Representation
uncertainty is a linear tolerance measured in the representation's length
unit. Each representation's `GLOBAL_UNIT_ASSIGNED_CONTEXT` supplies the
length and plane-angle scales for that representation and its reachable
representation-item closure. ISO 10303-43 associates a representation context
with each direct item root and every indirectly referenced `representation_item`
or `founded_item` in its graph. A generic `REPRESENTATION_RELATIONSHIP`
relates representations but does not make one representation part of the
other. `REPRESENTATION_MAP` and `MAPPED_ITEM` make the mapped representation
part of the containing representation: source items retain the mapped
representation's context, while the mapped item and its mapping target graph
use the containing representation's context. A
`PARAMETRIC_REPRESENTATION_CONTEXT` makes length units dimensionless; pcurve
definition values use that parametric context, and surface-chart conversion
uses the support surface's parameterization. ISO 10303-41
defines `GLOBAL_UNIT_ASSIGNED_CONTEXT` as a `representation_context`; its
`units` apply in that context, and each unit in its SET is a different kind.
ISO 10303-42 requires every `geometric_representation_item` to be founded in
a `geometric_representation_context` and assigns length and plane-angle units
globally within that context. ISO 10303-43 requires a
`value_representation_item` to be used in a representation whose context is a
`GLOBAL_UNIT_ASSIGNED_CONTEXT`; a value item used in multiple representations
must receive the same unit. The format defines no document-wide unit
occurrence or precedence between independent contexts.

CADIR decision: a carrier used by several representation contexts receives a
per-carrier scale only when all length or plane-angle scales for that carrier
agree. A conflict does not select a context or source occurrence; the carrier
uses the document fallback scale and produces a geometry loss. A value without
a usable representation unit context receives a fallback only when every
global unit context containing that dimension resolves to one equal scale,
with all dimension-unit occurrences in each such context agreeing. If no
context supplies the dimension, every unit record for that dimension must
resolve to the same scale. This fallback is salvage for an unscoped value,
not STEP unit ownership. It does not identify or select a unit occurrence,
and entity-id or source order never selects the scale. Conflicting contexts,
unresolved units, or a non-unique set leave unscoped values in source numeric
units and produce a document-unit unresolved error.
CADIR decision: `Tolerances.linear` stores one document-wide baseline. It is
projected only from resolvable positive length measures in
`GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT`. A candidate whose `name` is
`distance_accuracy_value` (case-insensitive) is preferred when exactly one
such candidate exists; this name is a CADIR convention, not a STEP precedence
rule. Otherwise exactly one resolvable length candidate is required. Angular
measures do not compete with length measures. Multiple candidates without a
unique named candidate leave the default linear tolerance and produce
`geometry.uncertainty-length-ambiguous`; no resolvable length candidate with
an unresolved candidate produces `geometry.uncertainty-length-unresolved`.
An empty candidate set leaves the default without a selection. The optional
description does not participate in name selection, and source order never
selects a candidate. Representation-scoped and qualified-item uncertainties
remain retained as source-native records because one document baseline cannot
encode their scopes; they do not replace `Tolerances.linear`.
Geometric-consistency checks use the selected document tolerance as their
baseline. Entity and solved-carrier tolerances can widen that baseline.

A conical surface accepts zero reference radius at its placement origin. Its
finite half-angle converts from the representation's plane-angle unit to
radians. A NURBS `closed` or `periodic` field uses the STEP LOGICAL domain.
TRUE and FALSE encode the property state. UNKNOWN is a valid LOGICAL value.
A `POLYLINE` with `n` points is the degree-one NURBS with those points as
control points and a clamped piecewise-linear knot vector. `UNIFORM_CURVE`,
`QUASI_UNIFORM_CURVE`, `BEZIER_CURVE`, and the corresponding surface entities
use default knot vectors with `control_count + degree + 1` entries in each
parameter direction. Uniform values are consecutive integers starting at
`-degree`, with multiplicity one. Quasi-uniform values start at zero, repeat
each endpoint `degree + 1` times, and repeat each interior value once. Bezier
values have endpoint multiplicity `degree + 1`, interior multiplicity `degree`,
and a control count that forms an integral number of degree-sized spans.
Complex `B_SPLINE_CURVE` and `B_SPLINE_SURFACE` records use the same defaults
when one of these subtype leaves is present. Rational weight aggregates have
the same shape as their control-point aggregates.

`CARTESIAN_TRANSFORMATION_OPERATOR_3D` stores a required local origin and
optional axis1, axis2, axis3, and scale attributes. Its transformation matrix
columns are normalized and orthogonal: axis3 defaults to +Z, axis1 is projected
onto the plane normal to axis3, and axis2 determines the sense of the projected
second axis. The 2D operator derives a perpendicular second axis from axis1 and
uses axis2 only to select its sense. Omitted scale is 1.

ISO 10303-42 `build_axes` forms the normalized placement axes from the axis and
reference direction. The axis defaults to +Z. `first_proj_axis` projects the
reference direction onto the plane normal to that axis. If the reference
direction is omitted, the projected direction is +X, except that an axis equal
to +X or -X selects +Y before projection. The second placement axis is the
normalized cross product of the placement axis and the projected reference.
The same rule applies to the default axis1 of
`CARTESIAN_TRANSFORMATION_OPERATOR_3D`. A supplied reference direction that
is parallel or anti-parallel to the placement axis violates `WR4`; it is not a
valid default case.

CADIR decision: the STEP reader classifies a normalized axis with
`abs(axis.x) >= 1 - 1e-12` as the +X/-X default case. This tolerance handles
numeric direction records and applies only to STEP placement and transformation
construction. If a supplied reference is non-parallel, the reader projects
that reference without applying the default-axis tolerance. If a supplied
reference is parallel or anti-parallel, the reader reports
`PlacementReferenceInferred` and uses the projected default reference. This
STEP-local salvage does not change neutral transform or chart semantics.

ISO 10303-42 defines `ELLIPSE` with positive `semi_axis_1` and
`semi_axis_2`. `semi_axis_1` lies in placement `p[1]` and `semi_axis_2` lies
in placement `p[2]`. Its source parameter is
`λ(u) = C + R1 cos(u) p[1] + R2 sin(u) p[2]`, where `R1` and `R2` are the
two stored semiaxes. The format does not require `R1 >= R2`; the parameter is
an angular parameter in the active plane-angle unit.

CADIR decision: the IR stores the longer semiaxis as `major_radius` and its
direction as `major_direction`. If `R1 >= R2`, `major_direction` is `p[1]`
and the canonical parameter is `v = u`. If `R1 < R2`, `major_direction` is
`p[2] = axis × p[1]`, `minor_radius` is `R1`, and the canonical parameter is
`v = u - π/2`. Numeric `TRIMMED_CURVE` selectors apply this phase after
angular-unit conversion. Cartesian selectors invert the canonical carrier and
therefore apply no phase. Curve replicas, nested trims, and spatial offsets
inherit the phase; the source STEP record is unchanged.

ISO 10303-42 defines `TRIMMED_CURVE` as a selected portion of an unchanged
basis curve. Trim selects are parameter values, Cartesian points, or both.
Cartesian selects on lines, circles, and ellipses resolve through the basis
curve's parameterization. Its local parameter domain is the directed trim
interval measured from the first select. For basis parameter `t` and trim
parameters `t1` and `t2`, the local parameter is `s = t - t1` for a TRUE sense
and `s = t1 - t` for a FALSE sense. On a cyclic basis, the forward directed
branch increases the second select by one period when it is below the first;
the reversed directed branch increases the first select by one period when it
is below the second. The domain is `0..abs(t2-t1)` after that branch
adjustment. The stored sense maps local parameters in the increasing or
decreasing parent direction. A
`CURVE_REPLICA` retains the complete parent relation, including a trim, and
inherits the parent's parameter range and parameterization; its transformation
changes model-space location and dimensions only. Deferred curve dependencies
resolve by graph fixpoint, including forward and nested replicas. Composite-
curve segments retain order, same-sense, transition continuity, and carrier
identity. A curve construction that references a `SURFACE_CURVE` uses its
3D `curve_3d` carrier for geometry and parameterization.

The endpoint vertices of an `EDGE_CURVE` trim its curve carrier. A
non-periodic carrier has an increasing parameter interval from the start
vertex witness to the end vertex witness. Its domain endpoints select the
first and last branches when the same model-space point occurs at more than
one parameter. A periodic carrier normalizes the start parameter into its
fundamental domain and stores the positive directed sweep to the end witness.
The sweep is not greater than one period. An edge has no parameter interval
when either endpoint cannot be inverted on the carrier or the witnesses do not
define such an interval.

Bounded-surface boundaries use `BOUNDARY_CURVE` or a degenerate pcurve. A
`BOUNDARY_CURVE` is a closed composite curve on its bounded surface. Its
segments resolve to bounded surface curves, bounded pcurves, or nested
composite curves on that surface. A plain three-dimensional composite curve
has a general curve role.

ISO 10303-42 defines `RECTANGULAR_TRIMMED_SURFACE` by applying the
`TRIMMED_CURVE` interval construction independently to its U and V boundary
parameters. It retains its basis surface, both parameter endpoint pairs, and
both parameter-direction senses as a surface subset. Its local U and V
domains are `0..abs(u2-u1)` and `0..abs(v2-v1)` after the directed cyclic
branch adjustment. A local parameter maps to `u1 + s` or `u1 - s`, and to
`v1 + t` or `v1 - t`, according to the stored senses. On a cyclic basis axis,
the forward branch increases the second endpoint by one period when it is
below the first; the reversed branch increases the first endpoint by one
period when it is below the second. A
`SURFACE_REPLICA` retains the complete parent relation,
including a rectangular or curve-bounded surface; its transformation changes
model-space location and dimensions while preserving the parent parameter
domain. Deferred surface dependencies resolve by graph fixpoint, including
forward and nested replicas. Its native entity is emitted again when those
values are available.

ISO 10303-42 defines each surface as `σ(u,v)` with independent parameters. For
`SURFACE_OF_LINEAR_EXTRUSION`, if the directrix is `λ(u)` and the extrusion axis
is `V`, the surface is `σ(u,v) = λ(u) + v V`; U follows the directrix
parameterization and V is unbounded. The extrusion-vector magnitude defines the
surface parameterization. For `SURFACE_OF_REVOLUTION`, if `C` is the axis
origin, `V` is its direction, and the directrix is `λ(v)`, the surface is
`σ(u,v) = C + (λ(v)-C) cos(u) + ((λ(v)-C)·V)V(1-cos(u)) + V × (λ(v)-C) sin(u)`;
U is the rotation angle in the current plane-angle unit and V follows the
directrix parameterization. A pcurve on either surface uses this U/V
parameterization. Its population supplies curve values in the established
chart; it does not establish a surface-wide scale or direction. A non-linear
directrix keeps its native parameterization.

Endpoint-derived calibration of a bounded or domain-valued procedural pcurve
is a CADIR decision. The decoder keeps the native carrier and may add a
use-scoped axis-aligned affine variant only for extrusion, linear-sweep, or
revolution surfaces. For a source axis with a nonzero endpoint span, a zero
destination span is rejected. A zero destination scale is allowed only when
the source axis is constant over the 33 evenly spaced samples of its complete
declared interval, including both endpoints. Equal source endpoints with
interior variation are rejected, and distinct source and destination endpoint
values use the affine map. Revolution U is preserved without scale or offset.
A variant that fails these checks remains opaque; the native pcurve is not
rewritten.

ISO 10303-42:2021 §4.5.57 defines every surface parameter as an independent
dimensionless value. The neutral chart conversion preserves the
parameterization of each defining equation while converting those values into
the IR carrier coordinates. A linear-extrusion U coordinate uses the directrix
scale. For a `LINE` directrix, §4.5.24 makes the `VECTOR` magnitude part of the
line parameterization, so that scale is the directrix vector magnitude times
its length-unit conversion. `CIRCLE` and `ELLIPSE` directrices use the
plane-angle conversion from §§4.5.26–4.5.27. `PARABOLA` and `HYPERBOLA` use
the dimensionless parameters in their trigonometric and hyperbolic equations;
`POLYLINE` uses its integer segment parameter from §4.5.33; and the B-spline
family uses its stored knot parameters. These directrix scales are one for
those non-linear and piecewise parameterizations. A `CURVE_REPLICA` takes its
parameterization from its parent, a `TRIMMED_CURVE` translates or reverses
the parent parameter without changing its scale, and an `OFFSET_CURVE_3D`
takes its parameterization from its basis curve. A `SURFACE_CURVE` uses its
`curve_3d` parameterization.

A linear-extrusion V coordinate is dimensionless because §4.5.67 makes the
extrusion vector's magnitude part of the vector already stored in document
length units; its scale is one. A revolution U coordinate uses the current
plane-angle conversion from §4.5.68 and its V coordinate uses the directrix
scale. The reader supports the line, circle, ellipse, parabola, hyperbola,
polyline, B-spline, and parameter-inheriting carrier forms above. A composite
directrix has the accumulated, piecewise parameterization of §4.5.44 rather
than one affine scale; CADIR therefore leaves it opaque for typed pcurve
conversion. An unsupported analytic or procedural directrix likewise remains
opaque and never receives an assumed unit scale.

ISO 10303-42:2021 §4.5.47 defines `PCURVE` as the composition `g(f(t))`,
where `f(t)` is the referenced two-dimensional curve in the parameter space of
`basis_surface` and `g(u,v)` is the surface parameterization. The
two-dimensional curve is not in the basis surface's representation context;
its coordinates are the surface parameters `u,v`, not Cartesian coordinates
or a separate plane-angle measure, and the curve is defined only within the
surface parameter range. A pcurve has no separate angular-unit override. The
reader does not choose a degree or radian interpretation from endpoint fit; an
angular-looking coordinate that fails the owning surface chart remains an
unusable pcurve carrier.

Orientation composes at each topology relation through face-bound orientation,
oriented-edge orientation, edge-curve `same_sense`, face `same_sense`, and
oriented-shell orientation. Reversing a relation reverses the occurrence
direction while the shared entity keeps its identity. A shell-based wireframe
creates an occurrence-specific edge whose endpoints follow the composed curve
direction. The edge occurrence carries the wireframe use. A committed body
graph has complete ownership and valid referenced indices. Recoverable
non-manifold incidence remains attached and is reported.

Connected face sets join through common edges or common vertices. Edge-based
and shell-based wireframe models preserve their connected edge and vertex
ownership. Each independent connected edge set or wire shell receives
owner-scoped neutral edge and vertex identities. Faceted B-reps materialize
polygon-loop straight edges and vertices as topology carriers. Oriented faces,
subfaces, seam edges, subedges, connected-edge subsets, and connected-face sets
resolve inherited attributes before topology is committed. A connected-edge or
connected-face subset resolves its own member list. Its parent reference must
resolve to the matching parent set type for the subset record to be typed;
parent lineage remains in the source records.

ISO 10303-42:2021 §6.4.2 states that a `MANIFOLD_SOLID_BREP` graph is
labelled, so every edge and vertex entity in that graph has a unique identity;
its edge boundaries may be shared by at most one other face and its B-rep is
represented by disjoint closed shells. Section 6.4.44 defines
`SHELL_BASED_SURFACE_MODEL` as a geometric representation item with a
nonempty SET of open or closed shells. A complete face may be shared by two
shells, coincident portions of shells shall reference the same source faces,
edges, and vertices, and a shell may exist independently of the model.
`FACE_BASED_SURFACE_MODEL` has a nonempty SET of connected face sets, with the
same common-face, common-edge, or common-vertex reference semantics for
connected sets. `MANIFOLD_SOLID_BREP` is a `solid_model` with one
`outer : CLOSED_SHELL`; the outer shell normal points away from the solid
interior. `BREP_WITH_VOIDS` is a
`MANIFOLD_SOLID_BREP` with a nonempty SET of oriented closed-shell voids. Each
void is disjoint from and enclosed by the outer shell, is not the outer shell,
and has its normal directed into the void (`orientation = FALSE`). `voids` is
a SET, so its source order has no format meaning. `FACETED_BREP` is a
`MANIFOLD_SOLID_BREP` whose faces are planar and whose edges are straight.
These source roots define model meaning and topology constraints. Reusing one
source edge or vertex reference preserves that source identity wherever the
referencing graph permits sharing; the Part 42 model definitions do not assign
that source entity to a CADIR body across distinct root instances.

CADIR decision: a topology root is identified by its most-specific source root
type and its resolved root carriers. For shell-based surface models and solid
roots, each carrier key is the base shell instance plus its resolved forward
orientation; an oriented-shell chain resolves to its base shell and composes
the orientation. For face-based surface models, each carrier key is the
connected-face-set instance with forward orientation. Carrier SET order is not
part of the key. A `BREP_WITH_VOIDS` key includes the outer shell and every
void shell, with each resolved orientation.

Roots with one identical key are aliases and reuse the body committed for the
lowest STEP instance number in that key. Physical record order does not select
the body. A different root type, carrier instance, or resolved shell
orientation creates a distinct root even when the roots share source shell,
edge, vertex, or face records. A distinct root is an ownership boundary. When
one distinct root exists, source edge and vertex identities remain shared
within that root. When multiple distinct roots exist, every root scopes its
shell, edge, and vertex identities by root instance; a root with multiple shell
owners also scopes carriers by shell. Solid roots are `Solid` bodies and
surface roots are `Sheet` bodies. This preserves independent roots without
claiming one global CADIR identity.

Sheet and wire representations commit each independently resolvable shell or
connected set. A failed member produces a decode loss. Solid roots, including
every shell in `BREP_WITH_VOIDS`, commit atomically. A mandatory member failure
rejects the solid root. The outer shell of `BREP_WITH_VOIDS` must decode to one
connected IR shell; a split outer shell rejects the root because the IR stores
the outer role by position. One STEP face shared by several shell occurrences
maps to one owner-scoped CADIR face per occurrence. Boundary edges and
vertices remain shared when their owner scope is unambiguous.

CADIR decision: `Region.shells[0]` is the outer shell. The remaining entries
are void shells sorted by resolved base shell instance, forward orientation,
and source shell instance as a tie-break. The source `voids` SET order does
not select a neutral shell slot. The outer shell is not repeated in the void
suffix.

A face boundary uses an `EDGE_LOOP` coedge ring, a `POLY_LOOP` point ring, or
a `VERTEX_LOOP` vertex at a surface singularity. A vertex loop emits a
vertex-only boundary. ISO 10303-42:2021 §5.5.16 defines `POLY_LOOP` as an
ordered coplanar collection of points with implicit straight segments. Section
§5.5.19 states that a face may have an implicit surface when its faces are
defined by `POLY_LOOP`; that surface is the plane containing the poly-loop
points. The same section defines the topological normal by the cross-product
rule toward the face interior and requires all poly-loop orientations of one
face to produce the same normal. Section §5.5.17 gives
`FACE_BOUND.orientation` the meaning of retaining or reversing the loop sense.

CADIR decision: for a base `FACE` without an explicit `FACE_SURFACE`, the
decoder selects the first `FACE_OUTER_BOUND` in source order, or the first
valid boundary when no outer bound is declared. It applies
`FACE_BOUND.orientation` to the selected loop. For a `POLY_LOOP`, the selected
points define the inferred plane. CADIR also applies this inference to an
`EDGE_LOOP` by using its resolved directed edge endpoints; this is a CADIR
extension beyond the Part 42 implicit-poly-loop rule. The plane origin is the
arithmetic centroid of the selected ring. Its u-axis is the projection of the
first global coordinate axis whose projection has the greatest length; ties
keep x, then y, then z. Every point must be within
`max(0.01, 1e-12 * ring_scale)` of that plane, where `ring_scale` is the
largest displacement from the centroid. A ring whose signed area is at most
`1e-12 * ring_scale^2`, or whose point residual exceeds that bound, does not
produce a plane. An `ORIENTED_FACE` keeps the base plane carrier orientation
and composes its reversal through face sense and boundary traversal.

CADIR decision: a topology member that requires a `VERTEX_POINT` with an
absent point carrier is mandatory and unrepresentable. Sheet and wire
representations omit only the failed independent member and retain complete
members. A solid-root transaction rejects the root. CADIR has no tolerant-point
or partial-solid carrier and does not infer coordinates. A geometric set with
surface members forms a sheet carrier. Curve-only and point-only sets remain
standalone geometry.

ISO 10303-42:2021 §5.5.18 defines `FACE_OUTER_BOUND` as a `FACE_BOUND` carrying
the outer-boundary semantics and states that no more than one boundary of a
face may have this type. Section §5.5.19 defines `FACE.bounds` as
`SET[1:?] OF FACE_BOUND`; its WR2 permits at most one `FACE_OUTER_BOUND`, and
the outer role is optional. Other face bounds are not outer bounds.

Section §5.5.17 defines the inherited `FACE_BOUND` attributes `bound` and
`orientation`. Section §5.5.18 declares `FACE_OUTER_BOUND` as a subtype with
no additional explicit attributes. Therefore, in a complex instance, the
`FACE_BOUND` partial supplies the boundary parameters and the empty
`FACE_OUTER_BOUND` partial supplies only the outer-boundary classification.

CADIR decision: subtype classification checks for the presence of the
`FACE_OUTER_BOUND` partial, while attribute lookup selects the partial that
carries the three `FACE_BOUND` parameters. This remains true when the two
partials are serialized in the opposite order. The latter order is
noncanonical Part 21 source; CADIR retains it and reports the source-order
loss, then applies the same attribute rule. An empty partial that supplies no
boundary parameters cannot create a loop or an implicit surface.

CADIR decision: when malformed input declares more than one outer bound, the
decoder rejects the containing topology shell/root. It assigns no outer role,
derives no implicit face carrier, retains the source face and its bound
records as opaque, and reports the malformed face and rejected topology root.
This refusal is independent of the order of the bounds aggregate. It does not
claim that ISO 10303-42 prescribes this salvage disposition for malformed
input.

`AXIS2_PLACEMENT_2D` defines the origin and positive-u axis of a parameter-space
conic. Its positive-v axis is the counterclockwise perpendicular. ISO
10303-42:2021 §4.5.47 requires a pcurve's definitional representation to have
exactly one item, that item to be a `CURVE`, and its dimensionality to be two.
Sections §4.5.56 and §4.6.2 require a `CURVE_REPLICA` transformation to have
the parent's dimension and require the replica graph to be acyclic.

CADIR decision: the reader admits one exact 2D line, circle, ellipse,
parabola, hyperbola, polyline, NURBS, trimmed curve, offset curve, or curve
replica from the definitional representation. A 2D `CURVE_REPLICA` retains its
parent parameterization and its 2D affine operator maps the parent coordinates
to the replica coordinates. After the one carrier is admitted, the reader
applies the owning surface chart scale once; it does not reinterpret the
coordinates as Cartesian values or apply a second document angle scale. An
active-record cycle or a graph at depth 256 or greater remains opaque, and the
recursion guard releases its active record on every return path. An
unrecognized composite 2D carrier remains opaque rather than becoming an
approximate pcurve. An unsupported 2D representation stays opaque and remains
detached from the coedge. ISO 10303-42:2021 §5.5.7 defines `SEAM_EDGE` as an
`ORIENTED_EDGE` with a `pcurve_reference`; its WR1 requires an `EDGE_CURVE`
whose geometry is a `SEAM_CURVE`, and its WR2 requires the reference to be one
of that seam curve's `associated_geometry` pcurves. The inherited edge
orientation refers
to the edge element, not to the pcurve sense. Sections §4.5.47 and §4.5.49
define the pcurve basis surface and require a seam curve's two pcurves to lie
on the same surface. Section §5.2.2.1 and function §5.6.4 define the pcurve
set associated with an edge curve: match a candidate's basis surface to the
face surface, and when multiple candidates share that surface, check their
connectivity in parameter space. A non-seam edge has no source field that
selects one of those candidates.

CADIR decision: a typed `SEAM_EDGE` uses its explicit pcurve only when the
reference is decoded, is a member of the edge's `SEAM_CURVE` associated
geometry, and has the coedge face surface as its basis. An invalid reference
does not fall back to another pcurve; the coedge remains without a pcurve and
reports a loss. For a non-seam edge with multiple same-surface candidates, the
reader maps each candidate through that surface and accepts a unique
endpoint-continuous fit. If several candidates tie, adaptive subdivision
compares their mapped loci over the endpoint interval. Equivalent loci are
one neutral carrier and the lowest STEP identity is retained; distinct tied
or otherwise unresolved candidates remain detached and produce a topology
loss. A declared pcurve trim is the endpoint witness when it maps to both edge
vertices; a stale declared trim can be replaced by an independently inverted
edge interval. Candidate list order does not select a non-seam carrier.

ISO 10303-42 defines `SURFACE_CURVE.associated_geometry` as a list of one or
two `PCURVE` or surface references. A selected pcurve identifies its basis
surface and its 2D parameter curve. `SURFACE_CURVE` takes its parameterization
from `master_representation`; each associated pcurve has the same sense as
`curve_3d`. The entity graph may reference one `PCURVE` from multiple surface
curves. ISO 10303-21:2016 §11.2 states that entity instances need not be
ordered and that an instance name may be referenced before its definition.
CADIR decision: association lookup uses instance identity and references,
never DATA serialization order. The source pcurve carrier is immutable. A chart
variant derived from one coedge's endpoint fit is a use-scoped pcurve carrier.
The coedge owns that variant through its `PcurveUse`; selecting a variant for
another coedge does not change the source carrier or the first coedge's
parameter range.

A topology-referenced curve or surface whose geometry fails transfer retains
its STEP identity as an unknown carrier linked to its opaque record. The body
topology keeps the relation. An optional pcurve that fails transfer leaves the
coedge usable and produces a loss. An unowned pcurve and its unshared 2D
dependency closure stay named opaque records. A shared dependency remains
typed when another retained carrier owns it. Failure of a mandatory topology
relation rejects the complete solid root. Records owned only by that root stay
opaque, and product bindings omit the body. Sheet and wire members use the
independent-member salvage rule. A `SURFACE_CURVE` with a star or non-reference
basis keeps its edge occurrence without a CADIR curve and produces a loss; the
decoder does not fabricate a curve carrier. A plane pcurve uses the document
length scale for both parameter axes and all length-valued 2D geometry. A
cylinder or cone uses plane-angle scale for `u` and length scale for `v`. A
sphere or torus uses plane-angle scale for both axes. NURBS surface parameter
axes are dimensionless. A pcurve carrier that cannot preserve its native
parameterization under an anisotropic surface-unit map remains opaque.
A surface-curve carrier and its pcurve represent the same point set but may
use different parameterizations; its edge vertices determine the occurrence
interval when no explicit trim is present. The carrier's own NURBS domain is
not an edge trim.

The writer records each omitted shell, face, loop, and edge relation as a
topology-transfer loss. Omitted outer shells, void shells, and outer bounds
are errors. Other omitted topology relations are warnings. The strict
unsupported policy rejects output when any topology-transfer loss exists.

ISO 10303-41 defines a `PRODUCT_DEFINITION` as one aspect or view of a
product for an identified life-cycle stage. Different
`PRODUCT_DEFINITION` instances for one `PRODUCT_DEFINITION_FORMATION` are
different views. `PRODUCT_DEFINITION_FORMATION.of_product` associates a
formation with its `PRODUCT`, and the formation identifier is unique within
the formations of that product. The standard permits a product to have
multiple definition groups. The product-definition function resolves these
associations as a `SET`; `PRESENTATION_LAYER_ASSIGNMENT.assigned_items` is also
a `SET`, so neither relationship supplies a view order.
CADIR decision: each linked `PRODUCT_DEFINITION` is one product-definition
view. A product with one definition uses identity
`step:product:product#<product>`. When one `PRODUCT` has multiple definitions,
each view receives a distinct identity suffixed by its definition instance.
Shape bodies and definition descriptions bind to their own view and are not
merged. Each definition that is not a usage child receives one root occurrence.
Every usage occurrence references the specific child definition view. When a
presentation layer references a `PRODUCT`, CADIR expands all of that
product's definition views in `PRODUCT_DEFINITION` DATA record order; this is
deterministic projection order, not a STEP view-order rule.
Product shape binds through `PRODUCT_DEFINITION_SHAPE` and
`SHAPE_DEFINITION_REPRESENTATION`. Every body-producing representation,
including `ADVANCED_BREP_REPRESENTATION` and
`GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION`, uses the same
source-root-to-body map. An `ADVANCED_BREP_REPRESENTATION` is typed when its
items resolve directly or through a mapped representation to committed
topology roots. Occurrence
transforms compose once from the
product-definition relationship into model space. Mapped representations and
context-dependent relationships that identify one placement apply that
placement once. A mapped occurrence uses a
`PRODUCT_DEFINITION_SHAPE` whose definition is the
`NEXT_ASSEMBLY_USAGE_OCCURRENCE`; its
`SHAPE_DEFINITION_REPRESENTATION` contains the mapped item that identifies the
child representation and its target placement. A mapped item target may be an
`AXIS2_PLACEMENT_3D` or a `CARTESIAN_TRANSFORMATION_OPERATOR_3D`; the mapped
transform is the target transform composed with the inverse mapping-origin
transform. Reused source topology roots of the same root type and shell
orientation reuse their committed body identity. Distinct topology roots
retain their governing root type, even when they share shell carriers.
ISO 10303-44 defines each `NEXT_ASSEMBLY_USAGE_OCCURRENCE` as one individual
constituent occurrence. A second use of the same child is a distinct
`NEXT_ASSEMBLY_USAGE_OCCURRENCE` when position or orientation is assigned.
ISO 10303-43 defines `representation.items` as `SET[1:?]` and
`representation_map.map_usage` as `SET[1:?]`. A representation may contain
several mapped items, and one representation map may be used by several mapped
items. Each `MAPPED_ITEM` independently maps the source representation from
its mapping origin to its mapping target. Neither multiplicity nor SET member
order selects one mapped item. The mapped-item assembly pattern binds one
occurrence through a separate `SHAPE_REPRESENTATION` containing its mapped
item, with `SHAPE_DEFINITION_REPRESENTATION` and
`PRODUCT_DEFINITION_SHAPE` identifying that occurrence. The
context-dependent pattern binds the occurrence through
`CONTEXT_DEPENDENT_SHAPE_REPRESENTATION`; its shape relationship uses the child
representation as `rep_1`, the parent representation as `rep_2`, and an
`ITEM_DEFINED_TRANSFORMATION` whose `transform_item_1` is in the child context
and whose `transform_item_2` is in the parent context.
ISO 10303-41 §23.4.4 defines `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION` as the
association of a shape-representation relationship with a product-definition
relationship. Its generic entity declaration has no uniqueness proposition for
`represented_product_relation`. The AP214 `coordinated_assembly_and_shape`
rule requires a qualifying context-dependent relationship for each
`NEXT_ASSEMBLY_USAGE_OCCURRENCE`, but does not define precedence between
multiple qualifying relationships. ISO 10303-44 §4.4.8 defines each
`NEXT_ASSEMBLY_USAGE_OCCURRENCE` as one individual constituent occurrence;
distinct uses receive distinct occurrence instances.
CADIR decision: a neutral occurrence admits exactly one resolved
`CONTEXT_DEPENDENT_SHAPE_REPRESENTATION` placement. If more than one resolved
relationship targets the same usage, the placement is ambiguous even when the
transforms are equal. The decoder selects no candidate, uses the identity
transform, and reports `product.nauo-placement-ambiguous`. This decision is
independent of Part 21 record order and numeric entity identifiers. A missing
or otherwise unresolved sole relationship remains an unresolved placement.
CADIR decision: a parent representation's mapped items do not bind repeated
uses of one child definition to individual occurrences. When each child
definition occurs once in a parent usage set and the complete mapped-child
sequence agrees with that usage set, CADIR may use physical record order as a
deterministic inference; this is a CADIR salvage rule and does not assign
meaning to the STEP `SET` order. Repeated child definitions disable that
inference even when their mapped targets differ. Without an occurrence-owned
mapped representation or a context-dependent placement, each repeated use
keeps the identity transform and reports `AssemblyPlacementsNotTransferred`.
ISO/TS 10303-1345 defines the mapped-item assembly link through an additional
separate `SHAPE_REPRESENTATION` with only the `MAPPED_ITEM`; a
`SHAPE_DEFINITION_REPRESENTATION` and a `PRODUCT_DEFINITION_SHAPE` link that
representation to the specific occurrence. ISO 10303-43 requires the mapping
origin to be in the mapped representation context and the mapping target to be
in the context of the representation that directly contains the mapped item. A
`REPRESENTATION_RELATIONSHIP` alone does not make one representation part of
the other. CADIR decision: an occurrence-owned placement candidate is valid
only when its `MAPPED_ITEM` is directly listed by a representation linked to
the occurrence's `PRODUCT_DEFINITION_SHAPE`, whose definition is the
`NEXT_ASSEMBLY_USAGE_OCCURRENCE`. A mapped item listed by an unrelated
representation does not place the occurrence and produces an
assembly-placement loss.
For the implicit parent-representation salvage path, CADIR considers only a
mapped item directly listed by a representation linked to the parent
definition and only when the complete mapped-child sequence agrees with the
parent usage sequence. This salvage rule is not a STEP occurrence association.
CADIR decision: for standalone mapped items that resolve to one body, an
identical transform is assigned once to that body's `transform`. Distinct
transforms cannot be represented by one body transform, so CADIR leaves the
body transform unset and reports `AssemblyPlacementsNotTransferred`.
Occurrence-owned mappings retain their occurrence transforms and do not enter
this body-level decision.
Repeated child uses without an
occurrence-specific shape representation remain ambiguous and report the
unresolved placement. A mapping whose origin and target are both 2D placement
or 2D transformation records is presentation geometry and does not change a
body placement. A mapped item directly owned by a drawing graph, or contained
in its geometric or tessellated item sets, is presentation geometry and does
not infer a product body placement.

Product definitions and product-definition formations use the inherited base
attribute prefix. A direct subtype carries that prefix in its own parameter
list. A multiple-inheritance complex instance uses the parameters of its
`PRODUCT_DEFINITION` or `PRODUCT_DEFINITION_FORMATION` partial. Product
records use the parameters of their `PRODUCT` partial. A presentation layer
item that references a `PRODUCT` expands to every CADIR product-definition
view derived from that product, in source-definition order. A
`PRESENTATION_LAYER_ASSIGNMENT` is the carrier for layer membership and layer
visibility. Its `name` is a `label` and may be empty. A valid assignment has
at least one assigned item. `INVISIBILITY` targeting that assignment sets the
layer's `visible=false`; it does not hide the assigned model or presentation
items.
The writer emits one assignment and a layer-targeted `INVISIBILITY` for each
emitted hidden layer on schemas that support visibility. A target schema that
does not support `INVISIBILITY` reports
`presentation.hidden-layer-visibility-unsupported`, and a hidden layer with no
emitted assignment reports `presentation.hidden-layer-omitted`.

A shape representation contains at least one representation item. In a
complex instance, its name, item list, and context use the populated
`REPRESENTATION` partial; an empty inherited subtype partial does not replace
those attributes. The two items of an `ITEM_DEFINED_TRANSFORMATION` belong to
the two representations connected by its representation relationship. An
ISO 10303-43 representation relationship with an
`ITEM_DEFINED_TRANSFORMATION` binds `transform_item_1` to `rep_1` and
`transform_item_2` to `rep_2`. ISO/TS 10303-1345 defines the occurrence
convention as child in `rep_1` and parent in `rep_2`: `transform_item_1` is in
the child frame and `transform_item_2` is in the parent frame.
CADIR decision: for a child-to-parent occurrence relationship, the placement
is `transform_item_2` composed with the inverse of `transform_item_1`. If the
relationship endpoints are parent-to-child, CADIR swaps the two items before
computing that same child-to-parent transform. Neither endpoint order nor
item order is ignored; an endpoint pairing that is absent or ambiguous leaves
the placement unresolved.
occurrence placement belongs to its defining relationship and representation
context. A
`SHAPE_REPRESENTATION_RELATIONSHIP` connects its two shape-representation
endpoints for body reachability and representation identity. A contextual
occurrence endpoint identifies a child or parent definition representation
when it is that representation or is connected to it by one or more
parameterized shape-representation relationships. These identity edges are
undirected. An empty inherited subtype partial has no endpoints and creates no
edge. In a complex instance, endpoint attributes come from the inherited
`REPRESENTATION_RELATIONSHIP` partial when the subtype partial has no
parameters. For an occurrence relationship, the transform maps the child
representation to the parent representation. If the relationship lists those
endpoints in reverse order, the item direction is inverted. If neither order,
or both orders, identifies the child and parent representations, the
occurrence placement is unresolved.

Exact and tessellated representations of one product remain linked when their
source item has one exact body owner. A tessellated solid, shell, or shape
representation may list a supported triangulated item directly or through a
tessellated geometric set. A product-linked shape representation supplies a
declaration. An exact body link or representation relationship supplies the
body owner to every supported leaf in the item graph. An isolated shape
representation is also admitted when its supported item identities are listed
by a product-linked shape representation. A shape representation without a
product link, shared product-linked items, or an exact representation link
remains a detached source association. A missing or ambiguous owner detaches
the tessellation, retains its source item association, and records a
`ReferenceGraphNotClosed` loss.
`TESSELLATED_SHAPE_REPRESENTATION_WITH_ACCURACY_PARAMETERS` uses the inherited
representation name, item set, and context for the same ownership rules. Its
accuracy-specific record remains source-native while the supported tessellated
items transfer.
`SHAPE_REPRESENTATION_WITH_PARAMETERS` uses the inherited representation name,
item set, and context. Its item set contains descriptive representation items,
directions, measure representation items, and placements. The reader applies
its context to reachable item units and uses its item set for inherited
representation membership, including validation properties. Unsupported item
semantics remain source-native.
ISO 10303-42 §6.4.64 defines `COMPLEX_TRIANGULATED_FACE` and §6.4.71 defines
`COMPLEX_TRIANGULATED_SURFACE_SET` as tessellated geometry arranged in triangle
strips and triangle fans. `triangle_strips` and `triangle_fans` are lists of
integer-index lists; each sublist supplies the vertex order through the
`COORDINATES_LIST` or `pnindex` table, and each sublist has at least three
indices. The point order shown in Figure 32 expands a strip row
`v[0]...v[n]` into `[v[i], v[i+1], v[i+2]]` for even `i` and
`[v[i+1], v[i], v[i+2]]` for odd `i`. A fan row expands into
`[v[0], v[i], v[i+1]]` for `i` from 1 through `n - 2`. Tessellated indices are
one-based. PNINDEX maps local points to shared coordinates. Triangle and fan
indices address local points in listed order. A normal aggregate of length one
applies to every local point; other normal aggregates align with the local
point table.

CADIR decision: a strip or fan row with fewer than three indices is invalid and
is skipped. Valid rows in the same complex triangulated entity still transfer;
the decoder does not invent a triangle from an invalid row.
`TESSELLATED_CURVE_SET` uses its `COORDINATES_LIST` and one-based `line_strips`
indices to transfer each strip as a separate polyline carrier. The reader does
not join strips or invent source parameters or a chordal bound.

`TESSELLATED_ANNOTATION_OCCURRENCE` carries a tessellated geometric set;
supported triangulated descendants transfer as detached tessellations. A
ISO 10303-42 §6.4.54 defines `REPOSITIONED_TESSELLATED_ITEM.location` as a
required `AXIS2_PLACEMENT_3D` and defines that placement as the origin and axis
system for the referenced point coordinates. A valid repositioned item applies
that placement, including nested repositioning, to a detached leaf. Detached
annotation leaves do not require an exact body owner and do not produce a
body-association loss. Unsupported annotation wrappers and unsupported
descendants remain native records. If one detached leaf is reached through
multiple distinct placement transforms, no transform is selected, source
coordinates remain, and `tessellation.placement-ambiguous` is recorded.

CADIR decision: a missing, wrong-typed, or otherwise unresolved repositioning
placement leaves the leaf in its inherited or source coordinate frame, retains
the repositioned wrapper as native data, and records
`tessellation.placement-unresolved`. It never substitutes an identity
placement or selects a placement by source order. A Part 21 reference to an
absent local instance is a structural reference error and does not reach this
decoder decision.

Styles resolve from a styled item through presentation assignments to color.
A style on a `GEOMETRIC_SET` or `GEOMETRIC_CURVE_SET` applies to each member,
and its style domain derives from those members; a point-only set uses
point-style semantics. Empty and NULL style assignments leave appearance
unchanged. Independent effective styles on one face or body retain every
appearance binding. `SURFACE_STYLE_TRANSPARENT` in a
`SURFACE_STYLE_RENDERING_WITH_PROPERTIES` sets the neutral alpha to
`1 - transparency`; zero is opaque and one is fully transparent. The neutral
scalar color is
set only when those styles produce one distinct color; conflicting colors
leave it unset and produce a metadata loss. A direct `STYLED_ITEM` or
`OVER_RIDING_STYLED_ITEM` still owns its curve, point, or surface target when
the assignment has no resolvable colour. `INVISIBILITY` targeting a styled item
sets `visible=false` on every appearance binding for that styled-item identity;
targeting a base styled item also hides bindings emitted for its overriding
styled items.

ISO 10303-46 makes `presentation_style_assignment.styles` a SET. Its WR1
forbids repeating a style type except `EXTERNALLY_DEFINED_STYLE` and
`SURFACE_STYLE_USAGE`; WR2 permits at most two `SURFACE_STYLE_USAGE` values;
WR3 requires two such values to apply to opposite surface sides.
`surface_side_style.styles` contains at most one value of each style type, and
ISO 10303-46 §6.4.62 defines
`surface_style_rendering_with_properties.properties` as `SET[1:2] OF
rendering_properties_select`; WR1 requires all property values to have
different types. Section 6.4.65 defines `SURFACE_STYLE_TRANSPARENT` and
restricts `transparency` to `0.0..1.0`. A valid rendering therefore has at most
one transparency property, and no transparency precedence is defined for a
second value. Distinct surface sides are handled by the CADIR side projection
above. The format does not define a color or transparency precedence for other
applicable style characteristics.

CADIR decision: if a malformed rendering record contains multiple finite
`SURFACE_STYLE_TRANSPARENT` values, the decoder retains the resolved color,
omits transparency for that rendering, and records
`presentation.surface-transparency-conflict`. It never selects a value from
the SET serialization order.

ISO 10303-46 §6.3.35 defines `surface_side` as `.POSITIVE.`, `.NEGATIVE.`,
and `.BOTH.`. `.POSITIVE.` is the side in the surface-normal direction,
`.NEGATIVE.` is the opposite side, and `.BOTH.` is both sides. Section 6.4.66
defines `SURFACE_STYLE_USAGE` as applying its `surface_side_style` to the
positive side, negative side, or both sides. Section 6.4.46 defines the
assignment's `styles` as a SET; WR2 permits at most two
`SURFACE_STYLE_USAGE` instances and WR3 requires two instances to specify
opposite sides. These rules define side applicability. They do not provide an
aggregate-order or neutral-IR colour precedence.

CADIR decision: one neutral surface color cannot represent separate positive
and negative appearances. The scalar projection therefore ranks
`SURFACE_STYLE_USAGE` as `.BOTH.` before `.POSITIVE.` before `.NEGATIVE.`.
It applies this projection independently of SET serialization order. The
ranking is a CADIR projection, not a STEP precedence rule.

ISO 10303-46 §6.2 states that it does not define the effect of a style
conflict, including the case where one `representation_item` is used by
several independent `STYLED_ITEM` instances. The `styles` attributes of
`presentation_style_assignment` and `STYLED_ITEM` are SET aggregates. The
`STYLED_ITEM` WR1 permits one style assignment, or more than one only when all
assignments are `PRESENTATION_STYLE_BY_CONTEXT` instances. No aggregate
position therefore selects one independent unscoped style over another.
`PRESENTATION_STYLE_BY_CONTEXT` applies only in its `style_context`.
`OVER_RIDING_STYLED_ITEM` is the explicit precedence relation: its style takes
precedence over its `over_ridden_style` when both are included in one
presentation.

CADIR decision: the scalar surface projection ranks the side policy before
color. Independent effective styled items without an override relation have
equal scalar rank. The projection does not use source instance order or alpha
to choose between different RGB colors. Equal-rank candidates with different
RGB colors leave the scalar color unset, retain every source style through its
appearance binding, and produce `presentation.conflicting-scalar-colors`.
When equal-rank candidates have the same RGB color, the projection retains the
lower alpha so a rendering transparency property is not lost beside an opaque
fill color. Independent effective styled items retain separate appearance
bindings.

Visibility remains binding-level and does not change visibility on a shared
geometry carrier. The writer emits binding-specific `INVISIBILITY` records for
emitted hidden styled items on schemas that support visibility; unsupported
schemas report `presentation.hidden-appearance-visibility-unsupported`. ISO
10303-46 assigns a `PRESENTATION_STYLE_BY_CONTEXT` to a representation item
and applies it only in its `style_context`. `style_context` can be a `group`,
`CONTEXT_DEPENDENT_SHAPE_REPRESENTATION`, `PRESENTATION_LAYER_ASSIGNMENT`,
`PRESENTATION_SET`, `REPRESENTATION`, `REPRESENTATION_ITEM`, or
`REPRESENTATION_RELATIONSHIP`. A `STYLED_ITEM` with more than one style uses
only context-qualified styles. ISO 10303-43 relates an item to a
representation context when the item is directly in a representation for that
context or is reached through any number of intervening representation or
founded items. Distinct contexts have no format-defined precedence.

CADIR decision: the STEP decoder has no requested presentation context. It
retains context-qualified style branches and their owning styled items as
named opaque source data and produces a
`presentation.context-dependent-style-unresolved` loss. It emits no neutral
appearance for an unavailable or unselected context and does not infer a
selection from direct membership. An unscoped sibling style may transfer
independently. An `ANNOTATION_PLANE` owns each
referenced surface carrier. A native presentation carrier without a neutral
geometry arena retains its carrier identity as the style target. Semantic PMI
retains every supported STEP `SHAPE_ASPECT` subtype as a shape-aspect target,
including a simple leaf subtype and a shape-aspect partial in a complex datum
feature. ISO 10303-47 defines `DATUM` as a `SHAPE_ASPECT` subtype with the
explicit `identification` attribute. In external mapping, the `DATUM` partial
supplies `identification`; the inherited `SHAPE_ASPECT` partial supplies
`name`, `description`, `of_shape`, and `product_definitional`. A datum requires
an empty `name` and `product_definitional = .F.`. CADIR reads these fields by
partial name, retains the source datum identity as a shape-aspect target, and
does not use the first complex partial as an attribute source. A complex datum
therefore retains its `identification` even when a recoverable noncanonical
partial order places `DATUM` after another partial.
A complex dimension uses its dimensional partial for its kind and all inherited
partials for its name, targets, and characteristic value.
ISO 10303-47 defines `DIMENSIONAL_CHARACTERISTIC_REPRESENTATION` as the
association of a dimension with its explicit representation. Its
`SHAPE_DIMENSION_REPRESENTATION.items` attribute is a `SET[1:?]` of
`shape_dimension_representation_item`; the item alternatives include
`MEASURE_REPRESENTATION_ITEM` and `COMPOUND_REPRESENTATION_ITEM`, so the
serialized aggregate order has no meaning. CAx-IF AP242 PMI Recommended
Practices 4.1 §5.2.1 identifies a nominal value by a
`MEASURE_REPRESENTATION_ITEM` whose inherited `REPRESENTATION_ITEM.name` is
`nominal value`, and requires that value even when no range is present. For a
value range, §5.2.4 identifies the items by `nominal value`, `upper limit`, and
`lower limit`.
CADIR decision: the reader traverses the complete item graph and compares the
specified names case-insensitively. One named nominal item supplies the
nominal. Multiple named nominal items are ambiguous and produce
`pmi.dimensional-nominal-ambiguous`. If no named nominal item exists, exactly
one reachable measure is accepted as malformed-source salvage; multiple
measures are ambiguous and produce
`pmi.dimensional-unnamed-measure-ambiguous`. Neither aggregate order nor
entity identity selects a value. Complex measure records referenced by a
characteristic representation remain typed measure carriers.
`GEOMETRIC_ITEM_SPECIFIC_USAGE` resolves a shape-aspect definition, including a
definition reached through a `SHAPE_ASPECT_RELATIONSHIP`, to its identified
geometric or topology item. A resolved face, edge, vertex, body, point, or curve
is added as a typed PMI target while the source shape-aspect target remains. An
unresolved usage remains source-native with its identity and links.
`DATUM_TARGET` and `PLACED_DATUM_TARGET_FEATURE` transfer as typed datum-target
definitions with their target form and identification, while their source
shape-aspect identity remains a PMI target. Standard placed-target forms are
point, line, rectangle, circle, and circular curve; another source description
is retained as an `Other` form. A `FEATURE_FOR_DATUM_TARGET_RELATIONSHIP` whose
related shape aspect is a datum target transfers its relating shape aspect into
the datum target basis; the writer emits the relationship for each shape-aspect
basis target. A relationship without a resolvable datum target remains
source-native. Geometric-item usages whose identified item is a Cartesian point
transfer that point as a typed PMI target.
Geometric validation properties read area, volume, and centroid values through
inherited `REPRESENTATION`, `MEASURE_REPRESENTATION_ITEM`, and
`MEASURE_WITH_UNIT` partials. Direct `AREA_UNIT` and `VOLUME_UNIT` subtypes and
their inherited `DERIVED_UNIT_ELEMENT` factors are typed. ISO 10303-43 defines
`representation.items` as `SET[1:?]`; a geometric-validation representation
is not limited to one item. CAx-IF Recommended Practices for Geometric and
Assembly Validation Properties permit one representation to combine validation
properties when they have the same `PROPERTY_DEFINITION.name` and definition.
The combined representation has an empty name, and each property is
instantiated at most once for the model element. A solid may therefore carry
one volume, one surface-area, and one centroid item in one combined
representation. Every referenced area, volume, or centroid item is evaluated;
derived-unit factors scale area and volume by their dimensions. CADIR decision:
a repeated reference to one item is evaluated once, and an unsupported item
produces a warning without suppressing supported siblings. Item order does not
select a validation value.
ISO 10303-47 §6.2 defines the first compartment of a tolerance frame with one
specific characteristic entity: `ANGULARITY_TOLERANCE`,
`CIRCULAR_RUNOUT_TOLERANCE`, `COAXIALITY_TOLERANCE`,
`CONCENTRICITY_TOLERANCE`, `CYLINDRICITY_TOLERANCE`, `FLATNESS_TOLERANCE`,
`LINE_PROFILE_TOLERANCE`, `PARALLELISM_TOLERANCE`,
`PERPENDICULARITY_TOLERANCE`, `POSITION_TOLERANCE`, `ROUNDNESS_TOLERANCE`,
`STRAIGHTNESS_TOLERANCE`, `SURFACE_PROFILE_TOLERANCE`, `SYMMETRY_TOLERANCE`,
or `TOTAL_RUNOUT_TOLERANCE`. ISO 10303-47 §6.4.9 declares
`GEOMETRIC_TOLERANCE` an abstract supertype, and §6.6.1 requires exactly one
of those characteristic subtypes for each geometric tolerance. A Part 21
complex instance uses the `GEOMETRIC_TOLERANCE` partial for inherited `name`,
`description`, `magnitude`, and `toleranced_shape_aspect`; its exact
characteristic leaf partial supplies the kind. CAx-IF AP242 PMI Recommended
Practices 4.1 §6.9.3 shows the same complex form with
`GEOMETRIC_TOLERANCE_WITH_MODIFIERS` and `POSITION_TOLERANCE` partials.
`GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE` supplies the datum-system link and
does not add a shape-aspect target. The defined-unit and defined-area-unit
partials retain their unit sizes and area shape; modifier aggregates retain
their enumeration values.
CADIR decision: the reader recognizes only the exact characteristic leaf
names above. An unrecognized direct entity name ending in `_TOLERANCE`,
including the abstract `GEOMETRIC_TOLERANCE` name, is not a kind and remains a
named opaque source record. An unrecognized partial inside a complex instance
does not override an exact leaf. Partial order does not select the kind. The
writer emits each supported IR kind with its corresponding characteristic
leaf entity. Presentation PMI retains
annotation identity, text, placement, and explicit occurrence visibility across
inherited annotation partials. `INVISIBILITY` targeting a transferred
presentation annotation sets its `visible=false`; it does not change visibility
on a shared geometry or tessellation carrier. Annotation placeholder
occurrences with a leader line transfer through the same presentation PMI
model. ISO 10303-46 §5.4.13 defines `ANNOTATION_TEXT_OCCURRENCE.item` as a
SELECT of `TEXT_LITERAL`, `ANNOTATION_TEXT`, `ANNOTATION_TEXT_CHARACTER`,
`DEFINED_CHARACTER_GLYPH`, and `COMPOSITE_TEXT`. Its recursive text graph uses
`TEXT_STRING_REPRESENTATION.items : SET[1:?]` (§5.4.46) and
`COMPOSITE_TEXT.collected_text : SET[2:?]` (§5.4.18); §5.2 describes these
entities as recursive collections. Aggregate order therefore has no text
composition meaning. A direct text carrier or a graph with exactly one
reachable text carrier supplies the presentation text.
CADIR decision: the reader traverses the complete reachable reference graph
and counts each text-carrier identity once. A graph with multiple reachable
text carriers has no ordered composition, so the text remains absent, a
metadata loss is emitted, and every carrier and unresolved composition record
remains named opaque data with its source links. Traversal order, aggregate
serialization order, and entity identity never select a carrier. Unmodeled
tessellated annotation carriers remain named opaque records with their source
links. `PLUS_MINUS_TOLERANCE` carries
numeric lower and upper
deviations, or the form variance, zone variance, grade, and source fields of
`LIMITS_AND_FITS`.

An `APLL_POINT` or `APLL_POINT_WITH_SURFACE` referenced by an annotation
placeholder, annotation-to-annotation, annotation-to-model, or auxiliary
leader line transfers its three-dimensional coordinates to a neutral point
with the APLL source identity. The APLL and leader-line records remain named
opaque records because the neutral model has no fields for `symbol_applied`,
`associated_surface`, or ordered leader-line semantics.

Drawing structure is a linked object graph. `DRAWING_DEFINITION` identifies the
drawing, `DRAWING_REVISION` identifies one revision of it, and
`DRAWING_SHEET_REVISION` identifies a sheet revision with its set of drawing
items, presentation context, and revision. `DRAWING_SHEET_REVISION_USAGE`
links a sheet revision to its drawing revision and carries the sheet sequence.
`PRESENTATION_VIEW` carries a named view, its set of representation items, and
presentation context. `PRESENTATION_SIZE` links a sheet revision to its
presentation size.
`DRAUGHTING_MODEL` carries a presentation model with its set of items and context;
in a complex instance, these attributes come from its inherited
`REPRESENTATION` partial.
`DRAUGHTING_MODEL_ITEM_ASSOCIATION` links model items to their semantic
definition. Its `definition` SELECT includes `PRODUCT_DEFINITION_SHAPE`. When
that property's `definition` resolves to a `PRODUCT_DEFINITION`, it identifies
that one product-definition view.
`DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER` carries the
same definition, draughting model, and callout links plus its annotation
placeholder occurrence. Complex association instances read these attributes
from their inherited `ITEM_IDENTIFIED_REPRESENTATION_USAGE` partial.
`DRAUGHTING_CALLOUT` carries a set of callout contents. These representation
aggregates are SETs; their serialization order has no drawing meaning. A
`PRODUCT` is not a `representation_item`, so a valid drawing representation
does not select one of several product-definition views from a bare product
reference. A product-definition shape supplies that view scope through its
owning product definition when its characterized definition is a
`PRODUCT_DEFINITION`. The reader transfers this scoped target to the
corresponding product-definition identity; other typed definition scopes follow
the generic target rule below.
Each other drawing relationship target transfers when its source record has
exactly one neutral, named opaque, or source-native identity. A terminal
source-typed target with no neutral identity owns
an identity-only `NativeRecord` in the STEP `drawing_targets` arena; its
source id and complete source type remain available for the relationship.
`INVISIBILITY` targeting a transferred drawing entity sets its `visible=false`;
it does not change the visibility of that entity's relationships or contents.
A representation-context relationship without a neutral context target uses an
identity-only source-native target in the STEP `drawing_targets` arena. An
annotation plane transfers through its plane carrier, and a mapped item
transfers through the items of its mapped representation, when that wrapper has
no identity of its own and the reachable carrier graph has exactly one neutral
identity and is acyclic. If a source record has multiple neutral identities and
the STEP relation has no type-defined scope that selects one, no target is
selected and the raw source parameter remains stored with
`drawing.relationship-target-ambiguous`. A typed wrapper whose carrier graph is
cyclic or yields no neutral identity receives its own source-native identity.
Target selection does not use identity ordering.
Unsupported drawing graphics retain their source entity and references without
becoming geometric carriers.
