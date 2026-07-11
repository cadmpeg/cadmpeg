# Format support

This document reports what the current repository can read, write, and round-trip. It separates semantic domains because container decoding, geometry, topology, design intent, presentation, and native writing progress independently.

No native format is complete. SolidWorks `.sldprt` has the broadest read/write path and is the [reference format](roadmap.md#reference-format-solidworks-sldprt) for full semantic support.

## Status terms

- **None:** the repository does not implement the domain.
- **Inspect:** cadmpeg identifies and reports the structure but does not transfer it into typed IR.
- **Partial:** cadmpeg transfers a typed subset and reports or preserves the remainder.
- **Complete:** the domain satisfies the public-fixture, byte-accounting, validation, round-trip, and fuzzing gates in the [roadmap](roadmap.md#progress-gates).

No profile below uses **Complete** yet. The public corpus starts empty, so current claims rest on code, generated fixtures, and explicit loss paths.

Entity provenance is separate from domain status. `byte_exact`, `derived`, `inferred`, and `unknown` describe how one IR value was obtained; they do not imply complete format support.

## At a glance

- **SolidWorks `.sldprt`:** partial semantic read, native write, and round-trip support.
- **Fusion 360 `.f3d`:** partial B-rep, design-record, and appearance read support; byte-exact replay and selected native edits.
- **Siemens NX `.prt`:** partial analytic, NURBS, trimmed-curve, and conditional topology read support; no native write.
- **CATIA V5 `.CATPart`:** partial analytic and freeform carrier decode with conditional standard-nested topology; no native write.
- **Creo Parametric `.prt`:** container and prototype-structure decode with derived datum-plane carriers; no placed model B-rep or native write.
- **STEP AP214:** partial B-rep export with explicit loss reporting.

## SolidWorks `.sldprt`

**Kernel:** Parasolid

**Role:** reference format for full semantic support

### Read profile

- **Container and versions: Partial.** The codec validates CRC-framed blocks, enumerates cache cells and the tail directory, extracts active Parasolid partitions, and preserves the source image. Coverage across historical schemas remains incomplete.
- **Geometry: Partial.** Analytic and NURBS surfaces and curves transfer into typed carriers. Offset, swept, blend, intersection, and other unsupported families remain opaque or produce unknown carriers.
- **Topology: Partial.** The codec builds body, lump, shell, face, loop, coedge, edge, and vertex ownership for supported layouts. Periodic seams, orientation, and several pcurves are derived. Older body layouts, schema-specific sheet classification, deltas tombstones, and some multi-shell cases remain open.
- **Tessellation: Partial.** Display-list geometry transfers into tessellation arenas and can be regenerated. Stable face-to-triangle ownership is not complete.
- **Design intent: Partial.** Configuration names, feature-history metadata, and typed feature-input lanes transfer. The codec does not reconstruct a replayable SolidWorks feature tree or alternate-configuration solids.
- **Product structure: None.** `.sldprt` part support does not include SolidWorks assembly documents or constraints.
- **Presentation and metadata: Partial.** Base colors, appearance bindings, previews, SolidWorks XML metadata, units, and selected attributes transfer. Full appearance precedence and all embedded metadata stores remain open.

### Write and round trip

- **Native write: Partial.** Unchanged IR with a retained source image writes byte for byte. Modified or source-less supported IR regenerates native blocks and a section directory.
- **Semantic write limits:** one lump per body, one shell per lump, no explicit face names, no stored edge parameter ranges, no periodic NURBS carriers, and bounded appearance data.
- **Round trip: Partial.** Byte-exact unchanged-file and semantic regeneration paths have generated-fixture tests. The public version and feature matrix remains to be built.

See [`formats/sldprt.md`](formats/sldprt.md) and [`formats/sldprt-open-items.md`](formats/sldprt-open-items.md).

## Fusion 360 `.f3d`

**Kernel:** ASM, derived from ACIS

### Read profile

- **Container and versions: Partial.** The codec selects the first `.smbh` entry, falling back to the first B-rep entry, and decodes linked Protein, Design, MetaStream, and ACT records. The authoritative relation among multiple asset folders and B-rep entries remains unresolved.
- **Geometry: Partial.** Analytic surfaces and curves, cached NURBS carriers, parameterizations, signed radii, and supported procedural definitions transfer. Law, taper, loft, skin, net, sweep, helix, variable-blend, and related families remain incomplete when no solved cache resolves.
- **Topology: Partial.** Shell-reachable bodies, shells, faces, loops, coedges, edges, and vertices transfer. Unsupported surface records retain topology with unknown geometry; some procedural edges lack curve carriers and some explicit pcurves remain unresolved.
- **Tessellation: None.** The codec does not transfer Fusion display meshes into the IR tessellation arena.
- **Design intent: Partial.** ASM history states, Design assignments, sketch-side records, construction recipes, persistent references, MetaStream identities, and ACT channels transfer. They do not yet form a complete replayable Fusion feature history.
- **Product structure: Partial.** Body transforms and root-component records transfer. Complete multi-component assembly structure and constraints do not.
- **Presentation and metadata: Partial.** Linked source attributes, Protein appearance assets, material properties, and body bindings transfer. External material-library display names and some schema fields remain unresolved.

### Write and round trip

- **Native write: Partial.** An unchanged retained source archive writes byte for byte. The writer patches model points, common analytic and NURBS B-rep curves and surfaces, pcurves, procedural caches, sketch geometry, constraints, history fields, design records, and supported appearance properties in their original records. Source-less generation writes multi-body B-reps with analytic or rational/non-rational NURBS carriers, inline non-rational NURBS pcurves, placements, and typed ASM history streams with bulletin boards and state-local records.
- **Write limits:** General writing requires a retained source archive and the original entity and record layouts. Source-less generation supports multiple placed bodies, regions, and shells with plane, cylinder, torus, or rational/non-rational NURBS faces; multiple loops; shared radial edges; line, circle, or rational/non-rational NURBS edge curves; and inline non-rational NURBS pcurves. Other analytic carriers remain limited to one-face archives. Wire topology and edits outside the listed fields are rejected.
- **Round trip: Partial.** Generated fixtures cover byte-exact replay and each writable geometry and sketch family.

See [`formats/f3d.md`](formats/f3d.md) and [`formats/f3d-open-items.md`](formats/f3d-open-items.md).

## Siemens NX `.prt`

**Kernel:** Parasolid in an SPLMSSTR container

### Read profile

- **Container and versions: Partial.** The codec decodes the SPLMSSTR directory and extracts and classifies embedded Parasolid partition, deltas, and related streams.
- **Geometry: Partial.** Points, analytic surfaces and curves, typed B-spline surfaces and curves, and supported type-133 trimmed curves transfer into IR.
- **Topology: Partial.** The body, shell, face, loop, fin, edge, and vertex graph attaches when fixed-record framing and references resolve. The active live-face set remains blocked on unresolved partition-to-deltas tombstones for other files.
- **Tessellation: None.**
- **Design intent: None.**
- **Product structure: Inspect.** External part dependencies are detected and reported, but instances, placements, and constraints do not transfer as an assembly graph.
- **Presentation and metadata: None.**

### Write and round trip

- **Native write: None.**
- **Round trip: None.**

Open geometry gates include rolling-ball and procedural blends, type-137 surface curves, freeform NURBS-offset blend spines, and other unsupported record families. Open structural gates include tombstone-to-live-face selection, assembly records, and NX object-model serialization.

See [`formats/siemens_nx.md`](formats/siemens_nx.md) and [`formats/siemens_nx-open-items.md`](formats/siemens_nx-open-items.md).

## CATIA V5 `.CATPart`

**Kernel:** CGM

### Read profile

- **Container and versions: Partial.** The codec decodes `V5_CFV2` containers and distinguishes standard-nested, FBB-only, zero-entity, float-packed, E5, and inner-without-directory layouts.
- **Geometry: Partial.** Standard-nested files transfer vertices, planes when their bridge records resolve, curved analytic surfaces, and supported edge curves. Other layouts transfer subsets of analytic or freeform carriers.
- **Topology: Partial.** Standard-nested files can emit a connected body, shell, face, loop, coedge, edge, and vertex graph when trim, support, and endpoint assignment resolve. Other parsed topology families are not yet connected to the common IR.
- **Tessellation: None.**
- **Design intent: None.**
- **Product structure: None.**
- **Presentation and metadata: None.** Persistent tags, attributes, materials, and appearance bindings do not transfer.

### Write and round trip

- **Native write: None.**
- **Round trip: None.**

Open gates include endpoint incidence for additional variants, orientation signs, pcurve attachment, spline edge curves, persistent tags, attributes, and the consolidated-stream tag resolver.

See [`formats/catia.md`](formats/catia.md) and [`formats/catia-open-items.md`](formats/catia-open-items.md).

## Creo Parametric `.prt`

**Kernel:** Granite, serialized through PSB

### Read profile

- **Container and versions: Partial.** The codec detects `#UGC:2`, enumerates sections, identifies ND and DEPDB layouts, and decodes supported PSB compact integers and floats.
- **Geometry: Partial.** ActDatums plane outlines transfer as derived plane carriers. VisibGeom surfaces and curves are counted and preserved as prototype records but do not transfer as placed model geometry.
- **Topology: None.** Prototype surface rows, half-edges, and loops can be identified during scanning, but no placed body topology enters the IR.
- **Tessellation: None.**
- **Design intent: None.**
- **Product structure: None.**
- **Presentation and metadata: Partial.** Container attributes and geometry censuses transfer as source metadata; features, materials, and display data do not.

`geometry_transferred` is true only when datum-plane carriers transfer. VisibGeom-only files report no transferred model geometry.

### Write and round trip

- **Native write: None.**
- **Round trip: None.**

The principal geometry gate is the unresolved general 8-byte PSB float-token formula needed to place prototype geometry in model space.

See [`formats/creo_prt.md`](formats/creo_prt.md) and [`formats/creo_prt-open-items.md`](formats/creo_prt-open-items.md).

## STEP AP214 export

The pure-Rust `cadmpeg-step` crate writes ISO 10303-21 AP214.

- **Geometry: Partial.** Planes, cylinders, cones, spheres, tori, lines, circles, ellipses, and rational or non-rational B-spline carriers map to STEP entities.
- **Topology: Partial.** Supported bodies emit a solid, shell, face, loop, edge, and vertex hierarchy. Faces with unknown surfaces and curveless edges are omitted with losses. The writer does not establish shell closure or manifold validity. Non-identity body transforms are reported and coordinates remain in body-local space.
- **Procedural geometry: Solved carriers only.** Source-native procedural definitions reduce to their analytic or NURBS carriers and produce an informational loss.
- **Tessellation: None.**
- **Product structure: None.**
- **Design intent: None.** Feature histories, sketches, construction recipes, Design records, and ACT records are not represented.
- **Presentation and metadata: None.** Colors, appearance assets, bindings, source attributes, and opaque records are not written.
- **Loss reporting: Partial.** Export reports omitted, reduced, or normalized IR content. It does not yet expose the roadmap's full preserved, mapped, solved, and lost outcome model.

## Maintaining these profiles

Per-format specifications in [`formats/`](formats/) define byte semantics. Adjacent `*-open-items.md` files contain unresolved fields and structures.

Support profiles describe repository behavior only. A profile changes when code and tests land, and every **Partial** domain must identify its remaining gates here or in the linked open-items document. Claims move to **Complete** only after satisfying the roadmap's public evidence and reliability gates.
