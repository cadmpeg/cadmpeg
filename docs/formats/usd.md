# OpenUSD layers (`.usda`, `.usdc`, `.usd`): Format Specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

This specification covers the OpenUSD layer encodings and the scene-description schemas for geometry, transforms, and shading. Section 2 to section 8 give the USDA text encoding. Section 9 gives the USDC binary encoding. Section 10 to section 16 give the schemas.

Record offsets, field widths, and endianness are also maintained as a machine-checked table in [`docs/layouts/usd.md`](../layouts/usd.md), generated from `docs/layouts/usd.toml`. That table is the canonical source for the numbers; the prose below carries the semantics. Unknown parts are in [`usd-open-items.md`](usd-open-items.md).

## 1. Encodings and file names

A layer is one file of scene description. A layer has one of two encodings:

| Encoding | First bytes | Section |
| --- | --- | --- |
| USDA text | `#usda ` or `#sdf ` | 2 to 8 |
| USDC binary (Crate) | `PXR-USDC` | 9 |

The file name extension `.usda` names a text layer and `.usdc` names a binary layer. The extension `.usd` names a layer of either encoding. The first bytes, not the extension, select the encoding.

A `.usdz` file is a package: a ZIP archive of layers and assets. Every member is stored with no compression and no encryption, and the data of every member starts at an offset that is a multiple of 64 bytes. The first member is the root layer of the package; its name has the extension `.usda`, `.usdc`, or `.usd`. An asset path in a package layer that has no scheme and is not absolute names a member of the package, relative to the directory of the layer member.

## 2. USDA lexical structure

### 2.1 Characters and lines

A USDA layer is UTF-8 text. A line ends at LF (`0x0a`) or CR (`0x0d`). A CR LF pair is two line ends; an empty line between them has no effect.

The grammar uses three classes of padding:

- **Inline padding** is a space (`0x20`), a tab (`0x09`), or a block comment.
- **Single-line padding** is inline padding or a line comment.
- **Multiline padding** is single-line padding or a line end.

A line end is significant. It separates statements (§3.3). Multiline padding is permitted only where this specification says so: inside brackets, parentheses, and braces, and between statements.

### 2.2 Comments

| Form | Extent |
| --- | --- |
| `#` | to the end of the line |
| `//` | to the end of the line |
| `/*` | to the next `*/`, across lines |

The header line (§3.1) is not a comment.

### 2.3 Token separator

Two adjacent word tokens are separated by at least one space or tab. A block comment between them does not replace that space: `def /*c*/"A"` is valid and `def/*c*/"A"` is not.

### 2.4 Identifiers

An identifier is a UTF-8 identifier: a first character of class XID_Start or `_`, then zero or more characters of class XID_Continue.

A **namespaced identifier** is one or more identifiers joined by `:`, for example `primvars:displayColor`. Property names (§6) are namespaced identifiers or keywords.

A **type identifier** is one or more identifiers joined by `::`, where no part is a keyword (§2.5).

### 2.5 Keywords

These words are keywords:

`add`, `AnimationBlock`, `append`, `bezier`, `class`, `config`, `connect`, `curve`, `custom`, `customData`, `default`, `def`, `delete`, `dictionary`, `displayUnit`, `doc`, `held`, `hermite`, `inherits`, `kind`, `linear`, `loop`, `nameChildren`, `None`, `none`, `offset`, `oscillate`, `over`, `payload`, `permission`, `post`, `pre`, `prefixSubstitutions`, `prepend`, `properties`, `references`, `relocates`, `rel`, `reorder`, `rootPrims`, `repeat`, `reset`, `scale`, `sloped`, `subLayers`, `suffixSubstitutions`, `specializes`, `spline`, `symmetryArguments`, `symmetryFunction`, `timeSamples`, `uniform`, `variantSet`, `variantSets`, `variants`, `varying`.

A keyword matches only as a whole word: the next character is not an identifier character. A keyword is not a type identifier and not a bare value identifier. A property name can be a keyword.

### 2.6 Numbers

```text
number   = [ "-" ] ( digits [ "." digits? ] [ exponent ]
                   | "." digits [ exponent ]
                   | "inf" )
         | "nan"
exponent = ( "e" | "E" ) [ "+" | "-" ] digits
integer  = [ "-" ] digits
```

A number has no leading `+`, no hexadecimal form, and no type suffix. `inf`, `-inf`, and `nan` are the IEEE 754 infinities and quiet NaN.

The declared value type gives the number its type (§7.2). An integer type admits a number whose value, truncated toward zero, is in the range of the type. A floating-point type admits every number; a value outside the range of the type is an infinity of its sign.

### 2.7 Strings

| Form | Delimiter | Line ends |
| --- | --- | --- |
| single-line | `"` or `'` | not permitted |
| multiline | `"""` or `'''` | permitted |

A string ends at the first unescaped closing delimiter. Inside a string, a backslash starts an escape:

| Escape | Value |
| --- | --- |
| `\\` | backslash |
| `\a` `\b` `\f` `\n` `\r` `\t` `\v` | `0x07` `0x08` `0x0c` `0x0a` `0x0d` `0x09` `0x0b` |
| `\x` then up to two hexadecimal digits | the byte of that value; no digit gives `0x00` |
| `\` then one to three octal digits | the byte of that value, modulo 256 |
| `\` then any other character | that character |

The string value is the byte sequence after escape replacement. A value that is not valid UTF-8 is malformed.

### 2.8 Asset paths

An asset path is a reference to an external file.

| Form | Content |
| --- | --- |
| `@` … `@` | any characters except `@` and a line end |
| `@@@` … `@@@` | any characters except a line end; `\@@@` is a literal `@@@` |

A triple-delimited asset path ends at the first run of three to five `@` that is not escaped. The last three `@` of the run close the path. The zero to two `@` before them are part of the path.

### 2.9 Path references

A path reference is `<` then a scene path then `>`, or the empty reference `<>`. A scene path is absolute (`</World/Geom>`) or relative (`<../Geom>`, `<.points>`). A path names a prim, a property (`</World/Geom.points>`), a relationship target, or a variant selection (`</A{v=x}B>`). A relationship target (§6.3) cannot contain a variant selection.

## 3. Layer

### 3.1 Header

The first line of a layer is the header. The header is the text `#usda ` or `#sdf `, then a version, then optional text to the end of the line.

| Header | Version |
| --- | --- |
| `#usda 1.0` | USDA 1.0 |
| `#usda 1.1` | USDA 1.1: spline tangent algorithms (§6.2.4) |
| `#usda 1.2` | USDA 1.2: array-edit values (§6.2.5) |
| `#usda 1.3` | USDA 1.3: spline loops with a boundary time (§6.2.4) |
| `#sdf 1.4.32` | the legacy cookie; the grammar of USDA 1.0 |

Each later version adds the named construct and keeps every earlier construct.

### 3.2 Layer grammar

```text
layer        = header line-end
               [ layer-meta ]
               { root-item | multiline-padding }
root-item    = prim-spec single-line-padding* ( line-end | end-of-file )
             | "reorder" sep "rootPrims" "=" name-list
               single-line-padding* ( ";" | line-end | end-of-file )
layer-meta   = "(" meta-sequence(layer-meta-item) ")"
```

`sep` is a token separator (§2.3). `=` is surrounded by optional inline padding.

### 3.3 Statement sequences

A **statement sequence** is one or more statements. Each statement after the first follows a statement separator: a line end or `;`, with optional single-line padding before it and optional multiline padding after it. A trailing separator after the last statement is permitted.

```text
meta-sequence(item) = multiline-padding* [ item { stmt-sep item } [ stmt-sep ] ] multiline-padding*
stmt-sep            = single-line-padding* ( line-end | ";" ) multiline-padding*
```

In a prim body (§5.2), each property, order, prim, and variant-set statement ends with its own separator.

### 3.4 Lists

A **list** is one or more items separated by `,`, with optional inline padding before each `,` and optional multiline padding after it. A trailing `,` is permitted. Brackets, parentheses, and braces that enclose a list permit multiline padding inside them.

```text
list(item) = item { inline-padding* "," multiline-padding* item } [ inline-padding* "," ]
```

## 4. Metadata

### 4.1 Metadata items

A metadata block is `(`, a statement sequence of metadata items, then `)`. Metadata blocks occur on the layer (§3.2), on a prim (§5.1), on a variant (§5.4), on an attribute, and on a relationship (§6).

```text
meta-item     = string                                   ; documentation
              | "doc" "=" string
              | meta-key "=" ( "None" | dictionary | typed-value )
              | list-op sep identifier "=" ( "None" | list-value )
meta-key      = "customData" | "symmetryArguments" | type-identifier
list-op       = "add" | "delete" | "append" | "prepend" | "reorder"
```

A bare string item is the `documentation` field. A metadata key is any identifier: a key that this document does not name is an extension field and keeps its value. On a layer, `customData` and `symmetryArguments` are not keys.

### 4.2 List operations

A list-valued field holds a list operation. The forms are:

| Form | Meaning |
| --- | --- |
| `key = [ … ]` | explicit: the field is exactly these items |
| `key = None` | explicit and empty |
| `prepend key = [ … ]` | put these items before the weaker items |
| `append key = [ … ]` | put these items after the weaker items |
| `delete key = [ … ]` | remove these items from the weaker items |
| `add key = [ … ]` | add these items if absent (deprecated form of `append`) |
| `reorder key = [ … ]` | order the weaker items by this list |

