# SolidWorks `.SLDPRT` + `.STEP` AP214 Corpus

This directory contains paired SolidWorks specimens: each `<stem>.SLDPRT` is paired with a ground-truth `<stem>.STEP` AP214 export, normalized to LF line endings. Every file is authored from a blank/default part template in SolidWorks 2021 SP5.1 and dedicated to the public domain under CC0-1.0.

Parts use millimeter-gram-second (`MMGS`) units except where custom linear units are configured (`units_*`), and coordinates are stated in millimeters (`mm`).

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

Baseline pairs covering reference-plane polarity, start-offset extrusion direction, sketch curve forms, and partial 90° revolution turn sense:

### `Pair1` — Principal Planes & Blind Direction (`DI-16`, `SF-01`)
- `Pair1/plane_front_blind`: `20 x 10` mm rectangle on `Front` plane extruded `+10` mm along profile normal.
- `Pair1/plane_front_flip`: `20 x 10` mm rectangle on `Front` plane extruded `10` mm with Reverse Direction.
- `Pair1/plane_right_blind`: `20 x 10` mm rectangle on `Right` plane extruded `10` mm along `+X`.
- `Pair1/plane_top_blind`: `20 x 10` mm rectangle on `Top` plane extruded `10` mm along `+Y`.

### `Pair2` — Extrude Start Offset (`DI-68`)
- `Pair2/extrude_start_offset_pos`: `20 x 10` mm rectangle on `Front` plane, blind extrude `10` mm from `+5` mm offset start.
- `Pair2/extrude_start_offset_flip`: `20 x 10` mm rectangle on `Front` plane, blind extrude `10` mm from flipped `5` mm offset start.

### `Pair3` — Concentric Circles & Dimension Ownership (`DI-70`, `DI-71`)
- `Pair3/sketch_concentric_one_dim`: Concentric circles (`r=5` mm, `r=10` mm) with inner circle dimensioned, blind extrude `10` mm.
- `Pair3/sketch_concentric_ring`: Concentric circles (`r=5` mm, `r=10` mm) with both circles dimensioned, blind extrude `10` mm.

### `Pair4` — Arcs, Ellipses & Parabolas (`EV-27`, `GC-11`)
- `Pair4/sketch_arcs_cw_ccw`: Closed profile with CCW arc, CW `0xffff` arc (`sldprt.md` §2.3), and interior full circle, blind extrude `5` mm.
- `Pair4/sketch_ellipse`: Full sketch ellipse, blind extrude `5` mm.
- `Pair4/sketch_parabola`: Closed profile bounded by a trimmed sketch parabola and line, blind extrude `5` mm.

### `Pair5` — Partial 90° Revolutions (`EV-25`)
- `Pair5/revolve_90_default`: `10 x 10` mm rectangle revolved `90°` in default direction about Y axis.
- `Pair5/revolve_90_flipped`: `10 x 10` mm rectangle revolved `90°` with Reverse Direction about Y axis.

---

## 2. Cuts, Two-Direction Extrusions & Up-To-Vertex (`DI-15`, `DI-16`, `DI-17`)

- `cut_blind_default`: `40 x 40 x 20` mm block (`z -10..10`), `10 x 10` mm square on `Front` plane (`z=0`) cut `5` mm blind (`z -5..0`) (`DI-16`).
- `cut_blind_flip`: `40 x 40 x 20` mm block (`z -10..10`), `10 x 10` mm square on `Front` plane (`z=0`) cut `5` mm blind with cut direction reversed (`z 0..5`) (`DI-16`).
- `extrude_two_dir_unequal`: `20 x 10` mm rectangle on `Front` plane, two-direction blind extrude `D1=5` mm, `D2=12` mm (`DI-16`).
- `cut_two_dir_unequal`: `40 x 40 x 30` mm block (`z -15..15`), `10 x 10` mm corner notch on `Front` plane `z=0` cut two-direction blind `D1=5` mm, `D2=12` mm (`DI-16`).
- `extrude_two_dir_through_all`: C-bracket (gap `y 5..20`), boss on top leg `y=25` with `D1=Blind 5` mm and `D2=Through All` bridging to bottom leg (`DI-17`).
- `extrude_up_to_vertex_low`: `8 x 8` mm boss on `Front` plane (`x 12..20`) extruded Up To Vertex `(10, 10, 10)` on step edge (`DI-15`).
- `extrude_up_to_vertex_high`: `8 x 8` mm boss on `Front` plane (`x 12..20`) extruded Up To Vertex `(10, 10, 22)` on step edge (`DI-15`).

