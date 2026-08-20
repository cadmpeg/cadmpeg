# Siemens NX `.prt` Open Items

This document lists the parts of the Siemens NX `.prt` format that we do not know. The specification `siemens_nx.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Parasolid streams

### PS-01. Unmatched compact tombstones

**Question.** What target relation does a compact tombstone use when its explicit `(type, xmt)` key does not match a partition entity?

**Known.** `siemens_nx.md` §4.2 "Tombstones form descending contiguous xmt runs" and `siemens_nx.md` §7.2 "A compact deltas tombstone is" define exact-key deletion and chronological replacement in the final BODY revision. The current-revision tombstone run can span topology, geometry, and attribute record types.

**Need.** We must know the target relation to apply an unmatched tombstone without deleting the wrong entity.

### PS-02. Terminal procedural-intersection branches

**Question.** How does a terminal type-38 or `0x5a` procedural intersection select a finite branch when it has no usable support-parameter lane or paired FIN-carried `SP_CURVE` witnesses?

**Known.** `siemens_nx.md` §6.3 "NX stores freeform edges and blend rails as construction relations with branch witnesses." `siemens_nx.md` §6.3 "The chart/start-term/end-term witness slots" and `siemens_nx.md` §6.3 "A FIN-carried SP_CURVE is a serialized terminal-branch witness" define procedural-intersection framing, support relations, endpoint witnesses, and accepted exact branches.

**Need.** We must know the selection rule to construct the terminal curve as exact geometry.

### PS-03. Coincident terminal parameters

**Question.** Which branch does a terminal procedural intersection use when distinct endpoints map to one procedural-curve parameter?

**Known.** `siemens_nx.md` §6.3 "The chart/start-term/end-term witness slots" and `siemens_nx.md` §6.3 "A FIN-carried SP_CURVE is a serialized terminal-branch witness" define endpoint witnesses and ordered procedural-curve branches. They do not define this folded terminal case.

**Need.** We must know the branch to preserve both endpoints and the curve orientation.

### PS-04. Degenerate procedural-support correction

**Question.** How does correction select terminal folds and multiple corrected branches for a degenerate support-0 array or a sentinel-truncated marker-4 plane-support array?

**Known.** `siemens_nx.md` §6.3 "When independent support inversion still leaves a procedural support lane incomplete" and `siemens_nx.md` §6.3 "Each CHART_s point first maps independently through one support carrier's inverse parameterization." define predictor inversion and correction for complete supported procedural curves.

**Need.** We must know the correction rule to construct these curves without an arbitrary branch choice.

### PS-05. NURBS-offset spine support identity

**Question.** Which saved carrier owns a NURBS-offset spine when its graph-only offset support has no established saved-carrier identity?

**Known.** `siemens_nx.md` §6.5 "A BLEND_SURF used by a FACE transfers as a procedural surface carrier." `siemens_nx.md` §6.5 "`values[0:2]` are nonzero signed support offsets `range[2]` in meters." and `siemens_nx.md` §6.5 "For an offset-intersection spine, each complete spine-side pcurve maps the spine parameter directly" define procedural support graphs, blend spines, and saved-carrier relations.

**Need.** We must know the identity relation to evaluate and transfer the spine as exact geometry.

### PS-06. Other deltas node families

**Question.** What complete record grammar and semantics does each deltas node family that the specification does not define use?

**Known.** `siemens_nx.md` §4.2 "A deltas stream is a schema-framed incremental edit log paired with a partition." and `siemens_nx.md` §4.2 "After complete record frames, compact tombstones, complete BODY revision" define the admitted deltas record families and their byte boundaries. A remaining byte region has no typed node grammar.

**Need.** We must know each grammar to delimit and transfer the remaining deltas records as typed data.

### PS-07. Deltas transmit-header XMT identities

**Question.** What is the semantic role of each of the two consecutive XMT identities in a deltas transmit header?

**Known.** `siemens_nx.md` §4.2 "A deltas transmit header is" defines the header grammar and requires two non-null consecutive identities.

**Need.** We must know the roles to relate the header identities to the deltas schema and body history.

### PS-08. Deltas BODY state tail

**Question.** What fields and semantics does the bounded state tail after the eight-reference deltas `BODY` prefix contain?

**Known.** `siemens_nx.md` §7.2 "BODY (`00 0c`) records delimit body revisions." and `siemens_nx.md` §4.2 "The state tail of a validated BODY revision begins immediately after its" define the prefix, tail boundary, revision counter, and current-revision rule.

**Need.** We must know the tail fields to transfer the complete body revision state.

### PS-10. Deltas type 45

**Question.** What is the canonical node-type name of deltas type 45 (`002d`), and what does each value mean?

**Known.** `siemens_nx.md` §4.2 "Type 45 has the complete deltas record" defines its count-selected binary64 grammar and record boundary.

**Need.** We must know the name and value roles to transfer the record as typed Parasolid state.

### PS-11. Deltas type 70

**Question.** What is the canonical node-type name of deltas type 70 (`0046`), and what does each field mean?

**Known.** `siemens_nx.md` §4.2 "Type 70 has the complete deltas record" defines its XMT identity, node ID, reference lanes, count, constants, and boundary.

**Need.** We must know the name and field roles to relate the record to its owner and referenced entities.

### PS-12. `ATTDEF_LIST` sentinel reference

**Question.** What is the semantic role of the leading sentinel reference in deltas type 74 `ATTDEF_LIST`?

**Known.** `siemens_nx.md` §4.2 "Type 74 `ATTDEF_LIST` has the complete deltas record" defines the record, active-count rule, sentinel value, slots, and boundary.

**Need.** We must know the role to represent the complete attribute-definition list without an untyped reference.

### PS-13. Deltas type 90 `GROUP`

**Question.** What does each of the five references and the `02|04` mode in deltas type 90 `GROUP` mean?

**Known.** `siemens_nx.md` §4.2 "Type 90 `GROUP` has the complete deltas record" defines the complete record grammar, reference statuses, mode values, and boundary.

**Need.** We must know the roles to construct group membership and ownership relations.

### PS-14. Deltas type 91

**Question.** What is the canonical node-type name of deltas type 91 (`005b`), and what does each field mean?

**Known.** `siemens_nx.md` §4.2 "Type-91 records are" defines its XMT identity, binary flag, six status-framed references, and boundary.

**Need.** We must know the name and field roles to transfer the record as typed Parasolid state.

### PS-15. Deltas type 101

**Question.** What is the canonical node-type name of deltas type 101 (`0065`), and what does each field mean?

**Known.** `siemens_nx.md` §4.2 "A complete type-101 record whose start lies inside a fixed-record candidate and" defines its boundary precedence. `siemens_nx.md` §4.2 "Type 101 has the complete deltas record" defines its complete field grammar.

**Need.** We must know the name and field roles to transfer the record as typed Parasolid state.

### PS-16. Deltas type 141

**Question.** What is the canonical node-type name of deltas type 141 (`008d`), and what is the role of each reference field?

**Known.** `siemens_nx.md` §4.2 "Type 141 has the complete deltas record" defines the four-reference status-framed grammar and boundary.

**Need.** We must know the name and reference roles to relate the record to its owner and operands.

### PS-17. `term_use` numeric tail

**Question.** What does each binary64 value in the count-selected deltas `term_use` tail mean?

**Known.** `siemens_nx.md` §4.2 "Direct and escaped type-40 `CHART_s`, type-41 `term_use`, type-59 blend-bound, and type-204 support-UV records use the layouts in section 6.3." defines the tail start, count-to-cardinality rule, finite values, and independent byte identity.

**Need.** We must know the value roles to transfer the complete terminal-use state.

### PS-18. Deltas tagged-reference lanes

**Question.** Which record owns each deltas tagged-reference lane, and what does each field mean?

**Known.** `siemens_nx.md` §4.2 "After complete record frames, compact tombstones, complete BODY revision" defines the tagged-reference lane grammar, record-kind values, and exact byte identity.

**Need.** We must know ownership and field roles to attach each lane to the correct typed record.

### PS-19. Deltas reference/type maps

**Question.** Which record owns each deltas reference/type map, and what does each entry mean?

**Known.** `siemens_nx.md` §4.2 "A reference/type map begins with either" defines the counted reference and type lanes and their byte boundary.

**Need.** We must know ownership and entry roles to apply the map to the correct entities.

### PS-20. Deltas four-reference state packets

**Question.** Which record owns each deltas four-reference state packet, and what is the role of each reference?

**Known.** `siemens_nx.md` §4.2 "One reference-state packet is" defines its reference-status grammar and exact boundary.

**Need.** We must know ownership and reference roles to attach the packet to typed state.

### PS-21. Deltas schema reference preambles

**Question.** Which declaration owns each deltas schema reference preamble, and what is the role of each field?

**Known.** `siemens_nx.md` §4.2 "A schema reference preamble is" defines the preamble variants, reference lanes, state bytes, and boundary.

**Need.** We must know ownership and field roles to construct the complete inline schema model.

### PS-22. Deltas reference-marker packets

**Question.** Which record owns each deltas reference-marker packet, and what do its references and marker mean?

**Known.** `siemens_nx.md` §4.2 "One reference-marker packet is" defines the packet grammar, marker values, and boundary.

**Need.** We must know ownership and marker semantics to attach the packet to typed state.

### PS-23. Deltas type-150 state packets

**Question.** What does each field and marker in a deltas type-150 state packet mean?

**Known.** `siemens_nx.md` §4.2 "A type-150 state packet is" defines the packet grammar, marker values, and boundary.

**Need.** We must know the roles to transfer the packet as typed type-150 state.

### PS-24. Inline deltas schema declarations

**Question.** What is the semantic role of each field in an inline deltas schema declaration?

**Known.** `siemens_nx.md` §4.2 "An inline schema declaration begins with one of eight exact headers." defines the declaration variants, names, signatures, references, states, and byte boundaries.

**Need.** We must know the field roles to construct a typed schema declaration instead of a framed declaration record.

### PS-25. Inline type-12 `BODY` instance state

**Question.** What does each field in inline type-12 `BODY` instance state mean, and how do its counts constrain its lanes?

**Known.** `siemens_nx.md` §4.2 "A type-12 `BODY` header binds the immediately following instance state." defines its reference lanes, count fields, scalar lanes, state bytes, and boundary.

**Need.** We must know the roles and cardinality rules to transfer the complete inline body state.

### PS-09. Delta tag `0x5a` name

**Question.** What is the canonical later-schema node-type name for delta tag `0x5a`?

**Known.** `siemens_nx.md` §4.1 "Type 38 is the XT `INTERSECTION` node." and `siemens_nx.md` §4.2 "Status-framed type-38 `INTERSECTION` records end after their six construction references" define tag `0x5a` as the `intersection_data` layout shared with type 38.

**Need.** We must know the name to give the node one stable schema identity.

**Note.** The closure added `INTERSECTION_DATA` as the canonical name and introduced the exact schema header used to recognize it. The tests construct that header from the same implementation constant. The global schema-anchor flag then authorizes later `0x5a` bytes without proving that they remain in the same schema scope. An independent serialized record is required before the name and anchor scope are settled.

### PS-26. Boundary pcurve completion without a chart

**Question.** Which serialized witness establishes the boundary pcurve of an EDGE whose supports carry no chart and no analytic isocurve relation?

**Known.** `siemens_nx.md` §5.3 "An EDGE may carry null curve reference `1` with a finite tolerance. With a null" defines chart-certified transfer for a null carrier and states that transfer does not synthesize a model-space line between the vertices. `siemens_nx.md` §5.3 "A null `EDGE.curve` may instead have a non-null owning `FIN.curve`. The FIN" defines the FIN-carrier case. Neither defines a boundary pcurve for a plane or quadric support that has no chart.

**Need.** We must know the witness to construct the boundary pcurve from the file. Endpoint inversion alone fixes only the two ends, so the interior of the interval carries no evidence, and a straight parameter-space chart on a plane asserts a straight model-space edge.

**Note.** The implementation now derives an affine candidate and checks it at carrier and support breakpoints. The candidate is not a serialized witness, and the test cases build the carrier and surface directly in IR. Breakpoint agreement does not establish the complete interior rule for an arbitrary serialized boundary. The closure is therefore a conservative gate, not evidence of the NX boundary-pcurve rule.

### PS-27. Unresolved EDGE end vertex

**Question.** What is the correct reading of an EDGE whose end vertex does not resolve to a decoded POINT?

**Known.** `siemens_nx.md` §5.3 "An EDGE belongs to the assembled B-rep only when a FIN in a fully resolved owned LOOP" and `siemens_nx.md` §5.3 "POINT is a geometric carrier. It becomes a topological vertex only through a validated `FIN.ver" define endpoint incidence through the FIN chain and the POINT-to-vertex condition. They do not define the case in which the resolved end vertex has no decoded POINT.

