# Autodesk Inventor IPT/IAM: Open Items

This document lists unknown Inventor byte semantics. [inventor.md](inventor.md) contains the settled format model.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. RSe selection and metadata

### RS-01. Multiple coherent databases

**Question.** Which typed field selects one database when several `V<n>/RSeDb` streams have schema 31 but differ in identity or save state?

**Known.** The storage-band number and database schema are independent. The decoder parses all database candidates and requires one coherent registry grammar.

**Need.** We must select one database without using path order or a save-version string.

**Note.** `coherent_schema` ignores unavailable database candidates and takes the first parsed schema before checking the remaining parsed schemas. A malformed candidate can therefore be absent from the grammar decision; database identity and save state remain unselected.

### RS-02. Metadata sections 5 and 6

**Question.** What item grammar and discriminator semantics apply to metadata sections 5 and 6?

**Known.** The backward spans establish their exact ranges and their join to section 4.

**Need.** We must type their records and references.

### RS-03. Section 7 long item form

**Question.** What fields select and define the section 7 item form whose item width is at least 76 bytes?

**Known.** The short section 7 form uses 32-byte items. Both forms have exact backward-framed ranges.

**Need.** We must validate the long item sequence without accepting arbitrary payload widths.

### RS-04. Revision and record-trailer variants

**Question.** Which revision-table versions, extended record-trailer property types, and trailer list forms govern RSe database variants outside the supported branch?

**Known.** The current envelope parses revision-table version 3 and the implemented trailer property and reference-list forms. Unsupported revision versions and trailer property types are refused after their owning record boundary is established.

**Need.** Each revision and trailer variant requires an exact field grammar, range rule, and native retention policy before semantic decode.

**Note.** `parse_revisions` treats every nonzero `kind == u16::MAX` payload selector as the short form and zero as the long form. It does not establish whether other selector values are valid or whether the selector has this polarity.

### RS-05. Registry object and node-count ownership

**Question.** Which registry object owns the count of node records that follow an entry, and what consistency rule applies when an entry contains more than one object?

**Known.** Each registry object stores a `node_count`. The current grammar uses the last object's value minus one as the entry's following node-record count. It does not require the object values to agree or identify the count-owning object.

**Need.** The count owner, bias, and multi-object consistency rule must be exact before the value can frame the following node records.

## 2. Kernel ownership

### KE-01. Multiple part carriers

**Question.** Which typed ownership and history references select the active model when a part contains more than one `PmBRep` segment or kernel-carrier record?

**Known.** The supported envelope has one `PmBRep` segment and one typed kernel-carrier record. Carrier signatures do not establish ownership.

**Need.** We must select active and historical carriers without appending all geometry.

### KE-02. ACIS save-format and carrier-footer bands

**Question.** Which ACIS save-format bands outside 217/218 and which segment-version carrier footers use a compatible SAB grammar and active-carrier semantic contract?

**Known.** ACIS 217 and 218 are the verified bands of the 32-bit header and SAB decoder. Other ACIS save-format bands take the same framing and record decode, carry an `Admission::Unverified` kernel layer with a `source.dialect-unverified` charge, and retain the carrier bytes when they read no geometry. The current carrier footer branch supports segment major versions 15 through 22 and 23 and later; earlier or otherwise incompatible variants are refused.

**Need.** Each additional ACIS or footer band requires direct-carrier framing, geometry, validation, and wrapper-parity evidence before activation.

**Note.** The original `KE-02` was removed by commit `87e40474c` after the 217/218 branch was added. That commit contains a shared implementation and synthetic 217/218 fixtures, but it does not establish the grammar for other ACIS bands or the remaining carrier-footer variants. The item is reopened here with the narrower scope above.

## 3. Product structure

### AS-01. Local prototypes and parent records

**Question.** Which record families define embedded local prototypes and parent-child placement inside one document?

**Known.** A `UFRxDoc` occurrence joins its external reference through the exact file-reference identifier and joins `AmDc` and `AmGraphics` through the exact occurrence identifier. Current-document external occurrences are root placements. A referenced assembly remains one unresolved external prototype because the codec does not load it. External-reference state bit `0x2000` marks a suppressed occurrence.

