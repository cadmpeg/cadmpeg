# SolidCraft designs

A SolidCraft design is a parametric document: a feature timeline, parameters,
sketches, components, and presentation data. Its file is a plain JSON
document or a ZIP container. The extensions are `.sc` and `.solidcraft`; the
extension does not select the encoding.

Open items are in [`solidcraft-open-items.md`](solidcraft-open-items.md).

## 1. Encoding

A file that starts with the ZIP local file header signature `50 4b 03 04` is
a container (section 2). Any other file is a plain JSON document (section 3)
in UTF-8, with an optional byte order mark.

## 2. Container

The container is a ZIP archive without ZIP64 records and on one disk. Member
names are unique, nonempty, use `/` as the separator, and have no empty, `.`,
or `..` component. Each member is stored or Deflate-compressed. Member order
carries no meaning.

| Member | Content |
| --- | --- |
| `container.json` | The container marker. |
| `document.json` | The design document (section 3), with its payloads moved to blobs. |
| `drawing.json` | The drawing document of a drawing container. |
| `blobs/<digest>.<extension>` | One payload. |
| `cache/model.json`, `cache/bodies.bin` | The model cache: the evaluated model of the document. |
| `cache/preview.json`, `cache/preview.bin` | A display mesh of the evaluated model. |
| `cache/thumbnail.json`, `cache/thumbnail.png` | A 240 × 150 picture of the model and its key. |
| `plugins/<key>` | The data of one application plugin, as bytes. |
| `provenance/log.cbor` | The signed save log. |

### 2.1 Marker

`container.json` is one JSON object.

| Marker | Meaning |
| --- | --- |
| `{"format":"solidcraft-container/1"}` | A design container, version 1. |
| `{"format":"solvecraft-container/1"}` | A design container, version 1, with the draft marker. |
| `{"format":"solidcraft-container/1","content":"drawing"}` | A drawing container, version 1. |

Any other marker comes from a later container version.

### 2.2 Blobs

A blob name is `blobs/`, the lowercase hexadecimal SHA-256 of the expanded
payload (64 digits), `.`, and an extension:

| Extension | Payload | Referencing member |
| --- | --- | --- |
| `step` | STEP Part 21 text | `step_blob` of an `import` feature |
| `mesh` | Triangle mesh | `mesh_blob` of a mesh of a `mesh_import` feature |
| `png`, `jpeg` | Image | `image_blob` of a canvas |
| `points` | Point cloud | `points_blob` of a point cloud |
| `hdr` | Radiance HDR image | `hdr_blob` of a render resource |
| `texture` | Render texture | `texture_blob` of a render texture resource |

The container holds one member for each distinct name. A referencing record
keeps its other members and holds an empty value in place of the moved
payload: `step` is `""` beside `step_blob`; `positions`, `triangles`, and
`corner_normals` are `[]` beside `mesh_blob`; `data` is `""` beside
`image_blob`, `hdr_blob`, and `texture_blob`; `points` and `colors` are `""`
beside `points_blob`.

### 2.3 Caches

The cache members are keyed to the SHA-256 of the exact `document.json`
bytes. A cache is redundant: the document alone defines the design.

### 2.4 Model cache

`cache/model.json` is the manifest of the model cache, and
`cache/bodies.bin` is its payload. The manifest is one JSON object:

| Key | Value |
| --- | --- |
| `format` | `solidcraft-model-cache/10` |
| `document` | The SHA-256 of `document.json`, lowercase hexadecimal. |
| `payload` | The SHA-256 of `cache/bodies.bin`. |
| `slices` | `[offset, length]` pairs. They are contiguous and cover the payload. |
| `bodies` | One entry per model body: `[slice]`, or `[slice, share, matrix]` for a placed copy, where `matrix` holds 16 numbers. |
| `shares` | The slice of each placed part. |
| `model` | The evaluation records. |

The `model` records hold `features` (each with `id`, `skipped`, and the
index of its `state`), `states` (each with its model bodies as runs of
`[first, count]`), and `model_bodies` (each with its `body` entry, its
persistent `key`, its `name`, and its `component`). The final state is the
state of the last feature whose `skipped` is false. A manifest whose
`document` is not the digest of the `document.json` that the container
holds belongs to another document.

A slice that starts with `SCBREP05` is a native B-rep body. One tagged
value follows the eight signature bytes:

| Tag | Value |
| --- | --- |
| 0, 1, 2 | null, false, true |
| 3, 4 | signed, unsigned 64-bit integer, little-endian |
| 5 | binary64, little-endian, finite |
| 6 | UTF-8 string: 32-bit little-endian length, then the bytes |
| 7 | array: 32-bit little-endian count, then the values |
| 8 | object: 32-bit little-endian count, then each key as a string without its tag, and its value |