**Need.** We must know the reading to separate a closed edge from an edge that lost one endpoint. Without that rule, the decoder retains neither the edge nor its dependent loop when the end vertex has no decoded POINT.

**Note.** The closure changes the decoder to drop unresolved edges except for one explicit closed-null-FIN case. The regression input is hand-built and malformed; it proves that the new path withholds an ambiguous edge, not that NX defines omission as the serialized rule. A valid implicit or omitted endpoint would be discarded by the current policy.

### PS-28. Compact tombstone boundary condition

**Question.** Which condition ends a compact tombstone, and does a following byte pair constrain it?

**Known.** `siemens_nx.md` §4.2 "**Tombstone:** a compact 6-byte deletion begins with `type:u16 BE`. A short XMT identity occupies" defines the tombstone as a self-delimiting six-byte form with two identity encodings. `siemens_nx.md` §4.2 "Tombstones form descending contiguous xmt runs that can span topology, geometry, attribute, intersection-auxiliary," defines the runs. Neither states a condition on the bytes after a tombstone.

**Need.** We must know the boundary condition to admit every deletion. The deltas walk resynchronizes byte by byte, so it needs some end condition, and it currently requires the following two bytes to decode as a known node kind. A deletion whose successor bytes open a family that this condition does not name is discarded, the entity survives the merge, and no loss records the discard.

