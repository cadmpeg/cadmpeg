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

**Known.** A `UFRxDoc` occurrence joins its external reference through its `reference_id`, checked against its first-section document identifier, and joins `AmDc` and `AmGraphics` through the exact occurrence identifier. Current-document external occurrences are root placements. A referenced assembly remains one unresolved external prototype because the codec does not load it. External-reference state bit `0x2000` marks a suppressed occurrence.

The schema-15 representation/model-state branch and its occurrence table are framed exactly. Unsupported property and export tags stop semantic transfer without inventing a record boundary.

**Need.** We must establish local component definitions and parent links from typed records before emitting non-root or local-prototype occurrences.

### AS-03. UFRx schema and occurrence-tag branches

**Question.** Which UFRxDoc schemas outside 11 through 15 and which occurrence property/export tags have stable field grammars and semantic roles?

**Known.** Schemas 11 through 15 and the supported occurrence/export tag sets are framed. Other schemas and tags are refused as unsupported semantic branches; their containing UFRx records remain bounded and are reported without inventing fields.

Pre-schema-15 level-of-detail records are framed, but their member semantics are not transferred: kind 1 members and the kind 2 subassembly level-of-detail selection are retained natively. Which members a level of detail suppresses, and how the active level of detail is chosen, is unproven.

Some assembly saves, for example weldment templates, store occurrence-table records that do not follow the occurrence grammar; they stay malformed UFRx records. Schema-15 model-state records are framed for section-version entry 2 values 16 or earlier and 18 or later; the suffix length at entry 2 value 17 is not established, and those records are refused. The schema-15 header grammar of an assembly with more than the default representation, model state, or level of detail is unknown.

The meaning of the eight export-entry state bytes, of the export padding byte, and of first-section tag `0x1e` (a second name that usually equals the component name) is unknown.

**Need.** Each additional schema or tag requires exact field widths, repeated-tag rules, identity joins, and loss behavior before semantic transfer.

**Note.** `parse_occurrence_export` chooses export branches from the first two u32 values and numeric thresholds (`count`, `next`, and `0xffff`) rather than a typed discriminator. A valid single-entry or future export form in the same numeric range can be consumed as the empty or alternate form.

### AS-02. Transform units and exceptional branches

**Question.** What semantics apply to non-active placement branches, scale, mirrored state, and singular matrices?

**Known.** The active `AmGraphics` branch stores a compact finite 4-by-4 transform and joins it to an `AmDc` occurrence by exact identifier. Placement translations use centimetres and are converted to neutral millimetres. No field in the external-reference prefix supplies an active placement.

**Need.** We must type the exceptional branches before transferring their scale, reflection, or singular-transform semantics.

### AS-04. Occurrence visibility and grounded state

**Question.** Which fields store the visibility and the grounded state of one assembly occurrence?

**Known.** External-reference state bit `0x2000` is the only decoded occurrence state. It marks a suppressed occurrence and sets `visible = false`. No other field sets `visible`.

The first `UFRxDoc` occurrence property section has the one-byte tags `0x07`, `0x0d`, `0x0f`, `0x10`, and `0x1d`. Tag `0x0f` and tag `0x10` change on one occurrence while all other occurrence, `AmDc`, and `AmGraphics` fields stay the same. Neither tag is visibility: the document preview shows the occurrence with each value. Tag `0x0f` is not the grounded state: an occurrence with tag `0x0f` set moves together with other occurrences when the constraint solution changes, and a one-occurrence assembly usually stores 0. Tag `0x0f` also stores values other than 0 and 1, for example `0x2a`, `0x37`, `0xd3`, and `0xff`. `AmDc` occurrence flag bits `0x00400000` and `0x00800000` change with no proven role. Bit `0x10` occurs only together with one of them.

Visibility is a property of a design-view representation. `DesignViewSegment` stores one record of type `41a2c29dd211cc99a0002aa976d456c9` for each representation, with a counted UTF-16LE name and a list of record references. Records of type `553a6b21d31123656000f587346bfbb0` contain a counted u32 list whose values are `UFRxDoc` occurrence keys, the first entry at the root. Records of types `2133049bd21187a9a0002ea976d456c9` and `2123c271d21187a9a0002ea976d456c9` store small state values. The active representation, the join from a state record to a key-path record, and the meaning of the state values are unknown.

