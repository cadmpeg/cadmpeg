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

### PS-26. Boundary pcurve relation without a chart

**Question.** Which serialized relation defines the boundary pcurve of an EDGE whose supports carry no chart and no analytic isocurve relation?

**Known.** `siemens_nx.md` §5.3 "An EDGE may carry null curve reference `1` with a finite tolerance. With a null" defines chart-certified transfer for a null carrier and states that transfer does not synthesize a model-space line between the vertices. `siemens_nx.md` §5.3 "A null `EDGE.curve` may instead have a non-null owning `FIN.curve`. The FIN" defines the FIN-carrier case. Neither defines a boundary pcurve for a plane or quadric support that has no chart.

**Need.** We must define the relation that fixes the complete parameter-space interior. Endpoint inversion fixes only the interval ends; it does not determine the curve between them.

### PS-27. Unresolved EDGE end vertex

**Question.** What is the correct reading of an EDGE whose end vertex does not resolve to a decoded POINT?

**Known.** `siemens_nx.md` §5.3 "An EDGE belongs to the assembled B-rep only when a FIN in a fully resolved owned LOOP" and `siemens_nx.md` §5.3 "POINT is a geometric carrier. It becomes a topological vertex only through a validated `FIN.ver" define endpoint incidence through the FIN chain and the POINT-to-vertex condition. They do not define the case in which the resolved end vertex has no decoded POINT.

**Need.** We must know the reading to separate a closed edge from an edge that lost one endpoint. Without that rule, the decoder retains neither the edge nor its dependent loop when the end vertex has no decoded POINT.

### PS-28. Compact tombstone boundary condition

**Question.** Which condition ends a compact tombstone, and does a following byte pair constrain it?

**Known.** `siemens_nx.md` §4.2 "**Tombstone:** a compact 6-byte deletion begins with `type:u16 BE`. A short XMT identity occupies" defines the tombstone as a self-delimiting six-byte form with two identity encodings. `siemens_nx.md` §4.2 "Tombstones form descending contiguous xmt runs that can span topology, geometry, attribute, intersection-auxiliary," defines the runs. Neither states a condition on the bytes after a tombstone.

**Need.** We must define the enclosing grammar or discriminator that distinguishes a six-byte tombstone from the same byte pattern inside another bounded payload. The following record family must not participate in tombstone identity.

### PS-31. Fixed-node frame ownership

**Question.** Which stream invariant owns a fixed-node frame when the node does not participate in a complete body-topology graph?

**Known.** `siemens_nx.md` §4.1 defines `node_id` as a big-endian u32 field with no smaller numerical bound. A full-domain interpretation is admitted when it preserves every baseline record and uniquely changes an incomplete body-topology graph into a complete graph. A typed XMT slot also admits the unique full-domain node of its required family, recursively through procedural carrier dependencies. These proofs admit mixed low and high node identities in topology, ownership, and carrier families. They do not prove a fixed frame with no complete-topology or typed-reference owner. Exact adjacency to complete fixed records on both sides is not an owner: opaque payloads can contain a complete fixed-record-shaped run whose end and start coincide with adjacent candidate boundaries.

**Need.** We must identify the sequential or enclosing owner of a fixed frame that has no complete-topology or typed-reference owner. The proof must exclude complete-looking fixed records inside opaque payloads.

**Conflict.** The ambiguity baseline still rejects several fixed-node families when `node_id` is greater than `1,000,000`. The cutoff cannot reject a topology node that completes the body graph or a uniquely referenced ownership or carrier node. It can still reject a valid unowned fixed frame.

## 2. Object model and body composition

### OM-01. Per-class OM field serialization

**Question.** What byte grammar and semantic role does each declared field of each NX OM class use?

**Known.** `siemens_nx.md` §7.1 "UG_PART begins with a 12-byte row table" and `siemens_nx.md` §7.1 "A labeled feature-history operation record begins at the fixed operation-header marker" define OM section boundaries, class and member declarations, store identities, compact indices, and expression records. `siemens_nx.md` §3.3 "A numeric expression table contains a `hostglobalvariables` root entity." defines typed fields for selected construction families.

