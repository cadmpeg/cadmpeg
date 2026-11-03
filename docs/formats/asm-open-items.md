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

### GC-32. Unadmitted rolling-ball branches

**Question.** Does an `rb_blend_spl_sur` record outside the complete and compact grammars carry another defined layout?

**Known.** `asm.md` §6.6 gives both admitted layouts. The complete form has ordered side graphs, a slice curve, offsets, radius selection, and cross-section fields. The compact form has a positional run of labelled supports, one spine, two radius values, enum `-1`, one solved cache, and an optional fit tolerance. A record outside both grammars remains opaque and retains its native payload and solved cache; it does not infer members by encounter order.

Optional side ranges, locations, and other doubles can occur after the actual offsets, so scalar encounter order does not identify the radius law. A support or nested construction can contain a later curve block, so curve encounter order does not identify the slice. The serialized cross-section selector, not subtype membership, determines whether the section is circular.

**Need.** A branch outside both admitted grammars must identify each field and state whether that branch is recoverable.

**Note.** The complete decoder and its structured grammar establish conforming records only; they do not establish the layout of a rejected branch.

### GC-33. Generator scale of a cone pcurve chart

**Question.** On a native `cone` with nonzero `sine`, is the axial distance of a pcurve point the first chart coordinate times `|cosine| × u_scale` or times `u_scale`?

**Known.** `asm.md` §6.2 "An embedded or face-bound pcurve on an analytic support" states `d = first × |cosine| × u_scale` along the native axis. Pcurves on circular cylinders with `cosine = 1` and with `cosine = -1` reach their edge vertices with `d = first × u_scale` along the native axis and the azimuth measured toward `axis × x`. The sign of `cosine` therefore does not change the chart. On a cylinder `|cosine| = 1`, so these pcurves do not separate the two scales. `analytic_support` in `nurbs/proc_curve.rs` implements the stated rule.

**Need.** A pcurve with known edge vertices on a cone with nonzero `sine` separates the two scales. With the wrong scale, every cone pcurve point has the wrong axial distance, and `cadmpeg check` reports the miss at the edge vertices.

**Note.** The unit tests in `nurbs/support_chart/tests.rs` construct vertices from the stated rule. They test the agreement of the chart conversion with the decoded cone frame for each sign of `sine` and `cosine`; they are not evidence for the native scale.

### GC-34. Pcurve interval eligibility tolerance

**Question.** What tolerance applies when an edge-use interval is compared with a pcurve knot domain?

**Known.** `asm.md` §6.4 says that only intervals whose endpoints lie in the pcurve knot domain are eligible and that the full knot domain is the fallback when neither signed edge interval is eligible. `pcurve_ranges_on_domain` in `brep/geometry.rs` uses `1.0e-9 * max(abs(domain span), 1.0)`, clamps an accepted overshoot to the knot boundary, appends the full domain, and its caller takes the first range.

If an endpoint overshoots by less than the hard-coded tolerance, the decoder silently changes it. If it overshoots by slightly more, the signed interval is rejected and the whole pcurve domain becomes the trim. The transition is an unrecorded selection threshold and no loss identifies it.

**Need.** A source rule or a specimen with endpoint error near the boundary must settle the tolerance, the clamping rule, and whether the full-domain fallback is valid after rejection.

**Note.** QA sweep locations: `crates/cadmpeg-asm/src/brep/geometry.rs:386-420` and `crates/cadmpeg-asm/src/brep/topology.rs:355-372`. The ordered signed candidates and full-domain fallback are documented, but the numeric threshold is not.

### GC-35. Reversal of an embedded blend support

**Question.** Which member of an embedded blend support gives the surface-normal side of that support?

**Known.** `cadmpeg-ir` defines `BlendSupport.reversed` as selecting the opposite surface-normal side. `emit_blend_surface` writes `reversed: false` at both support construction sites. The embedded support readers in `nurbs/proc_curve.rs` consume one Boolean after the analytic support fields of a plane, cone, sphere, or torus. `asm.md` §6.2 assigns that Boolean: it is the v-sense logical of a plane, sphere, or torus and the u-sense logical of a cone. It selects a parameter direction, not the surface-normal side. No other embedded support field is known to select the normal side, so `BlendSupport.reversed` has no reader on this path.