**Need.** We must find the field for each state, and prove each value with two documents that differ only in that state, before we set `visible` from a representation or emit a grounded state. A grounded occurrence can map to a neutral `AssemblyJoint` with `JointOperands::Grounded` and one connector on the occurrence. The IR needs no addition for it.

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

**Known.** Model roots, root labels, and group labels define the browser history order, the end-of-part position, and the flat-pattern history. Content-header flag bits define suppression and the below-end-of-part state. Feature labels identify consumed sketches, and pattern-feature participants identify seed features. Numeric PmDc parameters, planar sketches, generic feature records, rectangular-pattern records, mirror records, and end-of-features records have stable carrier-qualified identities. Generic, rectangular-pattern, and mirror records supply ordered property-reference vectors. Pattern records also supply ordered participant references. Typed feature properties identify part-operation, extent, hole, fillet, chamfer, Boolean, boundary-patch, feature-dimension, object-collection, fillet-edge-set, edge-collection, face-collection, selection-list, edge-item, body, profile-selection, and placement records. Closed one-sided, symmetric, two-sided, to, and from-to extrusion, drilled/countersink/counterbore hole, constant-radius edge-fillet, equal-distance edge-chamfer, full-revolution revolve about a work axis, and ten-slot shell branches have exact dimensions, native selections, placements, and native result-body identities. Feature-label records supply an owner, a label, a class identifier, and ordered participant references. Entity-style links retain associative entity identifiers and entity types. Planar point, line, circle, ellipse, placement, geometric-constraint, and dimensional-constraint graphs have exact record references and neutral identities. Exact RSe frames provide type identifiers and record ranges for the remaining design records. Thread annotations have nine typed property slots and a native face selection. The meanings of Boolean slots 1, 2, and 6, numeric slots 3 and 4, the slot-5 edge selection, and slot 7 remain unknown. The face-item selector address space and its join to input topology remain unknown. Loft section lists define ordered native section selections. Section records link to a kernel entity, a condition, two numeric parameters, and an optional Boolean. Loft and sweep records have typed control enumerations and result references. Split properties have two typed enumerations and two Boolean records. The Split tool enumeration selects a work-plane reference or a surface-body wrapper. Exact RSe frames provide type identifiers and record ranges for the remaining design records. Thread annotations have nine typed property slots and a native face selection. The meanings of Boolean slots 1, 2, and 6, numeric slots 3 and 4, the slot-5 edge selection, and slot 7 remain unknown. The face-item selector address space and its join to input topology remain unknown.

**Need.** The Thicken Boolean-to-side map, symmetric distance convention, offset-versus-solid mode map, inner selection records, and the meanings of its two opaque control records remain unresolved. Body-level evaluation dependencies, current-model body joins, pattern and mirror transforms, revolve extents other than a full revolution, extrusion targets other than work planes and plane faces, the meaning of the termination flag, non-default fillet laws, oriented chamfers, Shell slots 0, 6, 7, and 8, non-null Shell slot 4, the Shell slot-10 extension, nonempty Shell selection lists, and the remaining operation families require exact record joins before transfer. Shell face references require a join to historical faces and input bodies. The Shell controls require rules for thickness variation, direction, offset construction, corner joins, and intersections. Nonempty section type-7 maps need an entry grammar. Loft construction flags, section-condition values, section scalar fields, auxiliary selections, sweep orientation and scaling values, the sweep angular parameter role, and path extent controls need semantic rules before neutral loft or sweep construction. Split operation-mode values, the roles and polarity of the two Boolean properties, face-selection joins, and input-to-result body transitions require exact rules. These fields must resolve before the score advances to L4.


### DE-02. Compound parameter units and function expressions

**Question.** Which PmDc unit graphs define compound dimensions, derived units, and offsets, and which functions do function-call codes other than 1, 2, and 3 name?

**Known.** A scalar parameter unit with one supported numerator, no denominator, and no derived-unit reference transfers with exact length, angle, or dimensionless semantics. Literal, parameter-reference, unary, and binary arithmetic expression records form a closed ordered graph. Function-call codes 1, 2, and 3 are cosine, sine, and tangent.

**Need.** Compound or derived units and other function codes require exact dimensional and textual semantics before neutral transfer.

