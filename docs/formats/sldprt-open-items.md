# SolidWorks `.sldprt`: Open Items

This document lists the parts of the SolidWorks `.sldprt` format that we do not know. The specification `sldprt.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Body classification

### BC-04. Duplicate topology record identities

**Question.** Which occurrence is authoritative when one stream contains multiple valid topology records with the same attribute but different references or coordinates?

**Known.** Topology records are keyed by stream-local attribute. A complete short-index FACE includes five pointers after its sense. Complete EDGE and VERTEX bodies include their trailing owner pointers. POINT has exactly four pointers before its coordinate vector. Equal projected fields do not establish equality of complete record bytes. The specification does not define a duplicate-record rule or a native precedence field.

**Need.** We must select the authoritative topology record to avoid silently changing body connectivity or vertex positions when a duplicate identity is present.

### BC-08. Complete identity in single-face component selections

**Question.** Which single-face component-selection field, if any, supplies the complete ordered tail of a persistent face identity?

**Known.** Complete `FACE_ID_2001` and DisplayFace identities include tail length and ordered values. A source/local pair can name several distinct complete identities. A single-face selector carrying only that pair requires one distinct target. Generated-hole surface references expand the pair to a set and do not select one persistent face. A component selection has an ordered path of type signatures and optional local identifiers; path length and earlier local identifiers do not establish a persistent-identity tail.

**Need.** An explicit native field relation must distinguish a complete selector from a prefix-only selector before a colliding source/local pair can select one face.

### BC-09. Dummy-FIN partner corroboration

**Question.** Which native field corroborates the ring-order endpoints of an unwalked boundary-dummy canonical FIN before the dummy joins a face loop?

**Known.** `decode.rs` withholds an edge through `TopologyFinPartnerUnresolved` when an unwalked boundary-dummy canonical FIN lacks reciprocal partner corroboration of both ring-order endpoints, or when a mutual twin FIN contradicts the ring-order endpoint; every dummy in a public face loop carries this corroboration (#517).

**Need.** We must know the corroborating field that admits a dummy FIN to a face loop.

### BC-10. FIN-link withholding

**Question.** Which native field distinguishes a FIN forward link that names no reciprocal same-loop backward link from a valid ring closure?

**Known.** `decode.rs` withholds a ring through `TopologyFinLinkUnresolved` when a FIN forward link names no reciprocal same-loop backward link; the ring stays unwalked (#517).

**Need.** We must know the link grammar that walks such a ring.

## 2. Geometry carriers

### GC-02. Missing intersection witnesses

**Question.** Where are the authoritative chart and terminator replacements when an intersection composite references an absent witness record?

**Known.** `sldprt.md` §7.2 "Curve carriers: an edge's `00 10.refs[3]` can point to a `00 86` B-spline/list curve carrier," through `sldprt.md` §7.3 "The chart is a solved cache" define both intersection carriers and the three witness record families. The referenced terminators select the unique chart stride with the least endpoint displacement and replace the approximate chart endpoints. A complete width-4 support-UV record supplies the two solved pcurve caches. An absent or structurally inconsistent optional support-UV record does not invalidate the curve. Tolerance-bounded inversion constructs its pcurves on analytic and positive-weight NURBS supports. Consecutive inverses select one continuous parameter branch, the terminators supply the exact endpoint parameters, and analytic derivative bounds or rational Bézier residual control hulls certify the complete mapped segments against the chart tolerance. A `00 85` wrapper that names a composite without a chart bounds the intersection of the composite's supports between the wrapper's stored points (`sldprt.md` §7.3).

**Need.** We must locate an authoritative replacement when the referenced chart or terminator record is absent.

### GC-04. Offset-surface admission

**Question.** Which sense relation, distance band and bounded support domain admit an unchecked offset with negative support sense, a distance within model resolution or a partially typed support graph?

**Known.** `sldprt.md` §7.5 defines the validity-check state, unused logical, support, distance and nullable scale. A complete node graph with unique positively sensed supports carries the unchecked offset equation through typed analytic, NURBS and swept supports. The node reader resolves canonical pointer indexes. Unchecked transfer requires a distance beyond model resolution. Negative support sense, opaque support dependencies and bounded faces with intersection-chart trims retain unresolved validity.

**Need.** Resolve negative support orientation against the neutral carrier normal, the validity of offsets within model resolution, and the selected parameter domain when the support graph contains an opaque dependency.

### GC-05. Variable-radius law and result parameterization

**Question.** How does a complete variable-radius law, including intermediate controls, map to the parameterization and individual faces of every result patch?

**Known.** The design law and evaluated result have separate carriers. An owned generated-surface identity joins a typed FACE through its complete `FACE_ID_2001` identity; the FACE's surface reference selects the stored NURBS carrier. Resolve the attribute definition's identifier, not an adjacent KEY name. A checked join retains the ordered endpoint radii of its law, such as 5–50 mm or 50–4 mm, and the rational NURBS sections at the result endpoints reproduce those radii. The existing reader retains each law and the generated identities.

**Need.** Prove intermediate-control parameterization, complete selector-tail and collision behavior, and the explicit per-feature result-face association across arbitrary patches. Missing or nonunique complete identities must remain unresolved; geometric resemblance and history order cannot choose a result.

### GC-08. Duplicate geometry carrier identities

**Question.** Which occurrence is authoritative when one stream contains multiple valid geometry carriers with the same stream-local attribute?

**Known.** Geometry carriers are indexed by stream-local attribute. Analytic decode refuses a uniquely parsed carrier whose header framing disagrees with the Partition/Deltas role selected from the stream header. The role filter cannot recover a record rejected by the role-unknown parser, and the writer retains that conservative role-unknown path. Stream role does not prove chronology or duplicate authority.

**Need.** We must select the authoritative carrier without silently changing geometry when a duplicate record is present.

### GC-09. Conflicting surface carrier families

**Question.** Which complete physical node occurrence is authoritative when several source occurrences claim one face-support index?

**Known.** A face-support pointer selects one typed XT node, as defined in `sldprt.md` §7. Decode withholds typed surfaces when several existing surface, offset, blend, or sweep map categories claim the same index, including after stream merging. Analytic and B-surface records share one map, and swept and spun records share another; conflicts inside those maps remain outside this veto.

**Need.** A complete site-local node table must preserve physical candidates, common fields and full canonical indexes before duplicate resolution. Same-category duplicates, opaque targets and source-state precedence need independent ownership rules.

### GC-10. Normal sense of non-compact surface carriers

**Question.** Which field orients the surface normal against the natural normal of the neutral surface for a `00 43` or `00 44` swept or spun surface, a rolling-ball blend, and an offset surface?

**Known.** `sldprt.md` §7.1 gives the orientation marker of a compact analytic surface record (`00 32` through `00 36`) and composes it with the bridge marker of §4. §7.2 gives the sense byte of a `00 7c` B-spline surface-use wrapper. The `00 43`, `00 44`, blend, and offset records carry a `0x2b` or `0x2d` byte in the same compact-header position. `sldprt.md` does not give its meaning for these families.

**Need.** We must know the surface normal sense to give a face on one of these carriers its outward normal.

### GC-11. Compact curve marker exceptions

**Question.** Which field sets the edge direction when a compact circle with marker `0x2b` carries an edge whose stored direction runs against the curve parameter?

**Known.** `sldprt.md` §7.1 "The marker of a compact line, circle, or ellipse gives the direction" defines the marker rule. A circle edge between a cylinder face and a planar face can have marker `0x2b` while its display tessellation runs along the curve parameter. The specification defines no second field that overrides the marker.

**Conflict.** The decoder applies the marker rule to that edge and reports no loss, so the neutral edge trims the circle through the complementary arc of the tessellated arc.

**Need.** We must find the overriding field, or the defect in the marker rule, so that this edge does not trim the circle through the complementary arc.

### GC-12. Bounded-curve wrapper ranges that miss the edge vertices

**Question.** Which direction does an edge on a bounded-curve wrapper have when the wrapper range endpoints match the edge vertices in neither order?

**Known.** `sldprt.md` §7.1 "The marker of a compact line, circle, or ellipse gives the direction" states that a bounded-curve wrapper range sets the edge direction instead of the marker. The range start and end points match the edge vertices in order or in reverse order and so give the direction. The specification defines no direction for a range whose endpoints match the vertices in neither order. The wrapper carries its own orientation marker byte.

**Need.** We must define whether the wrapper marker, the basis curve marker, or another field sets the direction of such an edge.

### GC-13. `00 28` chart field offsets

**Question.** Which byte offsets hold the seventh f64, the two absent-value sentinels, and the point block of a `00 28` chart?

**Known.** `sldprt.md` §7.3 lists the ordered fields `count u32 BE, attr u16 BE, base_parameter f64 BE, base_scale f64 BE, chart_count u32 BE, chordal_error f64 BE`, one further f64, then the sentinels at body +36 and +44, then `count` point entries at body +52. The unnamed seventh f64 occupies body +34 through +41 and overlaps the first sentinel at body +36. `docs/layouts/sldprt.toml` records the overlap as a `[[record.discrepancy]]`; `brep/intersection.rs` reads the sentinels at `preamble + 36` and `preamble + 44` and the point block at `preamble + 52` with `preamble = body + 6`. §4 uses body-relative offsets elsewhere, so the two conventions collide in one document.

**Need.** We must know the native offsets to emit the record: seventh f64, sentinels, point block.

**Conflict.** The specification's stated offsets and the decoder's read offsets disagree, and no open item tracked the native truth.

### GC-49. Signs of the cone half-angle fields

**Question.** Can a cone store a negative `sin` or `cos` half-angle field, and what surface does it then describe?

**Known.** `sldprt.md` §7.1 "The line direction is unit length." gives each field in the open interval (0, 1), the cone identity within `1e-9`, and the half-angle `atan2(sin, cos)`. `siemens_nx.md` §6.1 gives the same rule. The SLDPRT decoder omits a CONE node with a negative field and reports `geometry.cone-half-angle-sign-unresolved`; the NX decoder reports `carrier.cone-half-angle-sign-unresolved`.

**Need.** We must know whether a signed half-angle is a valid cone or a different surface before a negative field can transfer as exact geometry.

### GC-51. Intersection guide parameterization

**Question.** Which guide constraint binds a chart parameter to the exact intersection of its two support surfaces?

**Known.** §7.3 defines the support pointers, chart interval, terminators and optional support-UV caches. The three-dimensional chart bounds its positional approximation with `chordal_error`. A bounded offset FACE can use these intersection edges.

**Need.** Derive the exact parameterization and branch selection from the chart and supports. The neutral representation needs a guide-curve relation or certified coincident support pcurves.

**Conflict.** A positional cache evaluates the interpolated chart. The exact intersection lies on both supports.

**Note.** An unchecked offset face with these trims keeps opaque geometry and `geometry.offset-validity-unresolved`.

## 3. Container metadata

### CM-01. Cache-cell prefix

**Question.** Which producer rule generates a new cache cell's opaque prefix values when no source bytes are available?

**Known.** The prefix occupies the `N=L/S` bytes between an independently anchored cell start and its marker. Equal section names, displacements, complete fixed headers, and unique target blocks do not determine equal prefix values. Retained cells preserve these bytes without assigning a producer meaning.

**Need.** A producer-side relation must establish a regeneration rule before a source-less writer can synthesize this field.

### CM-02. Cache-cell fill

**Question.** Which producer rule generates a new cache cell's opaque trailing values when no source bytes are available?

**Known.** The trailing field extends from the encoded name's end to `cell_start+S`. Equal section names, displacements, complete fixed headers, and unique target blocks do not determine equal trailing values. Zero and `ff` are opaque byte values, not structural rejection conditions. Retained cells preserve the complete range.

**Need.** A producer-side relation must establish a regeneration rule before a source-less writer can synthesize this field.

### CM-04. Opaque tail residual generation

**Question.** How does the producer generate the opaque prefix and residual bytes in the named tail-padding and anonymous footer cells?

**Known.** `sldprt.md` §1.4 defines the named-grid scales, footer scales, exact cell extents, EOCD count, and both central-directory locator equations. Bytes outside those fields remain opaque and are retained exactly; they do not select directory members.

**Need.** An independent producer oracle that explains or generates those residual bytes for newly constructed cells.

**Conflict.** None.

**Note.** Parsed grid geometry and EOCD fields do not establish a regeneration rule for opaque residuals.

### CM-07. Transformed enclosure opaque prologue

**Question.** How does the producer generate the 24 opaque bytes in a `moTransRefPlaneData_c` serializer prologue?

**Known.** `sldprt.md` §8 defines its fixed 25-byte prologue, presence byte, and optional ten-value enclosure. The stored center, corners, and radius do not define the opaque bytes.

**Need.** An independent producer rule or controlled native edit must determine these bytes for a newly authored record.

**Conflict.** None.

**Note.** Retained prologue bytes can be preserved without assigning a generation rule.

### CM-10. Complete effective-schema stream cursor

**Question.** Which base schemas and first-use field sequences establish every XT node boundary in unframed carriers?

**Known.** `sldprt.md` §3.1.1 defines the terminal node and the direct `ThirdPty/WELDMENT_STORAGE` length prefix. The chained wrapper section length bounds its concatenated frame outputs. Header-shaped signatures and terminal-shaped field bytes do not define an intrinsic boundary. The complete base-schema node cursor is separate from outer-extent and endpoint-wire checks.

A complete candidate header establishes its local description and schema
token. It does not establish an intrinsic endpoint of the preceding stream.
An interior header in coordinate or string data remains part of that stream.

**Need.** Complete selected base-schema field tables and Source-backed traversal of each effective field edit, fixed field, and variable count must establish the node cursor before intrinsic endpoints can be admitted without a carrier extent.

**Conflict.** The generic direct extractor ends an unframed stream at the next
syntactically complete header. It does not check that the preceding stream's
effective-schema node cursor reaches that position. A complete header-shaped
image in field data can therefore be classified as a new stream.

**Note.** Outer framing does not establish physical occurrence policy or permit salvage of an invalid CFB extent.

### CM-14. Configuration identity carriers

**Question.** Which carrier controls membership when Keywords `id` differs from manifest `swID`, or a native explicit `SourceIndex` conflicts with the complete manifest join?

**Known.** `sldprt.md` §2 defines the complete manifest-row, referenced-model, and decimal `swID` join for saved inactive and active configurations. A readable Keywords name must agree with that join. A missing or unreadable Keywords definition does not erase a complete saved identity. Missing partitions leave membership unresolved; decoded empty partitions establish empty membership. An update-required row can have a decoded partition: a row with `NeedsUpdate="YES"` can have a complete active partition-0 identity. The reader retains that saved identity without asserting regeneration freshness. Where a configuration stores both, Keywords `id` equals `swID`, so their covariance does not establish interchangeable authority. Every stored Keywords `id` names a stored `Config-N` stream index, and the partition of a lone Keywords configuration has its `id` as `N`, so `sldprt.md` §2 binds a configuration without `SourceIndex` through its `id`. Conflicting explicit `SourceIndex` values remain unresolved.

**Need.** A producer save or export must distinguish unequal Keywords `id` and manifest `swID`. A native dual-carrier witness must test explicit `SourceIndex` disagreement with the full manifest join.

**Note.** Saved partition membership does not establish current feature definitions or regenerated geometry. A manifest name and body set do not supply an unreadable Keywords definition or its feature states.

### CM-15. Outer version word

**Question.** Which values can the outer header version word take?

**Known.** `sldprt.md` §1.1 records the word (u32 big-endian at offset 4) with observed value `0x00000004`. In `dialect.rs`, `scan` stores `0` on the compound-file branch and stores the read value in the `outer_version` attribute; admission ignores the value.

**Need.** Observed values from native files settle whether the word is fixed.

**Conflict.** The specification records a fixed value; the decoder admits every value.

### CM-16. Signature tokens, member time/date words and `internal_attrs`

**Question.** Which rule gives a file's local, central and end-of-directory signature tokens, from its `file_id` or from other content? Which rule gives a member's packed time/date word? Which rule gives bit 0 of a member's `internal_attrs`?

**Known.** `sldprt.md` §1.1 states one local token per file, one central token per file and a third token in the end-of-directory root. The three tokens are nonzero and pairwise distinct, `file_id` is nonzero, and files with the same `file_id` carry the same three tokens. The time/date word is a raw ZIP modification time/date, the local and central headers of one member carry the same word, and several members of one file can carry the same word. Many words are not valid MS-DOS dates or times, and the word of one member name changes between files. §1.3 gives the ZIP APPNOTE meaning of bit 0 of `internal_attrs`, the producer's text hint. Members of one name carry 0 in some files and 1 in others, and the zlib text test on the payload disagrees with the bit.

**Need.** A writer with no source envelope must write these fields. The rules let it write the values SolidWorks writes. Until then, the SLDPRT writer writes values of its own and charges `export:record.residual-unavailable` for each field.

### CM-17. Keywords lead byte

**Question.** What does the lead byte `85` or `86` before the Keywords XML text record?

**Known.** `sldprt.md` §2 "An XML payload stores its text as UTF-8" defines both lead bytes; the decoder reads the text after either. Both forms declare UTF-8 and give the `Keywords` root the attributes `Name` and `id`. Every document with lead byte `85` holds an `Application` element with `Keyword` children. An `Application` element occurs after either lead byte, so that element does not select the byte. Both bytes occur in documents of the same `swVersion`. cadmpeg regenerates an edited member after the lead byte and declaration of its source member, and writes a member with no source member after lead byte `86` (`sldprt.md` §2).

**Need.** The producer rule that selects the byte, so that a writer selects it for a Keywords member with no source member.

### CM-18. Keywords `Application` element

**Question.** What do the `Application` element of a Keywords root and its `Keyword` children record?

**Known.** The `Application` element is the first child element of the `Keywords` root. Its one attribute is `Name`, with the value `Simulation`. Its children are `Keyword` elements, each with a `Name` attribute and a text value, such as `Analysis type` with `Static` and `Mesh type` with `Solid Mesh`; the names and values are in the language of the document. Neither element has an `id` or a `Type` attribute. The element occurs after either lead byte of CM-17. The decoder keeps each `Application` element with its subtree as an item of the native history content, in source order, and the writer writes it back when it regenerates the Keywords text. Neither element is a feature, and `history.application-not-transferred` names each `Application` element and its children. The decoder keeps 16 element levels of an `Application` element, the element itself being level 1; `history.application-nesting-cut` counts the elements below them and names the first. Stored `Application` elements nest 2 levels.

**Need.** The program state the keywords describe, and whether a target format has a record for it, before the decoder maps them to one.

## 4. Auxiliary lanes

### AL-01. DisplayLists face ranges

**Question.** How does a B-rep face select its triangle range in a DisplayLists block when no face key joins them?

**Known.** `sldprt.md` §7.3 "**`00 28` chart**" defines the DisplayLists descriptor table, strip lengths, and triangle-count relations. `sldprt.md` §4 "Primary ownership chain:" defines the B-rep face topology. `sldprt.md` §8 "The FACE owner of an `SWEntUnchanged` ATTRIBUTE" defines the face-key join: the integer field of an `SWEntUnchanged` ATTRIBUTE on a FACE equals the u32 at `M+0x64` of the table that face owns, when `M+0x60` is `1` and exactly one table and one current FACE carry the key. `sldprt.md` §5 "`SWEntUnchanged` has definition field codes `[1]`." defines the key family. `sldprt.md` §8 also defines the complete persistent identity path carried by matching B-rep attributes and DisplayLists references, as well as unambiguous ownership from incidence with one analytic or NURBS face support and from complete bounded planar, cylindrical, and conical trims. Procedural, polygonal, opaque, and non-exact NURBS supports, other coincident trims, and tables without a key or a complete matching identity do not supply the stored face-range mapping.

**Need.** We must know the stored mapping to attach tables that carry no unique face key, whose support is ambiguous or unsupported, including non-exact NURBS caches, and that carry no complete matching persistent identity.

**Note.** In native dictionaries the `SDL/TYSA_DENSITY` KEY can precede the `SWEntUnchanged` definition. The definition `identifier`, not adjacency, names the family (`sldprt.md` §5 "A definition is named by its `identifier`").

### AL-03. DisplayLists renderer flag meanings

**Question.** Which renderer states do the stored `B` and `C` flags identify?

**Known.** `sldprt.md` §8 defines the four supported extended header words, their Boolean metadata triples, the exact stored-normal classification, and the optional scalar/unit-vector tuple. The independent metadata word does not contribute to the header word.

**Need.** We must know the renderer meanings of `B` and `C` and the rule that selects their values when no retained metadata supplies them.

### AL-05. Retained and direct face appearance paths

**Question.** Which complete persistent-path relation selects retained feature faces and direct face appearances?

**Known.** `sldprt.md` §8 defines native display-state selection, three complete part-default class chains, body identity through `ATOM_ID_2001`, and generated-face feature scope through current `FACE_ID_2001` source and timestamp values. Four-word face values identify a feature-scoped owner set without a local face selector. A `moCompFace_c` object can carry a complete multi-component path. A terminal source, timestamp, and local triple can match more than one current face or omit another affected current face. A bare definition supplies no ownership. Unresolved paths supply no native face colour. Independent DisplayFace colour bindings remain on their stored display meshes.

**Need.** The complete path-to-current-face binding for retained or modified feature faces and direct face assignments, and a complete holderless part-default owner relation. A unique terminal triple in one current identity set does not establish a general path-truncation rule. A DisplayLists display-face table with conflicting reused class tags withholds its persistent join through `TessellationReusedTagAmbiguous` (`decode.rs`, #506, 78e93f783b). The reused class tag is part of this item's open binding question.

### AL-06. Decal mapping fields

**Question.** What do the two u32 values that begin a `moDecalProperties_c` body select, and what unit do the decal locations, width, and height use?

**Known.** `sldprt.md` §8 "A `moDecalProperties_c` instance is an 84-byte body" defines the five f64 fields. The u32 LE at `+0` is `0` or `1`, and the u32 LE at `+4` is `0` or `4`. Rotation takes the values `0`, `90`, `180`, and `270`. Width and height are metre-scale in most bodies; a width or height of exactly `500` occurs only with the pair `(1, 4)`, and a metre-scale size occurs with that pair too.

**Need.** We must know the mapping mode and the size unit before a decal can be placed on its faces.

### AL-07. Decal face lists in other DisplayLists forms

**Question.** Where does a DisplayLists decal record store its face list when the count is not at `+315`?

**Known.** `sldprt.md` §8 "A DisplayLists decal record stores its framed image path" defines one complete form. Other records store the same framed image path, and the bytes at `+6`, `+14`, and `+315` do not satisfy that grammar. A part can also declare `moDecalProperties_c` instances and store no DisplayLists decal record.

**Need.** We must know the other record forms, and the binding of a decal that has no DisplayLists record, to join every decal to its faces.

### AL-08. Visual-property payload fields

**Question.** What do the unnamed fields of a visual-property payload store?

**Known.** `sldprt.md` §8 "A visual-property payload begins at its packed colour" defines the names, the texture flag, and the image paths. The six optical scalars, the 104 bytes and 22 bytes of the texture block, the state cells, and the bytes around the appearance file path have no assigned meaning. The inline instance tag is resolved: `sldprt.md` §8 "The tag's low 15 bits are the class's index in the stream's archive map" states how its value follows from the stream.

**Need.** We must know the meaning of the texture scale and orientation fields, and of the six optical scalars.

## 5. Design intent

### DI-01. Other optional feature-manager node identities

**Question.** What native field identifies the role of each optional classless feature-manager node that does not satisfy a defined layout-scoped identity?

**Known.** `sldprt.md` §2 "An `moCurvePattern_c` feature-input object is immediately preceded by its seed feature object" through `sldprt.md` §2 "An `moLPattern_c` interval without a line-reference record carries each displayed translation" identify the annotations container, principal planes, model origin, lights-and-cameras container, ambient and directional lights, sheet-metal node, and exploded-views container. The identities depend on a complete native-class roster. Other source identifiers are allocation positions and are not role codes. `sldprt.md` §2 "A classless Keywords `Feature` whose `Type` token is `EquationDriven` is the equation" identifies the equation container by its operation-family token, which is a role code and needs neither a native class nor a reserved source identifier. `sldprt.md` §8 defines the explicit DisplayLists scene-object source binding. Anonymous scene-object counts do not identify Keywords records.

**Need.** We must distinguish the remaining binders, comments, body folders, materials, notes, sensors, favorites, history, selection sets, and markups.

### DI-02. Relation-manager settings boundary

**Question.** What complete relation-list and intervening-text grammar identifies the post-relation settings boundary for each supported relation-manager layout?

**Known.** `sldprt.md` §2 "The equation angular-unit mode is the byte" defines the settings-relative field and native Degrees/Radians encoding. The relation list contains a declared count of variable-length objects. Nested expressions and drafting-standard strings change the displacement of the settings prefix from the manager declaration. Display-angle units do not establish the equation mode.

**Need.** A counted parser must consume each complete relation and its nested expression objects, then frame any intervening text before it returns the settings prefix. It must distinguish unsupported manager layouts and missing configuration copies. Numeric signatures inside unparsed relation bytes do not establish that boundary.

**Conflict.** None.

**Note.** A field-relative layout does not define the preceding variable-length grammar. A mode value does not prove that a relation evaluator applied it to a bare angular operand.

### DI-03. Calculated default-property caches

**Question.** Which native state establishes that a saved calculated default-property cache is current for the requested property operand?

**Known.** `sldprt.md` §8.1 "The document default-property namespace is owned by" defines the complete carrier, FMTID, PID and alias identity. It distinguishes saved values from formula companions and live calculation. The admitted 14-value, six-place `SW-MassProp-Config-N` form binds a saved metric vector to `Config-N`; its first twelve fields follow the mass-properties API order, and its volume, area and mass use cubic metres, square metres and kilograms. The part and linked material density fields use kilograms per cubic metre. The vector's six-place values have half-unit source bounds. Configuration-qualified property namespace lookup is DI-04.

**Need.** A source-level freshness rule and an independently validated cache-owner relation that tie a saved mass-properties vector to current geometry. The configuration update state does not certify calculation-cache freshness.

**Conflict.** The serialized configuration state can say that it needs no update while its saved area differs from decoded geometry. The same vector's volume remains ambiguous when the B-rep has two valid orientations.

**Note.** A saved vector's volume and area can agree with measured geometry within their six-place bounds. A saved area can also differ from the area measured on both the native B-rep and the STEP export, and in both cases the configuration stores `swConfigurationNeedsUpdate="NO"`. The volume that such a vector's mass and density imply has a ±0.5 mm³ decimal bound; the saved area mismatch leaves this cache-derived value unverified against current geometry. The two trailing vector fields remain uninterpreted.

### DI-04. Configured-property resolver and legacy ownership

**Question.** What complete model/configuration/property resolver and legacy carrier grammar admit a configured property operand without borrowing another namespace or stale calculated value?

**Known.** `sldprt.md` §8.1 "Modern configuration custom properties use" defines the exact modern configuration-name/stored-ID/carrier join and its UserDefinedProperties section. `sldprt.md` §8.1 "A compound file without `id` values" defines the owner of the legacy `Config-0-Properties` stream when the configuration has no `id`. Stored IDs differ from element positions. Default, document custom and configuration custom properties are separate namespaces.

**Need.** Independent legacy configuration-property dictionary/FMTID ownership inside the `Config-N-Properties` stream, complete operand binding, and strict retained-writer behavior for configuration-local property edits. A saved formula does not establish currentness or override carrier ownership.

**Conflict.** Read-only configuration-property carrier facts do not supply the complete model/configuration-name/property binding. The saved default-property reader has a separate document owner and does not resolve configured custom properties.

**Note.** Read-only `saved_configuration_property` source attributes carry document-level carrier facts in the order `(member, namespace index, FMTID, PID, property name, VT type, optional literal)`. Literal `IsEquation="False"` string records and explicit empty/null records retain their distinct types; a blank string has a literal value, while empty/null has none. These facts do not bind a named configuration or evaluate a property reference. Other values and `FPVals` caches remain retained in the complete source carrier. Semantic writes require the recorded facts and carrier bytes to remain unchanged. Modern name/ID evidence does not establish property-cache freshness or complete evaluator execution. The ordered two-set legacy `Config-N-Properties` form supplies physical carrier facts from its UserDefinedProperties dictionary and saved strings. The index `N` alone does not bind a named configuration. A named base PID with a same-section `0x01000000 | PID` companion is withheld from this supported subset; absence of a companion does not certify a nonformula or current value. Repeated PID/name records, multiple aliases for one PID, and unsupported scalar forms do not select a value in this subset. The complete carrier remains retained, and semantic compound-property writes require a retained compound writer.

### DI-05. Compact offset framing and complete operand ownership

**Question.** What complete compact/current marker framing and source/result/driver/orientation graph admits an offset-edge relation, and what neutral geometric invariant does it impose when one operand is an external-reference record?

**Known.** `sldprt.md` §2 "An offset-edge marker relation retains native handles" distinguishes raw marker operands from a complete offset constraint. Line supports can be parallel at one positive separation while their bounded lengths differ. Repeated handles, state and sentinel values do not supply a positive offset magnitude or a complete owner join.

**Known (external operand).** `sldprt.md` §2 "The relation table of a sketch block follows the last record of the block" gives the invariant for two local operands: two parallel lines or two concentric arcs. An external-reference record stores backed-up points in the frame of its source sketch, so a comparison in the frame of the owning sketch does not apply.

**Need.** An independently established compact discriminator and record boundary, shape-specific local-ID handling, all source/result curve pairs, one shared positive driving magnitude, source direction and chain membership. General-curve offsets and nonlocal source projection require independent ownership and regularization evidence.

**Need (external operand).** We must know the invariant to project the native relation as a neutral sketch constraint.

**Conflict.** The current link parser uses the current marker layout and misses the compact pair. Its local-ID caller assumes the current record extent when links parse successfully; adding a shorter link form without changing that caller can read the following record. A whole copied compact header does not establish the role or allowed domain of each header word.

**Note.** Retain raw markers and do not infer a compact local ID, a generic fixed header word, or a neutral offset from geometric coincidence.

### DI-06. Arc-cardinal relation invariants

**Question.** What neutral geometric invariant does each top, bottom, left, and right arc-cardinal marker relation impose?

**Known.** `sldprt.md` §2 "A `Config-N-ResolvedFeatures` lane supplies the evaluated parameter state for configuration slot" through `sldprt.md` §2 "A primary line-or-circle geometry handle on a transformed line segment identifies that line" define operand resolution and the neutral invariants of the supported relation families.

**Need.** We must know the invariants to project these native relations as neutral sketch constraints.

### DI-07. Ambiguous relation-locus ownership

**Question.** Which profile locus does a relation operand select when its marker graph identifies a handle but does not identify one geometric locus?

**Known.** `sldprt.md` §2 "A Keywords configuration's decimal `id` attribute is the slot identity for" through `sldprt.md` §2 "Feature-input geometry-handle coordinates and the nested Parasolid profile differ by a signed" define handle chains, coordinate transforms, canonical shared-coordinate loci, and relation operand selection. Ambiguous chains and transform-dependent markers remain unresolved.

**Need.** We must select one locus to construct the neutral constraint.

### DI-08. Relation codes `29..32`, `36..41`, and `43..85`

**Question.** What neutral invariant and operand roles does each relation code in `29..32`, `36..41`, and `43..85` have?

**Known.** The native numeric taxonomy defines identities through code `85`. `sldprt.md` §2 "The relation table of a sketch block follows the last record of the block" gives the row layout and operand kinds. Relation tables use codes `32`, `40`, `50`, `53`, `55`, `56`, `57`, `62`, `64`, `65`, `66`, `67`, `75`, `80`, and `85` of this range. A patterned row (`57`) has two congruent operands, but the IR has no patterned constraint. A use-edge row (`32`) pairs an external-reference record with a local curve. An at-pierce row (`40`) has two point operands.

**Need.** We must know each invariant and operand role to project the remaining codes.

### DI-09. Relation codes above `85`

**Question.** What native relation does each code above `85` identify?

**Known.** The defined native numeric taxonomy ends at code `85`. No known relation-table row or sketch marker has a code above `85`.

**Need.** We must know the identities before we can define their operands and neutral invariants.

### DI-10. Ambiguous marker transforms

**Question.** Which marker-to-profile transform applies when coordinate sets permit more than one signed-axis transform and the transforms select different loci?

**Known.** `sldprt.md` §2 "Keywords length literals use the suffixes `uin`, `mil`, `mm`, `cm`, `in`, `ft`, `nm`, `um`, `µm`" through `sldprt.md` §2 "Operand tags" define transform selection, placement fallback, and the case in which all valid transforms give the same locus set.

**Need.** We must select one transform to bind each remaining marker to profile geometry.

### DI-11. Ambiguous linked reference markers

**Question.** Which profile entity does a reference marker select when its linked loci do not identify one entity?

**Known.** `sldprt.md` §2 "A Keywords configuration's decimal `id` attribute is the slot identity for" and `sldprt.md` §2 "Keywords length literals use the suffixes `uin`, `mil`, `mm`, `cm`, `in`, `ft`, `nm`, `um`, `µm`" define unique linked-handle and shared-entity resolution.

**Need.** We must select one entity to bind the reference operand.

### DI-12. Auxiliary radial framing and diameter ownership

**Question.** What complete auxiliary-record discriminator and source relation bind a radial reference, its center, diameter and construction state?

**Known.** The exact compact legacy and extended radial layouts, their native code and role fields, the `sgSlot` relation, and the current-prefix geometry-locus `sgArcHandle` point carrier identify supported dimensioned-circle records. A declared `sgEntHandle` operand that references the exact point carrier supplies its center even when no radial record is present. Tags `d4 80` and `d5 80` directly carry a finite same-feature point or constrained-point center whose local identifier equals the operand address; their explicit identity takes precedence over pair and radius searches. Other native `sgCircleDim` tags use that same explicit point identity only after no declared pair, child, geometry-locus, or unique circular-witness carrier resolves. The reference must resolve in the unique declaring `sgEntHandle` lane to a finite same-feature point or constrained-point marker whose local identifier equals the operand address. A point that is the radial member of a declared pair cannot supply this fallback center; an invalid or ambiguous identity remains native. A declared `sgEntHandle` operand also accepts an adjacent center/radial point pair when the center local identifier equals the radial object index; the radial local identifier is either absent or zero in the ordinary form, or nonzero in the indexed point form. Tag `6e 83` uses an unreferenced lane-local `sgEntHandle` to select an indexed adjacent point pair from an even coordinate-bearing roster; every pair must carry the center-local-to-radial-object join and a nonzero radial local identifier. A declared `sgEntHandle` operand with an explicit `sgSlot_c` marker accepts one unique scoped `sgSlotHandle` class with exactly two current `e7 88` cells: the first cell identifies the slot and the second identifies one of the slot's selected center points. That form carries a construction circle centered at the selected point. A coordinate-less `line_or_circle` marker before the operand with exactly two identical links to one earlier same-feature `Arc` marker is also an arc carrier when both local identifiers match and the arc passes its endpoint or unique-later-radial-witness validation. Multiple, mismatched, or invalid linked carriers remain native. The selected profile stream can omit construction circles.

Each omitted owner-qualified construction circle can have its own current or extended auxiliary envelope with repeated words that match feature-local point object indices. A matched object index can differ from the point's coordinate-roster ordinal, such as object index `7` at ordinal `5`. These address spaces cannot be interchanged. The API circle and stored diameter can agree numerically, but that agreement does not establish the auxiliary-to-diameter/center ownership graph.

**Need.** Independently established framing with opaque header words left opaque, the repeated field's address domain, and an explicit owned diameter/center association or a separately qualified owner rule. Preserve unique raw records and refuse ambiguous center, radial or diameter joins.

**Conflict.** A whole copied auxiliary header fixes an unassigned word as a discriminator. An externally known diameter followed by a unique radius match does not prove that the record names that diameter or center. The current Native marker collector and circle projection omit these auxiliary records and their owner circles; this remains an implementation gap.

**Note.** A construction observation does not establish a generic byte discriminator. Missing supported radial records do not imply construction. Construction state of a circle without a radial record is unassigned.

### DI-13. Marker-only profile placement

**Question.** How is a marker-only profile placed in model space when it has no local, contextual, or unique lane-wide compact reference-plane record?

**Known.** `sldprt.md` §2 "Point-reference cells address feature-local object indices within the owning feature object" defines model-space recovery from a planar sketch placement. `sldprt.md` §2 "When no data-class anchor selects a reference-plane frame layout, every structurally valid encoding" define the supported reference-plane frames.

**Need.** We must know the placement to transfer the profile from planar coordinates to model coordinates.

### DI-14. Other Keywords operation families

**Question.** What neutral operation does each Keywords operation family outside the typed neutral feature set represent?

**Known.** `sldprt.md` §2 "A detailed curve record is immediately followed by a curve-detail marker of the same generation:" through `sldprt.md` §2 "A bare `0` or `1` Keywords dimension bound to a unique driving distance" define the native records and operands of the supported history operations. Projected split-line operations are defined by `sldprt.md` §2 "An `moPLine_c` feature-input object containing exactly one `moPLineProject_c` class" and are outside this open item.

**Need.** We must know the operation semantics to transfer the remaining design history.

### DI-15. Up-to-vertex endpoint selector

**Question.** Which endpoint does the u32 selector in an up-to-vertex code-`3` edge-endpoint reference select?

**Known.** The point-reference form retains the edge and endpoint selector. The little-endian u32 at four bytes before the duplicated selection-vector marker is the `endpoint_selector`. `sldprt.md` §2 "A `Config-N-ResolvedFeatures` lane supplies the evaluated parameter state for configuration slot" states how ordinary point-reference object indices resolve.

**Need.** We must know the selector values to terminate the extrusion at the correct edge endpoint.

### DI-16. Travel sense of cuts and two-direction extrusions

**Question.** Which native fields set the travel sense of a cut and of a two-direction extrusion relative to the profile frame?

**Known.** The end-spec reversal flag at anchor +12 sets the first direction of a one-direction extrusion that adds material (`sldprt.md` §2). The flag does not predict the side of a cut. Of the one-sided cuts whose whole solid lies on one side of the sketch plane, the solid lies along the decoded normal for 62% of flag-`1` cuts and for 58% of flag-`0` cuts. Two-direction blind extrusions store flag `1` in 123 of 140 (38 of 54 joins, 85 of 86 cuts), and one-sided joins store it in 225 of 1,249. Of the two-direction joins with unequal depths, the first depth lies against the normal in 2 of 2 whose solid spans exactly the two depths. The Boolean word at anchor +8 and the nested face senses have no known meaning for the travel sense. A nested planar profile has a surface normal and a face sense.

**Need.** A cut or a two-direction extrusion of the wrong sense writes a different shape. Final-cap witnesses of cuts and of two-direction extrusions with unequal depths must establish the field that sets each sense and its composition with the profile frame (SF-03).

**Conflict.** For cuts, the reversal flag, the Boolean word at anchor +8 and the nested face senses each occur with both travel senses in final-cap witnesses.

**Note.** Later features can consume an extrusion end cap. A missing final cap leaves the historical travel witness unresolved.

### DI-17. Second-direction codes without a defined second side

**Question.** Does a nonzero second-direction code take effect when the direction count at anchor +4 is `1`? What second side does a second-direction code other than `0` or `1` define when anchor +4 is `0`, and what do first-direction codes `2` through `5` define with `0` at anchor +4?

**Known.** `sldprt.md` §2 "A compact extrusion end-spec child carries its class" defines the direction count at anchor +4, the blind first direction with an enabled second direction, and blind and through-all second sides. A blind or through-all child with `1` at +4 and second-direction code `1` has a through-all second side. `sldprt.md` §2 "Termination code `6` is mid-plane" defines a mid-plane child that ignores its second-direction word.

**Need.** We must know whether the second-direction word has an effect in a one-direction end spec, and the remaining second sides, to construct both extrusion extents.

**Conflict.** The SolidWorks API record of each feature with `1` at +4 gives one direction, also when the second-direction code is `1`; the decoder projects a through-all second side for those children. STEP geometry does not decide when the second direction exits the part, because both extents then give the same solid. The to-face rule in `sldprt.md` §2 "Termination code `4` is to-face" reads +22 as a Boolean reference-side flag, not as a second-direction code.

**Note.** A blind second side without a `D2` dimension stays unresolved; the decoder does not use the first-direction depth for it.

### DI-20. Feature-local face reconciliation

**Question.** What is the disposition of a feature-local face identity when no surviving face carries that identity?

**Known.** `sldprt.md` §2 "An extrusion object-name record is followed by four zero bytes, a little-endian u16 family word," through `sldprt.md` §2 "When operation objects and their dimension children form separate ordered groups, a blind" define the `moSingleFaceRef_w` path forms and their feature-local face selection. `sldprt.md` §4.1 "The fourth FACE reference is the owning SHELL." defines the typed face relation, and §5 defines how the terminal owner and feature-local face identity select the surviving face through `FACE_ID_2001`.

**Need.** We must distinguish a face consumed by a later operation from a face whose owner or identity attribute was not decoded.

### DI-21. Class-scoped inline extrusion operations

**Question.** What Boolean operation does the inline family `0x0140` with operation byte `00` represent for `moCutThin_c`, and what operation does each other unsupported family-word and operation-byte combination represent?

**Known.** `sldprt.md` §2 "Feature-tree" defines the supported ordinary extrusion combinations. The ordinary `0x0140`/`00` join mapping does not establish the operation of a `moCutThin_c` object. Family `0x01ca` with byte `00` or `02` establishes cut. A normalized Keywords `CutExtrudeThin` type also establishes cut independently. A complete configuration-local operation carrier is distinct from the base configuration's operation state.

**Need.** We must identify an independent, class-scoped Boolean discriminator for the `moCutThin_c` `0x0140`/`00` form and define the remaining family-word and operation-byte combinations.

### DI-22. Other sparse extrusion form codes

**Question.** What operation and record shape does each sparse extrusion object outside the defined form codes use?

**Known.** `sldprt.md` §2 "Keywords element order is serialization order, not regeneration order. Neutral regeneration" through `sldprt.md` §2 "Feature-tree" define direct, repeated-class, sentinel, terminated-trailer, and sparse-trailer objects. Form code `11` identifies an object form and does not determine its Boolean operation. A Keywords operation token or a complete inline operation trailer supplies the operation. Without either carrier, the operation remains unresolved.

**Need.** We must know the remaining form codes to parse each object and construct its Boolean operation.

### DI-23. Combine-body reconciliation

**Question.** How does a body selected by a compact `moCombineBodies_c` target or tool path map to a body in the final B-rep?

**Known.** The compact paths identify generated feature-local bodies. A complete target or tool path projects to one generated body reference whose feature is the terminal path producer, whose local identity is the ordered path-local sequence, and whose dependencies include every uniquely identified traversed history feature. `sldprt.md` §4.3 "00 01 00 01" through `sldprt.md` §6 "Body membership is the typed XT ownership graph." define final B-rep body identities and ownership. `FACE_ID_2001` carries face identities and does not bind a feature-local body identity.

**Need.** We must know the mapping to bind the Boolean target and tool bodies.

**Conflict.** A local identifier can also spell a typed-entry tag followed by a complete entry. The body-path reader withholds this ambiguous boundary; it does not select a corrected count. Other component-path boundaries and the dependency roster still require independent ownership evidence.

**Note.** Retaining dependencies independently of generated-body admission requires a proven complete vector boundary and unique history-feature ownership. A rejected terminal does not authorize an earlier component as a replacement body producer.

### DI-24. Delete-body reconciliation

**Question.** How does an `moDeleteBody_c` regeneration-input-local body identity map to a body in the final B-rep?

**Known.** The compact record retains the regeneration-input-local identity. `sldprt.md` §4.3 "00 01 00 01" through `sldprt.md` §6 "Body membership is the typed XT ownership graph." define final B-rep body identities and ownership.

**Need.** We must know the mapping to delete the selected final body.

### DI-25. Component-edge path reconciliation

**Question.** How does an edge selected by an entry-form `moCompEdge_c` path map to an edge in the final B-rep?

**Known.** The entry path identifies a generated feature-local edge. `sldprt.md` §3.2 "An attribute" through `sldprt.md` §4 "**Bridge `00 0e`:** `attr` is a pointer cell at body +0" define final B-rep edge identity and direction. `FACE_ID_2001` carries no edge identity. The `EDGE_PERM_ID_2_2003` attribute grammar is not defined.

**Need.** We must know the mapping to bind the selected operation edge.

### DI-26. Compact edge-vector B-rep reconciliation

**Question.** How does a compact edge vector map its feature-local edge path to a final B-rep edge?

**Known.** `sldprt.md` §2 defines the duplicated-marker framing, count, lane-local selector, typed and reference-list entry forms, separators, terminal slots, and lane-local type signatures. The ordered compact vector and its producer-feature paths are retained. The terminal component identifies a feature-local generated edge, but it does not by itself identify the final B-rep edge.

**Need.** We must define the topology join from the retained compact edge identity to the final B-rep edge before projecting the selected edge.

### DI-28. Sweep reference child versions

**Question.** Which carrier selects a component-source placement after a u32 header word, and which source-less forms establish an operand owner?

**Known.** `sldprt.md` §2 defines the ten-byte child prefix, the source-record placements at prefix +45, +69, and +81, and the two-handle and three-handle source records. The pre-contact and post-contact general-curve references identify the cross-section and path respectively. A component-reference curve is a path child. An unresolved reference does not establish a sketch or a final B-rep curve. In a further form, prefix +45 through +60 is `ff` and prefix +81 stores a u32 equal to the version word of a complete three-handle record at prefix +85. A component profile of this form names a planar sketch, and a component-reference curve of this form names a composite curve.

**Need.** Identify the placement selector and the reference namespace of the remaining source-less forms.

**Conflict.** In the version-word form, the named sketch can have no decoded profile and a decoded plane that differs from the plane of the compressed-body profile in the same feature-input lane that otherwise binds as the cross-section. A neighboring sketch does not replace an explicit reference whose child form is unsupported.

**Note.** A complete post-contact path source after a u32 header word supplies the path sketch with `feature.sweep-path-header-assumed`.

### DI-29. Composite sweep path identity

**Question.** How do the complete ordered component instances and persistent identifiers select the curves in an earlier feature result?

**Known.** The component-reference curve is the path operand. Its type signatures, instances, identifiers, and order are separate parts of the reference. Repeated local identifiers do not establish repeated curve instances. A resolved producer does not establish a final B-rep edge.

**Need.** We must bind the complete producer-result identity before representing individual generated path curves. Preserve the reference record when that join is unavailable.

### DI-30. Other compact sweep Boolean codes

**Question.** Which serialized carrier establishes the solid-sweep Merge Boolean independently of the compact class-form word?

**Known.** The Merge property is Boolean. An unrecognized compact word does not establish NewBody, Cut, or Intersect. A missing carrier does not establish Merge false. An owned unknown, malformed, or ambiguous operation cannot corroborate another lane.

**Need.** Bind the complete Merge carrier to its owning feature with independent true and false controls. Retain unresolved operations when the carrier is unavailable.

**Note.** A generated circular section supplies the solid construction and section diameter independently of Merge. The decoder applies the Merge reading to this construction and charges `feature.sweep-merge-assumed` whether or not a compact word resolved the shape.

**Conflict.** For an unresolved solid sweep, the compact binder still projects Join when every owned lane supplies word `15`. An owned unknown, malformed or ambiguous lane prevents that consensus, but agreement on word `15` does not supply the independent Merge Boolean carrier required here.

### DI-31. Native neutral-plane Draft direction reversal and outward flag

**Question.** Which native fields identify direction reversal and an independent outward flag of a neutral-plane Draft when Keywords omits them?

**Known.** `sldprt.md` §2 "A Draft feature-input interval uses the lane-scoped `moPlaneRef_w` token" defines neutral-plane Draft operands. `sldprt.md` §2 "A compact parting-line Draft uses direct duplicated component-vector records" defines the parting-line form, which has no outward operand. Keywords independently retains an explicit `Outward` Boolean for a neutral-plane Draft.

**Need.** We must recover the native outward flag of a neutral-plane Draft when Keywords omits it. The signed plane normal does not establish an independent pull reversal, an unflipped neutral-plane orientation, or the outward Boolean. Establish reversal with independent unflipped-plane and reversed-operation controls. Compact parting-line frame direction semantics require separate Source controls; they do not share the declared +80 field.

### DI-32. Other compact line-reference widths and owned pattern units

**Question.** Which complete enclosing grammars select additional compact line-reference directions, and which owned metric fields establish the corresponding pattern spacings?

**Known.** The complete 136-byte token-terminated eight-scalar form stores its unit vector at +64. Its trailer and next-object boundary select the width. An overlapping unit triple does not select another width. The raw display-spacing fields and bare history literals are separate carriers; a new direction does not establish the literals' length unit.

**Need.** Bind the complete dimension-record framing, feature owner, role and unit before enabling a pattern extent. Preserve unknown directions or spacing semantics when those joins are unavailable. Do not apply a document-wide scale from numeric agreement.

**Conflict.** The compact direction reader has no discriminator for the complete 136-byte token-terminated framing. Its callers supply a feature interval, not proof of each inner record boundary. Pattern spacing callers also accept bare positive history literals as millimetres; a newly selected direction does not establish that unit or authorize a pattern extent.

### DI-34. SWIFT implicit nominal construction

**Question.** Which nominal-geometry rule applies to each remaining feature-size annotation whose `Nominal` field is zero and whose `Dimension` field is absent or zero?

**Known.** `sldprt.md` §2.1 defines zero as an omitted nominal sentinel when `Dimension` is absent or zero. It defines diameter and depth nominal recovery from the rendered literal, declared decimal places, pattern or compound-feature traversal, and cylindrical or spherical nominal geometry. The rendered literal, not the unrounded geometry, supplies the labeled value. It defines width and length from named slot-geometry fields and radius from the named radius field of fillet, cylindrical, or spherical geometry. It defines counterbore diameter from the direct nominal cylinder and countersink diameter and angle from the direct nominal cone. An empty applied-feature graph and empty CAD identifiers supply no direct geometry and do not bind an unrelated rendered literal; an empty `GdtPattern` can additionally use the exact `Hole PatternN` to `LPatternN` history join when one seed has exactly one later consuming Hole with a positive diameter. Directional distance annotations recover plane, axis, compound-hole, and closed-slot feature-of-size locations from their named geometry.

**Need.** The empty-pattern history join is settled by `sldprt.md` §2.1. We must still define nonidentity `NominalTransform` semantics and the other `ComputeAnswerBy`, `Direction`, and `NormalTo` modes before those forms can supply a nominal. A zero direct-cylinder `ThreadDepth` does not establish an evaluated thread depth. Recovering that form requires a source-owned association between the annotation and its rendered depth, and a rule that distinguishes an omitted thread depth from an authored zero. A file-wide matching hole-depth literal and ordinary cylindrical extent do not establish that association.

### DI-35. Variable-fillet endpoint grouping and control ownership

**Question.** Which complete repeated dimension-object grammar separates the selected-edge roster from radius controls, and how do intermediate controls bind to the full radius law?

**Known.** Within one owned variable-fillet object, two proper reference subsets associate the final two selected references with their retained-endpoint radii. The full ordered component identity distinguishes references. A control equal to the whole selected vector does not identify an endpoint. Endpoint associations and complete radius-law controls are separate relations.

**Need.** Prove the complete counted and typed repeated-object boundary in every selected/control caller. A marker sequence alone does not establish that boundary. Bind each scalar through its owned named dimension record, preserve intermediate and exact-equal controls, and establish their parameter positions before constructing the full law. Preserve the existing explicit-endpoint and legacy contracts without inventing endpoint knots or a final B-rep mapping.

### DI-36. SurfaceCut discarded side

**Question.** Which native field in an `moSurfCut_c` interval selects the retained side of a surface cut?

**Known.** `sldprt.md` §2 defines the target-body reference-list vector and the cutting-surface component vector. The target projects as a body selection and the cutting surface projects as a face selection. The neutral `CutWithSurface.reverse` value remains optional; an absent native Boolean remains unresolved.

**Need.** Distinguish the wrapper selector byte and the tail words between the surface-body identity pair and the termination sentinels with labeled parts cut to opposite sides before assigning the reverse value.

### DI-39. Temporary-axis ownership in revolution features

**Question.** Which owned revolution input identifies the temporary-axis carrier when a profile-source join or another line-reference candidate is also present?

**Known.** `sldprt.md` §2 "An `moCirPattern_c` interval retains its repeated seed" defines the nine-scalar carrier through `+311`, its middle xyz triple at `+263` as the axis point, its final xyz triple at `+287` as the unit axis direction, its exact compact or declared boundary, and its valid zero stream address. The carrier fields do not establish which feature owns the axis or whether another independent source selects a different axis.

**Need.** A complete profile and axis ownership relation must connect the carrier to the revolution. Competing complete source operands must agree before the axis is admitted; source order and fallback priority do not establish agreement.

### DI-40. Extrusion profile identity without an explicit profile relation

**Question.** Which native relation binds the profile of a classless
`Extrusion` or `DissectableRoot` extrusion when `Profile` and
`DissectableChildren` do not identify a sketch?

**Known.** An explicit `Profile` attribute or a single dissected sketch child
binds the profile. Source identifiers identify feature records. Feature order alone does not
establish that an earlier sketch is the extrusion's regeneration input.

**Need.** We must decode the profile dependency before projecting the extrusion
with a neutral sketch profile.

**Note.** A unique unused earlier sketch does not establish a universal no-reuse rule. Profile ownership and configuration-scoped absence or conflict require independent source relations; an unsupported provisional choice must not block an earned relation.

### DI-41. Pattern seed identity without a resolved seed relation

**Question.** Which native relation binds the seed of a linear or mirror pattern
when its component vectors and persistent generated-surface identities do not
resolve a seed feature?

**Known.** Complete component paths and generated-surface identities retain provenance, but do not certify a complete selected-feature roster. A two-seed mirror can store paths that mention only one seed. A derived cosmetic-thread output identifies a generated child, not its input seed. Feature order does not establish ownership. The reader withholds nearest-thread, preceding-object and mirror-provenance seed guesses while preserving explicit seed relations, dimensions, directions, mirror planes and Native records.

**Need.** Decode the complete manager/instance-holder ownership and selected-list grammar, including configuration-scoped absence and conflicts. A bounded search witness or unique provenance candidate cannot establish complete selection cardinality. Circular and curve-driven seed families require their own independent selection proofs.

### DI-42. A configuration stream that holds the active configuration's values

**Question.** Does a `Contents/Config-N` stream show that its equation-manager values are not its configuration's current values?

**Known.** `sldprt.md` §2 states that an inactive configuration's values come from its own stream. A stream can hold, line for line, the values of the active configuration's stream while the configuration's own body shows other values. The specification gives no field that separates such a stream from one whose configuration has the active configuration's values.

**Need.** We must know this to report the configuration's values as not established. Without it, the decode keeps the active values and reports them as the configuration's.

### DI-43. Angular Equation Units in trigonometric evaluation

**Question.** Does the equation angular-unit mode set the unit that the equation manager's trigonometric functions take and give, and does `int()` truncate or floor a negative argument?

**Known.** `sldprt.md` §2 "The equation angular-unit mode is the byte" defines the mode field, with native value `0` for Degrees and `1` for Radians. SolidWorks documents the setting in the Equations dialog; its default is degrees. Where a later line stores an `atn` result as its input, an `atn` that gives radians reproduces that stored value in every configuration, and one that gives degrees reproduces none, while the same lines read their angle dimensions in degrees. `int()` truncates the manager's value, and a value a few ulps short of a whole number is that whole number (`sldprt.md` §2). Every stored `int()` argument is zero or positive.

**Need.** We must know the unit to evaluate a line that calls a trigonometric function. Without it, the decode gives such a line no value, and its parameter keeps its literal.

**Conflict.** After the last relation's lowercase name and value table, where a section holds the relation count minus one as a u32 followed by `00 00 ff ff ff ff`, the u32 before it is 0 or 1. It is 0 in a manager whose `atn` gives radians and in managers without trigonometric lines. It is 1 in managers whose equations come from an external file and in some other managers. A manager section can lack this anchor. If that u32 holds the mode field, value 0 in the manager whose `atn` gives radians disagrees with the native Degrees value `0`. No comparison places that u32 relative to the settings boundary `H`.

**Note.** DI-02 holds the relation-list grammar that locates `H`.

### DI-78. Equation-manager stored values in a part whose relations sit in several streams

**Question.** In a part whose equation-manager relations sit in several `Contents/Config-N` streams, are the values a stream stores beside a line's names the values of that stream's configuration, so that they test the active configuration's lines as they do in a part whose relations sit in one stream?

**Known.** `sldprt.md` §2 "A relation whose byte 1 after its text is `02` or `03` stores its sides as tokens" places the f64 stored beside each dimension reference. The decoder tests a line's tree against the values its relation stores beside the names it reads, and types a dimension of unread kind as a length from the value stored beside it (`sldprt.md` §2 "The active configuration's equation-manager lines are parameter expressions" and "A dimension with no `PMISemanticDataDB` record"). It reads the active configuration's stream, and refuses a line that another stream does not hold with the same right-hand side. The value stored on the assigned side can be another configuration's value, and a whole stream can hold another configuration's values (`sldprt.md` §2 "The name in a token is the name the manager resolved"). Where the stream read holds another configuration's value of a dimension a line reads and that value differs from the decoded one, the line stays source text. The decoder types a dimension as a length from the value the manager stores beside it where its display in the part's length unit equals that value, or where each value stored beside a name the line that drives it reads equals that name's decoded value, the line evaluated over those values gives the stored value, and the display is the stored value rounded to the document's length precision.

**Need.** A part whose relations sit in several streams, with each configuration's decoded dimension values (its Keywords snapshot or its `PMISemanticDataDB` records) to compare with the values its stream stores beside the same names, and each stream's lines evaluated over that stream's values. Where every stream stores its own configuration's values, the active stream's values test the active configuration's lines as in a part with one stream. Without it, a line of such a part that reads a value its stream stores for another configuration stays SolidWorks source text.

### DI-44. Sign of an integer scalar value

**Question.** Is the four-byte value of an `moIntegerParameter_c` named scalar signed or unsigned?

**Known.** `sldprt.md` reads it as a little-endian i32. Stored values are 0 through 1327, so each stored value reads the same as i32 and as u32.

**Need.** We must know the sign before a writer can store a negative value. Without it, the writer refuses one.

### DI-79. Trailer of a value-only scalar behind the 18-byte header

**Question.** What do the bytes from trailer +7 of a value-only scalar behind the 18-byte header hold where they are not the compact layout's trailer, and what do the first byte at trailer +21 and the byte at trailer +27 hold where trailer +21 stores `xx 00 00 00 02 00` with a first byte other than `01`?

**Known.** `sldprt.md` §2: in a part that writes the 18-byte header, a value-only scalar stores its f64 at the header's end and a trailer after it that opens with three zero bytes and the scalar object identifier. The compact layout's trailer stores `01 00 00 00 02 00` at +21 and the role at +27, `0` driving or `1` display. Other trailers of this form take these forms: `xx 00 00 00 02 00` at +21 with `xx` one of `02`, `03`, `0f`, `10`, `29`, `2b` and `2f`, and `0` or `1` at +27; `01 00 00 00 02 00` at +21 with nonzero bytes in +7 through +20 or in +28 through +34, where the compact layout's trailer holds zero bytes; `01 00 00 00 03 00` at +21; an f64 at +21; zero bytes from +7 through +34; a small little-endian u32 at +11. The decoder reads every scalar of these forms with the native role.

**Need.** Scalars of each first byte compared with the dimensions their names give: whether the +27 byte separates a driving value from a display value as it does in the compact layout, and which record kind each first byte names.

### DI-89. The 22-byte scalar header whose last eight bytes are zero

**Question.** Is the 22-byte header `00 00 00 00 00 00 00 40 ff ff ff ff 00 00 00 00 00 00 00 00 00 00` a named-scalar header whose value begins at its end, and which roles do its trailers assign?

**Known.** `sldprt.md` §2: a part writes all of its named scalars with one header width, and the primary and legacy layouts use a 22-byte header whose bytes 16 through 18 are `ff fe ff`. This other 22-byte header is the 14-byte value-only header and eight zero bytes. An f64 follows it, and a trailer of three zero bytes and the scalar object identifier follows the f64. A little-endian i32 can follow it instead, with such a trailer after the i32; the stored i32 values are positive counts. The decoder reads the value at the end of the 14-byte value-only header, so these scalars state 0. Behind this header, the `D1` parameter object of an offset reference plane stores the plane's distance in metres at the header's end, as its display shows; behind the same header and trailer form, sketch-dimension scalars store values at the header's end that their displays do not show in every case. The decoder takes such an offset plane's distance from its display and names it under `feature.offset-plane-distance-displayed`.

**Need.** Scalars of this header compared with the Keywords displays of the dimensions their names give: the value at the header's end equal to the displayed number in the part's unit settles the value offset, and the trailer bytes compared with the compact and primary trailers settle the role.

### DI-90. A value-only scalar whose trailer opens with a zero byte and a class reference

**Question.** Where does a value-only scalar behind the 18-byte header store its object identifier when the trailer after its f64 opens with a zero byte and a class token or class declaration, and is that zero byte the whole trailer, so that the class reference opens the next object?

**Known.** `sldprt.md` §2: such a trailer follows the f64 at the 18-byte header's end. The zero byte is at trailer +0. A class token occupies trailer +1 and +2, and a class declaration's `ff ff` marker begins at trailer +1, where a trailer of the first form holds zero bytes; that form's object identifier is at trailer +3. The classes declared there are `moDimTolerance_c` and `moEnvFolder_c`. The decoder reads the f64 at the 18-byte header's end. The u32 it reads as the object identifier at trailer +3 holds the four bytes after a class token, or a class declaration's u16 schema number and u16 name length.

**Need.** The object that the class reference opens, compared with the objects that follow trailers of the first form, and the scalar's f64 compared with the Keywords display of its dimension.

### DI-91. Relation instances of token-form dimension scalars

**Question.** Which relation instances do the token-form display scalars and driving scalars of a sketch form, and which of a feature's sketches binds the feature when they all form?

**Known.** `sldprt.md` §2 and §2.2: a relation class declared with its parameter class inside its first relation object, after that object's zero u32, is two map indices below the parameter class. A token-form scalar whose parameter-class token is two above its relation-class token, and names the parameter class that the lane declares once, has as its relation class the class whose declaration ends four zero bytes before that parameter-class declaration. The decoder forms the instance that these tokens name in one case only: the relation-class declaration opens a value-only scalar of the native role, the display scalar is the first scalar after it that a display or driving role frames, and the driving scalar follows the display scalar. This is the instance that the declaration gave before the decoder read the padded value-only scalar. Every other scalar takes the nearest preceding compatible declaration before the next relation-class declaration, or, where its record states a parameter of another kind than that declaration's relation, the relation class that its relation-class token names by the order rule of §2, or forms no instance. That declaration can be a class declared after the parameter class that the scalar's tokens name. Its map index is then above the parameter-class token, so the scalar's tokens rule the class out, and the decoder types the instance, and a driving scalar's constraint, with a family the tokens do not state. Relation points of the instances that the tokens name change which of a feature's marker sketch and profile-stream sketch binds it, and the decoder drops the relation-table rows of the sketch that gives way.

**Need.** For each feature whose binding changes, the marker sketch and the profile-stream sketch compared with the faces of the stored solid and with the operands of the sketch's relation-table rows, so that the sketch that holds the feature's profile and relation table is known before every token-named instance forms.

### DI-45. Relation-table row flag

**Question.** What does the u16 row flag `1` of a relation-table row mean?

**Known.** `sldprt.md` §2 "The relation table of a sketch block follows the last record of the block" gives the flag position. The solved geometry satisfies a flag-`0` row. A flag-`1` row has no established invariant.

**Need.** We must know the meaning to type or to write flag-`1` rows.

### DI-46. Point records with a curve or relation code

**Question.** Which field identifies a point record whose u32 at marker +17 is `2` or a relation code?

**Known.** Such a record stores the coordinate tag `1e 00` at marker +64 and holds a slot in the point array of its block. Its u32 at marker +17 is `2` or a relation code. The decoder types the relation that such a code names only where the entities that the record's links resolve to satisfy that relation, and keeps the relation native otherwise.

**Need.** We must draw these points to type the relations that name them.

### DI-70. Geometry of unresolved line-or-circle and arc handles

**Question.** Which sketch records carry the geometry of a line-or-circle or arc handle that no marker layout in `sldprt.md` resolves?

**Known.** `sldprt.md` gives coordinate-bearing marker codes `1` and `2` as line-or-circle and arc geometry handles. A handle whose record is a curve record of a framed `sgSketch` object with selector bit 0 clear takes its geometry from the point records its indices select (`sldprt.md` §2, "A marker whose record is a point record or a curve record"). Three kinds of handle remain. In a record of the compact `ff ff 07 00 01` generation with no f32 `-1` at +13, the same endpoint, sense and centre cell stands 14 bytes earlier: endpoint indices at +42 and +44, f64 `-1` at +50, the sense at +58 and the centre index at +62; the 14-byte header and point-record bounds of its `sgSketch` object are not framed. A handle whose selector has bit 0 set is open item EV-27. A handle whose `sgSketch` header is not framed has no point-record list. After its last sketch-geometry pass, the decoder gives each remaining entity native kind `sldprt:line-or-circle-handle` or `sldprt:arc-handle`, keeps its identity and relations, and charges `sketch.handle-unresolved`. An arc handle whose inline center and endpoints define no minor arc joins its sketch after that pass, with native kind `sldprt:arc-handle`, its identity and relations, and the same loss. A line handle whose two read endpoints coincide, one of them read from a legacy 140-byte single-incidence profile-point record (`sldprt.md` §2), keeps native kind `sldprt:line-or-circle-handle` and charges `sketch.line-degenerate` when its curve record gives no geometry. The decoder places the point records of a marker sketch on a plane oblique to the model axes with a transform that is not rigid, so an arc record whose endpoints lie at one distance from its centre in the file can end off its placed endpoint point records by more than the linear resolution, `1e-8` m. The decoder then reads no geometry from that record. A handle that a marker layout of `sldprt.md` §2 reads and that no other rule draws takes that arc after the last sketch-geometry pass, about the placed centre from the placed start point record to the direction of the placed end point record in the record's sense, and charges `sketch.arc-record-off-chord`. A marker that another rule of §2 draws as a line keeps that line and charges `sketch.arc-record-conflict`. A marker that the layouts skip gives no entity and charges `sketch.marker-record-unread`. A handle whose read values define no valid line, circle, or ellipse emits no entity, and neither does a kept handle that stores its own coordinates without the geometry locus `05 00 01 00` at marker +23.

**Need.** We must read these handles to complete the sketch geometry and its profiles.


### DI-47. Relation-table framing

**Question.** Which fields frame a sketch block's relation table: the row count, the start of the table, and the length of each row's trailer?

**Known.** `sldprt.md` §2 "The relation table of a sketch block follows the last record of the block" gives the row header, the handle cells, and the trailer forms of the typed codes. The u32 thirty bytes before a table's first row is not always its row count. The handle cells are new archive objects, so a row added or removed changes the archive's object indices after it.

**Need.** We must know the framing to add or remove a relation row. Without it, the writer refuses both.

### DI-48. Class of a repeated class token

**Question.** Which feature-input class does a repeated class token name, and which of the class and the `Type` token selects the Boolean operation of an extrusion?

**Known.** `sldprt.md` §2 "Keywords `Feature` elements use the `Type` attribute" gives the class of instances with the same `Type` token and the same class token. `sldprt.md` §2 "An extrusion feature-input object stores a little-endian u32 form code" gives the place of the class token. A repeated class token is the archive map index of its class with the high bit set. On native parts, the `Boss-Extrude` instances after the first carry the repeated class token that a `Cut-Extrude` instance carries, and the archive walk reads `moICE_c` for those objects. The first `Boss-Extrude` instance declares `moExtrusion_c`. Such a `Boss-Extrude` instance can store form code `5`.

**Need.** We must bind each history feature to its own class, and give an extrusion its Boolean operation.

**Conflict.** The decoder gives an unbound instance the class of its `Type` siblings, so these `Boss-Extrude` instances take `moExtrusion_c`. The serialized feature-input lanes do not hold the archive class map, so a validator cannot find the class of a repeated class token from them. The decoder maps form code `5` under `moICE_c` to a cut, and the `Type` token gives a boss.

### DI-49. SWIFT tolerance-zone selectors outside positions

**Question.** Which `ZoneType` values define tolerance-zone forms for each geometric-tolerance class?

**Known.** `sldprt.md` §2.1 defines `GdtPosition.ZoneType` value `2` as a spherical tolerance zone.

**Need.** Establish the class-specific mapping for the remaining selector values and annotation classes from independently identified zone owners.

### DI-50. SWIFT pattern to history feature

**Question.** How does a SWIFT pattern bind to the history feature that produced its controlled faces and driving dimensions?

**Known.** A `GdtDiameter` with an omitted nominal can reference an empty SWIFT `Hole Pattern<n>` while the history `LPattern<n>` with the same suffix has an empty seed list. A populated SWIFT hole pattern stores CAD identifiers and applied compound holes, such as six identifiers and two compound holes, and its suffix can exceed every history `LPattern` suffix. A `GdtDiameter` with an omitted nominal that refers to such a populated pattern can render its callout as `.812` in. The Hole Wizard feature whose placement lies on one of the pattern's hole axes carries a counterbore diameter of `.8125` in; its sketch parameter and the STEP counterbore geometry carry the same value.

**Need.** Establish a serialized identity relation between SWIFT patterns and history features. Resolve the populated pattern's CAD identifiers and applied holes to a history feature and its driving parameter, and determine which nominal source takes precedence when the rendered callout and history parameter differ.

**Conflict.** The rendered diameter is `.812` in and the required nominal is `.813` in; the matching STEP geometry and Hole Wizard parameter are `.8125` in. The populated pattern's suffix does not match any history `LPattern` suffix.

**Note.** The two pattern children reference cylinders of radius `10.31875` mm on two parallel axes. The decoded Hole Wizard feature has its placement on the first axis and counterbore diameter `20.6375` mm. A history `LPattern` displays a count of two and a spacing of `10` in, equal to the separation of those axes, but its decoded seed list is empty. This makes that `LPattern` a geometric candidate for the populated pattern's history owner without establishing identity. The numeric suffix alone does not establish an identity relation between the two pattern families.

### DI-51. FTC-06 spherical and controlled radius owners

**Question.** Which SWIFT feature and dimension fields bind a counted spherical-diameter callout, a reference spherical-radius callout, and controlled radius callouts to their intended features?

**Known.** The required PMI gives two spherical diameters of `1.250 ± .008` in, 24 radii of `.12 ± .02` in, a parenthesized spherical-radius callout `(SR 1.000)`, and two controlled radii of `.50 ± .02` in. The SWIFT spherical-diameter record is `GdtDiameter` type `3` and directly references one `GdtSphere` feature, whose radius is `15.875` mm; its native `Nominal` is zero and its implicit diameter is `31.75` mm. A second sphere feature of the same radius appears in the root feature roster. The STEP twin encodes a `1.250 ± .008` in spherical diameter that targets both spheres. The spherical-radius record is `GdtRadius` type `0` and directly references a `GdtSphere` feature whose radius is `12.7` mm; the native record stores zero nominal and `±.508` mm (`±.020` in), and the STEP twin encodes a `12.7` mm spherical radius with the auxiliary modifier. A further `GdtRadius` record supplies 24 `.125 ± .02` in radii with 24 target faces. A `GdtRadius` type `3` record references a fillet pattern whose applied-feature collection contains two `GdtFillet` records; its neutral PMI is `.50 ± .02` in with two target faces. `GdtDiameter` records use type `3` for sphere, pattern, and compound-hole owners. `GdtRadius` records use type `0` for the sphere owner and type `3` for fillet patterns.

**Need.** Establish a typed relation between the counted display and its two sphere owners. Determine whether the native spherical-radius magnitude and reference modifier correspond to the required callout and STEP annotation.

**Conflict.** The required PMI and STEP describe two `1.250` in spheres, while the spherical-diameter record and the spherical position record reference one sphere. The parenthesized `SR 1.000` and the STEP auxiliary spherical-radius annotation indicate reference status. The native spherical-radius record stores `±.020` in tolerance on a sphere of `.500` in radius; its history display parameter displays `SR0.5`, and the STEP spherical radius describes the same radius. The required PMI gives `1.000` in.

**Note.** The raw SWIFT presentation has one `2X <MOD-SPHDIA>` token without a nominal. A history text parameter carries `2X <MOD-SPHDIA>1.25`; its normalized value matches the `31.75` mm implicit diameter derived from the direct sphere owner of the spherical-diameter record. Exactly two unsuppressed `GdtSphere` features have radius `15.875` mm, including the direct owner. The STEP spherical-diameter annotation has four face targets on the two spheres. The decoder adds the second sphere's concrete face targets under the count-text and equal-radius assumption and reports `pmi.swift-spherical-diameter-count-text-equal-radius-assumed`. The typed identity relation between the display object and the spherical-diameter record remains unclassified. `GdtRadius` type `0` maps to a spherical radius when it has one direct class-matched sphere owner. A reference square-width candidate also uses type `0` in a different dimension class. The native spherical-radius record retains its stored `±.508` mm tolerance. The native radius tolerance, its reference status, and the treatment of its omitted source nominal remain unresolved. The counted display lies outside the semantic root and the spherical-diameter entity. The textual identifier of the spherical-diameter record occurs in the root annotation roster. The second sphere's feature identifier occurs once, in the root feature roster. The history text parameter belongs to the history `Annotations` feature. The `CadReferences` collection of the spherical-diameter record is empty. The parsed feature list of the referenced sphere is empty. In the SWIFT presentation archive, the `.025` `D|B|C` geometric-tolerance display and the presentation objects of the spherical-diameter and position records reference only the presentation mirror of the referenced sphere. No `moSwiftFR_c` object references the presentation mirror of the second sphere.

### DI-52. FTC-06 flat and conical taper ratios

**Question.** Which native SWIFT class and fields own the basic flat and conical taper ratios and their target shapes?

**Known.** The required PMI gives a flat taper basic ratio of `1:2` and a conical taper basic ratio of `1:3`. In the STEP twin, one `DESCRIPTIVE_REPRESENTATION_ITEM` carries `2X 1.00 : 3.00 #{conical-taper}` and another carries `#{slope}1.00 : 2.00`.

