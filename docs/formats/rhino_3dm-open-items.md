# Rhino 3DM Open Items

The following items were reopened by the 2026-08-15 QA audits. Settled format
rules remain in [`rhino_3dm.md`](rhino_3dm.md). Independent transfer evidence
remains in [`rhino_3dm-opennurbs-comparison.md`](rhino_3dm-opennurbs-comparison.md).

## Reopened items

### PP-01. Plug-in class payload grammar

**Question.** What payload grammar and semantics does each third-party plug-in class UUID select?

**Known.** `decode.rs:2460-2500` retains an unregistered object or table record as an opaque byte range. Section 20.6 states that a class UUID alone does not admit typed transfer.

**Need.** Establish the payload grammar and typed-transfer rule for each supported plug-in class, or explicitly define the class as permanently opaque with a machine-readable loss contract.

**Note.** Reopened because retaining the complete record preserves bytes but does not answer the original grammar or semantic question.

### PP-02. Plug-in class field semantics

**Question.** Which fields in a supported plug-in class payload carry transferable object state?

**Known.** The decoder does not inspect the payload after the class UUID and retains it through the opaque-record path at `decode.rs:2460-2500`.

**Need.** Define field boundaries, required fields, and neutral mappings for each supported plug-in class.

**Note.** Reopened for the same refusal-as-answer: opaque retention is a preservation policy, not field semantics.

### PP-03. Plug-in user-data dictionary grammar

**Question.** What record and value grammar applies to plug-in user-data dictionaries?

**Known.** Unknown user-data records are retained as opaque data under `rhino_3dm.md` §20.6 "An unregistered class UUID has no typed payload contract."; no typed dictionary grammar is established.

**Need.** Establish the dictionary record boundaries, value types, and transfer semantics for supported plug-in dictionaries.

**Note.** Reopened because the current path can preserve a dictionary without making any of its fields available to typed transfer.

### PP-04. Plug-in direct user-record grammar

**Question.** What payload grammar and ownership rules apply to plug-in direct user records?

**Known.** `decode.rs:2492-2500` retains unknown direct records as opaque bytes, and section 20.6 does not define their internal fields.

**Need.** Define the direct-record header, payload boundary, ownership, and typed or opaque admission rule.

**Note.** Reopened because the closure supplies no grammar for the originally requested direct-record fields.

### PP-05. Future object attribute items

**Question.** What width and value grammar applies to unknown future object-attribute items?

**Known.** Section 20.6 marks unknown future attribute items opaque at `rhino_3dm.md` §20.6 "Object attributes and layer extensions use tagged streams without a length for"; the typed parser cannot consume their fields.

**Need.** Establish a bounded skip grammar and preservation rule for each future item, including how its length is obtained.

**Note.** Reopened because whole-record retention does not establish the item boundary or permit later fields to be decoded.

### PP-06. Future layer and settings items

**Question.** What width and value grammar applies to unknown future layer and settings items?

**Known.** The current policy retains unknown future layer/settings material as opaque data, but the parser has no typed field grammar for it.

**Need.** Establish item boundaries, supported values, and forward-compatible skipping for future layer and settings items.

**Note.** Reopened because the retained bytes do not answer the original future-item grammar question.

### FV-01. Future object-class payloads

**Question.** Which later object-class payload versions are compatible with the typed decoder?

**Known.** Later or unregistered class payloads fall through to the opaque record path at `decode.rs:2460-2500`. Section 20.6 admits no typed transfer from a class UUID alone.

**Need.** Define versioned payload admission and field compatibility for each supported later object class.

**Note.** Reopened because byte preservation does not establish compatibility with any future payload version.

### FV-02. Future table-record payloads

**Question.** Which later table-record payload versions can be decoded as typed records?

**Known.** Unknown table records are retained by `decode.rs:2460-2500`; no version-specific field grammar is applied after the record identity.

**Need.** Define table-record version admission, field boundaries, and neutral mapping for later payloads.

**Note.** Reopened because retention does not establish typed table-record compatibility.

### FV-03. Future user-data payloads

**Question.** Which later user-data payload versions are typed, and which are opaque?

**Known.** Section 20.6 applies an opaque fallback to unregistered user-data at `rhino_3dm.md` §20.6 "An unregistered class UUID has no typed payload contract." without a versioned field grammar.

**Need.** Define version admission, field boundaries, and loss behavior for later user-data payloads.

**Note.** Reopened because the closure retains future bytes but does not resolve their versioned semantics.

### FV-04. Future presentation records

**Question.** Which later presentation-record versions may append fields before their bounded end?

**Known.** Several presentation readers still impose local version caps and require zero remaining bytes; for example `presentation.rs:611-678`, `771-898`, and `935-1023`.

