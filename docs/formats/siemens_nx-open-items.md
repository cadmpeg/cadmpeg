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

### PS-31. `OFFSET_SURF` discriminator and true-offset roles

**Question.** What do the `OFFSET_SURF` discriminator byte and `true_offset` field select?

**Known.** `siemens_nx.md` §6.1 "**OFFSET_SURF (60):** discriminator byte `+19` (`V`/`I`/`U`), `true_offset:u8 +20` (`0`/`1`), base surface ref" defines the layout and the evaluation `P = base(u,v) + offset_distance * unit_normal(u,v)`. It assigns no role to the discriminator or to `true_offset`.

**Need.** We must know the roles before the transferred surface states a parameter-direction sense. The decoder writes a forward sense for both parameters on every offset surface. If either field selects parameter reversal, every transferred offset surface states the wrong sense, and the sense comparison between two offset surfaces cannot separate them.

**Note.** The closure changes the specification to say that `V`, `I`, and `U` are status values and that neither field affects parameter direction. The tests only mutate synthetic bytes and assert that the decoder leaves senses unset. No serialized comparison establishes either field's meaning or rules out an orientation role.

### PS-32. Procedural-intersection support ordering

**Question.** Which reference of a type-38 construction is the primary support?

**Known.** `siemens_nx.md` §6.3 "For the `0x5a` delta twin the layout is fixed (primary = ref[0], bridge = ref[1]); for t" gives the rule: the `0x5a` twin has a fixed layout, and a type-38 construction takes its primary reference from the `0x00cc` marker, where marker 2 selects reference 0 and marker 3 selects reference 1.

**Need.** We must decide the primary-support rule so that a type-38 construction with two surface references does not attach its support lane to the wrong surface. The serialized-lane path does not apply the chart-tolerance test, so nothing later rejects the wrong attachment.

**Conflict.** The decoder does not apply this rule. `construction_supports` in `crates/cadmpeg-codec-nx/src/intersection.rs` tests reference 0 and then reference 1 for surface identity and takes the first that is a surface. The marker is decoded and retained, but it selects only the tuple width. The two rules agree when one reference is a type-59 bridge, and they disagree when both references resolve to surface records and the marker is 3. The support lane then attaches to the wrong surface. `siemens_nx.md` §6.3 also requires every evaluable lane point to reproduce its chart point inside the chart tolerance; the serialized-lane path does not apply that test, so nothing later rejects the wrong attachment.

**Note.** The closure validates support-UV lanes against the selected surface but does not independently establish that marker 3 reverses support order. The marker test and fixture were authored together with the rule. If the marker selects another values-array mode, both surface identity and pcurve attachment are wrong.

### PS-33. Chart point layout selection

**Question.** Which field selects the `Hvec` layout of a `CHART_s` point array?

**Known.** `siemens_nx.md` §6.3 "Hvec form depends on the stream: partition streams use **`xyz3`** (`x,y,z` meters); delt" gives the rule: the stream kind selects the layout. `siemens_nx.md` §6 "All geometric doubles are finite binary64 values in meters" states that the format imposes no model-magnitude bound.

**Need.** We must decide the layout selector so that a wide record is not read as narrow triples that cross field boundaries, and so that a charted intersection of a larger model is not dropped by the magnitude test.

**Conflict.** The decoder does not apply the stream rule. `chart_points` in `crates/cadmpeg-codec-nx/src/intersection.rs` tries the wide layout first, accepts it when every tangent is near unit norm and the native parameters ascend, and otherwise reads the same bytes with the narrow stride. The caller separates partition bytes from replacement-stream bytes already, so the stream kind is available and unused. A wide record that fails the norm test is then read as narrow triples that cross field boundaries, and the resulting point sequence is transferred as curve geometry. The same function rejects any coordinate at or above one hundred meters, which contradicts §6 and drops every charted intersection of a larger model.

**Note.** The closure passes a `StreamKind` into the parser, but the ext11 branch still admits points through tangent and parameter plausibility checks. The stream-kind mapping and the synthetic fixtures were introduced together; a serialized chart from corpus records has not yet verified that outer stream kind is the layout discriminator.

### PS-34. B-spline form-code semantics

**Question.** What does each B-spline form code mean?

**Known.** `siemens_nx.md` §9.3 "A type-126 B-surface descriptor stores U/V periodic logical flags" names the descriptor fields and assigns the former form-code positions as knot types. `siemens_nx.md` §9.3 "The B-spline knot type does not determine whether a control grid is rational or periodic." excludes one interpretation. The specification does not provide independent evidence for the value meanings.

**Need.** We must know the meaning of each code. The decoder admits the codes `1`, `4`, `5`, and `6`, and transfers the single code `6` as the periodic flag of the surface, curve, or pcurve. A periodic carrier whose code is not `6` transfers as open, so its seam trims as a boundary. Periodicity also gates the offset-surface cache relation, so a wrong flag admits or discards that relation.

**Note.** The closure moves periodicity to logical bytes and relabels the former form bytes as knot types, but the value meanings are asserted by the changed specification and synthetic descriptor tests. The current parser retains knot types only as an admission gate; their semantics remain unverified against corpus records.