One field can have one statement of each list-op form. In a single layer with no weaker layer, the effective list of a field is its explicit items, or its prepended items then its appended and added items.

### 4.3 Layer metadata

```text
layer-meta-item = string
                | "doc" "=" string
                | identifier "=" ( "None" | dictionary | typed-value )
                | list-op sep identifier "=" ( "None" | list-value )
                | "relocates" "=" relocates-map
                | "subLayers" "=" "[" [ list(sublayer) ] "]"
sublayer        = asset-path inline-padding* [ "(" meta-sequence(layer-offset) ")" ]
layer-offset    = ( "offset" | "scale" ) "=" number
relocates-map   = "{" [ list(path-ref inline-padding* ":" inline-padding* path-ref) ] "}"
```

Layer metadata keys with defined meaning:

| Key | Type | Meaning |
| --- | --- | --- |
| `defaultPrim` | token | the root prim that a reference with no prim path targets |
| `upAxis` | token `Y` or `Z` | the stage up axis; `Y` when absent (§10.1) |
| `metersPerUnit` | double | the length of one scene unit in metres; `0.01` when absent (§10.1) |
| `kilogramsPerUnit` | double | the mass of one scene unit in kilograms |
| `startTimeCode`, `endTimeCode` | double | the authored time range |
| `timeCodesPerSecond`, `framesPerSecond` | double | time scaling |
| `customLayerData` | dictionary | application data |
| `doc`, `comment` | string | documentation |
| `subLayers` | asset-path list | the stronger-to-weaker list of sublayers |
| `colorConfiguration`, `colorManagementSystem` | asset, token | color management |
| `relocates` | path map | namespace relocations |

### 4.4 Prim metadata

```text
prim-meta-item = meta-item
               | "kind" "=" string
               | [ list-op sep ] "references" "=" reference-list
               | [ list-op sep ] "payload" "=" payload-list
               | [ list-op sep ] ( "inherits" | "specializes" ) "=" path-list
               | [ list-op sep ] "variantSets" "=" name-list
               | "variants" "=" dictionary
               | "relocates" "=" relocates-map
               | ( "prefixSubstitutions" | "suffixSubstitutions" ) "=" string-map
               | "permission" "=" identifier
               | "symmetryFunction" "=" [ identifier ]
reference-list = "None" | reference | "[" [ list(reference) ] "]"
reference      = ( path-ref | asset-path inline-padding* [ path-ref ] )
                 inline-padding* [ "(" meta-sequence(reference-param) ")" ]
reference-param = "customData" "=" dictionary | layer-offset
payload-list   = "None" | payload | "[" [ list(payload) ] "]"
payload        = ( path-ref | asset-path inline-padding* [ path-ref ] )
                 inline-padding* [ "(" meta-sequence(layer-offset) ")" ]
path-list      = "None" | path-ref | "[" [ list(path-ref) ] "]"
name-list      = string | "[" list(string) "]"
string-map     = "{" [ list(string inline-padding* ":" inline-padding* string) ] "}"
```

Prim metadata keys with defined meaning:

| Key | Type | Meaning |
| --- | --- | --- |
| `kind` | token | model kind: `assembly`, `group`, `component`, `subcomponent`, or an extension |
| `references`, `payload` | list operation of references | composition arcs (§8.2) |
| `inherits`, `specializes` | list operation of paths | class arcs (§8.2) |
| `variantSets` | list operation of names | the variant sets of the prim |
| `variants` | dictionary of strings | the selected variant of each set (§5.4) |
| `apiSchemas` | list operation of tokens | applied API schemas |
| `active` | bool | `false` removes the prim and its descendants from the stage |
| `instanceable` | bool | the prim's composed content is a shared prototype |
| `hidden` | bool | an application display hint |
| `displayName` | string | the name an application displays for the prim |
| `assetInfo`, `customData` | dictionary | application data |

A prim is a model when its `kind` is `assembly`, `group`, `component`, or a kind derived from one of them, and its parent is the pseudo-root or a prim whose `kind` is `group` or `assembly`. A `component` holds no model below it. A prim with `kind` but with a parent that is not a `group` or `assembly` is not a model.

A reference with an asset path and no prim path targets the `defaultPrim` of the referenced layer. A reference with a prim path and no asset path targets a prim of the same layer stack. A layer offset maps time: `time_in_target = (time - offset) / scale`.

## 5. Prims

### 5.1 Prim specification

```text
prim-spec  = specifier sep [ prim-type sep ] string
             multiline-padding* [ prim-meta multiline-padding* ]
             "{" prim-body "}"
specifier  = "def" | "over" | "class"
prim-type  = type-identifier { inline-padding* "." inline-padding* type-identifier }
prim-meta  = "(" meta-sequence(prim-meta-item) ")"
```

The string is the prim name. A prim name is a UTF-8 identifier (§2.4). Two sibling prims do not have the same name.

| Specifier | Meaning |
| --- | --- |
| `def` | defines the prim |
| `over` | adds opinions to a prim that another specification defines |
| `class` | defines an abstract prim; the stage does not traverse it as scene content |

The prim type is a schema type name, for example `Xform` or `Mesh`. A prim with no type is untyped. An `over` usually has no type.

### 5.2 Prim body

```text
prim-body = multiline-padding* { prim-item multiline-padding* }
prim-item = ( order-stmt | property-spec ) single-line-padding* ( line-end | ";" )
          | ( prim-spec | variant-set-stmt ) single-line-padding* line-end
order-stmt = "reorder" sep ( "nameChildren" | "properties" ) "=" name-list
```

A child prim and a variant set statement end at a line end. A property and an order statement end at a line end or at `;`. A non-empty prim body therefore starts on a new line after `{`; an empty body `{ }` is valid on one line.

`reorder nameChildren` and `reorder properties` state an order for child prims and properties. A name that the prim does not have is ignored. Unnamed children and properties follow the named ones in their authored order.

### 5.3 Name order

The children of a prim, and its properties, have their authored order unless a reorder statement changes it.

### 5.4 Variant sets

```text
variant-set-stmt = "variantSet" sep string multiline-padding* "=" multiline-padding*
                   "{" { multiline-padding* variant } multiline-padding* "}"
variant          = string multiline-padding* [ prim-meta multiline-padding* ]
                   "{" prim-body "}"
```

A variant set holds named variants. A variant body has the grammar of a prim body and adds its opinions to the prim. The `variants` metadata of the prim selects one variant of each set. A selection is a string dictionary entry, for example `variants = { string shape = "round" }`. A set with no selection contributes no opinions.

## 6. Properties

### 6.1 Property specification

```text
property-spec = attribute-spec
              | relationship-spec
              | list-op sep relationship-type sep property-name [ "=" relationship-value ]
              | list-op sep attribute-decl inline-padding* "." inline-padding* "connect"
                "=" connect-value
property-name = namespaced-identifier | keyword
```

A property name is unique among the properties of its prim.

### 6.2 Attributes

```text
attribute-decl = [ "custom" sep ] [ ( "uniform" | "config" ) sep ] type-identifier
                 [ inline-padding* "[" inline-padding* "]" ] sep property-name
attribute-spec = attribute-decl
                 ( inline-padding* "." inline-padding*
                   ( "timeSamples" "=" time-samples
                   | "connect" "=" connect-value
                   | "spline" "=" spline )
                 | [ "=" attribute-value ] inline-padding* [ attribute-meta ] )
attribute-value = "None" | "AnimationBlock" | array-edit | typed-value
connect-value  = "None" | path-ref | "[" [ list(path-ref) ] "]"
```

- `custom` marks an attribute that no schema defines.
- `uniform` marks an attribute that does not vary over time. An attribute without a variability keyword is varying. `config` is a legacy variability.
- `[]` after the type name makes the attribute an array of that type.
- An assignment with a value states the default value. `None` states that the attribute has no default value. `AnimationBlock` blocks the spline and time samples of weaker layers.
- An attribute with no assignment declares the attribute and states no value.

The declaration, `.timeSamples`, `.connect`, and `.spline` statements of one attribute merge into one attribute. They agree on the type name. When two statements set the same field, the later statement sets it.

Attribute metadata uses the items of §4.1 and also these items:

| Item | Meaning |
| --- | --- |
| `displayUnit = identifier` | the display unit name |
| `permission = identifier` | edit permission |
| `symmetryFunction = [identifier]` | symmetry function name |

Attribute metadata keys with defined meaning:

| Key | Type | Meaning |
| --- | --- | --- |
| `interpolation` | token | primvar interpolation (§11.1) |
| `elementSize` | int | primvar element size (§11.1) |
| `allowedTokens` | token list | the permitted values of a token attribute |
| `colorSpace` | token | the color space of a color value |
| `customData`, `doc`, `displayName`, `displayGroup`, `hidden` | various | application data |

#### 6.2.1 Time samples

```text
time-samples = "{" multiline-padding* [ list(time-sample) ] multiline-padding* "}"
time-sample  = number inline-padding* ":" inline-padding* ( "None" | array-edit | typed-value )
```

Each sample maps a time code to a value. A `None` sample blocks the value at that time. Samples follow `,`; a sample without a separator before the next is malformed.

The value of an attribute at a time `t` is:

1. the time-sample value, when time samples are authored: the sample at `t`, or the linear interpolation of the samples on each side of `t` for an interpolable type, or the held value of the previous sample for another type; before the first sample, the first sample; after the last, the last;
2. otherwise the spline value at `t`, when a spline is authored;
3. otherwise the default value.

