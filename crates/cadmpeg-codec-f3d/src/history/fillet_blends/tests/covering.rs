// SPDX-License-Identifier: Apache-2.0
//! Blends whose supports share several edges along the blend axis.

use super::{
    bind_unbound, bind_unbound_fillet, blends, earlier, faces, fillet, group, history_between,
    input, line, native, one_mm, output, partial, proven, scope, slots, three_mm, transition, x, y,
    z, SCOPE_ID,
};
use crate::history::fillet_blends::{
    bind_fillet_groups, consumption_conflicts, transition_blends, Blend, Found, SharedEdge,
};
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCurveAxis, AsmHistoricalCylinder, AsmHistoricalEdge,
    AsmHistoricalEdgeSample, AsmHistoricalOptionalCarrierBinding, AsmHistoricalPoint,
    AsmHistoricalSurfaceAxis, AsmHistoricalSurfaceRadius, AsmHistoricalTopology,
};
use crate::loss::F3dLossCode;
use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::math::Point3;
use std::collections::HashSet;

/// Give edge `edge` end points at heights `start` and `end` on the z axis.
fn ends(topology: &mut AsmHistoricalTopology, edge: i64, [start, end]: [f64; 2]) {
    let (start_vertex, end_vertex) = (1_000 * edge + 1, 1_000 * edge + 2);
    topology.edge_vertices.push(AsmHistoricalEdge {
        edge,
        start_vertex,
        end_vertex,
    });
    for (vertex, height) in [(start_vertex, start), (end_vertex, end)] {
        topology.vertex_points.push(AsmHistoricalCarrierBinding {
            entity: vertex,
            carrier: vertex + 500,
        });
        topology.point_positions.push(AsmHistoricalPoint {
            point: vertex + 500,
            position: Point3::new(0.0, 0.0, height),
        });
    }
}

/// `input(z())` with edge 18, which faces 10 and 12 share, also along z, and
/// the end points of edges 17 and 18 at the heights `span_17` and `span_18`.
fn two_along(span_17: [f64; 2], span_18: [f64; 2]) -> AsmHistoricalTopology {
    let mut two = input(z());
    two.edge_samples.retain(|sample| sample.edge != 18);
    two.curve_axes.retain(|axis| axis.curve != 118);
    two.edge_samples.push(AsmHistoricalEdgeSample {
        bounds: None,
        edge: 18,
        interior: Point3::new(0.0, 0.0, 0.0),
        tangent: Some(z()),
    });
    two.curve_axes.push(AsmHistoricalCurveAxis {
        curve: 118,
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: z(),
    });
    ends(&mut two, 17, span_17);
    ends(&mut two, 18, span_18);
    two
}

/// `output()` whose blend boundary edges 30, 31, and 32 have interior points
/// at heights 0, 4, and 2: the blend's axial extent spans heights 0 to 4.
fn spanned_output() -> AsmHistoricalTopology {
    let mut output = output();
    for (edge, height) in [(30, 0.0), (31, 4.0), (32, 2.0)] {
        output.edge_samples.push(AsmHistoricalEdgeSample {
            bounds: None,
            edge,
            interior: Point3::new(0.0, 0.0, height),
            tangent: Some(x()),
        });
    }
    output
}

/// The blend of `spanned_output()` that reads edge 17 from the shared edges
/// 17, which covers the extent, and 18, each with its deletion status.
fn covering_reading(deleted_17: bool, deleted_18: bool) -> Vec<Blend> {
    vec![Blend {
        radius: 3.0,
        edge: 17,
        found: Found::Covering {
            shared: vec![
                SharedEdge {
                    edge: 17,
                    covers: true,
                    deleted: deleted_17,
                },
                SharedEdge {
                    edge: 18,
                    covers: false,
                    deleted: deleted_18,
                },
            ],
        },
    }]
}

