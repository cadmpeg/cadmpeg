# SolidWorks `.SLDPRT` + `.STEP` AP214 Corpus

This directory contains **56 paired specimens** (`112` files total: each `<stem>.SLDPRT` paired with a ground-truth `<stem>.STEP` AP214 export, normalized to LF line endings). Every file is authored from a blank/default part template in **SolidWorks 2021 SP5.1** and dedicated to the public domain under **CC0-1.0**.

Unless noted otherwise (`units_*`), parts use millimeter-gram-second (`MMGS`) units and coordinates are stated in millimeters (`mm`), areas in `mm²`, and volumes in `mm³`.

## Open-Item Coverage Index

| Open Item | Focus | Specimens |
| :--- | :--- | :--- |
| `DI-15` | Up-to-vertex endpoint selector | `extrude_up_to_vertex_low`, `extrude_up_to_vertex_high` |
| `DI-16` | Cut and two-direction extrude travel sense | `Pair1/plane_front_*`, `cut_blind_*`, `extrude_two_dir_unequal`, `cut_two_dir_unequal` |
| `DI-17` | Second-direction Through All / codes | `extrude_two_dir_through_all` |
| `DI-23` | Combine Subtract body reconciliation | `combine_subtract` |
| `DI-24` | Delete-body reconciliation | `delete_body_one_of_two` |
| `DI-28`, `DI-30` | Sweep reference children & circular sweep Merge Boolean | `sweep_circular_merge_on`, `sweep_circular_merge_off` |
| `DI-31` | Neutral-plane Draft direction reversal | `draft_neutral_inward`, `draft_neutral_reversed` |
| `DI-32` | Linear pattern directions & spacing units | `pattern_linear_two_dir` |
| `DI-36` | SurfaceCut retained/discarded side | `surface_cut_side_a`, `surface_cut_side_b` |
| `DI-39` | Temporary-axis & explicit reference-axis ownership | `revolve_temp_axis_90_*`, `revolve_ref_axis_90` |
| `DI-41` | Linear & two-seed mirror pattern seed selection | `pattern_linear_two_dir`, `pattern_mirror_two_seeds` |
| `DI-43` | Equation Manager trig angular units (Degrees vs. Radians) | `equations_trig_deg`, `equations_trig_rad` |
| `DI-68` | Extrusion start-offset end-spec ownership | `Pair2/extrude_start_offset_*` |
| `DI-70`, `DI-71` | Sketch handles, concentric circles, fillets, slots & ellipses | `Pair3/*`, `Pair4/sketch_arcs_cw_ccw`, `sketch_partial_ellipse`, `sketch_slot_and_fillet_arcs`, `sketch_circle_dim_radial_vs_center` |
| `DI-72` | Custom length unit codes (`cm`, `in`, `ft-in`, `mil`) | `units_cm`, `units_in`, `units_ft_in`, `units_mil` |
| `DI-73`, `DI-80` | Sketch plane binding & displaced-face marker coordinates | `revolve_offset_plane_90`, `sketch_on_displaced_box_faces` |
| `DI-77`, `EV-32` | Curve record `+17` values & state `0` construction loops | `sketch_construction_outer_loop` |
| `DI-81` | Offset-plane `u_axis` with inverted normal | `plane_offset_pos`, `plane_offset_flip_normal` |
| `DI-82` | Angled plane & derived offset plane `u_axis` | `plane_angled_30`, `plane_offset_from_angled_30` |
| `DI-83` | `sgCircleDim` circles vs. concentric construction circles | `sketch_circle_dim_radial_vs_center` |
| `DI-85`, `DI-86` | Offset-plane revolve axis & `moLineRef_w` prefix forms | `revolve_ref_axis_90`, `revolve_temp_axis_90_*`, `revolve_offset_plane_90` |
| `EV-25` | Partial revolution turn sense & two-dir revolves | `Pair5/*`, `revolve_cut_90_*`, `revolve_temp_axis_90_*`, `revolve_two_dir_30_60` |
| `EV-27`, `GC-11` | Sketch curve selector bits & compact curve markers | `Pair4/sketch_arcs_cw_ccw`, `sketch_slot_and_fillet_arcs` |
| `EV-30` | Elliptical arcs & B-spline sketch curves | `sketch_partial_ellipse`, `sketch_style_spline` |
| `EV-35` | Sequential creation/modification/deletion edit logs | `edit_log_step1_base`, `edit_log_step2_added_fillet`, `edit_log_step3_deleted_fillet` |
| `SF-01` | Principal Top and Right plane frame placement | `Pair1/plane_right_blind`, `Pair1/plane_top_blind` |
| `SF-03` | Sketch normal sign on offset & angled planes | `plane_offset_pos`, `plane_offset_flip_normal`, `plane_angled_30`, `plane_offset_from_angled_30` |

