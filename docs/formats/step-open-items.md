# STEP Open Items

This document lists the parts of STEP exchange formats that we do not know. The specification `step.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. External resources

### ER-01. URI resolution

**Question.** Which base URI and normalization rules apply to each relative URI in a REFERENCE section or a document-reference entity?

**Known.** `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." through `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." state that a REFERENCE entry binds a local resource name to a resource URI and that an out-of-file target is an external dependency.

**Need.** We must know the rules to identify the external resource that a relative URI selects.

### ER-02. Resource access

**Question.** Which retrieval and authentication procedure applies to each external resource URI?

**Known.** `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." identifies out-of-file URI targets as external dependencies. The clear-text exchange structure does not contain an access procedure.

**Need.** We must know the procedure to obtain the selected external resource.

### ER-03. Resource composition

**Question.** How does each external resource combine with the local instance graph?

**Known.** `step.md` §5 "Instance names are unique across all DATA sections." through `step.md` §5 "Instance names are unique across all DATA sections." define identity and reference resolution inside the DATA sections. `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." through `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." define local resource bindings and external dependencies.

**Need.** We must know the composition rule to resolve cross-resource identities and build one product graph.

### ER-04. Resource cache identity

**Question.** Which URI components and resource metadata determine whether two external resource references identify the same cached resource?

**Known.** `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." and `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." state that each REFERENCE entry contains a resource URI. The specification gives no cache-identity rule.

**Need.** We must know the identity rule to reuse a retrieved resource without combining different resources.

## 2. AP242 BO-Model sidecars

### BM-01. Sidecar envelope

**Question.** What XML grammar and file relationship identify an AP242 BO-Model sidecar?

**Known.** `step.md` §1 "The STEP codec reads ISO 10303-21 clear-text exchange structures whose" through `step.md` §1 "The STEP codec reads ISO 10303-21 clear-text exchange structures whose" identify an AP242 BO-Model XML sidecar as an encoding that is distinct from the Part 21 clear-text exchange structure.

**Need.** We must know the envelope to detect, parse, and associate the sidecar with its Part 21 exchange structure.

### BM-02. Sidecar composition

**Question.** How do AP242 BO-Model XML identities and values combine with the Part 21 instance graph?

**Known.** `step.md` §5 "Instance names are unique across all DATA sections." through `step.md` §5 "Instance names are unique across all DATA sections." define identity and reference resolution inside the Part 21 DATA sections. The specification gives no cross-encoding composition rule.

**Need.** We must know the composition rule to build one product graph from the Part 21 exchange structure and its sidecar.

## 3. Containers and other encodings

### CE-01. ZIP container layout

**Question.** Which ZIP entries, names, metadata, and relationships form an edition-3 exchange container?

**Known.** `step.md` §1 "The STEP codec reads ISO 10303-21 clear-text exchange structures whose" through `step.md` §1 "The STEP codec reads ISO 10303-21 clear-text exchange structures whose" identify a ZIP container as distinct from an uncompressed Part 21 exchange structure. `step.md` §2 "An uncompressed exchange structure has this outer grammar:" defines the uncompressed outer grammar.

**Need.** We must know the layout to locate and identify each exchange resource in the container.

### CE-02. ZIP resource composition

**Question.** How do references between exchange resources in an edition-3 ZIP container resolve?

**Known.** `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." through `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." define resource names and URIs in a Part 21 REFERENCE section. The specification gives no container-relative resolution rule.

**Need.** We must know the resolution rule to combine the contained resources into one product graph.

### CE-03. Part 28 XML grammar

**Question.** What XML grammar represents an AP203, AP214, or AP242 exchange structure in Part 28?

**Known.** `step.md` §1 "The STEP codec reads ISO 10303-21 clear-text exchange structures whose" through `step.md` §1 "The STEP codec reads ISO 10303-21 clear-text exchange structures whose" define support for Part 21 clear text and identify Part 28 XML as a distinct encoding.