---

## 3. Revolve Cuts, Axes & Two-Direction Revolves (`EV-25`, `DI-39`, `DI-73`, `DI-85`, `DI-86`)

- `revolve_cut_90_default`: `40 x 40 x 40` mm cube (`z -20..20`), `15 x 20` mm `Front`-plane rectangle revolve-cut `90°` default about Y centerline (`EV-25`).
- `revolve_cut_90_flipped`: `40 x 40 x 40` mm cube (`z -20..20`), `15 x 20` mm `Front`-plane rectangle revolve-cut `90°` with Reverse Direction about Y centerline (`EV-25`).
- `revolve_temp_axis_90_default`: Cylinder (`R=10` mm, axis `x=20 z=0`) plus disjoint `10 x 10` mm `Front`-plane boss revolved `90°` default about cylinder temporary axis (`EV-25`, `DI-39`).
- `revolve_temp_axis_90_flipped`: Cylinder (`R=10` mm, axis `x=20 z=0`) plus disjoint `10 x 10` mm `Front`-plane boss revolved `90°` reversed about cylinder temporary axis (`EV-25`, `DI-39`).
- `revolve_ref_axis_90`: `10 x 10` mm rectangle on `Front` plane (`x 10..20`) revolved `90°` about explicit reference axis (`Right ^ Front`) (`DI-39`, `DI-86`).
- `revolve_two_dir_30_60`: `10 x 10` mm rectangle on `Front` plane (`x 10..20`) two-direction solid revolve `D1=30°`, `D2=60°` about Y centerline (`EV-25`).
- `revolve_offset_plane_90`: `10 x 10` mm rectangle on `+15` mm `Front`-offset plane revolved `90°` about `Right ^ Plane1` reference axis (`x=0, z=15`) (`DI-73`, `DI-85`, `DI-86`).

---

## 4. Asymmetric Datum Planes, Angled Planes & Displaced Box Faces (`SF-03`, `DI-73`, `DI-80`, `DI-81`, `DI-82`)

The datum-plane fixtures extrude an asymmetric L-profile `(5,5)–(25,5)–(25,10)–(10,10)–(10,20)–(5,20)` by `10` mm so any 180° flip or reversed `u_axis` yields a distinct B-rep:

- `plane_offset_pos`: `+20` mm `Front`-offset plane (default normal) with asymmetric L-profile extruded `10` mm (`SF-03`, `DI-81`).
- `plane_offset_flip_normal`: `+20` mm `Front`-offset plane with Flip normal enabled and asymmetric L-profile extruded `10` mm (`SF-03`, `DI-81`).
- `plane_angled_30`: `30°` angled plane about origin Y axis (`Front ^ Right`) with asymmetric L-profile extruded `10` mm (`DI-82`, `SF-03`).
- `plane_offset_from_angled_30`: `+15` mm offset plane referencing a `30°` angled plane, with asymmetric L-profile extruded `10` mm (`DI-82`, `SF-03`).
- `sketch_on_displaced_box_faces`: Off-origin box (`x 10..30`, `y 15..35`, `z 5..25`) with `2` mm asymmetric L-cuts on `+X`, `-X`, `+Y`, `-Y`, and `-Z` faces (`DI-80`, `DI-73`).

---

## 5. Sketch Curves, Fillets, Slots, Construction Loops & Circle Dimensions (`DI-70`, `DI-71`, `DI-77`, `DI-83`, `EV-27`, `EV-30`, `EV-32`)

