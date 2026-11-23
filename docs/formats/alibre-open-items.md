# Alibre Design part: Open Items

This document lists the parts of the Alibre Design `.AD_PRT` format that we do not know. The specification `alibre.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English.

## 1. Geometry

### ALB-01. Skinning surface construction

**Question.** What are the fields of the `skinsur` subtype and the `exppc`, `parcur`, `nullbs`, and `intcurve` records of a lofted solid?

**Known.** The `spline` surface record holds the subtype `skinsur`, an integer 0, two flags, an integer 2, four doubles of `-1`, a double 0, the section curves, and a solved `nubs` block with its fit tolerance. The decoder reads the solved block as a B-spline carrier and keeps the construction as an unknown procedural surface (`alibre/geometry.procedural-surface-untyped`). The edges between a loft face and a planar cap have `intcurve` curves with `exppc` pcurves, and these curves carry no typed 3D carrier.

**Need.** A loft between two sections with a known profile, with the curve records of the edges compared against the sections.

## 2. Feature history

### ALB-02. Feature model in `STEP_DATA`

**Question.** How do the entities of the feature model map to features, sketches, constraints, and parameters in the neutral IR?

**Known.** `alibre.md` §4 lists the entity families. A `DESIGNSTATE` entity carries a name, an index, an operation reference, a flag, and a context reference. An `EXTRUDESKETCHOPERATION` entity references its sketch, its end condition, and its dimension. A `DOUBLEPARAMETER` entity holds a parameter name, a GUID, and a value in model units.

**Need.** Parts with one feature of each family (extrusion, revolution, fillet, hole, loft, pattern), saved in turn, with the new entity families compared.

### ALB-03. Other streams

**Question.** What do `PROP_DATA`, `VERSION`, `AUTOGRAPH_DATA`, `DESIGN_PARAMETER_DATA`, `HOLE_OPERATIONS_DATA`, `EXTERNAL_THREAD_OPERATIONS_DATA`, `CUSTOM_ITEM_PROP_DATA`, `STANDARD_DESIGN_PROP_DATA`, `GLOBAL_PARAMETER_DATA_CACHE`, and `LIGHT_CONFIGURATION_DATA` hold?

**Known.** All streams are present in every part. `AUTOGRAPH_DATA` holds text records of the form `#<n>{<number>,<n>,<n>,<n>,<n>,<word>,<n>}`. The streams other than `PROP_DATA`, `VERSION`, and `AUTOGRAPH_DATA` hold at most 48 bytes.

**Need.** Parts that change one document property, one hole operation, or one thread at a time, with the stream changes compared.

## 3. Documents other than parts

### ALB-04. Assemblies and sheet metal

**Question.** How do `.AD_ASM` assemblies and sheet-metal parts store their geometry?

**Known.** An assembly file holds the streams `STEP_DATA`, `CONSTITUENTS`, `PROP_DATA`, `VERSION`, `GLOBAL_PARAMETER_DATA_CACHE`, and `LIGHT_CONFIGURATION_DATA`, and holds no `SAB_DATA` stream. The decoder does not claim these files.

**Need.** An assembly of two parts with a known placement, and a part with one flange, with the `CONSTITUENTS` stream and the sheet-metal entities compared.

### ALB-05. Entity base integers

**Question.** What do the extra integer of the entity base and the extra integer of the `body` record mean?

**Known.** The extra base integer is `-1` in every record. The `body` integer is `0`. The decoder removes both before it reads the class fields.

**Need.** A save of a part with several bodies, with the `body` integers compared.