### PS-35. Escaped and direct fixed-record disambiguation

**Question.** Which test separates a direct large-index fixed record from an escaped record when the byte after the type is `ff`?

**Known.** `siemens_nx.md` §5.1 "Any fixed record may place an envelope escape byte `ff` between its type and xmt" states that the complete family field grammar disambiguates the two readings. `siemens_nx.md` §4.2 "Status-framed fixed records use a status byte in `0..=1` after each encoded reference." requires that exactly one reading ends before a recognized node type.

**Need.** We must decide which test separates the two readings so that a direct record whose remainder byte is `ff` keeps its identity, and so that BODY and REGION records are not selected by candidate order.

**Conflict.** The decoder applies neither test. `Graph::parse` in `crates/cadmpeg-codec-nx/src/topology.rs` builds both readings, filters each by family framing, and then prefers the escaped reading on a quality tie. The quality function scores only SHELL records and returns zero for every other kind, so the escaped reading always wins for FACE, LOOP, EDGE, FIN, VERTEX, and POINT. A direct record whose remainder byte is `ff` is then indexed under a different identity, and every reference that names it fails to resolve. The same comparison decides which of two records with equal type and identity keeps the graph slot, and it discards the other without reporting a loss. The family-framing filter that the specification names as the disambiguator tests only SHELL, FACE, LOOP, EDGE, FIN, VERTEX, and POINT records, and admits every other kind unconditionally. For BODY and REGION the stated rule therefore selects nothing, and the reading comes from candidate order.

**Note.** The original closure removed the old shell-only preference, but the current graph still ranks candidates in `topology.rs:718-856` with body-shape, boundary, reference-count, and node-quality heuristics. The ambiguity debt was not removed; it was generalized and is tracked again by PS-38.

### PS-36. Standalone `0x5a` record anchor

**Question.** What anchors a standalone `0x5a` intersection record in a deltas stream?

**Known.** `siemens_nx.md` §4.2 "The type-38 form has the header" gives the exact schema prefix, in which the name `intersection_data` precedes the `0x5a` tag at a fixed distance. `siemens_nx.md` §4.2 "Status-framed type-38 `INTERSECTION` records end after their six construction references" states that these records occur standalone and need no following recognized tag.

**Need.** We must know the anchor to admit exactly the real records. The decoder treats every `0x5a` byte as a candidate and accepts it when a header reference equals one, or when the name occurs anywhere in about eighty preceding bytes. Neither condition is the fixed prefix. A record whose header is farther upstream is dropped, and a payload byte run that satisfies the structural tests enters the model as a curve.

**Note.** The closure introduces an exact header constant and a stream-global `schema_anchor_seen` flag. The tests construct the header from that same constant. A valid variant header may be rejected, and an unrelated later `0x5a` can be admitted after one earlier anchor. Scope and alternate forms need independent serialized evidence.

### PS-37. NURBS record count and degree limits

**Question.** What bounds the counts, degrees, and pole counts of B-spline support records?

**Known.** `siemens_nx.md` §9.3 "Type 127 stores `00 7f [ff], 0000, count:u16 BE, xmt, value[count]:u16 BE`. Type 128 uses the same envelope and sto" defines the array-record envelopes and states that counts are nonzero and identities are non-null. `siemens_nx.md` §6.2 "Control-grid stride = `double_count / (u_pole_count · v_pole_count)`; `3` = non-rationa" gives the basis constraints that relate degree, pole count, and multiplicity sums.

**Need.** We must know the bounds, or confirm that the basis constraints are the only ones. The decoder locates these records by scanning the complete stream and admits a record only inside fixed numeric ranges for the array count, the degree, the pole count, and the distinct-knot count. A surface or curve outside those ranges is omitted, and its face keeps an unresolved carrier.

**Note.** The closure changes several descriptor fields from narrow reads to wider reads and removes explicit ceilings, then validates basis cardinality. The synthetic large-cardinality tests are constructed for that interpretation; corpus records have not yet verified the field widths or the absence of a format/resource bound. The count and degree rule remains open.

### PS-38. Fixed-record candidate ranking

**Question.** What serialized evidence selects one complete fixed-record interpretation when direct, escaped, or overlapping candidates all pass structural parsing?

**Known.** `siemens_nx.md` §5.1 requires the complete family grammar and record boundary to establish fixed-record identity. It does not define body-shape preference, reference-count preference, node-quality ranking, or an ownership rule based on scan order.

**Need.** We must know the discriminator before indexing a candidate by `(type, XMT)` or discarding an overlapping candidate. A wrong choice changes topology ownership and every dependent geometry relation.

**Note.** `Graph::parse` in `crates/cadmpeg-codec-nx/src/topology.rs:718-856` accepts the highest-ranked candidate after reference filtering and falls back to heuristic ranking when no candidate resolves. `crates/cadmpeg-codec-nx/src/framing.rs:24-77` supplies multiple complete interpretations. **Evidence:** the comparator uses body-shape, recognized-boundary, reference-count, and node-quality preferences; no serialized discriminator is read. **Counter-evidence:** reference-consistency checks and equal-score rejection discard some ambiguous candidates, and the ranking can be intended as recovery for malformed streams. **Failure:** if an incidental `00 kind` sequence occurs inside a valid record, or if direct and escaped readings both end at recognized boundaries, the comparator can retain one reading without a serialized discriminator, changing topology ownership and dependent geometry. This is a new selection debt found in the 2026-08-10 hostile sweep.