### DE-03. Additional sketch geometry and relations

**Question.** What does the planar-sketch `count_value` count, what locus semantics do nonempty constraint maps encode outside the coincident, tangent, distance, circle-center, and midpoint solver hints, and which PmDc records define elliptical arcs, splines, offset curves, projected geometry, text, and pattern relations, which `entity_flags` bit marks construction geometry, what the midpoint relation's trailing `value:u32` encodes, what the regular-polygon record's `value:u32` and the polygon relation's two trailing u32 values encode, what the tangent hint codes 1 and 2 state for a circular-arc operand, and which angle-dimension `flags` value selects the supplement of the line angle?

**Known.** The `count_value` is the u32 field between sketch state and the type-8 reference array. Point, bounded-line, circle, circular-arc, and closed-ellipse entities form closed planar sketch graphs. An ellipse record with two leading point references is bounded. Profile wire bodies use curves whose `entity_flags` hold bits `0x40`, `0x1000`, `0x40000`, and `0x80000`, so these bits do not mark construction geometry. Bits `0x8000` and `0x200000` occur only on curves that no profile wire uses. Coincident, parallel, perpendicular, tangent, horizontal, vertical, circle-center, equal-radius, equal-length, midpoint, fix, horizontal-alignment, vertical-alignment, collinear, symmetric, polygon, equal-distance, sketch-block, radius, diameter, distance, angle, horizontal-distance, and vertical-distance relations with empty constraint maps have exact operands. The midpoint value is 0 in every record examined. A tangent hint code keyed by a circular-arc operand does not change the tangent relation. Code 1 keyed by an arc occurs where the tangency is at the arc start; code 2 occurs where it is at the arc end and where it is inside the arc.

**Need.** The count requires an exact referent before validation uses it. Nonempty constraint maps outside the coincident, tangent, distance, circle-center, and midpoint solver hints require exact locus semantics before neutral transfer. A tangent hint code 1 or 2 keyed by a circular-arc operand requires its exact meaning before the hint can place the tangency. The polygon record value and the polygon relation values require exact referents before a polygon transfers as more than its edges and relations. Each additional entity and relation requires its complete solved geometry, ordered operands, parameter role, and profile-boundary semantics.

### DE-04. Parameter tables without a model owner

**Question.** Which record owns a PmDc parameter table whose context reference is null, and what scope applies to its parameters?

**Known.** These tables have flags `0x20` and two-entry member lists. No record references the table. Their parameters are referenced by records of type `b038b999d211c35b6000b3856c2387b0`. Their parameters remain native with a parameter-scope loss.

**Need.** The owning record and its feature or sketch identity must be exact before these parameters can enter a neutral scope.

### DE-05. Model-root and parameter-table fields

**Question.** What do the model-root flags, the model-root values other than values 3 and 9, the model-root `scale` and `terminal_value`, the parameter-table member list, and the parameter-table suffix encode?

**Known.** Value 3 references the root's parameter table and value 9 is null or repeats it. Value 12 is 1 in document model roots and 7 in flat-pattern model roots. The member list and the name map contain every parameter of the table and also other records. The suffix is retained by length.

**Need.** The model graph, the flat-pattern definition operands, and the table membership rule must be exact before the flat-pattern model can transfer more than its parameter scope.

### DE-06. Profile selections outside the whole-loop form

**Question.** What do these profile selections select: a selection whose entity link is an entity-style link; a wire edge whose naming attribute has another integer layout; a wire edge whose associative identifier names no curve of the consumed sketch; a wire whose edges do not form one closed ring; and one ring selected twice in one boundary patch?

**Known.** A selection whose wire body edges each run along a whole curve or a part of a curve of the consumed sketch is one closed ring of that sketch. Some wire bodies name associative identifiers that the consumed sketch gives to points. In some wire bodies an edge names one curve of the consumed sketch and lies on another curve of that sketch. Some naming attributes store three integers after the five leading fields.

**Need.** Each form requires its exact sketch curves or region rule before the extrusion profile can transfer.

### DE-07. Other PmDc segment versions

**Question.** Which content-header, stamp, list, and record forms do PmDc segments of major versions 24, 26, and 27 use, what do content-header stamps encode, and what roles do the scalar pair and the operand references of a state-2 point have?