The schema-15 representation/model-state branch and its occurrence table are framed exactly. Unsupported property and export tags stop semantic transfer without inventing a record boundary.

**Need.** We must establish local component definitions and parent links from typed records before emitting non-root or local-prototype occurrences.

### AS-03. UFRx schema and occurrence-tag branches

**Question.** Which UFRxDoc schemas outside 11 through 15 and which occurrence property/export tags have stable field grammars and semantic roles?

**Known.** Schemas 11 through 15 and the supported occurrence/export tag sets are framed. Other schemas and tags are refused as unsupported semantic branches; their containing UFRx records remain bounded and are reported without inventing fields.

**Need.** Each additional schema or tag requires exact field widths, repeated-tag rules, identity joins, and loss behavior before semantic transfer.

**Note.** `parse_occurrence_export` chooses export branches from the first two u32 values and numeric thresholds (`count`, `next`, and `0xffff`) rather than a typed discriminator. A valid single-entry or future export form in the same numeric range can be consumed as the empty or alternate form.

### AS-02. Transform units and exceptional branches

**Question.** What semantics apply to non-active placement branches, scale, mirrored state, and singular matrices?

**Known.** The active `AmGraphics` branch stores a compact finite 4-by-4 transform and joins it to an `AmDc` occurrence by exact identifier. Placement translations use centimetres and are converted to neutral millimetres. No field in the external-reference prefix supplies an active placement.

**Need.** We must type the exceptional branches before transferring their scale, reflection, or singular-transform semantics.

## 4. Properties, materials, and design intent

### PR-01. OLE property variants and code pages

**Question.** Which additional OLE code pages, property value types, section-directory variants, and preview payloads occur in Inventor documents, and what are their exact mappings?

**Known.** The current parser handles the supported little-endian property-set header, section ranges, scalar/vector/FILETIME/BLOB/clipboard forms, UTF-16, and the admitted `encoding_rs` code-page set. Unknown typed values remain native-only; an unsupported code page or property variant stops typed property projection after its stream has been identified.

**Need.** Additional code pages, typed variants, and preview encodings require exact decoding, range validation, and neutral/native mapping rules before transfer.

**Note.** When a property section has no code-page property, LPSTR and dictionary strings are decoded as Windows-1252. The format model does not establish that an absent code page selects Windows-1252.

### PR-02. Metadata property identity and conflict precedence

**Question.** Which OLE property-set identity and precedence rule select title, author, description, part number, and document kind when multiple sections expose matching names or built-in IDs?

**Known.** `MetadataProjection` accepts normalized names across property sets. It keeps the first nonempty title, author, description, or part number, but replaces the document kind whenever another parseable value is encountered. The traversal order is the only selection rule.

**Need.** A canonical FMTID/property identity and conflict rule are required before metadata can select document semantics or overwrite a neutral value.

### MA-01. Additional face appearance styles

**Question.** Which `PmGraphics` or `FBAttribute` style families bind a Protein appearance asset, texture, or non-diffuse presentation channel to a face?

**Known.** The `PmApp` document-default record selects one rendering-style record by a carrier-local one-based record reference. The rendering style stores the Protein asset GUID and asset-library identifier. Their unique catalog join supplies the default appearance for every part body. A PmGraphics face joins a transferred ASM face through the shared nonnegative Design key. Its object-style collection can select one primary-color style whose second RGBA vector is a direct diffuse face override. The face override has precedence over the body default.

**Need.** Each additional style family requires an exact owner path, channel semantics, and precedence before transfer.

**Note.** The implementation selects `PmGraphicsPrimaryColorStyle.colors[1]` as the diffuse face color without a record discriminator for the four color slots. The slot role remains part of the open channel-semantics question.

### MA-02. Protein material-property identity and precedence

**Question.** Which schema-qualified Protein property owns each material scalar, bitmap URI, bitmap URN, base-color channel, and texture-mapping value, and what value does each schema define when a property is absent?