**Need.** Define the supported version range and suffix handling for every presentation record, with evidence for each local ceiling.

**Note.** Reopened because a later presentation record can be rejected or degraded even when its bounded prefix is readable.

### FV-05. Future view and clipping records

**Question.** Which later view and clipping-record versions are accepted, and how are appended fields skipped?

**Known.** `views.rs:630-675` rejects clipping-plane minor versions above 3, while `rhino_3dm.md` §13.4 "The clipping-plane chunk has major version 1 and minor versions 0 through 5:" specifies fields introduced at later minors.

**Need.** Define version admission and bounded suffix parsing for view and clipping records through the settled minor range.

**Note.** Reopened because the current reader rejects specified later minors instead of consuming or skipping their bounded fields.

### LG-01. Direct V1 record dispatch

**Question.** Which direct V1 typecodes are typed records rather than opaque records?

**Known.** `rhino_3dm.md` §8.2 "V1 geometry is a flat sequence. The object reader dispatches these direct" lists text blocks, annotation leaders, dimensions, and pre-class NURBS records as direct V1 records. `legacy.rs:1218-1459` dispatches only units, points, legacy curve/face/shell, and mesh; the listed annotation and pre-class records fall through to `opaque_records`.

**Need.** Add the settled direct-V1 typecodes to the dispatch model or revise the specification to state their opaque rule and loss behavior.

**Note.** Reopened because a spec-listed direct record is not decoded as typed data. The later annotation decoder does not change the V1 dispatch table.

### LG-02. V2 class-payload compatibility

**Question.** Do all V2 class payloads use the later archive class-data grammar and version fields?

**Known.** `rhino_3dm.md` §8.2 "V2 uses the table and polymorphic class-record grammar in sections 7 through" states that all V2 class payloads use that grammar, while `chunks.rs:30-67` only establishes generic V2 chunk framing. The comparison document provides aggregate archive evidence but no class-specific V2 payload witness for this rule.

**Need.** Provide class-specific V2 evidence for the payload boundary and version fields, or narrow the specification to the classes established by evidence.

**Note.** Reopened as a promotion-to-spec gap. A V2 class can be admitted under an unverified later-archive assumption and then be falsely typed or lose unsupported fields.

### RS-01. Later-minor bounded suffixes

**Question.** Which versioned records accept unread fields appended before their bounded end?

**Known.** The global rule at `rhino_3dm.md` §4.2 "A reader consumes the fields defined for the payload major version. A" permits later-minor suffixes. `presentation.rs:611-678`, `771-898`, `935-960`, and `974-1023` still reject non-empty suffixes; other readers have similar local checks.

**Need.** Audit every versioned reader, remove unjustified zero-tail requirements, and document only writer-band ceilings supported by evidence.

**Note.** Reopened because a valid bounded later-minor suffix can make a containing presentation record fail or become opaque.

### RS-02. Later-minor version admission

**Question.** What version ceilings are valid for each versioned record, independent of its field suffix?

**Known.** The global rule rejects a minor only at a documented writer-band ceiling. Local readers still use narrow caps, including the clipping-plane cap in `views.rs:630-675`, despite the specified minor range through 5 at `rhino_3dm.md` §13.4 "The clipping-plane chunk has major version 1 and minor versions 0 through 5:".

**Need.** Establish record-specific version ceilings and parse or skip every settled later-minor field before the bounded end.

**Note.** Reopened because current admission rejects specified later minors. The clipping-plane minor-4/5 mismatch is the clearest concrete case.

### RS-04. Strict boolean validation in object attributes

**Question.** Are object-attribute boolean fields validated against the writer-version strictness threshold?

**Known.** `objects.rs:612-618` does not receive `writer_version`; tagged items at `objects.rs:887`, `921`, `942`, and `962` use permissive `reader.bool()`. The settled threshold is specified at `rhino_3dm.md` §4.2 "Stored Boolean fields use one byte. `0x00` is false and `0x01` is true. When"; the source reader rejects noncanonical values for modern writers.

**Need.** Thread writer-version context into object-attribute parsing and apply strict validation to each affected boolean field.

**Note.** Reopened because a modern archive containing byte `0x02` is currently accepted as true instead of rejected as malformed.

### RS-05. Unknown SubD symmetry enums

**Question.** What is the fallback for an unknown SubD symmetry type or coordinate-system enum?

**Known.** `subd.rs:1128-1207` rejects symmetry types outside 1 through 5 and coordinate systems above 2. The source reader maps unknown symmetry types to `Unset` and keeps the containing record readable. The global enum rule is at `rhino_3dm.md` §4.2 "Presence and enumeration fields that define a separate numeric grammar retain".

