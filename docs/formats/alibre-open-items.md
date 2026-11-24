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

**Known.** The `spline` surface record holds the subtype `skinsur`, an integer 0, two flags, an integer 2, four doubles of `-1`, a double 0, the section curves, and a solved `nubs` block with its fit tolerance. The decoder reads the solved block as a B-spline carrier and keeps the construction as an unknown procedural surface (`alibre/geometry.procedural-surface-untyped`). The edges between a loft face and a planar cap have `intcurve` curves with `exppc` pcurves. These curves carry no typed 3D carrier, and the decoder emits no pcurve for them (`alibre/geometry.pcurve-undecoded`).

**Need.** A loft between two sections with a known profile, with the curve records of the edges compared against the sections.

## 2. Feature history

### ALB-02. Feature families without a decoded record

**Question.** Which entities and attributes store a pattern, mirror, chamfer, shell, thread, sweep, draft, and rib?

**Known.** `alibre.md` §4 lists the operation entities of the extrusion, revolution, fillet, hole, loft, offset plane, and sketch. A state with another operation entity is a `DESIGNSTATE` whose operation attribute references that entity. The decoder keeps the state as a native feature named after the entity and reports `alibre/history.operation-unmapped`.

**Need.** Parts with one feature of each family, saved in turn, with the new entity families compared against the state list.

### ALB-06. Logicals and integers of the operation entities

**Question.** What do the logical and integer attributes of the operation and end-condition entities state?

**Known.** `BLINDENDCONDITION` attribute 2 is a logical that is false for an extrusion on the sketch normal. `BLINDANGLEENDCONDITION` attribute 0 and `THROUGHALLENDCONDITION` attribute 2 are logicals. `EXTRUDESKETCHOPERATION` stores the integer 0 at attribute 1 and the integer 2 at attribute 7, and a logical at attribute 9. `HOLEOPERATION` stores the integer 1 at attribute 1 and two logicals after the shape. `SIMPLEHOLE` stores the integer list `(0)` at attribute 0 and integers and logicals after the end condition. `BLOCKPRIMITIVE` ends with a logical. `DESIGNSKETCH` stores integers at attributes 4 to 6, an integer list such as `(0,4)` at attribute 7, a list of text numbers at attribute 8, and a null at attribute 9. `DESIGNSTATE` holds nine attributes, for example `DESIGNSTATE('Origin',0,#207,.T.,$,#192,#219,$,$)`. The decoder reads the name (0), the index (1), which it checks against the position of the state and reports as `alibre/history.state-index-mismatch` where they disagree, and the operation (2). `BLOCKPRIMITIVE` attribute 3 is an ordinal that the decoder does not read. Attribute 3 is a logical that is true in every committed state, 4 and 7 are null, 5 references a `PRODUCTVERSION` record, 6 references the `DESIGNPARTSTATECONTEXT`, and 8 references a `PRODUCTVERSION` record in the states of sketches and modeling features and is null in the others. The meaning of the logical at 3 and of the references at 4 to 8 is unknown, and the decoder reads none of them.

**Need.** One save of each operation with a single control changed (reversed extrusion direction, symmetric extrusion, extrusion to a face, hole direction, construction geometry in a sketch), with the changed attribute compared.

### ALB-07. Edge and face selection

**Question.** How does a `TOPOLOGYSIGNATURE` select edges and faces of the body?

**Known.** An `OPERATIONTARGETPROXY` of a fillet or of a sketch on a face references a `TOPOLOGYSIGNATURE` whose attribute 0 is a short text of the form `#<n>`. The text names no edge or face by an index of the `SAB_DATA` records. The decoder keeps the text as the native selection text of an unread selection, and reports `alibre/history.edge-selection-unresolved` for a fillet and `alibre/history.face-selection-unresolved` for a sketch on a face.

**Need.** A fillet on two different edges of one body, and sketches on two different faces, with the signature texts compared against the edge and face order of the body.

### ALB-08. Loft settings and sketch constraint kinds

**Question.** What do the loft attributes after the section list state, and which kinds of `CONSTRAINTPAIRUV` besides 2, 4, and 6 exist?

**Known.** `LOFTOPERATION` holds after the section list an empty list, the integer 2048, two lists of two reals `(1.,1.)` and `(0.1,0.1)` and `(0.,0.)`, a list of two logicals, the integer 4, and a logical. A `CONSTRAINTPAIRUV` of another kind, or with another pair of operands, is reported by `alibre/history.sketch-constraint-unmapped`.

**Need.** A loft with tangent end conditions and a changed section count, and sketches with one constraint of each kind (tangent, equal, perpendicular, dimension), with the records compared.

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

**Known.** The extra base integer is `-1` in every record. The `body` integer is `0`. The decoder removes both before it reads the class fields, and it reports the removal as `alibre/source.record-extensions-unprojected`.

**Need.** A save of a part with several bodies, with the `body` integers compared.