**Need.** A paired support/face specimen or an authoritative field description must give the native member that selects the normal side of an embedded blend support, or show that no such member exists. The answer determines whether the neutral field must be populated or removed from this path.

**Note.** QA sweep locations: `crates/cadmpeg-asm/src/nurbs/proc_curve.rs:2481-2583,2631-2733`, `crates/cadmpeg-asm/src/nurbs/blend.rs:384-440`, and `crates/cadmpeg-asm/src/brep/emit.rs:2441-2483`.

### GC-36. Ownership and width of decoded NURBS cache blocks

**Question.** Which NURBS block is the owning cache when a record contains multiple decodable blocks or when more than one integer width parses a candidate?

**Known.** `asm.md` §6.3 "**Construction-cache ownership**" defines ownership relative to the outer non-`ref` construction scope. The token-space `surface_cache`, `curve_cache`, pcurve fit-tolerance reader, procedural-curve record reader, and procedural-surface family readers consume only directly owned cache markers. The procedural-curve reader takes the last directly owned curve for source-wrapper families and the first for other families. Fixed-layout procedural-surface readers and the `comp_int_cur`, `subset_int_cur`, and `offset_int_cur` directrix helpers consume their construction fields in grammar order. The shared revision surface-tail reader returns the domain of the solved cache it consumes, so a containing construction does not scan the enclosing scope again. Generic binary surface, curve, and pcurve cache helpers try both integer widths but return a value only when exactly one `(width, marker)` candidate decodes; they do not infer a role from record-name bytes. Surface, curve, and pcurve patch-layout readers use directly owned markers at the stream's known integer width; their callers supply the final-cache, first-cache, or explicit-ordinal role. A generic binary helper can still accept one nested candidate because it has no enclosing-scope grammar.

If a nested support cache precedes an owning cache, a raw scan can return the support. If a wrapper has a source curve before its solved cache, a first-block caller returns the source; a last-block caller applied to a non-wrapper returns a later nested curve. A second valid block is accepted without an owner reference, and a wrong-width parse is accepted without a stream-width witness. The resulting face, edge, pcurve, or patch can therefore use a different carrier while remaining numerically valid.

**Need.** Every remaining read path must use the owning construction scope and the stream's known integer width, and must withhold when two blocks remain valid.

**Note.** QA sweep locations: `crates/cadmpeg-asm/src/nurbs/reader.rs:20-26,52-92`, `crates/cadmpeg-asm/src/nurbs/core.rs:146-187,551-630`, `crates/cadmpeg-asm/src/nurbs/pcurve.rs:160-171`, and `crates/cadmpeg-asm/src/nurbs/proc_curve.rs:560-583,2876-2935`. The format paragraphs in `asm.md` §6.3-§6.6 assert first/final cache roles, and owned-scope helpers exist, but neither is an independent witness for every generic caller.

### GC-37. Procedural construction identity with auxiliary outer definitions

**Question.** Which directly owned non-`ref` subtype definition identifies the procedural
construction when one record contains an auxiliary outer definition before the cache-bearing
construction?

**Known.** `asm.md` §6.3 "**Construction-cache ownership**" assigns the solved cache to the outer
non-`ref` construction that owns it. A record can contain auxiliary directly owned subtype
definitions before that construction. Cache selection admits the unique directly owned
cache-bearing scope. Construction-kind selection instead takes the first directly owned non-`ref`
subtype definition.

**Need.** Define the structural relation that identifies the record's procedural construction and
separates it from auxiliary outer definitions. The construction kind, construction scope, solved
cache, parameter domain, and native family must come from the same owner.

**Conflict.** An auxiliary definition that occurs first can supply `native_kind` while geometry comes
from a later unique cache-bearing construction. Family-specific interpretation can then use an
identity that does not own the admitted carrier.

**Note.** Do not use first-definition order as construction identity. Reject an ambiguous owner when
the record grammar does not identify one construction.

### GC-38. Reversed-sense pcurve charts

**Question.** Does a `TRUE` v-sense logical (`reverse_v`) negate the second pcurve chart coordinate of a plane, sphere, or torus, and does a `TRUE` cone u-sense logical (`reversed`) negate the first cone chart coordinate?

