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