**Need.** Join the ratio labels to native SWIFT records and identify their controlled features.

**Conflict.** The required PMI designates both ratios as basic. STEP carries each ratio as text and associates it with target geometry. The current decode emits neither ratio as neutral PMI.

**Note.** The STEP conical-taper text reaches its shape aspect through a representation and a semantic-text property; four NOTE face uses each target one conical surface. These surfaces form two spatial groups. Each has a half-angle of `0.165148677414627` radians and a pi sweep; `2 tan(half-angle)` evaluates to `1/3`. The STEP slope text reaches its shape aspect through a representation and a semantic-text property; it targets one planar face. One native collection contains a boss and a cone feature, and a second collection contains a cone and a boss feature. Each cone stores a full angle of `0.33029735482925343` radians, which also evaluates to ratio `1:3`. SWIFT surface-profile annotations reference both collections. `CadIdentifier` uses the `<lane>:<suffix>` identity syntax in §2.1, so reading that pair as a numeric ratio is a false candidate. Earlier native-stream searches used unspaced literals `1:2` and `1:3`, while STEP text includes spaces around the colon. Searches of the `SWIFT/Config0-Schema<n>` stream return zero ASCII and UTF-16LE hits for `1.00 : 2.00`, `1.00 : 3.00`, `slope`, `taper`, `conical`, `ratio`, `GdtSlope`, `GdtTaper`, and `GdtConicalTaper`. These results reject the literal-text and class-name candidates. The native ratio-label owners remain unresolved.