Object keys are in strictly increasing byte order. A slice holds at most
20,000,000 values, nests at most 48 deep, holds at most 1,000,000 entries
per collection and 1 MiB per string, and has no bytes after its value. The
value is `{metadata, runtime, solid}`. `solid` is B-rep schema 4
(SolidCraft `docs/native-brep.md`): regions, shells, faces with their
surface and loops, edges with their curve, interval, and uses, and
vertices, all in millimetres and radians. A face or an edge can carry
`nurbs`, the NURBS form whose parameters its domain, pcurves, and interval
use. Loops are stored about the surface's natural normal; an oriented
face with `forward` false uses the face reversed.

A slice that starts with `SCMESH01` is a mesh body: a 32-bit little-endian
header length, the JSON header (`positions`, `triangles`,
`corner_normals`, `color`, `paint`), each position as three binary64, each
triangle as three 32-bit indices, and, when `corner_normals` is not zero,
nine binary64 of corner normals per triangle.

## 3. Document

The design document is one JSON object.

### 3.1 Format

`format` is `solidcraft/N` or `solvecraft/N`, with `N` a nonzero decimal
format number. A document without `format` has format 1. Each format number
adds changes to the previous one; the newest released format is 57.

`format_pending` is an optional array of change names. A nonempty list names
changes after format `N` that the writing build applied.

### 3.2 Members

| Member | Content |
| --- | --- |
| `name` | The design name. |
| `units` | The length unit, `mm` by default. |
| `params` | The parameter table: records with `name`, `expr`, `unit`, and `model`. |
| `features` | The feature timeline, in order. Each record has `id`, `name`, `suppressed`, and a `type` tag. |
| `next_id` | An identifier counter. Readers compute it from the records. |
| `links` | The linked designs, by component id. `path` names each design file. |

Other top-level members hold the components, occurrences, assembly, rules,
configurations, canvases, point clouds, materials, appearances, views, notes,
render settings, and application data. Each member that is empty is absent.

### 3.3 Expressions

A length or an angle is a number or an expression string over the
parameters: `2 * width + 5 mm`, `angle / 2`, `atan2(h; w)`. The units are
`mm`, `cm`, `m`, `um`, `in`, `inch`, `ft`, `yd`, `km`, `mil`, `nm`, `deg`,
and `rad`; `°`, `"`, and `'` are degrees, inches, and feet. A unitless operand
of `+` or `-` takes the other operand's unit, and a unitless result takes
the expected unit: millimetres for a length, degrees for an angle. Function
arguments are separated by `;`, and a comma outside a call is a decimal
separator. `%` is the remainder, and two operands side by side multiply
(`2 width`). A leading `=` marks a formula and is not part of it, and a
definition `name = expression` is worth its expression. A parameter's value
is its `expr` in its `unit`; a `model` parameter was made by a dimension or a
feature.

### 3.4 Sketches

A `sketch` feature has a `plane` and a `sketch` record. A plane is a named
origin plane (`XY` with normal +Z; `XZ` with x +X, y −Z, and normal +Y; `YZ`
with x +Y, y +Z, and normal +X), a `custom` frame (`origin`, `x_dir`,
`y_dir`), a `placed` frame whose origin is length expressions, an `offset`
of another plane along its normal, a `flipped` plane, a `face` plane (with
the frame it had when it was picked, in `plane`), or a `construction`
plane feature. A sketch point `(u, v)` is `origin + u x + v y`, and the
sketch normal is x × y.

The sketch record lists `points` (`id`, `pos`, `fixed`) and `curves` that
index them: a `line` from `a` to `b`, a `circle` of centre `c` and radius
`r`, an `arc` counterclockwise about `c` from `a` to `b`, an `ellipse` of
centre `c`, major-axis end `m`, and minor radius `r`, a `spline` through
`pts`, and a `conic`. A `spline` without `control` is the natural cubic
interpolant of `pts` in the chord-length parameter: its second derivative
vanishes at both ends, and each span is a cubic Bézier piece. With `control`,
the spline is the clamped uniform B-spline of `degree` over `pts`. A curve
marked `construction`, `centerline`, or `infinite` bounds no profile.

### 3.5 Operations

An `extrude` sweeps the profiles of its `sketch` along the sketch normal.
Its `extent` has a `distance`, a `direction` (`positive`, `negative`, or
`symmetric`), and optionally `distance2` (the other side), `start_offset`,
`through_all`, `to`, and `from`. A symmetric distance is the travel to
each side under the `legacy_per_side` convention, and the total travel
under `total_distance`. A negative distance reverses the side.

A `revolve` turns the profiles of its `sketch` about its `axis` (a
`sketch_axis` `x` or `y`, a `world` axis, a `sketch_line`, a model edge, or
a construction axis) by `angle`, with `angle2` on the other side or split
evenly when `symmetric`. The sweep leaves the profile toward the sketch
normal: the rotation axis is the axis or its reverse, whichever turns the
profile's centre toward the normal.

