# Autodesk ShapeManager (ASM) stream: Open Items

This document lists the parts of the ASM stream format that we do not know. The specification `asm.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

A reference to the specification gives the section number and the start of the paragraph. An example is `asm.md` §6.3 `off_spl_sur`. Do not use line numbers. Line numbers become incorrect when the specification changes. The `scripts/check-doc-anchors.py` command makes sure that each reference finds exactly one paragraph.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Geometry carriers

### GC-01. `VBL_OFFSURF` and `skin_spl_sur2` payload layout

**Question.** What fields does a `VBL_OFFSURF` payload have? What fields does a `skin_spl_sur2` payload have?

**Known.** `VBL_OFFSURF` has the second spelling `offsetvbsur`. Most of these records include a final solved cache. The cache gives the exact face shape. An offset surface whose support is a `VBL_SURF` stays an `off_spl_sur` with the vertex-blend surface in the support slot: it is accepted on read in that form and is retained in it, and no rewrite into `VBL_OFFSURF` occurs. A `skin_spl_sur`, `skin_spl_sur2`, or `skinsur` payload written from the field order in `asm.md` §6.3 is refused on read, in a cache-first and in a context-first arrangement. The layouts are speculative, so a refusal separates neither a wrong field order from a wrong field count nor either of those from an unaccepted record name.

**Need.** Some records do not include a cache. For those records, the field layout is the only source of the face shape. We must know the layout to read them and to write them. The decoder keeps these records as opaque bytes now.

### GC-05. Second boolean of the `off_spl_sur` sense pair

**Question.** What does the second boolean of the revision-gated `off_spl_sur` sense pair control?

**Known.** `asm.md` §6.3 `off_spl_sur` gives the first boolean and the offset construction. The second boolean does not select the offset side and does not move the offset surface. It is set only in records whose first boolean is also set, and those records have a support surface whose stored parameterization is reflected relative to the model. All four states are accepted on read with either sign of the stored distance, and each is retained. A state change alone does not make the reader solve the cache again, so a record whose tail stores a cache gives no surface that separates the states.

**Need.** We must know which value to write when we make this record from a neutral model. `false` is the value for a surface built directly from a support surface and a distance, so only the reflected case remains open. A document whose `off_spl_sur` record stores a reflected support and a tail without a cache lets us read the states off the solved surface.

### GC-07. `off_spl_sur` extension run

**Question.** What are the field roles of the extension run in an `off_spl_sur` record?

**Known.** `asm.md` §6.3 `off_spl_sur` gives the run's field sequence, the prefix states that require it, the zero first integer, and the unit-independent tolerance. A run written to that sequence is accepted on read and is retained field for field, so the sequence is the true layout. The first fields agree with the start of a surface-to-surface intersection payload. That start is a presence logical, a six-integer header, an intersection curve, two pcurve logicals, and a tolerance. The four `-1` integers at the end are more than the three endpoint-term slots of that layout. The five integers after the leading zero are retained verbatim and have no established meaning. A run whose four `-1` integers hold `0 1 2 3` is refused on read, and so is a run whose two post-curve logicals are both true; a refusal does not give the accepted sets.

**Need.** We can now make a record that has a run, but only by copying a run we read. To make one from a neutral model we must know what the five integers, the two logicals, and the four `-1` integers hold, and which values each accepts.

### GC-08. Shared revision-gated surface tail enum values other than `0` and `2`

**Question.** What layouts do the shared revision-gated surface tail enum values other than `0` and `2` select?

**Known.** `asm.md` §6.3 "**Revision-gated spline-surface forms**" gives the layouts of `0` and `2`. Zero selects the solved cache and its fit tolerance; two replaces both with the two bool-gated parameter intervals and four closure and singularity enums. The decoder keeps a record with any other value as opaque bytes. Values `3` and `4` paired with the complete value-zero cache payload are refused on read. Tails written with the spelling `historical`, `optimal`, `none`, or `summary` and with the payload the same-named `law_spl_sur` mode selects are also refused. Each refusal rejects only the complete submitted value-and-layout pair; it does not reject that numeral with another layout or narrow the accepted spelling set.

**Need.** We cannot read a record with a value other than `0` or `2`. **Settling specimen:** a document whose shared surface tail carries another value gives that value's layout, and a document whose `off_spl_sur` tail carries form `2` shows the cacheless form on that carrier.

### GC-13. `cl_loft_spl_sur` tail kind values

**Question.** What are the nonzero values of the revision-gated `cl_loft_spl_sur` tail-kind integer? What layout does each nonzero value select?

**Known.** `asm.md` §6.3 `cl_loft_spl_sur` gives the kind-zero payload. The revision-gated form takes tail kind `0`; no nonzero kind has a known occurrence in that form. The decoder rejects every nonzero kind. A revision-gated payload written with tail kind `6` or `7` and with the kind-6 or kind-7 payload of the form that is not revision-gated is refused on read. A refusal separates a kind the revision-gated form does not have from a payload the revision-gated form encodes differently, so it does not bound the kind set.

**Need.** We cannot read a record with a nonzero kind.

### GC-16. Token tags of a revision-gated `VBL_SURF` `deg` boundary

**Question.** Which token tags does a revision-gated `VBL_SURF` `deg` boundary use for its location and for its two normals?

**Known.** `asm.md` §6.3 `VBL_SURF` gives the revision-form tag changes for the type names, the magic location, the support bounds, the curve endpoints, and circle form `3`. It does not give the tags of a `deg` boundary. The decoder writes `0x13` and two `0x14` tokens. In a record whose boundaries are accepted, a `plane` support stores its location in one `0x13` and each of its two directions in one `0x14`, and the leading triple of a boundary is one `0x14`, so the decoder's choice agrees with the tags the same record uses for a location and for a direction. A record whose boundary count is raised by one with a `deg` boundary appended is refused on read, with equal and with distinct normals alike. The appended boundary does not agree geometrically with the other boundaries and the record stores no cache to fall back on, so the refusal does not bear on the tags.

**Need.** A wrong tag makes a file that the reader refuses.

### GC-18. Blend-value selector values

**Question.** We must find three answers:

- whether selector values outside `0` through `7` exist
- the cross-sections selectors `2`, `4`, `5`, and `6` build
- the relation between a selector-seven `edge_offset` length and the solved second-side contact distance

**Known.** `asm.md` §6.3 `var_blend_spl_sur` gives the layouts of selectors `0` through `7`, the classified cross-section families, the side order, and the contact offsets. Selectors `2`, `4`, `5`, and `6` carry no selector-local payload; each retains its numeric value and continues at the same common tail. Their records store current solved caches, so the caches do not identify the cross-section laws. A rebuilt selector-seven surface meets its first side at the stored offset exactly and its second side short of the stored offset by near one per cent, far above the achieved fit tolerance, so the selector-seven offset is not exactly the second-side contact distance and no other relation is established.

**Need.** A record using a selector outside `0` through `7` would extend the value set. A record whose selector `2`, `4`, `5`, or `6` cache is not current would rebuild into the corresponding cross-section law.

### GC-19. Third-slot raise predicate of a `tvertex` record

**Question.** Which incident edges contribute the third slot's `1e-6` raise?

**Known.** `asm.md` §5.2 "**Tolerant vertex:**" gives the three slots, the first slot's content — the vertex's own stored tolerance, unit-converted like the other two slots — the second slot's construction from the incident edge-endpoint gaps and tolerant-edge tolerances, the third slot's raise over the incident edges that are not tolerant, and the set-state relation between the second and third slots. The predicate that selects which incident edges contribute the raise is narrower than "every edge that is not tolerant": some records carry a third slot equal to the second where that rule predicts a raise. A record whose second and third slots are equal does not separate candidate predicates.

**Need.** To write the third slot exactly we must know the contribution predicate. The decoder retains the stored slots, so the item blocks only writing from a neutral model.

### GC-21. Revision-gated `loft_spl_sur` type-zero member and ASM-integer gate

**Question.** What do the two nullable spline slots of a type-zero profile member hold, and are they BS2 pcurves or BS3 curves? What form does a type-zero member take in a save-format-23200 stream? At which save format version does the ASM integer start?

**Known.** `asm.md` §6.3 `loft_spl_sur` gives the two member forms and the save-format gate of the ASM integer. A type-zero member stores two nullable spline slots in place of the support surface and the first flag. The decoder keeps both slots and reads them as BS2 pcurves. Every observed type-zero slot is the null-spline sentinel, and that sentinel is the same token for a BS2 and a BS3 slot, so the slot type is undetermined: only a type-zero member with a non-null slot separates them, by the count of scalars per control point. No type-zero member occurs in a save-format-23200 stream. No stream with a save format version between 22600 and 23200 holds a revision-gated loft. The decoder reads the ASM integer in each stream with a save format version above 22600.

**Need.** We keep the two slots without a change. To write them from a neutral model, we must know what they hold. To write a type-zero member into a save-format-23200 stream, or into a stream with a save format version between 22600 and 23200, we must know if that stream keeps the ASM integer. **Settling specimen:** a document whose type-zero member carries a non-null spline slot separates the slot types by the count of scalars per control point, and a save-format-23200 stream holding a revision-gated loft settles the gate.

### GC-23. Cache-first intcurve leading enum values other than `0` and `2`

**Question.** What layouts do the cache-first intcurve leading enum values other than `0` and `2` select?

**Known.** `asm.md` §6.3 "**Cache-first subtype selection**" gives the layouts of `0` and `2`. The decoder reads both and retains a record with any other value verbatim. Value `2` paired with the complete value-zero cache payload is refused on read; the valid value-two layout replaces that cache rather than retaining it. A `par_int_cur` whose leading enum carries the spelling `summary`, `historical`, or `optimal` over an otherwise untouched cache-first payload is also refused. Each refusal rejects only the complete submitted value-and-layout pair and does not narrow other layouts or spellings.

**Need.** We cannot read a record with a value other than `0` or `2`. **Settling specimen:** a document whose cache-first intcurve leading enum carries a value other than `0` or `2` gives that value's layout. GC-27 gives the separate limit that value `2` reaches.

### GC-24. Binding of the law formula text infix operator `O`

**Question.** What are the precedence and associativity of the infix operator `O` in stored law formula text?

**Known.** `asm.md` §6.3 "**Law formulas**" gives `O` as composition with the right operand innermost, and gives the `MTRAIL` curve as a rail direction requiring no further construction input. A writer parenthesizes both `O` operands, so stored text never exercises the operator's binding against a neighbouring operator and never chains two occurrences. A `law_int_cur` written from the field order in `asm.md` §6.3 over a solved curve cache is refused on read, and the refusal covers the smallest form, whose law is `null_law` and which carries no operator at all. The field order and the arity encoding are both speculative, so the refusal bears on neither the operator token spellings nor the binding, and it does not show that the record is unreachable by a different field order.

**Need.** We must know the binding to parse law text that a different writer produced without full parenthesization. Text this codec emits is unaffected, because it parenthesizes both operands.

### GC-25. Payload after a true shared revision-gated surface tail logical

**Question.** What follows the closing logical of the shared revision-gated surface tail when that logical is true?

**Known.** `asm.md` §6.3 "**Revision-gated spline-surface forms**" ends the tail with six counted float arrays and one logical, for each value of the form enum. Revision-gated cylinder, loft, sweep, offset, revolution, and sum surfaces end at that tail and require the subtype close immediately after it. Other admitted shared-tail subtypes consume their defined suffix and then require the close. The decoder retains a construction with another token as opaque data while continuing to use a valid solved cache as the face carrier.

**Need.** A false logical is the only state with established semantics. Carriers with their own fields after the tail, such as `rb_blend_spl_sur` and `var_blend_spl_sur`, read their defined suffix immediately after either logical value. **Settling specimen:** a document whose shared revision-gated surface tail closes with a true logical shows whether the logical has semantic effect or selects a different suffix grammar.

### GC-26. Position of the `sss_blend_spl_sur` third-side graph

**Question.** Does the third-side graph of an `sss_blend_spl_sur` record come between the shared revision-gated surface tail and the three trailing integers, or after those integers?

**Known.** `asm.md` §6.6 `rb_blend_spl_sur` puts the third-side graph after the tail and before the three `tail_extension` integers. The two-support subtypes end with the tail and those integers, which fixes the integers as the last fields of that scope but does not fix the third-side graph against them. Replacing only the accepted subtype name `rb_blend_spl_sur` with `sss_blend_spl_sur` is refused in a fresh reader session while the unchanged control is accepted. The rename-only payload is therefore not the `sss_blend_spl_sur` grammar, but the refusal does not select either candidate graph position.

**Need.** The decoder and the source-less writer both use the position the specification gives. The wrong position makes every `sss_blend_spl_sur` record fail its decode and stay opaque, and makes a generated record ungrammatical. **Settling specimen:** a document holding an `sss_blend_spl_sur` record fixes the graph position against the trailing integers.

### GC-27. Solved carrier of a cache-first intcurve that stores no cache

**Question.** Which curve gives the parameter domain of a cache-first intcurve record whose leading enum is `2` and whose construction stores no curve block?

**Known.** `asm.md` §6.3 "**Cache-first subtype selection**" gives the layout of leading enum `2`: the record stores a bool-gated curve interval and a closed-form enum in place of the solved-curve cache and the fit tolerance that enum `0` stores. The shared cache-first context takes its parameter domain from the record's solved curve, and the record-level search takes the first curve block in the record as that curve. A form-`2` record therefore has a solved carrier only when a nested construction stores a curve block.

**Need.** A form-`2` record that stores no curve block anywhere gives the context no parameter domain, so the decoder retains the record verbatim and the neutral model loses the curve. To read such a record the shared context and every carrier that builds it must accept a record with no solved curve. Whether the record then takes its domain from the interval the form stores, from the support surfaces, or from a curve outside the record is not established. **Settling specimen:** a document holding a form-`2` record whose construction stores no curve block settles which of the three the domain comes from.

### GC-28. Parameter chart of a procedural spline support cache

**Question.** How does a pcurve in a procedural spline support's construction chart map to the parameter chart of that support's solved NURBS cache?

**Known.** A cache-first intcurve support can store `spline`, a subtype-table reference to a procedural spline-surface construction, and four optional bounds. The referenced construction supplies a solved NURBS cache. The intcurve pcurve uses the procedural construction's parameter chart. That chart is not necessarily the solved cache's chart. A `cl_loft_spl_sur` support can map one construction-chart isoline to a nonlinear curve in the solved cache chart. The intcurve and the cache therefore do not establish a direct pcurve-on-surface relation without a chart map.

**Need.** The decoder currently attaches the construction-chart pcurve directly to the solved NURBS support. This relation is invalid when the charts differ. We must retain or derive the exact chart map before the neutral support relation can be complete. A fitted map is not sufficient because it does not preserve the stored construction semantics.

### GC-29. Ownership of multiple 3D curve cache blocks

**Question.** Which 3D curve block is the writable cache when a carrier record contains multiple decodable blocks or supports more than one integer width?

**Known.** `first_curve_patch_layout` scans the admitted integer widths and marker positions and accepts the first block that decodes. `final_curve_patch_layout` uses the final decodable block for a different caller. Neither function verifies a record-specific owner, cache role, or relationship between the selected block and the carrier subtype.

**Need.** A record with more than one decodable 3D curve can make the writer patch a support or pcurve instead of the writable cache. We need an owner reference, subtype rule, or full-consumption invariant before the first-block selection can be used for writing.

### GC-32. Unadmitted rolling-ball branches

**Question.** Does an `rb_blend_spl_sur` record outside the complete and compact grammars carry another defined layout?

**Known.** `asm.md` §6.6 gives both admitted layouts. The complete form has ordered side graphs, a slice curve, offsets, radius selection, and cross-section fields. The compact form has a positional run of labelled supports, one spine, two radius values, enum `-1`, one solved cache, and an optional fit tolerance. A record outside both grammars remains opaque and retains its native payload and solved cache; it does not infer members by encounter order.

Optional side ranges, locations, and other doubles can occur after the actual offsets, so scalar encounter order does not identify the radius law. A support or nested construction can contain a later curve block, so curve encounter order does not identify the slice. The serialized cross-section selector, not subtype membership, determines whether the section is circular.

**Need.** A branch outside both admitted grammars must identify each field and state whether that branch is recoverable.

**Note.** The complete decoder and its structured grammar establish conforming records only; they do not establish the layout of a rejected branch.

### GC-33. Cone pcurve chart sign and scale

**Question.** What gives the sign and scale of the axial coordinate of a pcurve on a native `cone` support?

**Known.** `asm.md` §6.2 states that the first cone-chart coordinate is multiplied by `direction * cosine * u_scale`, with `direction` selected by `sine * cosine`. `native_support_chart` and `normalize_pcurve_for_surface_record` in `nurbs/proc_curve.rs` implement that formula.

**Conflict.** The current rule was written from the implementation change and synthetic token tests; no SAT/SAB witness in the corpus has yet separated this formula from the preceding `direction * u_scale` interpretation. The parse-failure branch also substitutes the canonical chart with no loss.

**Need.** A cone pcurve with a negative cosine, an offset-derived `u_scale`, and a known surface position would distinguish the chart direction and scale. Without that evidence, the current text is a promotion of the implementation's choice, not proof that the file format uses it.

**Note.** QA sweep locations: `crates/cadmpeg-asm/src/nurbs/proc_curve.rs:79-164`; the synthetic tests are at `:3111-3145`. A test that supplies tokens constructed by the same rule is counter-evidence only for arithmetic consistency, not for the native rule.

### GC-34. Pcurve interval eligibility tolerance

**Question.** What tolerance applies when an edge-use interval is compared with a pcurve knot domain?

**Known.** `asm.md` §6.4 says that only intervals whose endpoints lie in the pcurve knot domain are eligible and that the full knot domain is the fallback when neither signed edge interval is eligible. `pcurve_ranges_on_domain` in `brep/geometry.rs` uses `1.0e-9 * max(abs(domain span), 1.0)`, clamps an accepted overshoot to the knot boundary, appends the full domain, and its caller takes the first range.

If an endpoint overshoots by less than the hard-coded tolerance, the decoder silently changes it. If it overshoots by slightly more, the signed interval is rejected and the whole pcurve domain becomes the trim. The transition is an unrecorded selection threshold and no loss identifies it.

**Need.** A source rule or a specimen with endpoint error near the boundary must settle the tolerance, the clamping rule, and whether the full-domain fallback is valid after rejection.

**Note.** QA sweep locations: `crates/cadmpeg-asm/src/brep/geometry.rs:386-420` and `crates/cadmpeg-asm/src/brep/topology.rs:355-372`. The ordered signed candidates and full-domain fallback are documented, but the numeric threshold is not.

### GC-35. Reversal of an embedded blend support

**Question.** Which member of an embedded blend support gives the surface-normal side of that support?

**Known.** `cadmpeg-ir` defines `BlendSupport.reversed` as selecting the opposite surface-normal side. `emit_blend_surface` writes `reversed: false` at both support construction sites. The embedded support readers in `nurbs/proc_curve.rs` consume and discard a Boolean after analytic support fields: the plane, cone, sphere, and torus branches each read one such Boolean. `asm.md` §6.2 gives the top-level analytic layouts without assigning this trailing Boolean to a support-reversal member.

If the discarded Boolean is the support reversal, every reversed blend support is emitted on the wrong side. If it is another native field, `BlendSupport.reversed` has no reader and the neutral field is misleading. In both cases the current code selects a meaning without evidence.

**Need.** A paired support/face specimen or an authoritative field description must bind the Boolean to surface reversal or to another native member. The answer determines whether the neutral field must be populated or removed from this path.

**Note.** QA sweep locations: `crates/cadmpeg-asm/src/nurbs/proc_curve.rs:2481-2583,2631-2733`, `crates/cadmpeg-asm/src/nurbs/blend.rs:384-440`, and `crates/cadmpeg-asm/src/brep/emit.rs:2441-2483`. The field is structurally consumed, but no current specification paragraph assigns its semantics.

### GC-36. Ownership and width of decoded NURBS cache blocks

**Question.** Which NURBS block is the owning cache when a record contains multiple decodable blocks or when more than one integer width parses a candidate?

**Known.** `marker_positions` scans every marker without nesting, while `owned_marker_positions` exists specifically to exclude nested construction scopes. Token readers still use the raw scan in `surface_cache`, `curve_cache`, `decode_pcurve_cache`, `decode_curve_cache`, and the compound/directrix helpers. `surface_cache` chooses the first block when any `comp_spl_sur` bytes occur anywhere and otherwise the last block; `curve_cache` chooses the first. Binary readers try `INT_WIDTHS = [8, 4]` and return the first width with a decodable block. `procedural_curve_recursive` changes the ordinal to last for wrapper flags and first for other families. No path counts competing valid blocks or withholds on a tie.

If a nested support cache precedes an owning cache, a raw scan can return the support. If a wrapper has a source curve before its solved cache, a first-block caller returns the source; a last-block caller applied to a non-wrapper returns a later nested curve. A second valid block is accepted without an owner reference, and a wrong-width parse is accepted without a stream-width witness. The resulting face, edge, pcurve, or patch can therefore use a different carrier while remaining numerically valid.

**Need.** A specimen with multiple valid blocks and the stream header's integer width must establish the owner, scope boundary, ordinal, and whether a second candidate is invalid. Until then every read and patch path must use the owning scope and known width, and must withhold when two blocks remain valid. GC-29 covers the separate writer helper; this item covers the read paths.

**Note.** QA sweep locations: `crates/cadmpeg-asm/src/nurbs/reader.rs:20-26,52-92`, `crates/cadmpeg-asm/src/nurbs/core.rs:146-187,551-630`, `crates/cadmpeg-asm/src/nurbs/pcurve.rs:160-171`, and `crates/cadmpeg-asm/src/nurbs/proc_curve.rs:560-583,2876-2935`. The format paragraphs in `asm.md` §6.3-§6.6 assert first/final cache roles, and owned-scope helpers exist, but neither is an independent witness for every generic caller.

## 2. Topology

### TG-01. Missing partner coedge substitution

**Question.** How does a reader represent a coedge whose stored partner is not kept in the reachable topology?

**Known.** `asm.md` §5.2 gives the coedge partner link and §5.4 gives the radial pairing invariant. `emit_coedges` filters the stored partner through the kept-coedge set. When the partner is absent, it writes `radial_next` back to the coedge itself. A partner can be absent because an adjacent face or carrier was dropped for a missing or dangling surface reference in `keep_faces_and_carriers`. No loss records the replacement.

If one coedge of a valid pair is dropped because its face carrier is unavailable, the surviving coedge is emitted as a self-ring. Validation accepts the self-ring as a laminar boundary, so a broken decode is represented as a different topology rather than as an unresolved partner.

**Need.** A source rule or a retained native reference must distinguish a genuinely laminar self-ring from a partner lost during reachability filtering. The decoder must withhold the coedge or report the missing partner until that distinction is known.

**Note.** QA sweep locations: `crates/cadmpeg-asm/src/brep/topology.rs:90-103` and `crates/cadmpeg-asm/src/brep/emit.rs:3690-3751`. Self-radial laminar boundaries are valid counter-evidence, but the current code uses the same value for a dropped partner.

### TG-02. Body-kind inference from edge-use counts

**Question.** Which native member gives an ASM body's `Solid`, `Sheet`, or `General` kind?

**Known.** `asm.md` §5.2 documents the body flags/history field and topology links but does not define a body-kind member. `classify_body_kinds` in `brep/geometry.rs` sets `Wire` or `General` from visible topology, then labels a face-bearing body `Solid` when every counted edge has exactly two coedge uses and `Sheet` otherwise. It runs only after model transfer and records no inference or loss.

If a closed sheet or a body with a non-manifold internal face has every edge counted twice, the rule emits `Solid`. If a solid has a missing coedge, it emits `Sheet`. The same topological count can therefore select different native body meanings without reading a source discriminator.

**Need.** A body-kind field, an authoritative topology invariant, or a specimen with a body whose stored metadata separates solid and sheet states must settle the rule. Until then the neutral body kind is an inference from the output graph.

**Note.** QA sweep locations: `crates/cadmpeg-asm/src/brep/geometry.rs:1099-1168` and `crates/cadmpeg-asm/src/brep/mod.rs:673-681`. The edge-count rule is valid for ordinary two-manifold solids, but that does not establish it for all bodies.

### TG-03. Normal sign of an analytic procedural carrier

**Question.** Does a procedural circle/extrusion or rolling-ball construction with an antiparallel frame carry the same natural normal as the direct construction, or must its axis or face sense be reversed?

**Known.** `asm.md` §6.6 says that an extrusion direction is parallel to the circle normal and that a rolling-ball carrier uses parallel support and spine directions. `analytic_procedural_surface` tests `abs(dot)` and therefore accepts both parallel and antiparallel vectors. It stores the extrusion direction or spine direction as the IR axis without a sign correction. `emit_faces` reverses only for a record-level reversed spline or a marked inward-normal analytic surface.

If the directrix normal is opposite the chosen cylinder axis, the point set is unchanged but the IR surface parameterization and natural normal can be opposite. The face then keeps the wrong winding because the analytic recognition path does not provide a reversal marker.

**Need.** A specimen with an oriented directrix and an antiparallel extrusion or blend axis must settle whether parallel means oriented parallel, either sign, or a sign plus face-sense rule.

**Note.** QA sweep locations: `crates/cadmpeg-asm/src/brep/geometry.rs:625-652,732-782` and `crates/cadmpeg-asm/src/brep/emit.rs:3830-3847`. Existing tests deliberately use an antiparallel extrusion and therefore prove only that the implementation accepts it.

## 3. Attributes

### AT-01. Precedence of multiple colour attributes

**Question.** Which colour attribute wins when one ASM attribute chain contains more than one decodable colour record?

**Known.** `asm.md` §5.6 allows colour and feature-tag attributes to coexist but gives no precedence among `rgb_color`, `truecolor`, and `entatt_color-bt-attrib`. `attribute_chain_color` returns the first decodable matching record in chain order, skips an `rgb_color` whose channels fail the `0.0..=1.0` plausibility check, and then accepts a later colour record. It records no conflict or skipped candidate.

If a chain contains `truecolor` before `rgb_color`, the first record supplies the neutral colour even when the later record is the authoritative display colour. If the first `rgb_color` is out of range because its encoding is not normalized, the function silently selects a later record. Chain order and a numeric plausibility test therefore decide the result.

**Need.** A source rule or a specimen with multiple colour classes must establish precedence, channel units, and the treatment of an invalid first candidate.

**Note.** QA sweep location: `crates/cadmpeg-asm/src/brep/attributes.rs:132-193`. Chain order is counter-evidence only if the format explicitly defines it as precedence.

## 4. Text encoding

### TE-01. Migration-flag words of a `gen-attrib` record in the text encoding

**Question.** Which `ENUM_VALUE` integer does each migration-flag word of a text-encoded `gen-attrib` record select?

**Known.** `asm.md` §5.6 gives the binary attribute records. A binary `int64_attrib-name_attrib-gen-attrib` record stores four `ENUM_VALUE` tokens between its reference fields and its name string. The text encoding writes four words at those slots, from the set `keep`, `keep_one`, `keep_kept`, `ignore`, and `copy`. The word-to-integer map is not known. The text reader keeps these records with each word as an identifier token and does not select an integer, so a wrong integer cannot reach the attribute values.

**Need.** We must know the map to give a text-encoded `gen-attrib` record the same token stream as its binary form, and to write the words from a binary record.

### TE-02. History-partition marking in the text encoding

**Question.** How does a text stream mark a construction-history partition?

**Known.** `asm.md` §7.1 gives the header lines. The flags word keeps its binary semantics, so bit 0 is the history-partition flag. No further text-specific marking is known, and the record grammar for a text history partition is not known.

**Need.** A reader must know the marking to separate the solved records from history records; without it, a history-bearing text stream would read history records as model records.

### TE-03. Invalid text-header scale handling

**Question.** What scale values are valid in the SAT/SMT text header, and what must a reader do for zero, negative, or non-finite values?

**Known.** `asm.md` §7.1 defines `scale` as millimetres per model-space unit. `parse_header` accepts any value that `f64::parse` accepts and does not require finiteness, positivity, or an exact three-field line. `parse` converts positive values with `scale / 10.0` but substitutes `1.0` for every non-positive value; `NaN` also takes that branch because its comparison with zero is false. No loss is recorded.

If a stream carries zero, a negative value, or `NaN`, every length-bearing text field is decoded with the fallback factor instead of being rejected or retained. A malformed header can therefore produce plausible but wrongly scaled geometry and tolerances.

**Need.** An authoritative valid-domain rule or a specimen with an invalid header must settle whether the stream is malformed, uses a special unit convention, or requires a different conversion. The reader must not fabricate a scale while this is open.

**Note.** QA sweep locations: `crates/cadmpeg-asm/src/sat.rs:274-324,331-341`. The settled unit rule and existing valid positive headers are counter-evidence against treating the fallback as a valid convention.
