# SolidCraft open items

## SC-01: Container versions after 1

- **Question:** What do container markers after `solidcraft-container/1` change?
- **Known:** A reader that knows version 1 keeps an unknown marker and every member it does not read, and writes them back unchanged.
- **Need:** The member set and marker members of the next container version.

## SC-02: Planes built from the model

- **Question:** Can a sketch plane of type `midplane`, `two_edges`, `axis_point`, `three_points`, `tangent`, `along_path`, `perpendicular` or `through_point` be given a frame at decode time?
- **Known:** SolidCraft resolves these with `resolve_geo_plane` on the evaluated model state, from two faces, edges or points picked by name. The decoder reads the history, not the evaluated state. A face plane and a construction plane built from fixed values do have a frame, and the decoder uses it.
- **Decision:** The sketch placement stays unresolved and the loss `solidcraft.sketch.plane_unresolved` names it. The feature stays in the retained document, so nothing is lost on write-back. No accessor is added to the neutral plane type.
- **Need:** A reference to the frame of a named face from the cache decoder, if the placement is ever required in the neutral model.

## SC-03: Variable and spline fillet radii

- **Question:** Can a fillet of style `variable` or `spline` be typed as `FilletSize::Variable`?
- **Known:** SolidCraft places the stations per edge, from the end of the edge nearest the `start` position. The neutral stations run along an edge in its own direction, or along a chain that starts at the first edge's start vertex. Which end of an edge is nearest `start` depends on the edge geometry in the evaluated model.
- **Decision:** The fillet stays native, with its picks, names, radii, stations and `smooth` flag retained in the document, until the cache gives the edge ends of a pick.
- **Need:** The end vertices of each picked edge, from the cache decoder, to order the stations.

## SC-04: Operations the neutral pattern and sweep models cannot state

- **Question:** Can the neutral model state a mirror that joins its copies, a path pattern whose copies turn with the path, a sweep or loft with its rails reshaping the profile across the rails, a sweep held by a direction or by two rails, and a coil on a sketch profile?
- **Known:** `PatternTransform::Mirror` and `CurveDriven` have no flag for joining the copies or for orienting them with the path. A sweep orientation held by a direction, two rails, and the rail fit `stretch` have no neutral field. The coil of a drawn profile has no section form beside the generated ones.
- **Decision:** Each stays a native operation with its fields in the retained document. A pattern about a model face, edge, or construction axis uses the axis stored with it, as a face plane uses the frame it had when picked. A sweep carries its orientation (perpendicular or parallel), twist, and end scale, and a loft carries one or two rails with the default fit.
- **Need:** A join flag on the mirror pattern, an orientation flag on the path pattern, and a second guide with a fit mode on the sweep.

## SC-05: Sketch curves and relations without a neutral form

- **Question:** Can a periodic spline, a fit spline with tangent handles, and the sketch pattern, slot, and arc-length relations be typed?
- **Known:** A conic maps exactly to a rational quadratic NURBS (middle weight `rho / (1 - rho)`), and a control spline with its own knots and weights maps to a NURBS entity. A periodic spline needs the neutral periodic pole convention for a wrapped control polygon, and the fit spline with handles needs the source's Hermite construction; neither is pinned by a design. The three relation families have no `SketchRelation` form.
- **Decision:** They stay native relations and untyped curves, retained whole in the document.
- **Need:** A design that exercises a periodic spline and one with handles, to pin the pole convention and the construction, and relation forms in the neutral sketch model.

## SC-06: Application data without a neutral record

- **Question:** Can the look of a whole component, the render setups, the metadata, the plugin data, the browser groups, and a canvas's placement be typed?
- **Known:** A note decodes to a text annotation, a canvas image to an embedded asset, and a point cloud to a neutral cloud placed in the design. The neutral model has no record for an image placed on a plane, for a render setup, or for an application's own settings.
- **Decision:** Each stays a retained record of its own section, with identity and digest, and survives a rewrite.
- **Need:** A placed-image record, and a place for presentation settings, in the neutral model.