### DI-53. FTC-06 datum targets and target-line symbols

**Question.** Which native objects encode datum targets and their target-line symbols?

**Known.** The required PMI names datum targets `G1`, `H1`, `J1`, `J2`, `K1`, and `K2`, and shows datum-target symbols and target-line symbols for `J1`, `J2`, `K1`, and `K2`. The parsed SWIFT annotation tree contains `GdtDatum` records identified as `G`, `H`, `J`, and `K`, each with a feature reference.

**Need.** Identify the target-instance records and symbol owners, then bind each target identifier to its datum feature and emitted neutral target or symbol.

**Conflict.** The required PMI identifies six datum targets and four target-line symbols; the current PMI output projects base datum annotations without those target instances or symbols.

**Note.** `G` and `H` each reference one cylinder. `J` and `K` each reference a compound width with two ordered plane children. The datum records hold the base letters. The child order provides candidate pairs; the target-number field and symbol-owner reference remain unresolved.

### DI-54. FTC-06 controlled-feature owners

**Question.** Which four native feature owners correspond to two required pairs of controlled features and their position frames?

**Known.** The required PMI assigns two diameter `1.000` in features to position `.025` with datums `D|B|C`, and two size `1.106` in features to position `.025` with datums `A|B|C`. The SWIFT tree includes two `GdtPosition` records, both at `.025` in, that reference separate `GdtCompoundWidth` features; their datum references are `D|C|J` and `D|C|K`.