**Need.** We must know the remaining class grammars to decode feature history, constraints, attributes, and material bindings as typed fields.

### OM-02. `SKETCH` scalar-pair geometry

**Question.** What geometric quantities and coordinate spaces do the framed scalar pairs in a `SKETCH` construction payload represent?

**Known.** `siemens_nx.md` §2 "An operation label equal to `SKETCH` denotes a sketch history operation." `siemens_nx.md` §7.1 "A sketch payload scalar field is", `siemens_nx.md` §7.1 "A sketch repeated-type scalar pair is", `siemens_nx.md` §7.1 "A sketch fixed pair has one of four exact forms:", and `siemens_nx.md` §7.1 "A sketch scalar-vector lane is" define sketch record identity and the framed payload lanes but do not assign a model-space frame, sketch entity, or constraint role from equal scalar values.

**Need.** We must know the roles to construct neutral sketch geometry and constraints.

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

### OM-05. OM declaration trailing code

**Question.** What does the trailing byte in an OM class or member declaration mean?

**Known.** `siemens_nx.md` §7.1 "The first record at `oid_end` begins" defines the declaration length, `UGS::` or `m_` name, trailing-code boundary, and following registry suffix.

**Need.** We must know the code domain and the relation between the trailing
byte and the declared class or member semantics.

**Conflict.** The declaration grammar identifies the trailing byte as the
single byte after the printable `UGS::` or `m_` name and before the bounded
registry suffix. The name and suffix delimit the declaration but do not assign
a type, ownership, visibility, or field-role meaning to that byte.

### OM-06. OM registry suffix fields

**Question.** What does each byte in a bounded OM field-registry suffix mean?

**Known.** `siemens_nx.md` §7.1 "The first record at `oid_end` begins" defines the suffix boundary and the 11-to-14-byte prefix, fingerprint, and terminal-byte decomposition.

**Need.** We must know the semantic field roles represented by the layout
prefix, schema fingerprint, and terminal byte.

**Conflict.** The 11–14-byte suffix decomposes into a 2–5-byte prefix,
eight-byte fingerprint, and one terminal byte for both class and member
declarations. The decomposition does not assign a member type, cardinality,
ownership, or access role.

### OM-08. Blend-created faces

**Question.** How does a blend construction identify the faces created by that
blend?

**Known.** A nested operation body-write frame retains the persistent body
identity, partition-local GROUP node, endpoint tag, and write-state body-image
object. Endpoint tags `10`, `12`, and `15` select the body-image field.
Explicit Boolean operations independently require the body-image object to
equal the target and exclude every tool. A closed GROUP chain resolves current
members by topology family and kernel node identity and transfers current FACE,
EDGE, and VERTEX identities to the feature result state. A labeled or unlabeled
write whose GROUP records resolve to exactly one partition binds its persistent
body identity to that partition. When the partition has one body, every labeled
write with that identity writes that body. Independently, a unique plain-stream
alias equal to the persistent body identity binds that write to the unique body
in the selected stream without requiring body-image block resolution.

**Need.** We must identify the blend construction relation that assigns the
created-face subset.

**Conflict.** GROUP membership assigns topology to the producing feature but
does not by itself identify which member faces carry a particular blend-surface
construction.

### OM-10. Operation suppression fields

**Question.** How do the embedded operation state lanes encode suppression?

**Known.** `siemens_nx.md` §2 derives active state from the closed output-and-dependency graph and admits retained-history primary-body and partition-scoped body-write closure witnesses when neutral output projection cannot bind an operation to the selected body. `siemens_nx.md` §7.1 defines the feature-history journal, status rows, diagnostic messages, `m_rollForwardStates` groups, and object state-counter map. The native decoder retains exact status codes and payloads, message values and count/severity words, group rows, and counter-map rows. Status code `41` with plain payload is the built, normal state. The first three common-frame state bytes remain an untyped operation-state prefix; bytes 3 through 7 are legacy module inactivity, Parasolid-data mutation, split tracking, and group count. The saved-toggle stream carries independent toggle identities and states. The Parasolid `UGS/ObjectState` class is a one-field code-3 character attribute whose value is an ordinary type-84 string; its owner, when resolved, is a topology attribute-list relation. The OM registry classes `UGS::OM::ObjectStateCollection` and `UGS::OM::ObjectState` are distinct declarations. The `m_objectStateCollection` member has declaration code `78` in both UG_PART and RMFastLoad. That code is not a section-local class ordinal: the UG_PART class ordinals differ, and RMFastLoad does not declare the collection class. The product-anchored control form retains an aligned u32 value lane whose in-range values address same-store data blocks; it does not declare class references. An object record retains its object identifier and bounded bytes, but no class binding or typed field value.

