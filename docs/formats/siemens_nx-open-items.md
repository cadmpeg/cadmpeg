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

### PS-01. Deletion target relation

**Question.** Once an enclosing deletion frame is established, which target
relation applies when its explicit node key does not match a partition node?

**Known.** `siemens_nx.md` §4.2 "**Deletion framing:** complete enclosing fields own their bytes." separates byte ownership, target identity, and change state. `siemens_nx.md` §7.2 "A deletion relation requires an established node frame, exact target key," requires the owning change. A descending reference/type-map run does not establish deletions.

**Need.** Establish the deletion grammar in PS-28 and the target's partition
and pmark scope before applying any unmatched key.

### PS-02. Terminal procedural-intersection branches

**Question.** How does a terminal type-38 procedural intersection select a finite branch when it has no usable support-parameter lane or paired FIN-carried `SP_CURVE` witnesses?

**Known.** `siemens_nx.md` §6.3 "NX stores freeform edges and blend rails as construction relations with branch witnesses." `siemens_nx.md` §6.3 "The chart/start-term/end-term witness slots" and `siemens_nx.md` §6.3 "A FIN-carried SP_CURVE is a serialized terminal-branch witness" define procedural-intersection framing, support relations, endpoint witnesses, and accepted exact branches.

**Need.** We must know the selection rule to construct the terminal curve as exact geometry.

### PS-03. Coincident terminal parameters

**Question.** Which branch does a terminal procedural intersection use when distinct endpoints map to one procedural-curve parameter?

**Known.** `siemens_nx.md` §6.3 "The chart/start-term/end-term witness slots" and `siemens_nx.md` §6.3 "A FIN-carried SP_CURVE is a serialized terminal-branch witness" define endpoint witnesses and ordered procedural-curve branches. They do not define this folded terminal case.

**Need.** We must know the branch to preserve both endpoints and the curve orientation. The count-1 `H?` and `T?` forms and the count-2 `T?` form also need ownership and branch-admission rules; a complete finite Hvec lane alone does not establish either relation.

**Note.** A terminal singularity and its nearby branch selector have distinct serialized native parameters. Accumulated chart chords can fall on either side of the selector parameter and do not establish the singularity parameter. Complete physical LIMIT field roles are defined in `siemens_nx.md` §6.3 "**LIMIT (`0x29`, inserted field `term_use`)** records contain a leading count,".

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

**Known.** `siemens_nx.md` §4.2 "A deltas stream is a schema-framed incremental edit log paired with a partition." and `siemens_nx.md` §4.2 "After complete typed node frames, complete enclosing packets, and complete" define the admitted deltas record families and their byte boundaries. A remaining byte region has no typed node grammar.

**Need.** We must know each grammar to delimit and transfer the remaining deltas records as typed data.

### PS-08. BODY-associated change-state packets

**Question.** Which complete enclosing grammar separates a deltas BODY
instance from adjacent rollback or change-state packets?

**Known.** `siemens_nx.md` §4.2 "A BODY instance follows its effective field list." bounds the node through its declared fields. `siemens_nx.md` §7.2 "The WORLD `body` pointer identifies the head of the current BODY chain." defines the BODY chain head and current pmark identity. A recognized successor does not bound an otherwise unestablished state tail.

**Need.** Identify each packet's start, complete field cardinalities, and
relation to the owning pmark edge before assigning body change-state roles.

### PS-13. Deltas type 90 `GROUP`

**Question.** Which Deltas sequence establishes ownership of a status-framed type-90 GROUP, and what change-state role does its final reference status carry?

**Known.** `siemens_nx.md` §4.2 defines the GROUP's five pointer roles, member-family enum 1 through 10, and separate primary and supported Deltas field framing. A primary GROUP ends after `first_member` without a status byte. Its source boundary follows the effective schema cursor.

**Need.** Establish each Deltas instance's enclosing sequence and reference-status semantics before treating every field-shaped candidate as a group event. Resolve membership through separately owned MEMBER_OF_GROUP records and their distinct group and entity chains.

### PS-16. Deltas type 141

**Question.** What is the canonical node-type name of deltas type 141 (`008d`), and what is the role of each reference field?

**Known.** `siemens_nx.md` §4.2 "Type 141 has the complete deltas record" defines the four-reference status-framed grammar and boundary.

**Need.** We must know the name and reference roles to relate the record to its owner and operands.

### PS-18. Deltas tagged-reference lanes

**Question.** Which record owns each deltas tagged-reference lane, and what does each field mean?

**Known.** `siemens_nx.md` §4.2 "After complete typed node frames, complete enclosing packets, and complete" defines the tagged-reference lane grammar, record-kind values, and exact byte identity.

**Need.** We must know ownership and field roles to attach each lane to the correct typed record.

### PS-19. Deltas reference/type maps

**Question.** Which record owns each deltas reference/type map, and what does each entry mean?

**Known.** `siemens_nx.md` §4.2 "A reference/type map begins with either" defines the counted reference and type lanes and their byte boundary.

**Need.** We must know ownership and entry roles to apply the map to the correct entities.

### PS-22. Deltas reference-marker packets

**Question.** Which record owns each deltas reference-marker packet, and what do its references and marker mean?

**Known.** `siemens_nx.md` §4.2 "One reference-marker packet is" defines the packet grammar, marker values, and boundary.

**Need.** We must know ownership and marker semantics to attach the packet to typed state.

### PS-23. Deltas type-150 state packets

**Question.** What does each field and marker in a deltas type-150 state packet mean?

**Known.** `siemens_nx.md` §4.2 "A type-150 state packet is" defines the packet grammar, marker values, and boundary.

**Need.** We must know the roles to transfer the packet as typed type-150 state.

### PS-25. Inline type-12 `BODY` instance state

**Question.** What are the dynamic roles and constraints of the BODY fields `attributes_features`, `lattice`, `mesh`, `polyline`, `boundary_lattice`, `boundary_mesh`, `boundary_polyline`, `child`, `lowest_node_id`, and `mesh_offset_data`?

**Known.** `siemens_nx.md` §4.2 "The BODY effective field order with both lattice fields is:" defines the complete effective-field order and the 27-, 29-, 34-, 35-, and 36-field lists. The following paragraphs define pointer cardinalities, complete instance length, geometry and topology chain heads, resolution scalars, body and nominal-geometry enums, and indexed-transmit roles. Section 3.2 defines field extent and the first-instance count and index. Field names, pointer classes, XMT identities, and status bytes define the storage of the remaining fields.

**Need.** We must know how the remaining links and the lower node-id field constrain the body and its referenced nodes to assign their semantic roles.

### PS-28. Deletion frame and byte extent

**Question.** Which enclosing grammar establishes a deletion event, its exact
byte extent, and its target identity?

**Known.** `siemens_nx.md` §4.2 "**Deletion framing:** complete enclosing fields own their bytes." excludes deletion admission from a six-byte shape alone. `siemens_nx.md` §4.2 "A reference/type map begins with either" defines the complete map envelope. Opposite-aligned extended reference/type entries can form a descending apparent deletion run. A type-91 six-reference shape with a four-byte control of 2 also spans an apparent six-byte candidate; the role and admission of that control remain unestablished.

**Need.** Establish complete enclosing state and a discriminator for short and
extended deletion forms. Keep bounded rival interpretations separate from a
certified deletion grammar. Neither six-byte nor eight-byte extent follows
from the apparent tag and XMT alone.