### PS-39. Cross-form intersection XMT identity

**Question.** Can a type-38 construction and a schema-anchored single-byte `0x5a` construction share one stream-local XMT, and if so, which construction owns the chart and carrier relations?

**Known.** `siemens_nx.md` §6.3 distinguishes the type-38 and `intersection_data` construction forms. The decoder uses stream-local XMT values for chart, construction, and carrier lookup. The specification does not define collision handling across the two construction forms.

**Need.** We must know whether the two forms share one identity namespace or are mutually exclusive before joining charts and emitting native construction records.

**Note.** `crates/cadmpeg-codec-nx/src/intersection.rs:367-376` chains both forms, `crates/cadmpeg-codec-nx/src/native/parasolid.rs:1391-1414` emits both under one intersection-record identity stem, and `crates/cadmpeg-codec-nx/src/decode.rs:1317-1331` and `1468-1476` use last-write-wins maps keyed only by XMT. **Evidence:** the construction forms share the stream-local XMT key and no cross-form collision check exists. **Counter-evidence:** the format may guarantee that the forms are mutually exclusive or that their XMT namespaces cannot collide; no raw record establishes either rule. **Failure:** if both forms occur with one XMT, iteration order selects one chart and one carrier relation without rejecting the ambiguity. This issue was found in the hostile sweep.

### PS-40. Completion scope across Parasolid streams

**Question.** Which decoded entities does intersection support and support-UV chart completion use: the complete model, or only the entities that one Parasolid stream creates?

**Known.** `siemens_nx.md` §6.3 "When exactly one serialized support is null, an edge using the construction" gives the support rule from an edge and its incident face surfaces. `siemens_nx.md` §6.3 "After support completion, an incident FIN supplies a missing support-UV chart" gives the chart rule from an incident FIN. Neither rule limits the incidence to one stream. `siemens_nx.md` §2 "A deltas stream applies to the nearest preceding partition stream in segment" shows that one part contains more than one Parasolid stream.

**Need.** We must know the scope. The decoder must not leave a support or a chart unresolved when the model contains the incidence that completes it.

**Conflict.** `IntersectionIncidenceIndex::complete_from_stream` in `crates/cadmpeg-codec-nx/src/decode/pcurves.rs:275-283` indexes the loops, faces, edges, coedges, pcurves, and procedural curves after the counts of the previous stream, and completes only the constructions of the curves in that set. Identities carry the stream ordinal, so a construction of an earlier stream is never in that set again. Two phases change the same entities after the completion of their stream: `invalidate_inconsistent_support_uv_with_budget` in `crates/cadmpeg-codec-nx/src/decode/support_uv.rs:574-588` sets a side pcurve to none, and `attach_completed_intersection_pcurves_for_stream_with_budget` in the same file adds pcurves to coedges. No later completion uses either result.

## 2. Object model and body composition

### OM-01. Per-class OM field serialization

**Question.** What byte grammar and semantic role does each declared field of each NX OM class use?

**Known.** `siemens_nx.md` §7.1 "UG_PART begins with a 12-byte row table" and `siemens_nx.md` §7.1 "A feature-history operation record begins at the fixed operation-header marker" define OM section boundaries, class and member declarations, store identities, compact indices, and expression records. `siemens_nx.md` §3.3 "A numeric expression table contains a `hostglobalvariables` root entity." defines typed fields for selected construction families.

**Need.** We must know the remaining class grammars to decode feature history, constraints, attributes, and material bindings as typed fields.

### OM-02. `SKETCH` scalar-pair geometry

**Question.** What geometric quantities and coordinate spaces do the framed scalar pairs in a `SKETCH` construction payload represent?

**Known.** `siemens_nx.md` §2 "An operation label equal to `SKETCH` denotes a sketch history operation." `siemens_nx.md` §7.1 "A sketch payload scalar field is", `siemens_nx.md` §7.1 "A sketch repeated-type scalar pair is", and `siemens_nx.md` §7.1 "A sketch fixed pair has one of four exact forms:" define sketch record identity and the framed payload lanes but do not assign a model-space frame, sketch entity, or constraint role from equal scalar values.

**Need.** We must know the roles to construct neutral sketch geometry and constraints.

### OM-03. `DATUM_PLANE` scalar-pair geometry

**Question.** What geometric quantities and coordinate spaces do the framed scalar pairs in a `DATUM_PLANE` construction payload represent?

**Known.** `siemens_nx.md` §7.1 "A `DATUM_PLANE` payload begins" and `siemens_nx.md` §7.1 "A datum-plane object scalar-pair frame is" and `siemens_nx.md` §7.1 "A datum-plane descriptor block is exactly 40 bytes:" define datum-plane branches, resolved blocks, scalar-pair framing, descriptors, and feature identity.

**Need.** We must know the roles to construct the model-space reference plane.

### OM-04. `DATUM_CSYS` scalar-pair geometry

**Question.** What geometric quantities and coordinate spaces do the framed scalar pairs in a `DATUM_CSYS` construction payload represent?