**Need.** Retain unknown enum values natively, map them to the neutral fallback, and emit typed degradation without discarding the containing SubD record.

**Note.** Reopened because an unknown enum currently rejects the candidate instead of following the settled enum policy.

### SW-03. Mixed instance-member transforms

**Question.** Which entities emitted by one instance-definition member receive the member transform when the member emits both a body carrier and auxiliary geometry?

**Known.** `decode.rs:1906-2125` uses a checkpoint and treats the presence of a newly emitted body as the boundary for transforming bodies; it does not independently establish the handling of auxiliary entities from the same member. Existing tests cover separate member shapes but not a mixed-member witness.

**Need.** Define the mixed-member boundary and add an independent witness that proves the transform and retention behavior for every emitted entity kind.

**Note.** Reopened because a member that emits both a body and auxiliary geometry can leave the auxiliary geometry outside the transform decision, and aggregate instance tests do not establish the source boundary rule.

### TE-01. Class-specific transfer differential evidence

**Question.** Which Rhino-authored object classes differ from the committed transfer fixtures, and which byte-level fields cause the difference?

**Known.** The comparison harness records aggregate source floors and a synthesized point/structured tier at `integration_tests.rs:514-530` and in the opening source-tier and synthesized-tier paragraphs of `rhino_3dm-opennurbs-comparison.md`. It does not preserve a byte-level, class-by-class differential witness for each affected source file.

**Need.** Add class-specific transfer witnesses with byte-level differences and accepted/rejected outcomes for each affected class.

**Note.** Reopened because aggregate floors can pass while one class remains opaque; they do not identify the field difference needed to promote a transfer rule to the specification.

### FV-06. Later major payload admission

**Question.** Which later major versions of built-in payloads may enter typed decoding?

**Known.** `decode.rs:2460-2500` retains unsupported class and table records as opaque records. Section 20.6 gives the opaque fallback but defines no later-major field grammar or typed admission rule.

**Need.** Establish a major-version grammar and admission rule for each supported built-in payload, or state that the later major remains permanently opaque with a typed loss contract.

**Note.** Reopened because the closure converted an unknown major into a preservation decision. Complete byte retention does not establish typed compatibility or field boundaries.

### FV-07. Later minor payload suffixes

**Question.** Which fields and boundaries do later minor versions append to each built-in payload?

**Known.** The global rule permits bounded later-minor suffixes, but `decode.rs:2460-2500` can retain the complete record without identifying the suffix grammar. A bounded end does not identify the fields in an unsupported suffix.

**Need.** Define each supported minor suffix and its admission, skip, and loss behavior, or keep the complete payload opaque with an explicit typed loss.

**Note.** Reopened because the closure treats an unread suffix as opaque without establishing whether the known prefix remains admissible.

### IC-04. Quadrilateral triangulation diagonal

**Question.** Which diagonal does the neutral reader use for a quadrilateral mesh face?

**Known.** `mesh.rs:466-489` compares the two geometric diagonals and selects diagonal `0-2` on equality. Section 12.2 says to use the shorter diagonal. The local test exercises one unequal case but does not establish the source rule or the equal-diagonal tie.

**Need.** Provide an independent source rule for the diagonal calculation and tie case, then keep the derived exactness and `mesh.quad-topology-triangulated` loss contract aligned with that rule.

**Note.** Reopened because the closure fixed the neutral loss accounting but promoted one local geometric rule without an independent operand for equal diagonals.

### SW-02. Duplicate singleton metadata selection

**Question.** Which metadata record owns a singleton property or setting when the file contains more than one?

**Known.** `settings.rs:1295-1397` applies the last successfully read value and emits a duplicate warning. The specification now states the same rule, but the closure supplies no independent source authority for conflicting units, writer versions, or settings.

**Need.** Establish the source precedence rule for duplicate singleton records and retain a typed ambiguity or resolution diagnostic for conflicting values.

**Note.** Reopened as a promotion-to-spec gap. Reordering conflicting unit records changes coordinate scaling; reordering writer-version records changes version gates.

### SW-04. V1 vertex identity

**Question.** Does V1 topology identify shared vertices by source references or by trim connectivity?

**Known.** `legacy.rs:628-724` unions endpoint occurrences by loop adjacency and seam or mate connectivity, then assigns the arithmetic mean of incident curve endpoints at `legacy.rs:673-713`. The replacement for proximity matching is covered only by local constructed cases.

**Need.** Establish the source vertex-identity and position rule independently, including the behavior of distinct nearby endpoints and conflicting endpoint positions.

**Note.** Reopened because the closure promotes trim connectivity, mean position, and maximum tolerance to the format without an independent source rule.