**Note.** The closure removes the successor check and relies on the six-byte form. Its opaque-suffix regression is synthetic and does not show that the six-byte pattern cannot occur inside another bounded payload or that every such occurrence is a deletion. The fixed-length interpretation remains unverified against an independent stream.

### PS-29. Interleaved body revision sequences

**Question.** How does a deltas stream that holds more than one body sequence select the current revision of each body?

**Known.** `siemens_nx.md` §7.2 "BODY (`00 0c`) records delimit body revisions. The record prefix is" defines the revision prefix, the monotonic `node_id` counter, and the rule that the final validated BODY envelope begins the current revision. `siemens_nx.md` §9.2 "A deltas-stream BODY record with type `00 0c` and xmt `3` delimits a body" states that a `node_id` reset begins another interleaved body sequence.

**Need.** We must reconcile the two current-revision rules, or state that paired partition and deltas streams hold exactly one body sequence. The §7.2 last-envelope rule merges every other sequence's current-revision records as historical.

**Conflict.** The two rules disagree when a stream holds interleaved sequences. The §7.2 rule takes the last envelope in the stream, which belongs to one sequence, so the current-revision records of every other sequence fall before that offset and merge as historical. The decoder applies the §7.2 rule and never reads `node_id` for sequencing. The §9.2 `xmt == 3` delimiter is also not enforced. We must reconcile the two sections, or state that paired partition and deltas streams hold exactly one body sequence.

**Note.** The closure selects monotonic runs with `revision_direction`, which chooses the direction with fewer violations and uses the opposite transitions as resets. This is an order heuristic with no serialized sequence identity. An interleaved sequence such as alternating body revisions can be grouped as one run and lose a current revision. The synthetic tests do not establish the ownership rule.

### PS-30. Fixed-record field shift selection

**Question.** Which field establishes the escape and large-index shift of a fixed analytic record, and what bounds a record that the ownership graph does not own?

**Known.** `siemens_nx.md` §4.1 "Lengths are logical, before escape/large-index shifts. Each code is a Parasolid XT node ty" defines the tags and the record lengths before escape and large-index shifts. `siemens_nx.md` §6 "All geometric doubles are finite binary64 values in meters" states that the format imposes no model-magnitude bound.

**Need.** We must know the shift field so that recovery does not need a magnitude test. A model larger than one kilometer loses its recovered carriers, and an unrelated byte run can enter the model as an analytic carrier.

**Conflict.** The decoder recovers records that the ownership graph does not own by trying six field shifts in order and accepting the first whose payload passes a magnitude test. `crates/cadmpeg-codec-nx/src/geometry.rs` rejects a coordinate at or above `1.0e3` meters and a radius outside `1.0e-9` to `1.0e3`. Those bounds contradict §6. A model larger than one kilometer loses its recovered carriers, and an unrelated byte run inside a payload can pass the test and enter the model as an analytic carrier. Recovered carriers are not separable from graph-resolved carriers in the model, and an unreferenced recovered surface or curve is never removed. We must know the shift field so that recovery does not need a magnitude test.

**Note.** The closure replaces the shift scan with direct and escaped frame candidates and a parser-derived boundary check. No serialized field establishes the choice, and the successor boundary is itself recognized by the same candidate parser. If both candidates end at recognized tags, or neither does because of a trailer, a valid record is omitted; a false recognized successor can select the wrong reading.

## 2. Object model and body composition

### OM-01. Per-class OM field serialization

**Question.** What byte grammar and semantic role does each declared field of each NX OM class use?

**Known.** `siemens_nx.md` §7.1 "UG_PART begins with a 12-byte row table" and `siemens_nx.md` §7.1 "A feature-history operation record begins at the fixed operation-header marker" define OM section boundaries, class and member declarations, store identities, compact indices, and expression records. `siemens_nx.md` §3.3 "A numeric expression table contains a `hostglobalvariables` root entity." defines typed fields for selected construction families.

**Need.** We must know the remaining class grammars to decode feature history, constraints, attributes, and material bindings as typed fields.

### OM-02. `SKETCH` scalar-pair geometry

**Question.** What geometric quantities and coordinate spaces do the framed scalar pairs in a `SKETCH` construction payload represent?

**Known.** `siemens_nx.md` §2 "An operation label equal to `SKETCH` denotes a sketch history operation." `siemens_nx.md` §7.1 "A sketch payload scalar field is", `siemens_nx.md` §7.1 "A sketch repeated-type scalar pair is", and `siemens_nx.md` §7.1 "A sketch fixed pair has one of four exact forms:" define sketch record identity and the framed payload lanes but do not assign a model-space frame, sketch entity, or constraint role from equal scalar values.

**Need.** We must know the roles to construct neutral sketch geometry and constraints.

**Note.** A complete coordinate-pair record is retained as one native sketch
geometry record by its operation and pair identity when the sketch has no
stronger typed sketch entity graph. The retention does not assign a point,
curve, constraint, unit, coordinate frame, or profile role; the neutral sketch
placement remains unresolved. A separate named-point graph does not merge with
coordinate-pair records without an explicit pair-to-entity relation.

### OM-03. `DATUM_PLANE` scalar-pair geometry

**Question.** What geometric quantities and coordinate spaces do the framed scalar pairs in a `DATUM_PLANE` construction payload represent?

**Known.** `siemens_nx.md` §7.1 "A `DATUM_PLANE` payload begins" and `siemens_nx.md` §7.1 "A datum-plane object scalar-pair frame is" and `siemens_nx.md` §7.1 "A datum-plane descriptor block is exactly 40 bytes:" define datum-plane branches, resolved blocks, scalar-pair framing, descriptors, and feature identity.

**Need.** We must know one complete relation that assigns the framed scalar
pairs and descriptor or object records to a model-space origin, unit normal,
and in-plane axis.

**Conflict.** The branch indices select descriptor and object blocks but do not
assign plane-frame roles. Descriptor blocks carry an identity, schema index,
and label; scalar-pair frames carry two values. Equal descriptor identities
between `DATUM_PLANE` and `DATUM_CSYS` records do not assign either operation
as the source of an origin, axis, or normal.

**Note.** A `DatumPlane` definition requires one unique finite origin, normal,
and in-plane axis with the frame invariants established. Until that relation is
serialized, the operation remains `DatumPlaneUnresolved` while every admitted
branch, block, descriptor, and scalar-pair record remains native data.

