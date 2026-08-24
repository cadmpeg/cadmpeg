# IGES open items

This document lists the parts of the IGES format that we do not know. The specification `iges.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

Status requirements for resource-bounded decode, valid semantic output,
complete transfer accounting, semantic writing, target selection, independent
application acceptance, and writer stress are settled in the IGES
specification and support profile. They are not open format items.

# Unrecorded format rules

The items below record decode and write rules that the codec applies and that
neither IGES nor `iges.md` states. They come from a directed sweep of the codec
on 2026-08-08. Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now, with the code that depends on it.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and
  the code. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Use the identifier in commit messages and in code comments. This document uses
ASD-STE100 Simplified Technical English. Record names, field names, and token
values are technical names. They keep their source spelling.

Many items have one shape: the codec refuses an entity because a value misses a
threshold that the codec selected, or because a field is blank where the codec
requires an explicit value. A refusal is not a safe default. It removes geometry
from a conformant file.

## Dialect coverage

### DV-01. Full IGES 4.0 dialect support

**Question.** Which version-specific physical, Global, Directory, Parameter Data, entity, and transfer rules must be implemented and verified before Global version flag `6` joins the verified set?

**Known.** `iges.md` admits a flag `6` file in salvage mode, applies the 4.0 Global and Directory rules and the 4.0 entity/form read envelope, and reports `iges/source.dialect-unverified`. Full version-specific decode and strict admission are not established.

**Need.** Compare the complete IGES 4.0 document with the settled 5.1/5.2/5.3 model, implement each material difference, and add independent probes for Global defaults, version handling, entity ranges, pointer targets, constraints, and geometry projection. Remove the dialect loss and strict refusal only after valid 4.0 files have full semantic and transfer verification.

**Conflict.** The current read profile applies the 4.0 envelope but still uses later-version semantics for 4.0 physical, Parameter Data, entity, and transfer rules that are not yet verified. That is recovery, not full 4.0 support.

**Note.** Keep 4.0 salvage available while this item is open. Closing this item requires updating `iges.md`, `docs/format-support.md`, and the decoder together.

### DV-02. Full IGES 5.0 dialect support

**Question.** Which version-specific physical, Global, Directory, Parameter Data, entity, and transfer rules must be implemented and verified before Global version flag `8` joins the verified set?

**Known.** `iges.md` admits a flag `8` file in salvage mode and applies the IGES 5.0 Global, Directory, and entity/form envelope. That envelope inherits the IGES 4.0 main entity table and adds the V5.0 ECO forms; it excludes the B-rep family held for IGES 5.1. The decoder still reports `iges/source.dialect-unverified`. `GL-12` records that the authoritative IGES 5.0 comparison is not complete. Full version-specific decode and strict admission are not established.

**Need.** Obtain an authoritative IGES 5.0 specification, compare its complete rules with the settled 5.1/5.2/5.3 model, implement each material difference, and add independent probes for Global defaults, version handling, entity ranges, pointer targets, constraints, and geometry projection. Remove the dialect loss and strict refusal only after valid 5.0 files have full semantic and transfer verification.

**Conflict.** The current read profile has an exact V5.0 envelope but no authoritative comparison proves that the shared 5.1/5.2/5.3 semantics preserve every V5.0 meaning. That is recovery, not full 5.0 support.

**Note.** Do not use a later IGES draft as a substitute for the 5.0 specification. Keep 5.0 salvage available while this item is open. Closing this item requires updating `iges.md`, `docs/format-support.md`, and the decoder together.

## 2. Global metadata

Type 406 Forms 5001 through 9999 are settled. Under [IGES 4.0 §4.3.7](https://www.govinfo.gov/content/pkg/GOVPUB-C13-7b81ba8b0f709555f162cb496aa63b3b/pdf/GOVPUB-C13-7b81ba8b0f709555f162cb496aa63b3b.pdf) and [IGES 5.3 §4.97](https://paulbourke.net/dataformats/iges/IGES.pdf), each property has `NP` at Parameter index 1, `NP` variable values, and any additional pointer groups. `parameter.rs::entity_primary_end` applies the common boundary before generic recovery. The owner tests `type406_implementor_defined_forms_use_common_count_boundary` and `type406_implementor_defined_malformed_count_or_span_suppresses_generic_recovery` cover Forms 5557, 6007, and 9999 and malformed count or span input. The neutral model has no standard meaning for these forms, so the decoder retains their complete native entity records without semantic projection.

### GL-09. Implicit defaults that the owning field cannot use

**Question.** Which normative source authorizes the decoder to reject the data-type implicit default for Global fields 9, 11, and, in the 5.1–5.3 profile, 17?

**Known.** IGES 5.3 §2.2.2.1 sets the implicit default for an Integer field to zero, §2.2.2.2 sets the implicit default for a Real field to zero, and §2.2.2.3 sets the implicit default for a String field to NULL. §2.2.3 directs postprocessors to assign the explicit or implicit default to any empty field. The 5.1–5.3 Global field disposition matrix in `iges.md` follows those rules for fields 9, 11, and 17 only through an explicit semantic fallback. The IGES 5.0 Recommended Practices Guide instead makes field 16 optional with default `1`, permits field 17 to be omitted with field 16, defines field 17 equal to zero as relative line-weight mode, and makes fields 10 and 11 conditional on the use of `D` or `d` real tokens; the V5.0 resolver now applies those rules.

**Need.** Fields 9 and 11 still need an authoritative rule that excludes zero significant digits from their implicit defaults in the 5.1–5.3 profile. Field 17 still needs that rule for the 5.1–5.3 profile, whose field definition is a required physical width; the V5.0 relative-width rule is settled separately. Zero significant digits makes the representation uncertainty of a real token as large as the token, which admits every unit-vector, orthogonality, and transformation-frame invariant that §§2.2.4.3.9 and 2.2.4.3.11 exist to bound. A zero maximum line width in the 5.1–5.3 profile makes the §2.2.4.4.12 thickness ratio zero for every entity, which no display can use.

**Conflict.** §2.2.3 directs the postprocessor to assign the implicit default and states no exception. The 5.1–5.3 matrix does not assign it for fields 9, 11, and 17. The matrix reports a loss for each departure, so the deviation is visible in the transfer report, but the deviation stands. The V5.0 field-17 zero is not a departure because its version-specific guidance assigns it a distinct relative-width meaning.

**Note.** The discriminating check is a second normative source that constrains the implicit default by field semantics: search ANSI/US PRO/IPO-100-1996, the NIST IGES 5.x test suite documentation, and the published IGES 5.3 errata for a rule that excludes a required-no-default capability or physical-width field from the §2.2.2 implicit default. A source that upholds the literal §2.2.3 rule with no exception changes the fields 9, 11, and 17 absent dispositions in the 5.1–5.3 profile to zero and removes their losses on the absent path.

### GL-12. NISTIR 4412 (IGES 5.0) could not be acquired

**Question.** Does the IGES 5.0 specification define the read envelope and the Global contract in the same terms as IGES 5.3?

**Known.** Global version flag `8` names effective version 5.0, whose specification is NISTIR 4412, "Initial Graphics Exchange Specification (IGES) Version 5.0", Reed, Harrod, and Conroy, September 1990, about 600 pages. The document could not be acquired on 2026-08-20, so version 5.0 was not compared and stays outside the verified set. Every candidate source was tried and failed. NIST's own repository has no file: the legacy path pattern that serves `nbsir88-3813.pdf` and `nistir4600.pdf` returns HTTP 404 for `nistir4412.pdf` and for the case and name variants; `doi.org/10.6028/NIST.IR.4412` returns HTTP 404 and Crossref has no record, while the sibling DOI for NISTIR 4600 resolves; the NIST publication search lists nineteen IGES records and no 5.0; and `nvlpubs.nist.gov` has no Wayback snapshot for the path at any date. archive.org holds IGES 1.0, 2.0, 3.0, and 4.0 under `initialgraphicse*` identifiers and no 5.0, and its search API returns zero results for `NISTIR 4412`. The govinfo search API returns seventy-two records for the title, including 1.0, 3.0, and 4.0, and zero for `NISTIR 4412`. NTIS/NTRL, NASA NTRS, Semantic Scholar, OpenLibrary, Harvard LibraryCloud, and the K10plus union catalogue each return no 5.0 record, and K10plus lists 3.0, 4.0, 5.1, and 5.3. Google Books holds a metadata-only record with no preview and no download. HathiTrust, DTIC, the Library of Congress JSON search, UNT/TRAIL, and Stanford SearchWorks each refused the request with an HTTP 403, a CAPTCHA, or a bot-detection page, so those are unresolved rather than negative. The NIST FIREDOC record confirms the citation and states the document is available from the National Technical Information Service, which indicates print or microfiche only.

**Need.** Version 5.0 is the version between the last version verified against a document we hold and the first version in the verified set, so it is the one whose divergences are least predictable from the 4.0 comparison. Every remaining entity and Global divergence, including `GL-10` and `VN-01` through `VN-05`, may have closed at 5.0, at 5.1, or at 5.2, and without NISTIR 4412 the decoder cannot tell which. The IGES 5.3 front matter states that version 5.0 introduced new area fill patterns, new line font patterns, new electrical and printed wire assembly properties, a perspective view capability, and the Bounded Surface Entity, so 5.0 is where several of the entity divergences plausibly resolve.

**Note.** Do not substitute another document. A 754-page "DRAFT Baseline 1/99" IGES 5.x successor draft is reachable at a vendor mirror and is not NISTIR 4412: its own change log places it after IGES 5.3, and it was never published. It is a usable machine-readable cross-check for 5.3 wording and no evidence at all about 5.0. The remaining avenue is a source that refused an automated request: a signed-in HathiTrust catalogue search would settle whether a member library digitized the document, and as a United States government publication it would then be full-view. Failing that, an interlibrary loan or an NTIS paper or microfiche order is the fallback. Until the document is read, flag `8` keeps `iges/source.dialect-unverified`, which is the designed behavior.

## 3. Directory fields, the reference graph, and the native arenas

## 4. Geometry carriers and tolerances

## 5. Surfaces and topology

### ST-16. Neutral coordinate selected for a sewn boundary vertex

**Question.** Which neutral point represents IGES boundary endpoints that are
distinct but fall within the boundary sewing tolerance?

**Known.** Type 141 and Type 142 give model-space and parameter-space boundary
curves, but they do not give the face-local vertices required by neutral
topology. `iges.md` defines a sewing tolerance from the Global minimum
resolution and coordinate significance, takes the transitive
closure of endpoint proximity, rejects a component whose diameter exceeds that
tolerance, and assigns the lexicographically smallest endpoint as the component
representative. `crates/cadmpeg-codec-iges/src/entities/trimming.rs:140-189`
implements that rule. `create_boundary_vertices` then stores the selected
endpoint as the coordinate of the one neutral point and gives the resulting
vertex the sewing tolerance.

**Need.** Sewing establishes topological identity, but it does not make one
source endpoint the geometric value declared by every curve in the component.
The lexicographic rule replaces the other endpoint coordinates and makes one
source value authoritative without an IGES rule. The answer defines a derived
neutral coordinate and its relation to all source endpoints, or preserves the
endpoint disagreement so that a consumer can distinguish a sewn vertex from an
exact shared point.

## 6. Product structure, annotation, and presentation

## 7. Write path

### WR-16. Type 126 plane and normal inferred from control points

**Question.** Which neutral declaration supplies the Type 126 planar flag and
unit normal during semantic export?

**Known.** Type 126 stores a planar flag and, for a planar curve, a unit normal.
The neutral NURBS curve does not store either value.
`crates/cadmpeg-codec-iges/src/writer.rs:4668-4702` calls
`nurbs_plane_normal`, uses the presence of its result as the planar flag, and
writes the returned normal. `crates/cadmpeg-codec-iges/src/writer.rs:4719-4784`
classifies the control points with relative constants `1e-10` and `1e-12`. For
coincident control points it writes +Z. For collinear control points it chooses
the global axis least aligned with the first usable control-polygon direction
and constructs a perpendicular normal.

**Need.** The fixed thresholds are not a neutral curve tolerance or an IGES
declaration. Coincident and collinear control points also define no unique
plane normal. The writer therefore creates source semantics that the neutral
model does not contain. The answer adds explicit planarity and normal semantics
to the neutral curve, or emits a Type 126 declaration that does not claim an
inferred unique plane.