The **default time** is not a number. At the default time, the value is the default value. A time-sampled attribute with no default value has no value at the default time.

#### 6.2.2 Connections

A `.connect` statement connects the attribute to other attributes. The targets are attribute paths. A list op prefix edits the connection list (§4.2).

#### 6.2.3 Interpolable types

The floating-point scalars, vectors, matrices, and quaternions are interpolable. Quaternions interpolate spherically. Other types are held.

#### 6.2.4 Splines

```text
spline = "{" multiline-padding* [ list(spline-item) ] multiline-padding* "}"
```

A spline is a one-dimensional animation curve of a floating-point scalar attribute. Its items are a curve type (`bezier` or `hermite`), pre- and post-extrapolation, a loop item, and knots. A knot is `time: value`, optionally `pre-value & value`, then optional tangent and shaping parameters after a `;`. USDA 1.1 adds tangent algorithms; USDA 1.3 adds `loop` extrapolation with a boundary time.

#### 6.2.5 Array edits

```text
array-edit = "edit" inline-padding* "[" meta-sequence(array-edit-op) "]"
```

An array edit is a sequence of edit operations on the array value of a weaker opinion: `prepend`, `append`, `write … to [i]`, `insert … at [i]`, `erase [i]`, `minsize n [fill v]`, `resize n [fill v]`, and `maxsize n`. An operand is a literal element or a reference `[i]` to an element of the weaker array. Array edits are valid from USDA 1.2.

### 6.3 Relationships

```text
relationship-type = [ "custom" sep ] [ "varying" sep ] "rel"
relationship-spec = relationship-type sep property-name
                    ( inline-padding* "." inline-padding* "default" "=" path-ref
                    | inline-padding* "[" inline-padding* path-ref inline-padding* "]"
                    | [ "=" relationship-value ] inline-padding* [ relationship-meta ] )
relationship-value = "None" | path-ref | "[" [ list(path-ref) ] "]"
```

A relationship holds an ordered list of target paths. The targets are prim or property paths. A list op prefix edits the target list (§4.2).

## 7. Values

### 7.1 Value syntax

```text
typed-value  = atomic | tuple | "[" inline-padding* "]" | list-value | path-ref
atomic       = number | type-identifier | string | asset-path
tuple        = "(" multiline-padding* list(tuple-item) multiline-padding* ")"
tuple-item   = number | type-identifier | string | asset-path | tuple
list-value   = "[" multiline-padding* list(list-item) multiline-padding* "]"
list-item    = number | type-identifier | string | asset-path | list-value | tuple
dictionary   = "{" meta-sequence(dict-item) "}"
dict-item    = "dictionary" sep dict-key "=" dictionary
             | type-identifier [ inline-padding* "[" inline-padding* "]" ] sep dict-key
               "=" typed-value
dict-key     = string | identifier
```

A bare identifier as a value is valid only for the `bool` type (§7.2).

### 7.2 Value types

The declared type of an attribute or a dictionary entry gives its value type. The scalar types are:

| Type name | Value |
| --- | --- |
| `bool` | a number, `true` when nonzero; or `true`, `yes`, `false`, `no` in any letter case |
| `uchar` | unsigned 8-bit integer |
| `int`, `uint` | signed and unsigned 32-bit integer |
| `int64`, `uint64` | signed and unsigned 64-bit integer |
| `half`, `float`, `double` | IEEE 754 binary16, binary32, and binary64 |
| `timecode` | binary64 time code |
| `string` | UTF-8 string |
| `token` | an interned UTF-8 string, written as a string |
| `asset` | an asset path (§2.8), or a string that holds the path |
| `pathExpression` | a path expression, written as a string |
| `opaque` | an attribute with no value; only a declaration is valid |
| `group` | an `opaque` attribute that names a property group |

The compound types are tuples of a scalar type:

| Type name | Components | Component type |
| --- | --- | --- |
| `int2` `int3` `int4` | 2, 3, 4 | `int` |
| `half2` `half3` `half4`, `float2` …, `double2` … | 2, 3, 4 | the named scalar |
| `point3h` `point3f` `point3d` | 3 | a position |
| `normal3h` `normal3f` `normal3d` | 3 | a normal |
| `vector3h` `vector3f` `vector3d` | 3 | a direction |
| `color3h` `color3f` `color3d`, `color4h` … | 3, 4 | a color |
| `texCoord2h` …, `texCoord3h` … | 2, 3 | a texture coordinate |
| `quath` `quatf` `quatd` | 4 | a quaternion `(real, i, j, k)` |
| `matrix2d` `matrix3d` `matrix4d` | 2, 3, 4 rows | rows of `double` |
| `frame4d` | 4 rows | rows of `double` |

A role type (`point3f`, `normal3f`, `color3f`, and the others) has the storage of its plain type (`float3`). The role names the meaning.

A matrix is written as a tuple of row tuples. OpenUSD matrices act on row vectors: a point `p` maps to `p · M`, and the translation is in the last row.

Each type `T` has an array type `T[]`, written as a list. Arrays do not nest. The empty list `[]` is an empty array of any array type. A list value for a type without `[]`, a non-list value for a type with `[]`, and a tuple with the wrong number of components are malformed.

A type name that this section does not list is an extension type. Its value is the value text as written.

## 8. Composition

### 8.1 Stage population

A root layer stack populates a stage. The stage has a pseudo-root with the root prims as children. The stage traverses `def` prims and their descendants. A prim is in the traversal when every ancestor and the prim are `def` or are an `over` whose prim another arc defines, and none has `active = false`. A `class` prim and its descendants are not in the traversal.

A prim's opinions are, from strong to weak: its own specification, the opinions of its selected variants (§5.4), the opinions of its references and payloads in list order (§8.2), and the opinions of its inherited and specialized classes. A stronger opinion of a field hides a weaker one.

### 8.2 Arcs

| Arc | Target | Effect |
| --- | --- | --- |
| reference | a prim in another layer, or `</path>` in this layer | adds the target prim's composed opinions, and its descendants, under the referencing prim |
| payload | as a reference | as a reference; a client can defer loading |
| inherits | `</path>` in this layer stack | adds the class prim's opinions, weaker than local opinions |
| specializes | `</path>` in this layer stack | adds the class prim's opinions, weaker than every other arc |
| sublayer | a layer | adds all opinions of the sublayer, weaker than this layer |

The arc target's children become children of the referencing prim. The referencing prim's own specification is stronger than the target's opinions. A path that an opinion of a reference or payload target states maps from the target prim to the referencing prim; a path outside the target prim names nothing. A path that an opinion of an inherited or specialized class states outside the class prim names the same path in the stage.

### 8.3 Instancing

A prim with `instanceable = true` and at least one reference, payload, inherit, or specialize arc is an instance. All instances whose composed arcs are equal share one prototype. Descendants of an instance are the prototype's descendants. Opinions authored under an instance prim, below the instance root, have no effect.

### 8.4 Layer stacks

A relative asset path resolves against the directory of the layer that states it. In a layer of a USDZ package (§1), a relative asset path that names a member of the package names that member; another asset path resolves against the directory of the package file. An absolute asset path names its file directly. Two asset paths that name one file name one layer.

The layer stack of a layer is the layer, then the layer stacks of its sublayers in `subLayers` order. A layer occurs once in a stack; a sublayer that the stack already holds adds no opinions. The root layer stack is the layer stack of the decoded layer.

A prim path names a prim in the namespace of a layer stack. The opinions of a prim at path `P` in a stack are the specifications at `P` in each layer of the stack, strong to weak. The root prims of the stage are the root prims of every layer in the root layer stack, in the order of the strongest `reorder rootPrims` statement.

An inherit, a specialize, and a reference or payload with no asset path target a prim in the layer stack of their own arc's layer stack. A reference or payload with an asset path targets a prim in the layer stack of the layer that the asset path names: the prim that its prim path names, or the prim that the layer's `defaultPrim` names. The target prim path maps to the referencing prim, with the descendants of each.

## 9. USDC binary layer

A USDC layer (Crate) holds the same specs and fields as a text layer, in binary sections. All integers are little-endian. Floating-point values are IEEE 754 binary16, binary32, or binary64. An offset is a byte offset from the start of the layer.

### 9.1 Bootstrap header

A USDC layer starts with an 88-byte bootstrap header.

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 8 | identifier, ASCII `PXR-USDC` |
| 8 | 1 | major version |
| 9 | 1 | minor version |
| 10 | 1 | patch version |
| 11 | 5 | zero |
| 16 | 8 | `int64` byte offset of the table of contents |
| 24 | 64 | reserved |

The defined versions are:

| Version | Adds |
| --- | --- |
| 0.0.1 | the initial layout |
| 0.1.0 | a corrected path-item header layout |
| 0.2.0 | the prepend and append fields of list operations |
| 0.4.0 | compressed structural sections |
| 0.5.0 | compressed integer arrays; arrays store no rank |
| 0.6.0 | compressed floating-point arrays |
| 0.7.0 | 64-bit array sizes |
| 0.8.0 | payload list operations and payload layer offsets |
| 0.9.0 | the `timecode` value types |
| 0.10.0 | the `pathExpression` value type |
| 0.11.0 | layer-metadata relocates |
| 0.12.0 | splines |
| 0.13.0 | spline tangent algorithms |
| 0.14.0 | array edits |
| 0.15.0 | spline loops with a boundary time and time-code splines |

Version 0.3.0 is not a defined layout. A rule below that names a version applies to that version and to all later versions.