### OM-04. `DATUM_CSYS` scalar-pair geometry

**Question.** What geometric quantities and coordinate spaces do the framed scalar pairs in a `DATUM_CSYS` construction payload represent?

**Known.** `siemens_nx.md` §7.1 "A `DATUM_CSYS` payload begins" and `siemens_nx.md` §7.1 "An object-payload scalar-pair frame is" define the eight-reference construction lane, logical payload, scalar-pair framing, and feature identity.

**Need.** We must know one complete relation that assigns the payload scalar
fields and pair records to a model-space origin and three orthonormal axes.

**Conflict.** The eight construction references define ordered block identity,
and the first two blocks define one logical payload. Scalar fields, scalar
pairs, fixed pairs, and descriptor lanes have separate frames and no serialized
role relation. Column-row reuse and equal descriptor identity establish block
or history relations only; they do not assign an origin or axis.

**Note.** A coordinate-system definition requires one unique finite origin and
three orthonormal model-space axes with their handedness established. Until
those roles are serialized, the operation remains
`DatumCoordinateSystemUnresolved` while all complete lanes remain native data.

### OM-05. OM declaration trailing code

**Question.** What does the trailing byte in an OM class or member declaration mean?

**Known.** `siemens_nx.md` §7.1 "The first record at `oid_end` begins" defines the declaration length, `UGS::` or `m_` name, trailing-code boundary, and following registry suffix.

**Need.** We must know the code domain and the relation between the trailing
byte and the declared class or member semantics.

**Conflict.** The declaration grammar identifies the trailing byte as the
single byte after the printable `UGS::` or `m_` name and before the bounded
registry suffix. The name and suffix delimit the declaration but do not assign
a type, ownership, visibility, or field-role meaning to that byte.

**Note.** The trailing byte remains an exact declaration field for classes and
members. No semantic classification is assigned until its code domain and
owner relation are serialized.

### OM-06. OM registry suffix fields

**Question.** What does each byte in a bounded OM field-registry suffix mean?

**Known.** `siemens_nx.md` §7.1 "The first record at `oid_end` begins" defines the suffix boundary and the 11-to-14-byte prefix, fingerprint, and terminal-byte decomposition.

**Need.** We must know the semantic field roles represented by the layout
prefix, schema fingerprint, and terminal byte.

**Conflict.** The 11–14-byte suffix decomposes into a 2–5-byte prefix,
eight-byte fingerprint, and one terminal byte for both class and member
declarations. The decomposition does not assign a member type, cardinality,
ownership, or access role.

**Note.** The decoder retains the raw suffix and its exact three components for
each declaration. No OM field schema is constructed from those bytes until a
semantic role relation is serialized.

### OM-07. Offset-store body to segment-image relation

**Question.** How does a primary feature body field that resolves to an offset-store block identify a segment body-image object-index pair?

**Known.** `siemens_nx.md` §2 "A partition or plain cached-body wrapper word begins" and `siemens_nx.md` §2 "A primary feature body field in the object namespace reuses a segment body" define segment body-image tuples and prohibit a relation based only on equal integer values across namespaces. They also define primary-body fields and body selection.

**Need.** We must know the cross-store relation to attach the feature output and lineage to the correct body image.

**Note.** `crates/cadmpeg-codec-nx/src/native/features.rs:3686-3746` requires one offset store, one data-block use, and one unique segment alias, while `native/segments.rs:137-173` supplies the alias-equality join. The closure test constructs `FeatureBodySegmentUse` inputs and verifies uniqueness; it does not provide an NX record that makes an offset-store block and a segment alias the same object. The integer equality remains a promoted cross-store relation, so this item is reopened.

### OM-08. Other feature-history object relations

**Question.** What relation does each feature-history object index that is not a primary-body writer or Boolean tool use?

**Known.** `siemens_nx.md` §7.1 "A nested operation object-relation frame is" defines the exact nested frame, canonical endpoint encoding, ordered endpoint retention, and source offsets. The native decoder retains these frames as `feature_operation_object_relations` without assigning endpoint roles. `siemens_nx.md` §7.1 "A direct operation tagged-reference field is" defines the exact direct `0x17` field, canonical object-index encoding, optional unique offset-store target, and source offsets. The native decoder retains these fields as `feature_operation_tagged_references` without assigning endpoint roles. `siemens_nx.md` §7.1 "A direct operation data-block reference field is" defines the exact direct `0x03` field, canonical object-index encoding, optional unique offset-store target, and source offsets. The native decoder retains these fields as `feature_operation_data_block_references` without assigning endpoint roles. `siemens_nx.md` §2 "Within a feature-history record area, an operation header is encoded as the" and `siemens_nx.md` §2 "Input bindings from two or more distinct operation headers form an identity" and `siemens_nx.md` §2 "A body-affecting operation record contains exactly one primary-body field" define operation-header inputs, shared-block identity groups, primary-body lineage, and Boolean operands.

**Need.** We must map each retained nested frame to its owning feature relation before constructing feature dependencies or selections. The link tag and endpoint identities alone do not establish a body, operand, input, or output role.

**Conflict.** Nested relation tags and direct reference tags retain endpoint
identities and serialized order, but no endpoint owner or semantic role. Shared
operation-header input blocks establish operation identity groups only; they do
not join an unowned endpoint to a body, tool, input, or output.

**Note.** The decoder retains every complete object-relation and direct
reference frame as native data. No feature dependency or selection is emitted
until one unique endpoint-role relation is serialized.

### OM-10. Operation suppression fields

**Question.** How do the embedded operation state lanes encode suppression?

**Known.** `siemens_nx.md` §2 "Every feature producing a body in the selected current B-rep is active in the" derives active state for the closed output-and-dependency graph and leaves suppression outside that graph unresolved. `siemens_nx.md` §7.1 assigns the common-frame state bytes to legacy module inactivity, Parasolid-data mutation, split tracking, and group count. The saved-toggle stream carries independent toggle identities and states. The Parasolid `UGS/ObjectState` class is a one-field code-3 character attribute whose value is an ordinary type-84 string; its owner, when resolved, is a topology attribute-list relation. The OM registry classes `UGS::OM::ObjectStateCollection` and `UGS::OM::ObjectState` are distinct declarations.

**Need.** We must know the serialized suppression fields to construct operation state for all configurations.

**Conflict.** Neither the common-frame state lane nor the saved-toggle stream identifies a feature suppression value. The OM declarations do not provide a relation from an operation feature identity to an active state object. The Parasolid `UGS/ObjectState` lane provides a character value and, when present, a topology owner, but no operation owner or suppression domain.

**Note.** A suppression assignment requires a unique relation from an operation feature identity to a serialized state object and a second relation from that object to a typed state value. A common-frame field, toggle entry, OM declaration, or Parasolid topology attribute without both joins does not assign `suppressed`; operations outside a proven active closure remain unresolved.