**Need.** Trace each of the four controlled features through its SWIFT owner, applied-feature collection, and B-rep faces, then match the four resulting owner/frame pairs to the required groups.

**Conflict.** The source tree contains multiple `.025` frames on distinct owners, including the two compound-width candidates with datum sequences `D|C|J` and `D|C|K`. The four required owner-to-frame joins remain unresolved.

**Note.** The two `D|C|J` and `D|C|K` positions reach two `GdtWidth` records. Each width is `.500` in and references two planes; their datum sequences and sizes differ from the required groups. The `.025` `D|B|C` position references one sphere. The `.025` `A|B|C` position references four counterbores through a hole pattern; another position references a second hole pattern with `C|A|B`. Two history text parameters named with the `RD` prefix display diameter `1`, and two others display `1.106`. Their links to the controlled features remain unresolved. Two STEP dimensions carry `1.106` in between isolated line pairs. The STEP `D|B|C` position targets the two spheres; two STEP `A|B|C` positions target the `.415` in holes and `.625` in counterbores. Equal position magnitudes do not establish the four requested owner and datum combinations. The two `RD` diameter parameters match STEP auxiliary diameters at `25.4` mm. The two `RD` `1.106` parameters match the two STEP location dimensions at `28.0924` mm, which carry no auxiliary modifier. STEP auxiliary dimensions also occur in parts that have no `RD` history dimension, so the `RD` prefix does not select reference status.

### DI-55. FTC-06 spherical position owner set

**Question.** Which native relation applies a spherical position frame to both of its two controlled spheres?

**Known.** The required PMI and the STEP twin apply position `.025` with datums `D|B|C` to two spheres. The SWIFT `GdtPosition` record has a spherical zone and references one sphere feature; its `CadReferences` collection is empty. The second sphere is a separate sphere feature with the same `15.875` mm radius.

**Need.** Establish whether another SWIFT owner or a shared feature relation applies the position record to the second sphere, and resolve the native target set against both STEP sphere faces.

**Conflict.** The required PMI and STEP have two controlled spheres; the inspected SWIFT position record names one sphere feature.

**Note.** The parsed feature and annotation traversal assigns each sphere its own CAD identifier and centre. The raw SWIFT stream contains the second sphere's feature identifier once, in the root feature list. It contains the referenced sphere's identifier three times: the root feature list and the direct references of the position record and the spherical-diameter record. The sole occurrence of the second sphere's CAD identifier is in that sphere's own `CadReferences` collection. Both sphere entities contain `CadReferences` and `NomSphere`; their parsed feature lists are empty. The position record has an empty direct annotation list. Its three applied-datum children each reference one datum: D, B and C in order. The counted display is outside the semantic root, and the position record's annotation identifier occurs in the root annotation roster. These records leave the display-to-position and second-sphere relations unresolved. No `moSwiftFR_c` object in the presentation archive references the presentation mirror of the second sphere; the display that depicts the position record has a one-feature path to the presentation mirror of the referenced sphere.

### DI-57. FTC-08 square reference dimension

**Question.** Which native fields identify the square characteristic and reference-only status of a square reference dimension, and how do they map to neutral PMI?

**Known.** The required PMI shows a square symbol and `(1.100)` as a reference dimension. The native `GdtWidth` record stores `Type=0`, zero nominal and limits, and tolerances of `-0.25` and `+0.25` mm. It references a `GdtCompoundWidth` feature with two plane feature references. The current neutral output carries a `Size` dimension of `27.94` mm, a `±0.25` mm tolerance, and two opposed planar face targets.

**Need.** Establish the meaning of `Type=0` for `GdtWidth`, then identify the source fields for the square symbol and reference-only state and assert their neutral representation.

**Conflict.** The required PMI marks a square reference dimension. The native record's subtype and status mapping remain unestablished; the emitted neutral dimension has size and face targets.

**Note.** The referenced `GdtCompoundWidth` stores a `GeoOpenSlot` width of `27.94` mm and nominal length of `24.765` mm. The two source measurements therefore cannot establish a square from equal width and length. A history text parameter carries `<MOD-BOX>1.1`. The presentation sequence contains a `moRefSwiftDim_c` with that parameter, a `moSwiftFPlaneRef_c` point, and `<MOD-BOX>`. The point is a corner of one target face of the `GdtWidth` record. The typed link to the `GdtWidth` record remains unresolved. A reference spherical-radius candidate also stores `Type=0` and nonzero bilateral tolerances, but belongs to `GdtRadius`. `GdtDiameter` and `GdtDistanceBetween` records with `Type=0` can match STEP dimensions marked `theoretical`. The class-specific type codes remain unresolved. The display of the `<MOD-BOX>` parameter is a `moDisplayDistanceDim_c` in a later display list. Its `moSwiftFR_c` path names a presentation object; the `GdtCompoundWidth` that the `GdtWidth` record references has a `UserAttribute` equal to that object's identifier. The `GdtWidth` record is the only annotation that references that feature. The `moDimTolerance_c` object after each display parameter's value stores a u32 tolerance form first. It is `0` for the `<MOD-BOX>` parameter, for the parameter of the reference spherical-radius candidate, and for a `GdtDiameter` or `GdtDistanceBetween` parameter whose STEP twin has no auxiliary dimension; parameters whose STEP twins have auxiliary dimensions store other forms. The form therefore does not select reference status. The 96 bytes before each parameter name hold no byte that the two reference parameters share and the other parameters lack.