### PS-29. Aggregate pmark-edge selection

**Question.** How does an aggregate deltas transmit bind each change's source
and target pmarks to the owning partition's current state?

**Known.** `siemens_nx.md` §7.2 "A delta contains a unidirectional change between two adjacent pmarks." defines a delta edge separately from the aggregate. `siemens_nx.md` §7.2 "The WORLD `body` pointer identifies the head of the current BODY chain." defines the current BODY and pmark roots. `highest_node_id` bounds kernel node identifiers; physical BODY order, XMT 3, and numerical resets do not identify a revision interval.

**Need.** Identify each constituent delta's complete enclosing frame, source
pmark, target pmark, and partition ownership. Apply only edges that lead to
the selected current state, including branches, undone changes, and multiple
bodies. Keep an unestablished edge selection unresolved. This includes repeated type-38 intersection states: first-use and subsequent record spelling must not select their chart or carrier relations.

### PS-33. MESH FILE chart layout

**Question.** Which layout, if any, applies to `CHART_s` and `LIMIT` Hvec rows in a `MESH FILE` payload?

**Known.** The counted `TRANSMIT FILE` description distinguishes 24-byte transmitted point rows from 88-byte expanded Hvec rows. A `MESH FILE` description does not select either form.

**Need.** Serialized MESH FILE chart or terminal nodes with independently established physical boundaries and field declarations are required to determine their Hvec representation.

**Conflict.** None.

**Note.** Preserve the payload without assigning a Hvec layout from a broad plain-stream classification.

### PS-35. Sequential Deltas record ownership

**Question.** Which complete Deltas grammar advances the node cursor through revision controls, changed fields, and shared status bytes while preserving first-use schema state?

**Known.** Section 3.2 defines the stream-local embedded-schema ledger and the node-sequence owner. An unchanged type consumes its schema-match byte only at first use. A later `ff` can begin a direct extended index. Partition and plain streams use the complete declared field sequence; nested type-shaped bytes have no independent ownership. Local field completeness, graph completion, reference joins, and a following boundary do not select either reading of `ff`.

**Need.** Define the complete enclosing Deltas sequence, including transitions between control records and status-framed instance fields. The sequence must establish first-use state before admitting either local interpretation and must retain independent state for each transmit stream.

### PS-37. Basis ranges within counted NURBS storage

**Question.** Which values in a counted multiplicity or knot node belong to a descriptor's active basis when the storage count exceeds its distinct-knot count?

**Known.** `siemens_nx.md` §9.3 defines four-byte variable-node counts and the 16-bit degree and multiplicity fields. §6.2 gives the degree, pole-count, and expanded-knot cardinality constraints. A complete counted storage lane can extend beyond the number of values selected by a descriptor. Periodic storage can include imaginary knots outside its active interval.

**Need.** Establish the active range and the role of excess stored values before applying basis cardinality or individual multiplicity bounds to an entire storage node. Distinguish active basis values from padding and other retained values through the owning descriptor and schema.

## 2. Object model and body composition

### OM-01. Per-class OM field serialization

**Question.** What value-record grammar and semantic role does each declared NX OM member use?

**Known.** `siemens_nx.md` §3.3 "Class and member declarations before the boundary array use" defines registry token widths and values. `siemens_nx.md` §3.3 "Registry owner tokens are one-based class ordinals." defines the same-section class join. `siemens_nx.md` §3.3 "A storage code does not identify one member." defines declaration identity independently of storage codes and repeated names. `siemens_nx.md` §7.1 "The first record at `oid_end` begins" defines the reference, class, and member regions. `siemens_nx.md` §3.3 "A numeric expression table contains a `hostglobalvariables` root entity." defines selected typed value records.

**Need.** We must identify the remaining value-record boundaries, field types,
and semantic roles for feature history, constraints, attributes, and material
bindings. File-declared class and member lists do not supply those value
schemas.

**Conflict.** Registry storage codes, owner ordinals, and declaration names
identify registry metadata. They do not define a field's value-record grammar
or assign its semantic role.

### OM-02. `SKETCH` scalar-pair geometry

**Question.** What geometric quantities and coordinate spaces do the framed scalar pairs in a `SKETCH` construction payload represent?

**Known.** `siemens_nx.md` §2 "An operation label equal to `SKETCH` denotes a sketch history operation." `siemens_nx.md` §7.1 "A sketch payload scalar field is", `siemens_nx.md` §7.1 "A sketch repeated-type scalar pair is", `siemens_nx.md` §7.1 "A sketch fixed pair has one of four exact forms:", and `siemens_nx.md` §7.1 "A sketch scalar-vector lane is" define sketch record identity and the framed payload lanes. In the exact four fixed-pair branches, each `30` marker plus seven-byte suffix is a shifted binary64 atom whose emitted value is scaled by `1/4`; the first atom of the mixed branch uses the same rule. Shifted binary64 atoms are admitted only for the marker ranges defined in `siemens_nx.md` §7.1; other leading bytes do not form shifted binary64 values. The remaining lanes have no assigned model-space frame, sketch entity, or constraint role.

**Need.** We must identify the entity roles and coordinate space of the scalar fields, repeated pairs, coordinate pairs, scalar-vector lanes, and mixed-pair binary32 value to construct neutral sketch geometry and constraints. A named two-scalar record also requires independent axis, unit, and placement relations before it supplies a solved sketch point. The fixed-pair second value and the relationship between the named point records and sketch geometry require a typed owner or controlled differential.

### OM-03. `DATUM_PLANE` scalar-pair geometry

**Question.** What geometric quantities and coordinate spaces do the framed scalar pairs in a `DATUM_PLANE` construction payload represent?

**Known.** `siemens_nx.md` §7.1 "A `DATUM_PLANE` payload begins" and `siemens_nx.md` §7.1 "A datum-plane object scalar-pair frame is" and `siemens_nx.md` §7.1 "A datum-plane descriptor block is exactly 40 bytes:" define datum-plane branches, resolved blocks, scalar-pair framing, descriptors, and feature identity.

**Need.** We must know one complete relation that assigns the framed scalar
pairs and descriptor or object records to a model-space origin, unit normal,
and in-plane axis.

**Conflict.** The control and branch bytes are section-local class selectors;
the following descriptor and object indices select store blocks. These selectors
and references do not assign plane-frame roles. Descriptor blocks carry an identity,
schema index, and label; scalar-pair frames carry two values. Equal descriptor identities
between `DATUM_PLANE` and `DATUM_CSYS` records do not assign either operation
as the source of an origin, axis, or normal.

### OM-04. `DATUM_CSYS` scalar-pair geometry

**Question.** What geometric quantities and coordinate spaces do the framed scalar pairs in a `DATUM_CSYS` construction payload represent?

**Known.** `siemens_nx.md` §7.1 "A `DATUM_CSYS` payload begins", `siemens_nx.md` §7.1 "The logical payload formed by the first two resolved `DATUM_CSYS` blocks uses", `siemens_nx.md` §7.1 "An object-payload scalar-pair frame is", and `siemens_nx.md` §7.1 "A datum-coordinate-system fixed pair has one of two exact forms:" define the eight-reference construction lane, logical payload, optional numeric frames, and feature identity.

**Need.** We must know one complete relation that assigns the payload scalar
fields and pair records to a model-space origin and three orthonormal axes.

