# IGES open items

This document lists the parts of the IGES format that we do not know. The specification `iges.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

Status requirements for resource-bounded decode, valid semantic output,
complete transfer accounting, semantic writing, target selection, independent
application acceptance, and writer stress are settled in the IGES
specification and support profile. They are not open format items.

# Unrecorded format rules

The items below record decode and write rules that the codec applies and that
neither IGES nor `iges.md` states. They come from a directed sweep of the codec
on 2026-08-08. Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now, with the code that depends on it.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and
  the code. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Use the identifier in commit messages and in code comments. This document uses
ASD-STE100 Simplified Technical English. Record names, field names, and token
values are technical names. They keep their source spelling.

Many items have one shape: the codec refuses an entity because a value misses a
threshold that the codec selected, or because a field is blank where the codec
requires an explicit value. A refusal is not a safe default. It removes geometry
from a conformant file.



## 1. Physical representation and binary framing

### BIN-01. Binary envelope fixture coverage

**Question.** Which committed fixtures establish the Binary representation envelope?

**Known.** The golden harness pins `FIXTURE_EXTENSION=igs` only (`golden_tests.rs:23`). No committed fixture exercises the Binary Flag, section, control-byte, or null-padding envelope. Binary coverage is unit tests in `binary.rs` only. `format-support.md` calls the Binary representation structurally validated.

**Need.** Binary producer files or generated Binary fixtures must establish the envelope before the validated claim is settled.

## 2. Global metadata

## 3. Directory fields, the reference graph, and the native arenas

### DIR-01. Unreadable Directory interpretation fields

**Question.** What disposition applies when a Directory interpretation field is unreadable?

**Known.** The decoder charges `iges/directory.metadata-unreadable` when line font, level, view, label display, line weight, color, or subscript is unreadable (`directory.rs:271`). Interpretation fields and exact source cards remain available for a specimen.

**Need.** Producer documentation or controlled files must establish whether an unreadable field means a default, a null, or a refusal.

## 4. Geometry carriers and tolerances

## 5. Surfaces and topology

### BND-01. Boundary pcurve outside support domain

**Question.** What disposition applies when a boundary parameter curve leaves the finite parameter domain of its support surface?

**Known.** The decoder charges `iges/topology.boundary-pcurve-outside-support-domain` on both paths (`trimming.rs:2644,2657`). An authoritative pcurve outside the declared support bounds fails the boundary. An alternate pcurve outside the bounds retains the model-space curve.

**Need.** Producer files with out-of-domain pcurves must establish the correct disposition for each path.

### NUR-01. Non-finite NURBS transform result

**Question.** What disposition applies when a NURBS coordinate or parameter transformation produces a non-finite value?

**Known.** The decoder charges `iges/geometry.nurbs-transform-non-finite` and skips the rail when reversed second-rail knots are non-finite (`surfaces.rs:1619`).

**Need.** A source file that produces a non-finite knot under transformation must establish whether the rail is skipped or the record is refused.

## 6. Product structure, annotation, and presentation

### OCC-01. Product occurrence expansion limits

**Question.** What output and nesting limits bound product occurrence expansion?

**Known.** The decoder stops expansion at a configured output limit and charges `iges/occurrence.expansion-output-truncated` (`reader.rs:507`). It stops at a configured nesting-depth limit and charges `iges/occurrence.expansion-depth-truncated` (`reader.rs:516`).

**Need.** The largest honest product structure in producer files must establish limits that admit it, or a source rule must bound nesting.

### ATT-01. Unstatable attribute table count

**Question.** What disposition applies when a Type 422 attribute table instance states a count that no row grid supports?

**Known.** The decoder charges `iges/parameter.attribute-table-count-unstatable` and skips the attribute rows (`reader.rs:577`). `iges.md` states the Type 322 and Type 422 layout.

**Need.** Files with degenerate attribute counts must establish the correct disposition.

### DIS-01. Unprojected display data

**Question.** Which display data has no neutral projection, and what disposition applies to it?

**Known.** The decoder charges `iges/presentation.display-data-not-projected` for an invalid Type 116 display symbol pointer, for incomplete or non-finite Type 125 flash shape parameters, and for a Type 125 Form 0 record with no defining entity pointer (`geometry.rs:1812,1918,1926`).

**Need.** Producer display data variants must establish the projection for each valid form and the disposition for each defect.

### BNA-01. Conflicting body names

**Question.** Which name applies when one body owner has conflicting valid Type 406 Form 15 names?

**Known.** The decoder charges `iges/presentation.body-name-ambiguous` and leaves the body unnamed (`presentation.rs:665`). `iges.md` states that Form 15 attaches one name.

**Need.** A file with conflicting valid names must establish the correct selection rule.

## 7. Write path

### PRO-01. Procedural reduction in the writer

**Question.** Which procedural definitions reduce to solved carriers, and which refuse the write?

**Known.** The semantic writer reduces procedural surface and curve definitions to writable solved carriers and charges `iges/geometry.procedural-reduced` (`writer.rs:1118`).

**Need.** Neutral documents with procedural definitions must establish the reduction boundary.

### EDG-01. Sampled edge agreement and fitting

**Question.** What sample count and residual bound establish recovered edge agreement and B-spline fits?

**Known.** The writer compares recovered coedge curves at five parameters and charges `iges/geometry.edge-pcurve-agreement-sampled` for the sampled comparison (`writer/target.rs:90`). It fits recovered edges as B-splines and charges `iges/geometry.edge-pcurve-sampled-to-bspline` with a sampled residual bound (`writer/target.rs:96`).

**Need.** A tolerance basis must establish the sample count and the residual limit.

### SRC-01. Unavailable preserved source image

**Question.** What disposition applies when the preserved source image for byte-exact replay is unavailable?

**Known.** The writer declines replay and regenerates semantically; the export report's synthesis cause states that the source fidelity holds no IGES source image (`writer/target.rs`). `iges.md` states the replay rule for an intact source baseline.

**Need.** Source-less edit workflows must establish when regeneration is accepted and when it is refused.

### PAS-01. Omitted passthrough records

**Question.** Which native arenas does the semantic writer regenerate, and which does it omit?

**Known.** The semantic writer declares the native arenas it regenerates; the export wrapper charges `export:native.records-omitted` with the record ids of every other arena, including quarantined records without writable fields. The writer declares each native entity record after it emits the file. A surface entity is regenerated when the file holds the neutral surface projected from it; the writer declares it dropped when it withholds that surface, as it withholds an analytic surface (Types 192 to 198) on no face and a support surface with no written owner. A Type 123 direction is regenerated, in its `directions` record and its entity, when a written analytic surface (Types 190 to 198) names it as an axis or reference direction, and dropped otherwise. A curve entity, Type 106 included except Forms 1 to 3, is regenerated when the file holds a neutral curve projected from it. It is dropped when the writer withholds each such curve, as it withholds a support curve with no written owner. A Type 106 form with no neutral curve, such as a witness line, a centerline or a section line, is dropped in its entity and its `copious_data` record. A Type 116 point is regenerated when the file holds its neutral point or a written analytic surface names it as its location, and Type 106 Forms 1 to 3 when the file holds a neutral point of their tuples. Otherwise each is dropped: the location of withheld analytic surfaces, a point whose neutral point the writer withholds, as it withholds a support point with no written owner, and a point with no neutral point. A Type 124 transformation is regenerated when an entity the writer keeps names it, directly or through another kept transformation, and dropped in its entity and its `transformations` record otherwise. A model with no neutral faces gets no trimmed surface, B-rep shell or solid, so the writer declares its native topology entities (Types 141 to 144, 186 and 502 to 514), `manifold_solids` and `boundary_vertex_sewing` dropped. A Directory or Parameter card in `cards` takes the disposition of the entity that holds it, and a card of no typed entity is dropped. The writer writes its own Start text, so the Start section cards are dropped, and it regenerates the Global and Terminate sections. The wrapper lists each dropped record by id.

**Need.** A complete arena inventory must establish the regenerated set and the omitted set.

### SUP-01. Withheld support and free geometry

**Question.** Which support and free geometry does the semantic writer withhold on the topology path?

**Known.** The writer withholds a standalone edge whose curve is support geometry and charges `iges/writer.support-edge-not-represented` (`writer.rs:581`). It withholds unowned free vertices and charges `iges/writer.free-geometry-omitted` (`writer.rs:496`). It withholds free edges and curves that duplicate a written carrier and charges `iges/writer.duplicate-carrier-omitted` (`writer.rs:2407,2413`). `iges.md` states the withheld-parent rule.

**Need.** Topology-path writes must establish which support and free geometry is withheld and which is written.

### OUT-01. Body presentation without Directory owner

**Question.** Which body presentation has no Directory representation?

**Known.** The writer charges `iges/writer.body-name-not-represented` when a body name cannot encode as a Type 406 Form 15 name (`writer.rs:289`). It charges `iges/writer.body-color-not-represented` when a free-geometry body has no owning Directory Entry for color (`writer.rs:452`). It charges `iges/writer.body-opacity-not-represented` when Directory color cannot carry opacity (`writer.rs:323`). It charges `iges/writer.body-visibility-not-represented` when a free-geometry body has no single Directory Entry for visibility (`writer.rs:459`).

**Need.** Body presentation variants must establish the encoding for each case or the refusal condition.

### ENC-01. Encode golden target coverage

**Question.** What pins writer output for targets other than IGES 5.3?

**Known.** The encode golden decodes each fixture and re-encodes through the semantic writer with an explicit IGES 5.3 target only (`golden_tests.rs:81-90`). Targets 4.0, 5.0, 5.1, and 5.2 have no encode golden. `iges.md` states all five target profiles.

**Need.** Encode goldens for the remaining targets, or a settled statement that the 5.3 golden pins the shared writer path, must establish coverage.
