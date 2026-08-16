# Rhino openNURBS comparison

The comparison harness uses the 153 `.3dm` files in the openNURBS
`example_files` directory. It builds and runs `example_read` for every file,
decodes the same file with the Rhino codec, validates each successful CADIR,
checks the source object total for every archive version, and enforces a
non-decreasing supported-object count.

Run the comparison from the repository root:

```text
python3 tools/validate_rhino_opennurbs.py /path/to/opennurbs
```

The committed transfer floors are:

| Archive | Supported floor | Source objects |
| ------: | --------------: | -------------: |
|       2 |           1,989 |          2,342 |
|       3 |           2,413 |          2,477 |
|       4 |              47 |            173 |
|      50 |              92 |            198 |
|      60 |              28 |             37 |
|      70 |              31 |             46 |
|      80 |              24 |             39 |

The test fails when `example_read` refuses a file, `cadmpeg check` reports
an error, a source-object total changes, or a supported-object count falls
below its floor.

The class-wrapper scanner does not validate `TCODE_CLASS_DATA` as a flat byte
range. A class payload can interleave direct fields and complete nested chunks,
and only the class grammar can identify those ranges. Flat validation produced
false integrity failures on Rhino-authored Breps and prevented otherwise valid
object commits. Concrete payload readers validate their nested chunks and the
comparison pins the resulting object-transfer floor.
