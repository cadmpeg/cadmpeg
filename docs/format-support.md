# Format support

This document lists the repository's read, write, and round-trip support. Each codec has one ladder score per declared envelope and a profile for each domain.

SolidWorks `.sldprt` has the broadest native read/write path and serves as the [reference format](roadmap.md#reference-format-solidworks-sldprt) for full semantic support. Every native format remains incomplete.

## Support ladder

The L0–L9 ladder measures how much source semantics a codec recovers for use. L0–L4 add usable content categories. L5–L9 require complete coverage of the categories below them.

### Scoring rules

1. **Strict and cumulative.** A codec earns Ln by passing every gate through Ln. Its score equals the highest level whose gates all pass.
2. **Integer levels.** Capabilities above the score appear as extras. The ladder has no fractional or plus-marked levels.
3. **Resolve doubt downward.** A partially met gate fails.
4. **Require a usable slice.** A capability rung requires working support across mainstream files in the declared envelope. A single fixture, entity census, or opaque record capture cannot satisfy it.
5. **Pass inapplicable gates.** A format definition may establish that its document kind cannot contain a category. Missing fixtures cannot establish inapplicability.
6. **Score each envelope.** A codec declares version and layout-variant bands and receives one score per band. A single version can earn L9. State discontinuous support per band.
7. **Qualify the evidence.** Every score is **claimed** when code exists, **tested** when fixtures exercise it, or **proven** when it passes the [roadmap's](roadmap.md#progress-gates) public-fixture, byte-accounting, round-trip, and fuzzing gates.

### Levels

- **L0: Parsed.** The codec detects the format and document kind, parses container framing, and extracts document metadata plus a preview image or tessellation.
- **L1: Opened.** The codec navigates sections, directories, streams, compression, and checksums; identifies governing versions and layout variants; enumerates embedded assets and external references; and names undecoded content.
- **L2: Geometry.** Typed IR contains placed points, analytic curves and surfaces, and NURBS with correct units, parameterization, and model-space placement. Prototypes, datums, and display meshes require placement or exact geometry to satisfy this level.
- **L3: Model.** Connected B-rep from bodies through vertices includes ownership, orientation, trimming, placement, and transforms. Structural validation passes across the band. Connected topology may contain unknown carriers.
- **L4: Design records.** Features carry operation semantics, such as an extrude's profile, direction, and extent. Sketch geometry, ordering, and dependencies expose the history. Replayability belongs to higher levels.
- **L5: Shape complete.** Every geometry carrier family and topology case in the band decodes. Mainstream band files contain typed carriers throughout, and body and face colors transfer.
- **L6: Design complete.** Complete sketch constraints, dimensions, parameters, and expressions; every feature family with full operation semantics; configurations; history coherent enough to re-derive the model.
- **L7: Product.** Components, occurrences, placements, external references, mates, and persistent identity across the structure. Part-only formats satisfy inapplicable gates.
- **L8: Full document.** Presentation, PMI, annotations, and drawings where carried; application data typed or deliberately preserved with identity; complete byte accounting that classifies every byte as typed, structural, or part of a named opaque record.
- **L9: Writes back.** Semantic native writing supports edits, source-less generation, target-version selection, explicit rejection, and unsupported-record survival or refusal. Independent native applications accept verified round trips. Byte replay and bounded patching count as extras at lower levels.

### Current scores

| Codec                                      | Score          | Extras above score                                                                                    |
| ------------------------------------------ | -------------- | ----------------------------------------------------------------------------------------------------- |
| Autodesk Fusion `.f3d`                     | **L4 tested**  | native replay + patch + broad source-less generation, procedural carriers, ACT/Design/history records |
| SolidWorks `.sldprt`                       | **L4 tested**  | typed features, sketches, parameters, configurations, native replay + bounded generation              |
| Rhino `.3dm` (archive 50/60/70/80)         | **L9 tested**  |                                                                                                         |
| CATIA V5 `.CATPart` (standard-nested band) | **L2 claimed** | conditionally connected B-rep                                                                         |
| Siemens NX `.prt` (selected or terminal-lineage-resolved body images) | **L4 claimed** | procedural carriers, topology attributes, external-dependency inspection, named arrangements |
| Siemens NX `.prt` (unselected multi-partition history) | **L2 claimed** | connected candidate B-reps, external-dependency inspection                              |
| CATIA V5 `.CATPart` (other layout bands)   | **L1 claimed** |                                                                                                       |
| Creo Parametric `.prt`                     | **L1 claimed** | derived datum planes, prototype geometry census                                                       |
| Rhino `.3dm` (V3/V4)                       | **L1 tested**  | metadata and bounded object-record retention                                                          |
| Rhino `.3dm` (V1/V2 and archive 5)         | **L0 tested**  | header-only inspection; decode is rejected                                                            |
| STEP AP214                                 | translation    | partial B-rep export with explicit loss reporting                                                     |

Each current score applies to the envelope described in its profile.

## Status terms

- **None:** the repository lacks an implementation for the domain.
- **Inspect:** cadmpeg identifies and reports the structure without transferring it into typed IR.
- **Partial:** cadmpeg transfers a typed subset and reports or preserves the remainder.
- **Complete:** the domain satisfies the public-fixture, byte-accounting, validation, round-trip, and fuzzing gates in the [roadmap](roadmap.md#progress-gates).

Every current profile contains incomplete domains. Current claims rely on code, generated fixtures, and explicit loss paths while the public corpus remains empty.

Entity provenance and domain status measure different properties. `byte_exact`, `derived`, `inferred`, and `unknown` describe how cadmpeg obtained one IR value.

## At a glance

- **Autodesk Fusion `.f3d` (L4 tested):** design records, partial B-rep and appearance reads, byte-exact replay, native patching, and source-less generation.
- **SolidWorks `.sldprt` (L4 tested):** connected model reads, typed design records, native writes, and round trips.
- **Rhino `.3dm` (L9 tested for archive 50/60/70/80):** complete built-in model, product, presentation, annotation, metadata, application-data retention, and byte accounting, plus bounded semantic native writing with source-less generation, supported edits, explicit target selection, and atomic refusal. V3/V4 score L1; V1/V2 and archive 5 score L0.
- **CATIA V5 `.CATPart` (L2 claimed for the standard-nested band):** exact carriers and conditionally connected topology. Other layout bands score L1. Read only.
- **Siemens NX `.prt` (L4 claimed for single-body, `RMFastLoad`-selected, and terminal-feature-lineage-resolved body images; L2 claimed for unresolved multi-partition history):** exact carriers, connected topology, ordered feature operations, expressions, and typed sketch-point dependencies. Read only.
- **Creo Parametric `.prt` (L1 claimed):** container navigation, derived datum planes, and prototype geometry inspection. Read only.
- **STEP AP214 (translation):** partial B-rep export with explicit loss reporting.

## Rhino `.3dm`

**Model:** 3DM object graph

**Ladder: L9 tested for archive 50/60/70/80; L1 tested for V3/V4; L0 tested for V1/V2 and archive 5.**

### Read profile

- **Container and versions: Partial.** Archive 50/60/70/80 and V3/V4 use bounded chunk, checksum, table, object-record, class, attribute, userdata, properties, settings, layer, and EOF framing. V1/V2 and archive 5 expose the 32-byte header and archive version only; normal decode returns `NotImplemented`.
- **Geometry: Complete for archive 50/60/70/80 built-in shape carriers.** Points, point clouds, lines, arcs, polylines, polycurves, curves on surfaces with parameter/support carriers, persistent polyedge references, NURBS curves, NURBS surfaces, NURBS cages and morph controls, plane surfaces, clipping-plane carriers, revolution surfaces, sum surfaces, hatches with placed boundary loops, detail-view boundaries, meshes, extrusions, and SubD control cages transfer into typed IR. Registered legacy identities for polycurve, NURBS curve, NURBS surface, revolution surface, and Brep use the same typed readers as their current identities. Runtime proxy and component-reference classes do not carry independent archive payloads. Lengths and length-valued tolerances convert to millimeters; angles, unit vectors, knot values, UV values, relative tolerances, and hatch pattern scale remain unscaled. Third-party classes and future payload versions remain native unknown records with bounded bytes or a complete-record length and digest. V3/V4 geometry remains unknown.
- **Topology: Complete for archive 50/60/70/80 built-in Brep cases.** Breps transfer atomically into connected body, region, shell, face, loop, coedge, edge, vertex, 3D-curve, surface, and pcurve graphs. Invalid Brep topology retains the source record and decoded child carriers without committing a partial graph.
- **Tessellation: Partial for archive 50/60/70/80.** Mesh vertices, normals, faces, texture coordinates, colors, surface parameters, and ngons transfer where their channel invariants pass. Unsupported cache and channel metadata remains retained.
- **Design intent: Complete for the built-in 3DM design model.** Native revolution, sum-surface, polycurve, persistent polyedge-reference, curve-on-surface, extrusion, hatch, NURBS-cage, and curve/surface/cage morph constructions transfer with exact solved carriers or complete persistent-reference semantics. Morph operations retain captive identities, localizers, tolerances, preview mode, and structure-preservation mode. Built-in history records transfer as ordered native operations with persistent record identity, command identity, command version, every constructible built-in value family, complete object selections, polyedge constructions, SubD edge chains, object antecedents and descendants, and unambiguous producer dependencies. Numeric parameter identifiers and command UUIDs remain native identities because 3DM carries no application-independent names, expressions, constraint taxonomy, or feature-family schema for them. Modern and legacy V5 dimensions, text, leaders, center marks, text dots, hatches, and detail views transfer with their complete built-in definition and display state. Embedded history geometry transfers as unit-normalized geometry, complete topology, and exact construction semantics.
- **Product structure: Complete.** Definitions retain persistent identity, archive index, name, description, URL, units, ordered members, definition kind, nested link depth, appearance policy, and structured external-file identity. Occurrences retain persistent identity, definition identity, parent memberships, composed placement, visibility, name, and exact source-object association. Static and linked-and-embedded members expand recursively. Linked definitions without local geometry remain structured external product definitions. The 3DM product model does not carry assembly-mate semantics.
- **Presentation and metadata: Complete.** Typed records cover layers and hierarchy; complete object display attributes and rendering-material bindings; materials and texture slots; texture mappings; embedded and Windows bitmaps; groups; lights; linetypes; hatch patterns; text styles and font characteristics; dimension styles; global annotation, grid, and render settings; saved and active views; cameras, frusta, clipping planes, page settings, construction planes, wallpaper, and trace images; previews; notes; revisions; application identity; units; selectors; and file URLs. Modern and V5 annotations retain text, formula, placement, leader geometry, style identity or index, alignment, wrapping, scaling, and text-dot display state. Third-party classes, class userdata, attribute userdata, render userdata, and future extensions remain named exact records with class/item or record identity.
- **Recovery, retention, and accounting:** Chunk boundaries isolate semantic failures. The native byte ledger partitions the complete source without gaps or overlaps into typed archive-header bytes, structural framing/checksum/end-marker bytes, and named opaque complete-record bytes. Every non-object record has exact retained bytes; every object record links to an exact retained unknown-record entry. Complete unknown records are retained within per-record and per-document limits; larger records retain exact length and SHA-256 digest. Invalid Brep, extrusion, SubD, and instance candidates do not commit partial topology.

### Write and round trip

- **Native write: Bounded.** Explicit archive 50, 60, 70, and 80 targets write source-less point objects, grouped point clouds as free-vertex bodies, circles, native-canonical rational and non-rational NURBS curves, planes, native-canonical rational and non-rational NURBS surfaces, connected polygon sheet Breps with multiple faces, shared manifold edges, disjoint outer and inner loops, line or full-domain nonperiodic rational and non-rational NURBS edges, closed manifold planar solid Breps, bounded nonperiodic rational and non-rational NURBS faces, and standalone triangle meshes. Generated archives contain the canonical table sequence, a persistent default layer, and object attributes bound to that layer. Multiple independently owned Breps, free-vertex groups, and standalone geometry coexist in one object table; each Brep is preflighted and serialized from its isolated ownership graph. Every generated object receives a deterministic native UUID derived from its IR identity. Every NURBS-face coedge carries an explicit line or full-domain nonperiodic NURBS pcurve. The pcurve remains inside the surface parameter domain and maps through the surface onto the directed C3 edge within the declared face, edge, and pcurve tolerances. Planar NURBS edges retain their C3 carrier and generate an exact projected NURBS C2 for each directed face use. Explicit line and full-domain nonperiodic NURBS pcurves are written when they exactly equal that directed projection; their parameter range and fit tolerance are retained. Pcurves with incompatible geometry, ownership, wrapper state, native tail state, parameter domain, or surface/edge agreement are rejected. Writable Breps retain edge parameter domains, directed coedge rings, radial edge incidence, sheet or solid body classification, and vertex and edge tolerances. Solid output requires two opposite directed uses of every edge, a connected shell, and nondegenerate face loops. Free-vertex bodies and writable Breps retain object names, RGBA colors, and visibility through deterministic object attributes. Mesh normals, UV values, colors, surface parameters, and curvature channels retain their native scalar encodings. Archive 60, 70, and 80 use the double-vertex mesh extension; archive 50 uses float vertices and reports quantization. Unsupported arenas, cross-object topology carriers, periodic surface topology, non-manifold topology, other display state, native records, shared free-vertex points, strip topology, unknown mesh channels, and noncanonical NURBS contracts are rejected before output.
- **Round trip: Bounded.** Generated archives decode through the native reader for every supported target and are accepted by an independent native application for archive 50, 60, 70, and 80. Exact geometry and grouping are retained for the writable families except explicitly reported archive-50 mesh quantization. A decoded archive produced by this writer can be edited and semantically regenerated when its native namespace contains only the writer's accounting records, default layer, default object presentation, and supported object records. Any additional retained Rhino arena or nondefault native presentation state is refused rather than silently dropped or replayed against edited semantics.

See [`formats/rhino_3dm.md`](formats/rhino_3dm.md) and [`formats/rhino_3dm-open-items.md`](formats/rhino_3dm-open-items.md).

## SolidWorks `.sldprt`

**Kernel:** Parasolid

**Role:** reference format for full semantic support

**Ladder: L4 tested.** Unknown geometry carriers and topology cases block L5. Incomplete sketch constraints and feature families block L6.

### Read profile

- **Container and versions: Partial.** The codec validates CRC-framed blocks, enumerates cache cells and the tail directory, extracts active Parasolid partitions, and preserves the source image. Coverage across historical schemas remains incomplete.
- **Geometry: Partial.** Analytic and NURBS surfaces and curves transfer into typed carriers. Offset, swept, blend, intersection, and other unsupported families remain opaque or produce unknown carriers.
- **Topology: Partial.** The codec builds body, region, shell, face, loop, coedge, edge, and vertex ownership for supported layouts, including multiple regions and shells per body. Schema-32001 solid and sheet regions and schema-33103 solid regions follow their native region/lump/shell chains. Schema-33103 interleaved faces partition by native adjacency components rather than stream intervals. Disc14 faces partition through native region, shell, face-use, and face geometry rings. Partition face membership excludes superseded deltas geometry; deltas update referenced points and complete missing subordinate records. Periodic seams, orientation, and several pcurves are derived. Older body layouts and schema-33103 sheet classification remain open.
- **Tessellation: Partial.** Display-list geometry transfers into tessellation arenas and can be regenerated. Stable face-to-triangle ownership remains open.
- **Design intent: Partial.** Configurations transfer as neutral records with material and property overrides and retain their configuration-specific solids. Keywords history retains feature element tags, exact containment including id-less nodes, order, names, suppression, dimensions, expressions, and attributes. Unknown operation families retain their kind, dimensions, and non-parameter attributes in the neutral native-operation definition. Reference planes, axes, points, and coordinate systems retain complete model-space placement. Planar profile B-reps nested in feature-input lanes transfer as placed sketches with solved lines, circles, arcs, ellipses, and rational or non-rational NURBS, plus oriented profile loops. Boss and cut extrusions retain blind, symmetric, two-sided, through-all, and native-face termination, explicit direction, draft, profile, and Boolean operation. Explicit-axis revolutions retain one-sided, symmetric, and two-sided angular extents, profile, axis placement, and Boolean operation. Profile sweeps, lofts including boundary boss and cut forms, ribs, bending, twisting, tapering, and stretching flexes, drafts with selected faces and neutral planes, direct body Boolean combines, body deletion and isolation, body scaling about an explicit center, face deletion and replacement, face offset/translation/rotation, spherical and elliptical domes, linear and circular patterns, mirrors, constant and variable-radius fillets with selected edges, dimensional chamfers with selected edges, shells with removed faces, face thickening in either direction or both directions, and simple, counterbore, and countersink holes with explicit face, position, direction, and blind or through-all termination project to neutral operations and write edits through retained native records. Sketch constraints and other operation families remain open.
- **Product structure: None.** `.sldprt` support covers parts only.
- **Presentation and metadata: Partial.** Base colors, appearance bindings, previews, SolidWorks XML metadata, units, and selected attributes transfer. Full appearance precedence and all embedded metadata stores remain open.

### Write and round trip

- **Native write: Partial.** Unchanged IR with a retained source image writes byte for byte. Modified or source-less supported IR regenerates native blocks and a section directory.
- **Semantic write limits:** at most five regions per body and six shells per solid region; sheet regions require one shell. Explicit face names, stored edge parameter ranges, periodic NURBS carriers, and unbounded appearance data are not encoded.
- **Round trip: Partial.** Byte-exact unchanged-file and semantic regeneration paths have generated-fixture tests. The public version and feature matrix remains to be built.

See [`formats/sldprt.md`](formats/sldprt.md) and [`formats/sldprt-open-items.md`](formats/sldprt-open-items.md).

## Fusion 360 `.f3d`

**Kernel:** ASM, derived from ACIS

**Ladder: L4 tested.** Unresolved procedural families and unknown carriers block L5. Native writing exceeds the scored gates.

### Read profile

- **Container and versions: Partial.** The codec selects the first `.smbh` entry, falling back to the first B-rep entry, and decodes linked Protein, Design, MetaStream, and ACT records. The authoritative relation among multiple asset folders and B-rep entries remains unresolved.
- **Geometry: Partial.** Analytic surfaces and curves, cached NURBS carriers, parameterizations, signed radii, point-degenerate curves, exact, compound, ruled, sum, revolution, offset, rolling-ball, pipe, taper-family, loft, and G2-blend spline surfaces, exact-cache curve, compound/deformable curves, helix, vector-offset, surface-offset, spring, subset, projection, silhouette/taper-silhouette, two- and three-surface intersection, two-sided offset, and prefix-only blend/surface/parametric/skin constructions with null, analytic, or NURBS surface/UV support pairs transfer under both modern and legacy subtype names. Law-dependent net/skin/sweep forms, variable/vertex blends, and related families remain incomplete when no solved cache resolves.
- **Topology: Partial.** Shell-reachable solid, sheet, wire, and mixed-dimensional general bodies transfer with shells, faces, loops, coedges, wire edges, edges, and vertices. Closed edge incidence classifies solid bodies, open face incidence classifies sheet bodies, face-less wire membership classifies wire bodies, and bodies containing both faces and wire edges classify as general. Unsupported surface records retain topology with unknown geometry; free vertices, some procedural edges, and some explicit pcurves remain unresolved.
- **Tessellation: None.** Fusion display meshes remain outside the IR tessellation arena.
- **Design intent: Partial.** ASM history states, Design assignments, sketch-side records, construction recipes, persistent references, MetaStream identities, and ACT channels transfer. A complete replayable Fusion feature history remains open.
- **Product structure: Partial.** Body transforms and root-component records transfer. Multi-component structure and constraints remain open.
- **Presentation and metadata: Partial.** Linked source attributes, Protein appearance assets, material properties, body bindings, and per-body display visibility transfer. External material-library display names and some schema fields remain unresolved.

### Write and round trip

- **Native write: Partial.** An unchanged retained source archive writes byte for byte. The writer patches model points, common analytic and NURBS B-rep curves and surfaces, rational/non-rational pcurves, procedural caches, sketch geometry, constraints, history fields, design records, and supported appearance properties in their original records. Source-less generation writes multi-body B-reps containing solid, sheet, wire, and mixed-dimensional general topology with analytic or rational/non-rational NURBS carriers; exact, compound, ruled, sum, revolution, offset, rolling-ball, taper, loft, G2-blend, and translational-extrusion surface constructions; exact-cache, compound, deformable, helix, vector-offset, and subset curve constructions; inline rational/non-rational NURBS pcurves; placements; document tolerance metadata; direct body/face color attributes; body/face/edge persistent-design tags; coedge-to-sketch provenance; typed ASM history; Design object and construction streams; sketch geometry and constraints; ACT tables and component roots; and Protein appearances with body bindings.
- **Write limits:** General writing requires a retained source archive and the original entity and record layouts. Source-less generation supports multiple placed bodies, regions, and shells with plane, cylinder, cone, sphere, torus, or rational/non-rational NURBS faces; multiple loops; shared radial edges; line, circle, ellipse, point-degenerate, or rational/non-rational NURBS edge curves; inline rational/non-rational NURBS pcurves; face-less wire bodies with chained regions and shells; and mixed face-and-wire shells with one or more wire edges. Free vertices and edits outside the listed fields are rejected.
- **Procedural write invariant:** One solved curve has at most one procedural construction. Exact-cache, compound, deformable, helix, vector-offset, surface-offset, spring (including conditional null-carrier ranges), subset, ranged and early-close projection, silhouette/taper-silhouette, two- and three-surface intersection, two-sided offset, and prefix-only blend/surface/parametric/skin definitions with paired analytic/NURBS surface and NURBS UV supports serialize with their native construction fields, child carriers, role fields, and solved caches. Scalar offset and blend-spine definitions remain rejected source-less until their external fields can be emitted without semantic loss. No typed construction is reduced silently to a cache-only curve.
- **Round trip: Partial.** Generated byte fixtures cover byte-exact replay and each writable geometry, history, Design, sketch, ACT, and appearance family.

See [`formats/f3d.md`](formats/f3d.md) and [`formats/f3d-open-items.md`](formats/f3d-open-items.md).

## Siemens NX `.prt`

**Kernel:** Parasolid in an SPLMSSTR container

**Ladder: L4 claimed for single-body, `RMFastLoad`-selected, and terminal-feature-lineage-resolved body images; L2 claimed for unresolved multi-partition history.** The L4 envelope carries ordered feature and sketch records, resolved expression dependencies, solved sketch-point coordinates, and ordered sketch-to-datum dependencies. Multi-partition histories remain L2 unless every emitted partition receives one unambiguous terminal lineage status.

### Read profile

- **Container and versions: Partial.** The codec decodes the SPLMSSTR directory, extracts and classifies embedded Parasolid partition, deltas, and related streams, resolves short and extended segment wrappers from their encoded extension lengths, retains fixed-ID NX object-model entities at their external index boundaries, retains offset-only object-model column storage as distinct bounded blocks, and exposes strictly framed JPEG preview dimensions and asset identity.
- **Geometry: Partial.** Points, analytic surfaces and curves, typed B-spline surfaces and curves, supported type-133 trimmed curves, and exact OFFSET_SURF and BLEND_SURF procedural carriers transfer into IR. A procedural carrier has a required bidirectional link to its typed construction instead of an unknown geometry placeholder. Nested OFFSET_SURF carriers evaluate recursively over analytic, NURBS, or evaluable procedural supports with cycle rejection. BLEND_SURF evaluation remains open where finite domains or branch selection are unresolved.
- **Topology: Partial.** Active body-image bands emit connected body, shell, face, loop, fin, edge, and vertex graphs with ownership, orientation, trimming, and incidence. A typed edge carrier remains attached when its serialized trim limits fail endpoint validation; the unvalidated parameter range is omitted. Validated intersection support-UV charts attach as coedge pcurves with their parameter ranges and fit tolerances. Missing and sentinel-bearing support lanes are reconstructed atomically by inverse parameterization only when every point forward-evaluates within the effective chart tolerance; nested procedural blend spines add their cache-fit tolerance and the dependent curve records the resulting bound. Carriers with exactly equal parameterization share a complete lane. Finite-tolerance EDGE records with a null curve pointer transfer as procedural tolerant-intersection carriers over their two adjacent face surfaces; the edge vertices bound the relation. Non-null shell BODY and REGION references supply ownership identity when either record is omitted; shell layouts use either a FACE chain or a face anchor with FACE back-references. Solid and sheet kinds derive from edge incidence. Inline Parasolid coordinates are in part-model space and bodies have identity placement. A validated partition shell defines a current topology image; paired deltas topology records remain revision history. Supported non-topology replacements and tombstones use the last event for each exact key, except that a surviving topology reference keeps its partition point, curve, or surface carrier when no later replacement exists. `RMFastLoad` object membership selects every decisively represented active body image and removes inactive historical images. Without a decisive membership set, complete feature-history primary writers and later Boolean tool consumption select terminal bound partition images atomically. Feature-history composition remains unresolved when any emitted image lacks that complete lineage.
- **Tessellation: Partial.** Embedded JT 9 shape-LOD segments decode lossless and uniformly quantized coordinates, lossless and Deering normals, lossless and RGB/HSV-quantized colors, up to eight lossless or uniformly quantized texture-coordinate channels, and binary vertex flags. Topological dual-mesh connectivity and corner attribute indices reconstruct exactly. Logical shape-node to late-loaded payload ownership resolves through the property table. Common group-derived and instance-node paths accumulate affine transform attributes; each distinct root-to-shape path emits a tessellation with ordered instance provenance. Nonnegative-group triangles transfer in document units with transformed seam-preserving normals and codec-owned RGBA, texture, and flag channels parallel to deindexed vertices. Body association remains unresolved.
- **Design intent: Partial.** Feature-history record areas project ordered neutral features and retain exactly bounded operation records, byte-exact labels, and four object-index slots. `DATUM_PLANE` and `DATUM_CSYS` labels project as typed datum families with explicitly unresolved frames. Non-null header slots bind to uniquely addressed offset-only input blocks. Exact reuse of a resolved datum-plane or datum-coordinate-system construction block by a later operation input creates an ordered neutral feature dependency without assigning frame roles. Uniquely resolved parameter declarations appear as neutral parameter content on every consuming operation in serialized input order. Primary-body fields form ordered lineage dependencies and bind surviving segment body images as feature outputs; Boolean features also depend on the latest ordered tool-body writers. Unite, subtract, and intersect operations project as typed body-combine features with ordered native target/tool selections. `SEW` operations project as typed body-sew features; complete validated body operands supply the ordered body selection, while incomplete operands leave it unresolved. `TRIM BODY` operations project as typed body-trim features; one primary body and complete validated body operands supply the ordered target and tool selections, while incomplete operands leave both selections unresolved. The retained side remains unresolved. `EXTRUDE` operations with one complete construction profile project as typed extrusions with a native profile identity. A first-writer extrusion with one transferred solid output projects as new-body; later writer states remain Boolean-unresolved. Direction, termination, and draft remain unresolved. A single `OFFSET` output body whose owned offset carriers have one exact distance projects as a typed surface-offset feature with its native support-surface selection. A single `THICKEN_SHEET` output body whose owned offset carriers have one exact finite nonzero signed distance projects its absolute magnitude as thickness with native support-surface identities; its side remains unresolved. A single `BLEND` output body whose owned blend carriers are circular projects as a typed fillet with an exact common constant radius or a retained structural radius form. `BLOCK` projects ordered exact dimensions and, when exactly one output body has one matching owned three-band orthogonal planar extent system, its derived rigid placement. `SIMPLE HOLE` projects as a typed simple-hole operation; a `Hole_GeneralHole_Simple_Through_*` payload template projects through-all termination. A complete single construction group partitions through-hole operations by exact output-body ownership; equal-cardinality uniform owned bore cylinders supply one permutation-invariant diameter per body, and distinct output bodies may carry distinct diameters. The complete ungrouped operation set uses the same ownership rule. The complete `HOLE PACKAGE` operation set uses that ownership and bore-bijection rule without a simple-hole template. Complete output-body-owned pairs of coaxial conical boundary faces supply one common entry-and-exit chamfer diameter and included angle per body. Each neutral simple-hole feature owns its exact native template, redundantly witnessed scalar lane, and resolved construction-block references through source properties; model-space placement remains unresolved. `HOLE PACKAGE` retains unresolved form and placement fields. `RIB` and `CHAMFER` project as typed feature families with unresolved operands and dimensional fields. `Pattern Feature` and `Pattern Geometry` project as typed patterns with unresolved seed and transform fields. `SKETCH` operations project as ordered planar sketch history nodes and retain a typed sketch record joining the exact operation record to its resolved native input blocks in header-slot order. Named points bind through exact shared blocks or a unique same-store predecessor span; solved sketch planes, entities, profiles, and constraints remain unresolved. Segment tuples bind both serialized object-index aliases of each partition or plain cached-body image; either alias resolves feature selections and outputs to neutral body IDs when the image remains in the transferred model. Other named operations project as native features until their inputs are complete. Typed numeric expressions retain object identity, owning OM record, name, exact expression text, declared millimeter or degree unit, ordered exact-name parameter dependencies, and evaluated values for context-free and acyclic same-unit dependency arithmetic. Named arrangements retain ordered configuration names and default state. Exact agreement between the default arrangement and the part-level active-arrangement attribute assigns every selected current body to that active configuration; inactive configuration body states remain unresolved. Complete intermediate-body relations and other operation semantics remain open.
- **Product structure: Inspect.** Indexed EXTREFSTREAM handle-set records retain their record IDs, declared counts, four raw ID slots, normalized persistent handles, closing-duplicate form, opaque-tail lengths, and exact byte offsets. Equal persistent handles create explicit cross-stream identity edges between EXTREFSTREAM records and bounded OM records. End-anchored child-part names and paths transfer as ordered typed native records and also appear in source dependency metadata. The handle-to-child binding, assembly graph instances, placements, and constraints remain open.
- **Presentation and metadata: Partial.** Length-bounded Parasolid attribute-class declarations retain stream-local class and field-record identities, exact printable class name, declared field count, typed primary storage, ordered catalog references, variant header words, and inflated-stream offset. Topology records retain non-null attribute-list ownership. Framed type-81 lists, type-82 integer lanes, type-83 finite binary64 lanes, and type-84 printable values retain exact identities, ordered reference slots, revision records, and byte spans. A type-81 class discriminator resolves to its exact same-stream type-79 definition XMT. Unique topology-to-list and list-to-value chains transfer ordered integer, finite-double, and printable-string lanes to shell, face, loop, edge, coedge, and vertex source attributes with class-qualified family-and-ordinal names. Class-specific field assignment, colors, materials, and other presentation semantics remain unresolved.

### Write and round trip

- **Native write: None.**
- **Round trip: None.**

Open geometry gates include unresolved procedural-intersection branches, freeform NURBS-offset blend spines, and other unsupported record families. Open structural gates include unmatched tombstones, multi-partition feature composition, assembly records, and NX object-model field serialization.

See [`formats/siemens_nx.md`](formats/siemens_nx.md) and [`formats/siemens_nx-open-items.md`](formats/siemens_nx-open-items.md).

## CATIA V5 `.CATPart`

**Kernel:** CGM

**Ladder: L2 claimed (standard-nested band); L1 claimed (other layout bands).** L3 requires connected topology across the band. Current topology depends on resolved trim, support, and endpoint assignments.

### Read profile

- **Container and versions: Partial.** The codec decodes `V5_CFV2` containers and distinguishes standard-nested, FBB-only, zero-entity, float-packed, E5, and inner-without-directory layouts.
- **Geometry: Partial.** Standard-nested files transfer vertices, planes when their bridge records resolve, curved analytic surfaces, and supported edge curves. Other layouts transfer subsets of analytic or freeform carriers.
- **Topology: Partial.** Standard-nested files can emit a connected body, shell, face, loop, coedge, edge, and vertex graph when trim, support, and endpoint assignment resolve. Other parsed topology families remain disconnected from the common IR.
- **Tessellation: None.**
- **Design intent: None.**
- **Product structure: None.**
- **Presentation and metadata: None.** Persistent tags, attributes, materials, and appearance bindings remain outside the IR.

### Write and round trip

- **Native write: None.**
- **Round trip: None.**

Open gates include endpoint incidence for additional variants, orientation signs, pcurve attachment, spline edge curves, persistent tags, attributes, and the consolidated-stream tag resolver.

See [`formats/catia.md`](formats/catia.md) and [`formats/catia-open-items.md`](formats/catia-open-items.md).

## Creo Parametric `.prt`

**Kernel:** Granite, serialized through PSB

**Ladder: L1 claimed.** Prototype geometry lacks model-space placement required by L2. Derived datum planes and the geometry census exceed the L1 gates.

### Read profile

- **Container and versions: Partial.** The codec detects `#UGC:2`, enumerates sections, identifies ND and DEPDB layouts, and decodes supported PSB compact integers and floats.
- **Geometry: Partial.** ActDatums plane outlines transfer as derived plane carriers. VisibGeom surfaces and curves remain unplaced prototype records.
- **Topology: None.** Scanning identifies prototype surface rows, half-edges, and loops. Placed body topology remains outside the IR.
- **Tessellation: None.**
- **Design intent: None.**
- **Product structure: None.**
- **Presentation and metadata: Partial.** Container attributes and geometry censuses transfer as source metadata. Features, materials, and display data remain open.

`geometry_transferred` is true only when datum-plane carriers transfer. VisibGeom-only files report no transferred model geometry.

### Write and round trip

- **Native write: None.**
- **Round trip: None.**

The principal geometry gate is the unresolved general 8-byte PSB float-token formula needed to place prototype geometry in model space.

See [`formats/creo_prt.md`](formats/creo_prt.md) and [`formats/creo_prt-open-items.md`](formats/creo_prt-open-items.md).

## STEP AP214 export

The pure-Rust `cadmpeg-step` crate writes ISO 10303-21 AP214.

- **Geometry: Partial.** Planes, cylinders, cones, spheres, tori, lines, circles, ellipses, and rational or non-rational B-spline carriers map to STEP entities.
- **Topology: Partial.** Supported bodies emit a solid, shell, face, loop, edge, and vertex hierarchy. Export reports losses for omitted faces with unknown surfaces and curveless edges. Shell closure and manifold validity remain unchecked. Export reports non-identity body transforms and leaves coordinates in body-local space.
- **Procedural geometry: Solved carriers only.** Source-native procedural definitions reduce to their analytic or NURBS carriers and produce an informational loss.
- **Tessellation: None.**
- **Product structure: None.**
- **Design intent: None.** STEP output excludes feature histories, sketches, construction recipes, Design records, and ACT records.
- **Presentation and metadata: None.** STEP output excludes colors, appearance assets, bindings, source attributes, and opaque records.
- **Loss reporting: Partial.** Export reports omitted, reduced, or normalized IR content. The roadmap also requires preserved, mapped, solved, and lost outcomes.

## Maintaining these profiles

Per-format specifications in [`formats/`](formats/) define byte semantics. Adjacent `*-open-items.md` files contain unresolved fields and structures.

Support profiles describe repository behavior only. A profile changes when code and tests land, and every **Partial** domain must identify its remaining gates here or in the linked open-items document. Claims move to **Complete** only after satisfying the roadmap's public evidence and reliability gates.

Ladder scores change only when a per-gate review confirms every gate at the new level and below. A score's headline names the failing gate of the next level. Evidence words move independently of levels: **tested** requires fixtures exercising the scored gates, **proven** requires the roadmap's progress gates.
