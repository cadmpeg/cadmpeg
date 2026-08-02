# Siemens NX `.prt` Open Items

This document lists the parts of the Siemens NX `.prt` format that we do not know. The specification `siemens_nx.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Parasolid streams

### PS-01. Unmatched compact tombstones

**Question.** What target relation does a compact tombstone use when its explicit `(type, xmt)` key does not match a partition entity?

**Known.** `siemens_nx.md:1516` through `siemens_nx.md:1522` define exact-key deletion and chronological replacement in the final BODY revision. `siemens_nx.md:1544` states that deletion runs can span record families.

**Need.** We must know the target relation to apply an unmatched tombstone without deleting the wrong entity.

### PS-02. Terminal procedural-intersection branches

**Question.** How does a terminal type-38 or `0x5a` procedural intersection select a finite branch when it has no usable support-parameter lane or paired FIN-carried `SP_CURVE` witnesses?

**Known.** `siemens_nx.md:913` through `siemens_nx.md:1024` define procedural-intersection framing, support relations, endpoint witnesses, correction, and accepted exact branches.

**Need.** We must know the selection rule to construct the terminal curve as exact geometry.

### PS-03. Coincident terminal parameters

**Question.** Which branch does a terminal procedural intersection use when distinct endpoints map to one procedural-curve parameter?

**Known.** `siemens_nx.md:913` through `siemens_nx.md:1024` define endpoint witnesses and ordered procedural-curve branches. They do not define this folded terminal case.

**Need.** We must know the branch to preserve both endpoints and the curve orientation.

### PS-04. Degenerate procedural-support correction

**Question.** How does correction select terminal folds and multiple corrected branches for a degenerate support-0 array or a sentinel-truncated marker-4 plane-support array?

**Known.** `siemens_nx.md:913` through `siemens_nx.md:1024` define predictor inversion and correction for complete supported procedural curves.

**Need.** We must know the correction rule to construct these curves without an arbitrary branch choice.

### PS-05. NURBS-offset spine support identity

**Question.** Which saved carrier owns a NURBS-offset spine when its graph-only offset support has no established saved-carrier identity?

**Known.** `siemens_nx.md:913` through `siemens_nx.md:1024` define procedural support graphs. `siemens_nx.md:1050` through `siemens_nx.md:1107` define blend spines and saved carrier relations.

**Need.** We must know the identity relation to evaluate and transfer the spine as exact geometry.

### PS-06. Other deltas node families

**Question.** What complete record grammar and semantics does each deltas node family that the specification does not define use?

**Known.** `siemens_nx.md:487` through `siemens_nx.md:731` define the admitted deltas record families and their byte boundaries. A remaining byte region has no typed node grammar.

**Need.** We must know each grammar to delimit and transfer the remaining deltas records as typed data.

### PS-07. Deltas transmit-header XMT identities

**Question.** What is the semantic role of each of the two consecutive XMT identities in a deltas transmit header?

**Known.** `siemens_nx.md:491` through `siemens_nx.md:498` define the header grammar and require two non-null consecutive identities.

**Need.** We must know the roles to relate the header identities to the deltas schema and body history.

### PS-08. Deltas BODY state tail

**Question.** What fields and semantics does the bounded state tail after the eight-reference deltas `BODY` prefix contain?

**Known.** `siemens_nx.md:1495` through `siemens_nx.md:1505` define the prefix, tail boundary, revision counter, and current-revision rule.

**Need.** We must know the tail fields to transfer the complete body revision state.

### PS-09. Delta tag `0x5a` name

**Question.** What is the canonical later-schema node-type name for delta tag `0x5a`?

**Known.** `siemens_nx.md:485` and `siemens_nx.md:517` through `siemens_nx.md:520` define tag `0x5a` as the `intersection_data` layout shared with type 38.

**Need.** We must know the name to give the node one stable schema identity.

### PS-10. Deltas type 45

**Question.** What is the canonical node-type name of deltas type 45 (`002d`), and what does each value mean?

**Known.** `siemens_nx.md:555` through `siemens_nx.md:566` define its count-selected binary64 grammar and record boundary.

**Need.** We must know the name and value roles to transfer the record as typed Parasolid state.

### PS-11. Deltas type 70

**Question.** What is the canonical node-type name of deltas type 70 (`0046`), and what does each field mean?

**Known.** `siemens_nx.md:548` through `siemens_nx.md:553` define its XMT identity, node ID, reference lanes, count, constants, and boundary.

**Need.** We must know the name and field roles to relate the record to its owner and referenced entities.

### PS-12. `ATTDEF_LIST` sentinel reference

**Question.** What is the semantic role of the leading sentinel reference in deltas type 74 `ATTDEF_LIST`?

**Known.** `siemens_nx.md:568` through `siemens_nx.md:575` define the record, active-count rule, sentinel value, slots, and boundary.

**Need.** We must know the role to represent the complete attribute-definition list without an untyped reference.

### PS-13. Deltas type 90 `GROUP`

**Question.** What does each of the five references and the `02|04` mode in deltas type 90 `GROUP` mean?

**Known.** `siemens_nx.md:577` through `siemens_nx.md:583` define the complete record grammar, reference statuses, mode values, and boundary.

**Need.** We must know the roles to construct group membership and ownership relations.

### PS-14. Deltas type 91

**Question.** What is the canonical node-type name of deltas type 91 (`005b`), and what does each field mean?

**Known.** `siemens_nx.md:508` through `siemens_nx.md:515` define its XMT identity, binary flag, six status-framed references, and boundary.

**Need.** We must know the name and field roles to transfer the record as typed Parasolid state.

### PS-15. Deltas type 101

**Question.** What is the canonical node-type name of deltas type 101 (`0065`), and what does each field mean?

**Known.** `siemens_nx.md:522` through `siemens_nx.md:524` define its boundary precedence. `siemens_nx.md:585` through `siemens_nx.md:593` define its complete field grammar.

**Need.** We must know the name and field roles to transfer the record as typed Parasolid state.

### PS-16. Deltas type 141

**Question.** What is the canonical node-type name of deltas type 141 (`008d`), and what is the role of each reference field?

**Known.** `siemens_nx.md:541` through `siemens_nx.md:546` define the four-reference status-framed grammar and boundary.

**Need.** We must know the name and reference roles to relate the record to its owner and operands.

### PS-17. `term_use` numeric tail

**Question.** What does each binary64 value in the count-selected deltas `term_use` tail mean?

**Known.** `siemens_nx.md:526` through `siemens_nx.md:539` define the tail start, count-to-cardinality rule, finite values, and independent byte identity.

**Need.** We must know the value roles to transfer the complete terminal-use state.

### PS-18. Deltas tagged-reference lanes

**Question.** Which record owns each deltas tagged-reference lane, and what does each field mean?

**Known.** `siemens_nx.md:595` through `siemens_nx.md:603` define the lane grammar, tag values, references, statuses, and exact boundary.

**Need.** We must know ownership and field roles to attach each lane to the correct typed record.

### PS-19. Deltas reference/type maps

**Question.** Which record owns each deltas reference/type map, and what does each entry mean?

**Known.** `siemens_nx.md:605` through `siemens_nx.md:613` define the counted reference and type lanes and their byte boundary.

**Need.** We must know ownership and entry roles to apply the map to the correct entities.

### PS-20. Deltas four-reference state packets

**Question.** Which record owns each deltas four-reference state packet, and what is the role of each reference?

**Known.** `siemens_nx.md:615` through `siemens_nx.md:622` define its reference-status grammar and exact boundary.

**Need.** We must know ownership and reference roles to attach the packet to typed state.

### PS-21. Deltas schema reference preambles

**Question.** Which declaration owns each deltas schema reference preamble, and what is the role of each field?

**Known.** `siemens_nx.md:624` through `siemens_nx.md:635` define the preamble variants, reference lanes, state bytes, and boundary.

**Need.** We must know ownership and field roles to construct the complete inline schema model.

### PS-22. Deltas reference-marker packets

**Question.** Which record owns each deltas reference-marker packet, and what do its references and marker mean?

**Known.** `siemens_nx.md:637` through `siemens_nx.md:645` define the packet grammar, marker values, and boundary.

**Need.** We must know ownership and marker semantics to attach the packet to typed state.

### PS-23. Deltas type-150 state packets

**Question.** What does each field and marker in a deltas type-150 state packet mean?

**Known.** `siemens_nx.md:647` through `siemens_nx.md:654` define the packet grammar, marker values, and boundary.

**Need.** We must know the roles to transfer the packet as typed type-150 state.

### PS-24. Inline deltas schema declarations

**Question.** What is the semantic role of each field in an inline deltas schema declaration?

**Known.** `siemens_nx.md:656` through `siemens_nx.md:707` define the declaration variants, names, signatures, references, states, and byte boundaries.

**Need.** We must know the field roles to construct a typed schema declaration instead of a framed declaration record.

### PS-25. Inline type-12 `BODY` instance state

**Question.** What does each field in inline type-12 `BODY` instance state mean, and how do its counts constrain its lanes?

**Known.** `siemens_nx.md:709` through `siemens_nx.md:723` define its reference lanes, count fields, scalar lanes, state bytes, and boundary.

**Need.** We must know the roles and cardinality rules to transfer the complete inline body state.

## 2. Object model and body composition

### OM-01. Per-class OM field serialization

**Question.** What byte grammar and semantic role does each declared field of each NX OM class use?

**Known.** `siemens_nx.md:415` through `siemens_nx.md:461` define OM section boundaries, class and member declarations, store identities, compact indices, and expression records. `siemens_nx.md:1111` through `siemens_nx.md:1486` define typed fields for selected construction families.

**Need.** We must know the remaining class grammars to decode feature history, constraints, attributes, and material bindings as typed fields.

### OM-02. `SKETCH` scalar-pair geometry

**Question.** What geometric quantities and coordinate spaces do the framed scalar pairs in a `SKETCH` construction payload represent?

**Known.** `siemens_nx.md:238` through `siemens_nx.md:243` define sketch record identity. Section 7.1 defines the framed payload lanes but does not assign a model-space frame, sketch entity, or constraint role from equal scalar values.

**Need.** We must know the roles to construct neutral sketch geometry and constraints.

### OM-03. `DATUM_PLANE` scalar-pair geometry

**Question.** What geometric quantities and coordinate spaces do the framed scalar pairs in a `DATUM_PLANE` construction payload represent?

**Known.** `siemens_nx.md:1451` through `siemens_nx.md:1480` define datum-plane branches, resolved blocks, scalar-pair framing, descriptors, and feature identity.

**Need.** We must know the roles to construct the model-space reference plane.

### OM-04. `DATUM_CSYS` scalar-pair geometry

**Question.** What geometric quantities and coordinate spaces do the framed scalar pairs in a `DATUM_CSYS` construction payload represent?

**Known.** `siemens_nx.md:1435` through `siemens_nx.md:1449` define the eight-reference construction lane, logical payload, scalar-pair framing, and feature identity.

**Need.** We must know the roles to construct the model-space coordinate-system frame.

### OM-05. OM declaration trailing code

**Question.** What does the trailing byte in an OM class or member declaration mean?

**Known.** `siemens_nx.md:435` through `siemens_nx.md:440` define the declaration length, `UGS::` or `m_` name, trailing-code boundary, and following registry suffix.

**Need.** We must know the meaning to validate and transfer declaration metadata.

### OM-06. OM registry suffix fields

**Question.** What does each byte in a bounded OM field-registry suffix mean?

**Known.** `siemens_nx.md:435` through `siemens_nx.md:440` define the suffix boundary and the 11-to-14-byte prefix, fingerprint, and terminal-byte decomposition.

**Need.** We must know the field roles to construct the complete OM schema registry.

### OM-07. Offset-store body to segment-image relation

**Question.** How does a primary feature body field that resolves to an offset-store block identify a segment body-image object-index pair?

**Known.** `siemens_nx.md:122` through `siemens_nx.md:137` define segment body-image tuples and prohibit a relation based only on equal integer values across namespaces. `siemens_nx.md:208` through `siemens_nx.md:228` define primary-body fields and body selection.

**Need.** We must know the cross-store relation to attach the feature output and lineage to the correct body image.

### OM-08. Other feature-history object relations

**Question.** What relation does each feature-history object index that is not a primary-body writer or Boolean tool use?

**Known.** `siemens_nx.md:163` through `siemens_nx.md:228` define operation-header inputs, shared-block identity groups, primary-body lineage, and Boolean operands.

**Need.** We must know each remaining relation to construct complete feature dependencies and selections.

### OM-09. Embedded operation common-frame ownership

**Question.** Which operation owns each embedded common frame, and what does each field in its eight-byte state lane mean?

**Known.** Section 7.1 of `siemens_nx.md` defines the exact frame and state-lane boundaries for the admitted operation families.

**Need.** We must know ownership and field roles to attach the state to the correct operation.

### OM-10. Operation suppression fields

**Question.** How do the embedded operation state lanes encode suppression?

**Known.** `siemens_nx.md:68` through `siemens_nx.md:80` derive active state for the closed output-and-dependency graph and leave suppression outside that graph unresolved.

**Need.** We must know the serialized suppression fields to construct operation state for all configurations.

### OM-11. `DELETE` nullable-reference roles

**Question.** What object family can each of the five leading nullable references in a `DELETE` payload address, and what is the role of each slot?

**Known.** `siemens_nx.md:230` through `siemens_nx.md:236` define the five-slot field, canonical reference encodings, resolution rule, logical payload, and body-target exclusion.

**Need.** We must know the slot roles to decode the delete construction independently of its primary-body target.

### OM-12. Inactive-arrangement body state

**Question.** Which bodies belong to each inactive arrangement, and what per-body state does that arrangement select?

**Known.** `siemens_nx.md:54` through `siemens_nx.md:66` define arrangement identity and active body membership. Other arrangements have no body membership without a separate relation.

**Need.** We must know the relation to construct inactive configuration body sets.

### OM-13. Inactive-arrangement parameter state

**Question.** Which parameter values does each inactive arrangement select?

**Known.** `siemens_nx.md:75` through `siemens_nx.md:89` define complete parameter state only for a uniquely resolved active configuration.

**Need.** We must know the relation to construct inactive configuration parameter maps.

### OM-14. Operation terminal discriminators

**Question.** What does each type index, flag, and trailing index in an operation terminal discriminator lane mean?

**Known.** Section 7.1 of `siemens_nx.md` defines the exact terminal discriminator lanes for the admitted operation payloads and retains their serialized order.

**Need.** We must know the field roles to construct termination, direction, draft, and other operation controls.

### OM-15. `CPROJ` construction-reference roles

**Question.** Which `CPROJ` construction references select the source curve, target surface, direction, and combination controls?

**Known.** `siemens_nx.md:1393` through `siemens_nx.md:1399` define the ordered three-reference graph, block resolution, and logical payload.

**Need.** We must know the roles to construct a neutral projected curve.

### OM-16. `CPROJ_CMB` construction-reference roles

**Question.** Which `CPROJ_CMB` construction references select the source curves, target surfaces, directions, and combination controls?

**Known.** `siemens_nx.md:1393` through `siemens_nx.md:1399` define the ordered eight-reference graph, block resolution, and logical payload.

**Need.** We must know the roles to construct the combined projected curves.

### OM-17. `FSET` selection roles

**Question.** What does the `FSET` selector mean, and what selection role does each ordered object-reference group have?

**Known.** `siemens_nx.md:1348` through `siemens_nx.md:1350` define the selector, two separate reference groups, resolution rule, and logical payloads.

**Need.** We must know the roles to construct the selected face or feature set.

### OM-18. Pattern construction-reference roles

**Question.** Which ordered references in `Pattern Feature`, `Pattern Geometry`, and `Geometry Instance` select the seed, transform, and pattern controls?

**Known.** `siemens_nx.md:1352` through `siemens_nx.md:1365` define the construction graph, logical payload, and counted row forms.

**Need.** We must know the roles to construct neutral pattern dependencies and transforms.

### OM-19. Pattern-row scalar roles

**Question.** What does each scalar in a counted pattern row mean?

**Known.** `siemens_nx.md:1361` through `siemens_nx.md:1365` define the Q1.55 and wide-row scalar encodings, row order, exact values, and boundaries.

**Need.** We must know the roles to construct pattern coordinates, spacing, angles, and other controls.

### OM-20. Pattern-row selector roles

**Question.** What does each compact selector in a counted pattern row select?

**Known.** `siemens_nx.md:1363` through `siemens_nx.md:1365` define selector framing, non-null requirements, row ordinals, and exact tokens.

**Need.** We must know the roles to bind each row to its seed or transform operand.

### OM-21. `Multi Instance Output` roles

**Question.** What does each selector group and trailing reference in a `Multi Instance Output` lane mean?

**Known.** Section 7.1 of `siemens_nx.md` defines the ordered lane framing and retains its exact selectors and references.

**Need.** We must know the roles to relate pattern instances to their output bodies or geometry.

### OM-22. Equal pattern and profile labels

**Question.** What serialized relation establishes identity or a seed relation between blocks that have equal canonical line labels?

**Known.** `siemens_nx.md:163` through `siemens_nx.md:184` define operation input identity by resolved store block. Equal text in distinct pattern and profile blocks does not establish block identity.

**Need.** We must know the relation to connect a pattern to the correct seed without merging unrelated blocks.

### OM-23. `POINT` header fields

**Question.** What construction role does the `POINT` header reference have, and what does its `02|03` mode select?

**Known.** `siemens_nx.md:1371` defines the complete header grammar, canonical reference, mode values, and datum-point family.

**Need.** We must know the roles to select the point construction method and its operand.

### OM-24. `POINT` scalar triples

**Question.** What coordinate spaces and construction roles do the two ordered scalar triples in the selected six-scalar `POINT` lane use?

**Known.** Section 7.1 of `siemens_nx.md` defines the six-scalar lane, its selected form, values, and exact boundaries. A shared target block does not identify either triple as the model-space point.

**Need.** We must know the roles to construct the datum point at its authored model-space position.

### OM-25. `DRAFT` construction roles

**Question.** Which counted leading indices, ordered references, terminal indices, and tail fields select the drafted faces, neutral plane, pull direction, and draft angle?

**Known.** `siemens_nx.md:1375` through `siemens_nx.md:1391` define the leading index lane, four-reference graph, terminal lanes, scalar encodings, store resolution, and exact boundaries.

**Need.** We must know the roles to construct a neutral draft operation.

### OM-26. `SKIN` construction roles

**Question.** Which ordered `SKIN` references and branch groups select sections, guides, continuity, and terminal controls?

**Known.** `siemens_nx.md:1413` through `siemens_nx.md:1419` define the loft family, common construction envelope, ordered references, and logical payload.

**Need.** We must know the roles to construct the neutral loft surface or body.

### OM-27. `Studio Surface` construction roles

**Question.** Which ordered `Studio Surface` references and branch groups select control geometry, continuity, and terminal controls?

**Known.** `siemens_nx.md:1417` through `siemens_nx.md:1425` define the surface construction envelope, ordered references, logical payload, and feature family.

**Need.** We must know the roles to construct the neutral freeform surface.

### OM-28. Plain cached-body ownership

**Question.** Which feature owns each plain cached-body stream?

**Known.** `siemens_nx.md:122` through `siemens_nx.md:137` define segment tuples for partition and plain cached-body streams. `siemens_nx.md:1488` through `siemens_nx.md:1514` define body writers, operands, aliases, and terminal lineage.

**Need.** We must know the ownership relation to use a cached body as the correct feature result or tool.

### OM-29. `RMFastLoad` class records

**Question.** What is the per-class entity-record grammar in `RMFastLoad` outside its object-ID membership table?

**Known.** `siemens_nx.md:1506` through `siemens_nx.md:1510` define the counted object-ID table and active-body membership semantics.

**Need.** We must know the class grammars to transfer the remaining fast-load state as typed data.

## 3. Assembly and material data

### AM-01. Fast-load structure stream

**Question.** What is the field grammar and semantics of `/Root/FastLoad/Structure`?

**Known.** The stream has a big-endian bounded envelope. Its typed component
roster defines ordered named prototypes and a one-based prototype index for
each distinct occurrence. Other payload fields remain uninterpreted.

**Need.** We must know the remaining payload grammar, including hierarchy,
placement, UUID, and state fields.

### AM-02. Fast-load JT stream

**Question.** What is the field grammar and semantics of `/Root/FastLoad/JT`?

**Known.** `siemens_nx.md:245` through `siemens_nx.md:362` classify the stream separately from `/Root/UG_PART/DisplayJT` and retain its bounded container entry.

**Need.** We must know the grammar to decode its fast-load display relations as typed data.

### AM-03. Last-saved toggle stream

**Question.** What is the field grammar and semantics of `/Root/UG_PART/LastSavedToggleInfoStream`?

**Known.** `siemens_nx.md:245` through `siemens_nx.md:362` classify the stream and retain its bounded container entry.

**Need.** We must know the grammar to decode the saved toggle state as typed data.

### AM-04. `DisplayJT` outer-index values

**Question.** What does each nonzero outer-index row value in `/Root/UG_PART/DisplayJT` mean?

**Known.** Section 2 of `siemens_nx.md` defines the outer index, referenced JT segments, scene graph, shape data, and tessellation relations.

**Need.** We must know the row roles to relate each indexed value to the correct display object or segment.

### AM-05. Assembly occurrence placement

**Question.** Which serialized fields define the transform and units of each assembly occurrence placement?

**Known.** `siemens_nx.md:354` through `siemens_nx.md:362` define external child geometry and require occurrence placement. The OM expression grammar defines stored numeric expression values.

**Need.** We must know the fields to place each child part in assembly coordinates.

### AM-06. Assembly pattern dimensions

**Question.** Which serialized fields bind assembly pattern angles, counts, metric radii, and base frames to an occurrence pattern?

**Known.** `siemens_nx.md:448` through `siemens_nx.md:459` define `hostglobalvariables` names, units, formulas, and evaluated values. They do not define the occurrence-pattern binding.

**Need.** We must know the bindings to construct patterned assembly occurrences.

### AM-07. Child handle sets to occurrences

**Question.** How does each child-bound persistent-handle set identify one distinct assembly occurrence?

**Known.** The fast-load component roster preserves each occurrence ordinal and
its named prototype, including repeated uses. `EXTREFSTREAM` preserves child
paths and persistent-handle sets independently.

**Need.** We must know the mapping to preserve multiple occurrences of the same child part.

### AM-08. Residual `EXTREFSTREAM` tail fields

**Question.** What are the field boundaries and roles of the residual bytes in an indexed `EXTREFSTREAM` record tail?

**Known.** `siemens_nx.md:1533` through `siemens_nx.md:1539` define the indexed-record boundary, handle-set prefix, persistent-handle pairs, tagged references, string uses, and child binding. Other tail bytes remain opaque.

**Need.** We must know the fields to decode complete occurrence and external-reference state.

### AM-09. SDL/TYSA attribute values

**Question.** How does each Parasolid SDL/TYSA attribute instance assign its referenced value records to the fields declared by its type-79 class definition?

**Known.** `siemens_nx.md:1563` through `siemens_nx.md:1614` define attribute-class declarations, type-81 class selection, referenced value records, topology ownership, and neutral source-attribute names. The declaration includes ordered field type codes such as those for `SDL/TYSA_DENSITY` and `SDL/TYSA_BLEND_ID`.

**Need.** We must know the assignment to transfer class-specific material and topology attributes with semantic field names.

### AM-10. Face material bindings

**Question.** Which serialized relation binds a material or appearance to a Parasolid face identity?

**Known.** `siemens_nx.md:325` through `siemens_nx.md:352` define preview and texture assets and the material-texture catalog. `siemens_nx.md:1563` through `siemens_nx.md:1614` define topology-owned Parasolid attributes.

**Need.** We must know the relation to assign material and appearance state to neutral faces.
