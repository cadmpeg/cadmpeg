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



## 2. Global metadata

Type 406 Forms 5001 through 9999 are settled. Under [IGES 4.0 §4.3.7](https://www.govinfo.gov/content/pkg/GOVPUB-C13-7b81ba8b0f709555f162cb496aa63b3b/pdf/GOVPUB-C13-7b81ba8b0f709555f162cb496aa63b3b.pdf) and [IGES 5.3 §4.97](https://paulbourke.net/dataformats/iges/IGES.pdf), each property has `NP` at Parameter index 1, `NP` variable values, and any additional pointer groups. `parameter.rs::entity_primary_end` applies the common boundary before generic recovery. The owner tests `type406_implementor_defined_forms_use_common_count_boundary` and `type406_implementor_defined_malformed_count_or_span_suppresses_generic_recovery` cover Forms 5557, 6007, and 9999 and malformed count or span input. The neutral model has no standard meaning for these forms, so the decoder retains their complete native entity records without semantic projection.


## 3. Directory fields, the reference graph, and the native arenas

## 4. Geometry carriers and tolerances

## 5. Surfaces and topology

### TP-01. Traversal direction of Type 144 inner boundaries

**Question.** Does IGES 5.3 give a traversal direction to the inner boundaries of a Type 144 trimmed surface and to their Type 142 curves?

**Known.** `iges.md` gives the Type 144 fields and the nesting rules of its boundaries, but no traversal direction. The fixture `trimmed_plane_inner_loop.igs` lists its inner boundary counterclockwise in model space and in parameter space. The decoder (`crates/cadmpeg-codec-iges/src/entities/trimming.rs`) keeps that direction: the decoded inner loop runs counterclockwise about the +z oriented normal of its face.

**Need.** `cad-ir.md` requires an inner loop to run clockwise about the oriented normal. The decoder must reverse an inner boundary when IGES gives no direction or gives a direction that the neutral rule contradicts, and must keep it otherwise.

**Conflict.** The decoded inner loop contradicts `cad-ir.md`. A decision is necessary.

### TP-02. Loop and face orientation of a Type 514 void shell

**Question.** How do the Type 186 void-shell logical, the face orientation logicals of a Type 514 shell, and the edge-use logicals of the Type 508 loops of its Type 510 faces combine into a face sense and a loop direction?

**Known.** `iges.md` gives the Type 186 shell-use logicals (`SOF`, `VOF`), the Type 508 edge-use orientation logical, and the Type 510 and 514 fields. It does not give how these logicals combine into a face sense and a loop direction. In `explicit_void_solid.igs`, the decoder (`crates/cadmpeg-codec-iges/src/entities/brep.rs`) gives each void face the sense `reversed` and a loop that keeps the face on its right about that oriented normal. The face senses give the volume 511/48: the tetrahedron with legs 4 minus the tetrahedron with legs 0.5. The loops give 513/48.

**Need.** The decoder must produce void-shell loops that agree with the face sense, so that measurement and validation read one orientation.

**Conflict.** The decoded loop winding contradicts the decoded face sense. A decision is necessary.

## 6. Product structure, annotation, and presentation

## 7. Write path
