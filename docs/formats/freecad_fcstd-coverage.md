# FCStd coverage matrix

The primary profile is ZIP-packaged `SchemaVersion=4`, `FileVersion=1`. Each row requires an
independently authored, redistribution-cleared public fixture and the listed machine assertion.
Legacy schemas 2 and 3 are refusal profiles until their rows acquire explicit support.

| Gate | Fixture classes | Machine assertion |
| --- | --- | --- |
| L0 | stored, deflated, ZIP64, data descriptor; GUI/headless; thumbnail present/absent | detection identifies FCStd markers; inspection reports versions, kind, metadata, entry framing, and preview; the physical ledger closes |
| L1 | core and extension objects; embedded and external assets; unknown property; string table and element map present/absent | every object/property/asset has stable identity and owner; links retain order; every logical ledger closes; unsupported bands are named |
| L2 | analytic, Bezier, NURBS, trimmed, offset, swept, revolved, degenerate; text and binary B-rep; mesh and points | every required carrier is typed; native values and canonical units agree; representative evaluations meet declared tolerance |
| L3 | solid, sheet, wire, compound, compsolid, multi-shell, void, seam, degenerate, non-manifold | topology is connected and validates; placements apply once; persistent names resolve only when carried |
| L4 | attached sketch, dimensional parameters, additive feature, subtractive or dress-up feature | ordered history, dependencies, feature operands, state, and saved-result cross-links are complete |
| L5 | every carrier and topology branch in the generated census; object/body/face/edge/point appearance | census has no unknown required carrier; exact topology validates without healing; appearance ownership and precedence validate |
| L6 | all Sketcher geometry/constraint branches; expressions; spreadsheet; core Part and PartDesign operations | constraint, expression, cell, and operation graphs are typed and valid; design-domain loss report is empty |
| L7 | groups, parts, nested links, link arrays, external links, assemblies, joints | components and occurrences have distinct identity; local/world placements, persistent operands, missing externals, and cycles validate |
| L8 | GUI state, TechDraw, annotations, embedded files, Mesh, Points, FEM, CAM, Python-backed and extension data | presentation/drawing graphs validate; every application record is typed or named-retained; no payload executes; all physical/logical ledgers close |

Every gate also includes deterministic malformed-input cases for truncation, invalid counts and
indices, duplicate identities, missing owners, resource limits, and unsupported layout dispatch.

## Implementation checklist

Apply this boundary to every phase:

- Commit no analysis-only checkout file, fixture, excerpt, path, source name, symbol name,
  citation, or evidence statement.
- Transcribe no implementation or test structure. State format rules in cadmpeg terminology and
  derive original tests from repository-owned rules and authorized fixtures.
- Keep specifications limited to settled byte semantics and invariants; place only genuine layout
  unknowns in the open-items file.
- Keep every contribution self-contained and reproducible after all analysis-only resources are
  removed.
