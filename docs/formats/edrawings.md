# SolidWorks eDrawings package (`.eprt`, `.easm`): Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

This specification covers the container of the eDrawings part (`.eprt`) and assembly (`.easm`) package, the wrapper around the model stream in its `eModel` member, and the model stream that the codec decodes. Unknown parts are in [`edrawings-open-items.md`](edrawings-open-items.md).

## 1. Package

### 1.1 Container

A package is a ZIP archive with the framing in [`zip.md`](zip.md). The first local member is `eModel`. Members use the DEFLATE method (8).

### 1.2 Members

| Member name | Present in | Role |
| ----------- | ---------- | ---- |
| `eModel` | part, assembly | The model stream (§2). |
| `preview.jpg` | part, assembly | JPEG preview image. |
| `OLE` | part, assembly | 12 bytes: `38 4a 00 00` and eight zero bytes. |
| `scene.xml` | part, assembly | XML `SceneData` root with environment and lighting. |
| `materials.xml` | assembly | XML material table. |
| `FileProperties.xml` | assembly | XML document properties. |

The codec compares member names byte for byte and requires only `eModel`. The order of the members has no meaning to the codec.

## 2. The `eModel` member

### 2.1 Layout

The member is one byte string in this order:

| Part | Size | Content |
| ---- | ---- | ------- |
| Comment line | 15 bytes | ASCII `;; HSF V<major>.<minor> ` and `0x0a`. |
| `I` record | 5 bytes | Tag `0x49`, then a `u32` little-endian. |
| `B` record | 26 bytes | Tag `0x42`, one flag byte, then six `f32` little-endian. |
| `Z` record | to the end of the member | Tag `0x5a`, one zlib stream (§2.3), and the tail. |

The six `f32` of the `B` record are the minimum corner `x y z` and the maximum corner `x y z` of a bounding box. The unit is not known (open item EDR-02). The flag byte and the `I` value have no known meaning (open item EDR-01).

Records follow in file order. Each starts with a tag byte, then the fields of that record, and the wrapper ends after the `Z` tag. A tag other than `I`, `B`, and `Z` is an error. A record occurs at most once. At most eight records precede the `Z` tag. The codec accepts any order of `I` and `B` and does not search for the `Z` tag. With the comment line, the `I` record, and the `B` record in this order, the zlib stream starts at offset 47 (15 + 5 + 26 + 1); the layout table `docs/layouts/edrawings.toml` gives the offsets.

### 2.2 Stream version

The comment line states the HSF stream version. The codec maps the text `19.10` to the dialect `edrawings:hsf-19-10`, the text `18.00` to the dialect `edrawings:hsf-18-00`, and the text `19.00` to the dialect `edrawings:hsf-19-00`. Every other version text is `edrawings:unknown`, and its stream is read with the rules of `edrawings:hsf-19-10`. 

### 2.3 Zlib stream and tail

The zlib stream starts at the first byte after the `Z` tag. The stream ends with the Adler-32 sum, and its end is found by inflating, not by a stored length. The bytes after the end of the stream are the tail. The decoder transfers no tail byte (open item EDR-05). A tail other than one zero byte is kept out of the model and reported as `edrawings/stream.trailing-bytes`.

The inflated bytes are the HOOPS Stream Format body. Its first byte is `5b`, the ASCII character `[`. Section 3 gives the layout of the body.

The codec inflates under the session decompression caps. An inflated size above the per-expansion cap is a resource refusal, not a malformed file.

## 3. The model stream

The model stream is the inflated body of the `eModel` member. It is a sequence of records. Each record starts with an opcode byte. Every opcode has its own rule for the length of the record, so the reader never searches for the next record.  The walk from the first byte to the end record leaves no byte over.

### 3.1 Conventions

- All integers and floats are little-endian.
- A `pstr` is a `u8` length and that many bytes of Latin-1 text.
- A matrix is twelve `f32`: nine values for the three rows of a 3x3 block, then three values for the translation.
- A length is in metres. The codec multiplies by 1000 to give millimetres.