**Known.** `siemens_nx.md` §7.1 "A `DATUM_CSYS` payload begins" and `siemens_nx.md` §7.1 "An object-payload scalar-pair frame is" define the eight-reference construction lane, logical payload, scalar-pair framing, and feature identity.

**Need.** We must know the roles to construct the model-space coordinate-system frame.

### OM-05. OM declaration trailing code

**Question.** What does the trailing byte in an OM class or member declaration mean?

**Known.** `siemens_nx.md` §7.1 "The first record at `oid_end` begins" defines the declaration length, `UGS::` or `m_` name, trailing-code boundary, and following registry suffix.

**Need.** We must know the meaning to validate and transfer declaration metadata.

### OM-06. OM registry suffix fields

**Question.** What does each byte in a bounded OM field-registry suffix mean?

**Known.** `siemens_nx.md` §7.1 "The first record at `oid_end` begins" defines the suffix boundary and the 11-to-14-byte prefix, fingerprint, and terminal-byte decomposition.

**Need.** We must know the field roles to construct the complete OM schema registry.

### OM-07. Offset-store body to segment-image relation

**Question.** How does a primary feature body field that resolves to an offset-store block identify a segment body-image object-index pair?

**Known.** `siemens_nx.md` §2 "A partition or plain cached-body wrapper word begins" and `siemens_nx.md` §2 "A primary feature body field in the object namespace reuses a segment body" define segment body-image tuples and prohibit a relation based only on equal integer values across namespaces. They also define primary-body fields and body selection.

**Need.** We must know the cross-store relation to attach the feature output and lineage to the correct body image.

**Note.** `crates/cadmpeg-codec-nx/src/native/features.rs:3686-3746` requires one offset store, one data-block use, and one unique segment alias, while `native/segments.rs:137-173` supplies the alias-equality join. The closure test constructs `FeatureBodySegmentUse` inputs and verifies uniqueness; it does not provide an NX record that makes an offset-store block and a segment alias the same object. The integer equality remains a promoted cross-store relation, so this item is reopened.

### OM-08. Other feature-history object relations

**Question.** What relation does each feature-history object index that is not a primary-body writer or Boolean tool use?

**Known.** `siemens_nx.md` §7.1 "A nested operation object-relation frame is" defines the exact nested frame, canonical endpoint encoding, ordered endpoint retention, and source offsets. The native decoder retains these frames as `feature_operation_object_relations` without assigning endpoint roles. `siemens_nx.md` §7.1 "A direct operation tagged-reference field is" defines the exact direct `0x17` field, canonical object-index encoding, optional unique offset-store target, and source offsets. The native decoder retains these fields as `feature_operation_tagged_references` without assigning endpoint roles. `siemens_nx.md` §7.1 "A direct operation data-block reference field is" defines the exact direct `0x03` field, canonical object-index encoding, optional unique offset-store target, and source offsets. The native decoder retains these fields as `feature_operation_data_block_references` without assigning endpoint roles. `siemens_nx.md` §2 "Within a feature-history record area, an operation header is encoded as the" and `siemens_nx.md` §2 "Input bindings from two or more distinct operation headers form an identity" and `siemens_nx.md` §2 "A body-affecting operation record contains exactly one primary-body field" define operation-header inputs, shared-block identity groups, primary-body lineage, and Boolean operands.

**Need.** We must map each retained nested frame to its owning feature relation before constructing feature dependencies or selections. The link tag and endpoint identities alone do not establish a body, operand, input, or output role.

### OM-09. Embedded operation common-frame ownership

**Question.** Which operation owns each embedded common frame, and what do the state-lane fields other than `m_modifiesParasolidData` mean?

**Known.** `siemens_nx.md` §7.1 "A bounded operation payload's terminal common-frame suffix is" and `siemens_nx.md` §7.1 "An exact common frame is" define the exact frame and state-lane boundaries for the admitted operation families. The fourth state byte is the Boolean `m_legacyInactiveModules` field. The fifth state byte is the Boolean `m_modifiesParasolidData` field. The sixth and seventh state bytes are the exact two-byte `m_splitTrackingData` representation. The eighth state byte is the unsigned `m_groupCount` field. Legacy module inactivity is not feature suppression.

**Need.** We must know ownership and field roles to attach the state to the correct operation.

**Note.** Commit `80222d179` removed this item from the ledger without a serialized witness; its relevant change is documentation and common-frame handling, not an independent ownership record. Matching the frame shape across synthetic operations does not prove which operation owns an embedded frame or the meaning of every lane. The item is reopened.

### OM-10. Operation suppression fields

**Question.** How do the embedded operation state lanes encode suppression?

**Known.** `siemens_nx.md` §2 "Every feature producing a body in the selected current B-rep is active in the" derives active state for the closed output-and-dependency graph and leaves suppression outside that graph unresolved.

**Need.** We must know the serialized suppression fields to construct operation state for all configurations.

### OM-11. `DELETE` nullable-reference roles

**Question.** What object family can each of the five leading nullable references in a `DELETE` payload address, and what is the role of each slot?