### DI-58. FTC-08 parallelism modifier conflict

**Question.** Does the native source assign a tangent-plane modifier to a `.030` parallelism frame that requires free-state and tangent-plane modifiers?

**Known.** `GdtParallelism.IsTangentPlane` stores `0` when the tangent-plane modifier is clear and `1` when it is set. The required PMI asks for `.030` parallelism to datum A with free-state and tangent-plane modifiers, and on one surface repeats that frame with `.015` tangent-plane parallelism to datum D. The native `.030` record stores `.762` mm with `IsFreeState=1`, `IsTangentPlane=0`, and a direct feature reference to the same plane as a second native record. That second record stores `.381` mm with tangent-plane set and datum A. The separate `.015` tangent-plane datum-D frame matches the required frame. Neutral output carries `.030` free-state only and `.015` tangent-plane separately.

**Need.** Resolve the requirement for tangent-plane state on the `.030` frame against the native semantic and display records.

**Conflict.** The required PMI has both modifiers on `.030`; its native record has `IsTangentPlane=0`. The STEP twin assigns both modifiers to the `.030` frame on the same plane. The `.015` tangent-plane records have different magnitudes or datum references.

**Note.** The complete integer-property section of the `.030` record has one `IsTangentPlane` field, set to zero. The other `.030` annotation references a different plane and has both free-state and tangent-plane clear. The two `.015` tangent-plane annotations use a different magnitude; one shares the plane of the `.030` record and the other references a different plane. The corresponding display sequence places `<MOD-TANP>` with the `.015` parallelism row and `<MOD-FREES>` with the `.030` row. The semantic and display records agree on these modifier assignments.

### DI-59. FTC-08 between extents

**Question.** Which source references bind the `L1` and `L2` supplemental geometries of a surface profile as ordered between extents?

**Known.** The required PMI describes a surface profile between `L1` and `L2`; its `.015` callout shows the flatness symbol. The native surface-profile record stores `Tolerance=.381` mm and `OuterToleranceValue=.1905` mm and directly references a `GdtPattern` collection. That pattern has ten CAD references and one applied feature reference to a pocket feature; the pocket references two planes. Neutral output carries the `.015` surface-profile magnitude and ten face targets.

**Need.** Resolve the pattern's CAD references and the pocket's plane references to the `L1` and `L2` supplemental geometries, including their order, then determine whether the source relation can be emitted in neutral PMI.

**Conflict.** The required PMI names a profile tolerance and its callout shows flatness. The current neutral profile annotation carries its magnitude and face targets; `ToleranceConstraints` models tolerance zones and projections.

**Note.** In the `SWIFT/Config0-Schema<n>` stream, one `DetailItem` sequence carries `<GTOL-FLAT>`, `.015`, `L1`, and `L2` in that order. The preceding `DetailItem` sequence carries `<GTOL-SPROF>` and `.015`. These sequences distinguish the flatness callout from the profile callout. The native `GdtFlatness` record stores `.381` mm and references a plane whose face is outside the surface-profile target set. The two direct pocket planes are parallel with opposite normals; their CAD identifiers also occur in the ten-member pattern. The STEP `.015` profile references eight faces that match eight of the ten surface-profile targets. The two additional native targets are the pocket cap planes. STEP text items reference separate shape aspects for `L1` and `L2`; their geometry has no established relation to the profile group or the two cap planes. A typed presentation-to-annotation join and ordered supplemental geometry references remain to be resolved. `L1` and `L2` are the between-extent labels in the `moGtolData_c` string block of the `.015` flatness-symbol display. That display stores no feature path; it attaches through face references. The flatness-symbol display has one face reference. Its first hop is a `moPLineSurfIdRep_c`, so the face it selects follows the split-line identity layout, whose own fields in the presentation archive remain unresolved.

### DI-61. FTC-06 row 87 quantity scope

**Question.** How does a `2X` multiplier scope hole and counterbore callouts over a native set of four axes?

**Known.** The required PMI applies `2X` to both the `.415` in hole diameter and the `.625` in counterbore diameter with `.50` in depth. The SWIFT diameter, counterbore, depth, and position records reference one hole pattern. That pattern contains four applied compound-hole records. The native and STEP target geometry has four distinct axes for both `.415` and `.625` in cylinders.

**Need.** Bind the multiplier to its intended axes through the callout's leader or establish how it addresses two repeated groups of two; compare that grouping with the four pattern children and STEP target aspects.

**Conflict.** The required PMI gives `2X`; the referenced SWIFT pattern and STEP targets span four distinct axes through one four-child pattern.

**Note.** The hole pattern has four direct applied compound-hole children. Each child references the same supporting plane and its own cylinder and counterbore geometry. The presentation text for the `.415` callout uses the `<COUNT=#X >` placeholder; the adjacent counterbore text supplies `.625` and `.500`. A history text parameter displays `4X` with the same hole, counterbore and depth values. Its link to the pattern remains unresolved. The direct child list has no two-child subgroup to select either pair, and the placeholder requires its count source to establish the displayed multiplier. The geometric-tolerance display with that dimension parameter has a two-entry feature path, the hole pattern and its first compound-hole child, and depicts the position record. Its frame template is `<COUNT=#X >` followed by `<Gtol>`.


### DI-63. SWIFT `IGTOL` characteristic tokens

**Question.** What does the `IGTOL-` prefix of a SWIFT frame-row characteristic token, such as `<IGTOL-FLAT>` or `<IGTOL-PARA>`, select?

**Known.** `sldprt.md` §2.1 maps the `<GTOL-name>` tokens to characteristics. A display can store two `<IGTOL-FLAT>` rows with `.03` and `.015` and the free-state modifier on the first. Its face reference then selects the face that two `GdtFlatness` annotations target, with magnitudes `.762` and `.381` mm.

**Need.** Establish the prefix's general role and the remaining `IGTOL-` characteristic meanings.

**Note.** The decoder associates an `IGTOL-FLAT` display with its flatness annotations when the owner or face references select the complete set of at least two rows and all positive row magnitudes agree through one common unit factor. It reports `pmi.swift-internal-flatness-symbol-assumed` for that reading. Other internal symbols remain unresolved.


### DI-64. End of the last SWIFT geometric-tolerance display

**Question.** Which bytes end the last geometric-tolerance display of a SWIFT presentation archive whose datum-tag list is empty?

**Known.** `sldprt.md` §2.1 states that the u16 count of the second display list ends the last geometric-tolerance display, and that the first item of a non-empty second list declares `moDisplayDatumTag_c`. An archive whose second list is empty writes no `moDisplayDatumTag_c` declaration after the last geometric-tolerance display.

**Need.** Bound that display, so that its note, rows, and feature or face references decode.

**Note.** The decoder discards that display and reports `pmi.swift-presentation-unframed`.

### DI-65. Indexed-circle array and plane ownership

**Question.** Which native ownership fields associate equal-endpoint arc arrays with their point arrays and model-space sketch frames?

**Known.** The counted sketch arrays preserve point slots and contour ordinals. The current-prefix equal-endpoint arc form has native value zero, centre slot zero, sweep selector one, and four signed reference sentinels. The indexed-curve grammar in §2 supplies these fields. The marker classifier labels this form as a point; projection lacks its coordinate pair.

**Need.** Explicit array, profile, and plane ownership must select the point block and model-space frame, with carrier and applicable start-cap agreement.

**Conflict.** The first-point centre and indexed radial-witness reading has contrary carrier cases under the decoded sketch placements. The compact extended-prefix and legacy equal-endpoint readings also have contrary carrier cases.

**Note.** Retain these records under their current projection until the mapping resolves.

### DI-68. Extrusion end-spec ownership and depth attribution

**Question.** Which serialized children define the active termination kind, depth, direction, and second side of each withheld extrusion extent?

**Known.** Each withheld extent has one primary cause: display-length parsing, a face-reference form, a vertex-reference form, a traversal-tail form, an unbound depth scalar, an unsupported second-side code, an unavailable end-spec header, an additional first-direction code, or surface-extrusion ownership. Display-length parsing gives direction-depth pairs that are equal or unequal; equal pairs are the most frequent. Finite-cap checks of those extents give agreements, contrary cases, and unavailable results. The start type of the `moFromEndSpec_c` object reads as type 0, 5, or 1; type 0 is the most frequent. Each type 5 and type 1 start stays unresolved. In a type 5 extrusion whose sketch frame is not a fallback frame, the owned cap faces place the near cap at the scalar distance from the sketch plane on the normal side or on the opposite side, and place the far cap beyond the near cap or between the near cap and the sketch plane.

**Need.** Bind each active end-spec, depth scalar, travel direction, and second-side reference to its extrusion. Bind the side of the sketch plane on which a type 5 start lies and the meaning of a type 1 start to the extrusion. Check every proposed admission against the decoded B-rep cap distances in at least two independent design groups, including both caps for a two-sided or symmetric extent. Available truth and export geometry supply supplementary checks.

**Conflict.** A decoded termination kind can coexist with an unresolved length. A retained display length and a named scalar can coexist with an incomplete depth binding. The scalar can equal the separation of surviving caps while their distances from the bound profile plane differ from the scalar. The side of the sketch plane on which a type 5 start lies differs between extrusions with the same implicit travel direction.

**Note.** Each primary cause retains its unresolved extent and decode loss until its ownership and geometry checks succeed.

## 6. Container record semantics

### EV-03. `SWObjects` record content

**Question.** What fields and relations occupy the unresolved bytes of each `SWObjects` metadata record?

**Known.** The supported section-root Part scalar forms are four or twelve bytes with an exact class-specific continuation; a class string in opaque data does not establish a Part. Configuration-manager prefixes are versioned and own child graphs. The reader exposes the minor version, state count and UTC FILETIME only under the scalar-prefix bounds in §8; those fields do not establish a record end. Retained writes preserve an unchanged supported tuple; scalar edits and fresh generation remain refused. Appearance tails remain versioned; public color APIs alone do not prove the serialized nonprimary color or material-field assignments.

**Need.** We must know what the undecoded bytes hold before the writer can edit a retained record or claim that a generated record is complete.
### EV-05. Leading word of a compound-file `ResolvedFeatures` stream

**Question.** What does the u16 that opens `Contents/Config-N-ResolvedFeatures` in a compound-file part hold?

**Known.** `sldprt.md` §2.2: in a block-container part the stream opens with the u32 map count at which `Config-N` ended; in a compound-file part it opens with a u16 and then a class declaration. The compound-file stream continues the `Config-N` map: its first declaration takes the index at which `Config-N` ends. A sketch dimension stores a display scalar (role `1`) and a driving scalar (role `0`) with the same ordered operand cells, and the two values can differ. Each scalar stores its name record, the scalar header, an f64 value in metres, and a trailer that stores the role and the operand cells; each cell stores a u16 tag and a u16 marker address (§2). In the token form, the relation object before the name record stores the relation-class token at prefix +0, a zero u32, and the parameter-class token at prefix +6. A point operand whose cell resolves a marker (§2) has that marker's point as its candidate point. The candidate points of each other point operand are the sketch's entity at the list position that the marker address gives, the coordinate point at that ordinal among the sketch's coordinate points, and each point whose object index or local identifier equals the address. Where no pair of candidate points lies at the dimension's value, an operand whose cell resolves no marker takes every coordinate point of the sketch as a candidate, and an operand whose cell resolves a marker takes the candidates of its marker address and then every coordinate point. The decoder reads every class token as the lane's own, typed by the lane's parameter-class declarations under the order rule of `sldprt.md` §2 and by a relation class declared in place beside the token (`design.parameter-class-assumed`); it finds the two points of a sketch dimension that no declaration claims through its operands' candidate points, gives that dimension the point-to-point class of its relation-class token where exactly one pair of them lies at its driving value, or at its display value where it stores only a display scalar, within the part's linear tolerance. Where the lane declares a relation of that family on the same two points, or such a dimension earlier in lane order forms one, the dimension forms no second relation, and one `sketch.dimension-repeated` note per kept relation names the dimension's scalars and that relation. One `sketch.dimension-class-unknown` note per lane names the scalars of each other such dimension.

**Need.** The compound-file stream does not state the index at which `Config-N` ends, so a reader cannot tell a lane's own class token from a token that names a `Config-N` class, and cannot check a walk of `Config-N` against the stream. Reading every token as the lane's own is an assumption: a lane that writes a `Config-N` class token after one of its own parameter-class declarations gives that token the lane's class. The index at which the stream's map begins, from a field that states it or from a reference in the stream to an object the stream itself declares, settles it.

### EV-06. Named-object header and feature-object block fields

**Question.** What do the named-object header's u32 at +0 and +12, the feature-object block's unnamed u32 fields, its +41 f64, its two version-and-date pairs and its FILETIME hold? What are the bytes that `moEnvFolder_c` and `moDetailCabinet_c` write before the block's +55 string?

**Known.** `sldprt.md` §2.3 gives the offsets and widths of the named-object header and the feature-object block. The +29 and +37 version numbers include values that are `swVersion` values, such as 17000, and values that are not, such as 1195. The +29 version is not consistently smaller or larger than the +37 version.

**Need.** A writer has to fill these fields for a new or changed object, and a reader cannot state an object's provenance without them.

### EV-07. Favorites and selection-set folder bodies

**Question.** What do `moFavoriteFolder_c` and `moSelectionSetFolder_c` write after the feature-object block when they own items?

**Known.** With no items, `moFavoriteFolder_c` writes `00 00 00 00 01 00 00 00` and `moSelectionSetFolder_c` writes `00 00 01 00 00 00`. A folder that owns one item writes `01 00` and then a `moFavoriteFeatItemData_c` or `moSelectionSetData_c` object.

**Need.** A writer that adds a favorite or a selection set has to write the folder's count and the bytes that follow its items.

### EV-08. Reference-plane body fields

**Question.** What do the `moRefPlane_c` u32 at +64 and +136 and the f64 at +112, +120 and +128 hold?

**Known.** `sldprt.md` §2.3 gives their offsets. The +112 value is negative and the +120 value positive. The +128 value is zero or of magnitude below 1e-17. The +136 value is 0, 1, 4 or 5. The +64 value is zero in most planes, and `cd cd cd cd` in some.

**Need.** A writer that creates a reference plane has to fill these fields.

### EV-09. Bytes after a directional light's embedded block

**Question.** What does a `moDirectionLight_c` object write after the feature-object block embedded at its name string +60?

**Known.** `sldprt.md` §2.3 gives the light's bytes through that block. After it come eight zero bytes in files whose `swVersion` is 15000 to 19000, and twelve zero bytes from 11000 to 14000. Six zero bytes, or six zero bytes, a class tag and two zero bytes, also occur at 17000 and 19000.

**Need.** A writer that creates or changes a directional light has to write these bytes.

### EV-10. Feature body head fields

**Question.** What do the fields *h*, *f*, *t*, *g*, *e*, *x* and the *m* u32 values of the feature body head (`sldprt.md` §2.3) mean, and what do the `moPerBodyChooserData_c`, `moCompSolidBody_c` and `moPMarkRecord_c` objects that follow the head store?

**Known.** After *x* `1` comes a u16 that usually equals *m*, then a `moPerBodyChooserData_c` object. After *g* `1` usually come `moCompSolidBody_c` objects; other heads open with a class tag that the object walk does not name, and whether those are solid bodies is open. Every `moRevolution_c`, `moCut_c` and `moRevCut_c` object with *g* `0`, *e* `0` and *x* `1` writes u16 `0`, a reference, u16 `1`, u32 `1`, the same reference and further bytes. `moCombineBodies_c`, `moScale_c` and `moMoveFace_c` write u16 `1` and an object in place after *x*. The *m* values equal the feature-object block +20 value of other objects, which several folders share, so they are not identifiers. *t* is `3` in body-changing features and `0` in reference features, patterns and stored bodies, with `2` in a few objects.

**Need.** A writer that creates or changes a feature has to write these fields.

### EV-11. The bytes around a sketch feature's sketch object

**Question.** What does a `moProfileFeature_c` object write after its `sgSketch` object, and how is an in-place `sgSketch` body laid out up to its first sketch entity record?

**Known.** `sldprt.md` §2.3 gives the ten zero bytes and the `sgSketch` object. An in-place `sgSketch` usually has §2's sketch entity record marker `ff ff 1f 00 03` at its class tag +14, and some have no such marker there. A referenced `sgSketch` is usually followed by `01 00 00 00 01 00 00 00`.

**Need.** A writer has to emit the sketch object's framing and the profile's trailing bytes around the entity records.

### EV-12. Other fillet and chamfer forms

**Question.** What are the layouts of the `Fillet_c` and `Chamfer_c` objects that do not follow `sldprt.md` §2.3, and what do their fields mean?

**Known.** Some fillets write a long body of repeated records after the +54 u32, with `0e 01 00 00` and a class tag in each record. Some write a body shorter than 68 bytes. Some have other values in the first 54 bytes. In a fillet the two u32 at second-table +30 and +64 usually differ by one; in a chamfer the +30 value is usually zero.

**Need.** A writer that creates or changes a fillet or chamfer, including multi-edge and variable forms, has to write these bytes.

### EV-13. Feature folder bytes

**Question.** What does a `moFtrFolder_c` feature folder write after its eight zero bytes and u32 `1`, `1`, `0`, `1` when it does not end with a reference to itself, and how does it store the features it contains?

**Known.** A folder written with a class declaration, usually after a `moNotesAreaFtrFolder_c` folder, writes u32 `1` after those values and then further bytes; such a body that ends at a declaration is 177 to 187 bytes long. Some folders do not start with the reference and u32 values of `sldprt.md` §2.3, and some written with a class tag end with a reference to another object. The folder's contents are not among these bytes.

**Need.** A reader that shows a feature tree's folders, and a writer that groups features into them, have to read and write these bytes.

### EV-14. Persistent identity fields

**Question.** What do the own fields of the identity-representation objects after a reference's first hop, and the reference header's selector and record identifier mean? Which field selects one, two, or three Unicode strings and the 17- or 18-byte gap of a `moExtObject_c` object, how many bytes follow its configuration name in a file whose `swVersion` is 17000 or above, and what are the `moExtObject_c` forms whose u16 after the two string pointers is not `2`?

**Known.** `sldprt.md` §2.3 gives the `moFR_c` feature identifier and the three-string `moExtObject_c` form below `swVersion` 17000, and §2.1 gives the face identity of a reference's first hop. In `moFromSktEnt3IntSurfIdRep_c` the second u32 is usually `ffffffff`. In-place `moExtObject_c` objects hold one, two, or three Unicode strings after the u32, then 17 or 18 bytes before the configuration name; three-string objects occur with both gaps. Where a `moFR_c` object's feature identifier and time stamp follow such an object, 12 bytes separate them from the configuration name, also in files whose `swVersion` is 17000 or above. Hop lists whose class index the object walk cannot name, and identity-representation classes not in the table of `sldprt.md` §2.3, remain unparsed.

**Need.** A reader that resolves a selected face or edge to geometry, and a writer that creates a selection, have to read and write these fields.

**Conflict.** The history object reader requires three strings and skips 16 bytes after the configuration name in files whose `swVersion` is 17000 or above.

**Note.** The SWIFT presentation reader reads every in-place object with the 12-byte tail. It reports `pmi.swift-face-document-layout-assumed` for each display whose face selection depends on a one- or two-string object, and `pmi.swift-face-document-tail-assumed` for each display whose face selection depends on an in-place object in a file whose `swVersion` is 17000 or above or is not declared.

### EV-15. Feature-object block inputs

**Question.** What are the inputs of each feature class's feature-object block, and what are the bytes that follow the inputs of a `moDetailCabinet_c` cabinet?

**Known.** An extrusion's one input is a reference to the sketch feature it extrudes. A cabinet's inputs are its notes folders. In some files the cabinet's inputs are followed by a back-reference to the first input, eight zero bytes, a run of u32 `0` or `1` values and a back-reference to the last input, the same shape as the bytes a `moFtrFolder_c` folder writes of its own (§2.3); in others the last input is a `moFtrFolder_c` folder that writes them. Fourteen zero bytes instead of twelve then end the list in some files.

**Need.** A writer that creates a feature has to write its inputs.

### EV-16. Sketch object fields

**Question.** What do the unnamed fields of the sketch record head, point and curve records, the `sgSketch` settings block and its 61 trailing bytes mean, what are the eight relation groups, how are the compact `ff ff 07 00 01` records laid out, and what does the identifier in the trailer of a sketch's `moCompRefPlane_c` number?

