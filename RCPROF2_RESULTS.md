RailCore profile selection results

Branch: `f3d-railcore-profiles-3`. HEAD: `c0043d8db15832021ba2a0190b34fd1e5784be1e`. CADIR v11. `cadmpeg-ir` is unchanged.

The actual branch baseline retains all 29 requested selections as native. The change resolves 16; 13 remain native. Mapping uses the source Design, feature name, and occurrence of that name in timeline order. Dutchman design-wide ordinals label the requested rows.

Each entry joins its decoded identity pieces to the transition-certified boundary. Coincident lines and circles join through a positive-length shared interval. Each changed feature charges `feature.profile-coincident-boundary-assumed`. The cap, extent, and lateral-face certificates remain required. Eight independent source Designs support the reading; repeated insertions and revisions retain separate row labels.

| Assembly | Dutchman ordinal | Design | Feature | Cause | Result |
|---|---:|---|---|---|---|
| 300zl | 664 | Door | Extrude 1 | DR-654: stored line outside closed neutral loops | Native |
| 300zl | 737 | Right_Panel | Extrude 2 | DR-655: circle identity 392 has no curve record | Native |
| 300zl | 489 | PSU_Vent_Cover | Extrude 1 | DR-654: coincident boundary identities | Resolved |
| 300zl | 536 | Fan_Cover | Extrude 1 | DR-654: partial lines outside closed neutral loops | Native |
| 300zl | 856 | 300ZL Bed | Extrude 3 | DR-654: stored lines outside closed neutral loops | Native |
| 300zl | 360 | Y_Endstop_Mount | Extrude 1 | Split identities; coincident boundary pieces | Resolved |
| 300zl | 364 | E3DV6_Groove_Mount | Extrude 1 | Split identities; coincident boundary pieces | Resolved |
| 300zl | 544 | IR Probe Mount | Extrude 1 | Unbound profile member (DR-740) | Native |
| 300zl | 332 | DuetWifi v2 | Extrude 1 | DR-654: coincident boundary identities | Resolved |
| 300zlt | 335 | 300ZLT Back Panel | Extrude 1 | Loop / arrangement boundary identity mismatch | Resolved |
| 300zlt | 339 | 300ZLT Left_Panel | Extrude 1 | Loop / arrangement boundary identity mismatch | Resolved |
| 300zlt | 359 | 300ZLT Front Panel | Extrude 1 | Loop / arrangement boundary identity mismatch | Resolved |
| 300zlt | 370 | Right_Panel | Extrude 2 | DR-655: circle identity 392 has no curve record | Native |
| 300zlt | 433 | 300ZLT Bearing Block | Extrude 2 | Loop / arrangement boundary identity mismatch | Resolved |
| 300zlt | 456 | 300ZL Bed | Extrude 3 | DR-654: stored lines outside closed neutral loops | Native |
| 300zlt | 724 | Fan_Cover | Extrude 1 | DR-654: partial lines outside closed neutral loops | Native |
| 300zlt | 809 | PSU_Vent_Cover | Extrude 1 | DR-654: coincident boundary identities | Resolved |
| 300zlt | 283 | IR Probe Mount | Extrude 1 | Unbound profile member (DR-740) | Native |
| 300zlt | 602 | E3DV6_Groove_Mount | Extrude 1 | Split identities; coincident boundary pieces | Resolved |
| 300zlt | 859 | Y_Endstop_Mount | Extrude 1 | Split identities; coincident boundary pieces | Resolved |
| 300zlt | 840 | DuetWifi v2 | Extrude 1 | DR-654: coincident boundary identities | Resolved |
| zlv1 | 445 | DuetWifi v2 | Extrude 1 | DR-654: coincident boundary identities | Resolved |
| zlv1 | 634 | PSU_Vent_Cover | Extrude 1 | DR-654: coincident boundary identities | Resolved |
| zlv1 | 204 | E3DV6_Groove_Mount | Extrude 1 | Split identities; coincident boundary pieces | Resolved |
| zlv1 | 244 | Y_Endstop_Mount | Extrude 1 | Split identities; coincident boundary pieces | Resolved |
| zlv1 | 254 | IR Probe Mount | Extrude 1 | Unbound profile member (DR-740) | Native |
| mini | 11 | RCM_Front Panel PS | Extrude 6 | Unbound group 3229 members 3232, 3236, 3240, 3244 (DR-740) | Native |
| mini | 30 | RCM_RightPanel | Extrude 2 | Sketch_18641 absent | Native |
| mini | 222 | Duct_Printed_Mount_Adapter (From FS Kit) | Extrude 1 | Unbound group 204 member 214 (DR-740) | Native |


