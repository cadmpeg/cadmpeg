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

### ER-02. Resource access

**Question.** Which retrieval and authentication procedure applies to an external resource URI?

**Known.** Part 21 defines resource-resolution results and a UUID registry case. It does not select a transport, authentication, redirect, certificate, authorization, or freshness policy. The codec retains external references and performs no implicit access.

**Need.** We need a caller resource-provider contract before the codec can retrieve an external resource.

**Note.** QA audit: commit `7180ff02e` closed this item by recording that the codec does not access resources. This is a conservative boundary, not an implemented retrieval rule.

### ER-03. Resource composition

**Question.** How does an external resource occurrence combine with the local instance graph?

**Known.** Part 21 substitutes a referenced anchor item at one occurrence and defines transitive schema-population membership. The decoder retains the external occurrence but decodes only the supplied root exchange. It does not resolve the occurrence or compose the target graph.

**Need.** We need resource-qualified identity, schema, unit, and trust rules, plus a multi-resource witness that verifies occurrence substitution.

**Note.** QA audit: commit `c6e80f79c` closed this item by making root-only decode a CADIR decision. Refusal to import the subsidiary graph does not implement the Part 21 composition rule.

### ER-04. Resource cache identity

**Question.** Which URI components and resource metadata identify one cached external resource?

**Known.** Part 21 supplies a resource URI and can supply a last-visited timestamp and message digest. It does not define URI normalization, cache keys, validators, content negotiation, or representation equivalence. The codec has no resource cache.

**Need.** We need a caller cache contract and independent references that differ in URI spelling and freshness metadata.

**Note.** QA audit: commit `c07bf94c2` closed this item because the codec is cache-free. Absence of a cache does not settle cache identity.

## 3. Containers and other encodings

### CE-02. ZIP resource composition

**Question.** How do references between exchange resources in an edition-3 ZIP container resolve into one graph?

**Known.** Annex A.4 defines archive-relative addressing and root-anchor forwarding. The archive reader checks the addressed member but admits only `ISO-10303.p21` to semantic decode. It does not substitute an entity or value from a subsidiary graph.

**Need.** We need a multi-member archive witness and a resource-qualified graph model that verifies forwarded anchor substitution.

**Note.** QA audit: commit `2a5fd8ce8` documented root-only decode and deleted the item. Root-only refusal is not the resource-composition operand.

### CE-03. Part 28 XML grammar

**Question.** Which Part 28 configuration and generated schema define each supported AP203, AP214, or AP242 XML grammar?

**Known.** Part 28 is an EXPRESS-to-XML mapping. The current codec detects a Part 28 candidate and refuses it before Part 21 parsing. It has no Part 28 schema or decoder.

**Need.** We need the supported AP schema, Part 28 configuration, generated XML schema, and independent conforming exchanges.

**Note.** QA audit: commit `8fd1526d3` closed this item by documenting unsupported-input refusal. Detection and refusal do not decode the grammar.

### CE-04. Part 28 graph mapping

**Question.** How does each supported Part 28 XML construct map to the STEP instance graph and CADIR?

**Known.** The mapping is schema-driven. The codec has no XML entity, value, reference, or identity adapter.

**Need.** We need the exact mapping configuration and conformance witnesses for entity identity, references, aggregates, and omitted values.

**Note.** QA audit: commit `8fd1526d3` closed this item with the same unsupported-input refusal as CE-03. No graph-mapping operand executes.

### CE-05. Part 26 binary grammar

**Question.** Which Part 26 mapping and HDF5 layout define each supported AP203, AP214, or AP242 binary exchange?

**Known.** Part 26 is an EXPRESS-driven HDF5 mapping. The codec detects the HDF5 signature and refuses the input. It has no Part 26 mapping tables or HDF5 reader.

**Need.** We need the supported AP schema, mapping version, HDF5 layout, and independent conforming exchanges.

**Note.** QA audit: commit `8fd1526d3` closed this item by documenting unsupported-input refusal. Signature detection does not decode the grammar.

### CE-06. Part 26 graph mapping

**Question.** How does each supported Part 26 construct map to the STEP instance graph and CADIR?

**Known.** The mapping uses schema and population groups, compound entity types, optional bitmaps, instance identifiers, aggregates, and links. The codec implements none of these mappings.

**Need.** We need the exact mapping version and conformance witnesses for identity, references, aggregates, and optional values.

**Note.** QA audit: commit `8fd1526d3` closed this item with the same unsupported-input refusal as CE-05. No graph-mapping operand executes.

## 4. Signatures

### SG-04. Signature verification result

**Question.** Which executed checks make a retained STEP signature valid, invalid, or indeterminate?

**Known.** The parser retains the detached CMS object and signed byte range. It performs structural CMS admission but no digest, signed-attribute, signature, certificate-path, revocation, time, or trust-policy verification.

**Need.** We need a verifier interface, caller trust policy, and independently signed valid, modified, expired, revoked, and unknown-chain witnesses.

**Note.** QA audit: commit `88ccb2488` documented RFC 5652 conditions and proved that modified source bytes still retain the same structural CMS. No verification result is computed. This is closure without the verification operand.

## 5. Topology and pcurve decisions

### PC-02. Synthesized pcurve chart policy

**Question.** Which source rule authorizes an endpoint-derived pcurve chart transform on a procedural support surface?

**Known.** `reader/topology.rs:3080-3162` generates endpoint-derived parameterization variants for procedural surfaces. It samples a bounded domain and rejects some collapsed-axis results. The source pcurve does not declare this transform.

**Need.** We need producer files with declared and observed procedural-surface charts, plus an exact rule that distinguishes a real chart transform from endpoint coincidence.

**Note.** QA audit: commit `6b5b13114` closed this item by naming the synthesis a CADIR decision. The decision still fabricates an undeclared chart candidate and is not format evidence.

### TP-09. Bounded pcurve admission

**Question.** What proves endpoint fit and model-space locus equivalence for competing pcurve candidates?

**Known.** `reader/topology.rs:3067-3072` fixes seed-grid, subdivision, depth, and flatness constants. The selector admits a bounded numerical result, withholds unresolved ties, and uses STEP identity for equivalent ties. Part 42 supplies the candidate list but no global numerical selector.

**Need.** We need independent reordered, near-tied, crossing, and missed-minimum witnesses, plus exact inversion or validated interval bounds for endpoint fit and locus equivalence.

**Note.** QA audit: commit `ed6dd2432` closed this item by declaring the bounded heuristic a CADIR admission rule. Naming the limits does not prove the selected candidate or settle the verification gap.

## 6. Units and measures

## 7. Annotation, presentation, and tessellation

## 8. QA-reopened closure items

### BM-02. BO-Model composition

**Question.** How do BO-Model XML identities and values combine with a Part 21 instance graph?

**Known.** The encodings have separate identity systems and explicit external-reference constructs. The codec refuses BO-Model XML and does not join it to Part 21 by filename, UID, value, or order.

**Need.** We need an explicit AP242 cross-file identity relation, precedence policy, and independently paired XML and Part 21 exchanges.

**Note.** QA audit: commit `184b0ddbe` closed this item by retaining separate graphs and refusing XML input. No composition operand executes.