#[test]
fn a_blend_whose_supports_share_several_edges_along_its_spine_replaces_the_one_that_covers_it() {
    for (span_17, span_18) in [
        ([0.0, 4.0], [2.0, 4.0]),
        ([4.0, 0.0], [2.0, 4.0]),
        ([-1.0, 5.0], [0.0, 3.0]),
        ([0.0, 4.0], [-1.0, 3.0]),
        ([1.0e-7, 4.0 - 1.0e-7], [2.0, 4.0]),
    ] {
        assert_eq!(
            blends(&two_along(span_17, span_18), &spanned_output()),
            Some(covering_reading(true, true)),
            "edge 17 from {span_17:?}, edge 18 from {span_18:?}"
        );
    }
    assert_eq!(
        transition_blends(
            &transition(&[20], &[17, 19]),
            &two_along([0.0, 4.0], [2.0, 4.0]),
            &spanned_output()
        ),
        Some(covering_reading(true, false)),
        "the transition keeps edge 18"
    );
    assert_eq!(
        blends(&two_along([2.0, 4.0], [0.0, 4.0]), &spanned_output()),
        Some(vec![Blend {
            radius: 3.0,
            edge: 18,
            found: Found::Covering {
                shared: vec![
                    SharedEdge {
                        edge: 17,
                        covers: false,
                        deleted: true,
                    },
                    SharedEdge {
                        edge: 18,
                        covers: true,
                        deleted: true,
                    },
                ],
            },
        }]),
        "edge 18 covers the extent"
    );
}

#[test]
fn a_blend_whose_shared_edges_along_its_spine_do_not_name_one_covering_edge_states_nothing() {
    for (span_17, span_18, reason) in [
        ([0.0, 3.5], [2.0, 4.0], "no edge covers the extent"),
        (
            [1.0e-5, 4.0],
            [2.0, 4.0],
            "edge 17 stops short of the low end",
        ),
        (
            [0.0, 4.0 - 1.0e-5],
            [2.0, 4.0],
            "edge 17 stops short of the high end",
        ),
        ([0.0, 4.0], [-1.0, 5.0], "both edges cover the extent"),
    ] {
        assert_eq!(
            blends(&two_along(span_17, span_18), &spanned_output()),
            None,
            "{reason}"
        );
    }
    assert_eq!(
        transition_blends(
            &transition(&[20], &[18, 19]),
            &two_along([0.0, 4.0], [2.0, 4.0]),
            &spanned_output()
        ),
        None,
        "the transition keeps covering edge 17"
    );
}

#[test]
fn a_blend_whose_shared_edges_along_its_spine_lack_a_row_that_decides_the_cover_states_nothing() {
    let covered = || two_along([0.0, 4.0], [2.0, 4.0]);
    let mut unsampled = spanned_output();
    unsampled.edge_samples.retain(|sample| sample.edge != 32);
    let mut no_vertices = covered();
    no_vertices.edge_vertices.retain(|row| row.edge != 18);
    let mut two_vertex_rows = covered();
    two_vertex_rows.edge_vertices.push(AsmHistoricalEdge {
        edge: 18,
        start_vertex: 18_001,
        end_vertex: 18_002,
    });
    let mut no_point = covered();
    no_point.vertex_points.retain(|row| row.entity != 18_002);
    let mut two_points = covered();
    two_points.vertex_points.push(AsmHistoricalCarrierBinding {
        entity: 18_002,
        carrier: 17_502,
    });
    let mut no_position = covered();
    no_position
        .point_positions
        .retain(|row| row.point != 18_502);
    let mut two_positions = covered();
    two_positions.point_positions.push(AsmHistoricalPoint {
        point: 18_502,
        position: Point3::new(0.0, 0.0, 4.0),
    });
    let mut infinite = covered();
    infinite
        .point_positions
        .iter_mut()
        .find(|row| row.point == 18_502)
        .unwrap()
        .position = Point3::new(0.0, 0.0, f64::INFINITY);
    let mut unsampled_17 = covered();
    unsampled_17
        .edge_samples
        .iter_mut()
        .find(|sample| sample.edge == 17)
        .unwrap()
        .interior = Point3::new(f64::NAN, 0.0, 0.0);
    let mut other_radius = spanned_output();
    other_radius.surface_cylinders[0].radius = 3.5;
    let mut no_axis = spanned_output();
    no_axis.surface_cylinders[0].origin = Point3::new(f64::INFINITY, 3.0, 0.0);
    for (input, output, reason) in vec![
        (
            covered(),
            unsampled,
            "boundary edge 32 has no interior sample",
        ),
        (no_vertices, spanned_output(), "edge 18 has no vertex row"),
        (
            two_vertex_rows,
            spanned_output(),
            "edge 18 has two vertex rows",
        ),
        (
            no_point,
            spanned_output(),
            "a vertex of edge 18 has no point",
        ),
        (
            two_points,
            spanned_output(),
            "a vertex of edge 18 has two points",
        ),
        (
            no_position,
            spanned_output(),
            "a point of edge 18 has no position",
        ),
        (
            two_positions,
            spanned_output(),
            "a point of edge 18 has two positions",
        ),
        (
            infinite,
            spanned_output(),
            "a point of edge 18 is not finite",
        ),
        (
            unsampled_17,
            spanned_output(),
            "covering edge 17 has no finite interior sample",
        ),
        (
            covered(),
            other_radius,
            "the cylinder row has another radius",
        ),
        (covered(), no_axis, "the cylinder row has no finite origin"),
    ] {
        assert_eq!(blends(&input, &output), None, "{reason}");
    }
}