### 3.2 Stream layout

| Part | Content |
| ---- | ------- |
| Header block | `[`, a `u32` L, L + 1 bytes, `]`. The content has no use for decoding. |
| Camera | `}`, one byte, eleven `f32`, a `pstr`. |
| User options | `U` records. |
| Line styles | One optional `h` record. |
| Segment tree | Segments that hold the document, the sheets, and the instance tree (§3.5). |
| Separator | One byte `0x01`. |
| Object store | One segment with shells and shell instances in any order (§3.6). |
| Separator | One byte `0x01`. |
| Side objects | `s` records, each closed by `)` (§3.9). |
| Separators | Two bytes `0x01`. |
| Trailer | `[`, 22 bytes that end with `]`, then the end record. |

The reader checks the header with its length. It accepts a bracket record at any other offset only when it is the trailer: the bytes from `[` through `]` and the final `z` are 24 bytes and end the stream. The decoder does not need the order of the parts. It takes the records as they come.

### 3.3 Records

| Opcode | Name | Bytes after the opcode |
| ------ | ---- | ---------------------- |
| `[` | Header or trailer | See §3.2. |
| `}` | Camera | 45 bytes, then a `pstr`. |
| `U` | User option | A `u16` n, then n bytes of Latin-1 text. |
| `0x16` | Wide user option | A `u16` n, then n UTF-16LE characters (2n bytes). It has the role of `U`. |
| `h` | Line styles | One version byte (1). Then, while the next byte is `J`: that byte, a `pstr`, a `u16` n, and n bytes. |
| `(` | Open segment | A `pstr` name. |
| `)` | Close | None. It closes a segment or a side object. |
| `<` | Include | A `pstr` path. |
| `{` | Style | A `pstr` path. If the byte after `{` is 0: that byte, a `pstr` path, and a `pstr` condition. |
| `?` | Condition | A `pstr`. |
| `%` | Modelling matrix | A matrix. |
| `$` | Texture matrix | A matrix. |
| `~` | Colour | One or more mask bytes, then red, green, blue. A mask byte with the top bit set (0x80) is followed by another mask byte. |
| `"` | Material | Two flag bytes and a `pstr`. If bit 3 (`0x08`) of the second flag byte is set, seven more bytes follow the `pstr`. In `edrawings:hsf-18-00` and `edrawings:hsf-19-00` the colour of a body comes from it (§3.11). |
| `0` | Tag | A `u32`. |
| `=` | Line weight | An `f32`. The record is the first of the chain `=`, optional `-`, `0x0e`, optional `0x0d`, `~`. |
| `0x0e` | Edge weight | An `f32`. |
| `-` | Line pattern | The byte `0xff`, then a `pstr` that names a line style. |
| `0x0d` | Edge pattern | The byte `0xff`, then a `pstr` that names a line style. |
| `@` | Hole marker symbol | One byte, the symbol code. |
| `H`, `R`, `V` | Attributes | See §3.4. |
| `q` | Object slot | None. |
| `S` | Shell or shell placeholder | See §3.6. |
| `&` | Shell instance | See §3.7. |
| `s` | Side object | See §3.9. |
| `0x01` | Separator | None. |
| `0x00` | Pad | None. The reader accepts a zero byte between records. |
| `z` | End | None. No byte follows it. |

An opcode that is not in the table is an error. The reader never skips unknown bytes.

### 3.4 Attribute records

`H` and `R` start with mask words, then the same number of value words, then extra bytes. An `H` word is a `u16`, and an `R` word is a `u32`. The top bit of a mask word means that another mask word follows. The reader accepts at most four mask words. The number of extra bytes depends on the list of mask words. The reader holds a table of the admitted lists, and a list that is not in the table is an error. The table holds five `H` lists and seven `R` lists. The `R` list `0x00100000` has 16 bytes and then a run: pairs `ff 01` or single bytes `ff`. The first two bytes after the 16 bytes select the form, and the run ends at the first byte that does not continue it. The records carry no geometry, and the decoder discards them. The meaning of the bits is not known (open item EDR-07).