**Conflict.** The eight construction references define ordered block identity,
and the first two blocks define one logical payload. Scalar fields, scalar
pairs, fixed pairs, and descriptor lanes have separate frames and no serialized
role relation. Column-row reuse and equal descriptor identity establish block
or history relations only; they do not assign an origin or axis. Complete numeric
frames can also occur outside a coordinate-system payload. The frame bytes and
field codes alone do not select an origin coordinate or an axis component.

### OM-06. OM registry signature and storage semantics

**Question.** How is the eight-byte class signature constructed, and what semantic domains do class and field storage values represent?

**Known.** `siemens_nx.md` §3.3 "A complete `UGS::` class declaration has one storage token, one base token," and §3.3 "The class reference token is a one-based ordinal into the maximal contiguous" define the complete token grammar, opaque signature, section-local reference join, and independently framed plain-name fields.

**Need.** We must know the complete inputs and algorithm for the class signature and the serialized or runtime relations that assign a storage value to type, cardinality, visibility, access, or object-record binding. A controlled class-registration trace or primary schema definition must supply those relations.

**Conflict.** A class name can have different signatures, so the name alone does not determine the bytes. Equal storage values and names do not identify one declaration or establish a value grammar. The reference ordinal and field owner ordinal provide structural joins only.

### OM-08. Blend-created faces

**Question.** How does a blend construction identify the faces created by that
blend?

**Known.** A nested operation body-write frame retains the persistent body
identity, partition-local GROUP node, endpoint tag, and write-state body-image
object. Endpoint tags `10`, `12`, and `15` select the body-image field.
The `BLEND` and `FACE_BLEND` labels with body-write frames identify edge-blend
and face-blend operations. When the owned circular blend-carrier projection is
complete, the corresponding neutral family and constant radius are available.
The edge or face selections and created-face relation remain separate
questions. GROUP member XMTs are checked against the exact same-family node in
the delta-merged partition graph before the guarded unique-node fallback; this
resolves current topology identities without assigning creation provenance.
Explicit Boolean operations independently require the body-image object to
equal the target and exclude every tool. A closed GROUP chain resolves current
members by topology family and kernel node identity and transfers current FACE,
EDGE, and VERTEX identities to the feature result state. A labeled or unlabeled
write whose GROUP records resolve to exactly one partition binds its persistent
body identity to that partition. When the partition has one body, every labeled
write with that identity writes that body. Independently, a unique plain-stream
alias equal to the persistent body identity binds that write to the unique body
in the selected stream without requiring body-image block resolution.
Parasolid topology attribute ownership is independently resolved from a valid
attribute-list head and the first leading reference of each type-81 record; the
third leading reference is not required for that ownership relation.

The structural integer-key relation in `siemens_nx.md` §9.4 "The named
`UG2/CMOD_FEAT_ATTRIBUTE` instances with one trailing reference carry" links a
unique GROUP-owned FEAT scalar to the first integer of same-partition,
FACE-owned FACR pairs. It retains every matching instance and the second
integer without assigning creation direction.

**Need.** A controlled before/after construction must distinguish newly
introduced faces from faces that retain or later acquire the same key. We must
also determine the second FACR integer's identity domain and stability across
operation and topology edits.

**Conflict.** GROUP membership and FEAT/FACR equality do not establish creation
direction. Several FACR instances can belong to one FACE, and second integers
repeat across instance scopes. Missing current values or unmatched faces do not
prove absence from an earlier construction state.

### OM-10. Operation suppression fields

**Question.** How do the embedded operation state lanes encode suppression?

**Known.** `siemens_nx.md` §2 derives active state from the closed output-and-dependency graph and admits retained-history primary-body and partition-scoped body-write closure witnesses when neutral output projection cannot bind an operation to the selected body. `siemens_nx.md` §7.1 defines the feature-history journal, status rows, diagnostic messages, `m_rollForwardStates` groups, and object state-counter map. An audit-trail section has a distinct role-gated complete-row grammar and the native decoder retains its ordinal, optional selector, timestamp, tagged value, exact row bytes, and source offsets. The native decoder retains exact status codes and payloads, message values and count/severity words, group rows, and counter-map rows. The three retained Drone tables support a counter-row ordinal endpoint domain with 261 in-range uses; list positions and the counter rows' object-index values belong to separate lanes. Status code `41` with plain payload is the built, normal state. The first three common-frame state bytes remain an untyped operation-state prefix; bytes 3 through 7 are legacy module inactivity, Parasolid-data mutation, split tracking, and group count. The saved-toggle stream carries independent toggle identities and states. The Parasolid `UGS/ObjectState` class is a one-field code-3 character attribute whose value is an ordinary type-84 string; its owner, when resolved, is a topology attribute-list relation. The OM registry classes `UGS::OM::ObjectStateCollection` and `UGS::OM::ObjectState` are distinct declarations. The `m_objectStateCollection` member has declaration code `78` in both UG_PART and RMFastLoad. That code is not a section-local class ordinal: the UG_PART class ordinals differ, and RMFastLoad does not declare the collection class. The product-anchored control form retains an aligned u32 value lane whose in-range values address same-store data blocks; it does not declare class references. An object record retains its object identifier and bounded bytes, but no class binding or typed field value.

**Need.** We must know the serialized suppression fields to construct operation state for all configurations.

**Conflict.** Neither the common-frame state lane nor the saved-toggle stream identifies a feature suppression value. A status row identifies an object and retains a code, but the code has no suppression meaning by itself. A roll-forward relation row retains two counter-row ordinal endpoints in the worked Drone tables, but a counter-row ordinal does not identify an operation or a typed suppression value. The operation identity, state-object identity, and typed state value require separate unique joins. The `78` member code does not supply the missing class reference. The OM declarations and product-anchored value lane do not provide a relation from a class declaration to an object record or from an object record to a typed state value. The Parasolid `UGS/ObjectState` lane provides a character value and, when present, a topology owner, but no operation owner or suppression domain.

**Note.** Independent replay covers all 229 retained input dispositions and
62,359 emitted counter rows across 172 successful production decodes. The three
worked Drone files remain production-limit refusals; their complete raw group
and counter tables are verified independently without claiming a successful
whole-model decode. The older private all-files-decode census does not establish
that current production limits admit those files. The 217 successful decodes
emit no roll-forward groups; group census completeness remains unresolved for
those products.

### OM-11. `DELETE` construction-payload fields

**Question.** Which fields in the concatenated `DELETE` construction payload encode its operands and construction parameters?

**Known.** `siemens_nx.md` §2 "A `DELETE` payload begins with one nullable reference field" defines the counted nullable field, canonical reference encodings, column-fragment resolution, and concatenation in field order. The explicit fragment count is the count byte minus one; five- and six-fragment forms use `06` and `07`. No explicit fragment identifies the separate primary-body target.

**Need.** Identify complete inner payload frames, their field widths, and their operand relations before projecting delete-construction parameters.

**Conflict.** Physical column boundaries split inner frames. Consecutive block ordinals and physical adjacency do not establish field boundaries. A repeated inner header does not identify operands or delimit every following field.

**Note.** The worked spur-gear-housing payload repeats `2d fb 7c df d9 d7 bd bb 00 00 00 50 28 00 00 2a` at payload-relative offsets `0`, `177`, and `354`. Its production decode remains a resource refusal. Directory-bounded offset-table candidates without the admitted product frame require a separate store-grammar proof; monotonic offsets alone do not establish an OM store.

