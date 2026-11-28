// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::history_records::{AsmDeltaState, AsmEntityVersion, AsmHistoricalTopology};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::NonZeroLength;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    SketchPlacement, SketchProfiles,
};

const TOLERANCE: f64 = 1.0e-6;

fn extent(first: f64, second: f64) -> ExtrudeExtent {
    let side = |length| ExtrudeSide {
        termination: LinearTermination::Blind {
            length: NonZeroLength::new(length).unwrap(),
        },
        draft: None,
    };
    ExtrudeExtent::TwoSided {
        first: side(first),
        second: side(second),
    }
}

fn prism(low: f64, high: f64) -> AsmHistoricalTopology {
    let mut topology = super::super::tests::prism_over(2.0);
    let position = |point: &mut Point3| point.z = low + point.z * (high - low) / 2.0;
    for row in &mut topology.point_positions {
        position(&mut row.position);
    }
    for row in &mut topology.surface_planes {
        position(&mut row.origin);
    }
    for row in &mut topology.curve_axes {
        position(&mut row.origin);
        row.direction.z *= (high - low) / 2.0;
    }
    topology
}

fn select(
    before: AsmHistoricalTopology,
    after: AsmHistoricalTopology,
    extent: &ExtrudeExtent,
    direction: &ExtrudeDirection,
) -> Option<PlanarProfileRef> {
    select_with_ambiguity(before, after, extent, direction, false)
}

fn select_with_ambiguity(
    before: AsmHistoricalTopology,
    after: AsmHistoricalTopology,
    extent: &ExtrudeExtent,
    direction: &ExtrudeDirection,
    ambiguous: bool,
) -> Option<PlanarProfileRef> {
    select_from(
        before,
        after,
        extent,
        direction,
        ambiguous,
        &ExtrudeStart::ProfilePlane {},
    )
}

fn select_from(
    before: AsmHistoricalTopology,
    after: AsmHistoricalTopology,
    extent: &ExtrudeExtent,
    direction: &ExtrudeDirection,
    ambiguous: bool,
    start: &ExtrudeStart,
) -> Option<PlanarProfileRef> {
    let sketch = Sketch {
        id: SketchId::mint("synthetic:test:sketch#two-sided").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        native_ref: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::default(),
    };
    let entities: Vec<_> = [
        ((0.0, 0.0), (2.0, 0.0)),
        ((2.0, 0.0), (2.0, 1.0)),
        ((2.0, 1.0), (0.0, 1.0)),
        ((0.0, 1.0), (0.0, 0.0)),
        ((1.0, 0.0), (1.0, 1.0)),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (start, end))| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:curve#{index}")).unwrap(),
            sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            })
            .unwrap(),
        )
    })
    .collect();
    let states: Vec<_> = [(2, after), (1, before)]
        .into_iter()
        .map(|(state_id, topology): (i64, AsmHistoricalTopology)| {
            let versions = topology
                .faces
                .iter()
                .map(|face| AsmEntityVersion {
                    entity_ref: *face,
                    record_ref: state_id * 100 + face,
                })
                .collect();
            (
                AsmDeltaState {
                    id: format!("synthetic:test:state#{state_id}"),
                    parent: "synthetic:test:history#two-sided".into(),
                    byte_offset: 0,
                    state_id,
                    version_flag: 1,
                    state_flag: 0,
                    previous_ref: None,
                    next_ref: None,
                    node_index: state_id,
                    partner_ref: None,
                    owner_ref: 0,
                    bulletin_boards: Vec::new(),
                    records: Vec::new(),
                    transition: None,
                },
                Some(topology),
                versions,
            )
        })
        .collect();
    let alternate = ambiguous.then(|| {
        let mut alternate = states.clone();
        for (state, _, _) in &mut alternate {
            state.parent = "synthetic:test:history#two-sided-other".into();
            state.id = format!("synthetic:test:other-state#{}", state.state_id);
        }
        crate::test_support::history::history(
            "synthetic:test:history#two-sided-other".into(),
            0,
            None,
            false,
            alternate,
        )
    });
    let history = crate::test_support::history::history(
        "synthetic:test:history#two-sided".into(),
        0,
        None,
        false,
        states,
    );
    let mut histories = vec![history];
    if let Some(alternate) = alternate {
        histories.push(alternate);
    }
    crate::test_support::with_decode_context(|ctx| {
        let work = ProfileGeometryWork::new();
        let result = super::super::added_candidate(
            &sketch,
            ScopedExtrudeProfileResolution {
                entities: &entities,
                spatial_entities: &[],
                histories: &histories,
                linear_tolerance: TOLERANCE,
                angular_tolerance: TOLERANCE,
                profile_work: &work,
                ctx,
            },
            (2, 1),
            Sweep {
                operation: &BooleanOp::Join,
                start,
                direction,
                extent,
            },
        );
        assert!(work.assumed_flags().into_iter().all(|flag| !flag));
        result
    })
    .unwrap()
}