### OM-11. `DELETE` nullable-reference roles

**Question.** What object family can each of the five leading nullable references in a `DELETE` payload address, and what is the role of each slot?

**Known.** `siemens_nx.md` §2 "`DELETE` is a body-deletion operation only when its bounded record contains the" and `siemens_nx.md` §2 "A `DELETE` payload begins with one nullable reference field" define the five-slot field, canonical reference encodings, resolution rule, logical payload, and body-target exclusion.

**Need.** We must know the slot roles to decode the delete construction independently of its primary-body target.

**Conflict.** Each slot carries only a nullable canonical object index. Unique
offset-store resolution identifies a block, but the slot has no discriminator,
schema tag, or independent relation that assigns an object family or semantic
role. Slot order and five-block concatenation preserve construction order only;
the separate primary-body field does not label any of these slots.

**Note.** The five slots and any complete logical payload remain native
records. A slot-specific neutral projection requires one independent relation
for its object family, role, and payload schema; the body-deletion projection
does not supply that relation.

### OM-12. Inactive-arrangement body state

**Question.** Which bodies belong to each inactive arrangement, and what per-body state does that arrangement select?

**Known.** `siemens_nx.md` §2 "`/Root/part/arrangements` has an `Arrangements` root." and `siemens_nx.md` §2 "A unique part-owned `NX_Arrangement` string attribute names the active" define arrangement identity and active body membership. Other arrangements have no body membership without a separate relation.

**Need.** We must know the relation to construct inactive configuration body sets.

**Conflict.** The arrangement XML contains only configuration name, default
flag, and order. The current B-rep body census has no arrangement key, and the
active attribute join identifies only the selected configuration. Feature
closure also describes the current body set, not an alternate arrangement's
membership or per-body state.

**Note.** Inactive configurations retain their identities and unresolved body
sets. The active body set must not be copied to an inactive configuration
without a body-membership and state relation owned by that configuration.

### OM-13. Inactive-arrangement parameter state

**Question.** Which parameter values does each inactive arrangement select?

**Known.** `siemens_nx.md` §2 "When exactly one active configuration has complete body membership, the same" and `siemens_nx.md` §2 "The same active configuration retains the complete current parameter state when" define complete parameter state only for a uniquely resolved active configuration.

**Need.** We must know the relation to construct inactive configuration parameter maps.

**Conflict.** Neutral parameter identities, evaluated values, ownership scopes,
and dependencies contain no arrangement identity or per-arrangement override
selector. The complete parameter map is derived only for the unique active
configuration after the global parameter graph passes its ownership and order
checks.

**Note.** Inactive configurations retain empty unresolved parameter maps. The
active map must not be copied to another configuration without a
configuration-owned value or override relation.

### OM-14. Operation terminal discriminators

**Question.** What does each type index, flag, and trailing index in an operation terminal discriminator lane mean?

**Known.** `siemens_nx.md` §7.1 "A bounded operation payload's terminal common-frame suffix is" defines the exact terminal discriminator lanes for the admitted operation payloads and retains their serialized order.

**Need.** We must know the field roles to construct termination, direction, draft, and other operation controls.

**Conflict.** The terminal lane supplies two compact indices, four flag bytes,
and an ordered trailing-index lane. No serialized type definition, operation
control declaration, or unique relation assigns those fields to a termination,
direction, draft, or other semantic control. A link to an immediately
preceding common frame establishes byte ownership only; it does not assign
field roles.

**Note.** The complete terminal discriminator remains native data with its
exact indices, flags, order, and offsets. A neutral control requires a unique
field-role relation and a decoded value domain for that operation family.

### OM-15. `CPROJ` construction-reference roles

**Question.** Which `CPROJ` construction references select the source curve, target surface, direction, and combination controls?

**Known.** `siemens_nx.md` §7.1 "A `CPROJ` payload contains at most one construction-reference field framed as" defines the ordered three-reference graph, block resolution, and logical payload.

**Need.** We must know the roles to construct a neutral projected curve.

**Conflict.** The three references use one canonical encoding and resolve only
to offset-store blocks. The fixed middle and suffix markers delimit the field,
but do not identify a source curve, target surface, direction, or combination
control. Reconstructed strings remain payload-owned and have no independent
role relation.

**Note.** Preserve the ordered references, logical payload, and strings as
native projected-curve data. A neutral source or target requires a unique
object-family and field-role relation.

### OM-16. `CPROJ_CMB` construction-reference roles

**Question.** Which `CPROJ_CMB` construction references select the source curves, target surfaces, directions, and combination controls?

**Known.** `siemens_nx.md` §7.1 "A `CPROJ_CMB` payload contains at most one construction-reference graph framed as" defines the ordered eight-reference graph, block resolution, and logical payload.

**Need.** We must know the roles to construct the combined projected curves.

**Conflict.** The branch lanes prove repeated-anchor equality and preserve
branch order, while the tail preserves two additional references. All eight
non-repeated references still resolve only to offset-store blocks; no field
marker assigns curve, surface, direction, or combination semantics.

**Note.** Preserve the complete graph, branch-anchor relations, logical
payload, and strings as native data. Combined neutral projection requires an
independent relation for every source and control role.

### OM-17. `FSET` selection roles

**Question.** What does the `FSET` selector mean, and what selection role does each ordered object-reference group have?

**Known.** `siemens_nx.md` §7.1 "An `FSET` operation payload contains at most one two-group reference graph framed as" defines the selector, two separate reference groups, resolution rule, and logical payloads.

**Need.** We must know the roles to construct the selected face or feature set.

**Conflict.** The printable selector is retained as an opaque value, and the
two groups are distinguished only by their serialized first/second position.
Every reference uses the same object-index form and resolves only to an
offset-store block. No selector vocabulary, target schema, or relation to a
face or feature set assigns either group a selection role.

**Note.** Preserve the selector, group order, resolved references, and both
logical payloads as native data. A neutral selection requires a unique
selector domain and object-family relation for each group.

### OM-18. Pattern construction-reference roles

**Question.** Which ordered references in `Pattern Feature`, `Pattern Geometry`, and `Geometry Instance` select the seed, transform, and pattern controls?

**Known.** `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one ten-slot construction-reference graph in one of two exact layouts." and `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one transform lane." define the two construction graph framings, logical payload, and counted row forms.

**Need.** We must know the roles to construct neutral pattern dependencies and transforms.

**Conflict.** The graph layouts distinguish framing variants and preserve slot
order, but every non-null reference uses the same object-index form. Repeated
anchors, terminal slots, and the `Geometry Instance` one-reference form do not
identify a seed, transform, or control family. No relation joins a graph slot
to a transform row or an operation input with that role.

**Note.** Preserve pattern references, construction payloads, and transform
lanes as native data. A neutral pattern dependency or transform requires an
independent seed and control-role relation.

