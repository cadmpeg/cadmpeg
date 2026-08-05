# FreeCAD `.FCStd`: Open Items

This document lists the parts of the FreeCAD `.FCStd` format that we do not know. The specification `freecad_fcstd.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. Legacy persistence

### LP-01. Schema 2 object grammar

**Question.** What object grammar does `Document.xml` use when `SchemaVersion=2`?

**Known.** `freecad_fcstd.md` §1 "Schema versions 2 and 3" identifies schema 2 as a legacy envelope. `freecad_fcstd.md` §2 "`Document.xml` is the authoritative application object and property graph." states that `Document.xml` is the authoritative application object and property graph.

**Need.** We must know the grammar to decode and validate each schema 2 object boundary and value.

### LP-02. Schema 2 property grammar

**Question.** What property grammar does `Document.xml` use when `SchemaVersion=2`?

**Known.** `freecad_fcstd.md` §1 "Schema versions 2 and 3" states that earlier property encodings belong to separate legacy envelopes. `freecad_fcstd.md` §3 "`ProgramVersion` is metadata." states that property type and value tag select parsing dispatch.

**Need.** We must know the grammar to decode and validate each schema 2 property boundary and value.

### LP-03. Schema 3 object grammar

**Question.** What object grammar does `Document.xml` use when `SchemaVersion=3`?

**Known.** `freecad_fcstd.md` §1 "Schema versions 2 and 3" identifies schema 3 as a legacy envelope. `freecad_fcstd.md` §2 "`Document.xml` is the authoritative application object and property graph." states that `Document.xml` is the authoritative application object and property graph.

**Need.** We must know the grammar to decode and validate each schema 3 object boundary and value.

### LP-04. Schema 3 property grammar

**Question.** What property grammar does `Document.xml` use when `SchemaVersion=3`?

**Known.** `freecad_fcstd.md` §1 "Schema versions 2 and 3" states that earlier property encodings belong to separate legacy envelopes. `freecad_fcstd.md` §3 "`ProgramVersion` is metadata." states that property type and value tag select parsing dispatch.

**Need.** We must know the grammar to decode and validate each schema 3 property boundary and value.

### LP-05. Legacy object-layout dispatch

**Question.** Which version fields and type fields select each pre-schema-4 object layout?

**Known.** `freecad_fcstd.md` §1 "Schema versions 2 and 3" states that a decoder must identify the governing version before it refuses an unsupported layout. `freecad_fcstd.md` §3 "`ProgramVersion` is metadata." lists the structural attributes that select parsing dispatch.

**Need.** We must know the selection rule to choose the correct object grammar before object decoding starts.

### LP-06. Legacy property-encoding dispatch

**Question.** Which version fields, property types, and value tags select each property encoding before schema 4?

**Known.** `freecad_fcstd.md` §3 "`ProgramVersion` is metadata." states that document schema, file version, property type, and value tag select parsing dispatch.

**Need.** We must know the selection rule to choose the correct property grammar before property decoding starts.

## 2. Auxiliary records

### AR-01. Application-specific side-entry framing

**Question.** What byte framing does each application-specific side-entry family use when no typed property grammar identifies the family?

**Known.** `freecad_fcstd.md` §2 "`Document.xml` is the authoritative application object and property graph." states that an entry gets semantic meaning from a typed reference in `Document.xml` or `GuiDocument.xml`. An unreferenced entry remains a named archive record. `freecad_fcstd.md` §11 "Application data without a neutral representation retains its owning object and property" defines exact retention for application data without a neutral representation.

**Need.** We must know the framing to parse and validate record boundaries in these side entries.

### AR-02. Application-specific side-entry values

**Question.** What does each field in an application-specific side-entry family mean when no typed property grammar identifies the family?

**Known.** `freecad_fcstd.md` §11 "Application data without a neutral representation retains its owning object and property" requires retention of the owning object, property, declared application type, links, source order, XML bytes, side-entry bytes, byte spans, lengths, and digests.

**Need.** We must know the field meanings to transfer the side entry to a typed native or neutral record.

## 3. GUI properties

### GP-01. Other GUI property grammars

**Question.** What value grammar does each GUI property runtime type use when `freecad_fcstd.md` does not define that type?

**Known.** `freecad_fcstd.md` §11 "Format-neutral document and view presentation arenas represent GUI state." through `freecad_fcstd.md` §11 "For shape-bearing objects, the view provider's shape color" define document presentation, view-provider state, object appearance, topology color arrays, and their precedence. Each other GUI property retains its owner, runtime type, status, ordered value elements, side-entry references, exact XML, and byte range.

**Need.** We must know each grammar to parse and validate the property as a typed presentation value.

### GP-02. Other GUI property semantics

**Question.** What presentation value does each GUI property runtime type represent when `freecad_fcstd.md` does not define that type?

**Known.** `freecad_fcstd.md` §11 "GUI records retain view-provider identity separately from application-object identity." states that GUI records keep presentation data linked to its owner. Each undefined GUI property retains its runtime type and ordered value elements.

**Need.** We must know the value semantics to transfer the property to the correct neutral presentation field.

## 4. Sketch geometry

### SG-01. Circular-arc frame rotation `AngleXU`

**Question.** Which arc segment does a `Part::GeomArcOfCircle` carrier select when its `AngleXU` attribute is not zero?

**Known.** FreeCAD's reader (`GeomArcOfCircle::Restore` in FreeCAD source `src/Mod/Part/App/Geometry.cpp`) rotates the reference frame by `AngleXU` around the normal before it applies `StartAngle` and `EndAngle`. In that frame, an endpoint at parameter `t` sits at the global angle `t + AngleXU`. FreeCAD's writer (`GeomArcOfCircle::Save`) computes `AngleXU` from the circle's stored X axis. The reader accepts a carrier without the attribute and then uses zero. `freecad_fcstd.md` §11 "Sketch point, line, circle, circular-arc, ellipse, and elliptical-arc carriers" does not name `AngleXU` for circular arcs.

**Conflict.** The decoder reads `CenterX`, `CenterY`, `Radius`, `StartAngle`, and `EndAngle`, and it does not read `AngleXU`. A carrier with a non-zero `AngleXU` decodes to a different segment of the same circle. Both arc endpoints move by the rotation angle. Profile chaining then fails, or it joins the wrong entities, and no loss note reports the cause.

**Need.** The decoder must apply the rotation to both angle bounds, or it must keep the carrier as a named native geometry record with a blocking note. We must also learn how often saved documents carry a non-zero value. Sketcher creates arcs with default axes, and a transformation of the geometry can rotate the stored X axis.

### SG-02. Conic conventions without a FreeCAD-saved witness

**Question.** Do FreeCAD-saved documents give the elliptical-arc, hyperbola, and parabola carriers the parameterization that the decoder applies?

**Known.** FreeCAD source (`Save` and `Restore` for `GeomEllipse`, `GeomArcOfEllipse`, `GeomArcOfHyperbola`, and `GeomArcOfParabola` in `src/Mod/Part/App/Geometry.cpp`) defines the convention. `AngleXU` is the counterclockwise angle from the sketch X axis to the major axis. `StartAngle` and `EndAngle` are OCCT curve parameters. For a bounded ellipse, the point at parameter `t` is `center + major_radius * cos(t) * major_direction + minor_radius * sin(t) * minor_direction`, where the minor direction is the major direction rotated by a quarter turn. The decoder's endpoint evaluation applies the same formula. No fixture in the corpus is a FreeCAD-saved document that carries a bounded elliptical arc, a rotated ellipse, a hyperbola, or a parabola, so no test compares the decoder against bytes that FreeCAD wrote.

**Need.** A FreeCAD-saved document with this geometry gives an independent witness for the convention. Agreement between the decoder and a source reading can hide one shared misreading. A saved document removes that possibility, and its absence keeps the conic decode arms unproven against real output.

### SG-03. FreeCAD-saved conic fixture authoring

**Question.** Which FreeCAD-saved fixtures must the corpus contain to cover conic sketch geometry and profile chaining?

**Known.** `corpus/freecad_fcstd/author_fixtures.py` authors fixtures through the FreeCAD Python API on a machine with a FreeCAD installation. The current build machine has no FreeCAD installation. `corpus/freecad_fcstd-fixture-charter.md` states that a synthetic parser input establishes no ladder score. No current fixture carries an arc with a non-zero sweep that a profile chain consumes.

**Need.** We must extend the authoring script with one sketch that contains a circular arc with a non-zero `AngleXU`, a bounded elliptical arc with a rotated major axis, a full rotated ellipse, a hyperbola, a parabola, and line segments that meet the arc endpoints. The rotation angles must stay away from multiples of a quarter turn, because at those angles a frame built with swapped axes produces the same points. We must then run the script under FreeCAD, commit the saved documents, and pin their decode, inspect, encode, and STEP goldens. These fixtures resolve SG-01 and SG-02, and they can support format-support claims because FreeCAD wrote them.
