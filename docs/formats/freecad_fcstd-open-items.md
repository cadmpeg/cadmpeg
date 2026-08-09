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

## 1. Sketch geometry

### SG-02. Conic conventions without a FreeCAD-saved witness

**Question.** Do FreeCAD-saved documents give the elliptical-arc, hyperbola, and parabola carriers the parameterization that the decoder applies?

**Known.** FreeCAD source (`Save` and `Restore` for `GeomEllipse`, `GeomArcOfEllipse`, `GeomArcOfHyperbola`, and `GeomArcOfParabola` in `src/Mod/Part/App/Geometry.cpp`) defines the convention. `AngleXU` is the counterclockwise angle from the sketch X axis to the major axis. `StartAngle` and `EndAngle` are OCCT curve parameters. For a bounded ellipse, the point at parameter `t` is `center + major_radius * cos(t) * major_direction + minor_radius * sin(t) * minor_direction`, where the minor direction is the major direction rotated by a quarter turn. The decoder's endpoint evaluation applies the same formula. No fixture in the corpus is a FreeCAD-saved document that carries a bounded elliptical arc, a rotated ellipse, a hyperbola, or a parabola, so no test compares the decoder against bytes that FreeCAD wrote.

**Need.** A FreeCAD-saved document with this geometry gives an independent witness for the convention. Agreement between the decoder and a source reading can hide one shared misreading. A saved document removes that possibility, and its absence keeps the conic decode arms unproven against real output.

### SG-03. FreeCAD-saved conic fixture authoring

**Question.** Which FreeCAD-saved fixtures must the corpus contain to cover conic sketch geometry and profile chaining?

**Known.** `corpus/freecad_fcstd/author_fixtures.py` authors fixtures through the FreeCAD Python API on a machine with a FreeCAD installation. The current build machine has no FreeCAD installation. `corpus/freecad_fcstd-fixture-charter.md` states that a synthetic parser input establishes no ladder score. No current fixture carries an arc with a non-zero sweep that a profile chain consumes.

**Need.** We must extend the authoring script with one sketch that contains a circular arc with a non-zero `AngleXU`, a bounded elliptical arc with a rotated major axis, a full rotated ellipse, a hyperbola, a parabola, and line segments that meet the arc endpoints. The rotation angles must stay away from multiples of a quarter turn, because at those angles a frame built with swapped axes produces the same points. We must then run the script under FreeCAD, commit the saved documents, and pin their decode, inspect, encode, and STEP goldens. These fixtures resolve SG-02, and they can support format-support claims because FreeCAD wrote them.