### 9.2 Table of contents

The table of contents is a `uint64` section count, then one 32-byte record for each section.

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 16 | section name, ASCII, zero-padded |
| 16 | 8 | `int64` byte offset of the section |
| 24 | 8 | `int64` byte size of the section |

The structural sections are `TOKENS`, `STRINGS`, `FIELDS`, `FIELDSETS`, `PATHS`, and `SPECS`. A section that the table does not name is empty. A reader ignores a section of another name.

A **vector** is a `uint64` element count, then the elements. A token index, a string index, a path index, a field index, and a field-set index are `uint32` values.

### 9.3 Compressed buffers

A compressed buffer is LZ4 block data in a chunked frame. The first byte is the chunk count.

- When the chunk count is 0, the rest of the buffer is one LZ4 block.
- Otherwise, each chunk is an `int32` byte size, then an LZ4 block of that size. One chunk expands to at most `0x7E000000` bytes.

The expansion of a buffer is the concatenation of the expansions of its chunks. An LZ4 block is a sequence of tokens. A token byte holds the literal length in its high four bits and the match length minus 4 in its low four bits. A nibble of 15 continues in the next bytes: each byte adds its value, and a byte below 255 ends the length. The literal bytes follow the literal length. Then a `uint16` match offset and the match length extension follow, except after the last literals of the block. A match copies bytes from the given distance back in the output of the same chunk; the copy can overlap its own output.

### 9.4 Compressed integers

A compressed integer sequence of `n` integers is a `uint64` byte size, then a compressed buffer of that size. The buffer expands to the coded integers. 32-bit sequences and 64-bit sequences have the same coding with different widths.

The coded integers are, in sequence:

1. The common difference: an `int32` (32-bit sequence) or an `int64` (64-bit sequence).
2. `(2n + 7) / 8` code bytes. Integer `i` has the 2-bit code at bits `2(i mod 4)` of byte `i / 4`.
3. The differences that are not the common difference, in sequence order.

| Code | Difference |
| --- | --- |
| 0 | the common difference |
| 1 | `int8` (32-bit sequence) or `int16` (64-bit sequence) |
| 2 | `int16` (32-bit sequence) or `int32` (64-bit sequence) |
| 3 | `int32` (32-bit sequence) or `int64` (64-bit sequence) |

Integer `i` is integer `i - 1` plus its difference, with integer −1 equal to 0. The addition is modulo 2^32 or 2^64. For example, the 32-bit sequence 123, 124, 125, 100125, 100125, 100126, 100126 has the differences 123, 1, 1, 100000, 0, 1, 0, the common difference 1, and the codes 1, 0, 0, 3, 1, 0, 1.

### 9.5 Structural sections

`TOKENS` holds the token table. It starts with a `uint64` token count. Before 0.4.0, a `uint64` byte size and that many bytes follow. From 0.4.0, a `uint64` expanded size, a `uint64` compressed size, and a compressed buffer of that size follow. The bytes are the tokens in index order, each ended by a zero byte. Tokens are UTF-8.

`STRINGS` is a vector of token indices. String `i` is the token at entry `i`.

`FIELDS` holds the fields. A field is a token index (the field name) and a 64-bit value representation (§9.6).

- Before 0.4.0, `FIELDS` is a vector of 16-byte records: 4 bytes of padding, the token index, and the value representation.
- From 0.4.0, `FIELDS` is a `uint64` field count `n`, a compressed 32-bit sequence of `n` token indices, a `uint64` byte size, and a compressed buffer of that size. The buffer expands to the `n` value representations, 8 bytes each.

`FIELDSETS` holds runs of field indices. Each run ends with the value `0xFFFFFFFF`. A field set index is the position of the first field index of a run. Before 0.4.0, `FIELDSETS` is a vector of field indices. From 0.4.0, it is a `uint64` count `n` and a compressed 32-bit sequence of `n` entries.

`PATHS` holds the path table. It starts with a `uint64` path count. The path tree follows in depth-first order. The first item is the absolute root path `/`. Each other item is a child of a parent path: its element token is a prim name, a variant selection `{set=variant}`, a variant set `{set=}`, or, for a property path, a property name.

Before 0.4.0, each item is a header record. Version 0.0.1 starts each header with 4 bytes of padding.

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 4 | path index of the item |
| 4 | 4 | element token index |
| 8 | 1 | bits |
| 9 | 3 | padding |

Bit 0 of the bits is set when the next item is the first child of this item. Bit 1 is set when this item has a next sibling. Bit 2 is set when the element is a property name. When both bit 0 and bit 1 are set, an `int64` offset of the sibling item follows the header. When only bit 1 is set, the sibling item follows the header. When neither bit is set, the run of siblings ends.

From 0.4.0, the path count is followed by a `uint64` item count `m`, then three compressed 32-bit sequences of `m` integers: the path indices, the element token indices, and the jumps. A negative element token index `-t` names token `t` as a property name. For item `i` with jump `j`:

| Jump | Meaning |
| --- | --- |
| `j = 0` | item `i + 1` is the next sibling; the item has no child |
| `j = −1` | item `i + 1` is the first child; the item has no sibling |
| `j > 0` | item `i + 1` is the first child; item `i + j` is the next sibling |
| `j = −2` | the item has no child and no sibling |

`SPECS` holds the specs. A spec is a path index, a field-set index, and a spec type.

- Version 0.0.1: a vector of 16-byte records, 4 bytes of padding then the three `uint32` values.
- Versions 0.1.0 to 0.2.0: a vector of 12-byte records of the three `uint32` values.
- From 0.4.0: a `uint64` spec count `n`, then compressed 32-bit sequences of `n` path indices, `n` field-set indices, and `n` spec types.

| Spec type | Spec |
| --- | --- |
| 1 | attribute |
| 2 | connection |
| 3 | expression |
| 4 | mapper |
| 5 | mapper argument |
| 6 | prim |
| 7 | pseudo-root (the layer) |
| 8 | relationship |
| 9 | relationship target |
| 10 | variant |
| 11 | variant set |

### 9.6 Value representation

A value representation is a `uint64`.

| Bits | Field |
| --- | --- |
| 0 to 47 | payload |
| 48 to 55 | value type |
| 60 | array edit |
| 61 | compressed |
| 62 | inlined |
| 63 | array |

| Type | Value | Type | Value |
| --- | --- | --- | --- |
| 1 | `bool` | 31 | dictionary |
| 2 | `uchar` | 32 | token list operation |
| 3 | `int` | 33 | string list operation |
| 4 | `uint` | 34 | path list operation |
| 5 | `int64` | 35 | reference list operation |
| 6 | `uint64` | 36 | `int` list operation |
| 7 | `half` | 37 | `int64` list operation |
| 8 | `float` | 38 | `uint` list operation |
| 9 | `double` | 39 | `uint64` list operation |
| 10 | `string` | 40 | path vector |
| 11 | `token` | 41 | token vector |
| 12 | `asset` | 42 | specifier |
| 13 | `matrix2d` | 43 | permission |
| 14 | `matrix3d` | 44 | variability |
| 15 | `matrix4d` | 45 | variant selection map |
| 16 | `quatd` | 46 | time samples |
| 17 | `quatf` | 47 | payload |
| 18 | `quath` | 48 | `double` vector |
| 19 | `double2` | 49 | layer offset vector |
| 20 | `float2` | 50 | string vector |
| 21 | `half2` | 51 | value block |
| 22 | `int2` | 52 | boxed value |
| 23 | `double3` | 53 | unregistered value |
| 24 | `float3` | 54 | unregistered value list operation |
| 25 | `half3` | 55 | payload list operation |
| 26 | `int3` | 56 | `timecode` |
| 27 | `double4` | 57 | `pathExpression` |
| 28 | `float4` | 58 | relocates |
| 29 | `half4` | 59 | spline |
| 30 | `int4` | 60 | animation block |

A scalar of type 1 to 4, 7, 8, 10 to 12, or 42 to 44 is always in the low 32 bits of the payload: the value bits, a string index (`string`), or a token index (`token`, `asset`). A specifier is 0 `def`, 1 `over`, or 2 `class`. A permission is 0 `public` or 1 `private`. A variability is 0 `varying` or 1 `uniform`; the value 2 is read as `uniform`.

Any other value is inlined when the inlined bit is set. The low 32 bits of the payload then hold:

- for `int64` and `uint64`, an `int32` or `uint32` value;
- for `double`, a binary32 value;
- for a vector type, one `int8` value for each component;
- for a matrix type, one `int8` value for each diagonal entry; the other entries are 0;
- for a dictionary, nothing: the dictionary is empty.

When the inlined bit is clear, the payload is the offset of the value bytes. A value block (51) and an animation block (60) have no payload.

### 9.7 Value encodings

A scalar of a value type is its components in sequence. A vector has its components in index order. A matrix has its rows in sequence. A quaternion is the three imaginary components, then the real component; the text form states the real component first. In an array, a `string` and an `asset` element is a string index, and a `token` element is a token index.

An array (array bit set) has the payload offset of its data. Payload 0 is the empty array. Before 0.5.0, the data starts with a `uint32` rank. The element count follows: a `uint32` before 0.7.0, a `uint64` from 0.7.0. The elements follow.

From 0.5.0, an array of `int`, `uint`, `int64`, or `uint64` with the compressed bit set has fewer than 16 elements in sequence, or a compressed sequence of the element count. An `int64` and a `uint64` array use 64-bit sequences.