#[test]
fn a_covering_edge_within_the_blend_radius_of_the_axis_states_nothing() {
    let mut near = two_along([0.0, 4.0], [2.0, 4.0]);
    near.edge_samples
        .iter_mut()
        .find(|sample| sample.edge == 17)
        .unwrap()
        .interior = Point3::new(1.5, 1.5, 0.0);
    assert_eq!(blends(&near, &spanned_output()), None);
}

/// `spanned_output()` with a second blend face 21 on carrier 121, a cylinder
/// along z, that meets faces 10 and 12, which share edge 18.
fn second_blend_on_18() -> AsmHistoricalTopology {
    let mut output = faces(&[
        (10, &[30, 33]),
        (11, &[31]),
        (12, &[32, 34]),
        (20, &[30, 31, 32]),
        (21, &[33, 34]),
    ]);
    for surface in [120, 121] {
        output.surface_cylinders.push(AsmHistoricalCylinder {
            surface,
            origin: Point3::new(3.0, 3.0, 0.0),
            axis: z(),
            radius: 3.0,
        });
        output.surface_radii.push(AsmHistoricalSurfaceRadius {
            surface,
            radius: 3.0,
        });
    }
    output.edge_samples = spanned_output().edge_samples;
    output
}

#[test]
fn a_blend_that_replaces_an_edge_that_a_covering_blend_consumes_states_nothing() {
    assert_eq!(
        transition_blends(
            &transition(&[20, 21], &[17, 18, 19]),
            &two_along([0.0, 4.0], [2.0, 4.0]),
            &second_blend_on_18()
        ),
        None
    );
}

/// A torus blend face 20 on carrier 120 about the z axis meets faces 10, 11,
/// and 12. Its boundary edges 30, 31, and 32 have interior points at heights
/// 0, 4, and 2. In the input, faces 10 and 11 share edge 17, a circle about
/// the z axis from height 0 to 4, and edge 19 along x; face 12 owns edge 18,
/// a circle about the z axis from height 2 to 4, which face 10 shares when
/// `shared_18`.
fn torus(shared_18: bool) -> (AsmHistoricalTopology, AsmHistoricalTopology) {
    let mut input = if shared_18 {
        faces(&[(10, &[17, 18, 19]), (11, &[17, 19]), (12, &[18])])
    } else {
        faces(&[(10, &[17, 19]), (11, &[17, 19]), (12, &[18])])
    };
    for edge in [17, 18] {
        input.edges.push(edge);
        input.edge_curves.push(AsmHistoricalOptionalCarrierBinding {
            entity: edge,
            carrier: Some(100 + edge),
        });
        input.curve_axes.push(AsmHistoricalCurveAxis {
            curve: 100 + edge,
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: z(),
        });
        input.edge_samples.push(AsmHistoricalEdgeSample {
            bounds: None,
            edge,
            interior: Point3::new(1.0, 0.0, 0.0),
            tangent: Some(y()),
        });
    }
    ends(&mut input, 17, [0.0, 4.0]);
    ends(&mut input, 18, [2.0, 4.0]);
    line(&mut input, 19, x());
    let mut output = faces(&[(10, &[30]), (11, &[31]), (12, &[32]), (20, &[30, 31, 32])]);
    output.edge_samples = spanned_output().edge_samples;
    output.surface_axes.push(AsmHistoricalSurfaceAxis {
        surface: 120,
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: z(),
    });
    output.surface_radii.push(AsmHistoricalSurfaceRadius {
        surface: 120,
        radius: 3.0,
    });
    (input, output)
}