### OM-12. Arrangement-specific component state

**Question.** Which bounded native carriers join an assembly, arrangement, and component occurrence to the suppression-control flag, suppression state, optional controlling expression, and arrangement-specific positioning constraints?

**Known.** `siemens_nx.md` §2 "`/Root/part/arrangements` has an `Arrangements` root." defines arrangement identity, default flag, and child order. The unique part-owned `NX_Arrangement` string attribute forms an active-name identity relation. Arrangement state has component occurrences as subjects; expressions remain in the part's current global parameter graph. Positioning constraints can have arrangement-specific state in assemblies.

**Need.** We must know the carrier framing, identity references, suppression-control flag, suppression-state discriminator, expression reference, and positioning-constraint relation to construct arrangement-specific component state.

**Conflict.** Arrangement XML has no component-state or expression-reference fields. The current B-rep and feature closure have no arrangement key. Active-name agreement supplies identity without component membership or suppression.

### OM-15. `CPROJ` construction-reference roles

**Question.** Which `CPROJ` construction references select the source curve, target surface, direction, and combination controls?

**Known.** `siemens_nx.md` §7.1 "A `CPROJ` construction-reference lane begins with" defines the complete counted reference lane, block resolution, and logical payload. `siemens_nx.md` §7.1 "The first payload byte of a `CPROJ` or `CPROJ_CMB` operation" defines the same-section parameter-class and hidden-normal pair-element class indices.

**Need.** We must know the roles to construct a neutral projected curve.

**Conflict.** The counted lane references use one canonical encoding and resolve only
to offset-store blocks. The repeated header index and fixed suffix delimit the lane,
but do not identify a source curve, target surface, direction, or combination
control. Reconstructed strings remain payload-owned and have no independent
role relation. A parameter-class name does not identify a physical projection direction or bind a reference to an operand. The eight-byte scalar in the hidden-normal header has no established tolerance role, unit, or field-specific scale.

### OM-16. `CPROJ_CMB` construction-reference roles

**Question.** Which `CPROJ_CMB` construction references select the source curves, target surfaces, directions, and combination controls?

**Known.** `siemens_nx.md` §7.1 "A `CPROJ_CMB` payload contains at most one construction-reference graph framed as" defines both complete storage graphs, serialized reference occurrences, block resolution, and logical payload.

**Need.** We must know the roles to construct the combined projected curves.

**Conflict.** The branch lanes prove repeated-anchor equality and preserve
branch order, while the tail preserves two additional references. Repeated references select the same storage fragment. All reference
occurrences still resolve only to offset-store blocks; no field
marker assigns curve, surface, direction, or combination semantics.

### OM-17. `FSET` selection roles

**Question.** What does the `FSET` selector mean, what selection role does each ordered object-reference group have, and which target namespace and role do the counted compact indices use?

**Known.** `siemens_nx.md` §7.1 "An `FSET` operation payload contains at most one two-group reference graph framed as" defines the selector, two separate reference groups, resolution rule, and logical payloads. `siemens_nx.md` §7.1 "A prefix-anchored `FSET` index lane is" defines the separate counted compact lane and explicit empty spelling.

**Need.** We must know the roles to construct the selected face or feature set.

**Conflict.** The printable selector is retained as an opaque value, and the
two groups are distinguished only by their serialized first/second position.
Every reference uses the same object-index form and resolves only to an
offset-store block. No selector vocabulary, target schema, or relation to a
face or feature set assigns either group a selection role. Compact values that equal operation ordinals do not identify member features. An empty byte lane does not establish an empty neutral selected set.

### OM-18. Pattern construction-reference roles

**Question.** Which serialized relations assign seed, transform, and control roles within the logical pattern construction payload, separate counted reference lane, and `Geometry Instance` storage field?

**Known.** `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one ten-slot construction-reference graph in one of two exact layouts." defines both graphs and their ordered logical payload. `siemens_nx.md` §7.1 "`Pattern Feature` payloads contain at most one counted reference lane." defines the separate counted lane. `siemens_nx.md` §7.1 "A `Geometry Instance` reference addresses one offset-store block." defines its storage target without an instance-record boundary.

**Need.** Identify the inner field framing, semantic target namespace, and relation to counted rows and operation inputs before constructing neutral pattern dependencies and transforms.

**Conflict.** Graph slots address storage fragments. Physical block boundaries split inner fields, and a `Geometry Instance` target can begin or end inside a recurring inner frame. Graph slot order, repeated anchors, and terminal slots do not assign seed or transform roles. A counted reference can share an operation-input target without identifying that input as a seed or defining its relation to a transform row.

### OM-19. Pattern-row scalar roles

**Question.** Which owning-pattern relations assign each counted instance-row scalar its physical role, units, scale, and reference frame?

**Known.** `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one transform lane." and `siemens_nx.md` §7.1 "`Pattern Feature` also admits the wide-row form" define shifted binary scalar encodings, exact values, row order, and boundaries. `siemens_nx.md` §7.1 "The pattern row schema index addresses the owning OM section class declaration" defines the instance-class identity.

**Need.** Identify the owning pattern definition, scalar field roles, unit and scale rules, reference frame, and meaning of the fifth wide-row cell before constructing neutral pattern coordinates or controls.

**Conflict.** The instance class and terminal modes select object identity and byte layout; they do not assign pitch, span, angle, axis, or coordinate roles. Equality with an expression or a final-geometry coordinate does not bind a row to that expression or geometric instance. A rotation-matrix identity in four wide-row cells does not assign the fifth cell. Counted-row selectors have no resolved instance or frame relation.

### OM-20. Pattern-row selector roles

**Question.** Which owning-pattern relation maps a compact row key to an instance, and what do keys outside ordinary instance numbering mean?

