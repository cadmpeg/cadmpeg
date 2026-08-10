# Autodesk Inventor IPT/IAM: Open Items

This document lists unknown Inventor byte semantics. [inventor.md](inventor.md) contains the settled format model.

## 1. RSe selection and metadata

### RS-01. Multiple coherent databases

**Question.** Which typed field selects one database when several `V<n>/RSeDb` streams have schema 31 but differ in identity or save state?

**Known.** The storage-band number and database schema are independent. The decoder parses all database candidates and requires one coherent registry grammar.

**Need.** We must select one database without using path order or a save-version string.

### RS-02. Metadata sections 5 and 6

**Question.** What item grammar and discriminator semantics apply to metadata sections 5 and 6?

**Known.** The backward spans establish their exact ranges and their join to section 4.

**Need.** We must type their records and references.

### RS-03. Section 7 long item form

**Question.** What fields select and define the section 7 item form whose item width is at least 76 bytes?

**Known.** The short section 7 form uses 32-byte items. Both forms have exact backward-framed ranges.

**Need.** We must validate the long item sequence without accepting arbitrary payload widths.

## 2. Kernel ownership

### KE-01. Multiple part carriers

**Question.** Which typed ownership and history references select the active model when a part contains more than one `PmBRep` segment or kernel-carrier record?

**Known.** The supported envelope has one `PmBRep` segment and one typed kernel-carrier record. Carrier signatures do not establish ownership.

**Need.** We must select active and historical carriers without appending all geometry.

### KE-02. Binary ACIS records

**Question.** What binary ACIS record grammar applies after the `ACIS BinaryFile` header?

**Known.** The Inventor record envelope and exact carrier range are defined.

**Need.** The grammar belongs in `cadmpeg-asm` so direct and embedded decode use one implementation.

## 3. Product structure

### AS-01. Local prototypes and parent records

**Question.** Which record families define embedded local prototypes and parent-child placement inside one document?

**Known.** A `UFRxDoc` occurrence joins its external reference through the exact file-reference identifier and joins `AmDc` and `AmGraphics` through the exact occurrence identifier. Current-document external occurrences are root placements. A referenced assembly remains one unresolved external prototype because the codec does not load it. External-reference state bit `0x2000` marks a suppressed occurrence.

The schema-15 representation/model-state branch and its occurrence table are framed exactly. Unsupported property and export tags stop semantic transfer without inventing a record boundary.

**Need.** We must establish local component definitions and parent links from typed records before emitting non-root or local-prototype occurrences.

### AS-02. Transform units and exceptional branches

**Question.** What semantics apply to non-active placement branches, scale, mirrored state, and singular matrices?

**Known.** The active `AmGraphics` branch stores a compact finite 4-by-4 transform and joins it to an `AmDc` occurrence by exact identifier. Placement translations use centimetres and are converted to neutral millimetres. No field in the external-reference prefix supplies an active placement.

**Need.** We must type the exceptional branches before transferring their scale, reflection, or singular-transform semantics.

## 4. Materials and design intent

### MA-01. Additional face appearance styles

**Question.** Which `PmGraphics` or `FBAttribute` style families bind a Protein appearance asset, texture, or non-diffuse presentation channel to a face?

**Known.** The `PmApp` document-default record selects one rendering-style record by a carrier-local one-based record reference. The rendering style stores the Protein asset GUID and asset-library identifier. Their unique catalog join supplies the default appearance for every part body. A PmGraphics face joins a transferred ASM face through the shared nonnegative Design key. Its object-style collection can select one primary-color style whose second RGBA vector is a direct diffuse face override. The face override has precedence over the body default.

**Need.** Each additional style family requires an exact owner path, channel semantics, and precedence before transfer.

### DE-01. Feature and history record graph

**Question.** Which typed records define history order, feature operands, suppression, and result-body transitions?

**Known.** Numeric PmDc parameters, planar sketches, generic feature records, and end-of-features records have stable carrier-qualified identities. A generic feature record supplies an ordered property-reference vector. Typed feature properties identify part-operation, extent, hole, fillet, Boolean, boundary-patch, feature-dimension, object-collection, fillet-edge-set, edge-collection, edge-item, body, profile-selection, and placement records. A constant-radius fillet edge set stores its edge collection, radius, selection-mode, and continuity references. An edge item stores an integer index-reference list and two scalar fields. Feature-label records supply an owner, a label, a class identifier, and ordered participant references. Entity-style links retain associative entity identifiers and entity types. Planar point, line, circle, ellipse, placement, geometric-constraint, and dimensional-constraint graphs have exact record references and neutral identities. Exact RSe frames provide type identifiers and record ranges for the remaining design records.

**Need.** Suppression state, authoritative evaluation order, linked-attribute chains, topological result transitions, and each operation family's complete property roles must resolve before neutral feature transfer.

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

**Known.** The compound reader admits valid CFB version 3 and 4 containers. The Inventor semantic envelope is schema 31 and metadata version 8.

**Need.** Each governing variant requires a separate finite support envelope.

### EN-02. Drawing and presentation documents

**Question.** Which segment families and records authoritatively identify and define IDW and IPN document semantics?

**Known.** Property metadata can name drawing and presentation kinds. Their semantic decode is not part of the IPT/IAM envelope.

**Need.** Future family support must extend the Inventor codec without treating these documents as parts or assemblies.
