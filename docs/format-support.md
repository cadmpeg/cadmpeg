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
| Fusion 360 `.f3d`                          | **L4 tested**  | native replay + patch + broad source-less generation, procedural carriers, ACT/Design/history records |
| SolidWorks `.sldprt`                       | **L3 tested**  | feature metadata and input lanes, tessellation, native replay + bounded generation                    |
| CATIA V5 `.CATPart` (standard-nested band) | **L2 claimed** | conditionally connected B-rep                                                                         |
| Siemens NX `.prt`                          | **L2 claimed** | conditional connected B-rep, external-dependency inspection                                           |
| CATIA V5 `.CATPart` (other layout bands)   | **L1 claimed** |                                                                                                       |
| Creo Parametric `.prt`                     | **L1 claimed** | derived datum planes, prototype geometry census                                                       |
| STEP AP214                                 | translation    | partial B-rep export with explicit loss reporting                                                     |

Each current score applies to the envelope described in its profile. The codecs have yet to declare explicit version bands.

## Status terms

- **None:** the repository lacks an implementation for the domain.
- **Inspect:** cadmpeg identifies and reports the structure without transferring it into typed IR.
- **Partial:** cadmpeg transfers a typed subset and reports or preserves the remainder.
- **Complete:** the domain satisfies the public-fixture, byte-accounting, validation, round-trip, and fuzzing gates in the [roadmap](roadmap.md#progress-gates).

Every current profile contains incomplete domains. Current claims rely on code, generated fixtures, and explicit loss paths while the public corpus remains empty.

Entity provenance and domain status measure different properties. `byte_exact`, `derived`, `inferred`, and `unknown` describe how cadmpeg obtained one IR value.

## At a glance

- **Fusion 360 `.f3d` (L4 tested):** design records, partial B-rep and appearance reads, byte-exact replay, native patching, and source-less generation.
- **SolidWorks `.sldprt` (L3 tested):** connected model reads, feature metadata, native writes, and round trips.
- **CATIA V5 `.CATPart` (L2 claimed for the standard-nested band):** exact carriers and conditionally connected topology. Other layout bands score L1. Read only.
- **Siemens NX `.prt` (L2 claimed):** exact carriers and conditionally connected topology. Read only.
- **Creo Parametric `.prt` (L1 claimed):** container navigation, derived datum planes, and prototype geometry inspection. Read only.
- **STEP AP214 (translation):** partial B-rep export with explicit loss reporting.

## SolidWorks `.sldprt`

**Kernel:** Parasolid

**Role:** reference format for full semantic support

**Ladder: L3 tested.** Feature records contain metadata and input lanes. L4 requires operation semantics.

### Read profile

- **Container and versions: Partial.** The codec validates CRC-framed blocks, enumerates cache cells and the tail directory, extracts active Parasolid partitions, and preserves the source image. Coverage across historical schemas remains incomplete.
- **Geometry: Partial.** Analytic and NURBS surfaces and curves transfer into typed carriers. Offset, swept, blend, intersection, and other unsupported families remain opaque or produce unknown carriers.
- **Topology: Partial.** The codec builds body, lump, shell, face, loop, coedge, edge, and vertex ownership for supported layouts. Periodic seams, orientation, and several pcurves are derived. Older body layouts, schema-specific sheet classification, deltas tombstones, and some multi-shell cases remain open.
- **Tessellation: Partial.** Display-list geometry transfers into tessellation arenas and can be regenerated. Stable face-to-triangle ownership remains open.
- **Design intent: Partial.** Configuration names, feature-history metadata, and typed feature-input lanes transfer. Replayable SolidWorks feature trees and alternate-configuration solids remain open.
- **Product structure: None.** `.sldprt` support covers parts only.
- **Presentation and metadata: Partial.** Base colors, appearance bindings, previews, SolidWorks XML metadata, units, and selected attributes transfer. Full appearance precedence and all embedded metadata stores remain open.

### Write and round trip

- **Native write: Partial.** Unchanged IR with a retained source image writes byte for byte. Modified or source-less supported IR regenerates native blocks and a section directory.
- **Semantic write limits:** one lump per body, one shell per lump, no explicit face names, no stored edge parameter ranges, no periodic NURBS carriers, and bounded appearance data.
- **Round trip: Partial.** Byte-exact unchanged-file and semantic regeneration paths have generated-fixture tests. The public version and feature matrix remains to be built.

See [`formats/sldprt.md`](formats/sldprt.md) and [`formats/sldprt-open-items.md`](formats/sldprt-open-items.md).

## Fusion 360 `.f3d`

**Kernel:** ASM, derived from ACIS

**Ladder: L4 tested.** Unresolved procedural families and unknown carriers block L5. Native writing exceeds the scored gates.

### Read profile

- **Container and versions: Partial.** The codec selects the first `.smbh` entry, falling back to the first B-rep entry, and decodes linked Protein, Design, MetaStream, and ACT records. The authoritative relation among multiple asset folders and B-rep entries remains unresolved.
- **Geometry: Partial.** Analytic surfaces and curves, cached NURBS carriers, parameterizations, signed radii, point-degenerate curves, helix, vector-offset, and subset constructions, and supported procedural definitions transfer. Law, taper, loft, skin, net, sweep, variable-blend, and related families remain incomplete when no solved cache resolves.
- **Topology: Partial.** Shell-reachable solid, sheet, wire, and mixed-dimensional general bodies transfer with shells, faces, loops, coedges, wire edges, edges, and vertices. Closed edge incidence classifies solid bodies, open face incidence classifies sheet bodies, face-less wire membership classifies wire bodies, and bodies containing both faces and wire edges classify as general. Unsupported surface records retain topology with unknown geometry; free vertices, some procedural edges, and some explicit pcurves remain unresolved.
- **Tessellation: None.** Fusion display meshes remain outside the IR tessellation arena.
- **Design intent: Partial.** ASM history states, Design assignments, sketch-side records, construction recipes, persistent references, MetaStream identities, and ACT channels transfer. A complete replayable Fusion feature history remains open.
- **Product structure: Partial.** Body transforms and root-component records transfer. Multi-component structure and constraints remain open.
- **Presentation and metadata: Partial.** Linked source attributes, Protein appearance assets, material properties, and body bindings transfer. External material-library display names and some schema fields remain unresolved.

### Write and round trip

- **Native write: Partial.** An unchanged retained source archive writes byte for byte. The writer patches model points, common analytic and NURBS B-rep curves and surfaces, rational/non-rational pcurves, procedural caches, sketch geometry, constraints, history fields, design records, and supported appearance properties in their original records. Source-less generation writes multi-body B-reps containing solid, sheet, wire, and mixed-dimensional general topology with analytic or rational/non-rational NURBS carriers, helix and vector-offset curves, translational-extrusion and rolling-ball-blend definitions with solved caches, inline rational/non-rational NURBS pcurves, placements, document tolerance metadata, direct body/face color attributes, body/face/edge persistent-design tags, coedge-to-sketch provenance, typed ASM history, Design object and construction streams, sketch geometry and constraints, ACT tables and component roots, and Protein appearances with body bindings.
- **Write limits:** General writing requires a retained source archive and the original entity and record layouts. Source-less generation supports multiple placed bodies, regions, and shells with plane, cylinder, cone, sphere, torus, or rational/non-rational NURBS faces; multiple loops; shared radial edges; line, circle, ellipse, point-degenerate, or rational/non-rational NURBS edge curves; inline rational/non-rational NURBS pcurves; face-less wire bodies with chained regions and shells; and mixed face-and-wire shells with one or more wire edges. Free vertices and edits outside the listed fields are rejected.
- **Procedural write invariant:** One solved curve has at most one procedural construction. Helix, vector-offset, and subset definitions serialize with their native construction fields, child curves, role fields, and solved caches. Typed intersection, projection, scalar/two-sided offset, and blend-spine definitions are rejected by source-less writing until their construction tails can be emitted without semantic loss; they are never reduced silently to cache-only curves.
- **Round trip: Partial.** Generated byte fixtures cover byte-exact replay and each writable geometry, history, Design, sketch, ACT, and appearance family.

See [`formats/f3d.md`](formats/f3d.md) and [`formats/f3d-open-items.md`](formats/f3d-open-items.md).

## Siemens NX `.prt`

**Kernel:** Parasolid in an SPLMSSTR container

**Ladder: L2 claimed.** L3 requires topology across the band. Current topology depends on fixed-record framing and resolved references, while partition-to-deltas tombstones block live-face selection.

### Read profile

- **Container and versions: Partial.** The codec decodes the SPLMSSTR directory and extracts and classifies embedded Parasolid partition, deltas, and related streams.
- **Geometry: Partial.** Points, analytic surfaces and curves, typed B-spline surfaces and curves, and supported type-133 trimmed curves transfer into IR.
- **Topology: Partial.** The body, shell, face, loop, fin, edge, and vertex graph attaches when fixed-record framing and references resolve. The active live-face set remains blocked on unresolved partition-to-deltas tombstones for other files.
- **Tessellation: None.**
- **Design intent: None.**
- **Product structure: Inspect.** The codec reports external part dependencies. Assembly graph instances, placements, and constraints remain open.
- **Presentation and metadata: None.**

### Write and round trip

- **Native write: None.**
- **Round trip: None.**

Open geometry gates include rolling-ball and procedural blends, type-137 surface curves, freeform NURBS-offset blend spines, and other unsupported record families. Open structural gates include tombstone-to-live-face selection, assembly records, and NX object-model serialization.

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