**Known.** `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one transform lane." defines exact unsigned compact keys and explicit row ordinals. The same-section row class is `UGS::ModlUtils::PatternInstance`. Ordinary keys can number repeated instances from two with the master numbered one. The separate counted reference lane retains its own ordered object indices.

**Need.** Identify the owning-pattern key domain and serialized key-to-instance relation before assigning seed, transform, or output operands. Establish the interpretation of zero and extended keys without converting the compact-index field to a signed integer or importing the two public API index domains.

**Conflict.** Compact keys can differ from row ordinal plus one and recur in distinct operation-owned lanes. Numeric equality with a data-block, OM object, operation, or fast-load identity does not establish a typed reference. No serialized relation joins the counted reference lane to these keys or binds either lane to construction-graph roles.

### OM-21. `Multi Instance Output` roles

**Question.** Which serialized relations bind source-topology tags, target-label indices, and master-to-instance map handles to concrete output geometry?

**Known.** `siemens_nx.md` §7.1 "`Multi Instance Output` payloads contain at most one counted output lane" defines exact rows and token retention. The following class-index and instance-map rules identify a topology-tracking table, source-topology tags, target-label indices, complete repeated-instance ordinal groups, and ordered instance-map handles. The table's reserved count slot is separate from the master instance.

**Need.** Resolve the source-tag and target-label namespaces, the map-owned handle namespace, and a serialized handle-to-instance ordinal relation before mapping each pattern instance to bodies or geometry.

**Conflict.** A source-topology tag is not a demonstrated Parasolid FACE identity. Target-label indices do not identify final FACE order. A map handle has no serialized ordinal byte and is not resolved by numeric equality with an OM block, fast-load object, feature operation, body identity, body-image index, GROUP node, or partition-local XMT identity. Creator and face-relation attributes establish feature-local topology populations without joining individual table rows to faces.

### OM-22. Pattern/profile named-target roles

**Question.** Which relation assigns a shared pattern/profile construction target its seed or member role and pairs the individual named members that address it?

**Known.** `siemens_nx.md` §7.1 "A canonical named-target envelope begins with" defines exact names bound to canonical object references in reconstructed pattern and witnessed extrusion profile payloads. Its block resolution and ownership rules establish a construction-level association through an exact shared resolved target. Equal names in separate fields do not establish reference identity.

**Need.** Identify the shared target's construction role and an individual member relation, with a controlled change that distinguishes renaming, suppression, instance order, and target ownership.

**Conflict.** Different names on each side can address the same target. A profile name can lack the complete target envelope while an equal pattern name carries it. Shared target identity and preceding feature order do not identify a seed, pair names, or make physical construction-block adjacency an object relation.

### OM-23. `POINT` header mode

**Question.** Which authored property distinguishes the `02` and `03` alternatives of the `POINT` construction header?

**Known.** `siemens_nx.md` §7.1 "The `POINT` operation payload begins with one construction header." and "The header reference selects a six-scalar lane ending in its addressed offset-store block." define the complete grammar, canonical reference, opaque mode, and exact lane ownership. The reference selects the lane's end block, while the lane crosses a physical block boundary.

**Need.** Vary construction method and Smart Object update settings independently, with repeated operations in both orders, to distinguish authored settings from occurrence or allocation order.

**Conflict.** The mode byte carries no property identity. A shared column can end one lane and begin another; a reference from another construction to its predecessor establishes storage overlap without typing the mode or creating an operation input.

### OM-24. `POINT` scalar triples

**Question.** What coordinate spaces and construction roles do the two ordered scalar triples in the selected six-scalar `POINT` lane use?

**Known.** `siemens_nx.md` §7.1 "The header reference selects a six-scalar lane ending in its addressed offset-store block." defines the complete lane, ordered values, raw encodings, and exact boundaries. Its header selects storage and retains an opaque mode. Neither field supplies a coordinate-space or construction-role relation.

**Need.** Vary the authored point, each construction help point, name location, and datum coordinate system independently while recording the NXOpen construction and update settings before save.

**Conflict.** Byte continuity and matching triples elsewhere in a store do not establish model space, units, an evaluated point, a help point, or a name location. A topology export can omit datum geometry and cannot identify a native construction role from numeric equality alone.

### OM-25. `DRAFT` construction roles

**Question.** Which counted leading indices, ordered references, terminal indices, and tail fields select the drafted faces, neutral plane, pull direction, and draft angle?

**Known.** `siemens_nx.md` §7.1 "The `DRAFT` operation payload begins" and `siemens_nx.md` §7.1 "The same payload contains exactly one four-reference construction graph." define the leading index lane, four-reference graph, terminal lanes, scalar encodings, store resolution, and exact boundaries.

The eighteen-byte `25 25 41` discriminator frames complete shifted-IEEE
binary64 atoms. Its `30` and `b0` markers carry the sign and exponent;
physical block boundaries do not delimit an atom. This encoding establishes
numeric values without assigning coordinate space, units, or draft roles.

**Need.** We must know the roles to construct a neutral draft operation.

**Conflict.** The leading lane, four-reference graph, identity frames,
scalar lanes, and terminal indices are separately framed and store-resolved,
but no field-role relation assigns faces, plane, pull direction, angle, or
termination semantics. Shared store identity and serialized order establish
construction ownership only.

### OM-26. `SKIN` and `THRU_CURVE` construction roles

**Question.** Which complete `SKIN` and `THRU_CURVE` branch allocations select ordered section strings, and which control bytes encode alignment, continuity, construction, patch, and result settings?

**Known.** `siemens_nx.md` §7.1 "The `SKIN` and `THRU_CURVE` operation labels identify loft-family constructions.", §7.1 "A `THRU_CURVE` payload begins with the exact construction-reference envelope", §7.1 "The envelope is followed by one counted branch group.", and §7.1 "`SKIN` and `Studio Surface` payloads share one exact common construction-reference envelope." define the loft family, exact envelopes, counted branches, and ordered storage references. Complete `THRU_CURVE` and `SKIN` allocations form consecutive block runs when all envelope, member, and terminal references are taken in serialized order. Their physical boundaries can split structured rows and scalar atoms, including the member/terminal boundary. Branch declared counts include their terminal references; the `THRU_CURVE` group count includes an implicit owner slot. The NXOpen ThroughCurvesBuilder has an ordered SectionsList and separate alignment, continuity, construction, patch, and body-preference controls. Its API has no guide collector. The API does not assign native branch or control-byte roles.

**Need.** We must know the roles to construct the neutral loft surface or body.

**Conflict.** The `SKIN` envelope preserves fourteen references in order, the
`THRU_CURVE` envelope preserves nine references in order, and both operations'
branch groups preserve modes, state lanes, members, and terminal references.
The positional allocation and branch ordinal provide storage order but no
relation assigning section strings, continuity, or terminal controls. The
envelope-reference slots are not all one consecutive reference-only run.
Scalar pairs and strings are payload-owned and do not add those roles. The
operation-owned data blocks may still contain selections of existing curve
objects, so allocation ownership does not establish neutral curve identity.

**Note.** Change one authored section or setting at a time and query the NX
builder before save. Compare complete allocations and all controls. An output
body relation or an empty interchange sibling does not identify input curves.

### OM-27. `Studio Surface` construction roles

**Question.** Which ordered `Studio Surface` references and branch groups select control geometry, continuity, and terminal controls?

**Known.** `siemens_nx.md` §7.1 "`SKIN` and `Studio Surface` payloads share one exact common construction-reference envelope." and `siemens_nx.md` §7.1 "The `Studio Surface` operation label identifies a freeform-surface construction." define the common envelope, counted branches, eight post-branch storage references, complete consecutive allocation, and feature family. Physical block boundaries can split structured rows, tokens, and scalars. Their storage order does not identify input curves.

**Need.** We must know the roles to construct the neutral freeform surface.

**Conflict.** `Studio Surface` shares the fourteen-reference envelope and
counted branch grammar with `SKIN`; the operation label selects the family but
does not label individual references or branch members. No serialized field
assigns control geometry, continuity, or terminal semantics.

**Note.** Create controlled native variants that change one continuity setting, add one section, add one guide, or reverse one string independently. Query the authored section and guide collectors before save. Compare the complete operation and allocation. Body-write relations select outputs; incomplete output topology and an empty interchange sibling supply no input-ownership witness.

### OM-38. `SWP104` logical construction roles

**Question.** Which logical fields assign authored construction roles to the four scalars, counted column sequence, and independent state lane?

**Known.** `siemens_nx.md` §7.1 "A bounded `SWP104` payload begins with one construction branch" defines one-based parameter and profile class indices. §7.1 "Member and successor references are non-null canonical payload object indices." defines the consecutive physical column sequence. Its block boundaries do not delimit logical fields. The witness count is independent of the member count.

**Need.** Decode the logical rows across column boundaries and join their native endpoints to the profile, path, result mode, orientation, transition, transformation, twist, and scale fields required for a neutral feature.

**Conflict.** Parameter-class identity, physical adjacency, and equal scalar values do not identify the authored operands or distinguish dormant construction policies.

### OM-39. `SHELL` construction roles

**Question.** Which serialized bodies, opening faces, thickness, side, offset mode, corner join, and intersection policies define a `SHELL` operation?

**Known.** `siemens_nx.md` §7.1 "The `SHELL` operation label identifies a thin-wall shell construction." defines the feature family and retains the neutral shell definition with unresolved construction fields. The leading owning-section class selector names `UGS::HOLLOW_PARMS`; the body-write endpoint selector names `UGS::CM_BREF_DATA` through the same one-based registry. The operation record, body-history relations, and topology graph remain independently available.

**Need.** We must identify the complete construction-role relation to populate the shell input bodies, removed faces, wall thickness, outward side, offset mode, corner join, and intersection policies.

**Conflict.** The operation label identifies the shell family but does not assign roles to its object-index fields, body-write output, or result topology. Output body ownership does not identify the pre-operation bodies or opening faces, and the current scalar and reference lanes do not provide a unique thickness, side, mode, join, or intersection-policy witness. The first shifted-binary64 field is not a wall-thickness witness. Six following variable-width atoms and two binary controls lack assigned semantic roles. The four-byte atom reading must be established for this field family; the assembly-matrix and sketch-field readings do not supply that assignment.

### OM-40. `ENLARGE` construction roles

**Question.** Which serialized input faces, extension law, extent, and copy or associativity controls define an `ENLARGE` operation?

**Known.** `siemens_nx.md` §7.1 "The `ENLARGE` operation label identifies a surface-enlarge construction." defines the feature family and retains the unresolved surface-extension definition. The bounded native construction prefix retains a one-based `UGS::MODIFYSRF_isotrim_parms` class index, six ordered unresolved storage indices, a separate middle index, and four zero or shifted-binary64 scalar atoms. Its following body-write relation identifies the result.

**Need.** We must identify the complete construction-role relation to populate the selected faces, extension law, extent, and copy or associativity controls.

**Conflict.** The operation label and body-write output identify the surface-enlarge family and result body, but the native construction indices and scalar lane do not assign the pre-operation face set or the extension and copy controls. Result topology does not identify those authored roles.

### OM-41. `EXTRACT_FACE` construction roles

**Question.** Which serialized faces and associativity, copy, and sew controls define an `EXTRACT_FACE` operation?

**Known.** `siemens_nx.md` §7.1 "The `EXTRACT_FACE` operation label identifies a face-extraction construction." defines the family and retains its dedicated unresolved neutral definition. Its bounded native prefix retains the one-based `UGS::ModlFeature::ExtractFaceParms` class selector, four tagged words, two ordered counted relation lanes, terminal index, navigator text, and complete span. A resolved body-write relation retains a sheet result body.

**Need.** We must identify the complete source-face and result-control relation to construct an extract-face operation.

**Conflict.** The operation label and resolved sheet result identify the family, but the ordered construction revision lanes and body-write relation do not assign source faces or associativity, copy, or sew controls. Lane endpoints join owner-local operation-state ordinals where the complete state table is present; neither lane is a literal result-face or result-edge list. Result sheet ownership does not identify the authored source selection.

### OM-42. `SYMBOLIC_THREAD` axial extent and unresolved face relations

**Question.** Which serialized relation assigns the axial thread extent, and how are faces identified when the checked placement witness has no unique cylindrical candidate?

**Known.** `siemens_nx.md` §7.1 "The `SYMBOLIC_THREAD` operation label identifies" defines the family, the checked duplicate coordinate witness, nominal diameter designation, and unique physical-cylinder selection. Complete profile, designation, and mode text frames and exact construction reference tokens remain native. The operation has no body output relation.

**Need.** We must identify the operation-to-dimension relation that assigns axial extent, the roles of the construction reference tokens and older inline scalar, and any authored face relation needed when geometry or unique ownership is unavailable.

**Conflict.** A selected face's axial span can differ from the thread extent. The coordinate witness and nominal diameter do not identify an extent dimension. Ambiguous tuple boundaries, absent or incomplete geometry, placed bodies, and multiple physical candidates preserve unresolved selection rather than choosing an arbitrary face.

### OM-43. `DATUM_AXIS` construction roles

**Question.** Which serialized relation assigns a complete model-space line and its source/reference roles to each `DATUM_AXIS` or `EXTRACT_DATUM_AXIS` operation?

**Known.** `siemens_nx.md` §7.1 "The `DATUM_AXIS` and `EXTRACT_DATUM_AXIS` operation labels identify" defines the family, owning construction classes, column-storage limits, and owner-local state relation. These history features have no admitted body output or body-write bridge. Their neutral axis definitions remain unresolved.

**Need.** Identify the operation-to-geometry relation, finite origin and direction, authored source/reference roles, and orientation controls. Any association with semantic lines in a sibling representation must establish identity independently of list order or analytic proximity.

**Conflict.** Direct input indices can address column fragments shared with earlier operations and split inside serialized values. Extracted-axis selectors collide among distinct persistent identities and also occur on datum-plane and string-extraction records. Semantic centre lines can outnumber extracted-axis operations and need not coincide with cylinder or cone axes. None of these relations uniquely assigns a line to an operation.

### OM-44. `BRIDGE_CURVE` construction roles

**Question.** Which serialized relation assigns ordered source curves, end-condition modes, and continuity controls to each `BRIDGE_CURVE` operation?

**Known.** `siemens_nx.md` §7.1 "The `BRIDGE_CURVE` operation label identifies" defines the family, owning construction-class chain, and storage/reference limits. The history features retain unresolved neutral construction without admitted body outputs or body-write bridges.

**Need.** Identify operation-to-curve ownership, ordered endpoint/source identities, end-condition and continuity modes, direction reversal, tangent magnitude, shape controls, and any independently assigned fit or tolerance quantities.

**Conflict.** The seven varying construction references select physical column fragments whose boundaries can split rows. Terminal references can reuse one column across distinct operations. Both equal `0.04` atoms also occur in generic operation headers, so they do not independently identify two endpoint quantities. Long record bounds can include global lifecycle rows. Shared boundary curves have two perpendicular incident-curve candidates at each endpoint; sheet-local topology attributes differ between copies. Neither list order, geometric coincidence, nor these attributes assign authored source ownership or end modes.

### OM-45. `FILL_HOLE` construction roles

**Question.** Which serialized relation assigns a source-hole boundary, fill geometry, continuity, and healing controls to each `FILL_HOLE` operation?

**Known.** `siemens_nx.md` §7.1 "The `FILL_HOLE` operation label identifies" defines the family, owning constructor class, exact body-write relation, counted storage references, and variable-width scalar continuation. A body result is retained when independently resolved; it can be shared with a `SEW` operation without identifying a fill patch or source loop.

**Need.** Reconstruct the complete logical relation across selected column chunks, join its source and result to independently identified pre/post topology, and assign boundary membership, fill geometry, continuity, and healing controls. Establish any physical role and transform for the scalar triple independently.

**Conflict.** The header and payload selectors address column fragments; boundaries can split a scalar, row, or hexadecimal identifier. Member selectors are duplicated, and later selector groups permute contiguous storage ranges. The post-write tuple also occurs in other operation families, can be copied across many raw locations, and has no direct equality with decoded geometry anchors. The shared body lineage and unjoined GROUP values do not identify an authored hole boundary or patch.

### OM-46. `MOVE_FACE` construction roles

**Question.** Which logical face-recognition and motion relations establish the authored face selection and motion law of a `MOVE_FACE` operation?

**Known.** `siemens_nx.md` §7.1 "The `MOVE_FACE` operation label identifies" defines the three owning component classes, exact core framing, ordered body/GROUP/image result list, and following-lane boundary. Component references select column fragments when their offset store resolves; an unjoined reference is not null. Result GROUP members belong to result topology.

**Need.** Reconstruct the logical component rows across physical chunks, establish the motion representation and its values, and join authored source faces to independently identified pre/post topology. Assign move, heal, paste, and step options through serialized relations.

**Conflict.** Selected chunk boundaries split identifiers, framed rows, and duplicated tokens. Serialized body/image order establishes result lineage, not an authored source selection. Local control 5/6 changes framing without an independent motion-mode assignment. Nearby expressions and matching list lengths do not bind a motion magnitude. A following `CM_object` lane is outside the complete core and is not motion data.

### OM-47. `CYLINDER` construction roles

**Question.** Which logical relations bind a cylinder's dimension expressions and authored placement to its operation?

**Known.** `siemens_nx.md` §7.1 "The `CYLINDER` operation label identifies" defines the complete native core, six signed or quarter-scaled scalar slots, nine payload references, result association, and create/unite/subtract mode. The mode is retained as `cylinder.boolean_mode`. Full-cylinder output carriers establish a result radius and can establish an untrimmed cap span. A result direction does not establish an authored axis.

**Need.** Reconstruct the logical rows selected by the payload references and establish unique operation-to-dimension and operation-to-placement relations.

**Conflict.** Payload references can select physical column fragments. Header indices, fixed-table words, and inline expression declaration values belong to distinct namespaces and do not supply a generic dimension join. A direction-like scalar triple can disagree with the output cylinder axis. Later trimming can remove the cap pair needed to recover an original height from the result.

### OM-48. `MOVE_OBJECT` construction roles

**Question.** Which logical motion relations establish selected objects, authored translation and rotation, move/copy mode, and copy count?

**Known.** `siemens_nx.md` §7.1 "The `MOVE_OBJECT` operation label identifies" defines the complete core, scoped `MotionOptions` reference, result body write, and null-terminal boundary. The reference can select a physical fragment whose boundary splits a scalar or row. Result lineage can establish a same-body dependency between operations. Analytic carrier matching can establish a net rigid transform.

**Need.** Reconstruct logical motion rows across storage fragments, bind the selected objects and authored parameters to each operation, and distinguish move from copy through serialized fields.

**Conflict.** A net half-turn can be decomposed into rotation about an offset axis or rotation about a global axis plus translation. Multiple operations can share one matched source/result transform. A degree expression equal to the net angle has no operation binding by that equality. A null terminal excludes the following `MethodicObject` lane. An API default or invariant control byte does not establish serialized copy mode or count.

### OM-49. `COPY_FACE` construction roles

**Question.** Which serialized relations bind the selected source faces, authored motion, target body, paste mode, and reverse direction?

**Known.** `siemens_nx.md` §7.1 "The `COPY_FACE` operation label identifies" defines the complete core, ordered label map, signed recipe atoms, scoped construction references, and result body write. Header and payload references select storage fragments. Result geometry and topology tags can establish a face-set correspondence and a net rigid transform.

**Need.** Bind authored selection and motion to the operation, distinguish paste and reverse controls, and identify a target body when the operation uses one.

**Conflict.** A result-face tag need not use the source-face namespace. Equal expression or recipe values do not bind an authored parameter. A shared result GROUP and column index can have equal numbers in different namespaces. The face-copy API does not establish serialized associativity or sew controls.

### OM-50. `LINKED_FACE` construction roles

**Question.** Which serialized fields select associativity and creation-timestamp versus final-source-body update?

**Known.** `siemens_nx.md` §7.1 "The `LINKED_FACE` operation label identifies" defines the named source-body, source-face and WAVE-context tables, source-handle relation, destination part, position-dependence, and exact retained payload. The link creates a sheet body in the destination part; a target body to modify is not required. Body writes can remain unresolved or bind to streams without decoded face topology.

**Need.** Assign associativity and timestamp-update controls through changed native values with independent policy evidence. Assign the unparsed payload tail and recover missing result topology.

**Conflict.** An invariant `Rules` string is not an associativity Boolean. A timestamp value is not a timestamp-policy Boolean. A numeric equality with a segment object index is insufficient when the persistent identity belongs to the alias namespace. A source-body hexadecimal handle does not follow the narrow decimal source-face sequence rule.

### OM-51. `THRU_CURVE_MESH` construction roles

**Question.** Which serialized relations assign the authored curve networks, primary/cross directions, four continuity controls, and requested result preference?

**Known.** `siemens_nx.md` §7.1 "The `THRU_CURVE_MESH` operation label identifies" defines the complete core, two ordered profile groups, physical-fragment references, and exact named `body_id` result relation. An exact decoded body establishes result kind and boundary geometry. A joined partition without decoded topology leaves its result kind unresolved.

**Need.** Reconstruct logical construction rows across fragment boundaries and bind source curves, order, directions, continuity objects and constraint faces. Distinguish a requested preference from the resulting body kind.

**Conflict.** Two storage indices are not two curve objects. Geometric boundary directions do not name native groups primary or cross. G0-compatible geometry and public G0 defaults do not assign a serialized control byte. A sheet result does not prove an explicit Sheet preference. A count-only match must not replace an unresolved partition result with an unclaimed plain stream.

### OM-52. `CONE` construction roles

**Question.** Which serialized relations bind the authored dimensions and base-point placement of a `CONE` operation?

**Known.** `siemens_nx.md` §7.1 "The `CONE` create core selects" defines the complete native create frame, six scalar slots, thirteen payload references, one auxiliary reference, result association and repeated ordinal. Source property `cone.boolean_mode` retains create mode. Result carriers and circular boundaries establish cone geometry independently of authored construction bindings.

**Need.** Bind compact references and storage fragments to unique dimension expressions and a base-point object. Distinguish the builder direction from the cone carrier's canonical axis.

**Conflict.** Numeric ordinal selections can cover both cone and unrelated expressions. A carrier half-angle is not a sweep angle, and a result point or axis does not establish its serialized construction relation. An association index equal to one expression index plus one does not define an expression selector.

### OM-53. Body-writing thread construction roles

**Question.** Which serialized fields define the selected faces, thread standard, pitch, handedness, axial extent, and physical or cosmetic result mode of `THREADS` and `DETAILED_THREAD` operations?

**Known.** `siemens_nx.md` §7.1 "The `THREADS` and `DETAILED_THREAD` complete cores select" defines the complete native frame, inline text fields, reference columns, body-write association and inherited state lanes. The construction roles are not assigned by these storage boundaries.

**Need.** We must identify the selected cylindrical faces, thread designation, pitch, handedness, axial extent, and physical or cosmetic result mode to construct a complete neutral thread operation and bind its result body.

**Conflict.** File-level thread-law bundles do not establish operation-to-law ownership. Numeric namespace equality does not establish selected faces. A continuation selected as `UGS::OM::OptTagValues` after a terminal object has no complete admitted grammar.

### OM-54. Body-writing `SPHERE` construction roles

**Question.** Which serialized relations bind a body-writing `SPHERE` operation to its construction type, center object and diameter expression?

**Known.** `siemens_nx.md` §7.1 "The `SPHERE` subtract core selects" defines the complete native frame, six scalar slots, five payload references, packed result-association token, secondary body relation and counted inherited-state branches. Source property `sphere.boolean_mode` retains subtract mode. The analytic result carrier supplies center and radius; face loops define trimming.

**Need.** Join the native references to unique center and diameter inputs and distinguish center-plus-diameter construction from construction through an arc. Assign inherited controls only through a discriminating native relation.

**Conflict.** Several expressions can equal the effective diameter. Six direction scalars do not supply a center, and a hemispherical face does not define an authored half-sweep. Numeric table ordinals and object identities are separate namespaces; no generic binding follows from their equality.

### OM-55. `DELETE FACE` construction roles

**Question.** Which serialized fields define the selected faces and healing policy of a body-writing `DELETE FACE` operation?

**Known.** `siemens_nx.md` §7.1 "The `DELETE FACE` complete core selects" defines the owning parameter and manager classes, scalar and packed lanes, body-write association and both counted inherited-state branches. The base class is `UGS::FEATURE_PARMS`; it is not `UGS::OM::Set`.

**Need.** We must identify the selected faces and healing policy to construct a complete neutral delete-face operation.

**Conflict.** The label and body-write relation identify the family, but the admitted object-index and payload lanes do not assign a unique face selection or healing-policy value. Encoding a default healing value would invent operation state.

### OM-57. `MIRROR_FACE` construction roles

**Question.** Which serialized fields define the source faces, mirror plane, and copy or replace mode of a body-writing `MIRROR_FACE` operation?

**Known.** `siemens_nx.md` §7.1 defines a body-writing `MIRROR_FACE` record as a body-affecting face-mirror construction. The label and body-write frame establish the family, and the result topology retains the body identity, but the bounded object-index slots do not assign source-face, mirror-plane, or result-mode roles.

**Need.** We must identify the source faces, mirror plane, and result mode to construct a complete neutral face-mirror operation and bind its result body.

**Conflict.** The operation label and body-write relation identify the family, but the admitted object-index and payload lanes do not assign unique source-face, mirror-plane, or copy-or-replace roles.

### OM-58. `SUBDIVISION_BODY` construction roles

**Question.** Which serialized fields define the source body and subdivision roles of a body-writing `SUBDIVISION_BODY` operation?

**Known.** `siemens_nx.md` §7.1 defines the operation label, body-write frame and owning parameter-class selector. The bounded header has four null object-index slots. The parameter class and its construction, mesh-constraint, settings and connection siblings declare no members. The admitted object, parameter and named-attribute relations assign no source body or subdivision controls. The body-identity segment use identifies a unique plain stream, but that stream has no decoded neutral body.

**Need.** A serialized source-body relation and subdivision controls, plus complete result-body storage that can be admitted as a neutral body.

**Conflict.** The operation label, owning class and result-stream identity do not assign source-body or subdivision roles. A result-stream binding does not establish a decoded neutral body.

### OM-59. `TOPOLOGY_OPTIMIZATION` construction roles

**Question.** Which serialized fields define the input bodies, optimization objective, and result mode of a body-writing `TOPOLOGY_OPTIMIZATION` operation?

**Known.** `siemens_nx.md` §7.1 defines a body-writing `TOPOLOGY_OPTIMIZATION` record as a body-affecting topology-optimization construction. The record carries two input blocks and a body-write frame, but the admitted block references and payload lanes do not assign their roles.

**Need.** We must identify the input bodies, optimization objective, and result mode to construct a complete neutral topology-optimization operation and bind its result body.

**Conflict.** The operation label, input blocks, and body-write relation identify the family, but the bounded block and payload lanes do not assign unique input, objective, or result-mode roles.

### OM-60. `RMFastLoad` physical words and active membership

**Question.** Which complete physical word arrays carry active topology membership when the membership-table admission does not select them?

**Known.** `siemens_nx.md` §7.1 locates the complete physical boundary and word arrays from the header span count and unique monotone boundary structure. §7.2 defines the separately admitted count-and-identity table and its shared FACE, EDGE, and VERTEX identity space. Physical framing alone does not assign active status to a table word.

**Need.** A discriminating membership relation for complete physical arrays without an admitted product-adjacent identity table, including the rule for empty or inactive sets.

**Conflict.** The physical span count locates storage; it does not identify a topology kind or establish active membership. A product boundary and a topology registry declaration constrain the admitted identity table but do not assign semantics to every complete physical array.

### OM-61. `RMFastLoad` opaque storage roles

**Question.** What do header word `h[1]` and the terminal triple tokens, preamble, and trailer encode?

**Known.** `siemens_nx.md` §7.1 "`RMFastLoad` uses entry-relative physical column storage." defines the unique boundary array and complete terminal triple table. Boundaries can split complete product strings and logical rows. The terminal and class-registry counts are independent; none of the three token positions supplies a universal section-local class selector.

**Need.** Identify independently discriminated logical fields and their native identity relations. Preserve opaque bytes until a complete row supplies the required selector or relation.

### OM-62. Plain declarations after the final complete member head

**Question.** Which independent boundary delimits a plain-name declaration lane after the final complete `m_` head?

**Known.** `siemens_nx.md` §3.3 "The complete member head ends immediately after the owner token." defines the self-delimiting head. A bounded lane between consecutive member heads can contain independently framed plain-name declarations whose owner tokens join same-section classes.

**Need.** Identify an independent bound and an exact lane grammar for plain-name declarations after the final head.

**Conflict.** A complete head proves its own end. It does not prove where a following plain-name declaration lane ends. A complete-looking next declaration alone does not establish that lane.

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

**Question.** Which serialized relation binds each placement frame to one child occurrence, and which field defines its translation unit?

**Known.** `siemens_nx.md` §2.3 "An assembly placement frame in `/Root/UG_PART/UG_PART` begins with" defines the complete matrix and text frame. `siemens_nx.md` §2.3 "The native placement retains" excludes an ordinal join by source order.

**Need.** We must identify the child occurrence and translation unit to apply the matrix to external child geometry.

**Conflict.** Repeated UUIDs identify groups of placement records. They do not select a distinct repeated child occurrence. The five-byte trailer has no established instance or unit role.

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
### AM-11. JT 9 high-degree lane count

**Question.** What field gives the number of high-degree face-attribute-mask lanes in a JT 9 topologically compressed representation?

**Known.** `siemens_nx.md` §2.3 "The JT 9 topologically compressed representation begins with Int32 Compressed Data Packet Mk. 2" defines the fixed prefix packets, the split packets, the vertex-record header agreement test, and the rule that exactly one lane count must satisfy it. It states that one or more high-degree lanes occur. It gives no count field and no maximum.

**Need.** We must know the field to frame the packet sequence directly. The decoder tries lane counts from one to sixty-four and keeps the unique count that satisfies the agreement test. Sixty-four is not a format bound. A representation that carries more lanes matches no count, and the decoder then drops the topology, vertex-record, and coordinate-array data and every mesh derived from them.

**Note.** The closure removed the `1..=64` ceiling and scans until a unique vertex-header agreement in `crates/cadmpeg-codec-nx/src/native/display_jt.rs:817-911`, but a corpus JT representation has not yet verified that the packet stream has no count field or maximum. The regression fixture was constructed with sixty-five lanes to exercise the new scan. The lane-count rule remains unsupported, so this item is reopened.

## 4. Test evidence