**Known.** `siemens_nx.md` §2 "`DELETE` is a body-deletion operation only when its bounded record contains the" and `siemens_nx.md` §2 "A `DELETE` payload begins with one nullable reference field" define the five-slot field, canonical reference encodings, resolution rule, logical payload, and body-target exclusion.

**Need.** We must know the slot roles to decode the delete construction independently of its primary-body target.

### OM-12. Inactive-arrangement body state

**Question.** Which bodies belong to each inactive arrangement, and what per-body state does that arrangement select?

**Known.** `siemens_nx.md` §2 "`/Root/part/arrangements` has an `Arrangements` root." and `siemens_nx.md` §2 "A unique part-owned `NX_Arrangement` string attribute names the active" define arrangement identity and active body membership. Other arrangements have no body membership without a separate relation.

**Need.** We must know the relation to construct inactive configuration body sets.

### OM-13. Inactive-arrangement parameter state

**Question.** Which parameter values does each inactive arrangement select?

**Known.** `siemens_nx.md` §2 "When exactly one active configuration has complete body membership, the same" and `siemens_nx.md` §2 "The same active configuration retains the complete current parameter state when" define complete parameter state only for a uniquely resolved active configuration.

**Need.** We must know the relation to construct inactive configuration parameter maps.

### OM-14. Operation terminal discriminators

**Question.** What does each type index, flag, and trailing index in an operation terminal discriminator lane mean?

**Known.** `siemens_nx.md` §7.1 "A bounded operation payload's terminal common-frame suffix is" defines the exact terminal discriminator lanes for the admitted operation payloads and retains their serialized order.

**Need.** We must know the field roles to construct termination, direction, draft, and other operation controls.

### OM-15. `CPROJ` construction-reference roles

**Question.** Which `CPROJ` construction references select the source curve, target surface, direction, and combination controls?

**Known.** `siemens_nx.md` §7.1 "A `CPROJ` payload contains at most one construction-reference field framed as" defines the ordered three-reference graph, block resolution, and logical payload.

**Need.** We must know the roles to construct a neutral projected curve.

### OM-16. `CPROJ_CMB` construction-reference roles

**Question.** Which `CPROJ_CMB` construction references select the source curves, target surfaces, directions, and combination controls?

**Known.** `siemens_nx.md` §7.1 "A `CPROJ_CMB` payload contains at most one construction-reference graph framed as" defines the ordered eight-reference graph, block resolution, and logical payload.

**Need.** We must know the roles to construct the combined projected curves.

### OM-17. `FSET` selection roles

**Question.** What does the `FSET` selector mean, and what selection role does each ordered object-reference group have?

**Known.** `siemens_nx.md` §7.1 "An `FSET` operation payload contains at most one two-group reference graph framed as" defines the selector, two separate reference groups, resolution rule, and logical payloads.

**Need.** We must know the roles to construct the selected face or feature set.

### OM-18. Pattern construction-reference roles

**Question.** Which ordered references in `Pattern Feature`, `Pattern Geometry`, and `Geometry Instance` select the seed, transform, and pattern controls?

**Known.** `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one ten-slot construction-reference graph in one of two exact layouts." and `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one transform lane." define the two construction graph framings, logical payload, and counted row forms.

**Need.** We must know the roles to construct neutral pattern dependencies and transforms.

### OM-19. Pattern-row scalar roles

**Question.** What does each scalar in a counted pattern row mean?

**Known.** `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one transform lane." and `siemens_nx.md` §7.1 "`Pattern Feature` also admits the wide-row form" define the Q1.55 and wide-row scalar encodings, row order, exact values, and boundaries.

**Need.** We must know the roles to construct pattern coordinates, spacing, angles, and other controls.

### OM-20. Pattern-row selector roles

**Question.** What does each compact selector in a counted pattern row select?

**Known.** `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one transform lane." and §7.1 "`Pattern Feature` payloads contain at most one counted reference lane." define selector framing, non-null requirements, row ordinals, counted references, and exact tokens.

**Need.** We must know the roles to bind each row to its seed or transform operand.

### OM-21. `Multi Instance Output` roles

**Question.** What does each selector group and trailing reference in a `Multi Instance Output` lane mean?

**Known.** `siemens_nx.md` §7.1 "`Multi Instance Output` payloads contain at most one counted output lane" defines the ordered lane framing and retains its exact selectors and references.

**Need.** We must know the roles to relate pattern instances to their output bodies or geometry.

### OM-22. Equal pattern and profile labels

**Question.** What serialized relation establishes identity or a seed relation between blocks that have equal canonical line labels?

**Known.** `siemens_nx.md` §2 "Input bindings from two or more distinct operation headers form an identity" and `siemens_nx.md` §7.1 "`Pattern Feature` and `Pattern Geometry` payloads contain at most one ten-slot construction-reference graph in one of two exact layouts." define operation input identity by resolved store block. Equal text in distinct pattern and profile blocks does not establish block identity.

**Need.** We must know the relation to connect a pattern to the correct seed without merging unrelated blocks.

**Note.** Commit `80222d179` removed this item as a documentation-only change. Refusing an equal-label join is a conservative ambiguity policy, not evidence that equal labels never identify a seed or that another serialized relation is absent. The closure cited no pattern/profile record from the corpus, so the item is reopened.