---

## 1. Initial Plane, Start-Offset, Sketch Curve & Revolve Pairs (`Pair1`–`Pair5`)

These 13 baseline pairs establish reference-plane polarity, start-offset extrusion direction, sketch curve forms, and partial 90° revolution turn sense. Metrics below are from paired `.SLDPRT` / `.STEP` measurement (`cadmpeg measure` and `cadmpeg diff --geometry`):

### `Pair1` — Principal Planes & Blind Direction (`DI-16`, `SF-01`)
- `Pair1/plane_front_blind`: `20 x 10` mm rectangle on `Front` plane extruded `+10` mm along profile normal. (`bodies=1`, `faces=6`, `edges=12`, `area=1000.0` mm², `vol=2000.0` mm³, `centroid=[10.0, 5.0, 5.0]` mm, `bbox=[0.0, 0.0, 0.0]..[20.0, 10.0, 10.0]` mm)
- `Pair1/plane_front_flip`: `20 x 10` mm rectangle on `Front` plane extruded `10` mm with Reverse Direction. (`bodies=1`, `faces=6`, `edges=12`, `area=1000.0` mm², `vol=2000.0` mm³, `centroid=[10.0, 5.0, -5.0]` mm, `bbox=[0.0, 0.0, -10.0]..[20.0, 10.0, 0.0]` mm)
- `Pair1/plane_right_blind`: `20 x 10` mm rectangle on `Right` plane extruded `10` mm along `+X`. (`bodies=1`, `faces=6`, `edges=12`, `area=1000.0` mm², `vol=2000.0` mm³, `centroid=[5.0, 5.0, -10.0]` mm, `bbox=[0.0, 0.0, -20.0]..[10.0, 10.0, 0.0]` mm)
- `Pair1/plane_top_blind`: `20 x 10` mm rectangle on `Top` plane extruded `10` mm along `+Y`. (`bodies=1`, `faces=6`, `edges=12`, `area=1000.0` mm², `vol=2000.0` mm³, `centroid=[10.0, 5.0, -5.0]` mm, `bbox=[0.0, 0.0, -10.0]..[20.0, 10.0, 0.0]` mm)

### `Pair2` — Extrude Start Offset (`DI-68`)
- `Pair2/extrude_start_offset_pos`: `20 x 10` mm rectangle on `Front` plane, blind extrude `10` mm from `+5` mm offset start. (`bodies=1`, `faces=6`, `edges=12`, `area=1000.0` mm², `vol=2000.0` mm³, `centroid=[10.0, 5.0, 10.0]` mm, `bbox=[0.0, 0.0, 5.0]..[20.0, 10.0, 15.0]` mm)
- `Pair2/extrude_start_offset_flip`: `20 x 10` mm rectangle on `Front` plane, blind extrude `10` mm from flipped `5` mm offset start. (`bodies=1`, `faces=6`, `edges=12`, `area=1000.0` mm², `vol=2000.0` mm³, `centroid=[10.0, 5.0, 0.0]` mm, `bbox=[0.0, 0.0, -5.0]..[20.0, 10.0, 5.0]` mm)

