# Autodesk Fusion 360 `.f3d`: CADIR writing

Writer and export contract for F3D. Decode semantics live in
`../formats/f3d.md`.

Writers author the document's neutral content first. Scalar document parameters and configuration tables are regenerated from neutral values, expressions and overrides. Supported native-only metadata supplies its own typed fields. A native projection that disagrees with a neutral record is regenerated or omitted. Synthesized archives consume no retained source bytes, including raw historical and sketch-text payloads. Replay and patch require empty `unrepresented` and the content that synthesis can author. An unsupported edit selects synthesis and reports `PatchDeclined`.

Unsupported procedural constructions use a solved carrier when available. Unsupported parameter-space trims and faces are omitted locally with Error-severity losses; remaining faces can form an open sheet. Unsupported whole arenas and native records have explicit export dispositions and losses. A writer refuses only malformed/self-inconsistent input, target-format capacity limits, or an unresolvable target. An empty geometry carrier is valid output and reports `NoExportableSolids`; caller policy decides whether to accept it.
