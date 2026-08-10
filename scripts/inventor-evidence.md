# Inventor evidence harness

`inventor-evidence.py` is a local census and wrapper-framing oracle. It reads
regular files by content, follows CFB FAT, DIFAT, directory, mini-FAT, and
stream chains, and parses the RSe database, registry, Meta Stream 8 tables,
bulk envelope, record frames, and typed kernel-carrier envelope.

The JSON result contains ordinal document ids, declared envelope tuples,
logical ranges, physical stream spans, and SHA-256 digests. It does not emit
input paths or source file names. Use an output path outside the repository
for any result that contains observations from a local input collection.

Run the synthetic regression test with:

```text
python3 -m unittest -v scripts/test_inventor_evidence.py
```

Run a census with:

```text
python3 scripts/inventor-evidence.py --root INPUT_ROOT --output RESULT.json
```

`--compare --cadmpeg PATH` adds active-carrier wrapper/direct semantic and
validation comparisons. It normalizes codec id namespaces and excludes
presentation-only geometry color fields from the shared geometry fingerprint.
It uses one temporary carrier file per comparison and applies the configured
`--timeout` independently to every command.

`--sweep --cadmpeg PATH` runs inspect, decode, and validate in desktop and
service profiles, in salvage and strict modes, and repeats decode and validate
to check deterministic output. Each command receives its own timeout.