From 0.6.0, an array of `half`, `float`, or `double` with the compressed bit set has fewer than 16 elements in sequence. Otherwise a code byte follows the count:

- `i`: a compressed 32-bit sequence of the element values as integers.
- `t`: a `uint32` table size, the table elements, and a compressed 32-bit sequence of one table index for each element.

A dictionary is a `uint64` entry count, then each entry: a string index (the key) and a boxed value. A **boxed value** is an `int64` offset relative to the position of that offset, to a value representation. The reader continues after the value representation. A boxed value (52) and an unregistered value (53) have the payload offset of a boxed value.

A list operation is a header byte, then the vectors that the header names, in this order: explicit items, added items, prepended items, appended items, deleted items, ordered items.

| Bit | Meaning |
| --- | --- |
| 0 | the list operation is explicit |
| 1 | explicit items follow |
| 2 | added items follow |
| 3 | deleted items follow |
| 4 | ordered items follow |
| 5 | prepended items follow |
| 6 | appended items follow |

A token, string, or path item is an index. A reference item is a string index (the asset path), a path index (the prim path), a layer offset, and a dictionary (the custom data). A payload item is a string index and a path index; from 0.8.0, a layer offset follows. A layer offset is a `double` offset and a `double` scale. An empty asset path or the empty path means that the item names none.

A path vector, a token vector, a string vector, a `double` vector, and a layer offset vector are vectors of their elements. A variant selection map is a `uint64` count, then pairs of string indices: the variant set and the selected variant. Relocates are a vector of path index pairs: the source and the target.

Time samples are a boxed value of the sample times (a `double` vector or a `double` array), then an `int64` offset relative to the position of that offset, to a `uint64` sample count and one value representation for each sample.

A spline is binary spline data and custom data at its payload offset. An array edit is a representation of the values, a representation of the indices, and a byte, at its payload offset.

### 9.8 Specs and statements

A spec states the same statement as the text statement of §3 to §6 at its path. The fields of the spec are the metadata items and fields of that statement. A field holds the value of the text item of the same name, with these exceptions:

| Spec | Field | Text form |
| --- | --- | --- |
| pseudo-root | `primChildren` | the order of the root prims |
| pseudo-root | `primOrder` | `reorder rootPrims` |
| pseudo-root | `subLayers`, `subLayerOffsets` | `subLayers` items and their offsets |
| pseudo-root | `layerRelocates` | `relocates` |
| prim, variant | `specifier`, `typeName` | the specifier and the type name |
| prim, variant | `primChildren`, `properties`, `variantSetChildren` | the order of the children, properties, and variant sets |
| prim, variant | `primOrder`, `propertyOrder` | `reorder nameChildren`, `reorder properties` |
| prim | `variantSetNames` | `variantSets` |
| prim | `variantSelection` | `variants` |
| variant set | `variantChildren` | the order of the variants |
| attribute | `typeName`, `custom`, `variability` | the declaration |
| attribute | `default` | the assignment of the default value |
| attribute | `timeSamples` | `.timeSamples` |
| attribute | `connectionPaths` | `.connect` |
| relationship | `targetPaths` | the target list |
| any | `documentation` | `doc` |
| any | `comment` | the bare string |

A list operation states one text item for each part, with the list-operation keyword of that part. An explicit list operation states the item with no keyword. A value block is `None`.

## 10. Stage metrics

### 10.1 Units and up axis

Every length in a layer is in scene units. One scene unit is `metersPerUnit` metres; when `metersPerUnit` is absent, it is `0.01`. Angles in schemas are degrees unless a schema states otherwise.

`upAxis` names the stage's up axis, `Y` or `Z`. When `upAxis` is absent, it is `Y`. The up axis is a statement about the scene, not a transform: coordinates are not rotated by it.

## 11. Common geometry schemas

### 11.1 Primvars

A primvar is an attribute in the `primvars:` namespace. It holds a value that varies over the elements of a geometric primitive.

| `interpolation` | One value per |
| --- | --- |
| `constant` | primitive |
| `uniform` | face of a mesh, or curve of a curve batch |
| `varying` | point, interpolated linearly; for a curve batch, see §13.1 |
| `vertex` | point, interpolated with the surface basis |
| `faceVarying` | face-vertex of a mesh |

A primvar with no `interpolation` metadata is `constant`. `elementSize` (default 1) is the number of array entries per element.

An **indexed primvar** has a companion attribute `primvars:<name>:indices` of type `int[]`. The primvar array then holds distinct values, and each index selects a value for one element.

`primvars:displayColor` (`color3f[]`) and `primvars:displayOpacity` (`float[]`) state the display color and opacity of a primitive.

A **texture-coordinate primvar** holds `(u, v)` texture coordinates: a primvar of type `texCoord2h[]`, `texCoord2f[]`, or `texCoord2d[]`, or a primvar of type `half2[]`, `float2[]`, or `double2[]` whose name is `st` or `st` followed by decimal digits. The primvar `st` is the default set of a mesh. A `(u, v)` of `(0, 0)` is the lower left corner of an image, and `(1, 1)` its upper right corner.

### 11.2 Imageable

| Attribute | Type | Unauthored value | Meaning |
| --- | --- | --- | --- |
| `visibility` | token | `inherited` | `invisible` hides the prim and its descendants |
| `purpose` | token | `default` | `default`, `render`, `proxy`, or `guide` |

### 11.3 Point-based

A point-based primitive (Mesh, BasisCurves, NurbsCurves, NurbsPatch) has:

| Attribute | Type | Meaning |
| --- | --- | --- |
| `points` | `point3f[]` | point positions in local coordinates |
| `normals` | `normal3f[]` | normals; `interpolation` metadata as a primvar |
| `extent` | `float3[2]` | local bounding box |

`primvars:normals` overrides `normals` when both are authored.

### 11.4 Xformable

A xformable prim states its local transform, relative to its parent, as an ordered stack of transform ops. The token array `xformOpOrder` names the ops. Each op is an attribute `xformOp:<type>` or `xformOp:<type>:<suffix>`. An entry `!invert!<op name>` applies the inverse of the op. The entry `!resetXformStack!`, which is first when present, states that the prim does not inherit the transform of its parent.

| Op type | Value | Map of a point `p` |
| --- | --- | --- |
| `translate` | 3 components `t` | `p + t` |
| `translateX`, `translateY`, `translateZ` | 1 component | translation along the axis |
| `scale` | 3 components `s` | `(s.x·p.x, s.y·p.y, s.z·p.z)` |
| `scaleX`, `scaleY`, `scaleZ` | 1 component | scaling along the axis |
| `rotateX`, `rotateY`, `rotateZ` | 1 angle in degrees | right-handed rotation about the axis |
| `rotateXYZ`, `rotateXZY`, `rotateYXZ`, `rotateYZX`, `rotateZXY`, `rotateZYX` | 3 angles in degrees, `(x, y, z)` | rotation about the first named axis, then the second, then the third |
| `orient` | quaternion `(real, i, j, k)` | rotation by the normalized quaternion |
| `transform` | `matrix4d` `M` | `(p · M) / w`, when the last column of `M` is `(0, 0, 0, w)` and `w ≠ 0` |

The first op in `xformOpOrder` is the outermost: the local transform maps a point by the last op first. An op whose attribute has no value is the identity. A prim with no `xformOpOrder` has the identity transform.

### 11.5 Intrinsic primitives

The intrinsic primitives, with the value of each size attribute when it is not authored, are `Cube` (`size` 2), `Sphere` (`radius` 1), `Cylinder` (`radius` 1, `height` 2, `axis` `Z`), `Cone` (`radius` 1, `height` 2, `axis` `Z`), `Capsule` (`radius` 0.5, `height` 1, `axis` `Z`), `Plane` (`width` 2, `length` 2, `axis` `Z`), `Cylinder_1` and `Capsule_1` (`radiusTop`, `radiusBottom`). Each is centered at the local origin.

| Type | Solid |
| --- | --- |
| `Cube` | the axis-aligned cube of edge `size` |
| `Sphere` | the ball of `radius` |
| `Cylinder` | the cylinder of `radius` along `axis`, from `-height / 2` to `height / 2` |
| `Cone` | the cone along `axis` with its base of `radius` at `-height / 2` and its apex at `height / 2` |
| `Cylinder_1` | the frustum along `axis` with radius `radiusBottom` at `-height / 2` and `radiusTop` at `height / 2`; a zero radius is an apex |
| `Capsule` | the cylinder of `radius` from `-height / 2` to `height / 2` along `axis`, closed by a hemisphere of `radius` at each end |
| `Capsule_1` | as `Capsule`, with `radiusBottom` and `radiusTop` at the two ends |
| `Plane` | the rectangle normal to `axis`: `width` along `x`, or `z` when `axis` is `X`, and `length` along `y`, or `z` when `axis` is `Y` |

`axis` is `X`, `Y`, or `Z`. A `Plane` is a surface with no volume.

## 12. Mesh

A `Mesh` prim is a polygonal surface.

| Attribute | Type | Unauthored value | Meaning |
| --- | --- | --- | --- |
| `faceVertexCounts` | `int[]` | | the number of vertices of each face |
| `faceVertexIndices` | `int[]` | | point indices of the face-vertices, face by face |
| `holeIndices` | `int[]` | | faces that are holes |
| `orientation` | token | `rightHanded` | `rightHanded` or `leftHanded` |
| `subdivisionScheme` | token | `catmullClark` | `catmullClark`, `loop`, `bilinear`, or `none` |
| `doubleSided` | bool | `false` | both sides are visible |
| creases and corners | | | `cornerIndices`, `cornerSharpnesses`, `creaseIndices`, `creaseLengths`, `creaseSharpnesses` |