**Known.** `asm.md` §6.2 states both negations. In authored streams, the `forward_v` plane, sphere, and torus charts and the `forward` cylinder chart reach their edge vertices. No authored stream has a face-bound pcurve on a `reverse_v` plane, sphere, or torus, on a `reversed` cone, on a sphere with a negative radius, or on a torus with a negative minor radius. In the fifteen available Fusion designs, face-bound pcurves lie only on `forward_v` planes, spheres, and tori and on `forward` cylinders; the 54 `reverse_v` plane faces and the 205 `reversed` cylinder faces carry none, and no face lies on a `reversed` cone with nonzero `sine`, a sphere with a negative radius, or a torus with a negative minor radius. `analytic_support` in `nurbs/proc_curve.rs` implements the stated rule.

**Need.** A pcurve with known edge vertices on each of these supports, in a stream a modeller wrote, confirms the rule. A wrong sign mirrors the pcurve in the support chart, and `cadmpeg check` reports the miss at the edge vertices.

**Note.** Builder fixtures do contain these pcurves. `nurbs/support_chart/tests.rs` has one on every listed support, `nurbs/pcurve/tests/side_charts.rs` has blend pcurves on `reverse_v` planes and on a `reversed` inward cylinder, and the `cadmpeg-codec-sat` decode tests have a triangle on a `reverse_v` plane. Each fixture places its vertices by the stated rule, so it tests that the decoder follows the rule; it is not evidence for the native convention.

### GC-39. Chart of the secondary blend pcurves

**Question.** In which chart are the secondary and tertiary pcurves of a rolling-ball side, the secondary and tertiary pcurves of a rolling-ball third side, the second pcurve of a G2 blend side, the tertiary pcurve of a G2 first-side `none` branch, and the post pcurve of a variable blend stored?

**Known.** `asm.md` §6.2 gives the support chart of the primary side pcurve, which is the contact track. The layouts store the other pcurves after the same support, but no field relates them to that support or to another surface. The decoder keeps their stored control points. In the eight available Fusion designs, the 1120 rolling-ball sides store `nullbs` in the primary, secondary, and tertiary slots, the decoded variable blends store no post pcurve, and no record is a G2 blend.

**Need.** A stream with one of these pcurves present, together with the surface whose chart reproduces a known point of the construction, gives the chart. A pcurve on an analytic support then converts with the rule of §6.2, and the F3D writer inverts it.

### GC-40. Support parameters of a revolved conic

**Question.** How does the neutral model state an intersection support whose pcurves use the parameters of a `rot_spl_sur` construction with an `ellipse` profile, when the solved B-spline surface of that construction has other parameters at the same points?

**Known.** `asm.md` §6.3 `rot_spl_sur` gives the profile curve, axis, and solved surface. In an `int_int_cur` whose support is such a surface, the support pcurve stores the conic angle of the profile `ellipse` as U and the right-handed revolution angle about the stored axis direction as V. The solved surface uses rational quadratic arcs whose knots are the same angles; between knots its parameter is not the angle. One stream with an oblate revolved ellipse misses its intcurve end points by 0.065 mm and 0.104 mm when the pcurve is evaluated on the solved surface, and by less than `1e-8` mm when the same parameters are evaluated on the exact revolution or mapped through the half-angle relation of each rational arc.

**Need.** The support contract and the pcurve evaluation require the exact construction. The decoder can state the exact revolution with a conic directrix, but a direct procedural carrier cannot keep the solved surface and its fit tolerance, and a procedural carrier with a solved cache evaluates on the cache.

**Conflict.** `cad-ir.md` reads `cache_fit_tolerance` as the deviation between the construction and its solved carrier at the same parameters. These solved surfaces agree with the construction in position, not in parameters. The model needs a decision: retain such a cache without evaluation precedence, map pcurves into the cache parameters, or keep the support pcurve out of the contract.

### GC-41. Origin of the `helix_int_cur` apex term

**Question.** Does the apex term of a `helix_int_cur` count revolutions from angle zero or from the interval start `t0`?

**Known.** `asm.md` §6.3 `helix_int_cur` counts the pitch term from angle zero: the edge endpoints of helices with a nonzero `t0` lie on their vertices only with `q=t/(2π)`. The formula of that paragraph and the evaluator apply the same `q` to the apex term. No decoded helix has both a nonzero `t0` and a nonzero apex factor, and at `t0=0` the two origins agree.