**Known.** Segment major versions 23, 25, and 28 through 31 use the header and record forms of the specification. A stamp is −1 or a nonnegative value; nonnegative stamps include values whose hundreds equal the segment major version. A state-2 point has two finite scalars and two entity references. The point position gives its solved coordinates.

**Need.** Each other version requires its header and record forms before its PmDc records decode. A stamp requires an exact referent before it can affect transfer. The scalar and operand roles require exact semantics before this association can define a neutral relation.

### DE-08. Sheet-metal feature records

**Question.** What do the bend operands of a sheet-metal plate, the other thickness-side enumeration values, and the bend, corner, hem, and flat-pattern definition properties encode?

**Known.** Feature labels with class identifiers `c65f5855d311730c60003aa99dccefb0`, `f258f74cd3114f24600047a99dccefb0`, `7f588068b74378a0559515946f7a1c12`, `852f6bb0d411db6f60004daa9dccefb0`, and `aaf21093d344209a04cc799656cd7e09` label generic feature records of a plate, a bend, a corner, a hem, and a flat-pattern definition. `inventor.md` §13 defines the plate profile, direction, thickness, Boolean sides, enumeration side 0, direct result body, and 17-property result-collection form. A bend-free new-body plate has complete planar base-flange operands. Some plates also hold `92596ba9d311efa86000b3a99dccefb0` in slot 4 and `83ce7786a946cde11ebd99b4fe02afb2`, `64880596d3119a76600081a99dccefb0`, `e9345026d5114c081000e596c28b7ab5`, `ce2d9b29ad43d8802f132c9a40c9ba90`, and `051772e5a74f641a01386e8f3cc02218` in slots 9 through 13. Bend, hem, and corner records hold records of the same types. A bend holds an edge collection, two parameters, and `3441b690d511d8026000a28d4aa6fbb0`; a hem holds `e5b810b3d411a37a600050aa9dccefb0`. A flange label owns a flange record (`inventor.md` §13), which names its plate, bend, and corner records. The sheet-metal style supplies parameters named `Thickness`, `BendRadius`, `BendReliefWidth`, `BendReliefDepth`, `CornerReliefSize`, `MinimumRemnant`, `TransitionRadius`, `JacobiRadiusSize`, `GapSize`, `SeamGap`, and `KFactor`.

**Need.** Other thickness-side enumeration values require their distribution about the sketch plane. Bend and attached-plate features require the selected historical faces or edges, thickness ownership, inside radius, bend angle, side, bend position, and result-body transition. Corner features require seam and relief geometry, mode, and dimensions. Hem features require edge operands, bend position, form, form dimensions, fold direction, inside radius, and result bodies. Flat-pattern definitions require their construction operands.

### DE-09. Additional edge naming forms

**Question.** Which selector forms and naming families resolve edge items outside the direct schema-44 matched-attribute form?

**Known.** An edge-item index reference names a selector by its PmDc source index. A direct selector and an ASM matching attribute share a source index and an edge key. A generated attribute on the same edge supplies its generating feature and associative name. The archive can contain this edge after a later operation removes it from the active body.

**Need.** Additional selector forms, secondary naming arrays, matched-attribute qualifiers, generated roles, and naming families require exact operands and joins before neutral edge resolution.

## 5. Adjacent envelopes

### EN-01. Other CFB and RSe versions

**Question.** Which RSe schema, metadata, record-trailer, and carrier variants accompany CFB version 4 or metadata versions other than 8?

**Known.** The compound reader admits valid CFB version 3 and 4 containers, and a DIFAT chain terminated by FREESECT only at its exact FAT-implied count. FREESECT terminators on directory, mini-FAT, and stream chains have not been observed. The Inventor semantic envelope is schema 31 and metadata version 8. Those grammars are applied to every document, whatever it declares; a declaration outside them leaves the document dialect-unverified.

**Need.** Each governing variant requires a separate finite support envelope.

### EN-02. Drawing and presentation documents

**Question.** Which segment families and records authoritatively identify and define IDW and IPN document semantics?

**Known.** Property metadata can name drawing and presentation kinds. Their semantic decode is not part of the IPT/IAM envelope.

**Need.** Future family support must extend the Inventor codec without treating these documents as parts or assemblies.