The length of `faceVertexIndices` is the sum of `faceVertexCounts`. Each index is less than the length of `points`. A face has at least three vertices. A face whose count is below three is degenerate and is not part of the surface. A face listed in `holeIndices` is not part of the surface.

For `rightHanded`, the front side of a face is the side from which its vertices go counterclockwise. For `leftHanded`, the front side is the opposite side.

`subdivisionScheme = none` states that the faces are the surface. Another scheme states that the faces are a control cage and the surface is the limit surface of that scheme.

A face-varying primvar has one value per entry of `faceVertexIndices`. A uniform primvar has one value per face.

### 12.1 Geometry subsets

A `GeomSubset` child prim of a Mesh names a subset of the mesh's elements.

| Attribute | Type | Meaning |
| --- | --- | --- |
| `elementType` | uniform token | `face`, `point`, `edge`, `segment`, or `tetrahedron`; `face` when not authored |
| `indices` | `int[]` | the element indices |
| `familyName` | uniform token | the family that the subset belongs to |

The `subsetFamily:<familyName>:familyType` attribute of the mesh is `partition` (each element is in exactly one subset of the family), `nonOverlapping` (each element is in at most one), or `unrestricted`. Material binding (§15) uses the family `materialBind`.

## 13. Curves

### 13.1 BasisCurves

A `BasisCurves` prim is a batch of curves.

| Attribute | Type | Unauthored value | Meaning |
| --- | --- | --- | --- |
| `curveVertexCounts` | `int[]` | | the number of points of each curve |
| `type` | uniform token | `cubic` | `linear` or `cubic` |
| `basis` | uniform token | `bezier` | `bezier`, `bspline`, or `catmullRom`; cubic only |
| `wrap` | uniform token | `nonperiodic` | `nonperiodic`, `periodic`, or `pinned` |
| `widths` | `float[]` | | curve widths |

The points of curve `i` follow those of curve `i - 1` in `points`. Curve `i` has `n = curveVertexCounts[i]` points `P[0] … P[n-1]`.

A **linear** curve has segments `P[j]–P[j+1]`. A nonperiodic linear curve has `n - 1` segments. A periodic linear curve has `n` segments; the last joins `P[n-1]` to `P[0]`.

A **cubic** segment is defined by four points. The basis has a step: 3 for `bezier`, 1 for `bspline` and `catmullRom`. Segment `k` uses points `k·step … k·step + 3`.

| Wrap | Segment count | Condition on `n` |
| --- | --- | --- |
| `nonperiodic` | `(n - 4) / step + 1` | `(n - 4) mod step = 0` and `n ≥ 4` |
| `periodic` | `n / step` | `n mod step = 0`; indices wrap modulo `n` |
| `pinned` | `n - 1` | `n ≥ 2`; `bspline` and `catmullRom` only |

A pinned curve adds the phantom points `P[-1] = 2·P[0] − P[1]` and `P[n] = 2·P[n-1] − P[n-2]`, then evaluates as a nonperiodic curve of `n + 2` points. For `bezier`, `pinned` is `nonperiodic`.

On a segment with points `G = (G0, G1, G2, G3)` and parameter `t ∈ [0, 1]`, the point is `[t³ t² t 1] · B · G`, where `B` is:

| Basis | `B` |
| --- | --- |
| `bezier` | `[[-1, 3, -3, 1], [3, -6, 3, 0], [-3, 3, 0, 0], [1, 0, 0, 0]]` |
| `bspline` | `1/6 · [[-1, 3, -3, 1], [3, -6, 3, 0], [-3, 0, 3, 0], [1, 4, 1, 0]]` |
| `catmullRom` | `1/2 · [[-1, 3, -3, 1], [2, -5, 4, -1], [-1, 0, 1, 0], [0, 2, 0, 0]]` |

A uniform primvar has one value per curve. A vertex primvar has one value per point. A varying primvar has one value per segment end: segment count plus one for a nonperiodic or pinned curve, and the segment count for a periodic curve.

### 13.2 NurbsCurves

A `NurbsCurves` prim is a batch of non-uniform rational B-spline curves.

| Attribute | Type | Meaning |
| --- | --- | --- |
| `curveVertexCounts` | `int[]` | the number of control points of each curve |
| `order` | `int[]` | the order (degree plus one) of each curve |
| `knots` | `double[]` | the knot vectors of all curves, concatenated |
| `ranges` | `double2[]` | the parameter range of each curve |
| `pointWeights` | `double[]` | the weight of each control point |

Curve `i` with `n` control points and order `k` owns `n + k` consecutive entries of `knots`. Knot values do not decrease. `n ≥ k ≥ 1`.

When `ranges` is authored, curve `i` is defined on `ranges[i]`, with `knots[k-1] ≤ min < max ≤ knots[n]`. Otherwise curve `i` is defined on `[knots[k-1], knots[n]]`.

When `pointWeights` is authored, it has one entry per point and the curves are rational: the curve is `Σ wⱼ Nⱼ(t) Pⱼ / Σ wⱼ Nⱼ(t)`. Points are not premultiplied by their weights. Otherwise every weight is 1.

A closed or periodic NURBS curve is written as an open curve whose first and last points coincide, with the knot vector that gives the required continuity.

## 14. NurbsPatch

A `NurbsPatch` prim is a tensor-product NURBS surface with optional trim curves.

| Attribute | Type | Unauthored value | Meaning |
| --- | --- | --- | --- |
| `uVertexCount`, `vVertexCount` | int | | control-point counts |
| `uOrder`, `vOrder` | int | | orders (degree plus one) |
| `uKnots`, `vKnots` | `double[]` | | knot vectors, `uVertexCount + uOrder` and `vVertexCount + vOrder` entries |
| `uRange`, `vRange` | `double2` | | the parameter ranges of the surface |
| `uForm`, `vForm` | uniform token | `open` | `open`, `closed`, or `periodic` |
| `pointWeights` | `double[]` | | control-point weights |
| `points` | `point3f[]` | | control points |

`points` has `uVertexCount · vVertexCount` entries. The control point at index `(i, j)`, with `i` in U and `j` in V, is `points[j · uVertexCount + i]`: the U index varies fastest.

When `uRange` is authored, the surface is defined for `u` in `uRange`, with `uKnots[uOrder-1] ≤ min < max ≤ uKnots[uVertexCount]`. Otherwise it is defined on `[uKnots[uOrder-1], uKnots[uVertexCount]]`. `vRange` is the same in V.

`pointWeights` has the meaning of §13.2, per control point of the grid.

`uForm` and `vForm` state modeling intent. `closed` states that the first and last control-point rows in that direction coincide. `periodic` states that the first `order - 1` rows coincide with the last `order - 1` rows and that the knot spacing repeats. The form does not change evaluation.

The surface normal is the cross product `∂S/∂u × ∂S/∂v` for `orientation = rightHanded`, and its negation for `leftHanded`.

### 14.1 Trim curves

| Attribute | Type | Meaning |
| --- | --- | --- |
| `trimCurve:counts` | `int[]` | the number of curves in each trim loop |
| `trimCurve:orders` | `int[]` | the order of each trim curve |
| `trimCurve:vertexCounts` | `int[]` | the number of control points of each trim curve |
| `trimCurve:knots` | `double[]` | the knot vectors of all trim curves, concatenated |
| `trimCurve:ranges` | `double2[]` | the parameter range of each trim curve |
| `trimCurve:points` | `double3[]` | homogeneous control points `(u·w, v·w, w)` |

The trim curves are in the `(u, v)` parameter space of the patch. The curves of one loop join end to start and form a closed loop. The loops of `trimCurve:counts` follow each other; their curves follow each other in the flat arrays.

A trim-curve point `(x, y, w)` is homogeneous: its parameter-space position is `(x / w, y / w)` and its weight is `w`.

## 15. Shading

### 15.1 Material binding

A prim binds a material with the relationship `material:binding`, which targets a `Material` prim. A binding on a prim applies to the prim and its descendants, unless a descendant has its own binding. A binding of a GeomSubset of the `materialBind` family applies to the subset's faces. A binding with the purpose suffix (`material:binding:full`, `material:binding:preview`) applies to that purpose only.

The binding relationship's `bindMaterialAs` metadata is `weakerThanDescendants` (the default) or `strongerThanDescendants`.

### 15.2 Materials and shaders

A `Material` prim has outputs `outputs:surface`, `outputs:displacement`, and `outputs:volume`, with optional render-context namespaces (`outputs:mdl:surface`). An output connects (§6.2.2) to an output of a `Shader` prim.

A `Shader` prim names its shader type with `uniform token info:id`, or with `info:implementationSource` and a source asset. Its parameters are `inputs:` attributes. An input has a value or a connection to another shader's output.

### 15.3 UsdPreviewSurface

The shader `info:id = "UsdPreviewSurface"` is the interchange surface. Its inputs are:

| Input | Type | Unauthored value |
| --- | --- | --- |
| `inputs:diffuseColor` | `color3f` | `(0.18, 0.18, 0.18)` |
| `inputs:emissiveColor` | `color3f` | `(0, 0, 0)` |
| `inputs:useSpecularWorkflow` | int | 0 |
| `inputs:specularColor` | `color3f` | `(0, 0, 0)` |
| `inputs:metallic` | float | 0 |
| `inputs:roughness` | float | 0.5 |
| `inputs:clearcoat` | float | 0 |
| `inputs:clearcoatRoughness` | float | 0.01 |
| `inputs:opacity` | float | 1 |
| `inputs:opacityThreshold` | float | 0 |
| `inputs:ior` | float | 1.5 |
| `inputs:normal` | `normal3f` | `(0, 0, 1)` |
| `inputs:displacement` | float | 0 |
| `inputs:occlusion` | float | 1 |

An input can connect to the output of a texture shader (`UsdUVTexture`) instead of holding a value.

### 15.4 MaterialX

A material output with the render context `mtlx` (`outputs:mtlx:surface`) connects to a MaterialX shader. Its `info:id` is the name of a MaterialX node definition, `ND_<node>_<output type>`, and its inputs have the node definition's input names. The MaterialX types map to USD types: `float` to float, `color3` to `color3f`, `vector3` to `float3`, `boolean` to bool, `integer` to int, and `filename` to asset. The shader output is `outputs:out`.

| Input of `ND_open_pbr_surface_surfaceshader` | Type | Unauthored value |
| --- | --- | --- |
| `base_weight` | float | `1` |
| `base_color` | `color3f` | `(0.8, 0.8, 0.8)` |
| `base_metalness` | float | `0` |
| `specular_weight` | float | `1` |
| `specular_color` | `color3f` | `(1, 1, 1)` |
| `specular_roughness` | float | `0.3` |
| `specular_ior` | float | `1.5` |
| `coat_weight` | float | `0` |
| `coat_roughness` | float | `0` |
| `emission_luminance` | float | `0` |
| `emission_color` | `color3f` | `(1, 1, 1)` |
| `geometry_opacity` | float | `1` |

The diffuse color is `base_weight · base_color`.

| Input of `ND_standard_surface_surfaceshader` | Type | Unauthored value |
| --- | --- | --- |
| `base` | float | `1` |
| `base_color` | `color3f` | `(0.8, 0.8, 0.8)` |
| `metalness` | float | `0` |
| `specular_color` | `color3f` | `(1, 1, 1)` |
| `specular_roughness` | float | `0.2` |
| `specular_IOR` | float | `1.5` |
| `coat` | float | `0` |
| `coat_roughness` | float | `0.1` |
| `opacity` | `color3f` | `(1, 1, 1)` |

The diffuse color is `base · base_color`. `ND_standard_surface_surfaceshader_100` has the unauthored values `base = 0.8` and `base_color = (1, 1, 1)`.

### 15.5 MDL

A material output with the render context `mdl` (`outputs:mdl:surface`) connects to a shader with `uniform token info:implementationSource = "sourceAsset"`, the MDL module in `uniform asset info:mdl:sourceAsset`, and the material of the module in `uniform token info:mdl:sourceAsset:subIdentifier`. The inputs have the MDL parameter names.

| Input of the `OmniPBR` material | Type | Unauthored value |
| --- | --- | --- |
| `diffuse_color_constant` | `color3f` | `(0.2, 0.2, 0.2)` |
| `diffuse_tint` | `color3f` | `(1, 1, 1)` |
| `metallic_constant` | float | `0` |
| `reflection_roughness_constant` | float | `0.5` |
| `enable_opacity` | bool | `false` |
| `opacity_constant` | float | `1` |

The diffuse color is `diffuse_tint · diffuse_color_constant`, component by component. The opacity is `opacity_constant` when `enable_opacity` is true, and `1` otherwise.

### 15.6 Textures

An image texture drives a surface shader input through a connection to a texture shader, or, in an `OmniPBR` material, through an asset input. An asset path is relative to the layer that authors it, or names a member of the package that holds that layer.

`UsdUVTexture` reads the image in `asset inputs:file` at the coordinates of `float2 inputs:st`. `token inputs:wrapS` and `token inputs:wrapT` give the wrap mode along `u` and `v`: `repeat`, `mirror`, `clamp`, `black`, or `useMetadata` (the mode the image file states, and `repeat` when it states none). The output `float3 outputs:rgb` is the color, and the outputs `float outputs:r`, `outputs:g`, `outputs:b`, and `outputs:a` are one channel each. Each output is `scale · texel + bias`, component by component, with `float4 inputs:scale` (unauthored value `(1, 1, 1, 1)`) and `float4 inputs:bias` (unauthored value `(0, 0, 0, 0)`). A tangent-space normal map has `inputs:scale = (2, 2, 2, 1)` and `inputs:bias = (-1, -1, -1, 0)`. `token inputs:sourceColorSpace` is `raw`, `sRGB`, or `auto`; a texture of a non-color input is `raw`.

`UsdPrimvarReader_float2` outputs `float2 outputs:result`, the value of the primvar that `string inputs:varname` names. `UsdTransform2d` maps `float2 inputs:in` to `float2 outputs:result = translation + R(rotation) · (scale ⊙ in)`, with `float2 inputs:scale` (unauthored value `(1, 1)`), `float inputs:rotation` in degrees counterclockwise (unauthored value `0`), and `float2 inputs:translation` (unauthored value `(0, 0)`).

A `MaterialX` image node `ND_image_<type>` reads `asset inputs:file` at `float2 inputs:texcoord`, with the address modes `string inputs:uaddressmode` and `inputs:vaddressmode` (`constant`, `clamp`, `periodic`, or `mirror`). `ND_tiledimage_<type>` reads at `texcoord · uvtiling + uvoffset`, with `float2 inputs:uvtiling` (unauthored value `(1, 1)`) and `float2 inputs:uvoffset` (unauthored value `(0, 0)`), and repeats the image. `ND_geompropvalue_vector2` outputs the primvar that `string inputs:geomprop` names, and `ND_texcoord_vector2` the texture-coordinate set `int inputs:index` (`0` is `st`). `ND_extract_<type>` outputs channel `int inputs:index` of `inputs:in`, `0` to `3` for red to alpha. `ND_normalmap_vector2` and `ND_normalmap_float` map a tangent-space normal map image `inputs:in` to a normal.

| Surface input | `UsdPreviewSurface` | `OpenPBR` | Standard Surface | `OmniPBR` |
| --- | --- | --- | --- | --- |
| base color | `diffuseColor` | `base_color` | `base_color` | `diffuse_texture` |
| emission | `emissiveColor` | `emission_color` | `emission_color` | `emissive_color_texture` |
| metalness | `metallic` | `base_metalness` | `metalness` | `metallic_texture` |
| roughness | `roughness` | `specular_roughness` | `specular_roughness` | `reflectionroughness_texture` |
| opacity | `opacity` | `geometry_opacity` | `opacity` | `opacity_texture` |
| normal | `normal` | `geometry_normal` | `normal` | `normalmap_texture` |
| occlusion | `occlusion` | | | |
| coat | `clearcoat` | `coat_weight` | `coat` | |
| coat roughness | `clearcoatRoughness` | `coat_roughness` | `coat_roughness` | |
| specular color | `specularColor` | `specular_color` | `specular_color` | |

An `OmniPBR` material reads every texture at the coordinate set `st`, transformed as `UsdTransform2d` transforms it, with `float2 inputs:texture_scale` (unauthored value `(1, 1)`), `float inputs:texture_rotate` in degrees (unauthored value `0`), and `float2 inputs:texture_translate` (unauthored value `(0, 0)`).

## 16. Physics

The `UsdPhysics` schemas state rigid bodies, collision, mass, physics materials, joints, and articulations.

### 16.1 Units

The layer metadata `kilogramsPerUnit` (double, unauthored value `1`) gives the kilograms per mass unit of the layer. Lengths are in `metersPerUnit` (§10.1). Angles are in degrees. A density is mass per cubed length, a moment of inertia mass times squared length, a force mass times length per squared second, and a torque mass times squared length per squared second, all in layer units.

### 16.2 Rigid bodies and articulations

`PhysicsRigidBodyAPI` makes a xformable prim and its descendants one rigid body.

| Attribute | Type | Unauthored value | Meaning |
| --- | --- | --- | --- |
| `physics:rigidBodyEnabled` | bool | `true` | the simulation includes the body |
| `physics:kinematicEnabled` | bool | `false` | the body moves only by its animated transform |
| `physics:velocity`, `physics:angularVelocity` | `vector3f` | `(0, 0, 0)` | initial linear velocity, and angular velocity in degrees per second, in the space that the prim's transform maps into |
| `physics:simulationOwner` | rel | no target | the `PhysicsScene` prims that simulate the body |
| `physics:startsAsleep` | uniform bool | `false` | the body starts at rest |

`PhysicsArticulationRootAPI` marks the root of an articulation: the tree of rigid bodies that the joints under the prim connect, which the simulation solves as one system. The prim is a rigid body of the tree or an ancestor of its bodies.

### 16.3 Mass and collision

`PhysicsMassAPI` states mass properties in the prim's local frame.

| Attribute | Type | Unauthored value | Meaning |
| --- | --- | --- | --- |
| `physics:mass` | float | `0` | mass; `0` is not stated |
| `physics:density` | float | `0` | density; `0` is not stated |
| `physics:centerOfMass` | `point3f` | `(-inf, -inf, -inf)` | center of mass; a value that is not finite is not stated |
| `physics:diagonalInertia` | `float3` | `(0, 0, 0)` | principal moments of inertia about the center of mass; all zero is not stated |
| `physics:principalAxes` | `quatf` | `(0, 0, 0, 0)` | rotation from the local axes to the principal axes; all zero is not stated |