**Need.** A conical helix with a nonzero `t0` and a nonzero apex factor, together with its edge vertices, separates the two origins. A wrong origin offsets the radius of every evaluated point by the same length.

### GC-42. Profile angle of a `helix_spl_circ`

**Question.** Which point of the profile circle does a profile parameter `u` select, and what does the angle stored before the path control?

**Known.** `asm.md` §6.3 `helix_spl_circ` gives the swept circle. The stored angle is `0`, `π`, or `-π`. The map `u ↦ C(t)+r·(-cos(u+a)·ρ̂(t)+sin(u+a)·n)`, with `a` the stored angle and `ρ̂(t)` the unit vector of `ρ(t)`, puts the pcurves of helix and spline edges on their edges. It puts some pcurves of circular cap edges a quarter or half turn away from their edges. On two coil faces with `a=0`, a zero apex factor, and a path interval starting at `t=0`, every trim pcurve endpoint, including the `u=±π` ends of the circular cap-edge pcurves, maps onto its edge vertex, and `u=±π` gives the outer seam helix at distance `|major|+r` from the axis.

**Need.** A writer that emits pcurves on this surface needs the map. A rule that also holds for the cap-edge pcurves settles it.

### GC-43. Tangential and apex terms of a `helix_spl_line` profile

**Question.** Does the `y` coefficient of a `helix_spl_line` direction scale `τ(t)`, and does the profile vector include the apex factor's radial scale?

**Known.** `asm.md` §6.3 `helix_spl_circ` gives the linear surface `S(u,t)` with `y` on `τ(t)` and no apex scale. Every decoded direction has `y=0`, and every decoded path has a zero apex factor, so neither term is observed.

**Need.** A thread surface with a tangential profile component or on a conical path, together with its edges, fixes both terms.

### GC-44. Blend supports whose cache misses the intersection curve

**Question.** Which contract does an `int_int_cur` meet when its support is a blend spline surface whose solved cache lies farther from the intersection curve than the cache fit tolerance?

**Known.** `asm.md` §6.3 "**`int_int_cur`** has context-first" states the construction as the intersection of the two ordered supports, each with a BS2 parameter curve. An embedded `spline` support decodes to its solved NURBS cache; its subtype and its cache fit tolerance are not retained. For `rb_blend_spl_sur` and `srf_srf_v_bl_spl_sur` supports the cache fit tolerance is at most `0.01` mm, but the intersection curve endpoints lie up to `0.033` mm from the cache surface, and the support pcurve endpoints mapped through the cache lie up to `0.066` mm from them. The edges on these curves carry no tolerance. `asm.md` §6.3 "**`var_blend_spl_sur` / `srf_srf_v_bl_spl_sur`**" defines an approximation-current integer: `0` marks a cache that does not describe the surface.

**Need.** The approximation-current values and cache fit tolerances of the failing supports, together with the exact blend surface evaluated at the support pcurve endpoints, show whether the cache or the pcurve departs from the construction. That evidence fixes the endpoint contract of such a support.

### GC-45. Shift and scale of a reversed `off_surf_int_cur`

**Question.** How do the shift and scale of an `off_surf_int_cur` relate the curve parameter to the parameter of its base curve, and how does a reversed record sense change that relation?

**Known.** `asm.md` §6.3 "**`off_surf_int_cur`**" stores base-surface U and V intervals, a base curve with its interval, and distance, shift, and scale doubles; it does not state the map that shift and scale define. `asm.md` §6.6 "An `intcurve` or `spline` record" states that a reversed intcurve parameterizes as the negation of its cache and keeps its support pcurves on the subtype parameter. The decoder restates the support context of a reversed record on `-t`, swaps the two base endpoints so that each stays paired with its context endpoint, and keeps the base ranges, shift, and scale as stored.

**Need.** A reversed and a forward `off_surf_int_cur` on the same base curve, with the base-curve parameter of known points on the cache, give the map and whether reversal negates scale or moves shift.

### GC-46. Selector of a reversed `sss_int_cur`

**Question.** Does the integer selector of an `sss_int_cur` choose an intersection branch by the direction of the curve, so that a reversed record selects another branch?

