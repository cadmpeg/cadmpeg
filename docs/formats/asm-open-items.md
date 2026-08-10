# Autodesk ShapeManager (ASM) stream: Open Items

This document lists the parts of the ASM stream format that we do not know. The specification `asm.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

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

**Need.** We must know which value to write when we make this record from a neutral model. `false` is the value for a surface built directly from a support surface and a distance, so the item blocks only the reflected case. **Blocked on a specimen:** a document whose `off_spl_sur` record stores a reflected support and a tail without a cache lets us read the states off the solved surface, and no such document is available to read.

### GC-07. `off_spl_sur` extension run

**Question.** What are the field roles of the extension run in an `off_spl_sur` record?

**Known.** `asm.md` §6.3 `off_spl_sur` gives the run's field sequence, the prefix states that require it, the zero first integer, and the unit-independent tolerance. A run written to that sequence is accepted on read and is retained field for field, so the sequence is the true layout. The first fields agree with the start of a surface-to-surface intersection payload. That start is a presence logical, a six-integer header, an intersection curve, two pcurve logicals, and a tolerance. The four `-1` integers at the end are more than the three endpoint-term slots of that layout. The five integers after the leading zero are retained verbatim and have no established meaning. A run whose four `-1` integers hold `0 1 2 3` is refused on read, and so is a run whose two post-curve logicals are both true; a refusal does not give the accepted sets.

**Need.** We can now make a record that has a run, but only by copying a run we read. To make one from a neutral model we must know what the five integers, the two logicals, and the four `-1` integers hold, and which values each accepts.

### GC-08. Shared revision-gated surface tail enum values other than `0` and `2`

**Question.** What layouts do the shared revision-gated surface tail enum values other than `0` and `2` select?

**Known.** `asm.md` §6.3 "**Revision-gated spline-surface forms**" gives the layouts of `0` and `2`. Zero selects the solved cache and its fit tolerance; two replaces both with the two bool-gated parameter intervals and four closure and singularity enums. The decoder keeps a record with any other value as opaque bytes. Values `3` and `4` paired with the complete value-zero cache payload are refused on read. Tails written with the spelling `historical`, `optimal`, `none`, or `summary` and with the payload the same-named `law_spl_sur` mode selects are also refused. Each refusal rejects only the complete submitted value-and-layout pair; it does not reject that numeral with another layout or narrow the accepted spelling set.

**Need.** We cannot read a record with a value other than `0` or `2`. **Blocked on a specimen:** a document whose shared surface tail carries another value gives that value's layout, and a document whose `off_spl_sur` tail carries form `2` shows the cacheless form on that carrier; no such document is available to read.

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

**Need.** We keep the two slots without a change. To write them from a neutral model, we must know what they hold. To write a type-zero member into a save-format-23200 stream, or into a stream with a save format version between 22600 and 23200, we must know if that stream keeps the ASM integer. **Blocked on a specimen:** a document whose type-zero member carries a non-null spline slot separates the slot types by the count of scalars per control point, and a save-format-23200 stream holding a revision-gated loft settles the gate; no such document is available to read.

### GC-23. Cache-first intcurve leading enum values other than `0` and `2`

**Question.** What layouts do the cache-first intcurve leading enum values other than `0` and `2` select?

**Known.** `asm.md` §6.3 "**Cache-first subtype selection**" gives the layouts of `0` and `2`. The decoder reads both and retains a record with any other value verbatim. Value `2` paired with the complete value-zero cache payload is refused on read; the valid value-two layout replaces that cache rather than retaining it. A `par_int_cur` whose leading enum carries the spelling `summary`, `historical`, or `optimal` over an otherwise untouched cache-first payload is also refused. Each refusal rejects only the complete submitted value-and-layout pair and does not narrow other layouts or spellings.

**Need.** We cannot read a record with a value other than `0` or `2`. **Blocked on a specimen:** a document whose cache-first intcurve leading enum carries a value other than `0` or `2` gives that value's layout, and no such document is available to read. GC-27 gives the separate limit that value `2` reaches.

### GC-24. Binding of the law formula text infix operator `O`

**Question.** What are the precedence and associativity of the infix operator `O` in stored law formula text?

**Known.** `asm.md` §6.3 "**Law formulas**" gives `O` as composition with the right operand innermost, and gives the `MTRAIL` curve as a rail direction requiring no further construction input. A writer parenthesizes both `O` operands, so stored text never exercises the operator's binding against a neighbouring operator and never chains two occurrences. A `law_int_cur` written from the field order in `asm.md` §6.3 over a solved curve cache is refused on read, and the refusal covers the smallest form, whose law is `null_law` and which carries no operator at all. The field order and the arity encoding are both speculative, so the refusal bears on neither the operator token spellings nor the binding, and it does not show that the record is unreachable by a different field order.

**Need.** We must know the binding to parse law text that a different writer produced without full parenthesization. Text this codec emits is unaffected, because it parenthesizes both operands.

### GC-25. Payload after a true shared revision-gated surface tail logical

**Question.** What follows the closing logical of the shared revision-gated surface tail when that logical is true?

**Known.** `asm.md` §6.3 "**Revision-gated spline-surface forms**" ends the tail with six counted float arrays and one logical, for each value of the form enum. The specification gives no payload after that logical, and the decoder ends the tail there for either value.

**Need.** A false logical is the only state the decoder can account for. A carrier whose tail is its last field, such as the revision-gated `cyl_spl_sur`, would end its scope at a true logical and drop the bytes after it without a diagnostic, and would then write the record back short. A carrier with its own fields after the tail, such as `rb_blend_spl_sur` and `var_blend_spl_sur`, reads those fields at the wrong offset instead and keeps the whole record as opaque bytes. No subtype scope has a full-consumption check that would separate the two outcomes from a correct decode. **Blocked on a specimen:** a document whose shared revision-gated surface tail closes with a true logical shows what follows it, and no such document is available to read.

### GC-26. Position of the `sss_blend_spl_sur` third-side graph

**Question.** Does the third-side graph of an `sss_blend_spl_sur` record come between the shared revision-gated surface tail and the three trailing integers, or after those integers?

**Known.** `asm.md` §6.6 `rb_blend_spl_sur` puts the third-side graph after the tail and before the three `tail_extension` integers. The two-support subtypes end with the tail and those integers, which fixes the integers as the last fields of that scope but does not fix the third-side graph against them. Replacing only the accepted subtype name `rb_blend_spl_sur` with `sss_blend_spl_sur` is refused in a fresh reader session while the unchanged control is accepted. The rename-only payload is therefore not the `sss_blend_spl_sur` grammar, but the refusal does not select either candidate graph position.

**Need.** The decoder and the source-less writer both use the position the specification gives. The wrong position makes every `sss_blend_spl_sur` record fail its decode and stay opaque, and makes a generated record ungrammatical. **Blocked on a specimen:** a document holding an `sss_blend_spl_sur` record fixes the graph position against the trailing integers, and no such document is available to read.

### GC-27. Solved carrier of a cache-first intcurve that stores no cache

**Question.** Which curve gives the parameter domain of a cache-first intcurve record whose leading enum is `2` and whose construction stores no curve block?

**Known.** `asm.md` §6.3 "**Cache-first subtype selection**" gives the layout of leading enum `2`: the record stores a bool-gated curve interval and a closed-form enum in place of the solved-curve cache and the fit tolerance that enum `0` stores. The shared cache-first context takes its parameter domain from the record's solved curve, and the record-level search takes the first curve block in the record as that curve. A form-`2` record therefore has a solved carrier only when a nested construction stores a curve block.

**Need.** A form-`2` record that stores no curve block anywhere gives the context no parameter domain, so the decoder retains the record verbatim and the neutral model loses the curve. To read such a record the shared context and every carrier that builds it must accept a record with no solved curve. Whether the record then takes its domain from the interval the form stores, from the support surfaces, or from a curve outside the record is not established. **Blocked on a specimen:** a document holding a form-`2` record whose construction stores no curve block settles which of the three the domain comes from, and no such document is available to read.

### GC-28. Parameter chart of a procedural spline support cache

**Question.** How does a pcurve in a procedural spline support's construction chart map to the parameter chart of that support's solved NURBS cache?

**Known.** A cache-first intcurve support can store `spline`, a subtype-table reference to a procedural spline-surface construction, and four optional bounds. The referenced construction supplies a solved NURBS cache. The intcurve pcurve uses the procedural construction's parameter chart. That chart is not necessarily the solved cache's chart. A `cl_loft_spl_sur` support can map one construction-chart isoline to a nonlinear curve in the solved cache chart. The intcurve and the cache therefore do not establish a direct pcurve-on-surface relation without a chart map.

**Need.** The decoder currently attaches the construction-chart pcurve directly to the solved NURBS support. This relation is invalid when the charts differ. We must retain or derive the exact chart map before the neutral support relation can be complete. A fitted map is not sufficient because it does not preserve the stored construction semantics.

### GC-29. Ownership of multiple 3D curve cache blocks

**Question.** Which 3D curve block is the writable cache when a carrier record contains multiple decodable blocks or supports more than one integer width?

**Known.** `first_curve_patch_layout` scans the admitted integer widths and marker positions and accepts the first block that decodes. `final_curve_patch_layout` uses the final decodable block for a different caller. Neither function verifies a record-specific owner, cache role, or relationship between the selected block and the carrier subtype.

**Need.** A record with more than one decodable 3D curve can make the writer patch a support or pcurve instead of the writable cache. We need an owner reference, subtype rule, or full-consumption invariant before the first-block selection can be used for writing.

## 2. Text encoding

### TE-01. Migration-flag words of a `gen-attrib` record in the text encoding

**Question.** Which `ENUM_VALUE` integer does each migration-flag word of a text-encoded `gen-attrib` record select?

**Known.** `asm.md` §5.6 gives the binary attribute records. A binary `int64_attrib-name_attrib-gen-attrib` record stores four `ENUM_VALUE` tokens between its reference fields and its name string. The text encoding writes four words at those slots, from the set `keep`, `keep_one`, `keep_kept`, `ignore`, and `copy`. The word-to-integer map is not known. The text reader keeps these records with each word as an identifier token and does not select an integer, so a wrong integer cannot reach the attribute values.

**Need.** We must know the map to give a text-encoded `gen-attrib` record the same token stream as its binary form, and to write the words from a binary record.

### TE-02. History-partition marking in the text encoding

**Question.** How does a text stream mark a construction-history partition?

**Known.** `asm.md` §7.1 gives the header lines. The flags word keeps its binary semantics, so bit 0 is the history-partition flag. No further text-specific marking is known, and the record grammar for a text history partition is not known.

**Need.** A reader must know the marking to separate the solved records from history records; without it, a history-bearing text stream would read history records as model records.