### OM-23. `POINT` header fields

**Question.** What construction role does the `POINT` header reference have, and what does its `02|03` mode select?

**Known.** `siemens_nx.md` §7.1 "The `POINT` operation payload begins with one construction header." defines the complete header grammar, canonical reference, mode values, and datum-point family.

**Need.** We must know the roles to select the point construction method and its operand.

### OM-24. `POINT` scalar triples

**Question.** What coordinate spaces and construction roles do the two ordered scalar triples in the selected six-scalar `POINT` lane use?

**Known.** `siemens_nx.md` §7.1 "The header reference selects a six-scalar lane ending in its addressed offset-store block." defines the six-scalar lane, its selected form, values, and exact boundaries. A shared target block does not identify either triple as the model-space point.

**Need.** We must know the roles to construct the datum point at its authored model-space position.

### OM-25. `DRAFT` construction roles

**Question.** Which counted leading indices, ordered references, terminal indices, and tail fields select the drafted faces, neutral plane, pull direction, and draft angle?

**Known.** `siemens_nx.md` §7.1 "The `DRAFT` operation payload begins" and `siemens_nx.md` §7.1 "The same payload contains exactly one four-reference construction graph." define the leading index lane, four-reference graph, terminal lanes, scalar encodings, store resolution, and exact boundaries.

**Need.** We must know the roles to construct a neutral draft operation.

### OM-26. `SKIN` construction roles

**Question.** Which ordered `SKIN` references and branch groups select sections, guides, continuity, and terminal controls?

**Known.** `siemens_nx.md` §7.1 "The `SKIN` and `THRU_CURVE` operation labels identify loft-family constructions." and `siemens_nx.md` §7.1 "`SKIN` and `Studio Surface` payloads share one exact common construction-reference envelope." define the loft family, common construction envelope, ordered references, and logical payload.

**Need.** We must know the roles to construct the neutral loft surface or body.

### OM-27. `Studio Surface` construction roles

**Question.** Which ordered `Studio Surface` references and branch groups select control geometry, continuity, and terminal controls?

**Known.** `siemens_nx.md` §7.1 "`SKIN` and `Studio Surface` payloads share one exact common construction-reference envelope." and `siemens_nx.md` §7.1 "The `Studio Surface` operation label identifies a freeform-surface construction." define the surface construction envelope, ordered references, logical payload, and feature family.

**Need.** We must know the roles to construct the neutral freeform surface.

### OM-28. Plain cached-body ownership

**Question.** Which feature owns each plain cached-body stream?

**Known.** `siemens_nx.md` §2 "A partition or plain cached-body wrapper word begins" and `siemens_nx.md` §7.2 "Across the ordered feature-history sections, the last non-`DELETE` operation carrying a primary-body field is that body object's latest writer." define segment tuples for partition and plain cached-body streams, body writers, operands, aliases, and terminal lineage.

**Need.** We must know the ownership relation to use a cached body as the correct feature result or tool.

### OM-29. `RMFastLoad` class records

**Question.** What is the per-class entity-record grammar in `RMFastLoad` outside its object-ID membership table?

**Known.** `siemens_nx.md` §2.1 "| `/Root/FastLoad/RMFastLoad`" and `siemens_nx.md` §2.3 "`/Root/FastLoad/Structure` begins with the twelve-byte envelope" define the fast-load object-ID table and the bounded structure stream. The per-class entity-record grammar outside the membership table remains unresolved.

**Need.** We must know the class grammars to transfer the remaining fast-load state as typed data.

### OM-30. Hole-package feature hierarchy

**Question.** Does a `HOLE PACKAGE` operation own its related `SIMPLE HOLE` operations as child features, replace them as the authored neutral feature, or coexist with them as an independent operation?

**Known.** `siemens_nx.md` §7.1 "A `HOLE PACKAGE` payload contains at most one construction-group lane framed as" and `siemens_nx.md` §7.1 "One package lane relates to one simple-hole construction group when their four resolved block identities are equal in serialized order" define the four-block package lane and its unambiguous equality relation to a simple-hole construction group. The equality does not encode hierarchy, dependency direction, or neutral feature identity.

**Need.** We must identify the serialized hierarchy or operation-role field before collapsing, parenting, or suppressing either neutral feature family.

**Conflict.** This item, the specification, and the decoder disagree, and the specification disagrees with itself. `siemens_nx.md` §7.1 "A `HOLE PACKAGE` operation related to one simple-hole construction group owns" states the ownership as fact and adds that the internal operations do not also project as neutral history features. `siemens_nx.md` §7.1 "One package lane relates to one simple-hole construction group when their four resolved block identities are equal in serialized order" asserts the same ownership in its fourth sentence and then states that the equality does not assign hole parameters, placement, output, suppression, or dependency direction. The decoder applies the ownership: `hole_package_projection` in `crates/cadmpeg-codec-nx/src/native/attach.rs` collects the group's `SIMPLE HOLE` labels as internal operations, and `attach_feature_operations` removes them from the emitted features and transfers their output, diameter, treatments, and axis placements to the package. This item says that field is not yet identified, so one of the three must change. Resolve the specification against the serialized evidence before the decoder keeps deleting history features.