`V` has fixed forms. The forms are three of fixed length and one texture definition: the bytes `40 00 74`, two `pstr`, and six bytes. Another form is an error. A `V` of a fixed form before a `q` marks a text slot, and a texture definition before a `q` marks a texture slot.

### 3.5 Segment tree

A segment has a name and holds records and other segments. The reader keeps the stack of open segments. A close without an open segment is an error, and an end record inside an open segment is an error. The depth limit is 256.

The instance tree is below the segment `includes`. It has these nodes:

- `assempart<N>` and `instance<N>` are placed instances. Each holds a `part` segment. The `part` segment holds the matrix `%` and one or more includes `<` of body paths. A `part` without a matrix is placed at the identity.
- `subassem<N>` is a sub-assembly. It holds nodes. A matrix on a sub-assembly is open item EDR-09.

A node may hold a `U` record with a `name=` field, and a `reference` segment. The `reference` segment holds the segments `refname` and `path`. Each of them holds a text slot. A node may hold a segment `name` with a text slot. A `reference` segment outside every node names the document.

A library body is a segment whose name holds `/shared/body` and starts with `/include library/` or with `../`. The relative form is a chain of `../` components that ends in `<name>_include/shared/body<N>`. The first instance that uses a body defines it inside its `part` segment. The definition holds the colour `~` and sub-segments. Each sub-segment holds shell placeholders. The further instances of a body hold only the include. The include path and the definition name are equal.

### 3.6 Object slots and shells

Every `q` takes the next object id, from 0. The k-th `q` of the stream refers to object k of the stream. The ids of the shells, the shell instances, and the side objects form the range from 0 up to the number of `q`, with no gap and no repeat. A repeated id is reported, and the first object is kept. A text slot has a `q` after the `)` that closes its segment. A shell slot has the placeholder, then `q`, in its sub-segment.

A shell placeholder is the bytes `S 90 02 00 00`. A shell is stored in the object store as:

| Field | Size | Meaning |
| ----- | ---- | ------- |
| `S`, `0x0e` | 2 | Opcode and a constant. |
| Id | 4 | The object id. |
| Zero | 1 | Constant. |
| N | 4 | The vertex count. It is at least 1 and at most 2^24, and the vertices must fit in the stream. |
| Vertices | 12 N | `x y z` as `f32`. |
| `0x01`, L | 5 | Face list marker and length. |
| Face list | L | One byte for the index width (8 or 16), then strips. A strip is a count and that many indices, each of the index width. |
| `01 07 0C`, M | 7 | Normals marker and length. |
| Normals | M | The decoder skips them. It computes normals from the triangles. |
| Range block | 0 or 24 + K | Optional (§3.6.1). |
| Face tag | 0 or more | Optional (§3.6.1). |
| Zero | 1 | Terminator. |

The width is 8 when N is at most 254 and 16 when N is at least 261. The width for N from 255 to 260 is open item EDR-15. A strip of n indices gives n - 2 triangles. Triangle i of a strip s is (s[i], s[i+1], s[i+2]) for even i and (s[i+1], s[i], s[i+2]) for odd i. Every triangle is counter-clockwise seen from outside, and each inner edge of a closed body is used once in each direction. A strip index outside the shell, a strip longer than the list, or another width makes the shell invalid. The decoder leaves an invalid shell out of its body and reports it.

One shell is the display mesh of one surface patch. Shells do not share vertex indices, so the decoder welds points by position to rebuild the adjacency. Shells of neighbouring surface patches tessellate their common curved edge independently: a vertex of one shell can lie on an edge of the other shell instead of at one of its vertices (a T-junction). Copies of one point differ by about one single-precision step of the coordinate.

### 3.6.1 Range block and face tag

The byte after the normals selects the optional parts:

- `03` starts the range block: `03 01 02`, four `f32` `u0 v0 u1 v1`, the byte `08`, a `u32` K, and K bytes. `u0` and `u1` are 0, and K is 2N, one `u16` for each vertex. Each of the K bytes is `00` or `ff`.
- `63` starts the face tag: `63`, a user option (`U` or `0x16` with its operands), and a zero byte. The option text has the form `swfaceid=<decimal>`.
- Any other byte must be the zero terminator.

The range block comes before the face tag. A block with another head, a non-zero `u`, or another K is an error.

### 3.7 Shell instances

An `&` record has the opcode, a `u64` source id, a `u64` own id, one byte (1), and a matrix. The source is a shell. The instance uses the strips of the source and applies the matrix to its points. The matrix is local to the body. The matrices can have a negative determinant, and then a change of the winding makes the body inconsistent. The decoder never changes the winding of an instance. The decoder resolves one level. An instance of an instance is invalid. An own id above 32 bits is an error.

### 3.8 Matrices

A matrix `%` or `&` is applied to a row vector: `p' = p * R + t`, where R is the 3x3 block and t is the translation. The rows of R are the images of the basis vectors. The matrices of `part` segments are absolute in the assembly, so the decoder does not compose them with a parent node. The decoder keeps a matrix of a sub-assembly as the transform of the sub-assembly occurrence and reports it (open item EDR-09).

### 3.9 Side objects, colours, names, and display states

A side object is `s` and a `u32` id. It opens a scope that the next `)` closes. The next byte selects the kind:

- `x` is a text object: 12 zero bytes, the byte 5, a length byte, the UTF-16LE text, and a zero byte. If the length byte is `0xfe`, a `u16` length follows. The length counts bytes.
- `i` is an image object: 12 zero bytes, a `u32`, a `pstr`, nine bytes, a `u32` length, the image bytes, and the bytes `00 ff`.
- `X`, `l`, `C`, `c`, or `0x10` starts a geometry object (§3.9.1).
- Any other byte starts the next record, and the object is empty.

The colour of a body is the `~` record directly in its library segment. The display name of an instance is the text of its `refname` slot, then the text of its `name` slot, then its `name=` option, then its segment name. The document name is the text of the `refname` slot of the `reference` segment outside every node. The decoder does not read the `path` slots, because they hold file paths of the author.

The segment `displaystates` holds a `U` record with a `current=` field and one segment per display state. A display state holds one segment per node with a `U` record of `path=` and `vis=on` or `vis=off`. The decoder applies the current state to the nodes whose segment name is the last component of the path.

### 3.9.1 Geometry objects

A geometry object is a run of items that continues up to the byte that is no item:

| Opcode | Item | Bytes after the opcode |
| ------ | ---- | ---------------------- |
| `X` | Point | Three `f32`. |
| `l` | Line | Six `f32`: two points. |
| `C` | Three-point arc | Nine `f32`, then a zero byte. |
| `c` | Circle | Nine `f32`, then a zero byte. |
| `0x10` | Polyline set | Two bytes, a `u32` T, `u32` point counts until their sum is T, then T `f32` of the `x` array, T `f32` of the `z` array, and one `f32` of the `y` value for each polyline. |

A sum above T is an error. The object takes one object id like every side object, and the `q` after its `)` carries that id. The segments that hold hole marker symbols (`star` and `plus`) hold two `X` items, and the segments that hold axes hold one `l` item.

Hole markers are 2D annotation outside every body. A hole marker segment is `(axisNN`, a `U` option, `-`, `0x0d`, `~`, `(star` or `(plus`, `@`, `)`, `q`, `)`, `q`.

### 3.9.2 Text options

A user option is a list of `key=value` fields separated by commas. A value that starts with a backtick ends at the next backtick and may hold commas. The `name` field gives the name of a node, whatever other fields the option holds (for example `mbd_comp_name`, `showbits0`, and `componentdescription`). `U` and `0x16` options have the same fields.

### 3.10 Limits

The reader refuses a stream that exceeds these limits: segment depth 256; 100000 tree nodes; 100000 library bodies; 8000000 object slots; 8000000 objects; 200000 display components; 2^24 vertices in a shell. The B-rep builder limits the mesh of one body to 5000000 triangles and 15000000 points. The decoder charges the entities and the retained mesh bytes to the session.