The source-part batch checks 26 source versions. Every check returns zero. E3D Extrude 2 also changes from native to Sketch regions in all three assemblies.

`cadmpeg diff --geometry` agrees for every changed source except Bearing Block's whole-design comparison. Its changed target body has equal area 12871.973406097126 mm², volume 25232.20320651837 mm³, centroid, bounds, closure, and orientation. That whole-design comparison is inconclusive because an unrelated body is unmeasured and occurrence placement is unavailable. Door has an inconclusive comparison and unchanged profiles and geometry arenas. Mini's complete profile set is unchanged.

Final planar-face measurements from `cadmpeg measure`:

| Design | Extrude transition | Final caps | Area (mm²) |
|---|---|---|---|
| PSU Vent Cover | 16 from 11; blind 1.5 | face 411 at z=15.4; face 459 at z=16.9 | 10345.487088112153; 9869.487080959596 |
| Fan Cover | 9 from 2; blind 1.5 | face 194 at z=0; face 65 at z=1.5 | 1526.0350021529198; 1354.03500215292 |
| DuetWifi v2 | 8 from 7; blind 15 | face 155 at z=15; face 87 at z=0 | 2268.29115279534; 12340.582305590675 |

Later Extrudes and chamfers change final cap areas. The selection certificate uses the Extrude's historical caps and inserted sides. Final measurements check the resulting body.

Checks:

- `cargo test -q -p cadmpeg-codec-f3d stored_loop_cap`: 6 passed.
- `cargo test -q -p cadmpeg-codec-f3d`: 5013 passed, 3 ignored; 2 documentation tests passed.
- `cargo clippy -q -p cadmpeg-codec-f3d --all-targets -- -D warnings`: passed.
- `cargo fmt -p cadmpeg-codec-f3d`: passed; all six changed Rust files returned with `--output`.
- Source policy, documentation anchors, open-item IDs, and `git diff --check`: passed.

DR-740 records unbound profile members. DR-654 retains the interior-spur question. DR-655 retains missing curve records.

Public sweep: all 174 files complete with zero timeouts or execution failures. The supplied main baseline is `main-d1a670528c`. Both runs hold 3801 bodies, 2471 features, and 3 model errors. Native Extrude selections decrease from 273 to 270; resolved selections increase from 381 to 384.

| Changed file vs supplied main | Profile changes | Cause |
|---|---|---|
| `Voron_Trident_xy_joints.f3d` | scopes 2168355, 3841063, 4296402: `sketch_selection` to `sketch_regions` | Native selections within known Sketches resolve to bounded regions. All three were already resolved at the lane's original HEAD. |

This patch adds zero profile changes on the public set. The original CADIR v11 branch binary and the patched release binary produce identical neutral arenas and source digests for the changed file. Its geometry comparison has 77 matched body pairs with zero disagreeing quantities and 7 unmeasured bodies on each side. Occurrence placement and unavailable measurements make the whole-document verdict inconclusive.

The 3 model errors occur in `Voron_Trident_ptfe_holder.f3d` (1) and `Voron_Trident_rear_vertext_upper_brace.f3d` (2), with equal counts on both runs.

Mini's complete model check returns zero errors. The release CLI build passes. Its SHA-256 is `f7334d827a967a516a41ec6f235f7421cba4b61e5adc76bd2fa4f15fa05444ce`. Source-part proofs use the debug CLI with SHA-256 `43b19d8e0474bba2c6854372b73b16dc45f48c0a0674e656e7a1f970b03ef220`; both builds use the same source.

Generated scripts, binaries, and detailed evidence are retained under `.scratch-rcprof2/`.

The standing lane rule forbids Git metadata writes. Changes remain unstaged. `COMMIT_MSG.txt` contains the owner's commit message. Commit, push, and fork CI dispatch are blocked by that rule and the sandbox. CI run ID: none. The full workspace suite remains for CI.