**Known.** Material projection scans matching property IDs and accepts the first match for texture mapping values, bitmap paths, bitmap URNs, and normalized color candidates. It also maps `generic_refraction_index` and `transparent_refraction_index` to one neutral key, so a later map insertion replaces the earlier value.

**Need.** Schema declarations must establish property ownership, channel precedence, ambiguity behavior, and omission semantics before material values can be transferred.

**Note.** The current projection supplies schema-independent values when mapping properties are absent: map channel 1, UVW source 0, zero offsets and rotation, unit scale, enabled repeat, zero real-world mapping values and bump depth, and unit normal scale. These values are not yet established as the omission semantics of each Protein schema.

### DE-01. Feature and history record graph

**Question.** Which typed records define history order, feature operands, suppression, and result-body transitions?

**Known.** Numeric PmDc parameters, planar sketches, generic feature records, rectangular-pattern records, mirror records, and end-of-features records have stable carrier-qualified identities. Generic, rectangular-pattern, and mirror records supply ordered property-reference vectors. Pattern records also supply ordered participant references. Typed feature properties identify part-operation, extent, hole, fillet, chamfer, Boolean, boundary-patch, feature-dimension, object-collection, fillet-edge-set, edge-collection, edge-item, body, profile-selection, and placement records. Closed extrusion, drilled/countersink/counterbore hole, constant-radius edge-fillet, and equal-distance edge-chamfer branches have exact dimensions, native selections, placements, and native result-body identities. Feature-label records supply an owner, a label, a class identifier, and ordered participant references. Entity-style links retain associative entity identifiers and entity types. Planar point, line, circle, ellipse, placement, geometric-constraint, and dimensional-constraint graphs have exact record references and neutral identities. Exact RSe frames provide type identifiers and record ranges for the remaining design records.

**Need.** Suppression state, authoritative evaluation dependencies, current-model body joins, pattern and mirror transforms, revolve operands, two-sided and face-terminated extents, non-default fillet laws, oriented chamfers, and the remaining operation families require exact record joins before transfer. These fields must resolve before the score advances to L4.

### DE-02. Compound parameter units and function expressions

**Question.** Which PmDc unit graphs and function records define compound dimensions, derived units, offsets, and function-call expression text?

**Known.** A scalar parameter unit with one supported numerator, no denominator, and no derived-unit reference transfers with exact length, angle, or dimensionless semantics. Literal, parameter-reference, unary, and binary arithmetic expression records form a closed ordered graph.

**Need.** Compound or derived units and function calls require exact dimensional and textual semantics before neutral transfer.

### DE-03. Additional sketch geometry and relations

**Question.** What does the planar-sketch `count_value` count, what locus semantics do nonempty constraint maps encode, and which PmDc records define circular arcs, splines, offset curves, projected geometry, text, pattern relations, and branched profile regions?

**Known.** The `count_value` is the u32 field between sketch state and the type-8 reference array. Point, bounded-line, circle, and ellipse entities form closed planar sketch graphs. Coincident, parallel, perpendicular, tangent, horizontal, vertical, circle-center, equal-radius, radius, diameter, horizontal-distance, and vertical-distance relations with empty constraint maps have exact operands.

**Need.** The count requires an exact referent before validation uses it. Nonempty constraint maps require exact locus semantics before neutral transfer. Each additional entity and relation requires its complete solved geometry, ordered operands, parameter role, and profile-boundary semantics.

## 5. Adjacent envelopes

### EN-01. Other CFB and RSe versions

**Question.** Which RSe schema, metadata, record-trailer, and carrier variants accompany CFB version 4 or metadata versions other than 8?

**Known.** The compound reader admits valid CFB version 3 and 4 containers. The Inventor semantic envelope is schema 31 and metadata version 8. Those grammars are applied to every document, whatever it declares; a declaration outside them leaves the document dialect-unverified.

**Need.** Each governing variant requires a separate finite support envelope.

### EN-02. Drawing and presentation documents

**Question.** Which segment families and records authoritatively identify and define IDW and IPN document semantics?

**Known.** Property metadata can name drawing and presentation kinds. Their semantic decode is not part of the IPT/IAM envelope.

**Need.** Future family support must extend the Inventor codec without treating these documents as parts or assemblies.