**Need.** We must know the serialized suppression fields to construct operation state for all configurations.

**Conflict.** Neither the common-frame state lane nor the saved-toggle stream identifies a feature suppression value. A status row identifies an object and retains a code, but the code has no suppression meaning by itself. A roll-forward relation row retains two object endpoints, but the operation identity, state-object identity, and typed state value require separate unique joins. The `78` member code does not supply the missing class reference. The OM declarations and product-anchored value lane do not provide a relation from a class declaration to an object record or from an object record to a typed state value. The Parasolid `UGS/ObjectState` lane provides a character value and, when present, a topology owner, but no operation owner or suppression domain.

### OM-11. `DELETE` nullable-reference roles

**Question.** What object family can each of the five leading nullable references in a `DELETE` payload address, and what is the role of each slot?

**Known.** `siemens_nx.md` §2 "`DELETE` is a body-deletion operation only when its bounded record contains the" and `siemens_nx.md` §2 "A `DELETE` payload begins with one nullable reference field" define the five-slot field, canonical reference encodings, resolution rule, logical payload, and body-target exclusion.

**Need.** We must know the slot roles to decode the delete construction independently of its primary-body target.

**Conflict.** Each slot carries only a nullable canonical object index. Unique
offset-store resolution identifies a block, but the slot has no discriminator,
schema tag, or independent relation that assigns an object family or semantic
role. Slot order and five-block concatenation preserve construction order only;
the separate primary-body field does not label any of these slots.

### OM-12. Inactive-arrangement body state

**Question.** Which bodies belong to each inactive arrangement, and what per-body state does that arrangement select?

**Known.** `siemens_nx.md` §2 "`/Root/part/arrangements` has an `Arrangements` root." and `siemens_nx.md` §2 "A unique part-owned `NX_Arrangement` string attribute names the active" define arrangement identity and active body membership. Other arrangements have no body membership without a separate relation.

**Need.** We must know the relation to construct inactive configuration body sets.

**Conflict.** The arrangement XML contains only configuration name, default
flag, and order. The current B-rep body census has no arrangement key, and the
active attribute join identifies only the selected configuration. Feature
closure also describes the current body set, not an alternate arrangement's
membership or per-body state.

### OM-13. Inactive-arrangement parameter state

**Question.** Which parameter values does each inactive arrangement select?

**Known.** `siemens_nx.md` §2 "When exactly one active configuration has complete body membership, the same" and `siemens_nx.md` §2 "The same active configuration retains the complete current parameter state when" define complete parameter state only for a uniquely resolved active configuration.

**Need.** We must know the relation to construct inactive configuration parameter maps.

**Conflict.** Neutral parameter identities, evaluated values, ownership scopes,
and dependencies contain no arrangement identity or per-arrangement override
selector. The complete parameter map is derived only for the unique active
configuration after the global parameter graph passes its ownership and order
checks.

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

### OM-15. `CPROJ` construction-reference roles

**Question.** Which `CPROJ` construction references select the source curve, target surface, direction, and combination controls?

**Known.** `siemens_nx.md` §7.1 "A `CPROJ` payload contains at most one construction-reference field framed as" defines the ordered three-reference graph, block resolution, and logical payload.

**Need.** We must know the roles to construct a neutral projected curve.

**Conflict.** The three references use one canonical encoding and resolve only
to offset-store blocks. The fixed middle and suffix markers delimit the field,
but do not identify a source curve, target surface, direction, or combination
control. Reconstructed strings remain payload-owned and have no independent
role relation.