### OM-19. Pattern-row scalar roles

**Question.** What does each scalar in a counted pattern row mean?

**Known.** `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one transform lane." and `siemens_nx.md` §7.1 "`Pattern Feature` also admits the wide-row form" define the Q1.55 and wide-row scalar encodings, row order, exact values, and boundaries.

**Need.** We must know the roles to construct pattern coordinates, spacing, angles, and other controls.

**Conflict.** Row schemas and terminal modes select byte layout and scalar
width, not physical meaning. The Q1.55 and binary scalar lanes retain finite
values, order, selectors, and row ordinals, but provide no units, axis or
spacing labels, or relation to a seed/reference role.

**Note.** Preserve every row scalar with its encoding and offsets. Do not map
the values to coordinates, spacing, angles, or other neutral controls until a
pattern-family schema assigns those roles.

### OM-20. Pattern-row selector roles

**Question.** What does each compact selector in a counted pattern row select?

**Known.** `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one transform lane." and §7.1 "`Pattern Feature` payloads contain at most one counted reference lane." define selector framing, non-null requirements, row ordinals, counted references, and exact tokens.

**Need.** We must know the roles to bind each row to its seed or transform operand.

**Conflict.** A row selector is a non-null compact value retained with its
row ordinal and token offset. The counted reference lane has its own ordered
object indices, but no serialized equality or ownership field joins either
lane to a construction-graph reference, seed, or transform operand.

**Note.** Preserve selectors and counted references as separate native lanes.
Selector equality or ordinal proximity alone does not establish a seed or
transform binding.

### OM-21. `Multi Instance Output` roles

**Question.** What does each selector group and trailing reference in a `Multi Instance Output` lane mean?

**Known.** `siemens_nx.md` §7.1 "`Multi Instance Output` payloads contain at most one counted output lane" defines the ordered lane framing and retains its exact selectors and references.

**Need.** We must know the roles to relate pattern instances to their output bodies or geometry.

**Conflict.** The lane provides selector values, serialized instance and row
ordinals, an instance count, and trailing object references. These fields are
bound only by row order and count invariants; no relation identifies a
selector target, an output-body namespace, or the geometry represented by a
trailing reference.

**Note.** Preserve the complete lane as native instance-output data. Do not
create output bodies or geometry bindings without an independent target and
instance relation.

### OM-22. Equal pattern and profile labels

**Question.** What serialized relation establishes identity or a seed relation between blocks that have equal canonical line labels?

