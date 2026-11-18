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