### OM-16. `CPROJ_CMB` construction-reference roles

**Question.** Which `CPROJ_CMB` construction references select the source curves, target surfaces, directions, and combination controls?

**Known.** `siemens_nx.md` §7.1 "A `CPROJ_CMB` payload contains at most one construction-reference graph framed as" defines the ordered eight-reference graph, block resolution, and logical payload.

**Need.** We must know the roles to construct the combined projected curves.

**Conflict.** The branch lanes prove repeated-anchor equality and preserve
branch order, while the tail preserves two additional references. All eight
non-repeated references still resolve only to offset-store blocks; no field
marker assigns curve, surface, direction, or combination semantics.

### OM-17. `FSET` selection roles

**Question.** What does the `FSET` selector mean, and what selection role does each ordered object-reference group have?

**Known.** `siemens_nx.md` §7.1 "An `FSET` operation payload contains at most one two-group reference graph framed as" defines the selector, two separate reference groups, resolution rule, and logical payloads.

**Need.** We must know the roles to construct the selected face or feature set.

**Conflict.** The printable selector is retained as an opaque value, and the
two groups are distinguished only by their serialized first/second position.
Every reference uses the same object-index form and resolves only to an
offset-store block. No selector vocabulary, target schema, or relation to a
face or feature set assigns either group a selection role.

### OM-18. Pattern construction-reference roles

**Question.** Which ordered references in `Pattern Feature`, `Pattern Geometry`, and `Geometry Instance` select the seed, transform, and pattern controls?

**Known.** `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one ten-slot construction-reference graph in one of two exact layouts." and `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one transform lane." define the two construction graph framings, logical payload, and counted row forms.

**Need.** We must know the roles to construct neutral pattern dependencies and transforms.

**Conflict.** The graph layouts distinguish framing variants and preserve slot
order, but every non-null reference uses the same object-index form. Repeated
anchors, terminal slots, and the `Geometry Instance` one-reference form do not
identify a seed, transform, or control family. No relation joins a graph slot
to a transform row or an operation input with that role.

### OM-19. Pattern-row scalar roles

**Question.** What does each scalar in a counted pattern row mean?

**Known.** `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one transform lane." and `siemens_nx.md` §7.1 "`Pattern Feature` also admits the wide-row form" define the Q1.55 and wide-row scalar encodings, row order, exact values, and boundaries.

**Need.** We must know the roles to construct pattern coordinates, spacing, angles, and other controls.

**Conflict.** Row schemas and terminal modes select byte layout and scalar
width, not physical meaning. The Q1.55 and binary scalar lanes retain finite
values, order, selectors, and row ordinals, but provide no units, axis or
spacing labels, or relation to a seed/reference role.

### OM-20. Pattern-row selector roles

**Question.** What does each compact selector in a counted pattern row select?

**Known.** `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one transform lane." and §7.1 "`Pattern Feature` payloads contain at most one counted reference lane." define selector framing, non-null requirements, row ordinals, counted references, and exact tokens.

**Need.** We must know the roles to bind each row to its seed or transform operand.

**Conflict.** A row selector is a non-null compact value retained with its
row ordinal and token offset. The counted reference lane has its own ordered
object indices, but no serialized equality or ownership field joins either
lane to a construction-graph reference, seed, or transform operand.

### OM-21. `Multi Instance Output` roles

**Question.** What does each selector group and trailing reference in a `Multi Instance Output` lane mean?

**Known.** `siemens_nx.md` §7.1 "`Multi Instance Output` payloads contain at most one counted output lane" defines the ordered lane framing and retains its exact selectors and references.

**Need.** We must know the roles to relate pattern instances to their output bodies or geometry.

**Conflict.** The lane provides selector values, serialized instance and row
ordinals, an instance count, and trailing object references. These fields are
bound only by row order and count invariants; no relation identifies a
selector target, an output-body namespace, or the geometry represented by a
trailing reference.

### OM-22. Equal pattern and profile labels

**Question.** What serialized relation establishes identity or a seed relation between blocks that have equal canonical line labels?