**Known.** `siemens_nx.md` §2 "Input bindings from two or more distinct operation headers form an identity" and `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one ten-slot construction-reference graph in one of two exact layouts." define operation input identity by resolved store block. Equal text in distinct pattern and profile blocks does not establish block identity.

**Need.** We must know the relation to connect a pattern to the correct seed without merging unrelated blocks.

**Conflict.** A canonical line label is payload content, not a persistent
object identity. Pattern and profile construction blocks retain separate
operation-owned block identities; only an exact shared input block or another
unique serialized relation can join them. Equal text labels do not provide
that relation.

**Note.** Keep equal-label blocks separate and preserve each operation's
native references. A pattern seed join requires one unique block or explicit
cross-record identity relation.

### OM-23. `POINT` header fields

**Question.** What construction role does the `POINT` header reference have, and what does its `02|03` mode select?

**Known.** `siemens_nx.md` §7.1 "The `POINT` operation payload begins with one construction header." defines the complete header grammar, canonical reference, mode values, and datum-point family.

**Need.** We must know the roles to select the point construction method and its operand.

**Conflict.** The header reference selects one bounded six-scalar lane and
the mode selects a serialized branch, but neither field names an operand
family or construction method. The header-to-lane boundary proves ownership
only; it does not assign the lane to a model point, support, or other datum
construction role.

**Note.** Preserve the header, mode, selected lane, and exact cross-block
ownership as native point data. A neutral point construction requires a
mode-domain and operand-role relation.

### OM-24. `POINT` scalar triples

**Question.** What coordinate spaces and construction roles do the two ordered scalar triples in the selected six-scalar `POINT` lane use?

**Known.** `siemens_nx.md` §7.1 "The header reference selects a six-scalar lane ending in its addressed offset-store block." defines the six-scalar lane, its selected form, values, and exact boundaries. A shared target block does not identify either triple as the model-space point.

**Need.** We must know the roles to construct the datum point at its authored model-space position.

**Conflict.** The selected lane contains two ordered triples of finite scalar
values and no coordinate-space, axis, unit, or role discriminator. Byte
continuity across the preceding and selected blocks establishes one lane, not
which triple is the authored point or how the other triple participates.

**Note.** Retain both triples with their exact scalar encodings and offsets.
Do not project either triple to model-space geometry without a unique role and
coordinate-frame relation.

### OM-25. `DRAFT` construction roles

**Question.** Which counted leading indices, ordered references, terminal indices, and tail fields select the drafted faces, neutral plane, pull direction, and draft angle?

**Known.** `siemens_nx.md` §7.1 "The `DRAFT` operation payload begins" and `siemens_nx.md` §7.1 "The same payload contains exactly one four-reference construction graph." define the leading index lane, four-reference graph, terminal lanes, scalar encodings, store resolution, and exact boundaries.

**Need.** We must know the roles to construct a neutral draft operation.

**Conflict.** The leading lane, four-reference graph, identity frames,
scalar lanes, and terminal indices are separately framed and store-resolved,
but no field-role relation assigns faces, plane, pull direction, angle, or
termination semantics. Shared store identity and serialized order establish
construction ownership only.

**Note.** Preserve every complete draft lane and graph as native data. Neutral
draft projection requires independent relations for the drafted selection,
reference plane, direction, angle, and result controls.

### OM-26. `SKIN` construction roles

**Question.** Which ordered `SKIN` references and branch groups select sections, guides, continuity, and terminal controls?

**Known.** `siemens_nx.md` §7.1 "The `SKIN` and `THRU_CURVE` operation labels identify loft-family constructions." and `siemens_nx.md` §7.1 "`SKIN` and `Studio Surface` payloads share one exact common construction-reference envelope." define the loft family, common construction envelope, ordered references, and logical payload.

**Need.** We must know the roles to construct the neutral loft surface or body.

**Conflict.** The common envelope preserves fourteen references in order, and
the branch groups preserve family, mode, members, and terminal references.
These fields use shared reference grammars and provide no relation assigning
sections, guides, continuity, or terminal controls. Scalar pairs and strings
are payload-owned and do not add those roles.

**Note.** Preserve the envelope, reconstructed payload, scalar pairs, strings,
and branch groups as native loft data. Neutral surface construction requires
independent section, guide, and control-role relations.

### OM-27. `Studio Surface` construction roles

**Question.** Which ordered `Studio Surface` references and branch groups select control geometry, continuity, and terminal controls?

**Known.** `siemens_nx.md` §7.1 "`SKIN` and `Studio Surface` payloads share one exact common construction-reference envelope." and `siemens_nx.md` §7.1 "The `Studio Surface` operation label identifies a freeform-surface construction." define the surface construction envelope, ordered references, logical payload, and feature family.

**Need.** We must know the roles to construct the neutral freeform surface.

**Conflict.** `Studio Surface` shares the fourteen-reference envelope and
counted branch grammar with `SKIN`; the operation label selects the family but
does not label individual references or branch members. No serialized field
assigns control geometry, continuity, or terminal semantics.

**Note.** Preserve the complete surface envelope and branch payload as native
freeform-surface data. Neutral projection requires a unique role relation for
each control-geometry and continuity field.

### OM-28. Plain cached-body ownership

**Question.** Which feature owns each plain cached-body stream?

**Known.** `siemens_nx.md` §2 "A partition or plain cached-body wrapper word begins" and `siemens_nx.md` §7.2 "Across the ordered feature-history sections, the last non-`DELETE` operation carrying a primary-body field is that body object's latest writer." define segment tuples for partition and plain cached-body streams, body writers, operands, aliases, and terminal lineage.

**Need.** We must know the ownership relation to use a cached body as the correct feature result or tool.

**Conflict.** The tuple identifies the stream wrapper, classification, two
aliases, and role word. A primary-body field or resolved segment operand can
establish a unique alias-use relation, and ordered feature history can
establish the latest writer and consumer for that alias component. Neither
the tuple nor its role word identifies the feature that authored a plain
cached-body image when no unique primary or operand relation exists. Stream
order and alias equality do not assign feature ownership.

**Note.** Preserve every plain cached-body binding and each unique
primary/operand relation independently. Use terminal lineage only after the
complete status relation resolves. Do not assign a cached stream to a feature
result or tool without a unique feature-field relation; retain ownership as
unresolved.

### OM-29. `RMFastLoad` class records

**Question.** What is the per-class entity-record grammar in `RMFastLoad` outside its object-ID membership table?

**Known.** `siemens_nx.md` §2.1 "| `/Root/FastLoad/RMFastLoad`" and `siemens_nx.md` §2.3 "`/Root/FastLoad/Structure` begins with the twelve-byte envelope" define the fast-load object-ID table and the bounded structure stream. The per-class entity-record grammar outside the membership table remains unresolved.

**Need.** We must know the class grammars to transfer the remaining fast-load state as typed data.

### OM-33. Decisive active-body membership

**Question.** What makes an `RMFastLoad` membership assignment decisive for one body image against another?

**Known.** `siemens_nx.md` §7.2 "`RMFastLoad` stores the active object-id set alongside the partition and deltas body records." defines the membership table, the shared FACE, EDGE, and VERTEX identity space, independent per-image assignment, and the rule that an image without active membership is retained unless another image has a decisive membership assignment. It does not define decisive.

**Need.** We must know the condition to select the active bodies. The current decoder retains every image whose complete nonempty FACE, EDGE, and VERTEX node-ID set is a subset of the active set, but the format does not establish whether that subset relation is decisive, whether active IDs may be stale or unioned, or how multiple matching images should be handled. The selection deletes other bodies and their complete topology and geometry from the model, so an unsupported membership rule removes a current body permanently. The exact feature-history rule runs only when this condition declines, so membership semantics take precedence over it.

**Note.** `crates/cadmpeg-codec-nx/src/decode/build.rs:1306-1340` requires every participating FACE node ID and every EDGE or VERTEX identity referenced by a retained FIN to resolve before applying the subset relation. The subset rule, active-set authority, and union/stale behavior remain unresolved; a membership match is not promoted beyond that admission boundary.

### OM-37. Final field declaration in a pointerless OM section

**Question.** What terminates the final member-field declaration in a section without a unique valid record-area pointer?

**Known.** `siemens_nx.md` §7.1 "The first record at `oid_end` begins `04 01, declared_len:u8, version_text[declared_len-2], 00`" defines the product record and the bounded registry suffix through the next length-framed `m_` declaration. A section-relative `u32 LE` word after the type registry is a record-area pointer only when its forward target remains inside the section, starts with the three control words and product record, and is unique. When valid, that pointer bounds the complete field registry.

**Need.** We must know the terminal marker or alternate boundary for the final field declaration when no valid record-area pointer exists. A pointerless section cannot establish a complete field registry from the settled byte structure alone.

### OM-39. Stream-level omission of unselected body images

**Question.** Which serialized state permits the geometry decode to omit a complete Parasolid stream?

**Known.** `siemens_nx.md` §7.2 "`RMFastLoad` stores the active object-id set alongside the partition and deltas body records." defines the active object-id set, the independent assignment to each body image, and the retention rule for an image that has no active membership. `siemens_nx.md` §7.1 "Within one feature-history record area, operation records are stored in reverse" gives the reversed record order that supplies the terminal status of a body. Neither statement relates membership or terminal status to the contents of a stream, and neither permits the omission of a stream.

**Need.** We must know the state. The decoder reads the stream ordinal from the text of each selected body identity and decodes only those streams. The carriers, topology, and intersections of every other Parasolid stream are then absent from the model, and no loss code reports the omission. The rules that select the images are open in OM-33 and OM-31.

**Note.** `crates/cadmpeg-codec-nx/src/decode/build.rs:141-191` makes the selection before geometry construction, `build.rs:1283-1304` maps the selected identities to stream ordinals, and `build.rs:233-247` omits each other stream and keeps its bytes as opaque data. The commit that added the rule also changed two golden documents in the same change: one partition stream and fifty analytic surface carriers left the decoded output.

## 3. Assembly and material data

### AM-01. Fast-load structure stream

**Question.** What is the field grammar and semantics of `/Root/FastLoad/Structure`?

**Known.** `siemens_nx.md` §2.3 "`/Root/FastLoad/Structure` begins with the twelve-byte envelope" and `siemens_nx.md` §7.1 "UG_PART begins with a 12-byte row table" define the big-endian bounded OM envelope and typed class and member declarations. The typed component roster defines ordered named prototypes and a one-based prototype index for each distinct occurrence. A second counted table stores UUID identities, and a parallel one-based index associates every occurrence with one UUID. Other payload fields remain uninterpreted.

**Need.** We must know the remaining payload grammar, including hierarchy,
placement, UUID, and state fields.

### AM-02. Fast-load JT stream

**Question.** What is the field grammar and semantics of `/Root/FastLoad/JT`?

**Known.** `siemens_nx.md` §2.1 "| `/Root/FastLoad/JT`" and `siemens_nx.md` §2.1 "| `/Root/UG_PART/DisplayJT`" classify the stream separately from `/Root/UG_PART/DisplayJT` and retain its bounded container entry.

**Need.** We must know the grammar to decode its fast-load display relations as typed data.

### AM-03. Last-saved toggle stream

**Question.** Which native objects do the saved toggle identities address, and
does an `On` or `Off` member control feature suppression, visibility, or another
state domain?

**Known.** `siemens_nx.md` §2.2 "`/Root/UG_PART/LastSavedToggleInfoStream` is one atomic payload:" defines the complete counted stream envelope and retains each 32-hex-digit identity and `On`/`Off` state exactly. The member count is independent of the feature-operation-label count. The toggle identities have no proven join to feature-operation records.

**Need.** We must identify the addressed object namespace before projecting any
member as a neutral suppression or visibility state.

### AM-04. `DisplayJT` outer-index values

**Question.** What does each nonzero outer-index row value in `/Root/UG_PART/DisplayJT` mean?

**Known.** `siemens_nx.md` §2.3 "`/Root/UG_PART/DisplayJT` begins with `version:u32 LE, count:u32 LE`" and `siemens_nx.md` §2.3 "Each indexed JT document extends from its header offset" define the outer index, referenced JT segments, scene graph, shape data, and tessellation relations.

**Need.** We must know the row roles to relate each indexed value to the correct display object or segment.

### AM-05. Assembly occurrence placement

**Question.** Which serialized fields define the transform and units of each assembly occurrence placement?

**Known.** `siemens_nx.md` §2.3 "Assembly `.prt` files contain no inline Parasolid partition, deltas, or plain cached-body streams." defines external child geometry and requires occurrence placement. The OM expression grammar defines stored numeric expression values.

**Need.** We must know the fields to place each child part in assembly coordinates.

### AM-06. Assembly pattern dimensions

**Question.** Which serialized fields bind assembly pattern angles, counts, metric radii, and base frames to an occurrence pattern?

**Known.** `siemens_nx.md` §3.3 "A numeric expression table contains a `hostglobalvariables` root entity." defines `hostglobalvariables` names, units, formulas, and evaluated values. It does not define the occurrence-pattern binding.

**Need.** We must know the bindings to construct patterned assembly occurrences.

### AM-07. Child handle sets to occurrences

**Question.** How does each child-bound persistent-handle set identify one distinct assembly occurrence?

**Known.** `siemens_nx.md` §2.3 "Each `metadata` and `prototype` is" and `siemens_nx.md` §7.1 "**Persistent-handle identity.**" define occurrence ordinals, named prototypes, persistent handles, and their independent occurrence counts.

**Need.** We must know the mapping to preserve multiple occurrences of the same child part.

**Conflict.** A complete external-reference record now provides a child name,
child directory, ordered handle lane, and record identity. The FastLoad roster
provides prototype and UUID indices for its occurrence lane. No serialized
field joins an external-reference record or handle to one FastLoad occurrence
ordinal. Equal child names, repeated handles, and equal UUID groups do not
establish that instance-level relation.

**Note.** Preserve the complete child binding and the FastLoad occurrence lane
as separate native relations. Do not construct neutral occurrences or assign
placements until a unique child-to-occurrence join and a transform owner are
decoded.

### AM-08. Residual `EXTREFSTREAM` tail fields

**Question.** What are the field boundaries and roles of the residual bytes in an indexed `EXTREFSTREAM` record tail?

**Known.** `siemens_nx.md` §9.1 "An `EXTREFSTREAM` record region begins with" and `siemens_nx.md` §9.1 "Slot zero names the child `.prt`, slot one is the reference code," define the indexed-record boundary, handle-set prefix, persistent-handle pairs, tagged references, string uses, and child binding. Other tail bytes remain opaque.

**Need.** We must know the fields to decode complete occurrence and external-reference state.

### AM-09. SDL/TYSA attribute values

**Question.** How does each Parasolid SDL/TYSA attribute instance assign its referenced value records to the fields declared by its type-79 class definition?

**Known.** `siemens_nx.md` §9.4 "A shell, face, loop, edge, FIN, or vertex topology record with one uniquely resolved" and `siemens_nx.md` §9.4 "When a value resolves without a unique declared-field assignment, its neutral" define attribute-class declarations, type-81 class selection, referenced value records, topology ownership, and neutral source-attribute names. The declaration includes ordered field type codes such as those for `SDL/TYSA_DENSITY` and `SDL/TYSA_BLEND_ID`. The specification assigns the two fields of `SDL/TYSA_DENSITY` by name. Every other class falls back to the zero-based declared field ordinal and declared field code.

**Need.** We must know the assignment to transfer class-specific material and topology attributes with semantic field names.

**Note.** This item was closed by an assignment for one class. The question covers every class, and `SDL/TYSA_BLEND_ID` is named in the item itself and remains unassigned. A per-class table with one entry does not answer a per-class question, so the item is open again.

The closure test only exercises already-populated `ParasolidAttributeFieldUse` values; it does not show the raw SDL/TYSA records assigning those values. The ordinal/code fallback in the production mapper remains unsupported by an independent serialized witness, so this item stays open.

### AM-10. Physical material bindings

**Question.** Which serialized relation binds a physical material to a Parasolid face identity?

**Known.** `siemens_nx.md` §2.3 "Each `/Root/materialsTif/<name>` file entry contains one TIFF stream." and `siemens_nx.md` §9.4 "The type-81 definition reference selects an attribute class when it equals" define preview and texture assets, the material-texture catalog, and topology-owned Parasolid attributes. `siemens_nx.md` §7.1 "An explicit display-color assignment addresses a face when" defines the complete face appearance relation; a palette color is not a physical-material assignment.

**Need.** We must know the relation to assign physical-material state to neutral faces without treating a display color, texture asset, or topology attribute as a material identity.

## 4. Test evidence

### TE-01. Golden coverage of the native arenas named by open items

**Question.** Which native arenas does the golden fixture set populate, and which fields therefore have no snapshot witness?

**Known.** `crates/cadmpeg-codec-nx/src/golden_tests.rs` names two hundred and thirty-three arenas and asserts that the fixture set populates at least one hundred and twenty-two of them. The golden documents serialize the complete decoded document, so a changed field of a populated arena moves a golden. The fixture set populates one hundred and twenty-seven arenas. One hundred and six arenas are populated by no fixture.

**Need.** We must have a snapshot witness for the fields that carry open items. `feature_operation_tagged_references`, `feature_operation_data_block_references`, and `feature_pattern_counted_reference_lanes` received new §7.1 grammars, and no fixture populates them. `feature_body_data_block_uses`, `feature_body_segment_uses`, `feature_operation_body_members`, and `feature_operation_body_operands` carry the relations of OM-07, OM-08, and OM-40. `feature_hole_package_construction_group_lanes` and `feature_simple_hole_construction_groups` carry the relation of OM-30. `saved_toggle_streams` carries AM-03 and the `fast_load_component_*` arenas carry AM-01 and AM-07. A change to any of these fields moves no golden, so the unit tests written with each change are the only evidence.