**Known.** In some sketches the dimension and relation groups hold one u16 count fewer than the usual three plus eight, with the same entries otherwise: a polygon that is the last group's entry elsewhere appears one group earlier. The header fields do not tell the two apart, but no group count has bit 15 set, so a reader detects the shorter form by the class tag that stands where the eighth count would be; which group is dropped is not known. The relation groups observed hold one-operand relations (types `4`, `5`, `17`) in the first group and two-operand relations (types `9`, `12`) in later groups. Some sketches write four zero bytes after the eight groups before the first `suObList` pointer. The zero bytes between the last curve record and the dimension count usually number 36 when the last curve list holds lines and 22 when it holds arcs; 34 and 24 occur in the rest. The compact generation occurs in files whose sketches open their records with `ff ff 07 00 01`. In parts of `swVersion` 14000, the plane-reference identifier of a top-level user sketch takes the values `102`, `103`, `104`, `105`, `111` and `139`, and that of the origin sketch takes `101`, `102`, `103`, `104` and `107`. The user sketch's identifier exceeds the origin sketch's. The origin sketch's identifier can equal a body identifier of the part's `moAtom_c` objects; the user sketch's equals none of them, and is smaller than all of them or one past the largest. The u32 after the identifier is `1` or `6` in a user sketch and `0` in an origin sketch; the u32 before the origin sketch trailer's i32 is `4` or `3`. The order of the identifiers of two user sketches of one part is not known. In a sketch's projection of the origin point, the u32 at +4 of its handle's block (`102` to `1324`; equal to its sketch's plane-reference identifier where the block holds version `14000`) and the u32 at +15 of its unit component (`10001`, `10002` or `10021`) are not named.

**Need.** A reader that recovers sketch geometry and constraints, and a writer that creates a sketch, have to read and write these fields.

### EV-17. Extrusion feature fields

**Question.** Where is the item count of a feature dimension's placement block, what are the 76 bytes before an extrusion's `moFromEndSpec_c`, what does the 84-byte bounding-box form's 20-byte tail hold, what do extrusion and ICE objects write after their end specification in two-direction and up-to-surface forms, what are the `moBBoxCenterData_c` forms other than the 70- and 72-byte ones, how is an ICE whose second body chooser and mark record are written without a class tag (by value) laid out, and which faces of the extruded body do a blind extrusion's two chooser face references name?

**Known.** After the end specification come the dimension display objects of the feature's dimensions, an up-to-surface face selection when the end type is `4`, and a `moFromEndSpec_c` object. Some bounding boxes write twelve more bytes before the next object. Some ICE objects write a body chooser's count and face references, and 64 zero bytes in place of a mark record, with no class tag. Chamfers have at least three forms: the one of `swVersion` 15000–16000 is read; in files of 12000 the dimension display is followed by u32 `1`, four empty `CString` values and f64 values, and in files of 17000 and 19000 an object follows the first `suObList` at once. A `moSketchBlockInst_c` writes, after its block definition, a 16-byte identifier, u16 `0x68`, u16 `11`, eight zero bytes, a `moDetailFolder_c`, transforms and `moAbsolutePoint_c` objects. A silhouette edge's backed-up data (u32 `9`, f64 π, …, the handle block, a `moArcBackedUpData_c` and the name `Silhouette Edge`) is not read. A blind extrusion's `moPerBodyChooserData_c` in parts of `swVersion` 14000 holds two face references. Where the extrusion extrudes one circle, each reference holds three hops: a `moEndFaceSurfIdRep_c` (end `0` in the first reference, `1` in the second; region `0`), a `moFromSktEntSurfIdRep_c` naming the circle's curve identifier, and a null pointer; the references of other profiles hold five, six or more hops. The `moBBoxCenterData_c` after a chooser holds a point, a non-zero length and a non-zero identifier; the point's component along the sketch normal can be half the depth.

**Need.** A reader that recovers an extrusion's depth, direction and bodies, and a writer that creates an extrusion, have to read and write these fields.

### EV-18. In-context reference fields

**Question.** What do the 64 zero bytes before an in-context reference's context component, the ten bytes a face group writes after its compressed bodies, the six f64 of a group chain, the u16 group count of the sketch in-context block, the ten zero bytes and four f64 of a face support, and the frame data after a `moFaceRefPlnData_c` face reference hold?

**Known.** The context component's two forms are distinguished by whether its identifier is repeated; form A carries sixteen `ff` bytes and form B two transform blocks. Each compressed body inflates to a Parasolid transmit file, the backed-up geometry of the support. A `moFaceRefPlnData_c` face reference is followed by `00 00`, u16 `1`, a context component, twelve zero bytes, i32 `-1` and a run of little-endian f64 values whose layout has not been decoded.

The face geometry after a `moFaceRefPlnData_c` in an in-context part has two observed forms. In one, an 80-byte head (six f64, an f64, three f64), a transform block at +80, a flag-`0` triple, 22 bytes, a second transform block and a 65-byte tail ending `04 00 00 00 ff ff ff ff 00 00 00 00`, a u32 identifier, a u32 and eight zero bytes (402 bytes). In the other it opens with the 58-byte handle block's fields (three i32 `-12345`, the version), twelve zero bytes, i32 `-1` and four f64 (`-1`, `1`, `1`, `-1`) (370 bytes). Lengths of 210, 298, 364, 392, 436, 442 and 474 also occur.

**Need.** A reader that keeps a part's in-context references, and a writer that preserves them, have to read and write these fields.

### EV-19. Configuration object stream

**Question.** What is the grammar of a `moRelMgr_c` object's detailing settings after its ten dimension styles, how are `moCThreadRefMgr_c` records and the untagged thread data laid out, how does a relation whose six bytes after its text are all zero store its equation sides, and what are the `Config-N` layouts of files below `swVersion` 14000?

**Known.** A per-version template of gap lengths and string positions reads the detailing settings of the versions it covers. In a part of `swVersion` 14000 the template holds `CString` values, the unit names and `moLengthUserUnits_c` objects among its byte runs; the count and lengths of its runs and strings differ between parts. Thread data with records writes 17-byte entries (u32, u32, a byte, f64), then f64 geometry of varying length, two `CString` values and three matrix rows; some streams stop there, others stop at equations, and no stream of 10000–12000 reaches its end, for want of templates for those versions. A relation whose six bytes after its text are all zero opens its first side with the same side class or token as the other relations and stores fewer tokens; a global-variable token's character range differs in length from its text. Its dimension reference is the relation-dimension class tag, six zero bytes, and the counted name, with no wrapper and no value.

**Need.** A writer that creates a part has to write the configuration stream, and a reader that keeps detailing settings, threads and equations has to read it.

### EV-20. Favourite handle tail

**Question.** Which field of a `moFavoriteHandle_c` decides the length of the bytes after its last `CString`?

**Known.** The tail is a u32 flag word and 50 bytes in most handles; 85 bytes after the flag when bit 16 is set, with u32 `0` and a `CString` more when bit 0 is also set; 52 bytes and a `CString` and 18 bytes more when bit 7 is set. It is absent after a pattern-count dimension's handle and when an object follows at once, and can be 28 bytes before a reference object. Before an extrusion's 76-byte block it is zero bytes: usually 54, 50 at `swVersion` 14000, and 52 in some handles of `swVersion` 16000 and 19000. At `swVersion` 15000 and above, no byte of the handle's first 382 bytes separates the 52 from the 54, so a reader finds the 76-byte block after a run of zero bytes. The 122 or 194 bytes of f64 values seen after handles with flag bits 11 and 15 set are the handle's own view block (resolved 2026-10-21; §2.3). Before a sketch's in-context block the tail can be 52 bytes, told by the list that follows: a chain list, or a group header whose group data a reader would otherwise take for a chain count.

**Need.** A writer has to write the tail a handle's reader expects; a wrong length shifts the object index count for the rest of the stream.

### EV-21. Thread feature fields

**Question.** What do the unnamed fields of a `moSweepThread_c` mean: the two f64 after the edge's handle block, the 40 bytes before u32 `1`, u32 `1`, and the f64 groups of the cylinder and surface references?

**Known.** Some streams that hold a thread parse to their end; the start edge's point, axis and length, the pitch, the size and the library path are identified. The cylinder geometry is eleven f64 and the surface geometry thirteen f64 and a u32. A face-reference slot holding a `moCircleRef_w` and a 40-byte gap holding non-zero values are not read; at `swVersion` 17000 the 74 bytes before the closing objects can hold u32 `1`, `0`, `1` at +8 in place of u32 `1`, `1` at +40, read by length.

**Need.** A writer that creates a cosmetic or modelled thread has to write these fields; a reader that keeps thread data has to read them.

### EV-22. Sketch text fields and in-context plane frames

**Question.** What do the unnamed fields of sketch text mean (the character format's other slots and four f64, the u32 pair before the character count, the six f64 of text on a path), and what do the two frames of a plane in an assembly's context, and the 47 bytes between them, distinguish?

**Known.** Text records, their character format and outlines are read to their end (§2.3), including text on a path. The outlines are the rendered glyphs, so a writer that changes the text has to regenerate them from the font. An in-context plane's two frames are equal, and the bytes between them are 31 zero bytes, `01` and 15 zero bytes.

**Need.** A writer that keeps a sketch's text, or a sketch placed on a face of another part of an assembly, has to write these records.

### EV-23. Sheet-metal bends and single-design forms

**Question.** What is the layout of a sheet-metal bend (`moOneBend_c`, the children of a `moEdgeFlange_c`), and of these forms: a polygon relation entry whose head differs from the documented one; a contour chooser whose tail selects sketch entities (a count, handles and identifiers `100000` + *k*); a spline tangent-magnitude dimension whose operands are data; the six f64 and u32 `0` that end one circular pattern at its stream's end; the 168 bytes that end a `moFlatPattern_c`; and the rest of a mirrored part (`moMirrorStock_c`: a `moBodyRef_w` whose `moCompBody_c` holds a context unit component, fields, a mirror frame and the mirrored body's compressed data, then further references and the mirror plane) and of an add-in's custom feature (`moMacroFeature_c`); the geometry of a `moCompRefAxis_c` (105 bytes, framed); the object index that some parts count after the first `moSweepSideSurfIdRep_c` hop (a reader's count falls one short there, and nothing in the bytes shows the extra object); and a sketch's `sgBlock` (its layout is stated in §2.3; open is one block of 720 triples whose sequence after the triples differs)?