### `Pair3` — Concentric Circles & Dimension Ownership (`DI-70`, `DI-71`)
- `Pair3/sketch_concentric_one_dim`: Concentric circles (`r=5` mm, `r=10` mm) with only inner circle dimensioned, blind extrude `10` mm. (`bodies=1`, `faces=4`, `edges=6`, `area=1413.7167` mm², `vol=2356.1945` mm³, `centroid=[0.0, 0.0, 5.0]` mm, `bbox=[-10.0, -10.0, 0.0]..[10.0, 10.0, 10.0]` mm)
- `Pair3/sketch_concentric_ring`: Concentric circles (`r=5` mm, `r=10` mm) with both circles dimensioned, blind extrude `10` mm. (`bodies=1`, `faces=4`, `edges=6`, `area=1413.7167` mm², `vol=2356.1945` mm³, `centroid=[0.0, 0.0, 5.0]` mm, `bbox=[-10.0, -10.0, 0.0]..[10.0, 10.0, 10.0]` mm)

### `Pair4` — Arcs, Ellipses & Parabolas (`EV-27`, `GC-11`)
- `Pair4/sketch_arcs_cw_ccw`: Closed profile with CCW arc, CW `0xffff` arc (`sldprt.md` §2.3), and interior full circle, blind extrude `5` mm. (`bodies=1`, `faces=7`, `edges=15`, `area=791.3223` mm², `vol=1151.0565` mm³, `centroid=[12.2646, 4.8245, 2.5]` mm, `bbox=[0.0, 0.0, 0.0]..[25.0, 10.0, 5.0]` mm)
- `Pair4/sketch_ellipse`: Full sketch ellipse, blind extrude `5` mm. (`bodies=1`, `faces=3`, `edges=2`, `bbox=[15.0, 12.5, -0.005]..[25.0, 17.5, 5.005]` mm; `cadmpeg diff --geometry` reports `body_count`, `bbox`, `closed`, `orientation.consistent`, and `occurrences` agreeing, with `area`/`volume`/`centroid` `compare.unavailable` due to differing STEP face counts)
- `Pair4/sketch_parabola`: Closed profile bounded by a trimmed sketch parabola and line, blind extrude `5` mm. (`bodies=1`, `faces=4`, `edges=6`, `area=348.1127` mm², `vol=333.3333` mm³, `centroid=[15.0, 8.0, 2.5]` mm, `bbox=[5.0, 5.0, 0.0]..[25.0, 10.0, 5.0]` mm)

### `Pair5` — Partial 90° Revolutions (`EV-25`)
- `Pair5/revolve_90_default`: `10 x 10` mm rectangle revolved `90°` in default direction about Y axis. (`bodies=1`, `faces=6`, `edges=12`, `area=1142.4778` mm², `vol=2356.1945` mm³, `centroid=[9.903, 5.0, -9.903]` mm, `bbox=[0.0, 0.0, -20.0]..[20.0, 10.0, 0.0]` mm)
- `Pair5/revolve_90_flipped`: `10 x 10` mm rectangle revolved `90°` with Reverse Direction about Y axis. (`bodies=1`, `faces=6`, `edges=12`, `area=1142.4778` mm², `vol=2356.1945` mm³, `centroid=[9.903, 5.0, 9.903]` mm, `bbox=[0.0, 0.0, 0.0]..[20.0, 10.0, 20.0]` mm)

---

## 2. Cuts, Two-Direction Extrusions & Up-To-Vertex (`DI-15`, `DI-16`, `DI-17`)

Each specimen was built via the SolidWorks COM API with body counts, face/edge counts, volumes, and bounding boxes queried before saving:

- `cut_blind_default`: `40 x 40 x 20` mm block (`z -10..10`), `10 x 10` mm square on `Front` plane (`z=0`) cut `5` mm blind (`z -5..0`) (`DI-16`). (`bodies=1`, `faces=10`, `edges=24`, `vol=31500.0` mm³, `bbox=[0.0, 0.0, -10.0]..[40.0, 40.0, 10.0]` mm)
- `cut_blind_flip`: `40 x 40 x 20` mm block (`z -10..10`), `10 x 10` mm square on `Front` plane (`z=0`) cut `5` mm blind with cut direction reversed (`z 0..5`) (`DI-16`). (`bodies=1`, `faces=10`, `edges=24`, `vol=31500.0` mm³, `bbox=[0.0, 0.0, -10.0]..[40.0, 40.0, 10.0]` mm)
- `extrude_two_dir_unequal`: `20 x 10` mm rectangle on `Front` plane, two-direction blind extrude `D1=5` mm, `D2=12` mm (`DI-16`). (`bodies=1`, `faces=6`, `edges=12`, `vol=3400.0` mm³, `bbox=[0.0, 0.0, -12.0]..[20.0, 10.0, 5.0]` mm)
- `cut_two_dir_unequal`: `40 x 40 x 30` mm block (`z -15..15`), `10 x 10` mm corner notch on `Front` plane `z=0` cut two-direction blind `D1=5` mm, `D2=12` mm (`DI-16`). (`bodies=1`, `faces=10`, `edges=24`, `vol=46300.0` mm³, `bbox=[0.0, 0.0, -15.0]..[40.0, 40.0, 15.0]` mm)
- `extrude_two_dir_through_all`: C-bracket (gap `y 5..20`), boss on top leg `y=25` with `D1=Blind 5` mm and `D2=Through All` bridging to bottom leg (`DI-17`). (`bodies=1`, `faces=19`, `edges=48`, `vol=13000.0` mm³, `bbox=[0.0, 0.0, 0.0]..[40.0, 30.0, 20.0]` mm)
- `extrude_up_to_vertex_low`: `8 x 8` mm boss on `Front` plane (`x 12..20`) extruded Up To Vertex `(10, 10, 10)` on step edge (`DI-15`). (`bodies=2`, `faces=14`, `edges=30`, `vol=3840.0` mm³, `bbox=[0.0, 0.0, 0.0]..[20.0, 20.0, 22.0]` mm)
- `extrude_up_to_vertex_high`: `8 x 8` mm boss on `Front` plane (`x 12..20`) extruded Up To Vertex `(10, 10, 22)` on step edge (`DI-15`). (`bodies=2`, `faces=14`, `edges=30`, `vol=4608.0` mm³, `bbox=[0.0, 0.0, 0.0]..[20.0, 20.0, 22.0]` mm)

---

## 3. Revolve Cuts, Axes & Two-Direction Revolves (`EV-25`, `DI-39`, `DI-73`, `DI-85`, `DI-86`)

