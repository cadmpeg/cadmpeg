# jq recipes for CADIR documents

Reach for `cadmpeg query` first: `summary` names the artifact kind,
`counts` lists the arenas this document actually has, and
`item FILE ARENA [ID...]` fetches records (`--fields a,b.c` projects TSV
with empty cells for absent values). jq is for what those nouns do not
express: field predicates, aggregation, and cross-arena joins. These
recipes exist because every mistake below has been made repeatedly; copy
the shapes instead of re-deriving them.

## Ground rules

- Most CADIR fields are optional and serialize as absent, not as `[]` or
  `{}`. Guard every access: `.model.faces[]?`, `(.outputs // [])`,
  `(.name // "")`. An unguarded access fails loudly on the first record
  that omits the field — or worse, `select` chains return nothing and
  look like a real "no matches".
- `|` binds looser than `,`. Inside `[…]` or `{…}`, parenthesize each
  step: `[(.faces|length), (.edges|length)]`. Without the parens the
  first result is piped onward and the error points somewhere useless.
- Bind the document root before any nested pipeline: `. as $doc`. After
  `select(…) as $x` the context is the matched element, so a bare
  `.model` is `null` from there on.
- Put any program longer than one line in a file and run
  `jq -f prog.jq doc.json`. Inlining a quoted program inside a
  shell-in-shell string manufactures syntax errors that point at the
  wrong column.
- jq math builtins are 0-arity filters or `f(a; b)` forms:
  `(map(.*.) | add) | sqrt`, `atan2(y; x)`. For real numeric work use
  `python3`; it is installed, `ruby` is not.

## Recipe: annotate one arena with another (INDEX join)

Faces carry a `surface` id; surfaces live in their own arena. Build the
lookup once, then annotate:

```sh
jq -f face-surfaces.jq doc.json
```

```jq
# face-surfaces.jq — face id, surface id, surface kind
. as $doc
| INDEX($doc.model.surfaces[]?; .id) as $surface
| $doc.model.faces[]?
| [.id, .surface, ($surface[.surface].geometry.type // "absent")]
| @tsv
```

`INDEX(stream; key_expr)` builds `{key: record}` in one pass. The
`// "absent"` keeps a dangling reference visible instead of exploding.

## Recipe: group children under a parent

Count faces per shell without a quadratic scan:

```jq
. as $doc
| ($doc.model.faces // [])
| group_by(.shell)
| map({shell: .[0].shell, faces: length})
| .[] | [.shell, .faces] | @tsv
```

`group_by` sorts by the key first; equal keys are adjacent, so each
group is complete.

## Recipe: three-way join through an id chain

Feature → its result faces → their surfaces, tolerating gaps at every
hop:

```jq
. as $doc
| INDEX($doc.model.faces[]?; .id) as $face
| INDEX($doc.model.surfaces[]?; .id) as $surface
| $doc.model.features[]?
| . as $f
| ($f.outputs // [])[]
| $face[.] // empty
| [$f.id, .id, ($surface[.surface].id // "absent")]
| @tsv
```

Every `// empty` and `// "absent"` is deliberate: an id that resolves
nowhere is data (a gap to report), not a crash.

## Recipe: same join across native and model namespaces

Native records reference model entities by full id string. The pattern
is identical — index the model arena, walk the native one:

```jq
. as $doc
| INDEX($doc.model.features[]?; .id) as $model
| $doc.native.<codec>.arenas.<arena>[]?
| [.id, (.model_ref // "" | $model[.].id // "unlinked")]
| @tsv
```

Substitute the codec, arena, and reference field for the document at
hand — read one record first with `cadmpeg query item` to see the real
field names instead of guessing them.
