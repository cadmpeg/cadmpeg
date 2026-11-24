# Alibre Design part and assembly: Open Items

This document lists the parts of the Alibre Design `.AD_PRT` and `.AD_ASM` formats that we do not know. The specification `alibre.md` gives the parts that we know.

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

### ALB-02. Feature model in `STEP_DATA`

**Question.** How do the entities of the feature model map to features, sketches, constraints, and parameters in the neutral IR?

**Known.** `alibre.md` §4 lists the entity families. A `DESIGNSTATE` entity carries a name, an index, an operation reference, a flag, and a context reference. An `EXTRUDESKETCHOPERATION` entity references its sketch, its end condition, and its dimension. A `DOUBLEPARAMETER` entity holds a parameter name, a GUID, and a value in model units.

**Need.** Parts with one feature of each family (extrusion, revolution, fillet, hole, loft, pattern), saved in turn, with the new entity families compared.

### ALB-03. Other streams

**Question.** What do `PROP_DATA`, `VERSION`, `AUTOGRAPH_DATA`, `DESIGN_PARAMETER_DATA`, `HOLE_OPERATIONS_DATA`, `EXTERNAL_THREAD_OPERATIONS_DATA`, `CUSTOM_ITEM_PROP_DATA`, `STANDARD_DESIGN_PROP_DATA`, `GLOBAL_PARAMETER_DATA_CACHE`, and `LIGHT_CONFIGURATION_DATA` hold?

**Known.** A part stores all of these streams. `AUTOGRAPH_DATA` holds text records of the form `#<n>{<number>,<n>,<n>,<n>,<n>,<word>,<n>}`. The streams other than `PROP_DATA`, `VERSION`, and `AUTOGRAPH_DATA` hold at most 48 bytes.

**Need.** Parts that change one document property, one hole operation, or one thread at a time, with the stream changes compared.

## 3. Documents other than parts

### ALB-04. Sheet metal

**Question.** How do sheet-metal parts store their geometry and their flat pattern?

**Known.** `alibre.md` §2 and §4 describe a solid part. The decoder reads a sheet-metal part as it reads a solid part.

**Need.** A part with one flange, with its `SAB_DATA` records and its `STEP_DATA` entities compared against a plain extrusion.

### ALB-05. Entity base integers

**Question.** What do the extra integer of the entity base and the extra integer of the `body` record mean?

**Known.** The extra base integer takes the value `-1`, and the `body` integer the value `0`. The decoder removes both before it reads the class fields, and it reports the removal as `alibre/source.record-extensions-unprojected`.

**Need.** A save of a part with several bodies, with the `body` integers compared.

### ALB-09. Assembly constraints

**Question.** How does an assembly store its constraints (mates)?

**Known.** `alibre.md` §5.2 lists the assembly entities. The `DESIGNASSEMBLY` entity has three lists, at attributes 21 to 23, that hold the empty list when the assembly has no constraint. The anchor of an occurrence is the logical at attribute 7 of its `DESIGNINCLUSION`. The decoder reports a non-empty list and every entity name that it does not know as `alibre/assembly.entities-unread`.

**Need.** Assemblies with one constraint of each kind (coincident, parallel, distance, angle, concentric), with the new entities and the three lists compared.

### ALB-10. Component references and document streams of an assembly

**Question.** How are the records of `CONSTITUENTS` framed, and what do the integers after the document GUID mean?

**Known.** `alibre.md` §5.3 gives the text of a record. Each record follows a byte `01` and one more byte, and the text repeats the reference, the second time after `:?/`. The two integers after the document GUID are 1 and 1 for a part and 2 and 1 for a subassembly; the `REPOSITORYVERSIONREFERENCE` states them at attributes 6 and 7. `PROP_DATA`, `VERSION`, and the two cache streams of an assembly are not read.

**Need.** An assembly that places a part from another drive and a part that is a new version of a stored part, with the framing bytes and the integers compared.

### ALB-11. Assembly configurations

**Question.** How do the configurations of an assembly differ in the state they give to each occurrence, and what does the configuration name of an inclusion select?

**Known.** `alibre.md` §5.2 gives the suppressed and hidden lists of the `DESIGNASSEMBLYCONFIGURATION`. A new assembly holds one configuration, `Config<1>`, and an inclusion names the component configuration `Config<1>`. The decoder states the state of the active configuration on the occurrences and reports `alibre/assembly.configuration-unmapped` only when an inclusion names a configuration other than `Config<1>` or the assembly holds a configuration besides the active one.

**Need.** An assembly with a second configuration that suppresses and moves one occurrence, and a component with a second configuration that an inclusion selects, with the entities compared.

### ALB-12. Flexible subassemblies and the state of nested occurrences

**Question.** How does an inclusion of a subassembly state the hidden, suppressed, and placed state of the components below it, and which attribute makes a subassembly flexible?

**Known.** An inclusion of a subassembly lists `DESIGNMEMBER` entities whose attribute 6 names a member of the subassembly. A visible nested member stores false at attribute 3. The decoder reports a hidden nested member as `alibre/assembly.nested-member-state-unread`. An inclusion of a subassembly lists no member of its own, so the decoder reads no hidden state for the occurrence itself. Attribute 10 of the inclusion holds false and may state flexibility.

**Need.** A subassembly with one hidden component and one suppressed component, and a flexible subassembly with a moved component, with the members and the attribute 10 compared.