- `revolve_cut_90_default`: `40 x 40 x 40` mm cube (`z -20..20`), `15 x 20` mm `Front`-plane rectangle revolve-cut `90°` default about Y centerline (`EV-25`). (`bodies=1`, `faces=10`, `edges=21`, `vol=60465.7083` mm³, `centroid=[20.7969, 20.0, 0.3721]` mm, `bbox=[0.0, 0.0, -20.0]..[40.0, 40.0, 20.0]` mm)
- `revolve_cut_90_flipped`: `40 x 40 x 40` mm cube (`z -20..20`), `15 x 20` mm `Front`-plane rectangle revolve-cut `90°` with Reverse Direction about Y centerline (`EV-25`). (`bodies=1`, `faces=10`, `edges=21`, `vol=60465.7083` mm³, `centroid=[20.7969, 20.0, -0.3721]` mm, `bbox=[0.0, 0.0, -20.0]..[40.0, 40.0, 20.0]` mm)
- `revolve_temp_axis_90_default`: Cylinder (`R=10` mm, axis `x=20 z=0`) plus disjoint `10 x 10` mm `Front`-plane boss revolved `90°` default about cylinder temporary axis (`EV-25`, `DI-39`). (`bodies=2`, `faces=9`, `edges=14`, `vol=12566.3706` mm³, `centroid=[23.2494, 2.5, -3.2494]` mm, `bbox=[10.0, -15.0, -25.0]..[45.0, 15.0, 10.0]` mm)
- `revolve_temp_axis_90_flipped`: Cylinder (`R=10` mm, axis `x=20 z=0`) plus disjoint `10 x 10` mm `Front`-plane boss revolved `90°` reversed about cylinder temporary axis (`EV-25`, `DI-39`). (`bodies=2`, `faces=9`, `edges=14`, `vol=12566.3706` mm³, `centroid=[23.2494, 2.5, 3.2494]` mm, `bbox=[10.0, -15.0, -10.0]..[45.0, 15.0, 25.0]` mm)
- `revolve_ref_axis_90`: `10 x 10` mm rectangle on `Front` plane (`x 10..20`) revolved `90°` about explicit reference axis (`Right ^ Front`) (`DI-39`, `DI-86`). (`bodies=1`, `faces=6`, `edges=12`, `vol=2356.1945` mm³, `centroid=[9.903, 5.0, -9.903]` mm, `bbox=[0.0, 0.0, -20.0]..[20.0, 10.0, 0.0]` mm)
- `revolve_two_dir_30_60`: `10 x 10` mm rectangle on `Front` plane (`x 10..20`) two-direction solid revolve `D1=30°`, `D2=60°` about Y centerline (`EV-25`). (`bodies=1`, `faces=6`, `edges=12`, `vol=2356.1945` mm³, `centroid=[13.5277, 5.0, 3.6247]` mm, `bbox=[5.0, 0.0, -10.0]..[20.0, 10.0, 17.3205]` mm)
- `revolve_offset_plane_90`: `10 x 10` mm rectangle on `+15` mm `Front`-offset plane revolved `90°` about `Right ^ Plane1` reference axis (`x=0, z=15`) (`DI-73`, `DI-85`, `DI-86`). (`bodies=1`, `faces=6`, `edges=12`, `vol=2356.1945` mm³, `centroid=[9.903, 5.0, 5.097]` mm, `bbox=[0.0, 0.0, -5.0]..[20.0, 10.0, 15.0]` mm)

---

## 4. Asymmetric Datum Planes, Angled Planes & Displaced Box Faces (`SF-03`, `DI-73`, `DI-80`, `DI-81`, `DI-82`)

The datum-plane fixtures extrude an asymmetric L-profile `(5,5)–(25,5)–(25,10)–(10,10)–(10,20)–(5,20)` by `10` mm so any 180° flip or reversed `u_axis` yields a distinct B-rep:

- `plane_offset_pos`: `+20` mm `Front`-offset plane (default normal) with asymmetric L-profile extruded `10` mm (`SF-03`, `DI-81`). (`bodies=1`, `faces=8`, `edges=18`, `vol=1500.0` mm³, `centroid=[12.5, 10.0, 25.0]` mm, `bbox=[5.0, 5.0, 20.0]..[25.0, 20.0, 30.0]` mm)
- `plane_offset_flip_normal`: `+20` mm `Front`-offset plane with Flip normal enabled and asymmetric L-profile extruded `10` mm (`SF-03`, `DI-81`). (`bodies=1`, `faces=8`, `edges=18`, `vol=1500.0` mm³, `centroid=[-12.5, 10.0, 15.0]` mm, `bbox=[-25.0, 5.0, 10.0]..[-5.0, 20.0, 20.0]` mm)
- `plane_angled_30`: `30°` angled plane about origin Y axis (`Front ^ Right`) with asymmetric L-profile extruded `10` mm (`DI-82`, `SF-03`). (`bodies=1`, `faces=8`, `edges=18`, `vol=1500.0` mm³, `centroid=[6.1603, -12.5, 9.3301]` mm, `bbox=[-0.6699, -25.0, 2.5]..[17.3205, -5.0, 18.6603]` mm)
- `plane_offset_from_angled_30`: `+15` mm offset plane referencing a `30°` angled plane, with asymmetric L-profile extruded `10` mm (`DI-82`, `SF-03`). (`bodies=1`, `faces=8`, `edges=18`, `vol=1500.0` mm³, `centroid=[-1.3397, -12.5, 22.3205]` mm, `bbox=[-8.1699, -25.0, 15.4904]..[9.8205, -5.0, 31.6506]` mm)
- `sketch_on_displaced_box_faces`: Off-origin box (`x 10..30`, `y 15..35`, `z 5..25`) with `2` mm asymmetric L-cuts on `+X`, `-X`, `+Y`, `-Y`, and `-Z` faces (`DI-80`, `DI-73`). (`bodies=1`, `faces=41`, `edges=102`, `vol=7040.0` mm³, `centroid=[20.1705, 25.1705, 15.4727]` mm, `bbox=[10.0, 15.0, 5.0]..[30.0, 35.0, 25.0]` mm)

---

## 5. Sketch Curves, Fillets, Slots, Construction Loops & Circle Dimensions (`DI-70`, `DI-71`, `DI-77`, `DI-83`, `EV-27`, `EV-30`, `EV-32`)

- `sketch_partial_ellipse`: Closed quadrant-I trimmed elliptical arc (center `(10,10)`, `a=15`, `b=8`) with closing lines extruded `10` mm (`DI-70`, `EV-30`). (`bodies=1`, `faces=5`, `edges=9`, `vol=942.5232` mm³, `centroid=[16.3666, 13.3953, 5.0]` mm, `bbox=[10.0, 10.0, 0.0]..[25.0, 18.0, 10.0]` mm)
- `sketch_style_spline`: 4-point `Front`-plane B-spline closed with two straight segments and extruded `10` mm (`EV-30`). (`bodies=1`, `faces=5`, `edges=9`, `vol=2735.2547` mm³, `centroid=[17.3565, 4.5326, 5.0]` mm, `bbox=[0.0, 0.0, 0.0]..[35.0, 14.0, 10.0]` mm; `cadmpeg diff --geometry` reports `area=1520.1937` mm², `bbox`, `closed`, `body_count`, and `occurrences` agreeing, with `volume`/`centroid` `compare.unavailable` due to differing STEP counts)
- `sketch_slot_and_fillet_arcs`: `50 x 30` mm `Front`-plane rectangle with two `R=5` mm corner fillet arcs and an interior `R=4` mm straight slot extruded `10` mm (`DI-70`, `EV-27`). (`bodies=1`, `faces=12`, `edges=30`, `vol=12790.0443` mm³, `centroid=[25.0, 15.0, 5.0]` mm, `bbox=[0.0, 0.0, 0.0]..[50.0, 30.0, 10.0]` mm)
- `sketch_construction_outer_loop`: Outer `40 x 40` mm construction rectangle (`state=0`) enclosing an inner `20 x 20` mm solid profile (`state=1`) extruded `10` mm (`EV-32`, `DI-77`). (`bodies=1`, `faces=6`, `edges=12`, `vol=4000.0` mm³, `centroid=[20.0, 20.0, 5.0]` mm, `bbox=[10.0, 10.0, 0.0]..[30.0, 30.0, 10.0]` mm)
- `sketch_circle_dim_radial_vs_center`: Concentric `R=25` mm construction circle plus dimensioned `R=15` mm and `R=6` mm profile circles extruded `10` mm (`DI-71`, `DI-83`). (`bodies=1`, `faces=4`, `edges=4`, `vol=5937.6101` mm³, `centroid=[0.0, 0.0, 5.0]` mm, `bbox=[-15.0, -15.0, 0.0]..[15.0, 15.0, 10.0]` mm)