#[test]
fn parallel_plane_offsets_select_one_positive_travel_reading() {
    let run = |first, second, direction, offsets: &[f64]| {
        let planes: Vec<_> = offsets
            .iter()
            .map(|offset| PlaneSides {
                offset: *offset,
                before: Vec::new(),
                after: Vec::new(),
            })
            .collect();
        travel(
            Sweep {
                operation: &BooleanOp::Join,
                start: &ExtrudeStart::ProfilePlane {},
                direction,
                extent: &extent(first, second),
            },
            &planes,
            TOLERANCE,
        )
    };
    let normal = ExtrudeDirection::ProfileNormal {};
    let reversed = ExtrudeDirection::ReversedProfileNormal {};
    assert_eq!(run(3.0, 2.0, &normal, &[-2.0, 3.0]), Some((-2.0, 3.0)));
    assert_eq!(run(3.0, 2.0, &reversed, &[-3.0, 2.0]), Some((-3.0, 2.0)));
    assert_eq!(run(3.0, 2.0, &normal, &[2.0, 3.0]), Some((2.0, 3.0)));
    assert_eq!(run(3.0, 2.0, &reversed, &[-3.0, -2.0]), Some((-3.0, -2.0)));
    assert_eq!(run(-3.0, 2.0, &normal, &[-2.0, 3.0]), None);
    assert_eq!(run(3.0, -2.0, &normal, &[2.0, 3.0]), None);
    assert_eq!(run(3.0, 2.0, &normal, &[-2.0, 2.0, 3.0]), None);
    assert_eq!(run(3.0, 2.0, &normal, &[-1.0, 3.0]), None);
    assert_eq!(run(3.0, 3.0, &normal, &[3.0]), None);
    assert_eq!(run(3.0, 3.0, &normal, &[-3.0, 3.0]), Some((-3.0, 3.0)));
}

#[test]
fn both_end_caps_certify_the_same_atomic_regions() {
    let normal = ExtrudeDirection::ProfileNormal {};
    let empty = AsmHistoricalTopology::default;
    let selected = select(empty(), prism(-2.0, 3.0), &extent(3.0, 2.0), &normal).unwrap();
    let PlanarProfileRef::SketchRegions { regions, .. } = selected else {
        panic!("two directed atomic regions");
    };
    assert_eq!(regions.len(), 2);
    assert!(select(empty(), prism(-2.0, 3.0), &extent(3.0, 1.0), &normal).is_none());
    assert!(select(empty(), prism(-2.0, 3.0), &extent(2.0, 2.0), &normal).is_none());
    assert!(select(empty(), prism(0.0, 3.0), &extent(3.0, 2.0), &normal).is_none());
    assert!(select(
        empty(),
        prism(-3.0, 2.0),
        &extent(3.0, 2.0),
        &ExtrudeDirection::ReversedProfileNormal {}
    )
    .is_some());
    let mut unmatched = prism(-2.0, 3.0);
    unmatched.surface_planes[0].origin.z = -1.0;
    assert!(select(empty(), unmatched, &extent(3.0, 2.0), &normal).is_none());
}

#[test]
fn a_join_checks_updated_caps_and_laterals_on_both_sides() {
    let normal = ExtrudeDirection::ProfileNormal {};
    let before = || prism(-1.0, 1.0);
    assert!(select(before(), prism(-2.0, 3.0), &extent(3.0, 2.0), &normal).is_some());
    let mut unsupported = prism(-2.0, 3.0);
    unsupported.surface_planes[2].origin.y = 0.5;
    assert!(select(before(), unsupported, &extent(3.0, 2.0), &normal).is_none());
    let mut unsupported_before = before();
    unsupported_before.surface_planes[2].origin.y = 0.5;
    assert!(select(
        unsupported_before,
        prism(-2.0, 3.0),
        &extent(3.0, 2.0),
        &normal
    )
    .is_none());
}

#[test]
fn incomplete_and_non_unique_transitions_retain_the_native_selection() {
    let normal = ExtrudeDirection::ProfileNormal {};
    let empty = AsmHistoricalTopology::default;
    assert!(
        select_with_ambiguity(empty(), prism(-2.0, 3.0), &extent(3.0, 2.0), &normal, true)
            .is_none()
    );
    let mut incomplete = prism(-2.0, 3.0);
    incomplete.face_loops.retain(|row| row.owner_ref != 0);
    assert!(select(empty(), incomplete, &extent(3.0, 2.0), &normal).is_none());
    assert!(select(
        prism(-2.0, 3.0),
        prism(-2.0, 3.0),
        &extent(3.0, 2.0),
        &normal
    )
    .is_none());
    let mut outside = prism(-1.0, 1.0);
    for row in &mut outside.point_positions {
        row.position.x *= 1.5;
    }
    for row in &mut outside.surface_planes {
        row.origin.x *= 1.5;
    }
    for row in &mut outside.curve_axes {
        row.origin.x *= 1.5;
        row.direction.x *= 1.5;
    }
    assert!(select(outside, prism(-2.0, 3.0), &extent(3.0, 2.0), &normal).is_none());
}