**Need.** We must know the grammar to parse record boundaries, values, and references from Part 28 XML.

### CE-04. Part 28 graph mapping

**Question.** How does each Part 28 XML construct map to the entity graph and invariants in `step.md`?

**Known.** `step.md` §5 "A parameter is an instance reference, integer, real, enumeration, string," through `step.md` §5 "Instance names are unique across all DATA sections." define Part 21 values, records, identities, and references. The specification gives no Part 28 mapping.

**Need.** We must know the mapping to apply schema decoding to a Part 28 exchange structure.

### CE-05. Part 26 binary grammar

**Question.** What HDF5 layout represents an AP203, AP214, or AP242 exchange structure in Part 26?

**Known.** `step.md` §1 "The STEP codec reads ISO 10303-21 clear-text exchange structures whose" through `step.md` §1 "The STEP codec reads ISO 10303-21 clear-text exchange structures whose" define support for Part 21 clear text and identify Part 26 binary or HDF5 as a distinct encoding.

**Need.** We must know the layout to parse record boundaries, values, and references from Part 26 data.

### CE-06. Part 26 graph mapping

**Question.** How does each Part 26 HDF5 construct map to the entity graph and invariants in `step.md`?

**Known.** `step.md` §5 "A parameter is an instance reference, integer, real, enumeration, string," through `step.md` §5 "Instance names are unique across all DATA sections." define Part 21 values, records, identities, and references. The specification gives no Part 26 mapping.

**Need.** We must know the mapping to apply schema decoding to a Part 26 exchange structure.

## 4. User-defined names

### UD-01. User-defined entity semantics

**Question.** What entity semantics does each user-defined `!` entity name select?

**Known.** `step.md` §3 "user_name" defines the syntax of a user-defined name. `step.md` §5 "Instance names are unique across all DATA sections." through `step.md` §5 "Instance names are unique across all DATA sections." require an unknown entity to retain its name, complete spans, and outgoing references.

**Need.** We must know the semantics to transfer a user-defined entity to typed native or neutral records.

### UD-02. User-defined type semantics

**Question.** What value semantics does each user-defined `!` type name select?

**Known.** `step.md` §3 "user_name" defines the syntax of a user-defined name. `step.md` §5 "A parameter is an instance reference, integer, real, enumeration, string," through `step.md` §5 "A parameter is an instance reference, integer, real, enumeration, string," define a typed parameter as a name with one parameter.

**Need.** We must know the semantics to decode the wrapped parameter as a typed value.

## 5. Signatures

### SG-01. Signature method selection

**Question.** Which SIGNATURE field identifies the signature method and its parameters?

**Known.** `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." through `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." define the complete byte boundary of a SIGNATURE section. The specification gives no field grammar for its content.

**Need.** We must know the selection rule to choose the correct signature verification method.

### SG-02. Signed byte sequence

**Question.** Which exact bytes does each signature method authenticate?

**Known.** `step.md` §2 "An uncompressed exchange structure has this outer grammar:" through `step.md` §2 "An uncompressed exchange structure has this outer grammar:" place the optional SIGNATURE section after all DATA sections and before the exchange terminator. `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." through `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." define the byte boundary of the SIGNATURE section.

**Need.** We must know the byte sequence to calculate the verification input.

### SG-03. Signature value encoding

**Question.** How does each signature method encode its signature value and verification material in the SIGNATURE section?

**Known.** `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." through `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." require retention of the complete SIGNATURE byte range. The specification gives no field grammar for the retained content.

**Need.** We must know the encoding to extract the signature value, keys, certificates, and method parameters.

### SG-04. Signature verification result

**Question.** Which validation conditions make each signature valid, invalid, or indeterminate?

**Known.** `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." through `step.md` §7 "ANCHOR entries bind a resource name to an in-file parameter value." define structural retention only. The specification gives no cryptographic validation conditions.

**Need.** We must know the conditions to report a signature verification result.
