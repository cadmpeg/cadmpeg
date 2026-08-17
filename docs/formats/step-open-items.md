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

## 3. Containers and other encodings

### CE-03. Part 28 XML grammar

**Question.** Which Part 28 configuration and generated schema define each supported AP203, AP214, or AP242 XML grammar?

**Known.** Part 28 is an EXPRESS-to-XML mapping. The codec refuses a Part 28 candidate before Part 21 parsing (`crates/cadmpeg-codec-step/src/codec.rs:365-374`).

**Need.** Define the supported AP schema, Part 28 configuration, generated XML schema, and independent conforming exchanges.

**Conflict.** Detection and refusal do not implement the Part 28 grammar.

### CE-04. Part 28 graph mapping

**Question.** How does each supported Part 28 XML construct map to the STEP instance graph and CADIR?

**Known.** The mapping is schema-driven. The codec has no XML entity, value, reference, or identity adapter (`crates/cadmpeg-codec-step/src/codec.rs:365-374`).

**Need.** Define the mapping configuration and conformance witnesses for entity identity, references, aggregates, and omitted values.

**Conflict.** Unsupported-input refusal provides no graph-mapping operand.

### CE-05. Part 26 binary grammar

**Question.** Which Part 26 mapping and HDF5 layout define each supported AP203, AP214, or AP242 binary exchange?

**Known.** Part 26 is an EXPRESS-driven HDF5 mapping. The codec refuses an HDF5 signature before Part 21 parsing (`crates/cadmpeg-codec-step/src/codec.rs:365-370`).

**Need.** Define the supported AP schema, mapping version, HDF5 layout, and independent conforming exchanges.

**Conflict.** Signature detection and refusal do not implement the Part 26 grammar.

### CE-06. Part 26 graph mapping

**Question.** How does each supported Part 26 construct map to the STEP instance graph and CADIR?

**Known.** The mapping uses schema and population groups, compound entity types, optional bitmaps, instance identifiers, aggregates, and links. The codec implements none of these mappings (`crates/cadmpeg-codec-step/src/codec.rs:365-370`).

**Need.** Define the mapping version and conformance witnesses for identity, references, aggregates, and optional values.

**Conflict.** Unsupported-input refusal provides no graph-mapping operand.

## 4. Signatures

## 5. Topology and pcurve decisions

## 6. Units and measures

## 7. Annotation, presentation, and tessellation

## 8. Product structure and placement

### BM-02. BO-Model composition

**Question.** How do BO-Model XML identities and values combine with a Part 21 instance graph?

**Known.** The encodings have separate identity systems and explicit external-file references (`step.md` §1 "Part 21 does not define a sidecar filename", `step.md` §1 "The BO-Model XML identity system is local"). The codec refuses BO-Model XML (`crates/cadmpeg-codec-step/src/codec.rs:365-379`).

**Need.** Define an AP242 cross-file identity relation, precedence policy, and independently paired XML and Part 21 exchanges.

**Conflict.** Refusing BO-Model XML avoids an accidental join but does not implement or settle cross-file composition.