#[test]
fn a_torus_blend_whose_supports_share_several_edges_along_its_spine_states_nothing() {
    let (input, output) = torus(false);
    assert_eq!(
        transition_blends(&transition(&[20], &[17, 18, 19]), &input, &output),
        Some(vec![proven(3.0, 17)])
    );
    let (input, output) = torus(true);
    assert_eq!(
        transition_blends(&transition(&[20], &[17, 18, 19]), &input, &output),
        None
    );
}

#[test]
fn a_covering_blend_edge_binds_with_a_note_that_names_the_shared_edges() {
    let bind = |edges: EdgeSelection, deleted: &[i64]| {
        let mut definition = fillet(edges, three_mm());
        let mut notes = Vec::new();
        bind_fillet_groups(
            &mut definition,
            &FeatureId::mint("f3d:test:feature#42").expect("identity grammar"),
            &scope(42),
            &[group(42, &[7])],
            1,
            (
                &transition(&[20], deleted),
                &two_along([0.0, 4.0], [2.0, 4.0]),
                &spanned_output(),
            ),
            &mut notes,
        );
        let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) = definition else {
            panic!("expected a fillet");
        };
        (slots(&groups[0].edges), notes)
    };
    let (bound, notes) = bind(native(), &[17, 18, 19]);
    assert_eq!(bound, ["17"]);
    assert_eq!(notes.len(), 1);
    assert_eq!(
        notes[0].code,
        F3dLossCode::HistoricalFilletEdgeCovering.kind()
    );
    assert_eq!(
        notes[0].message,
        "Fillet group 0 of feature f3d:test:feature#42: the edge at slot 17 of history state 1 is inferred from the blend geometry, because the blend's supports share 2 edges along its axis. The reading assumes the following about the rounded edge. It is the one shared edge whose span between its end points covers the blend's axial extent within 1e-6 mm at each end. Its interior sample point lies more than 1e-6 mm beyond the radius from the axis line. The transition deletes it. The reading also assumes that the blend consumes the other shared edges that the transition deletes. Shared edges in slot order: slot 17 (deleted, covers the extent, selected), slot 18 (deleted)."
    );
    let (_, notes) = bind(native(), &[17, 19]);
    assert!(
        notes[0]
            .message
            .ends_with("Shared edges in slot order: slot 17 (deleted, covers the extent, selected), slot 18 (kept)."),
        "{}",
        notes[0].message
    );
    let (bound, notes) = bind(partial(&[17]), &[17, 18, 19]);
    assert_eq!(bound, ["17"]);
    assert_eq!(
        notes
            .iter()
            .map(|note| note.code.clone())
            .collect::<Vec<_>>(),
        [F3dLossCode::HistoricalFilletEdgeCovering.kind()],
        "a stated slot keeps the note, because the consumed edges stay assumed"
    );
    let (bound, notes) = bind(earlier(&[17, 18]), &[17, 18, 19]);
    assert_eq!(
        bound,
        ["17", "18"],
        "a complete selection of every deleted shared edge disagrees and stays"
    );
    assert!(notes.is_empty(), "{notes:?}");
}

/// A covering blend of `edge` that consumes the deleted edge `consumed`.
fn consuming(edge: i64, consumed: i64) -> Blend {
    Blend {
        radius: 3.0,
        edge,
        found: Found::Covering {
            shared: vec![
                SharedEdge {
                    edge,
                    covers: true,
                    deleted: true,
                },
                SharedEdge {
                    edge: consumed,
                    covers: false,
                    deleted: true,
                },
            ],
        },
    }
}

#[test]
fn two_covering_blends_that_consume_one_edge_conflict() {
    let replaced = |edges: &[i64]| edges.iter().copied().collect::<HashSet<_>>();
    assert!(!consumption_conflicts(
        &[consuming(17, 18), consuming(19, 20)],
        &replaced(&[17, 19])
    ));
    assert!(consumption_conflicts(
        &[consuming(17, 18), consuming(19, 18)],
        &replaced(&[17, 19])
    ));
    assert!(consumption_conflicts(
        &[consuming(17, 18), proven(3.0, 18)],
        &replaced(&[17, 18])
    ));
    let mut kept = consuming(19, 18);
    if let Found::Covering { shared } = &mut kept.found {
        shared[1].deleted = false;
    }
    assert!(!consumption_conflicts(
        &[consuming(17, 18), kept],
        &replaced(&[17, 19])
    ));
}