---

## 6. Draft, Surface Cut, Circular Sweep, Patterns, Combine & Delete Body (`DI-23`, `DI-24`, `DI-28`, `DI-30`, `DI-31`, `DI-32`, `DI-36`, `DI-41`)

- `draft_neutral_inward`: `40 x 40 x 20` mm block, `10°` neutral-plane draft from `z=0` on `+X` and `+Y` faces (inward, `FlipDir=True`) (`DI-31`). (`bodies=1`, `faces=6`, `edges=12`, `vol=29261.6782` mm³, `centroid=[19.1455, 19.1455, 9.6928]` mm, `bbox=[0.0, 0.0, 0.0]..[40.0, 40.0, 20.0]` mm)
- `draft_neutral_reversed`: `40 x 40 x 20` mm block, `10°` neutral-plane draft from `z=0` on `+X` and `+Y` faces (outward, `FlipDir=False`) (`DI-31`). (`bodies=1`, `faces=6`, `edges=12`, `vol=34904.1416` mm³, `centroid=[20.9064, 20.9064, 10.2813]` mm, `bbox=[0.0, 0.0, 0.0]..[43.5265, 43.5265, 20.0]` mm)
- `surface_cut_side_a`: `40 x 40 x 20` mm block sliced by slanted extruded surface (`z 5..15` across `y 0..40`) keeping default side (`DI-36`). (`bodies=1`, `faces=6`, `edges=12`, `vol=16000.0` mm³, `centroid=[20.0, 22.2222, 5.1852]` mm, `bbox=[0.0, 0.0, 0.0]..[40.0, 40.0, 13.3333]` mm)
- `surface_cut_side_b`: `40 x 40 x 20` mm block sliced by slanted extruded surface (`z 5..15` across `y 0..40`) keeping flipped side (`DI-36`). (`bodies=1`, `faces=6`, `edges=12`, `vol=16000.0` mm³, `centroid=[20.0, 17.7778, 14.8148]` mm, `bbox=[0.0, 0.0, 6.6667]..[40.0, 40.0, 20.0]` mm)
- `sweep_circular_merge_on`: `40 x 40 x 10` mm plate plus `D=6` mm circular-profile L-sweep on `z=10` face with Merge enabled (`DI-30`, `DI-28`). (`bodies=1`, `faces=11`, `edges=24`, `vol=16393.7764` mm³, `centroid=[20.0966, 20.0235, 5.1507]` mm, `bbox=[0.0, 0.0, 0.0]..[40.0, 40.0, 13.0]` mm)
- `sweep_circular_merge_off`: `40 x 40 x 10` mm plate plus `D=6` mm circular-profile L-sweep on `z=10` face with Merge disabled (`DI-30`, `DI-28`). (`bodies=2`, `faces=11`, `edges=16`, `vol=16787.5528` mm³, `centroid=[20.1887, 20.0459, 5.2346]` mm, `bbox=[0.0, 0.0, 0.0]..[40.0, 40.0, 13.0]` mm)
- `pattern_linear_two_dir`: `60 x 40 x 10` mm plate with `6 x 6` mm through-cut patterned `3 x 2` at `15` mm spacing along `+X` and `+Y` edges (`DI-32`, `DI-41`). (`bodies=1`, `faces=30`, `edges=84`, `vol=21840.0` mm³, `centroid=[30.5934, 20.3462, 5.0]` mm, `bbox=[0.0, 0.0, 0.0]..[60.0, 40.0, 10.0]` mm)
- `pattern_mirror_two_seeds`: `40 x 40 x 10` mm block centered on `Right` plane with two seed features (`6x6` cut + `6x6x5` boss) mirrored across `Right` plane (`DI-41`). (`bodies=1`, `faces=26`, `edges=60`, `vol=16000.0` mm³, `centroid=[0.0, 20.36, 5.225]` mm, `bbox=[-20.0, 0.0, 0.0]..[20.0, 40.0, 15.0]` mm)
- `combine_subtract`: `15 x 15 x 20` mm target body (`x 10..25`, `y 10..25`, `z -5..15`) split into two disjoint `15 x 15 x 5` mm bodies (`z -5..0` and `z 10..15`) by subtracting a `30 x 30 x 10` mm tool body (`z 0..10`) via Combine Subtract (`DI-23`). (`bodies=2`, `faces=12`, `edges=24`, `vol=2250.0` mm³, `centroid=[17.5, 17.5, 5.0]` mm, `bbox=[10.0, 10.0, -5.0]..[25.0, 25.0, 15.0]` mm)
- `delete_body_one_of_two`: Two disjoint `20 x 20 x 10` mm bodies (`x 0..20` and `x 30..50`) with the first body (`x 0..20`) deleted via Body-Delete/Keep (`DI-24`), retaining `x 30..50`. (`bodies=1`, `faces=6`, `edges=12`, `vol=4000.0` mm³, `centroid=[40.0, 10.0, 5.0]` mm, `bbox=[30.0, 0.0, 0.0]..[50.0, 20.0, 10.0]` mm)

