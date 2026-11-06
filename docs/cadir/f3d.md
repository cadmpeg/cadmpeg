# Autodesk Fusion 360 `.f3d`: CADIR writing

Decode and export contract for F3D. The format grammar is in
[f3d.md](../formats/f3d.md).

Body recipes, construction operands, edge selections, and face selections
project to `feature_input_topologies`. Referenced sheet-metal settings feed their
features; unreferenced settings project to a native feature. Sketch points,
curve identities, and relations project to sketch records. Canvas and decal images
project to assets, and mesh features to tessellations. Parameter `source_kind`,
`owner_record_index`, `unit`, and `evaluated_scalar` stay in typed native
records. Decoded features have `needs_recompute = false`.

Writers author the document's neutral content first. Scalar document parameters and configuration tables are regenerated from neutral values, expressions and overrides. Supported native-only metadata supplies its own typed fields. A native projection that disagrees with a neutral record is regenerated or omitted. Synthesized archives consume no retained source bytes, including raw historical and sketch-text payloads. Replay and patch require empty `unrepresented`. An unsupported edit selects synthesis and reports `PatchDeclined`.

Unsupported procedural constructions use a solved carrier when available. Unsupported parameter-space trims and faces are omitted locally with Error-severity losses; remaining faces can form an open sheet. Unsupported whole arenas and native records have explicit export dispositions and losses. Writer errors are `InvalidInput`, `TargetCapacity`, `UnsupportedTarget`, `ResourceLimit`, or `Io`. Empty geometry output charges `export:output.empty`; caller policy decides whether to accept it.