- `sketch_partial_ellipse`: Closed quadrant-I trimmed elliptical arc (center `(10,10)`, `a=15`, `b=8`) with closing lines extruded `10` mm (`DI-70`, `EV-30`).
- `sketch_style_spline`: 4-point `Front`-plane B-spline closed with two straight segments and extruded `10` mm (`EV-30`).
- `sketch_slot_and_fillet_arcs`: `50 x 30` mm `Front`-plane rectangle with two `R=5` mm corner fillet arcs and an interior `R=4` mm straight slot extruded `10` mm (`DI-70`, `EV-27`).
- `sketch_construction_outer_loop`: Outer `40 x 40` mm construction rectangle (`state=0`) enclosing an inner `20 x 20` mm solid profile (`state=1`) extruded `10` mm (`EV-32`, `DI-77`).
- `sketch_circle_dim_radial_vs_center`: Concentric `R=25` mm construction circle plus dimensioned `R=15` mm and `R=6` mm profile circles extruded `10` mm (`DI-71`, `DI-83`).

---

## 6. Draft, Surface Cut, Circular Sweep, Patterns, Combine & Delete Body (`DI-23`, `DI-24`, `DI-28`, `DI-30`, `DI-31`, `DI-32`, `DI-36`, `DI-41`)

- `draft_neutral_inward`: `40 x 40 x 20` mm block, `10°` neutral-plane draft from `z=0` on `+X` and `+Y` faces (inward, `FlipDir=True`) (`DI-31`).
- `draft_neutral_reversed`: `40 x 40 x 20` mm block, `10°` neutral-plane draft from `z=0` on `+X` and `+Y` faces (outward, `FlipDir=False`) (`DI-31`).
- `surface_cut_side_a`: `40 x 40 x 20` mm block sliced by slanted extruded surface (`z 5..15` across `y 0..40`) keeping default side (`DI-36`).
- `surface_cut_side_b`: `40 x 40 x 20` mm block sliced by slanted extruded surface (`z 5..15` across `y 0..40`) keeping flipped side (`DI-36`).
- `sweep_circular_merge_on`: `40 x 40 x 10` mm plate plus `D=6` mm circular-profile L-sweep on `z=10` face with Merge enabled (`DI-30`, `DI-28`).
- `sweep_circular_merge_off`: `40 x 40 x 10` mm plate plus `D=6` mm circular-profile L-sweep on `z=10` face with Merge disabled (`DI-30`, `DI-28`).
- `pattern_linear_two_dir`: `60 x 40 x 10` mm plate with `6 x 6` mm through-cut patterned `3 x 2` at `15` mm spacing along `+X` and `+Y` edges (`DI-32`, `DI-41`).
- `pattern_mirror_two_seeds`: `40 x 40 x 10` mm block centered on `Right` plane with two seed features (`6x6` cut + `6x6x5` boss) mirrored across `Right` plane (`DI-41`).
- `combine_subtract`: `15 x 15 x 20` mm target body (`x 10..25`, `y 10..25`, `z -5..15`) split into two disjoint `15 x 15 x 5` mm bodies (`z -5..0` and `z 10..15`) by subtracting a `30 x 30 x 10` mm tool body (`z 0..10`) via Combine Subtract (`DI-23`).
- `delete_body_one_of_two`: Two disjoint `20 x 20 x 10` mm bodies (`x 0..20` and `x 30..50`) with the first body (`x 0..20`) deleted via Body-Delete/Keep (`DI-24`), retaining `x 30..50`.

---

## 7. Custom Linear Units, Trigonometric Equations & Sequential Edit Logs (`DI-43`, `DI-72`, `EV-35`)

- `units_cm`, `units_in`, `units_ft_in`, `units_mil`: Identical physical `20 x 20 x 10` mm block with a `20` mm sketch dimension authored under `swUnitSystem_Custom` with `swUnitsLinear` set to centimeters (`swCM`), inches (`swINCHES`), feet-inches (`swFEETINCHES`), and mils (`swMIL`) (`DI-72`).
- `equations_trig_deg`: `20 x 20` mm block with equations `"A" = atn(1)` and `"D1@Boss-Extrude1" = 20 + 10*sin(1)` evaluated in Degrees (`DI-43`).
- `equations_trig_rad`: `20 x 20` mm block with the same equations evaluated in Radians (`DI-43`).
- `edit_log_step1_base`: Sequential edit log step 1: `20 x 20 x 10` mm base block before adding a fillet (`EV-35`).
- `edit_log_step2_added_fillet`: Same document saved sequentially after adding an `R=3` mm edge fillet (`EV-35`).
- `edit_log_step3_deleted_fillet`: Same document saved sequentially after deleting the edge fillet (`EV-35`).