### 3.11 Streams written by SolidWorks
The dialect `edrawings:hsf-18-00` holds the streams that declare version `18.00`. The rules below hold for the streams that declare version `18.00` (open item EDR-10). Every rule of sections 3.1 to 3.10 that this section does not name holds unchanged: the shell record, the shell instance, the object slot, the text object, the camera, the user option, the matrix, and the end record.

| Record | Rule |
| --- | --- |
| Header block `[` | The closing `]` stands L + 7 bytes after the opcode. |
| Trailer `[` | The 24 bytes are `09 00 00 00 be 2c`, 15 zero bytes, and `]z`. |
| `V` text form | The 8 bytes are `ff 00 ff ff 00 00 03 00`. Forms `ff 00 ff ff 00 00 07 00` and `80 00 00 01 00 00` are not part of this dialect. The 2D form `df 56 0e 20 00 00` is unchanged. |
| `t` | The opcode, two `pstr`, and the six bytes `01 80 02 00 01 01` define a texture. A `V` with the two bytes `40 00` follows and marks a texture slot. |
| `"` material | The `pstr` reads `diffuse color=(r=<f> g=<f> b=<f>)`, or in the backtick form of §3.9 followed by `,diffuse texture=(thread-mod)`. The flags `01 09` come with an empty `pstr` and the seven extra bytes of §3.3. |
| `~` | Does not occur in the body library. The colour comes from the materials. The records of the 2D annotation segments keep the form of §3.3. |
| `R` | Mask `0x00100000` with value `0x00100000`, then 16 bytes and a run of the pair `ff 01`. |
| `H` | Mask `0x0004` has one extra byte. Masks `0x0002` and `0x8000 0x0002` keep their extra count. |
| Line styles | A run of `J` entries (§3.3) may stand without `h` and the version byte. An `h` record with the version byte may hold no entry. |
| Image object `i` | One byte replaces the `u32` after the 12 zero bytes. |
| `0`, `?`, `{ 00`, style segments | Are not part of this dialect. |

An opcode, a mask list, or a `V` form that is not in the tables of this section is an error with the byte offset.

### 3.12 Streams of version 19.00

The dialect `edrawings:hsf-19-00` holds the streams that declare version `19.00`. The rules of §3.11 hold with these differences:

| Record | Rule |
| --- | --- |
| Image object `i` | The `u32` of §3.9 stands after the 12 zero bytes, as in `edrawings:hsf-19-10`. |
| Line styles | An `h` record with the version byte precedes the `J` entries. The bare run of §3.11 is also accepted. |
| Trailer `[` | The 24 bytes are `09 00 00 00 be 2d`, 15 zero bytes, and `]z`. |

The tree, the colours, and the visibility follow §3.11 and §4. The hole marker segments of §3.9.1 occur in the 2D branch.

The tree has these nodes below `includes`:

```
subassemK  := ( reference  name  ( subassemJ | assempartJ )* )
assempartK := ( reference  part  name  [H 0x0002]  [U default*=...]  [material]  [V]  [R] )
reference  := ( (refname V q) (path V q) [(configname V q)] )
part       := ( ( [BODYDEF] < path )+  % matrix )
BODYDEF    := ( /include library/shared/bodyN  bodyNsub0 ... [material] )
```

A `subassem` node has no matrix and no body. An `assempart` node is one placed part: its `part` segment holds a list of includes and ends with one matrix. The library path has no `model_K_include` level. The first part that includes a body defines it in place; later parts hold only the include. A part may include many bodies, and all of them share the matrix of the part.

A shell instance may name a source shell of another body: the source id is a key of the global object table.

The matrix of a part is its absolute placement.

The materials give colours at three levels: the last item of a library body, the last item of a sub-segment, and a direct item of a part. A part-level material comes with the option `defaultcolor=<r> <g> <b>,defaulttransmission=<t>`. A sub-segment is a group of faces with its own material. The material of the body is the default colour of the body.