`physics:diagonalInertia` and `physics:principalAxes` are both authored or both not authored. A stated mass takes precedence over a density, and a density over the density of the collider material.

`PhysicsCollisionAPI` makes a gprim a collider, with `physics:collisionEnabled` (bool, unauthored value `true`). `PhysicsMeshCollisionAPI` on a `Mesh` collider gives the contact shape in `uniform token physics:approximation`:

| Token | Shape |
| --- | --- |
| `none` (unauthored value) | the mesh |
| `convexHull` | the convex hull of the mesh |
| `convexDecomposition` | a union of convex parts |
| `boundingCube` | the bounding box |
| `boundingSphere` | the bounding sphere |
| `meshSimplification` | a mesh with fewer faces |

### 16.4 Joints

A joint prim relates two bodies through `rel physics:body0` and `rel physics:body1`. A relationship with no target is the world. Each side has a joint frame in the local frame of its body: `point3f physics:localPos0`, `quatf physics:localRot0` (`(real, i, j, k)`), and the same with `1`. The joint constrains the two frames.

| Type | Constraint |
| --- | --- |
| `PhysicsFixedJoint` | the frames coincide |
| `PhysicsRevoluteJoint` | rotation about the axis `physics:axis` of the frames only |
| `PhysicsPrismaticJoint` | translation along the axis `physics:axis` of the frames only |
| `PhysicsSphericalJoint` | the origins coincide; the angles between the axes `physics:axis` stay within `physics:coneAngle0Limit` and `physics:coneAngle1Limit` (degrees, `-1` for no limit) |
| `PhysicsDistanceJoint` | the distance between the origins is in `[physics:minDistance, physics:maxDistance]` (`-1` for no limit) |
| `PhysicsJoint` | the limits of its `PhysicsLimitAPI` instances |
| `PhysxPhysicsGearJoint` | couples the revolute joints `physics:hinge0` and `physics:hinge1`: \|ω₁\| = `physics:gearRatio` · \|ω₀\|, with ω₀ and ω₁ the angular velocities of their joints. For gears with `N0` and `N1` teeth the ratio is `N0 / N1` |
| `PhysxPhysicsRackAndPinionJoint` | couples the revolute joint `physics:hinge` to the prismatic joint `physics:prismatic`: \|ω\| = `physics:ratio` · \|v\|, with ω the angular velocity of the revolute joint in degrees per second and v the linear velocity of the prismatic joint |

The signs of the coupled velocities follow the joint frames. Each relationship of a coupling joint targets exactly one joint of its type. `physics:axis` is `X` (unauthored value), `Y`, or `Z`. `physics:lowerLimit` and `physics:upperLimit` (float, unauthored values `-inf` and `inf`) bound the motion: degrees for a revolute joint and lengths for a prismatic joint.

| Attribute | Type | Unauthored value | Meaning |
| --- | --- | --- | --- |
| `physics:jointEnabled` | bool | `true` | the joint applies |
| `physics:collisionEnabled` | bool | `false` | the two bodies collide with each other |
| `physics:breakForce`, `physics:breakTorque` | float | `inf` | the load that breaks the joint |
| `physics:excludeFromArticulation` | uniform bool | `false` | the joint is not part of an articulation |

`PhysicsLimitAPI` is multiple-apply. Its instance name is a degree of freedom: `transX`, `transY`, `transZ`, `rotX`, `rotY`, `rotZ`, or `distance`. `limit:<dof>:physics:low` and `limit:<dof>:physics:high` (float, unauthored values `-inf` and `inf`) bound the degree of freedom; a low value above the high value locks it.

### 16.5 Drives

`PhysicsDriveAPI` is multiple-apply. Its instance name is `angular` for a revolute joint, `linear` for a prismatic joint, or a degree of freedom of `PhysicsLimitAPI` for a `PhysicsJoint`.

| Attribute (`drive:<dof>:physics:`) | Type | Unauthored value | Meaning |
| --- | --- | --- | --- |
| `type` | uniform token | `force` | `force`, or `acceleration` that ignores the masses |
| `targetPosition` | float | `0` | target position, degrees or length |
| `targetVelocity` | float | `0` | target velocity per second |
| `stiffness` | float | `0` | position gain |
| `damping` | float | `0` | velocity gain |
| `maxForce` | float | `inf` | largest force or torque |

The drive output is `stiffness · (targetPosition − position) + damping · (targetVelocity − velocity)`, limited to `maxForce`. The stiffness of an angular drive is torque per degree, and its damping torque per degree per second.

### 16.6 Physics materials

`PhysicsMaterialAPI` on a `Material` prim states `physics:density`, `physics:staticFriction`, `physics:dynamicFriction`, and `physics:restitution` (float, unauthored value `0`). A collider takes the material of its `material:binding:physics` relationship, or of its all-purpose `material:binding` relationship (§15.1).

### 16.7 Physics scenes

A `PhysicsScene` prim states a simulated world. `physics:gravityDirection` (vector3f, unauthored value `(0, 0, 0)`) is the direction of gravity; the zero vector is the direction opposite the stage up axis. `physics:gravityMagnitude` (float, unauthored value `-inf`) is the gravitational acceleration in length units per squared second; `-inf` is standard gravity, 9.80665 metres per squared second.

### 16.8 PhysX schemas

The PhysX API schemas state the parameters of the PhysX solver. Their properties are in the namespace of their schema, for example `physxRigidBody:` for `PhysxRigidBodyAPI`, or in the namespace and the instance name of a multiple-apply schema, for example `physxLimit:angular:` for `PhysxLimitAPI:angular`.

| Schema | Prim |
| --- | --- |
| `PhysxSceneAPI`, `PhysxSceneQuasistaticAPI` | a `PhysicsScene` prim |
| `PhysxRigidBodyAPI`, `PhysxContactReportAPI`, `PhysxSurfaceVelocityAPI`, `PhysxForceAPI`, `PhysxTendonAttachmentAPI`, `PhysxTendonAttachmentRootAPI`, `PhysxTendonAttachmentLeafAPI` | a rigid body |
| `PhysxArticulationAPI` | an articulation root |
| `PhysxCollisionAPI`, `PhysxConvexHullCollisionAPI`, `PhysxConvexDecompositionCollisionAPI`, `PhysxSphereFillCollisionAPI`, `PhysxTriangleMeshCollisionAPI`, `PhysxTriangleMeshSimplificationCollisionAPI`, `PhysxSDFMeshCollisionAPI` | a collider; on a proxy mesh (§16), the collider of its B-rep body |
| `PhysxMaterialAPI` | a `Material` prim with `PhysicsMaterialAPI` |
| `PhysxJointAPI`, `PhysxPhysicsDistanceJointAPI`, `PhysxLimitAPI`, `PhysxJointAxisAPI`, `PhysxDrivePerformanceEnvelopeAPI`, `PhysxMimicJointAPI`, `PhysxTendonAxisAPI`, `PhysxTendonAxisRootAPI`, `PhysicsJointStateAPI` | a joint |

Each attribute is in the unit that the PhysX schema states for it, in layer units (§16.1). An angle is in degrees unless the schema states radians. `inf` and `-inf` are values with no bound.

The unit of an attribute of a joint can depend on the degree of freedom it acts on. The instance name of a multiple-apply schema names the degree of freedom: `rotX`, `rotY`, `rotZ`, and `angular` are angular, and `transX`, `transY`, `transZ`, `linear`, and `distance` are linear. On a schema without an instance name, the joint type names it: a prismatic or distance joint is linear, and another joint is angular.

| Attribute | Linear | Angular |
| --- | --- | --- |
| `physxJoint:armature`, `physxJointAxis:*:armature` | mass | mass · length² |
| `physxJoint:maxJointVelocity`, `physxJointAxis:*:maxJointVelocity`, `physxDrivePerformanceEnvelope:*:maxActuatorVelocity` | length / s | degree / s |
| `physxJointAxis:*:staticFrictionEffort`, `physxJointAxis:*:dynamicFrictionEffort` | force | torque |
| `physxJointAxis:*:viscousFrictionCoefficient`, `physxDrivePerformanceEnvelope:*:velocityDependentResistance` | force · s / length | torque · s / degree |
| `physxDrivePerformanceEnvelope:*:speedEffortGradient` | (length / s) / force | (degree / s) / torque |
| `physxLimit:*:stiffness` | force / length | torque / degree |
| `physxLimit:*:damping` | force · s / length | torque · s / degree |
| `physxLimit:*:bounceThreshold`, `physxMimicJoint:*:offset`, `state:*:physics:position`, `state:*:physics:velocity` | length, or length / s | degree, or degree / s |

`physxMimicJoint:*:gearing` is the unit of the mimic joint's degree of freedom over the unit of the degree of freedom that `physxMimicJoint:*:referenceJointAxis` names. `physxMaterial:compliantContactStiffness` is force / length and `physxMaterial:compliantContactDamping` is force · s / length; when `physxMaterial:compliantContactAccelerationSpring` is `true`, both are accelerations per unit of penetration and have no mass or length unit. `physxForce:force` is a force and `physxForce:torque` a torque when `physxForce:mode` is `force`; otherwise they are a linear acceleration and an angular acceleration in degrees per squared second.

A relationship of a PhysX schema, such as `physxMimicJoint:*:referenceJoint`, targets a joint or a rigid body.