**Known.** `asm.md` §6.3 "**`sss_int_cur`**" stores the surface-related prefix, an integer selector, and a third support surface with its BS2 parameter curve. It does not state what the selector chooses. The decoder restates the support context and the third side of a reversed record on `-t` and keeps the selector as stored.

**Need.** A reversed and a forward `sss_int_cur` through the same three supports, with their selectors, show whether the selector depends on the curve direction.

### GC-47. Native truth of the fixed-size record conventions

**Question.** Which stated convention gives the native record extent when the prose size, the stated offsets, and the layout table disagree: the declared byte total, the stated chunk offsets, or the derived tiling?

**Known.** `asm.md` §5.2 "**CoEdge (100 B):**" declares 100 bytes while the stated offsets end `chunk[10]` at +99; shifting every offset by one or declaring 99 bytes both close the record. `asm.md` §5.2 "**Edge (98 B):**" declares 98 bytes while the stated offsets end the continuity text at +99 with a one-byte gap at +88, unique among chunks. `asm.md` §5.2 "**Lump (61 B):**" states a reserved slot at +27 and the first shell at +43 with seven bytes between them versus a 9-byte chunk stride. `asm.md` §5.2 "**Transform (142 B):** 13×f64 (@+18..117)" reaches the declared total under neither the chunk reading nor the byte-range reading plus the three trailing enum bytes. `asm.md` §6.2 "**`cone` (161 B, covers cylinders)**" gives token order without offsets, and `asm.md` §6.2 "**`sphere` (134 B)**" omits the token tags for `dir1` and `dir2`, so neither total tiles from the listed slots.

**Need.** A specimen or an authoritative rule that selects the native extent for each record. The prose sizes stay unchanged; this item owns the decision.

**Note.** `docs/layouts/asm.toml` records each mismatch as a discrepancy or gap and leaves the side open. The prose sizing stays as stated until this item settles.

### GC-48. Closed vocabulary of the B-rep stats census keys

**Question.** What is the closed set of native-kind strings that the B-rep loss census reports for each stats map?

**Known.** `asm.md` §5.5 "Topology membership is defined by references" gives the ownership graph the census summarizes. The decoder retains per-kind census maps keyed by free-form native strings: omitted faces by null or dangling surface-reference condition, undecoded face surfaces by owned construction kind or record head, edge curves and coedge pcurves by full native record name, and residual records by native kind. These keys run on free-form native strings outside any pinned vocabulary, so a new native kind and a renamed key collide in the loss report.

**Need.** A pinned key vocabulary, or specimens exercising each census branch, that separates a new native kind from a key change. Until then the census keys remain unpinned strings.

**Note.** QA sweep location: `crates/cadmpeg-asm/src/brep/stats.rs:15-35`.

## 2. Topology

### TG-01. Missing partner coedge substitution

**Question.** How does a reader represent a coedge whose stored partner is not kept in the reachable topology?

**Known.** `asm.md` §5.2 gives the coedge partner link and §5.4 gives the radial pairing invariant. `emit_coedges` filters the stored partner through the kept-coedge set. When the partner is absent, it writes `radial_next` back to the coedge itself. A partner can be absent because an adjacent face or carrier was dropped for a missing or dangling surface reference in `keep_faces_and_carriers`. No loss records the replacement.

If one coedge of a valid pair is dropped because its face carrier is unavailable, the surviving coedge is emitted as a self-ring. Validation accepts the self-ring as a laminar boundary, so a broken decode is represented as a different topology rather than as an unresolved partner.

**Need.** A source rule or a retained native reference must distinguish a genuinely laminar self-ring from a partner lost during reachability filtering. The decoder must withhold the coedge or report the missing partner until that distinction is known.

**Note.** QA sweep locations: `crates/cadmpeg-asm/src/brep/topology.rs:90-103` and `crates/cadmpeg-asm/src/brep/emit.rs:3690-3751`. Self-radial laminar boundaries are valid counter-evidence, but the current code uses the same value for a dropped partner.

### TG-02. Body-kind inference from edge-use counts

**Question.** Which native member gives an ASM body's `Solid`, `Sheet`, or `General` kind?