A `box` has a `corner`, a `length`, a `width`, and a `height` along x, y,
and z, centred on the corner when `centred`. A `cylinder` and a `cone`
stand on `base` along `axis` with a `radius` (and a `top_radius`) and a
`height`. A `sphere` has a `center` and a `radius`, and a `torus` a
`center`, a `major`, and a `minor` radius about +Z. The `operation` of each
is `new_body`, `join`, `cut`, or `intersect`.

A pick is a model point that snaps to the nearest edge or face on each
evaluation. A `fillet` rounds the `edges` it picks by `radius` (a chord
width when its `style` is `chord`); a `chamfer` cuts them by `distance`
and `distance2` or `angle`. `edge_names` and `face_names` hold the
persistent names of the picked topology. A `shell` opens the `faces` it
picks and leaves a wall of `thickness` `inside`, `outside`, or `both`. An
`offset_face` moves the picked faces along their normals by `distance`.

A `move` translates its `bodies` by `translate` and turns them by `angle`
about `rotate_axis` through `origin`, or copies them when `copy`. A
`remove` deletes its `bodies`. A `combine` joins, cuts, or intersects its
`tools` into its `target`. A `scale` scales its `bodies` about `origin` by
`factor`, or by `factors` per axis.

A `pattern` repeats its `features`, or its `bodies`: `rectangular` along
`dir1` and `dir2` by `count` and `spacing`, `circular` about `axis` through
`origin` over a total `angle` (a full turn spreads the copies evenly), or
along a sketch `path`. An `alignment` of `right` places the copies along
the direction, `left` against it, and `center` alternately on both sides;
`reversed` turns the direction round. A `mirror` reflects its `features`
or `bodies` in its `plane`. A `sweep` moves a profile of its `sketch`
along the `path` curves of `path_sketch`; a `loft` blends the profiles of
its `sections`. A `coil` sweeps a `circular` or `square` section of
`section_size` along a helix of `diameter`, `pitch`, and `turns` about
`axis` from `base`, right-handed unless `clockwise`. Every model parameter
that a feature made is listed in its `param_names`.

### 3.6 Sketch constraints

The `constraints` of a sketch record refer to points by index (`p`, `q`)
and to curves by index (`l`, `c`, `a`, `b`). Geometric constraints are
`coincident`, `point_on_curve`, `horizontal`, `vertical`,
`horizontal_points`, `vertical_points`, `parallel`, `perpendicular`,
`collinear`, `tangent`, `equal`, `concentric`, `midpoint`, `symmetric`,
`fix`, and `smooth` (curvature continuity). A dimension has a `value` in
millimetres or radians and, when it drives one, the `param` that holds it:
`distance`, `distance_x`, `distance_y`, `point_line_distance`, `length`,
`line_angle` (from sketch +X), `angle` (counterclockwise from `a` to `b`,
plus a half turn when `flip`), `radius`, `diameter`, and the circle,
slot, arc, and offset dimensions. A `driven` dimension measures and does
not constrain. Two curves that index one point share it.

### 3.7 Components and joints

The design is its root component, id 0. A `component` has an `id`, a `name`,
and a `parent`, and is authored in its own frame. A feature, a sketch, and a
model body belong to the component named by their `component` (0 when it is
absent). An `occurrence` places a component inside the component its
`parent` names: its `transform` is a column-major 4×4 matrix `m[column][row]`
with the translation in `m[3]`, and its geometry stays in the component's
frame, so two occurrences of one component share its bodies. A `grounded`
occurrence does not move. The `assembly` section holds the `joints`, the
`links` between them, and the motion `studies`.

A joint mates the frame `a` of one occurrence to the frame `b` of another. A
snap names a frame: `frame` (an `origin`, a `z` axis, and an `x` axis), or
`point` (a point with an optional `z`, default +Z, and `x`). Frames of the
other snaps (`face_center`, `circle_center`, `edge`, and the plane snaps)
come from the model. By default the two z axes oppose, as two faces meet;
`flip` makes them agree. `offset_xy` moves the frame of `a` along its x and
y, `offset` moves the frame of `b` along its z, and `angle` turns it about
z. `values` hold the current degrees of freedom (an angle for `revolute`, a
distance for `slider`, an angle and a distance for `cylindrical`) and
`limits` hold one range, or `null`, for each.

The cache names every face of a model body in `names[k].faces`, in face
order; the model body's `names` is the index `k`.

### 3.8 Appearances

`appearances.bodies` maps a body name to a look: `name`, `color` as
eight-bit RGB, `opacity`, and optional physically based parameters.
`appearances.faces` lists face looks: the `body`, a `point` on the face, the
face's persistent `name`, and the `look`. The model cache's `names` records
list each body's face names in face order.