The visibility of a part comes from the display state that the `current=` field of `displaystates` names, or from the state that its `default=` field names when no current state is named. How a listing, `defaultvisibility=off`, and the listing of an ancestor combine is open item EDR-14.

## 4. Mapping to CADIR

The stream holds display meshes and no exact surfaces. The decoder builds a faceted B-rep for each library body that an instance uses.

1. All shells of the body are joined. Their points are in millimetres in the frame of the body, and the strips give counter-clockwise triangles.
2. The B-rep builder welds points that are closer than 1 micrometre (or 8 single-precision steps of the largest coordinate when that is larger), drops degenerate triangles, splits every unpaired edge at the unpaired-edge vertices that lie on it (within 10 micrometres) so that T-junctions close, merges unpaired-edge end points that are closer than 25 micrometres so that cracks close, pairs the triangles around an edge that more than two triangles share when they alternate in direction, joins the triangles into components through paired edges, drops closed components of zero volume, reverses a closed component whose signed volume has the wrong sign for its nesting, merges coplanar triangles into planar faces with straight edges (a planar patch that covers part of its plane twice becomes one face per triangle), and groups the faces of each component into one shell. A closed component that lies inside no other closed component is an outer shell with a positive signed volume and its own region. A closed component that lies inside one other closed component is a void shell with a negative signed volume; it is a further shell of the region of the component that holds it. Containment counts the closed components that enclose a point of the component, so a solid inside a void is an outer shell of a further region. A body whose edges are all shared by two triangles of opposite direction is a solid. Any other body is a sheet. Features smaller than 50 micrometres need the finer weld, because a coarser weld collapses them into degenerate triangles. A body that is a sheet after this build is built a second time with a weld distance of 1 nanometre (raised to 8 single-precision steps of the largest coordinate when that is larger), and the second build replaces the first only when it is a solid. A plate as thin as 1 micrometre welds flat at the first build, and the second build keeps it. The decoder does not join sheet bodies and does not add the missing faces (open item EDR-19).
3. A file with one instance and no sub-assembly is a plain part. It has one part definition that holds the body, and no occurrence. The matrix of the instance moves the points of the body. A matrix with a negative determinant reverses the triangles.
4. Any other file is an assembly. Each distinct list of bodies (body set) of an instance has one part definition that holds one CADIR body for each body of the list, and all instances with that list use the definition. A body that more than one body set lists has one CADIR body for each such set, built from the same shells, with the same name and colour, so that each CADIR body has one owning definition. The assembly has one group definition, and each sub-assembly has one group definition. A sub-assembly occurrence has the transform of its matrix when it holds one (§3.8), and the identity transform otherwise. An instance occurrence has its absolute matrix with the translation in millimetres. The CADIR transform has the column-vector form, so row i of its linear part holds the i-th element of each row of R.
5. The names of the occurrences are the display names. An occurrence that the current display state hides has `visible` set to false. In `edrawings:hsf-18-00` and `edrawings:hsf-19-00` the `visible` value of an unlisted part follows the reading of EDR-14.
6. In `edrawings:hsf-18-00` the colour of a body is its body material. The colours of sub-segments that differ from it, the colour and transparency overrides of parts, and those of display states are reported and not transferred.

Body ids come from the scope `body<k>`, with k the order of first use. An occurrence id comes from the index of its tree node in stream order, so an instance that has no occurrence keeps the id that it would have.

### 4.1 Parts

The decoder gives up these units of the package as parts. The part kind names the unit in a `record.unreadable` loss. The offset of a part in the model stream is an offset in the inflated stream, which the loss names `eModel`.

| Part kind | Unit | Framing |
| --- | --- | --- |
| `archive-entry` | The `eModel` member, when its zlib stream does not open or its model stream breaks a record rule | The ZIP entry (§1.2) |
| `model-object` | One shell, shell instance, or text object record that the decoder does not transfer | The record (§3.6, §3.7, §3.9) |
| `tree-node` | One instance node whose includes name no library body | The node segment, from its open record to its close record (§3.5) |