#[test]
fn an_unbound_fillet_takes_a_covering_history_only_when_no_other_history_binds() {
    let proven = || history_between("f3d:proven", input(z()), output());
    // History rows are kept in slot order.
    let covering = |id: &str| {
        let mut input = two_along([0.0, 4.0], [2.0, 4.0]);
        input.curve_axes.sort_by_key(|axis| axis.curve);
        input.edge_samples.sort_by_key(|sample| sample.edge);
        history_between(id, input, spanned_output())
    };
    let unresolved = || history_between("f3d:unresolved", input(z()), input(z()));
    let history_notes = |bindings: &crate::history::fillet_blends::FilletBlendBindings| {
        bindings
            .notes
            .iter()
            .filter(|note| note.code == F3dLossCode::HistoricalEdgeHistoryInferred.kind())
            .map(|note| note.message.clone())
            .collect::<Vec<_>>()
    };

    // One history binds by a shared edge and the other through the covering
    // reading: the shared-edge history ranks first and binds.
    let (edges, bindings) = bind_unbound(&mut [proven(), covering("f3d:covering")]);
    assert_eq!(slots(&edges), ["17"]);
    assert_eq!(
        bindings.inferred_scope_histories,
        [(SCOPE_ID.to_owned(), "f3d:proven".to_owned())]
    );
    assert_eq!(
        history_notes(&bindings),
        ["feature f3d:test:feature#42: its scope has no bound history and 2 histories hold its state pair; its edges resolve without the covering reading only in f3d:proven, and through it in 1 other history."]
    );
    assert!(bindings
        .notes
        .iter()
        .all(|note| note.code != F3dLossCode::HistoricalFilletEdgeCovering.kind()));

    let (edges, bindings) = bind_unbound(&mut [unresolved(), covering("f3d:covering")]);
    assert_eq!(slots(&edges), ["17"]);
    assert_eq!(
        bindings.inferred_scope_histories,
        [(SCOPE_ID.to_owned(), "f3d:covering".to_owned())]
    );
    assert_eq!(
        history_notes(&bindings),
        ["feature f3d:test:feature#42: its scope has no bound history and 2 histories hold its state pair; its edges resolve only in f3d:covering, through the covering reading."]
    );
    assert!(bindings
        .notes
        .iter()
        .any(|note| note.code == F3dLossCode::HistoricalFilletEdgeCovering.kind()));

    let (edges, bindings) =
        bind_unbound(&mut [covering("f3d:covering"), covering("f3d:other-covering")]);
    assert_eq!(edges, native());
    assert!(bindings.inferred_scope_histories.is_empty());
}

#[test]
fn a_covering_transition_that_binds_no_group_does_not_resolve_its_history() {
    let mut input = two_along([0.0, 4.0], [2.0, 4.0]);
    input.curve_axes.sort_by_key(|axis| axis.curve);
    input.edge_samples.sort_by_key(|sample| sample.edge);
    // The covering blend's radius matches no group, so neither history
    // binds a group, and both keep the stated partial selection.
    let (edges, bindings) = bind_unbound_fillet(
        fillet(partial(&[17]), one_mm()),
        &mut [
            history_between("f3d:covering", input, spanned_output()),
            history_between("f3d:unresolved", super::input(z()), super::input(z())),
        ],
    );
    assert_eq!(edges, partial(&[17]));
    assert!(bindings.inferred_scope_histories.is_empty());
    assert!(bindings.notes.is_empty(), "{:?}", bindings.notes);
}

#[test]
fn a_history_whose_blends_bind_no_group_does_not_resolve() {
    let mut input = two_along([0.0, 4.0], [2.0, 4.0]);
    input.curve_axes.sort_by_key(|axis| axis.curve);
    input.edge_samples.sort_by_key(|sample| sample.edge);
    // The covering history rebinds the partial selection; the unresolved
    // history binds no group, so its unchanged partial selection does not
    // make it a resolving history.
    let (edges, bindings) = bind_unbound_fillet(
        fillet(partial(&[17]), three_mm()),
        &mut [
            history_between("f3d:covering", input, spanned_output()),
            history_between("f3d:unresolved", super::input(z()), super::input(z())),
        ],
    );
    assert_eq!(slots(&edges), ["17"]);
    assert!(matches!(edges, EdgeSelection::Historical { .. }));
    assert_eq!(
        bindings.inferred_scope_histories,
        [(SCOPE_ID.to_owned(), "f3d:covering".to_owned())]
    );
}