**Known.** `asm.md` §5.2 documents the body flags/history field and topology links but does not define a body-kind member. `classify_body_kinds` in `brep/geometry.rs` sets `Wire` or `General` from visible topology, then labels a face-bearing body `Solid` through `faced_body_kind`: every counted edge has as many forward as reversed coedge uses, and at least one edge is counted. Edges that collapse to a point (null or `degenerate_curve` carrier and one vertex, `asm.md` §6.3) are not counted, because a point loop at a cone apex or sphere pole bounds no area. A non-manifold edge with a balanced partner cycle (`asm.md` §5.2) closes its faces like a two-use edge. Any other face-bearing body is `Sheet`. The rule runs only after model transfer and records no inference or loss. The source-less F3D writer applies the same rule to refuse a body whose declared kind contradicts its topology.

Specimens with all faces single-sided and one lump and shell include closed bodies with apex point loops and with a four-use edge where two pieces of material touch along a line. An exact two-use rule labels both as `Sheet`. If a closed sheet or a body with a non-manifold internal face has balanced edge uses, the rule emits `Solid`. If a solid has a missing coedge, it emits `Sheet`. Face sidedness does not enter the rule, and no specimen has a closed body with double-sided faces. The same topological count can therefore still select different native body meanings without reading a source discriminator.

**Need.** A body-kind field, an authoritative topology invariant, or a specimen with a body whose stored metadata separates solid and sheet states must settle the rule. Until then the neutral body kind is an inference from the output graph.

**Note.** QA sweep locations: `classify_body_kinds`, `faced_body_kind`, and `collapsed_edges` in `crates/cadmpeg-asm/src/brep/geometry.rs`, called from `decode_with_purpose` in `crates/cadmpeg-asm/src/brep/mod.rs`. The balance rule is valid for two-manifold solids, for touching-material edges, and for apex point loops, but that does not establish it for all bodies.

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

### AT-02. Precedence of multiple display-name attributes

**Question.** Which display name applies when one entity attribute chain contains more than one
nonempty `string_attrib-name_attrib-gen-attrib` record whose attribute name is `name`?

**Known.** `asm.md` §5.6 "`string_attrib-name_attrib-gen-attrib` stores" defines each `name` record as a body
or face display-name carrier. Attribute records have explicit forward and backward chain links. The
format model does not state whether chain order, migration flags, attribute ownership, or another
field selects among multiple nonempty name carriers.

**Need.** Define display-name precedence, including the roles of empty values, nested attribute
ownership, and the four keep/copy/ignore/copy flags. State whether multiple applicable name carriers
are valid and, if they are, which one supplies the neutral display name.

**Conflict.** The current neutral projection returns the first nonempty `name` value encountered in
the forward attribute chain. A different chain order can therefore change the neutral display name
without changing the set of name attributes.

**Note.** Preserve every native name attribute independently of the single neutral display-name
projection.

## 4. Text encoding

### TE-01. Migration-flag words of a `gen-attrib` record in the text encoding

**Question.** Which `ENUM_VALUE` integer does each migration-flag word of a text-encoded `gen-attrib` record select?

**Known.** `asm.md` §5.6 gives the binary attribute records. A binary `int64_attrib-name_attrib-gen-attrib` record stores four `ENUM_VALUE` tokens between its reference fields and its name string. The text encoding writes four words at those slots, from the set `keep`, `keep_one`, `keep_kept`, `ignore`, and `copy`. The word-to-integer map is not known. The text reader keeps these records with each word as an identifier token and does not select an integer, so a wrong integer cannot reach the attribute values.

**Need.** We must know the map to give a text-encoded `gen-attrib` record the same token stream as its binary form, and to write the words from a binary record.

### TE-02. History-partition marking in the text encoding

**Question.** How does a text stream mark a construction-history partition?

**Known.** `asm.md` §7.1 gives the header lines. The flags word keeps its binary semantics, so bit 0 is the history-partition flag. No further text-specific marking is known, and the record grammar for a text history partition is not known.

**Need.** A reader must know the marking to separate the solved records from history records; without it, a history-bearing text stream would read history records as model records.

### TE-03. Direction word after the degrees of a legacy surface block

**Question.** What does the word `u`, `v`, or `both` after the two degree fields of a save-format 102–600 surface block select?