### 3.9 Presentation and application data

`materials` maps a body name to a material name, and `user_materials` lists
the user's materials with a `density` and an `appearance`. `visibility`
lists the `hidden_bodies` by name and the `hidden_occurrences` by path of
occurrence ids, with other hidden and shown sets. A named view is an orbit
camera: its eye is `target` moved back by `distance` along `(-sin yaw cos
pitch, -cos yaw cos pitch, sin pitch)`, and an `fov` of 0 is orthographic.
The sections `metadata`, `plugin_data`, `render`, `favorites`,
`browser_groups`, `timeline_groups`, `browser_order`, `configs`, `plastic`,
`sheet`, `parts`, `text_params`, `param_comments`, `print_faces`,
`body_components`, `body_offsets`, `notes`, `canvases`, and `point_clouds`
are application data that a rewrite keeps as the section.

A note is `{id, anchor, text, hidden}`: its text callout anchors at a design
point. A canvas is a reference image placed on a plane: its bytes are the
member `image_blob` of a container or the base64 `data` of plain JSON, and
`plane`, `center`, `width`, `angle`, `opacity`, `flip`, `locked`, and
`component` place it. A point cloud keeps its points as the blob
`points_blob` of a container (plain JSON: base64 `points` and `colors`) in
`SCPNTS01` form: the magic, the point count and a colour flag as u64
little-endian words, then x, y, z per point as f32 and, when flagged, r, g, b
per point as bytes. The points are millimetres from the cloud's `origin`,
and its `transform`, a 4 by 4 row-major matrix, moves them into the design.

### 3.10 Earlier formats

A design of an earlier format reads through one upgrade step for each format
it passes. Most steps add a field that older designs leave at its default.
The steps that change what a reader sees are: format 3, the appearances of a
body are looks, not bare colours; format 4, a joint's `flip` has the opposite
sense; format 5, a feature of a design with components lists its
`participants`; format 6, the XZ origin plane has the normal +Y, and an older
reference keeps the frame with the x axis along +X and the y axis along +Z;
format 28, a two-sided extrude follows its `direction`; format 40, the seeds
of a pattern or mirror and a construction plane reference are feature ids; and
format 51, the second distance of a two-sided extrude stays on its own side
unless `second_follows_first` is set.

A two-sided extrude has the sides `first` (against the normal for a negative
`direction`) and `second` of `distance2`. A distance keeps its side whatever
its sign, so an extrude whose sides do not lie on the two sides of the sketch
plane has no neutral two-sided form.

### 3.11 Exactness

A value the document states is byte-exact and carries no note. Three kinds
of value are not stated, and each is noted in the source fidelity:

- A parameter's `value` evaluated from an expression that names other
  parameters is `derived`.
- The `expression` of a dimension parameter is this decoder's text, built
  from the dimension's value, so it is `inferred`.
- The `operands` of a grounded joint and the `schema` of a look are this
  decoder's own construction, so they are `inferred`.

A default that the migration of an earlier format supplies is an inference
of the migration, not a value of the document. Its records carry no
note; the migrated XZ frame and the participants of an earlier design are
the open cases.

### 3.12 Plastic features

`boss`, `lip`, `rest`, and `snap_fit` have no neutral operation in this
format. Each decodes as a native operation of its kind, with its scalar
fields (`position`, `direction`, `height`, `width`, `thickness`, and so on)
and its parameter names. Each is charged `solidcraft/history.features-undecoded`.
Their geometry is in the model cache, so the body they make decodes; only the
operation is native. A neutral form needs an operation in the IR, which is a
change to the shared feature schema and is not made by this codec.

### 3.13 Body transfers

A `body_transfer` records that the named `bodies` change owner from the
component `from` to the component `to`, with a `transform` that converts
their geometry into the destination frame. The cache already holds each body
in its final component, so the owner is decoded from the cache. The record is
a reparent of the body's owner: `source_owner` and `target_owner` name the
two product definitions, and `target_transform` carries the placement. The
body names are not carried by the neutral record.

### 3.14 Pending changes

A design that lists `format_pending` is written by a build whose next format
has changes no release has shipped. The codec reads such a design as the
format after the newest released one, when every listed change is one of the
eleven it knows by name:

`bounding-solid-side-margins`, `joint-lateral-offsets`,
`move-rotate-about-edge`, `render-coat-flakes`, `sketch-3d-constraints`,
`sketch-3d-dimensions`, `sketch-3d-relations`, `sketch-reference-placements`,
`step-import-solid-items`, `surface-sweep-exact-spline-path`, and
`whole-profile-holes-share-a-curve`.

A listed change the decoder reads is applied: `joint-lateral-offsets` gives
the joints their `offset_xy`. A change it does not read is named by a loss.
A design listing any other change stays on the residual path.
