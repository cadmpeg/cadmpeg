# Format support matrix

This matrix rates what cadmpeg can decode and export on a shared fidelity ladder. When research coverage exceeds the in-repo codec, the matrix states both statuses.

**Repository reality check:** the Rust workspace includes codecs for every researched format:

- **`.f3d` B-rep:** topology graph, analytic and cached NURBS carriers, inline/reference pcurves, transforms, attributes, and material bindings.
- **`.sldprt` analytic B-rep:** single synthetic shell, opaque B-splines.
- **`.CATPart` carriers:** standard-nested vertex cloud plus curved analytic surfaces, no topology.
- **NX `.prt` B-rep:** container, analytic carriers, and a reconstructed topology graph where the stream's records resolve.
- **Creo `.prt` structure:** container plus PSB tokens, no transferred geometry.

The repository exports **STEP AP214** through a pure-Rust writer. "Research" means demonstrated outside this repository.

---

## The L0–L6 fidelity ladder

A format's status is the highest level it reaches with byte-derived confidence.

| Level  | Name                | What it means                                                                                                              |
| ------ | ------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| **L0** | Container           | The on-disk wrapper is decoded: streams, blocks, entity tables, sizes, offsets are enumerated. You can `inspect` the file. |
| **L1** | Mesh                | Tessellated/faceted geometry is recovered (triangles, not analytic surfaces). Enough to visualize.                         |
| **L2** | Analytic geometry   | Exact surfaces and curves are recovered (planes, cylinders, splines) rather than their tessellation.                       |
| **L3** | Exact topology      | The B-rep graph is recovered: bodies, shells, faces, loops, coedges, edges, vertices, correctly connected.                 |
| **L4** | Assemblies          | Multi-part structure, instances, and their placement transforms are recovered.                                             |
| **L5** | Parametric features | Feature history / construction intent (sketches, extrudes, fillets as operations) is recovered.                            |
| **L6** | Native write        | cadmpeg can _write_ the format back.                                                                                       |

Export targets are rated on the same ladder for what they can _emit_.

---

## Input format status

| Format          | Ext        | Kernel                         | Reached                                                    | Status                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| --------------- | ---------- | ------------------------------ | ---------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Fusion 360      | `.f3d`     | ASM (ACIS-derived)             | **L3** (research) / **L3** (in-repo)                       | In-repo: the active SAB stream decodes into the full B-rep graph with analytic and cached NURBS carriers. Subtype references, inline/ref pcurves, signed radii, body transforms, linked attributes, Protein appearance assets, Design assignments, MetaStream entity ids, and ACT channels transfer into IR. See [`formats/f3d.md`](formats/f3d.md) and [`formats/f3d-open-items.md`](formats/f3d-open-items.md).                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| SolidWorks      | `.sldprt`  | Parasolid                      | **L2–L3 partial; L6 partial**                              | In-repo: CRC-validated framing; analytic and NURBS carriers; explicit solid/sheet body ownership; derived periodic seams and pcurves; tessellation, appearance, metadata, configurations, and feature history. Unchanged files write byte-exactly. Source-less and modified typed IR regenerate native blocks and a section directory. Unsupported surface families and unresolved schema-33103 sheet classification remain open. See [`formats/sldprt.md`](formats/sldprt.md) and [`formats/sldprt-open-items.md`](formats/sldprt-open-items.md).                                                                                                                                                                                                                                                                                                                                     |
| Creo Parametric | `.prt`     | Granite (PSB on disk)          | **L0** (research) / **L0** (in-repo)                       | In-repo: `#UGC:2` detection, section enumeration, ND/DEPDB layout identification, PSB compact-int and compact-float decoding, and VisibGeom surface/curve censuses. `geometry_transferred=false`: VisibGeom stores prototype geometry, so cadmpeg preserves it as unknown passthroughs rather than presenting it as placed model geometry. Research identifies the format as PSB, covers the container and prototype rows, and leaves per-instance geometry blocked by the unresolved 8-byte PSB float-token formula. See [`formats/creo_prt.md`](formats/creo_prt.md) and [`formats/creo_prt-open-items.md`](formats/creo_prt-open-items.md).                                                                                                                                                                                                                                         |
| Siemens NX      | `.prt`     | Parasolid (SPLMSSTR container) | **L2–L3 partial** (research) / **L2–L3 partial** (in-repo) | In-repo: the SPLMSSTR container decodes, embedded Parasolid streams are extracted and classified, and points plus analytic surfaces/curves decode as carriers validated against paired STEP exports. Where a stream's topology records resolve, the body→shell→face→loop→fin→edge→vertex graph is reconstructed and attached; a stream yielding no topology is a counted loss. Loss-reported gaps: untyped B-spline/blend records and assembly files as external dependencies. Research covers fixed record grammar, topology layouts, analytic carriers, and procedural rolling-ball blends. Open gates: tombstone-to-live-face selection, assembly/constraint records, freeform NURBS-offset blend spines, and NX object-model field serialization. See [`formats/siemens_nx.md`](formats/siemens_nx.md) and [`formats/siemens_nx-open-items.md`](formats/siemens_nx-open-items.md). |
| CATIA V5        | `.CATPart` | CGM (proprietary, unpublished) | **L2–L3 partial** (research) / **L1–L2 partial** (in-repo) | In-repo: the `V5_CFV2` container and inner stream directory decode, all five storage variants are detected, and the standard-nested variant emits a vertex point cloud plus curved analytic surface carriers. Loss-reported gaps: no topology graph, located-but-undecoded plane carriers, and detect-only support for the other four variants. Research covers the container, five storage variants, analytic surfaces/curves, face meshes, and much of the topology. Open gates: endpoint incidence, orientation signs, persistent tags, and the consolidated-stream tag resolver. See [`formats/catia.md`](formats/catia.md) and [`formats/catia-open-items.md`](formats/catia-open-items.md).                                                                                                                                                                                      |

Per-format specifications and open-item notes live in [`formats/`](formats/).

---

## Export target status

| Target | Ext     | Level it can emit           | Status                                                                                                                                                                                                    |
| ------ | ------- | --------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| STEP   | `.step` | L2–L3 (analytic + topology) | **Working (partial).** Pure-Rust AP214 writer (`cadmpeg-step`): manifold B-rep with analytic and B-spline carriers, explicit loss report for anything unrepresentable. No presentation/color mapping yet. |

---

## How to read a status

- **"decoded" / "mapped"**: byte-derived and reproducible against test files.
- **"partial"**: some of the level is reached but a named gate blocks the rest.
- **"planned"**: designed and on the roadmap, not implemented.
- **"research"**: demonstrated in the research effort behind cadmpeg; not yet landed in this repository.

This matrix is conservative; if the code does not back up a claim here, open an issue.