**Known.** A bend is a named feature, then 122 bytes, its radius display, a `moCompFeature_c`, 33 bytes, its `moCompEdge_c` and its display objects, then a block. One block form is 215 or 287 bytes long. It opens with u32 `0` (the last display's flag word), u32 `8`, u32 `0x01000000`, u32 `0`, u16 `0x0100`, eight bytes that repeat in its middle (such as `20 5d 67 ce ce 72 ef bc`) and a frame of 3, 6 or 12 f64; its last 67 bytes are always f64 `1`, u8 `0`, eight `ff` bytes, u32 `1`, u32 `1`, thirty zero bytes and twelve `ff` bytes. Most streams write 34 further bytes before those: f64 `1`, three f64 `0` and u16 `0`. A reader frames that form by the next named object: a class tag, the name object's class tag and a Unicode `CString`. The other form opens `00 00 80 00 08 00 00 00`; it is 608 bytes and ends at the next object (such as a `moScalerDim_c`). A sheet-metal edge flange (`moEdgeFlange_c`) writes its counted children, then a display object, then a block that a reader frames by the next named object (such as a bend). A display dimension parsed at top level writes a view block after its favourite handle, opening u32 `0`, u32 `8`, u32 `0x01000000`, u32 `0`, u16 `0x0100` and the repeated eight-byte identifier; it ends at the next display object (such as 472 bytes). The same block after a display inside a feature belongs to that feature. A top-level display dimension without that view block writes placement data before the next object: an empty Unicode `CString`, u32 values and zero bytes, all within 64 bytes of its end; without a `CString`, f64 and u32 values end at the next display object within 128 bytes.

**Need.** A writer that creates sheet-metal flanges has to write bends; a reader that keeps them has to read them.

## 7. Assembly documents

### AS-01. Mate-list record layout

**Question.** What is the layout of the `moMate*` records in `Contents/Config-N-MatesList` (`sldprt.md` §11.3), and how does each operand bind to its component instance and face?

**Known.** The section holds mate class tokens, mate feature names, instance names `<instance name>@<assembly name>`, the referenced documents' paths, and `SurfIdRep` face references. No field offsets are known.

**Need.** We must know the layout to transfer a mate as an assembly joint. Without it, the mates transfer only as occurrence placements.

### AS-02. Component placements of a compound-file assembly

**Question.** Where does a compound-file assembly (dialect `sw-version-pre-12000`) store its component instances and their placements?

**Known.** The compound file has `Contents/Config-0-MatesList` and no `swXmlContents` sections, so no `COMPINSTANCETREE`. `Contents/Config-0` is the candidate.

**Need.** We must know the records to transfer the occurrence tree of such an assembly. Without them, the occurrence structure does not transfer.

### AS-03. Stored placement of path-driven components

**Question.** Is the stored `swTransform` of a component that a path or chain mate drives the solved position of that component?

**Known.** Every leaf instance of a chain sub-assembly can lie 0.69 mm (median) to 85 mm from the nearest instance of the STEP export of the same assembly, while the instances of the other sub-assemblies agree to 3e-11 mm. Instances under a sub-assembly can also agree with neither the parent-relative nor the root-relative composition of §11.2. The export state of such STEP exports is not known.

**Need.** We must know whether such placements need a solve before a consumer can use them as positions.

### AS-04. Parasolid streams of an assembly document

**Question.** What do the Parasolid streams in the `Contents/Config-N-ResolvedFeatures`, `Contents/Config-N-MatesList`, and `Contents/Config-N-GhostPartition` sections of an assembly document hold?

**Known.** The part decode of such a document gives no body. The streams are not in embedded documents (`sldprt.md` §11.4). Assembly features (cuts, holes) and the cached geometry of mate faces are the candidates.

**Need.** We must know their content to transfer it or to state that it is not model geometry. Without it, the document keeps the part losses that report an untransferred Parasolid model.

### EV-25. Turn sense of a partial revolution

**Question.** Is the turn of a partial `moRevolution_c` or `moRevCut_c` whose reverse byte is `0` right-handed about the axis direction its record stores, or about another direction that agrees with it in the stored forms below, such as the drawn direction of the sketch line on the axis?

**Known.** The revolve body's turn block (`sldprt.md` §2.3) holds the reverse byte at +2, `0` or `1`. The tail of the record's `moLineRef_w` stores a point on the axis and a unit axis direction (`sldprt.md` §2). These stored forms show the turn:

- The axis passes through the origin with stored direction (0, 1, 0). The profile is a 10 mm by 10 mm rectangle on the plane z = 0, at x from 10 to 20 mm and y from 0 to 10 mm, and a construction line of the sketch runs from (0, 0) to (0, 64.77) mm. The angle is 90° on one side. With reverse byte `0` the stored solid lies at z from -20 to 0 mm. With reverse byte `1` it lies at z from 0 to 20 mm.
- The axis passes through (0, 0, -10) mm with stored direction (0, 0, 1). The profile is a half disc of radius 10 mm on the plane y = 0 at x from -10 to 0 mm, and its straight edge runs along the axis from z = -10 to z = 10 mm. The angle is 180° on one side and the reverse byte is `1`. The stored solid lies at y from 0 to 10 mm.

In the first form, reverse byte `0` gives the right-handed turn about the stored direction and reverse byte `1` the opposite turn. In the second form, reverse byte `1` gives the left-handed turn about the stored direction. In both forms, the sketch line on the axis runs along the stored direction. The decoder searches the revolve's object interval for the turn block and its followers, and reads the reverse byte only when exactly one offset matches. It accepts the reference after the block as a `suObList` declaration or as any class token, because such a token can name a class that `Contents/Config-N` declares (`sldprt.md` §2.2, open item EV-05). The decoder writes the stored direction to `RevolutionAxis.direction`, reversed for reverse byte `1`, and charges `design.revolve-sense-assumed` for each revolve that turns less than a full turn on one side or turns on two sides.

**Need.** Save one profile revolved 90° on one side about a sketch centreline, once with the centreline drawn in the direction of the axis and once against it, both with reverse byte `0`. Compare the stored axis direction, the reverse byte and the side of the stored solid in the two records.

**Note.** A full turn and a mid-plane angle sweep the same region in either sense, so they do not answer this question.

### EV-27. Sketch curve records with selector bit 0 set or on the geometry locus

**Question.** What do the endpoint and centre indices of a sketch curve record select when bit 0 of its selector (+37) is set, and when bit 0 is clear and its locus (+23) is `05 00 01 00`?

**Known.** The selector takes the values `4`, `5`, `12`, `13`, `68`, `69`, `132` and `133`. Selectors `68` and `132` mark the compact legacy link-bearing forms of `sldprt.md` §2. With bit 0 clear and a locus other than `05 00 01 00`, the indices select point records of the same `sgSketch` object (`sldprt.md` §2.3 "In a curve record whose selector (+37) has bit 0 clear"). With bit 0 clear and locus `05 00 01 00`, the current-prefix 92-byte selector-4 line that `sldprt.md` §2 reads takes each endpoint value plus one as the object index of a point marker. The decoder reads the indices of the other such records as point-record positions and charges `sketch.point-record-index-basis-assumed`. With bit 0 set, the centre index of a full circle can select the centre of a cylindrical face of the stored solid while its endpoint index selects a point record that is not on that face. In a rectangular profile, a 92-byte line record with an identity trailer at +88 reads as the bottom line under the one-based ordinal rule and as the top line under zero-based endpoint indices; the stored solid has a top face and the decoded profile has none.

**Need.** The field or list that gives such a record's curve, checked against the faces its feature produces.

### EV-30. Sketch curve records of curve type 2

**Question.** What does curve type `2` select in an `sgSketch` curve record, and how does the decoder type its geometry?

**Known.** `sldprt.md` §2.3 states the curve types `0`, `0x40`, `1` and `0xffff`. Records of curve type `2` occur, and each uses the legacy record layout. The decoder keeps the line another pass drew on such a marker, and charges `sketch.arc-record-conflict` with the reason that names the unknown sense word.

**Need.** For each curve-type-2 record, its point records and fields against the B-rep edges of the feature that consumes the sketch.

### EV-31. Arc records of a sketch whose marker transform is not a similarity

**Question.** Which placement of a sketch's point records makes its arc records end on their endpoint point records when the marker transform of the sketch scales one sketch axis more than the other?

**Known.** The decoder places the point records of a framed `sgSketch` object through the marker transform of its sketch (`sldprt.md` §2, "A marker whose record is a point record or a curve record"). When that transform scales the two sketch axes differently, an arc record's arc, placed through it, ends off one of its endpoint point records by more than the linear resolution. The decoder gives such a record no geometry, keeps the line or handle that the other §2 rules give its marker, and charges `sketch.arc-record-conflict`. A marker that the §2 layouts skip has no other reading: it gives no entity, and the decoder charges `sketch.marker-record-unread`. A profile that runs through the arc stays open.

**Need.** The frame of such a sketch's point records, checked against the circular edges and side faces of the feature that consumes the sketch.

### EV-32. The state of a sketch curve record

**Question.** What do states `0` and `2` of an `sgSketch` curve record (u16 at +29) select, and which curves of state `0` belong to the profile of their sketch?

**Known.** A curve record of state `0` has u32 `0` after its endpoint indices, and a curve record of state `1` or `2` has u32 `1` there (`sldprt.md` §2.3 "In a curve record whose selector (+37) has bit 0 clear"). Every construction curve has state `0`. State `2` occurs in one sketch saved at three `swVersion` values, schema `07 00`: four lines and three arcs, which together form the sketch's one closed loop. Most non-construction curves of state `0` lie off the stored solid: an outer rectangle around a profile of state `1`, or a second record of a curve that a record of state `1` already gives. In one sketch, twelve lines of state `0` each run along a straight edge of the solid from end to end, and each is the only curve that joins two arcs of state `1`. The decoder reads a curve record of state `0` as a curve outside the profile, which joins no profile loop, and reads a curve record of state `1` or `2` as a curve of the profile. The design states a curve of state `0` as construction and charges `design.sketch-construction-assumed` where the record's construction bit is clear, except in a sketch where a curve of state `1` or `2` closes a loop only with curves of state `0`: there every curve of state `0` stays profile geometry. The profile loops the decoder resolves keep the curves of state `0` out in these sketches too, so there the extrude's `Sketch` pick closes regions the loops do not hold; read with every curve of state `0` of the sketch, the loop rule closes fewer loops or none.

**Need.** For each curve record of state `0` or `2`, its curve against the edges and faces of the feature that consumes its sketch, and the fields of its record that differ from those of a curve record of state `1`.

### EV-33. The four identifiers below an extrusion

**Question.** Which objects hold the four identifiers between an extrusion's depth parameter and the extrusion?

**Known.** `sldprt.md` §2.3 gives the identifier order of a sketch and its extrusion: the sketch, its dimension parameters, the extrusion's depth parameter, four identifiers and the extrusion. No `KeyWords` record, named-object header or parameter trailer that the decoder reads holds the four identifiers. The extrusion's objects after its end specification include its dimension display objects, a body chooser and a `moFromEndSpec_c` (EV-17).

**Need.** The class of each object that holds one of the four identifiers, and the field that holds it, so that a writer that creates an extrusion numbers its objects as native parts do and a feature added to a part takes none of the four.

**Conflict.** The writer numbers a generated part's user features first, from `23`, and their dimension parameters after them, the extrusion's depth parameter last. It writes the extrusion one above its sketch and nothing at the four identifiers. A feature added to a part takes an identifier past the largest that the decoder reads in the part, so it takes none of the four.

### EV-34. The light set of the default tree

**Question.** Which `moDirectionLight_c` objects and which `moEqnFolder_c` identifier does a part hold when its `moEnvFolder_c` holds fewer than three directional lights?

**Known.** `sldprt.md` §2.3 gives the two identifier sets of the default nodes: directional lights `13`, `14` and `15` with the `moEqnFolder_c` `16`, or those lights and a fourth `16` with the `moEqnFolder_c` `61`. Parts of other versions that hold exactly two directional lights, at `13` and `14`, hold the `moEqnFolder_c` at an identifier other than `16`, some at `15`. Parts that hold the `moAmbientLight_c` at `12` and the `moEqnFolder_c` at `16` with `15` empty hold their directional lights at other identifiers.

**Need.** The light set and `moEqnFolder_c` identifier a writer gives a generated part, so that its default tree is one native parts hold.

**Conflict.** The writer writes two directional lights, `13` and `14`, and the `moEqnFolder_c` at `16`, and leaves `15` empty. No native part holds that combination. The first identifier set with a third directional light at `15` is a combination native parts hold.

### EV-35. The next object identifier after an edit

**Question.** What does a save that adds or deletes a node write to the creation log and to the next object identifier of `Contents/Config-N-ModelHeader` and `Header2`?

**Known.** `sldprt.md` §2.3 gives the creation log, a `suObList` of `moLogs_c` records in increasing identifier order, and after it a u32 time stamp and the u32 next object identifier (`model_header_log_tail`). A `moLogs_c` record holds `moStamp_c` objects of kind `0` (created), `1` (modified) and `2` (deleted). The next object identifier is one above the largest `moLogs_c` identifier, or larger. A parameter object's trailer can hold an identifier at or above the next object identifier (EV-36).

**Need.** Two saves of one part, before and after a feature is added, and two before and after a feature is deleted, with their creation logs and next object identifiers compared: the `moLogs_c` record and stamps of the added or deleted feature, the time stamps, and the change of the next object identifier.

**Conflict.** The writer of an edited part keeps the source part's `ModelHeader` and `Header2` streams, so an added feature has no `moLogs_c` record and the next object identifier does not change. The writer gives an added feature an identifier past every identifier the decoder reads in the part's `KeyWords` records, named-object headers and parameter objects, and it does not read the next object identifier. The decoder reads the identifier of a parameter object with a tolerance or an integer value only from the object walk of the stream, and the walk reads no identifier from a feature-dimension parameter that writes its `moDimTolerance_c`. The writer counts no identifier from a parameter the walk gives none for: when that parameter holds the part's largest identifier, the added feature takes the same identifier. A part the writer generates holds no `ModelHeader` or `Header2` stream.

### EV-36. Parameter identifiers that other records hold

**Question.** What does the object identifier in a parameter object's trailer name when a `KeyWords` record or another parameter object of the part holds the same identifier?

**Known.** `sldprt.md` §2.3 gives the parameter trailer: a zero byte, the tolerance pointer and the u32 object identifier. Parameter objects can hold identifiers that `KeyWords` records also hold, among them the `moDirectionLight_c` identifiers `13` to `16`, and one identifier can be held by many parameter objects of one part. The parameter objects that hold these identifiers form a block whose first identifier is `13` or `14` and whose last is between `37` and `53`. Several parameter objects of the block can hold one identifier, and the values the block's parameter objects hold differ between parts. An identifier of the block can be at or above the part's next object identifier (EV-35).

**Need.** A part that holds such a block and a save of the same part without the feature or library object that brings the block, with their parameter objects, `KeyWords` records and next object identifiers compared.

**Conflict.** The writer counts every parameter identifier the decoder reads among the part's identifiers when it numbers an added feature, so the block can raise the added feature's identifier above the next object identifier.

### EV-37. The layout of the first identifiers by version

**Question.** What decides which of the two layouts of the first identifiers (`sldprt.md` §2.3) a part of a version other than 14000 holds?

**Known.** In the first layout `1` is the `moDetailCabinet_c`, `2`, `3` and `4` the Front, Top and Right planes and `5` the origin. In the second `2` is the `moEnvFolder_c`, `3`, `4` and `5` the planes and `6` the origin; its origin projection names feature `6` and a `moCompRefPlane_c` names the Front plane by feature `3` with plane word `3`. Parts of `swVersion` 14000 hold the first layout. Parts of other versions from 8000 to 16000 hold either layout.

**Need.** Two parts of one version from 8000 to 16000 that hold different layouts, with their creation logs (EV-35) compared: the version and stamps of each part's first `moLogs_c` records.

**Conflict.** The writer writes `swVersion` 14000 and the first layout.

### EV-38. Side-face identifiers of non-extruded producers and of unnamed profile lines

**Question.** Which profile line sweeps a side face whose local identifier no `+84` value of the profile names, and what do the entries of a blend edge's component path name for a producer other than an extrusion?

**Known.** `sldprt.md` §5 gives the join from the local identifier of an extrusion side face to its profile line, and the order of a component path's entries for an extrusion. A line record whose u32 at marker +80 is nonzero stores zero at +84 in two profiles. A side identifier of 1 cannot be told from the §2 direct-form selector, which stores u32 `1` at +84 with a nonzero identity at +88; the join states no face from such a record. In a seven-line profile, the closing line record has `00 00 02 00` at +80 and stores `1` at +84. In a rectangular profile, two of the four line records are line handles whose geometry is not read (EV-27), so the join states no sketch entity for their faces.

**Need.** The identifier that a record with nonzero `+80` gives the next line; whether the closing record's `+84` names the first line's face where it is nonzero; and the entry order and entities of a component path for producers other than an extrusion.

### EV-39. Partial turns of a circular feature pattern

**Question.** How does a `moCirPattern_c` whose angle is less than a full turn place its instances, and what does the byte after its axis tail select?

**Known.** The byte follows the circular pattern's axis tail (`sldprt.md` §2.3) and takes the values `0` and `1`. With a full turn the instances lie evenly around the axis for both values. A pattern whose listed angle dimension is `D3` holds the full turn. A pattern whose listed angle dimension is `D2`, with value 90° and 4 instances, places its copies 90° apart, so its angle is the step between neighbours. The decoder types only full-turn circular patterns from the walked object.

**Need.** Save one profile cut patterned circularly 4 times over 90°, once with equal spacing and once with the instance spacing, each with the pattern direction reversed and not reversed. Compare the angle parameter's name and value, the byte after the axis tail, and the angles of the copies in the stored solid.

### EV-24. Bytes after the last top-level object

**Question.** What owns the four zero bytes some `ResolvedFeatures` streams write after their last top-level object, and the two zero bytes an in-context part writes after a top-level `moProfileFeature_c` whose sketch plane is written in place?

**Known.** `sldprt.md` §2.2 gives the stream framing. The trailing bytes lie outside every top-level object. The specification gives no field that names them.

**Need.** A writer that reemits the stream has to know whether to write them.

### GC-31. Mesh facet payload and ownership

**Question.** How do the mesh receive index and auxiliary partition data select complete vertex and triangle arrays for each mesh-support face?

**Known.** `parasolid.md` §3 defines the type-201 MESH field list and the reciprocal MESH/FACE link through SHELL to BODY. A MESH node can carry null `psm_imesh`, `pff_imesh`, and `rcv_key` pointers with a positive `rcv_index`. The same partition section can contain a separate `: MESH FILE` stream with type-189 PSM_MESH nodes, type-191 integer combs, type-193 vector combs, and their typed teeth. PSM_MESH owner pointers can be null. Its pool and index fields link to comb records; comb teeth arrays link to typed value arrays. The IR admits indexed triangle tessellations with body and face references, and polygonal support surfaces with a chordal-deflection bound. The decoder reports the distinct mesh-body count with `sldprt/geometry.mesh-body-not-transferred` and retains the source image.

**Need.** Establish the cross-stream receive-index mapping, precision and comb encodings, coordinate units, triangle grouping, orientation, and complete per-body ownership. Check each reconstructed mesh against independent body areas and volumes before transfer.

### SF-01. Remaining principal component reference forms and profile coordinates

**Question.** Which placement and coordinate convention applies to mixed Top and Right component records, principal component records beside another declared or component record of the profile's object or only in the preceding object, enclosing-context component records, lane component references, block definitions, and nested profile streams?

**Known.** `sldprt.md` §3.3 gives the Top and Right placement and the sketch coordinates of a marker or compact profile whose own object holds a principal-plane component record. A declared Top reference takes the Top frame of `sldprt.md` §2. A profile whose own object holds a principal-plane component record takes the plane that record names (`sldprt.md` §2), and that plane supports it. The decoder places the hole positions that a profile gives on the frame of its sketch. For a Front reference whose source identifier is `2` and resolves to the Front frame of `sldprt.md` §2, and whose profile holds principal-plane component records that do not name its plane by the rule of `sldprt.md` §2, or whose profile is a sketch block definition that holds principal-plane component records in its own object, the decoder keeps the Front lane fallback frame, N=-Y and U=-Z, and charges `sketch.plane-binding-assumed`; a Front reference with feature number `3`, in parts that number the principal planes from `3`, keeps the Front frame; the principal planes store the frames of `sldprt.md` §2. Where a profile object that holds a principal-plane component record also holds declared or component records of its own that give one source other than its declared Front record, or a component frame record lies from the start of the preceding object through the end of the profile, the decoder keeps that source or frame. A Right principal-plane component record that lies only in the preceding object keeps the binding of that object's reference. On the fallback frame the local reading of the marker pair and the model-component reading (`sldprt.md` §2, open item DI-80) each place about as many sketches on B-rep vertices. A profile with Right component records can have curves that lie on edges of the final solid only under the mirrored Right mapping, or under none of the principal mappings. Nested profile streams carry their own coordinate records.

**Need.** Determine the byte fields that bind each remaining reference form to its profile coordinates and normal direction. Establish each complete rule with independent profile and mirrored-polarity controls. For Right component records, a mirrored-polarity control separates the §3.3 Right mapping from its mirror, and the comparison separates finite edge support from intersections with carrier extensions.

### SF-03. Sign of the decoded sketch normal off the principal planes

**Question.** Which fields set the sign of a sketch normal, on a datum plane, on a plane off the model origin, and on an axis-aligned plane through the origin?

**Known.** The end-spec reversal flag gives the travel of a one-direction extrusion that adds material relative to the decoded sketch normal (`sldprt.md` §2). Of the one-sided joins whose solid spans exactly their depth, the solid agrees with that rule in 428 of 433 where the sketch plane contains the model origin, and in 13 of 20 where it does not. Of the 12 joins that disagree, 3 have a datum plane on the sketch plane that faces opposite the decoded normal, 7 have a sketch plane off the origin, and 2 have neither. Where the stored flag is `1`, the solid lies against the decoded normal in 89 of 135 one-sided joins; where it is `0`, the solid lies along it in 493 of 532. Of the 45 flag-`1` one-sided joins whose whole solid lies along the decoded normal, 24 have a sketch plane off the model origin and 21 an axis-aligned sketch plane through it. Of those 21, 4 have a datum plane on the sketch plane that faces opposite the decoded normal, and 15 have no datum plane on the sketch plane.

**Need.** A decoded normal of the wrong sign reverses the travel of each extrusion on that sketch. Sketches on datum planes of both orientations and sketches off the origin must establish the field that sets the sign.

**Note.** Where the solid contradicts a decoded reversal, the decoder keeps the travel that the solid shows (`feature.extrude-reversal-contradicted`). Where the solid neither holds nor contradicts a decoded reversal, the decoder keeps the reversal and names it (`feature.extrude-reversal-assumed`). A configuration state reads the flag of its own lane. The decoder takes the decoded B-rep as the solid of the active configuration (the active solid). Where the model feature travels against the normal, the state of the active configuration follows the model feature. The decoder tests each other reversed state of the active configuration against the active solid. An inactive state whose definition equals the model feature's or the active state's takes the active solid's verdict on that feature or state. Where the active solid contradicts the reversal of an extrusion, the reversed state of that extrusion travels along the normal in every configuration, and the decoder names each such state that the model feature's note does not name, with its configuration (`feature.extrude-reversal-contradicted`). For this fallback the decoder takes the sketch normal of the extrusion as the same in every configuration. Each other reversed configuration state rests on its flag, and the decoder names it with its configuration (`feature.extrude-reversal-assumed`). A write that clears a model reversal writes `0` to the flag in every lane, and names each lane outside the active configuration whose flag it clears (`writer.extrude-reversal-configuration-cleared`). To decide whether an edit changed the lanes, a write reads the configuration states back from the lanes and settles them by the same rules against the same solid, recovering each model extrusion's reading before the settlement from the direction the decoder left. The decoder does not test the opposite case: 39 joins with flag `0` whose whole solid lies against the normal keep travel along it.

### DI-69. Extrusion Boolean carriers

**Question.** Which complete feature-input carriers select the Boolean operation of the remaining extrusion forms?

**Known.** A withheld operation is a sheet extrusion, an extended-reference continuation, a terminated header, or a compact join class binding. Sheet extrusions are the most frequent. The saved configuration states of these features have unresolved operations.

**Need.** Establish the class-scoped operation carrier and the configuration-local state against producing-face orientation and body ownership.

**Note.** Withheld operations retain their existing losses.

### DI-71. Profile chains of marker sketches with dimensioned circles

**Question.** Which feature-input records give the complete closed curve set of a marker-backed sketch whose only closed curve is a dimensioned circle?

**Known.** A circular dimension on a point handle projects one circle with the dimension radius, centered on the handle. The sketch then has no profile chain, and an extrusion that names its sketch feature keeps the feature reference. Extrusion and revolution features both name such sketches. When an extrusion's sketch has exactly one circle, the extrusion usually has a cylinder face that is coaxial with the circle and has its radius; in the others, no cylinder face has both the axis and the radius. Other coordinate handles of the same sketch can lie at radii of further coaxial cylinder faces.

**Need.** Establish the records that give every circle and curve of these sketches, so that a profile chain is admitted only when the sketch curve set is complete. Prove each chain against the produced side faces.

**Conflict.** One dimensioned circle can be one of several concentric profile circles. A whole-sketch profile from that circle alone fills the region inside the outer circle.

### DI-83. Circles built from circular-dimension records

**Question.** Which curve record does an `sgCircleDim` dimension measure, and when is its handle marker the curve's center and when a point of the curve?

**Known.** An `sgCircleDim` relation record stores the dimension's value and its operands, and no curve (`sldprt.md` §2, circular dimensions). Its handle marker is the measured curve's center or a point of that curve: an end of an arc, or a point on a circle. Where the handle resolves through a unique coordinate transform and no circle of its sketch equals the one the record describes, the decoder builds a full circle centered at the transformed handle coordinate with the dimension's radius, adds it outside every selected profile chain, and makes the relation record its geometry carrier. Where the handle was the measured curve's center, the built circle has the center and radius of that stored circle or arc within the linear resolution: the decoder's equality test compares circles only, and more tightly. Where the handle was a point of the curve, the built circle's center lies on a stored curve of its radius, one radius from that curve's center. Where the operand resolves to a stored arc's center marker, the decoder's carrier choice can replace it with an arc-end marker. One dimension record can build two circles from two markers; the dimension names one of them.

The design reads a built circle in this order. A built circle that has the center and radius of stored circles or arcs of its sketch within the linear resolution, all on one carrier, is left out, and the dimensions that named it, radial or native with a value, name the first of those curves in record order; where another relation names it, it is construction. A built circle on the carrier of an earlier built circle that the design does not read on a stored curve is construction, or left out where no relation names it, before any cylinder face is read, so the profile holds that circle once. A built circle that a cylinder face of its radius witnesses, on an axis along the sketch normal through its center, is kept. Where its center lies on stored curves of its radius that share one carrier, the design leaves it out only where every relation that names it is a radial dimension, and those name the first of those curves; a native distance measures from the built center, one radius from that curve's center, so it keeps the circle as construction. A built circle that no relation names is left out. Any other built circle is construction. The record does not say which of several curves on one carrier a dimension measures. `design.sketch-dimension-circle` names each circle the design leaves out or states construction, with its reading, what the design does with it, and where it took the first of several curves.

**Need.** The operand-to-marker resolution of `sgCircleDim` where the resolved marker lies one radius from a stored curve's center, compared with the curve record that the solid confirms, and what in the record tells a handle at the center from a handle on the curve.

**Conflict.** A cylinder face witnesses a circle whichever feature cut it: a built circle that a later feature's hole repeats stays profile geometry. The witness reads cylinders along the sketch normal, so a built circle that a revolution sweeps has none.

### DI-72. Length unit codes above `3`

**Question.** Which length units do the `moLengthUserUnits_c` unit codes above `3` name?

**Known.** `sldprt.md` §2.3 gives the codes `0` to `3`. The decoder states no length unit for a document that writes another code.

**Need.** A document that writes a code above `3` and stores a length scalar beside its Keywords display, so that their quotient names the unit.

### DI-87. The base form of each unit object of one units table

**Question.** Does every unit object of a `moUnitsTable_c` open with a base of the form its `moNumberUserUnits_c` object has?

**Known.** `sldprt.md` §2.3 gives the three base forms: 44 and 48 bytes in a compound document, 62 bytes in a block-container document. The decoder measures the `moNumberUserUnits_c` object, from the end of its class declaration to the next class declaration, and reads the `moLengthUserUnits_c` display decimal places and u16 unit code at that form's offsets. A 48-byte length unit base read at the 44-byte offsets gives as its unit code the low u16 of its last u32, which is `0` or `1`.

**Need.** A compound document whose `moLengthUserUnits_c` base and `moNumberUserUnits_c` object differ in length, or a field of the length unit object that states its own base form.

### DI-88. The end of a units table

**Question.** At which byte does the last unit object of a `moUnitsTable_c` end?

**Known.** The table writes a u16 count (12, 16 or 17) and that many unit objects. A unit object is its class declaration or a reference to an earlier one, a base, and what its class adds (`sldprt.md` §2.3). `moLengthUserUnits_c` adds an object pointer to a dual length unit, which the table writes in place as a second length unit object, and `moDensityUnits_c` adds a length-unit pointer when its flag byte is set. The decoder searches from the count to the end of the configuration stream for the first `moLengthUserUnits_c` and `moNumberUserUnits_c` declarations.

**Need.** The encoding of a null and of an in-place object pointer in each unit class that writes one, so that a walk of the table's count of objects ends at the table's last byte and bounds the search.

### DI-77. The +17 value of a sketch curve record

**Question.** What does the little-endian u32 at +17 of a curve record in an `sgSketch` curve list hold? Which construction state does selector `12` declare for a full circle with role u16 `1` at +27?

**Known.** For current-layout list curves with selector `4` or `12`, `framed_curve` decides type `0` as a line. Type `1` followed by u16 `0`, or type `0xffff` followed by u16 `0xffff`, is an arc or circle when four i32 `-2` words follow the centre index (`sldprt.md` §2 and §2.3). Lines and arcs in these cases both carry +17 values `1`, `2` and `4`. A line with value `4` (the horizontal relation code) or `5` (the vertical relation code) can run in any direction in its sketch. The decoder selects the geometry kind from the curve type for these cases. It reports every overridden constraint-owning relation code on a framed list curve with `sketch.listed-curve-relation-dropped`, naming the marker, the curve form and the +17 code, once for each marker of every feature-input lane, whichever projection reads its sketch. A relation still addresses the points that such a record's links reach. The decoder draws each such point as a construction point in a sketch read from the marker's own lane, at the place where the fitted marker transform and the sketch frame agree within the linear resolution, or at the one place that a curve end or centre of the sketch or a vertex or circular-edge centre of the solid marks. It reports a point whose place stays open with `sketch.relation-point-place-unsettled`, and a point whose record form reads a subnormal coordinate pair with `sketch.point-coordinates-subnormal`. Other list curves keep their current layout-selected reading of +17, including relation kinds, and remain open under DI-77; their geometry comes from the record by point-record position whatever +17 holds (`sldprt.md` §2, "A marker whose record is a point record or a curve record"). These include current-layout selector-4 style splines of type `0x40` and selector-68 list curves. Selector bit 3 marks construction geometry. The decoder reads a full circle with role u16 `1` at +27 as profile geometry on every projection path; no selector-12 list curve is such a circle. The decoder marks selector-12 role-1 list lines as construction from bit 3 and reports `sketch.listed-line-construction-assumed`. Some current-layout selector-12 list lines with role `1` have resolved geometry to compare with the edges of the final solid.

**Need.** The value compared with the relation and dimension groups of the same `sgSketch` object and with the record's handles. For the construction state, selector-12 role-1 circles and lines with a known construction state and profile-to-face binding.

**Conflict.** A compared selector-12 role-1 line with ordinal endpoints and one with roster indices lie on edges of faces of the final solid; most compared lines lie on none.

### DI-73. Sketch plane of profile sketches whose own object holds no plane record the decoder reads

**Question.** Which plane holds a profile sketch whose profile object holds no `moCompRefPlane_c` record that the decoder reads as a plane? Does a component reference-plane record of the profile's own object with `3` or `2` at +122, or with the byte `fb` at +126, place the profile?

**Known.** The object pointer at the end of a profile's `sgSketch` names its plane (`sldprt.md` §2.3). Where the profile writes that object in full, the record lies inside the profile object: a principal-plane component record names Top or Right by its selector, and the declared record with selector `3` names Front (`sldprt.md` §2). A profile object holds, of its own, a declared Front record, a principal-plane component record, a component reference-plane record (`sldprt.md` §2), a selector-0 record with a transform (below), several of these, or no record of this family. The component reference-plane record also has selector `0` and a transform; the two forms differ at +0, where the component reference-plane record stores the feature number of a datum plane and the selector-0 record stores `0`, `1` or `0x100ff`. The first three bind the profile (`sldprt.md` §2): the declared Front record and the principal-plane component record by the plane they name, and the component reference-plane record by its placement. Records of the component reference-plane layout also occur with `3` or `2` at +122, and with the byte `fb` or `f9` at +126 in place of the first `ff`. The decoder reads only component reference-plane records whose u32 at +122 is `4` and whose four bytes at +126 are `ff`, and places a profile by such a record of its own object. The profiles that hold a record with `3` or `2` at +122 are not consumed by a feature, and for most of them the B-rep holds no vertex at their points on either the record's frame or the frame the decoder gives. For the profiles that hold a record with `fb` at +126, the record's frame fits the B-rep of some better and of others worse than the frame the decoder gives. Whether such records place their profiles is open. The decoder reads no declared record in a feature-input lane that holds no class name `moCompRefPlane_c` (`sldprt.md` §2), and none whose object-kind byte is less than `0x65`; some declared Front records hold a smaller byte. The decoder places a profile whose own object holds only records it does not read, or none, by the record of the preceding object or the lane default. In the profiles whose stream an archive walk reads to the profile's end, each plane pointer the walk reads is a record of the profile's own object or a plane written in place (feature number `-1`). Counted by B-rep vertices at the sketch points, shifted along the normal by the lengths of the features that consume the sketch, some sketches placed on the Front plane by the record of the preceding object or the lane default lie on more vertices on the Top or the Right plane. For a revolved profile whose axis lies in both the Front and the Top plane, the revolved faces are the same on either plane, and the vertex count does not decide the plane.

The selector-0 record matches the principal-plane component record of `sldprt.md` §2 from +4 through its transform: the u32 at +4 is `0`, the selector is `0`, the matrix-present byte is `1`, and a row-major f64 matrix, a translation and the homogeneous scale `1` follow. The u32 at +0 is `0`, `1` or `0x100ff`; that it is a feature number is open. The byte at +126 is `fb`, the byte of the declared record's trailer, where the principal-plane component record has the sentinel `ffffffff`, in every such record whose u32 at +0 is `1` or `0x100ff` and in some whose u32 at +0 is `0`; whether that trailer belongs to this record is open. Its extent is open: 138 bytes by the extent of the principal-plane component record, 139 by that of the declared record. Read as the frame whose axes are the columns of the matrix `M` and whose origin is `-M * t` for the translation `t`, the inverse of the transform as for the component transform of `sldprt.md` §2, the record's frame fits more B-rep vertices or faces than the Front plane for more sketches than it fits fewer; with `t` read as the origin, it fits fewer for more sketches than it fits more. Whether the record names the profile's plane is open.

**Need.** Whether the selector-0 record of a profile's own object names its plane, and the frame it gives (the meaning of the u32 at +0, the owner of the trailer, the extent and the origin), compared with the faces of the solid; whether a component reference-plane record with `3` or `2` at +122, or with `fb` at +126, places its profile, compared with the faces of solids whose features consume such profiles; and, for a profile object that holds no record of this family, the record that names its plane.

### DI-74. `sgSketch` objects inside a sheet-metal feature object

**Question.** Which sketch do the `sgSketch` objects written inside the span of a `Sheet-Metal` feature object hold, and which feature consumes their curves?

**Known.** `sgSketch` objects with readable curve records (`sldprt.md` §2.3) can lie in the span of a `Sheet-Metal` feature object and before the next named feature object. No sketch feature object encloses them, so no neutral sketch receives their curves. A `Break-Corner` or a `SketchBend` feature object can enclose one such object.

**Need.** The object that writes each `sgSketch` (the sheet-metal feature itself, a sketch it absorbs, or a flat-pattern sketch), and the curves compared with the faces of the stored solid and its flat pattern.

### DI-75. The `sgSketch` object that holds a profile body in a stream read without a complete object walk

**Question.** In a `ResolvedFeatures` stream whose object walk (§2.3) stops before the end, which `sgSketch` object, and so which sketch feature, holds each profile body?

**Known.** A profile body follows little-endian u32 `3` and a zero flag byte (`sldprt.md` §2.3) and belongs to the innermost profile-family object that holds it. Without a complete walk, the feature-name records do not delimit those objects. A profile body can have curves whose radii or lengths can be compared with the curve records of the `sgSketch` lists of the same stream. For most such bodies, some list shares a radius or length with the body, and that list can be the list before the body. When only one list shares a value with the body, that list is most often one to eight lists earlier, and it can also be the list before the body or follow the body. The name span that holds a body often binds it to a sketch feature whose own list shares no value with the body.

**Need.** A complete object walk of these streams, or the field of the profile-family object that points to its `sgSketch` object, compared with the faces each consuming feature produces.

### DI-80. Feature-input marker coordinates on computed sketch frames and on axis-parallel support frames

**Question.** Which coordinates do the two feature-input marker fields hold on a sketch whose frame the decoder computes from other geometry, and on a support plane whose normal is parallel to a model axis and whose frame is not one of those on which `sldprt.md` §2 states the local reading?

**Known.** `sldprt.md` §2 "When profile anchors do not determine the transform" lists the support frames on which the fields are local sketch coordinates: oblique normals; frames with normal `+X` and u-axis `-Z`, or normal `-X` and u-axis `+Z`, whose origin lies on the `X` axis; and frames with normal `+Y` whose origin lies on the `Y` axis, with u-axis `-X`, or with u-axis `+X` off the model origin. A sketch projected from a B-rep face takes its frame from the Parasolid plane: its origin and reference direction. On such a frame with an oblique normal, the model-component reading takes the fields as the model-coordinate components along the two model axes other than the normal's dominant one. Such sketches have relation points that the two readings place differently, and no projected B-rep vertex lies at those points under either reading.

The other axis-parallel support frames include the frames of those families whose origin is displaced within the plane, off the model axis parallel to the normal; frames with normal `-Z` and u-axis `-X`; frames with normal `-Y` and u-axis `-X`; frames with normal `-X` and u-axis `-Z`; frames with normal `+X` and a u-axis other than `-Z`; the Top frame itself for a sketch that no principal-plane component record places; and the Front lane fallback frame (SF-01). Compared under the two readings, the B-rep vertices and faces of the features sketched on the frames with normal `+Y`, `-Y` or `-Z`, other than the Top frame and the Front lane fallback frame, split by the in-plane displacement of the frame origin from the foot of the model origin on the plane. Where the origin is not displaced, the local reading fits more sketches on the frames with normal `-Z` and u-axis `-X` and on the frames with normal `-Y` and u-axis `-X`. Where it is displaced, the model-component reading fits more sketches on the frames with normal `-Z` and u-axis `-X` and on the frames with normal `+Y` and u-axis `+X` or `-X`. On the `X`-family frames with a displaced origin, the two readings fit equally many sketches. A third reading fits the displaced frames: the local coordinates along the frame's axes measured from the foot of the model origin on the plane, not from the frame origin. On the `X`-family frames with a displaced origin, it fits each sketch that a witness decides at least as well as the local reading from the frame origin, and most of them better; under it a sketch can have all of its points on B-rep vertices and its lines on B-rep faces where under the other two readings it has none. On frames with normal `+Y` and a displaced origin, the third reading fits more sketches best than the model-component reading. On the Top frame, more sketches bound through the principal Top reference fit the local reading better than fit the model-component reading better, and sketches whose frame is a stored plane frame equal to the Top frame fit the model-component reading better. Where the model-component reading fits, the local reading often fits a coincident plane that the part stores with another normal sense or u-axis, such as a plane with normal `-Y` and u-axis `+X` where the sketch frame read has normal `+Y` and u-axis `+X`; the sketch then lies on a different plane than the one read for it (open item DI-73).

**Need.** Sketches of either kind whose markers lie on vertices or edges of the B-rep of their features, compared under the model-component reading, the local reading from the frame origin and the local reading from the foot of the model origin, together with the plane reference of each sketch (DI-73).

### DI-81. The u-axis of an offset-plane frame stored against the source normal

**Question.** Which u-axis does a stored frame of an offset reference plane give where its normal points against the source plane's normal?

**Known.** The stored `D1` distance, a frame of the offset plane at that distance from the source plane, and the point at the head of the plane-data object place the offset plane and state its side (`sldprt.md` §2). A stored frame of the offset plane has its normal along the source normal or against it; the u-axis of a frame along the source normal is the offset plane's u-axis. An object can also store a single frame whose normal is perpendicular to the normal of the source plane its object identifies, or a frame or plane-data point parallel to the source plane at neither zero nor the stored distance from it. The decoder takes the u-axis of a stored frame only where the frame's normal points the way of the source plane's normal and the u-axis lies in the plane; otherwise the offset plane keeps the source plane's u-axis. It names the planes whose frame or plane-data point is not the placed plane under `feature.offset-plane-frame-conflict`.

**Need.** Offset planes whose stored frame's normal points against the source normal, each with a sketch whose profile is not symmetric about the plane's origin, and the B-rep vertices of the features sketched on them compared with the sketch placed under each u-axis. For the perpendicular frames, the source identifier of the object compared with the planes of the part.

### DI-82. The u-axis of an angled reference plane whose offset planes store another

**Question.** Which in-plane axis does a reference-plane object with an angular `D1` and a complete frame store as its plane's u-axis, where the offset planes that take it as their source store a different u-axis for it?

**Known.** A reference-plane object with angular `D1` and a complete frame carries that frame as direct datum-plane placement, and the 121-byte angular frame stores a u-axis, a normal and a v-axis (`sldprt.md` §2). An angled plane can read u-axis `(0,1,0)` while the dual-frame offset planes whose source it is store its frame with u-axis `(0.707,0,-0.707)` and the same normal: the Front frame turned about `Y` by the plane's angle. A sibling angled plane turned the other way reads the u-axis its offset planes store. The sketches on its offset planes take the u-axis those planes store.

**Need.** Angled planes whose offset planes store their source frame, compared field by field with the angular frame, and the B-rep of features sketched on an angled plane itself.

### DI-84. Produced bodies of a configuration without a bound partition

**Question.** Which carrier states the bodies a feature produces in a configuration whose `Config-N-Partition` is absent or unbound?

**Known.** The `FACE_ID_2001` producers and the `LAST_BODY_MODIFYING_FEATURE_ID` attributes of a configuration's own partition state the bodies each feature produces in that configuration (`sldprt.md` §2, §5). The decoder gives the feature states of a configuration with unresolved body membership no produced body, and a decode loss counts the configurations with unresolved body membership. The document features name their producers over every decoded partition, so a document feature can name a body that the active configuration's partition does not hold.

`Config-N-FeatureBodies/LocalBodies` holds the bodies an Imported feature owns (`sldprt.md` §3.3). Where the active partition is an empty partition/deltas model, they are the active configuration's bodies. A configuration that is not active can store a `Config-N-FeatureBodies` stream with no `Config-N-Partition`. Whether those bodies are that configuration's bodies or the input of its import is not known. The decoder takes them into no state and charges the warning `config.state-imported-bodies-not-transferred` naming the configuration index; it does the same for a configuration that is not active whose partition holds no body. For the active configuration, whose bodies the model would then lack, the same loss is the error `geometry.imported-bodies-not-transferred`.

**Need.** A `Config-N` or `Config-N-ResolvedFeatures` record that names output bodies by an identity the partition also stores (`ATOM_ID_2001`), compared with a configuration whose partition holds the same bodies. For `Config-N-FeatureBodies`: a configuration that is not active whose partition is an empty partition/deltas model beside such a stream, or the Imported feature of its `Config-N-ResolvedFeatures` lane naming its body by an identity the stream also stores.

### DI-85. Revolution axes that lie outside the profile plane

**Question.** Which plane does the profile sketch of a revolution lie on where the axis read from its `moLineRef_w` record lies outside the plane the decoder binds the sketch to?

**Known.** The record ends with its tail, a scalar, a point and a unit direction (`sldprt.md` §2). The decoder keeps such an axis and names the revolution under `feature.revolve-axis-off-profile-plane`. Each such axis compared with the B-rep is the axis of a B-rep cylinder, cone or torus of the part, or passes through the centre of a B-rep sphere of the part. An axis runs parallel to the bound plane at an offset, along its normal, or oblique to it. The decoder tests the axis against the plane at `1e-8` m, the usual value of the BODY node's `resolution_linear` field (`sldprt.md` §4.1), in place of the value the part states. The sketches are bound by the plane records of open item DI-73. The bound plane of an offset reference plane can itself be misplaced: the decoder places it at the distance the history text of its `D1` displays, rounded to the displayed decimals, and reads a number without a unit as millimetres whatever the document's length unit. A profile sketch on such a plane can lie on a plane that holds its axis while the bound plane does not.

**Need.** For each revolution named, the plane record of its profile sketch, compared with the plane through the stored axis that holds the most profile points on B-rep vertices.

### DI-86. Words before the scalar record of a revolution `moLineRef_w` record

**Question.** What do the words before the scalar record store in a revolution `moLineRef_w` record whose prefix the forms of `sldprt.md` §2 do not describe?

**Known.** Each such record holds two `c7 cf ff ff` handle words, then a zero u32, a stream address, zero bytes, and the tail of `sldprt.md` §2 with its zero byte before a class declaration or an instance token. One prefix form has the source cell, the u32 values `0`, a nonzero variant, `1`, `0`, a nonzero stream address, a nonzero u32 eight bytes before the handles, and a zero stream address after the handles. Another has a source identifier and a nonzero identity with a null token and no `ff ff ff ff` word, in an interval that also declares an `moCompRefAxis_c` record. The decoder reads the axis from the tail of such a record and names the revolution under `feature.revolve-axis-record-prefix-unread`. Where the tails of several such records in one interval state different axes, the decoder takes no axis from them.

**Need.** For each prefix form, the profile object its source cell names, compared with the profile the revolution consumes, and the record that the u32 before the handles addresses.

### DI-92. Boolean operation of a revolution after a body

**Question.** Which stored field of an `moRevolution_c` object tells a revolution that makes a new body from a revolution that joins the bodies before it?

**Known.** The u32 before the object's class marker or object-name token (`sldprt.md` §2) is `6` on a revolution that makes the first body of the part. It is also `6` on a revolution that follows a body-making feature in a part that holds one body after it, where no later feature combines bodies. It is `8` on a revolution that follows a body-making feature. Where the bytes before that u32 are `ff ff ff ff`, a zero u32 and a nonzero u32, the u32 is `6` or `8`; other byte forms before the class marker give other values in the same position. The named-object header after the revolution's name string (`sldprt.md` §2.3) holds the same family word, operation byte and schema byte on every revolution. The decoder states form `8` as a join and leaves the operation of the other forms unread. The design states a revolution that adds material as a new body where no member before it can hold a body, and states no boolean, under `design.revolve-boolean-unstated`, after such a member.

**Need.** A part with a boss extrusion and a boss revolution that overlaps it, saved once with the revolution's merge-result option set and once with it clear, and the bytes of the two `moRevolution_c` objects compared.

### DI-93. Use-default state of an edge flange's radius and K-factor

**Question.** Which bytes of a `moEdgeFlange_c` feature state that its bend radius and K-factor are the sheet-metal feature's (the "use default" choice) in place of its own `D1` and `D2`?

**Known.** An edge flange stores its own `D1` (radius) and `D2` (K-factor) scalar and `PMISemanticDataDB` records after its `moOneBend_c` child (`sldprt.md` §2, sheet-metal dimensions). Where the sheet-metal feature states a radius and K-factor other than the edge flange's own `D1` and `D2` and the bend uses the sheet-metal feature's, the child's `D1` equals the sheet-metal feature's `D1` and is the inner radius of the bend's cylindrical faces. No byte of the edge flange is known to differ between the two choices.

**Need.** An edge flange that applies its own radius and K-factor in a part whose sheet-metal feature states others, beside the same part with the default chosen, compared byte for byte.

### DI-95. Operands and measured direction of a point-point distance relation

**Question.** Which marker roster do the two operand addresses of an `sgPntPntDist` relation index when their tag is a lane-local class token, and which field states whether the dimension measures the Euclidean, the horizontal, or the vertical separation of its two points?

**Known.** The relation object writes its relation class, a zero u32 and its parameter class, then a display-role scalar and a driving-role scalar of the same name (`sldprt.md` §2). Each scalar ends with two operand cells, compact 8-byte cells or 12-byte cells, each a class token and a u16 address, and then u16 `1`, u16 `1`, u16 `1` and four zero bytes. Read as feature-local object indices, the addresses of a relation can select two coordinate-bearing points whose horizontal or vertical separation equals the driving scalar and whose Euclidean separation differs from it. Read as zero-based ordinals over all coordinate-bearing markers of the owning feature in byte order, with the point records of DI-46 included, the same addresses select two points whose Euclidean separation equals the driving scalar. In such a sketch, the other relations of the class that use the same token also fit the ordinal reading, and their object-index reading selects no pair at the driving distance. The u16 triple after the operand cells is `1`, `1`, `1` in relations whose points are separated on one axis only and in relations whose points are separated on both. The decoder keeps a relation native when its resolved operands measure another value than its driving scalar.

**Need.** A sketch with two points on no common axis, dimensioned three times with point-to-point dimensions placed aligned, horizontal and vertical, saved, and the three relation objects and their `moDisplayDistanceDim_c` display objects compared, together with a second save in which a point marker is added before the dimensioned points.

### DI-97. The state after a feature whose records name two marks, and the tree order against the mark order

**Question.** Which state of the main line is the state after a feature whose `moPMarkRecord_c` records name two marks of the main line, and which order of the feature tree holds where the order of the feature elements of `Keywords` differs from the order of the features' marks on the main line?

**Known.** A record's features run from the nearest named object before it back to its holder (`sldprt.md` §3.4). A feature can hold records that name two different marks of the main line. The marks of the records follow the main line in stream order. The feature elements of `Keywords`, with each feature after the features it depends on, can place a feature after a feature whose record names a mark nearer the current mark.

**Need.** A part with three boss extrusions on independent sketches, saved, and saved again after the third extrusion is moved before the second in the feature tree, with the `Keywords` order and the marks of the three records compared in the two saves. A part with a hole-wizard hole on a face of a boss extrusion, saved, with the marks of every record of the hole compared to the marks of the main line.

## 8. Edited parts

### DR-1062. The edits a stored part can hold

**Question.** When the patch of an edited part and its synthesis without the SolidWorks history both refuse the document, which of its edits can the writer write into the stored part, and which edits must it keep as stored?

**Known.** The decoder stamps one codec digest over each kind of record: the neutral features (`sldprt_neutral_feature_local_sha256`), the neutral parameters (`sldprt_neutral_parameter_local_sha256`), the neutral configurations (`sldprt_neutral_configuration_local_sha256`), the native feature-history records (`sldprt_native_history_sha256`) and the native parameter entries (`sldprt_native_parameter_sha256`). A moved digest isolates the kind of record that the edit changes. The retained source image, decoded again, isolates each changed record by its id. The patch writes each edit into the record of its feature and refuses the document as a whole. The refusal names a record for some edits and names no record for others. When both paths refuse, the writer writes the stored part and charges one `export:edit.dropped` loss per arena, which names each record that the document adds, removes or edits.

**Need.** For each edit kind that the patch refuses, a write of the decoded source with that edit only and every other edit restored to its stored state, which shows whether the patch writes the other edits. A refusal that names the record which causes it lets the writer restore that record and write the rest.
