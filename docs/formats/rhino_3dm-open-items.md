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

### LG-02. V2 class-payload compatibility

**Question.** Do all V2 class payloads use the later archive class-data grammar and version fields?

**Known.** `rhino_3dm.md` §8.2 "V2 uses the table and polymorphic class-record grammar in sections 7 through" states that all V2 class payloads use that grammar, while `chunks.rs:30-67` only establishes generic V2 chunk framing. The comparison document provides aggregate archive evidence but no class-specific V2 payload witness for this rule.

**Need.** Provide class-specific V2 evidence for the payload boundary and version fields, or narrow the specification to the classes established by evidence.

**Note.** Reopened as a promotion-to-spec gap. A V2 class can be admitted under an unverified later-archive assumption and then be falsely typed or lose unsupported fields.

### RS-01. Later-minor bounded suffixes

**Question.** Which remaining versioned readers outside sections 7.1, 13.3,
13.4, 18.3, and 20.2-20.3 accept unread fields appended before their bounded
end?

**Known.** The global rule at `rhino_3dm.md` §4.2 permits later-minor suffixes.
The producer-backed readers covered by sections 7.1, 13.3, 13.4, 18.3, and
20.2-20.3 consume their known prefixes and skip bounded suffixes. Remaining
versioned readers still have local caps or zero-tail checks, including texture
mapping, font, text-style, dimension-style, view, object-attribute, and other
presentation readers.

**Need.** For the remaining readers, identify producer-supported minor ceilings
and suffix fields, then remove unjustified rejection or document a producer-
backed writer-band ceiling.

**Note.** Narrowed 2026-08-15. `ON_Texture::Read`, `ON_Material::Read`,
`ON_Material::Internal_ReadV5`, `ON_Group::Internal_ReadV5`, `ON_Light::Read`,
`ON_Linetype::Read`, `ON_UuidList::Read`, `ON_PlaneSurface::Read`,
`ON_ClippingPlane::Read`, `ON_ClippingPlaneSurface::Read`, and
`ON_DetailView::Read` establish the settled subset. The remaining readers need
the same producer-source audit or an independent witness.

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