---

## 7. Custom Linear Units, Trigonometric Equations & Sequential Edit Logs (`DI-43`, `DI-72`, `EV-35`)

- `units_cm`, `units_in`, `units_ft_in`, `units_mil`: Identical physical `20 x 20 x 10` mm block with a `20` mm sketch dimension authored under `swUnitSystem_Custom` with `swUnitsLinear` set to centimeters (`swCM`), inches (`swINCHES`), feet-inches (`swFEETINCHES`), and mils (`swMIL`) (`DI-72`). (`bodies=1`, `faces=6`, `edges=12`, `vol=4000.0` mm³, `centroid=[10.0, 10.0, 5.0]` mm, `bbox=[0.0, 0.0, 0.0]..[20.0, 20.0, 10.0]` mm)
- `equations_trig_deg`: `20 x 20` mm block with equations `"A" = atn(1)` and `"D1@Boss-Extrude1" = 20 + 10*sin(1)` evaluated in Degrees (`DI-43`). (`bodies=1`, `faces=6`, `edges=12`, `vol=8069.8096` mm³, `centroid=[10.0, 10.0, 10.0873]` mm, `bbox=[0.0, 0.0, 0.0]..[20.0, 20.0, 20.1745]` mm)
- `equations_trig_rad`: `20 x 20` mm block with the same equations evaluated in Radians (`DI-43`). (`bodies=1`, `faces=6`, `edges=12`, `vol=11365.8839` mm³, `centroid=[10.0, 10.0, 14.2074]` mm, `bbox=[0.0, 0.0, 0.0]..[20.0, 20.0, 28.4147]` mm)
- `edit_log_step1_base`: Sequential edit log step 1: `20 x 20 x 10` mm base block before adding a fillet (`EV-35`). (`bodies=1`, `faces=6`, `edges=12`, `vol=4000.0` mm³, `centroid=[10.0, 10.0, 5.0]` mm, `bbox=[0.0, 0.0, 0.0]..[20.0, 20.0, 10.0]` mm)
- `edit_log_step2_added_fillet`: Same document saved sequentially after adding an `R=3` mm edge fillet (`EV-35`). (`bodies=1`, `faces=7`, `edges=15`, `vol=3961.3717` mm³, `centroid=[10.0, 10.091, 4.9578]` mm, `bbox=[0.0, 0.0, 0.0]..[20.0, 20.0, 10.0]` mm)
- `edit_log_step3_deleted_fillet`: Same document saved sequentially after deleting the edge fillet (`EV-35`). (`bodies=1`, `faces=6`, `edges=12`, `vol=4000.0` mm³, `centroid=[10.0, 10.0, 5.0]` mm, `bbox=[0.0, 0.0, 0.0]..[20.0, 20.0, 10.0]` mm)