### SW-05. V1 seam-group curve ownership

**Question.** Which model-space curve owns a V1 seam or mate relation when the paired trims carry different curve states?

**Known.** `legacy.rs:523-558` glues only a curve-bearing trim to a curve-less trim. `legacy.rs:564-570` retains the first curve within a glued root, while the specification keeps two curve-bearing trims separate. The closure provides only local constructed cases for these alternatives.

**Need.** Establish the source seam and mate ownership rule and the required behavior for conflicting curve copies.

**Note.** Reopened because the closure promotes the exact-one and both-present decisions to the specification without an independent source witness.

### SW-06. Duplicate built-in userdata extensions

**Question.** Which built-in userdata extension owns a dimension or hatch when duplicate class UUIDs occur?

**Known.** `dimensions.rs:1009-1012` and `dimensions.rs:1046-1049` select the first matching extension. `hatch.rs:256-260` does the same. `decode.rs:988-1000` and `decode.rs:1097-1107` emit a duplicate-resolution diagnostic, but the closure supplies no independent source precedence rule.

**Need.** Establish the source uniqueness or precedence rule for each extension class and retain a typed ambiguity or resolution loss for conflicting duplicates.

**Note.** Reopened as a promotion-to-spec gap. Reordering extensions with different offsets or base-point data changes the transferred presentation.

### SW-07. V1 surface rational-form arbitration

**Question.** Which V1 surface rational-form byte is authoritative when the two stored bytes differ?

**Known.** `legacy.rs:349-361` reads both bytes, rejects values above 2, and selects the last nonzero byte. Section 8.2 lists two rational-form bytes and their forms but defines no agreement or precedence rule.

**Need.** Establish the relationship between the two bytes and the mismatch policy. Preserve or reject a disagreement instead of selecting by byte order.

**Note.** A disagreement such as `[1, 2]` versus `[2, 1]` changes the rational interpretation. Reordering the stored bytes changes the neutral surface without a source discriminator.

### SW-08. Failed view-record fallback

**Question.** What happens when a view record is framed but its child payload fails typed parsing?

**Known.** `views.rs:890-892` stops a view list on a chunk error without a loss. `views.rs:895-929` converts a parse error into a synthetic view with empty identity fields and all three visibility flags set true. The warning is stored in the native view record, and `views.rs:1000-1011` publishes that record without a typed loss.

**Need.** Retain the complete failed record as opaque or omit its typed view, and emit a typed loss. Do not expose fabricated visibility or identity as source state.

**Note.** Reopened as silent substitution. A malformed or later view can become a visible default view, and later views can disappear after the first framing error.

### SW-09. Ambiguous history producers

**Question.** How is a history dependency represented when more than one record produces the same descendant UUID?

**Known.** `history.rs:1295-1310` changes a duplicate producer entry to `None`. `history.rs:1321-1328` then omits every dependency using that descendant. Only dependencies that point to later unique producers are counted at `history.rs:1312-1319` and reported at `decode.rs:5333-5338`.

**Need.** Retain the ambiguity and emit a typed history-dependency loss when a duplicate producer prevents a neutral dependency edge.

**Note.** Reopened as an ignored witness. Duplicate producer records remain in native history, but the omitted neutral edge has no ambiguity loss and is indistinguishable from an absent relation.

### SW-10. Writer first-pcurve selection

**Question.** Which parameter-space curve use does the Rhino writer serialize when one coedge has more than one ordered pcurve use?

**Known.** `writer.rs:2419-2443` allows multiple pcurve uses on one coedge. `writer.rs:2332-2338`, `writer.rs:2450-2456`, and `writer.rs:2481-2489` select only the first use for geometry, tolerance, and NURBS validation.

**Need.** Emit every supported pcurve use, or reject a coedge with more than one use before writing. Preserve its ordered range and geometry semantics.

**Note.** A second pcurve with a different range or carrier is accepted by ownership validation and then omitted from the written C2 or trim payload. The first ordered use silently wins.

### SW-11. Extrusion closure and orientation constants

**Question.** Which source rule defines extrusion profile endpoint coincidence and orientation?

**Known.** `extrusion.rs:21-24` introduces absolute and relative closure tolerances. `extrusion.rs:328-390` approximates signed area by a fixed sample count per knot span. Section 16 states an archive point-coincidence rule and an oriented-area rule but gives neither the numeric tolerance nor the sampling authority.

**Need.** Establish the source coincidence tolerance and orientation algorithm, or mark the approximation as derived and refuse cases whose classification is not proven.

**Note.** Reopened as an invented-constant and plausibility-framing finding. A small endpoint gap changes cap admission, and a high-curvature rational profile can change area classification between samples.
