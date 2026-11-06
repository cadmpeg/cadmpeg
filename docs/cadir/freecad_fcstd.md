# FreeCAD FCStd: CADIR projection and writing

Objects project to features, sketches, parameters, products, and presentation.
Unconsumed properties project to attributes. Feature `source_properties` and
`ViewPresentation.properties` are empty; GUI properties have typed native
homes. Spreadsheet cell `alignment`, `style`, `foregroundColor`,
`backgroundColor`, and `displayUnit` stay with the cell's native data.
An unprojected vendor object becomes a `Native` feature with typed properties;
an unprojected `App::` object remains native. `Touched` sets the feature's
`needs_recompute` flag.

The writer catalog contains schema targets 2, 3, and 4. Schema 4 is the
default for authored or foreign documents. Patching requires empty
`unrepresented`; a patch that cannot express an edit selects synthesis. Typed
property value trees are rebuilt with XML escaping. JSON-mode Python properties
use their decoded values without executing Python. Opaque payloads enter
`unrepresented` through their retained-record marks.

The synthesized application graph includes primitive solids, boolean operations,
extrusions and revolutions, sweeps and lofts, holes and thread controls, patterns,
dress-up operations, datum/construction objects, binders, body groups and tips.
Planar sketches include decoded conics, rational B-splines, construction flags,
constraints, dimensional values, and representable external references. A native
sketch geometry or relation may use its independently typed native record when it
has no neutral form. Unsupported relations and operations are omitted with losses.

Operation dimensions bind to canonical parameters by identity. Parameters write
their current evaluated values and token-rendered `ExpressionEngine` formulas on
their owning object. Constraint expressions bind to `Constraints[n]`. A parameter
whose semantic name differs from a built-in control drives that control through
a live native expression; a diameter driving `Radius` uses a division by two.
Ownerless parameters use a generated `App::VarSet` named `Parameters` (uniquified
when needed), and decode back as ownerless parameters. This keeps globals separate
from spreadsheet cells. Spreadsheets write expressions, aliases, retained cell
styles, merged ranges, column widths, and row heights. Features use `Label` for
neutral display names while stable declaration names carry references.
XML 1.0-forbidden characters in emitted labels and scalar text are replaced with
U+FFFD, with a Warning naming the affected output property and reported arena
coverage. Legal tabs, newlines, and literal character-reference text are preserved.
A plain `Part::Feature` is a geometry carrier and creates no feature-history node.

Exact shapes use generated OCCT text BRep tables with subshape-first topology and
independent roots. Analytic and NURBS carriers, supported procedural trim/offset/
extrusion/revolution forms, pcurves, coedge senses, and placements come from the
neutral model. The writer does not invoke a modeling kernel: feature or parameter
edits retain the document's stored BRep. The canonical `needs_recompute` flag is
read from and written to the object declaration's `Touched` attribute; it does
not become a parallel application property.
An unsupported required carrier omits its containing body with an Error loss while
other bodies remain writable.

Products, nested occurrences, supported joints, visibility, and neutral appearance
bindings generate application/GUI records. Neutral presentation overrides conflicting
typed native GUI values. Typed native attachments, element maps, string tables,
application properties, and GUI records are rebuilt when their referents exist;
opaque side payloads and dangling native referents are omitted and reported.
Omitting an unreadable object also removes references to it from dependencies and
properties, with an Error loss, so independent content still forms a valid archive.

The synthesized writer supports drawing pages, views, and selected annotations,
and writes mesh geometry from tessellations. It reports unsupported fields and
records in those arenas. Subdivision surfaces, assets, feature input and result
topologies, configurations, spatial sketches and their entities and constraints,
PMI, and presentation layers are dropped with losses. Unconsumed admitted
scalar, vector, link, map, JSON, UUID, and integer-list properties project to
neutral attributes and are written from those attributes. Identity-only Python
payloads, object materials, and other admitted typed property forms without a
neutral home remain native content and are re-emitted without an attribute or
a loss. Opaque payloads stay
in fidelity and mark their served classes unrepresented. Each populated
dropped arena has an export loss. Geometry omissions use Error severity; unsupported
nonstructural metadata uses Warning. Omitted native namespaces use the shared
informational `export:native.records-omitted` loss.