**Known.** `asm.md` §7.5 "The `exactcur`" gives its position: after the two degree fields and before the closure words. The word is optional. The decoder keeps it as a text token and does not change the control grid for it.

**Need.** If the word selects a rational or periodic direction, the decoder must apply it to the grid to give the exact surface.

## 5. Law formulas

### LF-01. Value of `SIGN` at zero
**Question.** What value does a `SIGN` law have when its operand is zero?

**Known.** `asm.md` §6.3 "**Law formulas**" names `SIGN` as a unary operator. Its serialized form has one recursively framed operand and no field that selects a zero convention. The format model does not give its value at zero.

**Need.** A source rule or a law-driven construction with a defined value at an operand zero must settle whether `SIGN(0)` is zero, positive one, negative one, or has no value. This determines the section of a cacheless law-driven sweep at that parameter.

## 6. Construction history

### HS-01. Role of the first `history_stream` preamble field

**Question.** What does the first preamble field, `stream_size`, identify?

**Known.** `asm.md` §3.1 "The history-container record" gives the preamble. The linked `delta_state` chain runs from the current model toward the initial model, and its head is the current model. Usually `stream_size` equals the `state_id` of the head. A head can also have `state_id` `0` and be followed by the state whose `state_id` is `stream_size`. And `stream_size` can be a value that no `state_id` in the chain has.

**Need.** A writer that adds a state must write a correct `stream_size`.

## 7. External kernel layers

### XL-01. Semantics of SAT source-stream recovery

**Question.** What native record layout does each recovered SAT source extent carry, and which stream owns each rebased reference?

**Known.** `asm.md` §2.3 "Other pure ACIS and SpaceClaim SAB envelopes use" states that the byte layouts in §§5–6 cover the named ACIS bands. A SAT text-grammar section is still missing. The SAT decoder recovers independently headed concatenated streams with rebased entity tables, retains record extensions and legacy spline contexts natively, and withholds a later stream with an unreadable or different native layout. These behaviours carry the loss codes `SourceConcatenatedStreamsRecovered`, `SourceRecordExtensionsUnprojected`, and `SourceStreamLayoutUnprojected`. The SAT goldens are code-built streams with no source stream file, so they test that the decoder follows these rules on synthetic streams.

**Need.** Native SAT specimens with concatenated streams, record extensions, legacy contexts, and divergent later-stream layouts must settle each recovered extent's grammar and reference ownership. Until then the recovered extents stay unprojected.

**Note.** QA sweep locations: `crates/cadmpeg-codec-sat/src/loss.rs:30-69` and `crates/cadmpeg-codec-sat/src/decode.rs:317-361`. This item owns the SAT gap.

### XL-02. Native owners of unprojected SAT topology

**Question.** Which native records own a SAT shell with no admissible member or owner, a standalone face, and a body-owned wire?

**Known.** The SAT decoder omits a shell with no admissible member or owner, a standalone face with no shell owner, and a body-owned wire, and retains each source record. These behaviours carry the loss codes `TopologyShellUnprojected`, `TopologyFaceOwnerUnprojected`, and `TopologyWireOwnerUnprojected`.

**Need.** Native specimens or an authoritative ownership rule must give each record its neutral owner or confirm the ownerless case. Until then these records stay unprojected.

**Note.** QA sweep location: `crates/cadmpeg-codec-sat/src/decode.rs:460-490`. This item owns the SAT gap.

### XL-03. Projection of the Parasolid layer

**Question.** What neutral geometry does each named Parasolid schema row project, and what does the residual row retain?

**Known.** `siemens_nx.md` §3 "The partition zlib stream is preceded by" gives the embedded stream framing. The two-file `cadmpeg-parasolid` helper crate owns the `parasolid:` dialect rows and the schema-token grammar; it is not a codec facade entry and has no decoder. Three schema rows are named, with a residual row for every other schema, and the SLDPRT and NX hosts classify their embedded streams through these rows.

**Need.** A tracking decision: either a Parasolid projection specification with specimens for each named row, or an explicit statement keeping the layer identity-only. Until then embedded Parasolid streams stay unprojected.

**Note.** The schema-row map and the residual rule live in `crates/cadmpeg-parasolid/src/lib.rs:179-192`. This item tracks the layer without adding a facade or codec.