**Known.** `siemens_nx.md` §2 "Input bindings from two or more distinct operation headers form an identity" and `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one ten-slot construction-reference graph in one of two exact layouts." define operation input identity by resolved store block. Equal text in distinct pattern and profile blocks does not establish block identity.

**Need.** We must know the relation to connect a pattern to the correct seed without merging unrelated blocks.

**Conflict.** A canonical line label is payload content, not a persistent
object identity. Pattern and profile construction blocks retain separate
operation-owned block identities; only an exact shared input block or another
unique serialized relation can join them. Equal text labels do not provide
that relation.

### OM-23. `POINT` header fields

**Question.** What construction role does the `POINT` header reference have, and what does its `02|03` mode select?

**Known.** `siemens_nx.md` §7.1 "The `POINT` operation payload begins with one construction header." defines the complete header grammar, canonical reference, mode values, and datum-point family.

**Need.** We must know the roles to select the point construction method and its operand.

**Conflict.** The header reference selects one bounded six-scalar lane and
the mode selects a serialized branch, but neither field names an operand
family or construction method. The header-to-lane boundary proves ownership
only; it does not assign the lane to a model point, support, or other datum
construction role.

### OM-24. `POINT` scalar triples

**Question.** What coordinate spaces and construction roles do the two ordered scalar triples in the selected six-scalar `POINT` lane use?

**Known.** `siemens_nx.md` §7.1 "The header reference selects a six-scalar lane ending in its addressed offset-store block." defines the six-scalar lane, its selected form, values, and exact boundaries. A shared target block does not identify either triple as the model-space point.

**Need.** We must know the roles to construct the datum point at its authored model-space position.

**Conflict.** The selected lane contains two ordered triples of finite scalar
values and no coordinate-space, axis, unit, or role discriminator. Byte
continuity across the preceding and selected blocks establishes one lane, not
which triple is the authored point or how the other triple participates.

### OM-25. `DRAFT` construction roles

**Question.** Which counted leading indices, ordered references, terminal indices, and tail fields select the drafted faces, neutral plane, pull direction, and draft angle?

**Known.** `siemens_nx.md` §7.1 "The `DRAFT` operation payload begins" and `siemens_nx.md` §7.1 "The same payload contains exactly one four-reference construction graph." define the leading index lane, four-reference graph, terminal lanes, scalar encodings, store resolution, and exact boundaries.

**Need.** We must know the roles to construct a neutral draft operation.

**Conflict.** The leading lane, four-reference graph, identity frames,
scalar lanes, and terminal indices are separately framed and store-resolved,
but no field-role relation assigns faces, plane, pull direction, angle, or
termination semantics. Shared store identity and serialized order establish
construction ownership only.

### OM-26. `SKIN` and `THRU_CURVE` construction roles

**Question.** Which ordered `SKIN` and `THRU_CURVE` references and branch groups select sections, guides, continuity, and terminal controls?

**Known.** `siemens_nx.md` §7.1 "The `SKIN` and `THRU_CURVE` operation labels identify loft-family constructions.", §7.1 "A `THRU_CURVE` payload begins with the exact construction-reference envelope", §7.1 "The envelope is followed by one counted branch group.", and §7.1 "`SKIN` and `Studio Surface` payloads share one exact common construction-reference envelope." define the loft family, both exact construction envelopes, counted branch groups, ordered references, and the fourteen-block logical payload.

**Need.** We must know the roles to construct the neutral loft surface or body.

**Conflict.** The `SKIN` envelope preserves fourteen references in order, the
`THRU_CURVE` envelope preserves nine references in order, and both operations'
branch groups preserve modes, state lanes, members, and terminal references.
These fields use shared reference grammars and provide no relation assigning
sections, guides, continuity, or terminal controls. Scalar pairs and strings
are payload-owned and do not add those roles.

### OM-27. `Studio Surface` construction roles

**Question.** Which ordered `Studio Surface` references and branch groups select control geometry, continuity, and terminal controls?

**Known.** `siemens_nx.md` §7.1 "`SKIN` and `Studio Surface` payloads share one exact common construction-reference envelope." and `siemens_nx.md` §7.1 "The `Studio Surface` operation label identifies a freeform-surface construction." define the surface construction envelope, ordered references, logical payload, and feature family.

**Need.** We must know the roles to construct the neutral freeform surface.

**Conflict.** `Studio Surface` shares the fourteen-reference envelope and
counted branch grammar with `SKIN`; the operation label selects the family but
does not label individual references or branch members. No serialized field
assigns control geometry, continuity, or terminal semantics.

### OM-29. `RMFastLoad` class records

**Question.** What is the per-class entity-record grammar in `RMFastLoad` outside its object-ID membership table?

**Known.** `siemens_nx.md` §2.1 "| `/Root/FastLoad/RMFastLoad`" and `siemens_nx.md` §2.3 "`/Root/FastLoad/Structure` begins with the twelve-byte envelope" define the fast-load object-ID table and the bounded structure stream. The per-class entity-record grammar outside the membership table remains unresolved.

**Need.** We must know the class grammars to transfer the remaining fast-load state as typed data.

### OM-37. Final field declaration in a pointerless OM section

**Question.** What terminates the final member-field declaration in a section without a unique valid record-area pointer?

**Known.** `siemens_nx.md` §7.1 "The first record at `oid_end` begins `04 01, declared_len:u8, version_text[declared_len-2], 00`" defines the product record and the bounded registry suffix through the next length-framed `m_` declaration. A section-relative `u32 LE` word after the type registry is a record-area pointer only when its forward target remains inside the section, starts with the three control words and product record, and is unique. When valid, that pointer bounds the complete field registry.

**Need.** We must know the terminal marker or alternate boundary for the final field declaration when no valid record-area pointer exists. A pointerless section cannot establish a complete field registry from the settled byte structure alone.

### OM-38. `SWP104` leading construction roles

**Question.** Which sweep parameters and selections do the four scalars, mode, counted member references, state lane, and terminal reference encode?

**Known.** `siemens_nx.md` §7.1 "The `SWP104` operation label identifies a sweep-family construction." and §7.1 "A bounded `SWP104` payload begins with one construction branch" define the exact leading branch, independent declared and witnessed counts, ordered references, state-lane bounds, and terminal marker.

**Need.** We must identify the profile, path, result mode, orientation, transition, transformation, twist, and scale fields required for a neutral sweep feature.

**Conflict.** Serialized order, count relations, and offset-store identities do not assign semantic roles to the retained values or references.

### OM-39. `SHELL` construction roles

**Question.** Which serialized bodies, opening faces, thickness, side, offset mode, corner join, and intersection policies define a `SHELL` operation?

**Known.** `siemens_nx.md` §7.1 "The `SHELL` operation label identifies a thin-wall shell construction." defines the feature family and retains the neutral shell definition with unresolved construction fields. The operation record, body-history relations, and topology graph remain independently available.

**Need.** We must identify the complete construction-role relation to populate the shell input bodies, removed faces, wall thickness, outward side, offset mode, corner join, and intersection policies.

**Conflict.** The operation label identifies the shell family but does not assign roles to its object-index fields, body-write output, or result topology. Output body ownership does not identify the pre-operation bodies or opening faces, and the current scalar and reference lanes do not provide a unique thickness, side, mode, join, or intersection-policy witness.

### OM-40. `ENLARGE` construction roles

**Question.** Which serialized input faces, extension law, extent, and copy or associativity controls define an `ENLARGE` operation?

**Known.** `siemens_nx.md` §7.1 "The `ENLARGE` operation label identifies a surface-enlarge construction." defines the feature family and retains the unresolved surface-extension definition. The operation has a body-write relation in the admitted feature-history grammar.

**Need.** We must identify the complete construction-role relation to populate the selected faces, extension law, extent, and copy or associativity controls.

**Conflict.** The operation label and body-write output identify the surface-enlarge family and result body, but the current object-index and payload lanes do not assign the pre-operation face set or the extension and copy controls. Result topology does not identify those authored roles.

### OM-41. `EXTRACT_FACE` construction roles

**Question.** Which serialized faces and associativity, copy, and sew controls define an `EXTRACT_FACE` operation?

**Known.** `siemens_nx.md` §7.1 "The `EXTRACT_FACE` operation label identifies a face-extraction construction." defines the family and retains its dedicated unresolved neutral definition. A resolved body-write relation retains a sheet result body.

**Need.** We must identify the complete source-face and result-control relation to construct an extract-face operation.

**Conflict.** The operation label and resolved sheet result identify the family, but the operation object-index lanes and body-write relation do not assign source faces or associativity, copy, or sew controls. Result sheet ownership does not identify the authored source selection.

## 3. Assembly and material data

### AM-01. Fast-load structure stream

**Question.** What is the field grammar and semantics of `/Root/FastLoad/Structure`?

**Known.** `siemens_nx.md` §2.3 "`/Root/FastLoad/Structure` begins with the twelve-byte envelope" and `siemens_nx.md` §7.1 "UG_PART begins with a 12-byte row table" define the big-endian bounded OM envelope and typed class and member declarations. The typed component roster defines two exact candidate anchors, ordered named prototypes and a one-based prototype index for each distinct occurrence. A second counted table stores UUID identities, and a parallel one-based index associates every occurrence with one UUID. Other payload fields remain uninterpreted.

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

**Known.** `siemens_nx.md` §2.2 "`/Root/UG_PART/LastSavedToggleInfoStream` is one atomic payload:" defines the complete counted stream envelope and retains each 32-hex-digit identity and `On`/`Off` state exactly. A toggle identity that occurs once in the stream has an order-independent identity witness; duplicate identities have no such witness. The member count is independent of the feature-operation-label count. The toggle identities have no proven join to feature-operation records.

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

### AM-08. Residual `EXTREFSTREAM` tail fields

**Question.** What are the field boundaries and roles of the residual bytes in an indexed `EXTREFSTREAM` record tail?

**Known.** `siemens_nx.md` §9.1 "An `EXTREFSTREAM` record region begins with" and `siemens_nx.md` §9.1 "Slot zero names the child `.prt`, slot one is the reference code," define the indexed-record boundary, handle-set prefix, persistent-handle pairs, tagged references, string uses, and child binding. Other tail bytes remain opaque.

**Need.** We must know the fields to decode complete occurrence and external-reference state.

### AM-09. SDL/TYSA attribute values

**Question.** How does each Parasolid SDL/TYSA attribute instance assign its referenced value records to the fields declared by its type-79 class definition?

**Known.** `siemens_nx.md` §9.4 "A shell, face, loop, edge, FIN, or vertex topology record with one uniquely resolved" and `siemens_nx.md` §9.4 "When a value resolves without a unique declared-field assignment, its neutral" define attribute-class declarations, type-81 class selection, referenced value records, topology ownership, and neutral source-attribute names. The declaration includes ordered field type codes such as those for `SDL/TYSA_DENSITY` and `SDL/TYSA_BLEND_ID`. The specification assigns the two fields of `SDL/TYSA_DENSITY` by name. Every other class falls back to the zero-based declared field ordinal and declared field code.

**Need.** We must know the assignment to transfer class-specific material and topology attributes with semantic field names.

The closure test only exercises already-populated `ParasolidAttributeFieldUse` values; it does not show the raw SDL/TYSA records assigning those values. The ordinal/code fallback in the production mapper remains unsupported by an independent serialized witness, so this item stays open.

### AM-10. Physical material bindings

**Question.** Which serialized relation binds a physical material to a Parasolid face identity?

**Known.** `siemens_nx.md` §2.3 "Each `/Root/materialsTif/<name>` file entry contains one TIFF stream." and `siemens_nx.md` §9.4 "The type-81 definition reference selects an attribute class when it equals" define preview and texture assets, the material-texture catalog, and topology-owned Parasolid attributes. `siemens_nx.md` §7.1 "An explicit display-color assignment addresses a face when" defines the complete face appearance relation; a palette color is not a physical-material assignment.

**Need.** We must know the relation to assign physical-material state to neutral faces without treating a display color, texture asset, or topology attribute as a material identity.