The decoder gives up a `model-object` part for a shell or shell instance that does not decode, for the object of a shell slot outside every library body, and for each later object whose id an earlier object has. The object of a shell slot outside every library body is not given up when a shell instance names it as its source.

A record that the decoder drops only because it needs a given-up record is not given up by itself. The loss of the given-up record lists it as dropped. A body whose shells all fail to decode is dropped with the first of those shells. An instance whose bodies were all not built is dropped with the first body of its body set.

## 5. Detection

Detection reads the byte prefix only. A prefix that starts with the ZIP local header, whose first member is named `eModel`, and whose first bytes open with `;; HSF V`, has high confidence. For a DEFLATE member the probe inflates the first 1024 compressed bytes. A package that names an `eModel` member any other way has medium confidence. Any other input has no confidence, including a ZIP without an `eModel` member. The file extension takes no part.

## 6. Losses

| Code | Meaning |
| ---- | ------- |
| `edrawings/source.dialect-unverified` | The stream version is none of `19.10`, `18.00`, and `19.00`. |
| `edrawings/container.directory-recovered` | The ZIP central directory was unreadable. |
| `edrawings/stream.malformed` | The zlib stream did not open. |
| `edrawings/stream.trailing-bytes` | Bytes other than one zero byte follow the zlib stream. |
| `edrawings/model.malformed` | A record rule of the model stream failed (severity Blocking). The message gives the byte offset. The package carries no model data. |
| `edrawings/model.no-geometry` | The model stream has no instance with a body that the decoder could build. |
| `edrawings/model.object-invalid` | A slot, id, or include is unusable: a repeated object id, a shell slot outside every library body, or an instance without a library body. |
| `edrawings/model.shell-invalid` | Shells were left out of their body (§3.6). |
| `edrawings/geometry.reconstructed-from-mesh` | Bodies were rebuilt from display meshes as planar faces. It is present whenever a body is built. |
| `edrawings/geometry.open-body` | Bodies are not watertight and became sheets. |
| `edrawings/geometry.body-reversed` | Closed components whose signed volume had the wrong sign for their nesting had their triangles reversed. |
| `edrawings/geometry.degenerate-triangles` | Degenerate triangles were dropped, and closed components without a usable volume (zero-volume pieces and cavities thinner than the plane tolerance) were dropped. Open slivers below the part size and closed shells of fewer than four faces were dropped, and faces without an area were removed with their neighbours joined. The message counts each kind and the triangles of the dropped components. |
| `edrawings/geometry.body-failed` | A body could not be built from its mesh. |
| `edrawings/structure.placement-invalid` | An instance matrix has a value that is not finite. The identity replaced it. |
| `edrawings/appearance.face-colour-reduced` | The materials of sub-segments differ from the material of their body. The body colour replaced them (`edrawings:hsf-18-00`, `edrawings:hsf-19-00`). |
| `edrawings/appearance.override-dropped` | Part-level colour overrides and the colour or transparency overrides of display states were not transferred (`edrawings:hsf-18-00`, `edrawings:hsf-19-00`). |
| `edrawings/structure.visibility-inferred` | Occurrences take their visibility from `defaultvisibility=off` or from a listed ancestor, or keep their own listing against a listed ancestor with the other value (EDR-14). |
| `edrawings/annotation.not-transferred` | Geometry and image side objects (§3.9, §3.9.1) were read and not transferred. The message counts them. |
| `edrawings/structure.subassembly-placement-assumed` | A sub-assembly holds a matrix, and the decoder took it as the transform of the sub-assembly occurrence (§3.8, EDR-09). |
| `edrawings/structure.display-states-dropped` | Display states other than the current state were not applied (§3.9, EDR-09). |
| `edrawings/structure.occurrence-dropped` | An instance has no occurrence because none of its bodies was built. Its placement was not transferred. |

All codes are Warning except `edrawings/stream.trailing-bytes` (Info) and `edrawings/stream.malformed` and `edrawings/model.malformed` (Blocking).
