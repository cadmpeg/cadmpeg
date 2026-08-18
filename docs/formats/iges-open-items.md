# IGES open items

IGES L9 is not achieved. The current score is L8. The bounded semantic
writer and its independent-application checks are extras above L8; they do not
close the L9 gate while decode can time out, return invalid `CadIr`, or omit
semantic records from transfer.

This document lists the parts of the IGES format that we do not know. The specification `iges.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## P0 — Make decode terminating and resource-bounded

Fixed ASCII decode has exceeded the 30-second per-file guard on multiple
inputs. This is pathological and unacceptable for a production codec. The
decoder must not spend unbounded time in parameter assembly, reference graph
construction, topology projection, or geometric carrier recovery.

Required closure:

- instrument each decode stage and record the dominant cost for a reduced
  reproducer;
- bound every file-declared count, recursive traversal, graph walk, and
  geometry-recovery search with the service resource policy;
- return a deterministic structured resource error when a bound is exceeded;
- add synthesized regression fixtures for each pathological stage; and
- run the bounded full-file gate in CI so a timeout cannot be reported as a
  successful decode.

The item is closed only when every file in the declared envelope reaches a
terminal success or a bounded, classified error within the agreed limit.

## P0 — Decode success must imply valid `CadIr`

The decoder can return success for documents that `cadmpeg check` rejects.
Observed failures include edge parameter ranges outside their canonical curve
domains and edge curve endpoints that do not meet their vertex positions.

Required closure:

- canonicalize or reject carrier domains before committing edges;
- validate edge endpoints, pcurves, topology ownership, and transforms before
  returning decode success;
- commit no partial topology after a failed validation; and
- add synthesized fixtures for each failure class and run decode followed by
  `cadmpeg check` in the regression gate.

The item is closed only when a successful semantic decode is a valid `CadIr`,
not merely a parseable command result.

## P0 — Account for every omitted semantic record

Successful decodes still produce `record_not_typed` and
`material_not_transferred` losses for trimming, display, and other entity
branches. The read profile must not call these branches complete while the
decoder either drops their semantics or cannot prove their preservation.

Required closure:

- assign every unsupported or omitted semantic construct a stable loss code,
  severity, source identity, and retained native record;
- distinguish deliberate native preservation from geometric projection loss;
- make `--no-salvage` reject all losses that can change model, topology, product,
  or document meaning; and
- update the read profile only after loss coverage and validation pass.

## P0 — Re-establish the L9 gate

L9 remains open until bounded decode, valid-IR output, complete loss accounting,
semantic writing, target-version selection, and independent application
acceptance pass together. The bounded writer tests are not evidence that the
full declared read/write envelope passes this gate.

Required closure:

- run decode, validate, convert, and generated-file re-decode as one evaluated
  gate;
- require independent native-application acceptance for every writable
  profile, including edited and source-less documents; and
- keep the support table and codec README at L8 until this gate passes.

## P1 — Exercise the writer under fuzzing and continuous stress

The current IGES fuzz target exercises container detection, inspection, and
decode. It does not exercise semantic planning, target-version emission, or
writer rejection paths.

Required closure:

- add writer fuzz coverage for valid and malformed `CadIr` values;
- cover replay, source-less synthesis, target versions, topology, loss
  rejection, and unsupported native arenas;
- record a reproducible fuzz campaign and retain minimized regressions; and
- run the timeout and validation gates continuously rather than as an
  environment-only check.

# Unrecorded format rules

The items below record decode and write rules that the codec applies and that
neither IGES nor `iges.md` states. They come from a directed sweep of the codec
on 2026-08-08. Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now, with the code that depends on it.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and
  the code. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Use the identifier in commit messages and in code comments. This document uses
ASD-STE100 Simplified Technical English. Record names, field names, and token
values are technical names. They keep their source spelling.

Many items have one shape: the codec refuses an entity because a value misses a
threshold that the codec selected, or because a field is blank where the codec
requires an explicit value. A refusal is not a safe default. It removes geometry
from a conformant file.

## 1. Physical framing and lexical rules

### PH-03. The boundary between entity parameters and trailing pointer groups

**Question.** Where does an entity's own parameter list stop and its trailing pointer groups start?

**Known.** IGES 5.3 §§2.2.4.5.1–2 state that the type number precedes Index 1, that the entity type/form table supplies the last specified parameter number `NV`, and that the two counted pointer groups follow those parameters before the record delimiter. `parameter.rs::specified_parameter_end` registers Type 102 Form 0 at token `N+2`, where Parameter Data index 1 supplies positive `N` and indexes 2 through `N+1` are the counted constituent pointers; Type 106 through `copious_tuple_width` at tokens `4+2*N`, `3+3*N`, or `3+6*N` for the form-required IP; Type 402 Forms 1, 7, 14, and 15 at token `N+2`, where index 1 supplies `N` and indexes 2 through `N+1` are member pointers; it registers Type 110 Forms 0–2 at token 7 and Type 116 Form 0 at token 5. Section §4.4 supplies the Type 102 count-and-pointer layout. Sections §§4.6–4.11 supply the Type 106 IP/count layouts and form constraints. Sections §§4.81, 4.85, 4.89, and 4.90 supply the Type 402 group count/member layouts. Section §4.16 lists Type 116 X, Y, Z, and the optional display-symbol pointer at indexes 1–4; §§2.2.1 and 2.2.3 require the defaulted slot to remain represented when later groups follow and default the remaining fields at the record delimiter. `parameter.rs::defaulted_trailing_count` accepts the omitted count fields only for a registered boundary, so Type 116 accepts explicit or omitted `PTR`, an association-only suffix with defaulted `NP`, and a property-only suffix with explicit `NA=0`. `native.rs` uses the selected groups for Type 402 ownership, Type 406 ownership, and parameter-reference provenance.

**Need.** We need the exact `NV` formula for each remaining supported entity/form pair, including optional or defaulted entity fields. Type 102 Form 0 is settled. A candidate with multiple valid unregistered boundaries must remain raw and produce the structured ambiguity loss. An unresolved pointer at a proven boundary remains a parameter reference and graph finding; unresolved candidates without a proven boundary remain part of the remaining entity-specific work.
Type 106 is now settled for every supported form/IP width. The remaining work is the exact formula for other supported variable-width entity/form pairs.
Type 402 Forms 1, 7, 14, and 15 are now settled. The remaining work is the exact formula for other supported variable-width entity/form pairs.
Type 308 Form 0 is now settled. `N` is at Parameter Data index 3, its ordered member pointers occupy indexes 4 through `3+N`, and the first trailing count is token `4+N`. PH-03 remains open for the other supported variable-width layouts.
Type 504 Form 1 is now settled. Positive `N` is at Parameter Data index 1; each edge tuple occupies five tokens `(CURV, SVP, SV, TVP, TV)`; and the first trailing count is token `2+5*N`. A missing, nonpositive, wrong-typed, or truncated count or tuple list suppresses generic suffix recovery, while a wrong-typed field in a complete tuple span retains the count-defined boundary. The official §4.144 table, the public Open CASCADE `IGESSolid_ToolEdgeList::ReadOwnParams`/`WriteOwnParams`, the repository writer, and the controlled witnesses `/home/pcurve/side2/tmp/freecad-l9/ph03-type504-valid.igs` (`77ab26957ededd797134e7790ccdc0d03775b687dd0d2f21cdfe3ea412334d97`), `/home/pcurve/side2/tmp/freecad-l9/ph03-type504-two-edges.igs` (`3d5a2e1434ee33b187102b0e31c952871c3557edefd9337c476b7de097aba8eb`), `/home/pcurve/side2/tmp/freecad-l9/ph03-type504-wrong-tuple-field.igs` (`1f1426ea09860e9e88f24dc220df2591d6c524b0ae44ea59dce173ef8946c8c8`), `/home/pcurve/side2/tmp/freecad-l9/ph03-type504-wrong-count-type.igs` (`41b3bc142489be6e8bb5ab8358e2f474663d5942caca75956c770a50eeb63784`), `/home/pcurve/side2/tmp/freecad-l9/ph03-type504-zero-count.igs` (`e8dd8fd5fb3612c18b07b0d4e24c7c168624f2e0b40e154c11651401bfe2130c`), and `/home/pcurve/side2/tmp/freecad-l9/ph03-type504-truncated-tuple.igs` (`2af8b3a2cd31aee8681ce65b28dc64a6dcd4c6230961d092ca61d8270778aaa8`) establish the boundary and CADIR admission decision. Owner tests cover one- and two-edge widths, a wrong tuple field, and invalid or truncated primary spans. PH-03 remains open for the other supported variable-width layouts.
Type 508 Form 1 is now settled. Positive `N` is at Parameter Data index 1. Use `i` starts at token `c_i`, with `c_1=2`, and occupies five fixed tokens `(TYPE, EDGE, NDX, OF, K)` followed by `K_i` `(ISOP, CURV)` pairs; the next use starts at `c_i+5+2*K_i`, so the first trailing count is token `2+5*N+2*ΣK_i`. The official [IGES 5.3 §4.145](https://paulbourke.net/dataformats/iges/IGES.pdf) table, the public Open CASCADE [`IGESSolid_ToolLoop::ReadOwnParams` and `WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/V7_8_1/src/IGESSolid/IGESSolid_ToolLoop.cxx), the repository `entities/brep.rs` reader and `writer.rs` emitter, and the rebuilt controlled witnesses at loop offset `0x4bf` establish the nested width. Witness SHA-256 values are `5eff8715fb5e6838c154e21313c4fdbabf9382e80429a840a6863031eafa7d48` (K=0), `8e7a144169a9d48f321c788327f3fb8be10ab09c2518d35f8836f4b9602ae9d4` (K=1), `845b8b9a89bd65a4f736f9a573d9d0f4f710c34831daa459d18f67d66807f0bcf` (N=2 mixed widths), `95596dacd75f911854b1c85fb685ed2e485ed439c23265b39597e2a0fd0e313c` (wrong complete ISOP), `e2f39479a968e2727195497d0a46bf5ab2327cb51c10d7271eaecd3fbdd8753f` (wrong N type), `59383a83bb1a573fa96625decdebb8e9f2d9cde935e578573706655ed0b4a3c0` (zero N), `972553f7fec4e94b69ba47dc645fbf5f67787133f46c62962f453257912a5154` (wrong K type), `4dfcf84cf67105f3e5ad3b4d1d6ac697830c1e2b27a89080480109ebda7402c6` (negative K), `125c6538eadf154b8a676a7dadb6ca4ac1fc744422d96009e6b18ba675e2cb6a` (truncated use), and `983c7ac44df14977390c86fa74c082ddf9d0b29fc9d9f0e32043f0297706e982` (truncated pair). Valid K=0, K=1, and mixed-width witnesses resolve association/property groups at parameter indexes `(8,10)`, `(10,12)`, and `(15,17)`; a wrong complete ISOP retains `(10,12)` and emits only the loop projection loss. Wrong or incomplete count/width spans suppress suffix recovery. Owner tests are `type508_count_driven_boundary_follows_nested_edge_use_widths`, `type508_wrong_typed_use_field_keeps_nested_count_boundary`, and `type508_invalid_count_or_nested_width_suppresses_generic_suffix_candidate`. PH-03 remains open for the other supported variable-width layouts.
Type 510 Form 1 is now settled. Positive `N` is at Parameter Data index 2, `OF` is at index 3, the ordered LOOP pointers occupy indexes 4 through `3+N`, and the first trailing count is token `4+N`. A missing, nonpositive, wrong-typed, or truncated count/list suppresses suffix recovery; a wrong `OF` in a complete span retains the boundary and the face projection loss. The official [IGES 5.3 §4.146](https://paulbourke.net/dataformats/iges/IGES.pdf) table, public Open CASCADE [`IGESSolid_ToolFace::ReadOwnParams` and `WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/V7_8_1/src/IGESSolid/IGESSolid_ToolFace.cxx), repository `entities/brep.rs` and `writer.rs`, and the rebuilt witnesses establish this rule.
Type 514 Forms 1 and 2 are now settled. Positive `N` is at Parameter Data index 1; each face-use pair is `(FACE, OF)` at indexes `2+2*i` and `3+2*i`; and the first trailing count is token `2+2*N`. Form 1 is closed and Form 2 is open. A missing, nonpositive, wrong-typed, or truncated count/list suppresses suffix recovery; a wrong orientation in a complete pair span retains the boundary and the shell projection loss. The official [IGES 5.3 §4.147](https://paulbourke.net/dataformats/iges/IGES.pdf) table, public Open CASCADE [`IGESSolid_ToolShell::ReadOwnParams` and `WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/V7_8_1/src/IGESSolid/IGESSolid_ToolShell.cxx), repository `entities/brep.rs` and `writer.rs`, and the rebuilt witnesses establish this rule.
Type 404 Forms 0 and 1 are now settled. `N` is at Parameter Data index 1. Form 0 repeats `(VPTR, XORIGIN, YORIGIN)` with width 3; Form 1 repeats `(VPTR, XORIGIN, YORIGIN, ANGLE)` with width 4. `M` is at index `2+3*N` or `2+4*N`, respectively, and the first trailing count is token `3+3*N+M` or `3+4*N+M`. Explicit zero `N` and `M` are valid. A missing, wrong-typed, negative, or truncated count/list suppresses suffix recovery; a wrong view or annotation pointer in a complete span keeps the boundary and emits the drawing projection loss. The official [IGES 5.3 §4.96](https://paulbourke.net/dataformats/iges/IGES.pdf) table defines both forms; public Open CASCADE [`IGESDraw_ToolDrawing::ReadOwnParams` and `WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/V7_8_1/src/IGESDraw/IGESDraw_ToolDrawing.cxx) and repository `entities/drawing.rs:84-138` independently establish the Form 0 view/annotation sequence; repository `parameter.rs::specified_parameter_end` and the rebuilt Form 0/Form 1 witnesses establish the checked boundaries.
Type 402 Forms 3 and 4 are now settled. Form 3 stores positive `N1` at index 1, nonnegative `N2` at index 2, `N1` Type 410 view pointers at indexes 3 through `2+N1`, and `N2` displayed-entity pointers at indexes `3+N1` through `2+N1+N2`; its first trailing count is token `3+N1+N2`. Form 4 stores the same counts and displayed-entity list with width-5 `(DEV,LF,DEF,CN,LW)` view blocks, so its first trailing count is token `3+5*N1+N2`. Explicit `N2=0` is valid; the CADIR boundary requires the `N2` token. A missing, wrong-typed, negative, or truncated count/list suppresses suffix recovery; a wrong complete view field or displayed-entity pointer keeps the boundary and emits the view-visibility projection loss. The official [IGES 5.3 §§4.82–4.83](https://paulbourke.net/dataformats/iges/IGES.pdf) tables, public Open CASCADE [`IGESDraw_ToolViewsVisible::ReadOwnParams`/`WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/V7_8_1/src/IGESDraw/IGESDraw_ToolViewsVisible.cxx) and [`IGESDraw_ToolViewsVisibleWithAttr::ReadOwnParams`/`WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/V7_8_1/src/IGESDraw/IGESDraw_ToolViewsVisibleWithAttr.cxx), repository `entities/drawing.rs:330-408` and `native.rs:3596-3660`, and the rebuilt witnesses establish this rule.
Type 230 Form 0 is now settled. `N` is at Parameter Data index 8, its ordered island pointers occupy indexes 9 through `8+N`, and the first trailing count is token `9+N`. PH-03 remains open for the other supported variable-width layouts.
Type 320 Form 0 is now settled. `NA` is at Parameter Data index 3, its ordered child pointers occupy indexes 4 through `3+NA`, `NC` is at index `7+NA`, its nullable connect-point pointers occupy indexes `8+NA` through `7+NA+NC`, and the first trailing count is token `8+NA+NC`. PH-03 remains open for the other supported variable-width layouts.
Type 406 Form 14 is now settled. Positive `NP` is at Parameter Data index 1, its `NP` string values occupy indexes 2 through `1+NP`, and the first trailing count is token `2+NP`. A missing, nonpositive, wrong-typed, or truncated count/list is malformed; a wrong-typed value token does not move the count-defined boundary.
Type 406 Form 7 is now settled. `NP=1` is at Parameter Data index 1, the reference-designator string occupies index 2, and the first trailing count is token 3. A complete primary span keeps that boundary for a wrong-typed string; a wrong or omitted `NP`, or a truncated primary span, suppresses generic suffix recovery. The official [IGES 5.3 §4.103](https://paulbourke.net/dataformats/iges/IGES.pdf) table, public Open CASCADE [`IGESAppli_ToolReferenceDesignator::ReadOwnParams`, `WriteOwnParams`, and `OwnCheck`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/V7_8_1/src/IGESAppli/IGESAppli_ToolReferenceDesignator.cxx), repository `native.rs`, `entities/structure.rs`, and `parameter.rs::specified_parameter_end`, and the rebuilt witnesses at source offset `0x288` establish the rule. Witness SHA-256 values are `2de99be65925d021933655d1d15d2980280b8c5ffab8b28afe41986b4c63df81` (valid), `ecd19f524b7afd60ffb7cca88942ac17f4943f368f0cf3b1cfda0d6dd31f728b` (wrong string), `0f77c02d15bf50c4e4998109416a25650e0b8b483ef04de960fbf85472f54f76` (wrong `NP`), `df15ed2260d11c9628c61c4406a511d6138c8e1bd3230096dd8b7d24c4a23049` (omitted `NP`), and `5f059ff897b1d6e619fd4bab49466ee7f44c3d1f650ddca0509829573408ae25` (truncated `RD`). All five pass `cadmpeg inspect`; post-registration `dump`, `query`, and `check` retain the valid and wrong-string association at parameter index 4, suppress it for the malformed three cases, and report zero check findings. Owner tests are `type406_form7_fixed_boundary_follows_reference_designator`, `type406_form7_wrong_typed_value_keeps_fixed_boundary`, and `type406_form7_malformed_primary_span_suppresses_generic_suffix_candidate`.
Type 322 Form 0 is now settled. Positive `NA` is at Parameter Data index 3; each descriptor occupies three tokens at `4+3*i` through `6+3*i`; and the first trailing count is token `4+3*NA`. Omitted `AVC` defaults to one, while zero `AVC` stores no value token. Missing, nonpositive, wrong-typed, or truncated `NA` or descriptor data is malformed, while a wrong-typed descriptor field does not move the count-defined boundary.
Type 322 Forms 1 and 2 are now settled. Positive `NA` is at index 3 and descriptor `i` starts at `C_i`, with `C_0=4`, `C_(i+1)=C_i+3+AVC_i` for Form 1, and `C_(i+1)=C_i+3+2*AVC_i` for Form 2. Omitted `AVC` has effective count one; zero `AVC` consumes no value slots. Form 1 consumes one value token per count. Form 2 consumes a value and one nullable Type 312 pointer slot per count. The first trailing count is token `C_NA`. A missing, negative, or wrong-typed `AVC`, or truncated counted slots, is malformed; wrong typed values or display pointers keep the proven boundary and retain their projection or reference loss.
Type 302 Forms 5001–9999 are now settled. Positive `K` is at Parameter Data index 1; class `c` starts at token `2 + Σ(j<c)(3+N_j)` and stores `BP`, `OR`, positive `N_c`, and `N_c` item types; and the first trailing count is token `2 + 3*K + ΣN_c`. Missing, nonpositive, wrong-typed, or truncated `K` or class data is malformed, while a wrong item type does not move the count-defined boundary.
Type 422 Forms 0 and 1 are now settled. A resolved negative Structure pointer to a Type 322 Form 0 definition supplies `S = Σ AVC_i`; Form 0 stores one `S`-value tuple and begins its trailing groups at token `1+S`, while Form 1 stores positive `NR` and `NR` row-major tuples and begins its trailing groups at token `2+NR*S`. Missing, malformed, unresolved, or non-Form0 definitions and malformed Form 1 row fields suppress suffix recovery; wrong typed instance values keep the definition-derived boundary.
Type 316 Form 0 is now settled. Positive `NP` is at Parameter Data index 1; unit entry `i` occupies the three tokens at indexes `2+3*i` through `4+3*i`; and the first trailing count is token `2+3*NP`. Missing, nonpositive, wrong-typed, or truncated `NP` or unit-entry data is malformed, while a wrong-typed unit value does not move the count-defined boundary.
Type 402 Form 16 is now settled. `NTR=1` is at Parameter Data index 1, `N` is at index 2, the transformation pointer is at index 3, and `N` coplanar entity pointers occupy indexes 4 through `3+N`; the first trailing count is token `4+N`. CADIR admission requires positive `N`; malformed `NTR`, count, or entity-list data suppresses suffix recovery, while a wrong transformation or entity pointer does not move the count-defined boundary.
Type 402 Form 21 is now settled. `ND=1` is at index 1, `NG` is at index 2, the dimension tuple occupies indexes 3 through 5, and each related-geometry entry occupies five tokens beginning at index `6+5*i`; the first trailing count is token `6+5*NG`. CADIR admission requires positive `NG` and a complete geometry span. Wrong-typed geometry fields keep the count-defined boundary; wrong `ND`/`NG`, nonpositive `NG`, and truncated geometry lists suppress suffix recovery.

**Note.** The synthesized Type 110 witness in `ambiguous_trailing_pointer_boundary_file` established token 7. Type 116 closure uses the official §4.16 table, the public Open CASCADE `IGESGeom_ToolPoint::ReadOwnParams`/`WriteOwnParams` and `IGESData_IGESWriter::Associativities`/`Properties` paths, the headless FreeCAD export SHA-256 `29c8182fe1d5a3289fc13d48713b9717d7c3ab19517721b606b9b4fcf1920626`, and synthesized explicit-zero, omitted, association-only, and property-only witnesses with SHA-256 values `e78b9d345166bb981ff3d7d083c2e8098895d7884b5853060cced1289e5553da`, `e65b616754437692a1d612de6da861c0a7df107522c06fc89b180b602b910ab8`, `a08ca27312639b231d962fff0c4d6f7d5ebb1fe418cb66936c7534a9049fd367`, `3b805654dcf676ca357b92ec3e5332ea2c687bfb15bd96921b652477ad6abfc9`, `f7e67667a05c5a3b70463123fe3b656ec591fa80021d3d92`, and `0175891d2ee37220fb9d95be7b8da80f903e46af410af6ccb94d492f3ccc3ed9`. The six synthesized witnesses pass fresh inspect/dump/query/check runs with zero findings and losses; the FreeCAD export supplies the explicit-zero `PTR` witness but has an unrelated Type 402 reverse-pointer warning. The corrected `variable_schema_property_forms` fixture and reviewed snapshots record the explicit `NA=0` field. Type 102 Form 0 uses the official §4.4 table and the public Open CASCADE `IGESGeom_ToolCompositeCurve::ReadOwnParams`/`WriteOwnParams` path: the reader consumes `N` and then exactly `N` entities, and the writer emits `N` followed by the same list. Fresh synthesized witnesses are `/home/pcurve/side2/tmp/freecad-l9/ph03-type102-one-child.igs` SHA-256 `4fced53a2576aa9f1298c0f1ace8e0d69a6fbc9b7ee7ea178d9bb18526a9ef5b`, `/home/pcurve/side2/tmp/freecad-l9/ph03-type102-two-children.igs` SHA-256 `86e87b464d200fca31746e1d849214e785ff80e405dd77a11f99f9f0b6734f2b`, and malformed `N=0` `/home/pcurve/side2/tmp/freecad-l9/ph03-type102-invalid-count.igs` SHA-256 `7d53a1c50ba48f3065101ad68fe87cc6dd4a5d63aa1d00a953625e5eee602c58`. The two valid records resolve the Type 402 association at parameter indexes 4 and 5 with zero findings/losses; the malformed record has no inferred links and one attributed `entity.not-projected` loss. The owner tests are `type102_count_driven_boundary_follows_constituent_list` and `type102_invalid_count_suppresses_generic_suffix_candidate`. The witness generator initially used a nine-character Directory label, which shifted cards and caused `inspect` to reject the file; replacing it with the required eight-character `COMPOSIT` label fixed the harness and all three files passed inspect. Type 102 Form 0 is settled; PH-03 remains open for the remaining supported layouts.
Type 106 evidence: the official §§4.6–4.11 tables and public Open CASCADE `IGESGeom_ToolCopiousData::ReadOwnParams`/`WriteOwnParams` establish the IP/count widths. The valid witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type106-forms-1-2-3.igs` has SHA-256 `f24e980b7075c57b816df4152ceaec10aff6ab2d4e56f3b0d374a4dcd4bd5aa6`; the ambiguity witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type106-ambiguous.igs` has SHA-256 `2016274744a34b05235d2aa347f8cd646cab2572d29f8d00e081cc60f600b2be`; the invalid-count witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type106-invalid-count.igs` has SHA-256 `0514d5692cb66c8fcb92816d84427eba6295d7c6e5713356e4f9a32123b4f57c`. Before registration, the ambiguity witness produced two fully valid candidates for D3, D5, and D7 and three structured ambiguity losses. After registration, all three links resolve at parameter indexes 9, 10, and 16 and the valid and ambiguity checks have no findings or losses. The invalid-count witness retains no inferred links and one `entity.not-projected` loss. Owner tests are `type106_ip_width_defines_boundary_for_forms_1_2_and_3`, `type106_form_ip_mismatch_suppresses_generic_suffix_candidate`, `type106_form63_rejects_nonplanar_ip_before_suffix_recovery`, and `type106_nonpositive_count_suppresses_generic_suffix_candidate`. The first Form 3 generator omitted two data values; query exposed the wrong pointer index, so the generator now builds the final token sequence from integer arrays. Type 106 is settled; PH-03 remains open for other variable-width layouts.

Type 402 evidence: the official §§4.81, 4.85, 4.89, and 4.90 tables and public Open CASCADE `IGESBasic_ToolGroup::ReadOwnParams`/`WriteOwnParams`, `IGESBasic_ToolGroupWithoutBackP::ReadOwnParams`/`WriteOwnParams`, `IGESBasic_ToolOrderedGroup::ReadOwnParams`/`WriteOwnParams`, and `IGESBasic_ToolOrderedGroupWithoutBackP::ReadOwnParams`/`WriteOwnParams` establish one `N` followed by exactly `N` member pointers. The four-form witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type402-forms-1-7-14-15-ambiguous.igs` has SHA-256 `d27201c698b806eeed5b3b744c02936b9b0cb174600cffaf33dde1999608e6`; before registration, D3, D7, D9, and D11 each produced two fully valid candidates and no inferred suffix link; after registration, each selected the suffix at parameter indexes 4 and 5, with zero findings/losses. The negative-count witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type402-invalid-count.igs` has SHA-256 `e312aa784cc0c136011618678dec34525b33cc458bea5abdca0128e9fd8e209f`; before registration, its generic candidate falsely resolved a pointer at index 3, while after registration it retained no inferred suffix link and one `entity.not-projected` loss. Owner tests are `type402_group_forms_share_count_driven_boundary`, `type402_negative_count_suppresses_generic_suffix_candidate`, `type402_wrong_typed_member_keeps_count_boundary`, and `type402_zero_count_keeps_the_count_defined_boundary`. Type 402 is settled; PH-03 remains open for other variable-width layouts.

Type 308 evidence: [IGES 5.3 §4.73](https://paulbourke.net/dataformats/iges/IGES.pdf) and the public Open CASCADE [`IGESBasic_ToolSubfigureDef::ReadOwnParams` and `WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/master/src/DataExchange/TKDEIGES/IGESBasic/IGESBasic_ToolSubfigureDef.cxx) establish `DEPTH`, `NAME`, `N`, and exactly `N` member pointers. The valid witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type308-count-boundary.igs` has SHA-256 `fddac15c1c35e4f8a44f25a329af0e5f1698f0d50a1c2dcf4a67996822fbd478`; before registration it produced two fully valid trailing boundaries, and after registration the suffix resolved at parameter index 7 while member references resolved at indexes 4 and 5. `cadmpeg check` on the registered witness reported zero findings and losses. The negative-count witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type308-invalid-count.igs` has SHA-256 `ab4c0886ebd2ff4bfe0a327c2f5a62897e9889b57ae33823d0b1e71c4a00dbfa`; before registration it produced two fully valid candidates, and after registration it retained the Type 308 entity loss without ambiguity recovery. The wrong-member witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type308-wrong-member.igs` has SHA-256 `0e54e4e5df327fe1f21857d3200f5e736b89ed58bee256e1088bf732964f1db1`; its suffix association resolved at index 7 while the even member pointer remained a reference finding. Owner tests are `type308_count_driven_boundary_follows_member_list`, `type308_negative_count_suppresses_generic_suffix_candidate`, `type308_wrong_typed_member_keeps_count_boundary`, and `type308_zero_count_keeps_the_count_defined_boundary`. The witness harness corrected an invalid Hollerith length, a zero final pointer, and a missing explicit property count before the pre-registration ambiguity result was accepted.

Type 230 evidence: [IGES 5.3 §4.68](https://paulbourke.net/dataformats/iges/IGES.pdf) and public Open CASCADE [`IGESDimen_ToolSectionedArea::ReadOwnParams` and `WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/master/src/DataExchange/TKDEIGES/IGESDimen/IGESDimen_ToolSectionedArea.cxx) establish `N` at Parameter Data index 8 and exactly `N` island pointers at indexes 9 through `8+N`. The synthesized zero-island witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type230-zero.igs` has SHA-256 `ab23208d8bb793d8574cdea5224b3588fe5cef0cc29e8800ce96e4759b4a2548`; the one-island witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type230-one-island.igs` has SHA-256 `99df7751873e1f847e1cfab94aaa5826442a445d0fc89dd25626b29d3827c3d1`; the negative-count witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type230-negative-count.igs` has SHA-256 `d2542b1e58ab082dc115aa65e0b9abae6d9ef970c54bbde208458b4009fd567b`; the wrong-island witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type230-wrong-island.igs` has SHA-256 `e2fb1198492589050f6e63dbe7218e5e5689493328b5d1ce70bf4721e0b40690`; and the truncated-list witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type230-truncated-island.igs` has SHA-256 `f2fdcc07cf4c3d6da8c6fce974d6e36ad39ab8edaf285dada594fd9f73b7f5fc`. All five pass `cadmpeg inspect`. After registration, the valid one-island suffix starts at index 10 and its island pointer is index 9; the zero-island suffix starts at index 9 and its association pointer is index 10. The wrong-island case keeps the suffix at index 10 while retaining the wrong-type island reference; negative and truncated counts retain the Type 230 entity loss and suppress suffix recovery. The owner tests are `type230_count_driven_boundary_follows_island_list`, `type230_zero_count_keeps_the_count_defined_boundary`, `type230_negative_count_suppresses_generic_suffix_candidate`, `type230_wrong_typed_island_keeps_count_boundary`, and `type230_truncated_island_suppresses_generic_suffix_candidate`.
Type 320 evidence: [IGES 5.3 §4.78](https://paulbourke.net/dataformats/iges/IGES.pdf) and public Open CASCADE [`IGESDraw_ToolNetworkSubfigureDef::ReadOwnParams` and `WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/master/src/DataExchange/TKDEIGES/IGESDraw/IGESDraw_ToolNetworkSubfigureDef.cxx) establish `NA`, the child-pointer list, `TF`, `PRD`, `DPTR`, `NC`, and the connect-point list in that order. The synthesized independent-count witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type320-independent-counts.igs` has SHA-256 `30e0792b4c69d0a4147287278967730345b56fcce62bd196ed2edd99f55a00b9`; the zero-count witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type320-zero-counts.igs` has SHA-256 `96f0709a4d3ba27b80a41583829e66625413fc59a4028f289ef1e918ea1a2e0d`; the wrong-connect-point witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type320-wrong-connect-point.igs` has SHA-256 `e0f54839755e471c5cfa53395fa35535b21bf9646f77a59b86a810b969d79c02`; the negative-count witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type320-negative-member-count.igs` has SHA-256 `cc5daa40f2e3c6a6976f8ca843a79de0fc76f21bcc3f2b4f42407db3dc79f22c`; the truncated-member-list witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type320-truncated-members.igs` has SHA-256 `3c96a669308edb2600db4c091327b7f411936ae3a10b73d1e509d7bde958ef6c`; and the truncated-connect-point-list witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type320-truncated-connect-points.igs` has SHA-256 `e5e0a2fba7f522817fb3c9c028f39a3437193aa22a8fce95eca6803964cee0ad`. All six pass `cadmpeg inspect`. After registration, the independent-count suffix association resolves at index 12 with child pointers at indexes 4 and 5 and the connect-point pointer at index 10; the zero-count suffix association resolves at index 9; and the wrong-connect-point suffix remains at index 11 while its connect-point reference remains wrong-typed at index 9. Negative and truncated counts suppress suffix recovery. The six owner tests are `type320_count_driven_boundary_follows_member_and_connect_lists`, `type320_zero_counts_keep_the_count_defined_boundary`, `type320_wrong_connect_point_keeps_count_boundary`, `type320_negative_member_count_suppresses_generic_suffix_candidate`, `type320_truncated_member_list_suppresses_generic_suffix_candidate`, and `type320_truncated_connect_point_list_suppresses_generic_suffix_candidate`. The registered witnesses' `cadmpeg check` reports have zero findings; malformed and wrong-pointer files retain only their expected decode losses.
Type 406 Form 14 evidence: [IGES 5.3 §4.110](https://paulbourke.net/dataformats/iges/IGES.pdf) and public Open CASCADE [`IGESAppli_ToolFlowLineSpec::ReadOwnParams` and `WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/master/src/DataExchange/TKDEIGES/IGESAppli/IGESAppli_ToolFlowLineSpec.cxx) establish positive `NP` followed by exactly `NP` text values. The synthesized positive witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type406-form14-positive.igs` has SHA-256 `b8d96677b9e4d67d280f816c4944f45ca86552de5f50377590bd695e5141319d`; the zero-count witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type406-form14-zero-count.igs` has SHA-256 `6f1c1bd61a45d781944aebab52c0b2b479435b5771e820f2fc371f7a447f50bf`; the negative-count witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type406-form14-negative-count.igs` has SHA-256 `77547dfae877ae36732a760f1c4d1a2866db4166d5d3be1eed86998a428949cc`; the wrong-value witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type406-form14-wrong-value.igs` has SHA-256 `696faf82abd1e94a435a2a43528677cf885860e1c225e8334ff6156cbccdb864`; and the truncated-list witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type406-form14-truncated.igs` has SHA-256 `4068b3af0c5307c9e0d1d72e09465d26ea2f9b8f0a145df50a45ea13e1b83716`. All five pass `cadmpeg inspect`. After registration, the positive and wrong-value suffix association resolves at parameter index 5, which is token `2+NP+1` for `NP=2`; zero, negative, and truncated count/list witnesses suppress suffix recovery. The wrong-value property retains both counted value positions but emits the expected `entity.not-projected` loss; each witness's `cadmpeg check` report has zero check findings. Owner tests are `type406_form14_count_driven_boundary_follows_string_list`, `type406_form14_zero_count_suppresses_generic_suffix_candidate`, `type406_form14_negative_count_suppresses_generic_suffix_candidate`, `type406_form14_wrong_typed_value_keeps_count_boundary`, and `type406_form14_truncated_string_list_suppresses_generic_suffix_candidate`.

Type 322 Form 0 evidence: [IGES 5.3 §4.79](https://paulbourke.net/dataformats/iges/IGES.pdf) and public Open CASCADE [`IGESDefs_ToolAttributeDef::ReadOwnParams` and `WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/master/src/DataExchange/TKDEIGES/IGESDefs/IGESDefs_ToolAttributeDef.cxx) establish `NA`, the three descriptor fields, the omitted-`AVC` default, and the Form 0 sequence. The synthesized positive witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type322-form0-positive.igs` has SHA-256 `b4eb731fcf27101740a76db66a29cf8b4ba01f0ca252c0ee558ab8b9d68e6f11`; the zero-count witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type322-form0-zero.igs` has SHA-256 `60a7e84b0eb947a12ae81cc1972a6afa0032e3ad716a8b574ed290bdddb78231`; the negative-count witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type322-form0-negative.igs` has SHA-256 `7ffebf29282f63250ea2ceba30e5d4fd16800e4a26f5e50bfbf64ed9598efd8c`; the wrong-descriptor witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type322-form0-wrong.igs` has SHA-256 `dd891ef3237af9ce0289b52440bf0f69946c3d4d635551796fdbee780e94eaf3`; and the truncated-list witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type322-form0-truncated.igs` has SHA-256 `0b3e30b4dfd7763c9cac52e06bcff6d057a2f9a75d0a577c05066df84dc24391`. All five pass `cadmpeg inspect`. After registration, the positive and wrong-descriptor suffix associations resolve at parameter index 11 for `NA=2`; the zero, negative, and truncated witnesses suppress suffix recovery. The wrong-descriptor witness retains the expected `entity.not-projected` loss, and every witness's `cadmpeg check` report has zero check findings. Owner tests are `type322_form0_count_driven_boundary_follows_descriptor_list`, `type322_form0_zero_count_suppresses_generic_suffix_candidate`, `type322_form0_negative_count_suppresses_generic_suffix_candidate`, `type322_form0_wrong_typed_descriptor_keeps_count_boundary`, and `type322_form0_truncated_descriptor_list_suppresses_generic_suffix_candidate`.

Type 302 evidence: [IGES 5.3 §4.69](https://paulbourke.net/dataformats/iges/IGES.pdf) and public Open CASCADE [`IGESDefs_ToolAssociativityDef::ReadOwnParams` and `WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/master/src/DataExchange/TKDEIGES/IGESDefs/IGESDefs_ToolAssociativityDef.cxx) establish `K`, the per-class `BP`/`OR`/`N`/item-type sequence, and the repeated nested class width. The synthesized positive witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type302-form5001-positive.igs` has SHA-256 `e45152d26d1e6b86314ce86cc6d06a0a5f945746d8e24e57dd507018b1e3f287`; the zero-class-count witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type302-form5001-zero-class-count.igs` has SHA-256 `afe327a9dc90cfaab4aebe4898e219979962982d9a0d97c2d8d52d6c75cf466b`; the negative-class-count witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type302-form5001-negative-class-count.igs` has SHA-256 `4294405a32cc189e2032da678c38124991d6b7f8553ac0508380e424fe795ed5`; the zero-item-count witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type302-form5001-zero-item-count.igs` has SHA-256 `103bf99fbf1c2be76230b8f59dffed622f25a16482a3717772050f83551f57a6`; the wrong-item-count witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type302-form5001-wrong-item-count.igs` has SHA-256 `b5279fead8bc959822124ef74730eed0dee4a58df5f95d625428cb8c38d36fa4`; the wrong-item-type witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type302-form5001-wrong-item-type.igs` has SHA-256 `e423463186e7618f490b964bfca79446ba99ab242ecc37ab43b248ae7426cd34`; and the truncated-class witness `/home/pcurve/side2/tmp/freecad-l9/ph03-type302-form5001-truncated-class.igs` has SHA-256 `c6df729221d9e891892cb1e57fde32fe62f4cc80848a0d10ad2fe7318c3b8385`. All seven pass `cadmpeg inspect`. Before registration, generic recovery falsely links the zero-class, negative-class, zero-item, wrong-item-count, and wrong-item-type witnesses at pointer indexes 7, 7, 6, 8, and 8; the truncated witness has no complete candidate. After registration, the positive and wrong-item-type suffix associations resolve at pointer indexes 12 and 8; all malformed count/class witnesses have no suffix association. The wrong-item-type witness retains the expected `entity.not-projected` loss, and all seven `cadmpeg check` reports have zero check findings. Owner tests are `type302_form5001_count_driven_boundary_follows_nested_class_widths`, `type302_form5001_zero_class_count_suppresses_generic_suffix_candidate`, `type302_form5001_negative_class_count_suppresses_generic_suffix_candidate`, `type302_form5001_zero_item_count_suppresses_generic_suffix_candidate`, `type302_form5001_wrong_item_count_suppresses_generic_suffix_candidate`, `type302_form5001_wrong_item_type_keeps_count_boundary`, and `type302_form5001_truncated_class_suppresses_generic_suffix_candidate`.

Type 322 Forms 1/2 evidence: [IGES 5.3 §4.79](https://paulbourke.net/dataformats/iges/IGES.pdf) and public Open CASCADE [`IGESDefs_ToolAttributeDef::ReadOwnParams` and `WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/master/src/DataExchange/TKDEIGES/IGESDefs/IGESDefs_ToolAttributeDef.cxx) establish the repeated descriptor, default/zero `AVC`, Form 1 value stride, and Form 2 value/display-pointer stride. The synthesized Form 1 witnesses have SHA-256 values `659c8166f9933da3e1e8740d58fdb854ebc54d6aad1bf10a775a98bf0d12bcf3` (positive), `ef54b8b110ff589fc6cdcc195ffeb5c3bf050d0c0b6af9045915a13d808753ee` (zero counts), `8bba5cb55840e47f799682cabeeb1649ba892567d95c2f45a0983f5b98afe7c5` (wrong typed value), `bec39d3d7d8a53981c00370a5b2e776cbad4b2544b86d3fd3dfc692b3424c0b8` (wrong typed count), `54ce13d1408b96c6178057ce6d3535489d9630570ae7e94d0ed737c57120e758` (negative count), `c8821ed9271db3106205907e84625c209df4ffbd69c96f6dbd48b9169b78f720` (zero `NA`), and `3ba9fe256c0dc3b91d98206ff40cb25a8a8850403578a58c994eda60a1298cc2` (truncated value). The Form 2 witnesses have SHA-256 values `2fad6ac5cab21789035ae86957edf2ed27aa298c04c677618131fe047a159fc7` (positive), `9f4f37b9ccbff409a700935759a93ad9bccdec0830dc3df769414d74e9b3e77b` (zero counts), `a8b23c8d54f98d9eff84d2bc4654775f1f27d0cc9cf6cb083d04ed494020398c` (wrong typed value), `6a20cfd128612284a3bb47ef8ef53c35f4a1568e6546deb544a403a83e2c87c1` (wrong typed pointer), `7edc55d67d8580b5b3244d1d5a82fb6fece75a0de2bed1ccc463f44a4d10a15e` (wrong typed count), `fc1b4eafa6b45dbc48e4cbe46d9a65f2d16889637a8e0157b6b8873906bbf2c1` (negative count), and `456020738b20bc3a93f09ff5fa7fb87a23c38b25fc0a683131b3856817bcfb87` (truncated pointer). All fourteen pass fresh `cadmpeg inspect`, `dump`, and `check`; every check report has zero findings. Before registration, malformed AVC witnesses falsely recover suffixes at earlier tokens; after registration, valid Form 1 and Form 2 suffix associations resolve at pointer indexes 14 and 17 (count indexes 13 and 16), zero-count suffixes resolve at pointer index 11, and malformed count or slot witnesses have no suffix association. Wrong typed values keep those boundaries and retain `entity.not-projected`; the wrong Form 2 display pointer keeps the boundary and retains `graph.pointer-unresolved`. Owner tests are `type322_form1_count_driven_boundary_follows_value_widths`, `type322_form2_count_driven_boundary_follows_value_pointer_pairs`, `type322_forms12_zero_value_counts_keep_the_count_defined_boundary`, `type322_form1_wrong_typed_value_keeps_count_boundary`, `type322_form2_wrong_typed_value_keeps_count_boundary`, `type322_form2_wrong_typed_pointer_keeps_value_pair_boundary`, and `type322_forms12_invalid_value_width_suppresses_generic_suffix_candidate`.

Type 422 evidence: [IGES 5.3 §4.141](https://paulbourke.net/dataformats/iges/IGES.pdf) and public Open CASCADE [`IGESDefs_ToolAttributeTable::ReadOwnParams` and `WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/master/src/DataExchange/TKDEIGES/IGESDefs/IGESDefs_ToolAttributeTable.cxx) establish the negative Structure definition link, the Form 0 tuple, and the Form 1 row-major tuple width. The synthesized witnesses have SHA-256 `ba1c6329fcead3222d0236296c4fd4088990b3e1b63546f05f8908d2e4a17466` (valid unequal-width Forms 0/1), `81b7c58878db4106fd9057e49ccf57321fe695ccb413a4c894760f42ab1a76c9` (wrong typed values), `a0ecbd79a72c86def234185ce8174c68aaaa8037c13e5e3e0936d461f4b7f0fe` (zero-width definition), `8ef21b6cd5ae619831ae96fd1665301d9c0d1d615856e8680fda21466e3f6659` (unresolved definition), `1753ce7b90d4ed51a3ab985bfb32529a5c6cc618a096b9cdad9095d6312bb284` (wrong definition form), `942ef4727507af56f923bce928c385dd6d991e3b06f38105504d960fe3382ce5` (invalid definition `AVC`), `c30e4e333a8496a8862e7fc7800aa86dce9f8fe6ecf4e69ce86934ca9b976e1d` (malformed Form 1 row suffix), and `54384b9c0c7d21d0dda50249045c5521449d8ea7b8b0afa0ee7dd11e1ece353f` (wrong-typed row count). Before registration, the valid witnesses select pointer indexes 5, 9, and 4, while invalid-definition and malformed-row witnesses produce false generic links at indexes 2, 2, 5, and 3. After registration, the valid links remain at those definition-derived indexes; wrong typed values retain links at indexes 5 and 6; the invalid contexts have no inferred parameter suffix. All eight fresh `inspect`, `dump`, and `check` runs succeed with zero check findings. Owner tests are `type422_forms_use_referenced_definition_width_for_boundary`, `type422_wrong_typed_values_keep_definition_boundary`, `type422_zero_definition_width_keeps_boundary`, and `type422_invalid_context_suppresses_generic_suffix_candidate`.

Type 316 evidence: [IGES 5.3 §4.77](https://paulbourke.net/dataformats/iges/IGES.pdf) lists `NP` at Parameter Data index 1 and repeats the `(TYP, VAL, SF)` triple through index `1+3*NP`; the public Open CASCADE [`IGESDefs_ToolUnitsData::ReadOwnParams` and `WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/master/src/DataExchange/TKDEIGES/IGESDefs/IGESDefs_ToolUnitsData.cxx) reads/writes one type string, value string, and real scale per positive unit count. The synthesized witnesses are `ph03-type316-positive.igs` (`58ab941397344577f83ea5671392298209c1cfa4eeabddd9623c7a639dfec314`), `ph03-type316-ambiguous.igs` (`d7512ccabaf84bd991ba72a72c6dbd96be7e20b3f1668cdcff206110a66e21bf`), `ph03-type316-zero-count.igs` (`2a6519fd9b5c00f1b718095002e62730a0e8898d5d9037801f0e56d98ea13874`), `ph03-type316-negative-count.igs` (`f2346e3bb931f10ec19d3fd135137afd0cab3ddaeac78c2d9cf19ba8cef5e787`), `ph03-type316-wrong-typed-count.igs` (`349cd1424eabc048895fa9694fdbb9b6d13b2d636ad1e4a0931887c8efc578e9`), `ph03-type316-wrong-value.igs` (`d58bfbf3a86614539ea738163b8f78107dca7b55e291fad326cd5bf1c686e823`), and `ph03-type316-truncated.igs` (`343612f14207b0d02bf05c85ca2cc8c6e7bc519e15a4a75174a933356b0ac177`). All seven pass fresh `cadmpeg inspect`, `dump`, and `check` runs. Before registration, the ambiguous witness produced two complete suffix candidates and no link; the zero, negative, and wrong-typed count witnesses falsely recovered the three target pointers from token 2; the corrected positive witness was checked only after registration because its earlier harness version omitted the association-count token. After registration, the positive and ambiguous witnesses select the suffix at token 8, with association pointers at indexes 9, 10, and 11 in the ambiguous case; all malformed count/list witnesses have no inferred suffix. The wrong-value witness keeps the suffix at token 5 and pointer at index 6 while retaining `entity.not-projected`; every check report has zero findings. Owner tests are `type316_count_driven_boundary_follows_unit_triples`, `type316_entity_boundary_beats_two_valid_generic_suffixes`, `type316_invalid_count_or_truncated_units_suppresses_generic_suffix_candidate`, and `type316_wrong_typed_value_keeps_count_boundary`.

Type 402 Form 16 evidence: [IGES 5.3 §4.91](https://paulbourke.net/dataformats/iges/IGES.pdf) defines `NTR=1`, `N`, the transformation pointer, and the coplanar entity list. Public Open CASCADE [`IGESDraw_ToolPlanar::ReadOwnParams` and `WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/master/src/DataExchange/TKDEIGES/IGESDraw/IGESDraw_ToolPlanar.cxx) reads `NTR`, requires it to equal one, reads `N`, reads the optional Type 124 transformation, and reads exactly `N` entity pointers when positive; the writer emits the same sequence. The synthesized witnesses are positive `ph03-type402-form16-positive.igs` (`3a854c03df77cb381d97d22d072cf9d3d6916fc5d8c75ce7b9d32cc23b60cf38`), two entities `ph03-type402-form16-two-entities.igs` (`33e08eb1dabee052606b73f88efcc193aea6ef4acd6b03ab949898b0c18aab9c`), ambiguity `ph03-type402-form16-ambiguous.igs` (`224339756256bb8358333e6a428b8cccb2a130679970b7942ec2eb257ce6e2d1`), zero count `ph03-type402-form16-zero-count.igs` (`3d5536789987badf9ea87c7f05e2359eab900baa52e8d9f2dbcd53b343c1da15`), negative count `ph03-type402-form16-negative-count.igs` (`89c8c792a6a23ff31dc32317dbddf6851ae515b3c9fee1881b833d9861d0765f`), wrong-typed count `ph03-type402-form16-wrong-typed-count.igs` (`4e7ac5016798710a3ac6bec644b7e4668bb92ee180d8b1d5c50459837687e2b6`), wrong `NTR` `ph03-type402-form16-wrong-ntr.igs` (`13c18ae3cc9a105e8f9688c08a023daeb84a840c6960ebd4c541148946ebdddd`), wrong transformation `ph03-type402-form16-wrong-transform.igs` (`f60b02d340c67051339962fc409f6abd47b4870c38ba7d9e817e756c222d5624`), overflowing count `ph03-type402-form16-overflow-count.igs` (`a32340786b69f352f7eae36a86577b1a82c111ea4284fbe0b9e417c5bdbb4eea`), missing count `ph03-type402-form16-missing-count.igs` (`892a10a65aec7592c62a6858c5508451a134887bd0bf9b6b31e4ede56bc77042`), and truncated list `ph03-type402-form16-truncated-list.igs` (`09fb4a8f3e09cbc94c29a1bcbc2023b668c58f066bcb8bc1147de688ba8c568d`). All eleven pass fresh `cadmpeg inspect`, `dump`, and `check` runs. Before registration the ambiguity witness produced two complete target-valid suffixes; zero, negative, wrong-typed-count, wrong-`NTR`, overflowing, and truncated witnesses exposed false generic links, while missing count exposed none. After registration, the positive suffix begins at token 5 with its pointer at index 6, the two-entity suffix begins at token 6, and the ambiguity witness selects token 5 with pointers at indexes 6, 7, and 8; malformed count/list contexts have no association links. The wrong transformation witness keeps the suffix pointer at index 6 and retains the expected Type 124 reference loss. Every post-registration check report has zero findings. Owner tests are `type402_form16_count_driven_boundary_follows_entity_list`, `type402_form16_entity_boundary_beats_generic_suffix`, `type402_form16_invalid_primary_context_suppresses_generic_suffix_candidate`, `type402_form16_wrong_transform_keeps_count_boundary`, and `type402_form16_wrong_entity_pointer_keeps_count_boundary`.
Type 402 Form 21 evidence: [IGES 5.3 §4.95](https://paulbourke.net/dataformats/iges/IGES.pdf) defines `ND=1`, `NG`, the dimension tuple, the five-field geometry-entry stride, the ordered geometry list, the `DOF`/`DLF` meanings, and the two-arrowhead `NG=2` rule. Repository `entities/structure.rs:675-738` and `native.rs:2889-2931` already validate and project the same fields; `parameter.rs::specified_parameter_end` now registers the checked boundary `6+5*NG`. Corrected synthesized witnesses place the source Parameter Data record at byte offset `0x7e9`: `ps03-type402-form21-ng1-ambiguous.igs` (`e824ac9afded358357a2d4479c17056ce2168c72018c98f6db5d49d57c9e657f`), `ps03-type402-form21-ng2-boundary.igs` (`e4825d66ce893a909f9a73c1d43c6b5d2ec4030cc360892e408ca59e41e32633`), `ps03-type402-form21-wrong-pointer.igs` (`833b93611ed3e9edc73ab837c4a6cad3ddd26e4ba3e3f01c592bd113a3d24bc6`), `ps03-type402-form21-wrong-nd.igs` (`2b393c9d8cdf854d9c9426cf8436de8006d8426e5d340de76092e2632c3c997c`), `ps03-type402-form21-zero-ng.igs` (`4a7029b18089aab4096fbe30ff254308a4a9159628ee87f9ef621b631c747e80`), `ps03-type402-form21-wrong-ng-type.igs` (`a60b0ccadda219661d2c436fc30f59a1fa745bcda229abee42c3a289a7b80d58`), and `ps03-type402-form21-truncated.igs` (`e2738deb55b7fb751395ece0b35dd000cd0374065e89633707d0b833adf5f181`). All seven pass fresh per-file `cadmpeg inspect`, `dump`, and `check` runs. The registered NG=1 witness selects its trailing group at count token 11 and pointer token 12; NG=2 selects count token 16 and pointer token 17; the ambiguity pair is resolved without an ambiguity loss; the wrong-pointer witness retains the suffix and emits `entity.not-projected`; and wrong `ND`, zero/wrong-typed `NG`, and truncated-list records have no association link or ambiguity loss. Owner tests are `type402_form21_count_driven_boundary_follows_geometry_list`, `type402_form21_count_boundary_beats_target_valid_generic_suffix`, `type402_form21_wrong_geometry_pointer_keeps_count_boundary`, and `type402_form21_invalid_primary_context_suppresses_generic_suffix_candidate`.

Type 406 Form 5 is now settled. `NP=5` is at index 1; `WM`, `CC`, `EF`, `JF`, and `E` occupy indexes 2 through 6; and the first trailing count is token 7. A record may terminate before token 7, in which case unsupplied trailing numeric fields default to zero. An omitted `E` keeps its token slot when a suffix follows. CADIR requires `NP=5`, the declared field types and flag ranges, finite nonnegative `WM`, and a supplied finite `E` when `EF=2`; wrong typed values keep a complete table boundary, while a wrong `NP` or wrong-typed prefix suppresses generic recovery.

Type 510/514 evidence: the official [IGES 5.3 §§4.146–4.147](https://paulbourke.net/dataformats/iges/IGES.pdf) tables and public Open CASCADE [`IGESSolid_ToolFace::ReadOwnParams`/`WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/V7_8_1/src/IGESSolid/IGESSolid_ToolFace.cxx) and [`IGESSolid_ToolShell::ReadOwnParams`/`WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/V7_8_1/src/IGESSolid/IGESSolid_ToolShell.cxx) read and write the same count-driven lists. Repository `entities/brep.rs:467-567`, `writer.rs:1375-1445`, and `parameter.rs::specified_parameter_end` implement the corresponding boundaries. The controlled builder `/home/pcurve/side2/tmp/freecad-l9/ph03_type510_514_witnesses.py` places Type 510 Parameter Data at byte offset `0x6a5` and Type 514 at `0x798`; the rebuilt-CLI witnesses have SHA-256 values `bdaa9a4de592484c81e1a0206f161b03c3b8adc1b510294a4c2d892ccccbab94` (Type 510 N=1), `2475f09a5fd81c7cdc1764a7781f97080bc78cb9c63f15bcc0895d0b806bf4a0` (Type 510 N=2), `97ba29a26365a213b0b44f875a2b71819ba7b35a4faa6a7577ae54665755df14` (wrong `OF`), `993d94cf60850b4a6d265de1316f0788f2d838ff90547dd354d8b3bc1e3ce578` (wrong count type), `2b80cfe66142917458d42a205864d15b6fb77961fb537a684e2344cd94fe1893` (zero count), `ca2cb467beadce0f6fbcd992b66dc5312559b428544ccf62277ba796a0c74768` (truncated loop list), `38790bc43620f01b4a9036935818f8aea747da672a37a4ed29db9c1d2f4446fb` (Type 514 Form 1 N=1), `bc33923202fa453c60f3bc0345691f0564de98c49af5a88f4f8bff1636918c71` (Type 514 Form 2 N=1), `fb06fc1d1d675fd24441f08a9616e2437f2fdc9e20595c27a154864f1a4d9787` (Type 514 N=2), `a92e9fef02a525518b4ec0c20f19f9147e27561602271750f9d2291b022c943b` (wrong orientation), `d8a066d8d3c6bdd04632584ab4dbcbd161d3be3a3d712ff20813afe6feaa03df` (wrong count type), `2882ed0707db666665402f90ec5819b8798540404cf6f65dc324e0ea737573b5` (zero count), `778bb82c07cb6541025976b6562244ba82237bbccede01e61213548357f0a81c` (truncated pair), and `e1034d2784af5e952c51c7ff3234fbe9c02225f55c70a4dae3481d48475f1e51` (truncated list). Fresh per-file `cadmpeg inspect`, `dump`, `query`, and `check` runs pass. Valid and wrong-complete witnesses retain the expected suffix links at `(6,8)` and `(7,9)` for Type 510 and `(5,7)` and `(7,9)` for Type 514; malformed count/list witnesses retain no suffix links or ambiguity loss. Owner tests are `type510_count_driven_boundary_follows_one_and_two_loop_lists`, `type510_wrong_typed_outer_flag_keeps_count_boundary`, `type510_invalid_count_or_truncated_list_suppresses_generic_suffix_candidate`, `type514_count_driven_boundary_follows_form_and_face_lists`, `type514_wrong_typed_orientation_keeps_count_boundary`, and `type514_invalid_count_or_truncated_face_list_suppresses_generic_suffix_candidate`.

Type 216 Forms 0 through 2 are now settled. `DENOTE`, `DEARRW1`, `DEARRW2`, `DEWIT1`, and `DEWIT2` occupy indexes 1 through 5, so the first trailing count is token 6. The two witness slots use explicit zero for an absent witness and remain present in the fixed span. CADIR assigns the token-6 boundary whenever all five slots exist, retaining the suffix for wrong-typed or wrong-target primary values; a record shorter than the fixed boundary has no candidate there, and earlier pointer-shaped tokens are not reinterpreted. The official [IGES 5.3 §4.63](https://paulbourke.net/dataformats/iges/IGES.pdf), public Open CASCADE [`IGESDimen_ToolLinearDimension::ReadOwnParams` and `WriteOwnParams`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/V7_8_1/src/IGESDimen/IGESDimen_ToolLinearDimension.cxx), and [`IGESData_ParamReader::ReadEntity`](https://raw.githubusercontent.com/Open-Cascade-SAS/OCCT/V7_8_1/src/IGESData/IGESData_ParamReader.cxx) establish the five-reference sequence and nullable witness semantics. The controlled witnesses are `/home/pcurve/side2/tmp/freecad-l9/ph03-type216/ph03-type216-form0-valid.igs` (`be36bf65ecd0fd9533b1363bb310f65ff2583a9ab1a549c3efbf39ad817a4c54`), `ph03-type216-form1-valid.igs` (`ccf001fb57a04ef54a466038345684886035393e6485ebe8c90f952ff417a049`), `ph03-type216-form2-valid.igs` (`af19a47692fef79247d3133678f99c66415c1e5beac052a6630ab5e1e78ebd13`), `ph03-type216-wrong-witness-type.igs` (`525396fcdb4614d48854cf898dbde3dd1722f815a9d87ec29875dad582cc9231`), `ph03-type216-wrong-witness-pointer.igs` (`28ba27e1316333f01b7501e06f13ad262cc5abc67f3cc41a7d2e25c8aefd65fb`), `ph03-type216-truncated-primary.igs` (`6f8658167561e46fd10568ff53ec758192b45f83dd3cac9bef77198f0c18d4cc`), and `ph03-type216-omitted-witness-slot.igs` (`93290c5983425fce478304d16c8c484e8e6a688b1dca1dc93ea537e8464bdded`). Before registration the truncated witness selected the false association at parameter index 6; after registration the three valid forms and the complete malformed witnesses select the fixed boundary at token 6 and pointer index 7, while the truncated witness has no inferred suffix. All seven rebuilt service-profile inspect, dump, and check runs have zero check findings. Owner tests are `type216_forms_share_fixed_boundary`, `type216_complete_wrong_witness_keeps_fixed_boundary`, and `type216_truncated_primary_suppresses_generic_suffix_candidate`.

## 2. Global metadata

## 3. Directory fields, the reference graph, and the native arenas

## 4. Geometry carriers and tolerances

## 5. Surfaces and topology

### TP-01. The Global minimum resolution serves five unrelated roles

**Question.** Which topology and geometry decisions may use Global minimum resolution?

**Known.** `trimming.rs:59-74` derives a coordinate quantum from the largest coordinate and single-precision significance, then takes the maximum with Global minimum resolution. The result is used for ring closure, vertex merging, and stored edge and face tolerances.

**Need.** We need the source meaning of minimum resolution and separate rules for coordinate precision, curve-fit tolerance, topology sewing, and native tolerance fields.

**Note.** Closure audit 2026-08-10: reopened. Commit `6bb0de35f` supplied a formula and synthetic boundary tests, but the use of one value for these five roles is not established from the specification or witness files.

### TP-03. Declared surface parameter subranges are discarded with no loss

**Question.** Must a Type 141/142 surface-boundary use retain its declared surface parameter subrange?

**Known.** `trimming.rs` projects model curves and uses the neutral edge range; the declared surface parameter bounds are not retained in the native-to-IR path. The current documentation calls this intentional.

**Need.** We need the source semantics of the parameter subrange and a decision whether projection may discard it, must preserve it, or must report a loss.

**Note.** Closure audit 2026-08-10: reopened. Commit `8ddb25d46` added bounds handling and tests, but it did not establish whether the bounds are authoritative for the boundary use.

### TP-04. The Type 140 offset sign uses a per-kind representative normal

**Question.** Which normal determines the sign of a Type 140 offset indicator?

**Known.** `surfaces.rs:206-225` uses the support surface's bounds midpoint when finite and otherwise `(0, 0)` as the representative parameter, then evaluates a normal. The current documentation states this rule.

**Need.** We need the source rule for the offset sign and a representative point that is valid for bounded, unbounded, and varying-normal surfaces.

**Note.** Closure audit 2026-08-10: reopened. Commit `23554c501` changed implementation, documentation, and fixtures together. Neither midpoint selection nor the `(0, 0)` fallback is established from the specification or witness files.

### TP-06. Type 180 Form 1 requires a direct Type 186 operand

**Question.** Does a Type 180 Form 1 Boolean tree accept a Type 186 solid directly, or through a complete operand subtree?

**Known.** `brep.rs` recursively checks Type 180 and Type 430 references and accepts a Form 1 operand when the complete referenced subtree contains a Type 186. The current fixtures use project-generated nested trees.

**Need.** We need the operand rule for Boolean subtrees and the treatment of nested or malformed operands from the IGES specification or exporter-authored witness files.

**Note.** Closure audit 2026-08-10: reopened. Commit `34861ac75` made the recursive interpretation internally consistent, but the source rule remains unverified. The current documentation promotes the recursive choice to a settled format fact.

### TP-09. A model-curve pointer selects the first neutral edge

**Question.** How does a Type 141/142 model-curve pointer select a neutral edge when multiple edges use the same curve carrier?

**Known.** `trimming.rs:207-212` builds `edges_by_curve` with `or_insert`, and `trimming.rs:564-565` uses that first edge. `brep.rs:858-863`, `composite.rs:426-430` and `535-540`, `offsets.rs:222-226`, and `csg.rs:30-35` also choose the first matching edge. `Edge` permits repeated `CurveId` values with distinct vertices and parameter ranges. Type 141/142 carry curve entity pointers, not edge occurrence identities.

**Need.** We need an ownership invariant or a source rule that makes the curve-to-edge relation unique, or a resolution path that verifies each candidate's range and endpoints. A wrong choice transfers the wrong range or endpoints to a boundary, B-rep, offset, composite, or sweep.

**Note.** New item filed after the hostile sweep on 2026-08-10. With two edge occurrences sharing one curve but having different spans, storage order decides; no candidate comparison or rejection exists.

## 6. Product structure, annotation, and presentation

### PS-01. Parameter defaults are honored at selected token indices only

**Question.** Which parameter fields may be omitted, and what defaults do they receive?

**Known.** `drawing.rs` applies defaults at selected token indices for several annotation and drawing records. The current defaults are documented in the codec but are not all derived from a source table.

**Need.** We need the optionality and default for each affected field, with omitted and malformed tokens distinguished.

**Note.** Closure audit 2026-08-10: reopened. Commit `c486ba66d` made the selected defaults explicit and added fixtures, but it did not establish the complete field table from the IGES specification.

### PS-02. The same text-box metric has two different bounds

**Question.** What bounds apply to the Type 212/213 text-box metrics?

**Known.** `drawing.rs` applies distinct bounds to the two record forms, including nonnegative checks for Type 312 dimensions. The current documentation records these bounds.

**Need.** We need the field definitions and bounds for each form from the IGES specification or exporter-authored witness files.

**Note.** Closure audit 2026-08-10: reopened. Commit `8d2479c8b` changed the checks and the documentation together; the test fixtures do not establish that the bounds are format rules.

### PS-04. Enumerated value tables exist only in the source

**Question.** What are the complete enumerated tables for the supported drawing and presentation fields?

**Known.** `drawing.rs` and `presentation.rs` contain the accepted values. The current tests exercise selected values and the documentation repeats the implementation tables.

**Need.** We need the enumerated tables from the format source, including reserved and invalid values, and a rule for values outside each table.

**Note.** Closure audit 2026-08-10: reopened. Commit `4c91d071e` made the source tables explicit but did not cite the specification's tables.

### PS-05. Type 420 accepts a wrong-typed type flag and Type 320 does not

**Question.** Does the Type 420 type flag have a default, and may a non-integer token satisfy it?

**Known.** `structure.rs:1959` accepts a missing or non-integer token through `is_none_or`, while the corresponding Type 320 field rejects it. The documentation does not settle defaulting or token type.

**Need.** We need the default, token type, and allowed values for both fields, with one consistent malformed-token policy.

**Note.** Closure audit 2026-08-10: reopened. Commit `d11d59213` reconciled local behavior but did not establish the format rule.

## 7. Write path

### WR-01. An unclassified loop is written as an inner loop

**Question.** How does the writer encode a loop whose source does not classify it as outer or inner?

**Known.** `writer.rs:2321-2364` orders loop roles and returns the first loop that is not `Inner`. The Type 510 path uses `face_outer_loop` at `writer.rs:1375-1382`; the Type 144 path also promotes the first unclassified loop. `LoopBoundaryRole::Unspecified` is a real IR state.

**Need.** We need the encoding for an unclassified loop, or a refusal when no valid outer-loop representation exists. The choice must not depend on list order.

**Note.** Closure audit 2026-08-10: reopened. Commit `ed211eb05` documented the first-unclassified policy and added generated fixtures, but it cited neither the specification nor a witness file. Two unclassified loops with different containment or orientation can produce the wrong outer boundary.

### WR-02. The declared Global minimum resolution is tighter than the writer's own acceptance bound

**Question.** What minimum resolution must a generated file declare?

**Known.** `writer.rs:3232-3245` derives a generated value from model tolerances and a floor, while `writer.rs:3037-3040` accepts some pcurve gaps against a larger effective floor. The current documentation presents the generated resolution as the settled writer policy.

**Need.** We need the declared resolution derived from the tolerances the writer accepts, and a round-trip test that proves the reader and writer use compatible bounds.

**Note.** Closure audit 2026-08-10: reopened. Commit `17c19bdcb` changed the generated-resolution policy and fixtures together. The code needs an evidence-backed relation between accepted gaps and the declared value.

### WR-03. The Type 186 outer shell is the first shell by position

**Question.** Which shell of a region is the exterior shell?

**Known.** `cadmpeg-ir/src/topology.rs:97-110` documents ordered shells and `Region::exterior_shell()` returns `self.shells.first()`. `writer.rs:1442-1457` uses that accessor for Type 186. No validation proves containment, orientation, or producer ordering.

**Need.** We need the exterior shell identified from geometry, an explicit source role, or a validated IR invariant. List position alone must not invert a solid.

**Note.** Closure audit 2026-08-10: reopened. Commit `46a71f68c` introduced the IR wording, accessor, writer use, and tests together. This is promotion to an IR invariant, not evidence that all producers supply the order.

### WR-05. The target version changes one digit only

**Question.** What does `IgesWriteOptions::version` constrain?

**Known.** `writer.rs:295-310` rejects one unsupported Type 514 form for older targets, while `version.global_flag()` changes the Global version field. Other emitted entity families are not checked against the selected target version.

**Need.** We need the entity and form set of each target version, and a refusal when the model requires an entity the target does not define.

**Note.** Closure audit 2026-08-10: reopened. Commit `1a6b988e7` added a target entity check, but its coverage and version matrix are not independently established.

### WR-06. The analytic surface family is fixed with no fallback

**Question.** Which IGES surface entity should a generated file use for each analytic surface?

**Known.** `writer.rs` maps planes, cylinders, cones, spheres, and tori to Types 190, 192, 194, 196, and 198, and rejects unsupported native forms. The writer does not record why this family is preferred over Type 108/120/128 alternatives.

**Need.** We need the encoding choice and its interoperability evidence, plus a loss or refusal when the selected family is not supported by the target profile.

**Note.** Closure audit 2026-08-10: reopened. Commit `f4a07d64b` made the analytic-family choice explicit but did not establish portability or a target-profile rule.

### WR-07. Orthonormality gates refuse foreign frames instead of repairing them

**Question.** What frame perturbation must the writer accept?

**Known.** `writer.rs` uses `FRAME_REPAIR_DOT_LIMIT = 1e-6` in `orthonormal_pair`; the current writer repairs only within that project-selected bound and rejects larger residuals.

**Need.** We need a source or producer-derived bound for representational frame noise, and a rule for repair versus refusal.

**Note.** Closure audit 2026-08-10: reopened. Commit `dc2bd137a` added the repair threshold and synthetic boundary tests, but the bound is not established from the specification or witness files.

### WR-10. Fixed protocol constants with no IR source

**Question.** What are the correct `PREF`, creation-method, and hierarchy values for generated records?

**Known.** `writer.rs` emits fixed values for Type 141/142 preferences, Type 142 creation method, and Type 504 hierarchy. The values are justified by the current neutral IR and writer behavior, not by a complete source mapping.

**Need.** We need the correct value for each field and independent evidence for the Type 504 hierarchy difference.

**Note.** Closure audit 2026-08-10: reopened. Commit `82c13da5a` recorded protocol constants as settled without external format or producer evidence.

## 8. Evidence

### EV-02. The independent-application gate cannot detect wrong geometry

**Question.** What does FreeCAD acceptance prove about a generated file?

**Known.** `scripts/verify-iges-freecad.py` imports each file and refuses an import that gives no object or whose shapes are null or invalid (`:37-50`). It counts solids and faces and asserts nothing about them. A file with the wrong units, a mirrored surface, an inverted solid (WR-03), or an unbounded face (WR-01) imports as a valid shape and passes.

**Note.** The script is wired into no CI job and no test, and it needs a manual environment. No result artifact is committed, so no run is on record. The script globs `*.igs` only (`:68`). The CLI accepts and writes both `.igs` and `.iges` (`crates/cadmpeg/src/main.rs:168`), so a directory of `.iges` output is silently outside the check.

**Need.** The P0 gate above requires independent native-application acceptance. We need the acceptance criterion to compare geometry with the intended model, the glob to cover both extensions, and each run recorded.

### EV-03. The fixture builders and the decoder share one author

**Question.** Which decoder rules do the fixtures actually test?

**Known.** `iges-fixture-charter.md` states that builders serialize the rules in `iges.md`. A builder therefore writes the byte pattern that the decoder expects. Where a decoder rule is a guess, the builder embodies the same guess, and the test passes for both.

**Note.** GE-01 is the demonstrated case. Commit `f20d17e65` set `TRANSFORM_TOLERANCE` and authored the fixture that justifies it in the same commit, perturbed to 5e-11 against a threshold of 1e-10.

**Need.** We need each tolerance and default in sections 2 through 5 traced to evidence outside this repository, or marked as a project convention in `iges.md` rather than as a format rule.