#[test]
fn a_join_checks_deleted_faces_and_inserted_replacements() {
    let normal = ExtrudeDirection::ProfileNormal {};
    let before = || {
        let mut topology = prism(-1.0, 1.0);
        for face in &mut topology.faces {
            *face += 10;
        }
        for binding in &mut topology.face_surfaces {
            binding.entity += 10;
        }
        for loops in &mut topology.face_loops {
            loops.owner_ref += 10;
        }
        topology
    };
    assert!(select(before(), prism(-2.0, 3.0), &extent(3.0, 2.0), &normal).is_some());
    let mut outside = before();
    outside.surface_planes[0].origin.z = -4.0;
    assert!(select(outside, prism(-2.0, 3.0), &extent(3.0, 2.0), &normal).is_none());
}

#[test]
fn positive_blind_sides_can_certify_either_geometric_interval() {
    let empty = AsmHistoricalTopology::default;
    assert!(select(
        empty(),
        prism(2.0, 3.0),
        &extent(3.0, 2.0),
        &ExtrudeDirection::ProfileNormal {}
    )
    .is_some());
    assert!(select(
        empty(),
        prism(-3.0, -2.0),
        &extent(3.0, 2.0),
        &ExtrudeDirection::ReversedProfileNormal {}
    )
    .is_some());
    assert!(select(
        empty(),
        prism(-3.0, 2.0),
        &extent(3.0, 2.0),
        &ExtrudeDirection::ReversedProfileNormal {}
    )
    .is_some());
}

#[test]
fn ambiguous_end_offsets_retain_the_native_selection() {
    assert!(select(
        AsmHistoricalTopology::default(),
        prism(-TOLERANCE / 4.0, 3.0),
        &extent(3.0, TOLERANCE / 4.0),
        &ExtrudeDirection::ProfileNormal {},
    )
    .is_none());
}

#[test]
fn updated_face_membership_requires_unchanged_analytic_boundaries() {
    use crate::history_records::{
        AsmHistoricalEntityDelta, AsmHistoricalRowsDelta, AsmHistoricalTopologyDelta,
        AsmHistoricalTransition,
    };
    let before = prism(-1.0, 1.0);
    let transition = AsmHistoricalTransition {
        geometry_unchanged: false,
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta {
            updated: vec![2],
            ..Default::default()
        },
        topology: AsmHistoricalTopologyDelta::default(),
        record_revisions: Vec::new(),
        rows: AsmHistoricalRowsDelta::default(),
    };
    assert!(unchanged_face(&before, &before, &transition, 2));
    assert!(!unchanged_face(&before, &prism(-1.0, 2.0), &transition, 2));
    let mut changed_curve = before.clone();
    changed_curve.curve_axes[8].direction.x += 1.0;
    assert!(!unchanged_face(&before, &changed_curve, &transition, 2));
    let mut changed_surface = before.clone();
    changed_surface.surface_planes[2].origin.y = 0.5;
    assert!(!unchanged_face(&before, &changed_surface, &transition, 2));
    let mut revised_edge = transition;
    revised_edge.records.updated.push(308);
    assert!(!unchanged_face(&before, &before, &revised_edge, 2));
}

#[test]
fn extrude_offset_start_resolves_regions_between_both_end_caps() {
    let extent = ExtrudeExtent::OneSided {
        side: ExtrudeSide {
            termination: LinearTermination::Blind {
                length: NonZeroLength::new(1.0).unwrap(),
            },
            draft: None,
        },
    };
    for sign in [1.0, -1.0] {
        let start = ExtrudeStart::OffsetProfilePlane {
            offset: cadmpeg_ir::scalar::Length::new(sign * 2.0).unwrap(),
        };
        let direction = if sign > 0.0 {
            ExtrudeDirection::ProfileNormal {}
        } else {
            ExtrudeDirection::ReversedProfileNormal {}
        };
        let selected = select_from(
            AsmHistoricalTopology::default(),
            prism((sign * 2.0).min(sign * 3.0), (sign * 2.0).max(sign * 3.0)),
            &extent,
            &direction,
            false,
            &start,
        )
        .unwrap();
        let PlanarProfileRef::SketchRegions { regions, .. } = selected else {
            panic!("sketch regions")
        };
        assert_eq!(regions.len(), 2);
        assert!(select_from(
            AsmHistoricalTopology::default(),
            prism(0.0, 3.0),
            &extent,
            &direction,
            false,
            &start
        )
        .is_none());
    }
}