**Note.** The closure commit matched four resolved blocks and used synthetic feature records to exercise the projection. `native/attach.rs:5753-5869` still infers ownership from block equality and suppresses the child operations. A parent, child, or operation-role field has not yet been identified in corpus NX records. The equality was promoted to a hierarchy witness, so this item is reopened.

### OM-31. Feature-history construction-order evidence

**Question.** Which serialized field establishes the construction order of feature-history operations, inside one record area and between record areas?

**Known.** `siemens_nx.md` §7.1 "Within one feature-history record area, operation records are stored in reverse" and `siemens_nx.md` §7.1 "Neutral feature ordinals and dependency precedence order labels by descending" define the reversed order inside one area and the serialized order between areas. Neutral feature ordinals, dependency precedence, body lineage, and terminal-body selection all derive their order from that reversal. An operation label carries a name, four object-index lanes, and a source offset. It carries no sequence number, no timestamp, and no predecessor reference.

**Need.** We must know the field to order operations when one record area holds records of more than one construction generation, or when the serialized area order is not the construction order. Record order is the only current witness, so a file that stores areas or records in another order gives a reversed history with no diagnostic.

**Note.** The closure sorts feature sections by `min_source_offset` in `crates/cadmpeg-codec-nx/src/native/features.rs:39-58` and derives chronology from that order. The tests construct source offsets; they do not compare them with an independent serialized sequence or dependency field. Source-offset order was promoted to construction chronology, so this item is reopened.

### OM-32. All-terminal body-lineage mappings

**Question.** Which serialized state separates a complete body mapping in which every emitted body is terminal from a mapping whose terminal status is unresolved?

**Known.** `siemens_nx.md` §7.1 "Bodies named by validated segment binding tuples exist at the start of retained feature history." and `siemens_nx.md` §7.1 "A complete mapping may retain every emitted body; this is a resolved all-terminal result, not an unresolved selection." define writer and consumption ordering and admit the all-terminal case as resolved. Both a file whose operations supersede no body and a file whose lineage evidence is absent produce the same all-terminal set.

**Need.** We must know the state to separate the two cases. Without it, a part whose lineage evidence is missing transfers every emitted body as a current body instead of reporting the selection as unresolved.

**Note.** `crates/cadmpeg-codec-nx/src/decode.rs:1948-2001` treats `mapped == emitted` as the resolved all-terminal case. A synthetic empty-lineage case produces the same set as a valid all-terminal file, and a serialized discriminator has not yet been found in corpus records. This is a promotion of output compatibility to lineage evidence, so the item is reopened.

### OM-33. Decisive active-body membership

**Question.** What makes an `RMFastLoad` membership assignment decisive for one body image against another?

**Known.** `siemens_nx.md` §7.2 "`RMFastLoad` stores the active object-id set alongside the partition and deltas body records." defines the membership table, the shared FACE, EDGE, and VERTEX identity space, independent per-image assignment, and the rule that an image without active membership is retained unless another image has a decisive membership assignment. It does not define decisive.

**Need.** We must know the condition to select the active bodies. The current decoder retains every image whose complete nonempty FACE, EDGE, and VERTEX node-ID set is a subset of the active set, but the format does not establish whether that subset relation is decisive, whether active IDs may be stale or unioned, or how multiple matching images should be handled. The selection deletes other bodies and their complete topology and geometry from the model, so an unsupported membership rule removes a current body permanently. The exact feature-history rule runs only when this condition declines, so membership semantics take precedence over it.

**Note.** `crates/cadmpeg-codec-nx/src/decode.rs:1905-1945` selects every body whose complete nonempty topology-ID set is a subset of the active set. The subset rule, active-set authority, and union/stale behavior are not independently evidenced by a real NX file; the regression fixtures construct the sets consumed by the rule. The item is reopened.

### OM-34. OM registry schema-role precedence

**Question.** Which schema role does a linked OM registry take when it declares more than one role marker?

**Known.** `siemens_nx.md` §2 "Linked OM registries define their schema role by exact declarations:" names `UGS::Solid::Topol` for the model store, `UGS::FEATURE_RECORD` for feature history, `UGS::EXP_expression` for expressions, and `UGS::OM::SaveAuditTrail` for audit data when no preceding specialized marker applies. It orders the audit-data marker against the others. It does not order the first three against each other.

**Need.** We must know the precedence, or the rule that makes the first three markers mutually exclusive. The decoder tests them in a fixed order and takes the first present marker without testing the others. The role selects which sections the feature-history extractors walk, so a registry that carries two markers can supply operation labels, body references, and lineage from a store that is not feature history.

**Note.** `crates/cadmpeg-codec-nx/src/native/segments.rs:498-512` now rejects multiple role markers as ambiguous, but that is a conservative policy, not evidence that multiple markers are invalid or that one role has precedence. The closure test uses synthetic marker combinations. No serialized role discriminator was found, so this item is reopened.

### OM-35. Tagged-reference admission in a bounded record

**Question.** Which field separates a tagged-reference stream from per-class field data inside one bounded OM record?

