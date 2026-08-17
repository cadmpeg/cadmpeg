# STEP Open Items

This document lists the parts of STEP exchange formats that we do not know. The specification `step.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. External resources

### ER-03. Cross-resource composition

**Question.** How does a decoded root bind an addressed entity in a subsidiary or external STEP resource?

**Known.** A Part 21 ZIP root reference can address an anchor in a subsidiary member. An external reference retains its URI and fragment. The codec keeps subsidiary and external resources outside the root graph.

**Need.** Define the resource-qualified identity and validation needed before an addressed entity can enter CADIR. A numeric instance identifier from another resource must not become a root identity.

**Conflict.** The format supplies a resource and anchor target, but the decoder only checks internal member existence and records a note. The current policy requires caller composition without providing an executable composition operand.

**Note.** `crates/cadmpeg-codec-step/src/archive.rs:122-152` parses root references and checks `archive.entry`, while `crates/cadmpeg-codec-step/src/codec.rs:323-362` decodes only `root_view`. `step.md` §7 "For a ZIP container, Annex A.4 of the" and `step.md` §7 "For an edition-3 ZIP, the root exchange has" keep subsidiary bytes outside the root graph and assign composition to the caller. No current witness parses a subsidiary, resolves its anchor, validates schema, units, and coordinate context, and binds the target. Reopen for a composition witness or a further settled boundary.

## 3. Containers and other encodings

### CE-02. ZIP subsidiary composition

**Question.** How does the ZIP root compose an addressed subsidiary exchange into the decoded graph?

**Known.** The archive requires the exact root member `ISO-10303.p21`. Other members are subsidiaries. A root reference can name a subsidiary member and an anchor.

**Need.** Define the resource-qualified target identity, anchor resolution, validation, and graph binding for an addressed subsidiary. A member-existence check is not a composition rule.

**Conflict.** The decoder records an internal resource note after checking the central directory, but it does not read the subsidiary or import its graph. The specification leaves the composition operation to a caller.

**Note.** `crates/cadmpeg-codec-step/src/archive.rs:122-152` checks only that the member exists. `crates/cadmpeg-codec-step/src/codec.rs:323-362` passes only `root_view` to the reader and appends the note. The ZIP tests retain the subsidiary as an unparsed resource. This is a root-only refusal and does not close the requested composition operand.

### CE-03. Part 28 grammar admission

**Question.** Which Part 28 XML grammar and configuration admit an exchange for decoding?

**Known.** Part 28 requires an AP XML Schema derived from a selected EXPRESS schema edition. The marker identifies an alternate encoding but does not validate XML or select a schema.

**Need.** Define the grammar, AP schema edition, configuration, and validation result that admit a Part 28 exchange.

**Conflict.** The decoder detects a Part 28 marker and refuses it. That refusal does not answer which grammar and configuration form a valid Part 28 exchange.

**Note.** `crates/cadmpeg-codec-step/src/codec.rs:403-425` performs bounded marker detection, and `crates/cadmpeg-codec-step/src/codec.rs:371-374` returns `NotImplemented` before XML parsing. `step.md` §1 "CADIR decision: the STEP codec admits Part 21 clear text and its ZIP container" states that a caller must provide the exact configuration and generated schema. No current valid or invalid grammar witness reaches admission.

### CE-04. Part 28 schema mapping

**Question.** How do Part 28 XML elements and values map to EXPRESS entities, attributes, and references?

**Known.** An AP XML Schema is derived from an EXPRESS schema. XML prefixes, local names, filenames, and `xsi:schemaLocation` do not select the missing schema or configuration.

**Need.** Define the schema-driven mapping, identity rules, aggregate handling, and error behavior for a selected Part 28 configuration.

**Conflict.** The codec does not implement a schema-driven XML adapter and refuses the detected encoding. The refusal supplies no mapping or validation behavior.

**Note.** `crates/cadmpeg-codec-step/src/codec.rs:371-374` refuses Part 28 before a document graph is built. `step.md` §1 "CADIR decision: the STEP codec admits Part 21 clear text and its ZIP container" records the missing schema and graph-binding inputs as caller inputs. No mapping witness tests a selected schema against accepted, rejected, and ambiguous XML values.

### CE-05. Part 26 HDF5 mapping

**Question.** How does the Part 26 HDF5 representation map to EXPRESS schema data?

**Known.** Part 26 uses HDF5 and an EXPRESS schema. The mapping covers schema groups, populations, named types, datasets, row identifiers, aggregates, and reference handles, subject to the Part 26 rules.

**Need.** Define HDF5 validation, schema selection, dataset decoding, reference resolution, and malformed-data behavior.

**Conflict.** The decoder detects an HDF5 signature and refuses the input before reading HDF5 groups or EXPRESS data. A signature refusal does not define the mapping.

**Note.** `crates/cadmpeg-codec-step/src/codec.rs:384-400` checks signatures at bounded offsets, and `crates/cadmpeg-codec-step/src/codec.rs:365-370` returns `NotImplemented`. `step.md` §1 "CADIR decision: the STEP codec classifies an HDF5 signature at an allowed" lists the required caller inputs but provides no executable Part 26 decode witness. Reopen this item.

### CE-06. Part 26 graph binding

**Question.** How does Part 26 data bind to a Part 21 graph when both resources describe one exchange?

**Known.** Part 26 and Part 21 use separate encodings and require schema-specific identity and mapping rules. The codec does not compose them.

**Need.** Define the resource identity, graph-binding operands, conflict policy, and retention rules for a composed Part 26 and Part 21 result.

**Conflict.** The decoder refuses Part 26 and never compares or binds its graph to Part 21. The current caller-boundary statement does not supply a composition witness.

**Note.** `crates/cadmpeg-codec-step/src/codec.rs:365-370` refuses HDF5 before graph construction. `step.md` §1 "CADIR decision: the STEP codec classifies an HDF5 signature at an allowed" explicitly leaves graph binding to the caller. No witness proves that identities, references, units, or conflicts remain resource-scoped during composition.

## 4. Signatures

### SG-04. Signature verification result

**Question.** How does a verifier produce the required `valid`, `invalid`, or `indeterminate` result for a Part 21 signature?

**Known.** A signature contains detached CMS `SignedData` and authenticates the Table 1 alphabet projection of its signed source range. Verification needs the CMS bytes, detached content, and caller trust policy.

**Need.** Define the verification execution, signer selection, cryptographic failure behavior, missing-evidence behavior, and result retention.

**Conflict.** The parser structurally admits CMS and retains it, but no codec verifier computes the digest, checks signed attributes, verifies the signature, or applies trust policy.

**Note.** `crates/cadmpeg-codec-step/src/signature.rs:227-281` validates only the CMS envelope and detached form. `crates/cadmpeg-codec-step/src/parse.rs:124-151` labels the retained value as non-cryptographic, and `crates/cadmpeg-codec-step/src/reader/tests.rs:229-245` asserts opaque retention without a verification result. `step.md` §7 "At the CADIR boundary, one verifier input is the tuple" defines the verifier tuple and result but supplies no valid, invalid, or indeterminate execution witness. Reopen this item.

## 5. Topology and pcurve decisions

### TP-09. Global pcurve association

**Question.** Is a finite endpoint witness sufficient to select a non-seam pcurve and admit its edge relation?

**Known.** A non-seam edge with one same-surface candidate has no source selector. The reader evaluates declared endpoints or performs a bounded search and admits the relation when the returned endpoint residual is within tolerance.

**Need.** Define whether endpoint coincidence is the complete admission invariant or whether the selected pcurve must also prove global model-space locus equivalence and orientation.

**Conflict.** The implementation admits a relation from a finite witness, while a finite search cannot prove a global minimum or complete locus equivalence.

**Note.** `crates/cadmpeg-codec-step/src/reader/topology.rs:3061-3175` uses one candidate, 64 grid divisions, bounded Newton steps, and endpoint residuals; the comments state that the result is not a global minimum. `step.md` §8 "CADIR decision: a typed `SEAM_EDGE` uses its explicit pcurve only when" records the same limitation as an existential witness. A hostile curve with a second lower-residual basin or a divergent interior locus can pass its endpoints. Reopen the admission invariant.

## 6. Units and measures

## 7. Annotation, presentation, and tessellation

## 8. Product structure and placement
