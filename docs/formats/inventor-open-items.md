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

### AS-01. Prototype and hierarchy joins

**Question.** Which typed fields join an occurrence to its external document reference and parent occurrence, and which flags define visibility and suppression?

**Known.** The typed `AmDc` occurrence record owns a document-local occurrence identifier. The typed `AmGraphics` placement record repeats that identifier and supplies its compact affine transform. `UFRxDoc` stores external document references and an occurrence count. The count does not define prototype or hierarchy joins.

The schema-15 two-representation-string header branch and its model-state parameter table are framed exactly. Other schema-15 header branches place part configuration data before the common document fields and remain retained as unsupported branches.

**Need.** We must construct ordered occurrences with exact external prototypes, parents, visibility, and suppression without multiplying a reference count into invented entities.

### AS-02. Transform units and exceptional branches

**Question.** Which length unit applies to transform translations, and what semantics apply to non-active placement branches, scale, mirrored state, and singular matrices?

**Known.** The active `AmGraphics` placement branch stores a compact finite 4-by-4 transform and joins it to an `AmDc` occurrence by exact identifier. No field in the external-reference prefix supplies a placement.

**Need.** We must transfer transforms to neutral occurrences without assuming a unit conversion or exceptional-branch meaning.

## 4. Materials and design intent

### MA-01. Appearance assignment records

**Question.** Which `PmApp`, `FBAttribute`, or B-rep fields bind a Protein asset GUID to a body or face, and what precedence applies?

**Known.** Protein defines an asset catalog. A catalog entry alone is not an assignment.

**Need.** We must transfer body and face colors without assigning the sole compatible asset globally.

### DE-01. Design record graph

**Question.** Which typed records define parameter identity, sketches, constraints, history order, feature operands, suppression, and result-body transitions?

**Known.** Exact RSe frames provide type identifiers, record ranges, and carrier-qualified identities.

**Need.** Each neutral feature requires complete operation semantics and ordered operands.

## 5. Adjacent envelopes

### EN-01. Other CFB and RSe versions

**Question.** Which RSe schema, metadata, record-trailer, and carrier variants accompany CFB version 4 or metadata versions other than 8?

**Known.** The compound reader admits valid CFB version 3 and 4 containers. The Inventor semantic envelope is schema 31 and metadata version 8.

**Need.** Each governing variant requires a separate finite support envelope.

### EN-02. Drawing and presentation documents

**Question.** Which segment families and records authoritatively identify and define IDW and IPN document semantics?

**Known.** Property metadata can name drawing and presentation kinds. Their semantic decode is not part of the IPT/IAM envelope.

**Need.** Future family support must extend the Inventor codec without treating these documents as parts or assemblies.