**Known.** `siemens_nx.md` §7.1 "**Persistent-handle identity.** `e0 + handle:u32 BE` values are persistent handles forming a cr" defines the persistent-handle and tagged-reference token forms and states that they occur as pairs inside one externally bounded record. It defines an unconditional retention rule for offset-store control blocks only. It states no admission rule for an OM entity record.

**Need.** We must know the field to admit the correct references. A marker-shaped word can also be ordinary field data, so a token scan alone cannot separate the two. The decoder resolves this with two invented numbers: it accepts the longest suffix that holds at least eight persistent handles and whose tokens cover at least nine tenths of the remaining bytes. A shorter reference run is dropped complete and reports no loss. Field bytes before a long run are admitted as references and reach the model with decoded values.

**Note.** `crates/cadmpeg-codec-nx/src/om.rs:5379-5402` admits a tagged `28` token only when it immediately follows a persistent `e0` token. That adjacency rule is still a parser heuristic; the closure removed the prior density threshold but did not establish that an intervening field is impossible or that every adjacent pair is a reference. Synthetic paired-token tests are not independent serialized evidence. The item is reopened.

### OM-36. Named payload interval terminator

**Question.** What ends a named payload interval in an offset store?

**Known.** `siemens_nx.md` §7.1 "A named payload interval whose name is exactly `Point` followed by a positive decimal ordinal is a sketch point" defines the interval as ending exclusively at the next complete name field or at the reconstructed payload boundary, and rejects the typed point when an additional scalar occurs. `siemens_nx.md` §7.1 "A sketch payload name field is `66, compact_type, 03, declared_len:u8, text[declared_len-2], 00`" defines the name field. Block boundaries do not delimit values or named-record boundaries.

**Need.** We must know the terminator to apply the scalar-cardinality rule. The decoder adds one data block at a time and stops at the first accumulated span that holds exactly two scalars, so it never observes a third scalar in a later block. An interval that the format rejects then transfers as a typed point carrying the first two of its values.

**Note.** `crates/cadmpeg-codec-nx/src/om.rs:836-873` adds the current scalar-count and next-name checks, but the closure relied on synthetic named blocks and does not establish that the next name field is the serialized interval terminator in all offset-store records. The point interpretation remains a promoted framing rule, so this item is reopened.

### OM-37. Final field declaration in a pointerless OM section

**Question.** What terminates the final member-field declaration in a section without a unique valid record-area pointer?

**Known.** `siemens_nx.md` §7.1 "The first record at `oid_end` begins `04 01, declared_len:u8, version_text[declared_len-2], 00`" defines the product record and the bounded registry suffix through the next length-framed `m_` declaration. A section-relative `u32 LE` word after the type registry is a record-area pointer only when its forward target remains inside the section, starts with the three control words and product record, and is unique. When valid, that pointer bounds the complete field registry.

**Need.** We must know the terminal marker or alternate boundary for the final field declaration when no valid record-area pointer exists. A pointerless section cannot establish a complete field registry from the settled byte structure alone.

### OM-38. `RMFastLoad` membership table location

**Question.** Which field gives the position of the `RMFastLoad` active object-id table?

**Known.** `siemens_nx.md` §7.2 "`RMFastLoad` stores the active object-id set alongside the partition and deltas body records." defines the table as a little-endian count word followed by exactly that many ordered identity words, and states that FACE, EDGE, and VERTEX identities share the space. It does not give the position of the table.

**Need.** We must know the position field. The decoder walks forward from the class marker and takes the first offset whose count word and following identity words fall inside fixed numeric ranges. The count must reach fifty, so a part with fewer active identities never matches its own table, and the active-body selection silently does not run. A count above the upper range is rejected the same way. This location rule supplies the input to the membership decision in OM-33.

**Note.** `crates/cadmpeg-codec-nx/src/container.rs:400-435` takes the first count after the `UGS::Solid::Topol` marker whose candidate span reaches the product record. A plausible earlier count inside the bounded range can win before the real membership table, and the closure tests only synthetic placement. The first-candidate rule is not yet verified by a corpus field or invalidation witness, so this item is reopened.

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

### AM-11. JT 9 high-degree lane count

**Question.** What field gives the number of high-degree face-attribute-mask lanes in a JT 9 topologically compressed representation?

**Known.** `siemens_nx.md` §2.3 "The JT 9 topologically compressed representation begins with Int32 Compressed Data Packet Mk. 2" defines the fixed prefix packets, the split packets, the vertex-record header agreement test, and the rule that exactly one lane count must satisfy it. It states that one or more high-degree lanes occur. It gives no count field and no maximum.

**Need.** We must know the field to frame the packet sequence directly. The decoder tries lane counts from one to sixty-four and keeps the unique count that satisfies the agreement test. Sixty-four is not a format bound. A representation that carries more lanes matches no count, and the decoder then drops the topology, vertex-record, and coordinate-array data and every mesh derived from them.

**Note.** The closure removed the `1..=64` ceiling and scans until a unique vertex-header agreement in `crates/cadmpeg-codec-nx/src/native/display_jt.rs:817-911`, but a corpus JT representation has not yet verified that the packet stream has no count field or maximum. The regression fixture was constructed with sixty-five lanes to exercise the new scan. The lane-count rule remains unsupported, so this item is reopened.

## 4. Test evidence
