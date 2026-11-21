// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::extents::{
    ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, LinearTermination,
};
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::profiles::{PlanarProfileRef, SketchProfileRegion};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::NonZeroLength;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    SketchPlacement, SketchProfiles,
};

use super::super::cap_regions::compact::{tests::prism_over, Sweep};
use super::super::ScopedExtrudeProfileResolution;
use super::certified_regions_bounded_by;
use crate::design::geometry::ProfileGeometryWork;
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
use crate::loss::F3dLossCode;

const TOLERANCE: f64 = 1.0e-6;

fn xy_sketch() -> Sketch {
    Sketch {
        id: SketchId::mint("synthetic:test:sketch#stored-loop-cap").unwrap(),
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
    }
}

/// The sides of the rectangle from the origin to `(2, 1)`, a line from its
/// bottom to its top that divides it into two squares, and a line beside it.
fn divided_rectangle() -> Vec<SketchEntity> {
    let sketch = xy_sketch();
    [
        ("bottom", (0.0, 0.0), (2.0, 0.0)),
        ("right", (2.0, 0.0), (2.0, 1.0)),
        ("top", (2.0, 1.0), (0.0, 1.0)),
        ("left", (0.0, 1.0), (0.0, 0.0)),
        ("divider", (1.0, 0.0), (1.0, 1.0)),
        ("beside", (3.0, 0.0), (3.0, 1.0)),
    ]
    .iter()
    .map(|(name, start, end)| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:curve#{name}")).unwrap(),
            sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            })
            .unwrap(),
        )
    })
    .collect()
}

/// The region set certified for the stored curves `stored` when a Join of
/// length 2 from the sketch plane adds the prism over the whole rectangle.
fn select(entities: &[SketchEntity], stored: &[usize]) -> Option<Vec<SketchProfileRegion>> {
    let stored = stored.iter().map(|index| vec![*index]).collect::<Vec<_>>();
    select_candidates(entities, &stored).map(|(regions, _)| regions)
}

fn select_candidates(
    entities: &[SketchEntity],
    stored: &[Vec<usize>],
) -> Option<(Vec<SketchProfileRegion>, Vec<F3dLossCode>)> {
    let states = [(2, prism_over(2.0)), (1, AsmHistoricalTopology::default())]
        .into_iter()
        .map(|(state_id, topology)| {
            (
                AsmDeltaState {
                    id: format!("synthetic:test:state#{state_id}"),
                    parent: "synthetic:test:history#stored-loop-cap".into(),
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
                Vec::new(),
            )
        })
        .collect();
    let history = crate::test_support::history::history(
        "synthetic:test:history#stored-loop-cap".into(),
        0,
        None,
        false,
        states,
    );
    let stored: Vec<_> = stored
        .iter()
        .map(|pieces| {
            pieces
                .iter()
                .map(|index| entities[*index].id().clone())
                .collect::<Vec<_>>()
        })
        .collect();
    let selected = crate::test_support::with_decode_context(|ctx| {
        let work = ProfileGeometryWork::new();
        let resolution = ScopedExtrudeProfileResolution {
            entities,
            spatial_entities: &[],
            histories: std::slice::from_ref(&history),
            linear_tolerance: TOLERANCE,
            angular_tolerance: TOLERANCE,
            profile_work: &work,
            ctx,
        };
        certified_regions_bounded_by(
            &xy_sketch(),
            &stored,
            resolution,
            (2, 1),
            Sweep {
                operation: &BooleanOp::Join,
                start: &ExtrudeStart::ProfilePlane {},
                direction: &ExtrudeDirection::ProfileNormal {},
                extent: &ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: NonZeroLength::new(2.0).unwrap(),
                        },
                        draft: None,
                    },
                },
            },
        )
    })
    .unwrap();
    selected.map(|(profile, losses)| {
        (
            match profile {
                PlanarProfileRef::SketchRegions { regions, .. } => regions.to_vec(),
                _ => panic!("sketch regions"),
            },
            losses,
        )
    })
}

#[test]
fn a_cap_that_a_curve_outside_the_stored_loop_divides_selects_each_region() {
    let entities = divided_rectangle();
    let (regions, losses) = select_candidates(&entities, &[vec![0], vec![1], vec![2], vec![3]])
        .expect("certified regions");
    assert!(losses.is_empty());
    assert_eq!(regions.len(), 2);
    let divider = entities[4].id();
    for region in &regions {
        let SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } = region
        else {
            panic!("trimmed region");
        };
        assert!(hole_boundaries.is_empty());
        assert!(outer_boundary.iter().any(|use_| use_.entity == *divider));
    }
}

#[test]
fn a_stored_curve_that_bounds_no_certified_region_selects_nothing() {
    let entities = divided_rectangle();
    assert!(select(&entities, &[0, 1, 2, 3, 5]).is_none());
}

fn split_rectangle() -> Vec<SketchEntity> {
    let mut entities = divided_rectangle();
    for (name, low, high) in [("right-outside", 1.0, 2.0), ("right-far", 2.0, 3.0)] {
        entities.push(SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:curve#{name}")).unwrap(),
            xy_sketch().id,
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(2.0, low),
                end: Point2::new(2.0, high),
            })
            .unwrap(),
        ));
    }
    entities
}

#[test]
fn a_split_identity_uses_its_piece_on_the_certified_boundary() {
    let entities = split_rectangle();
    let (regions, losses) = select_candidates(&entities, &[vec![0], vec![1, 6], vec![2], vec![3]])
        .expect("certified boundary piece");
    assert_eq!(losses, vec![F3dLossCode::FeatureProfilePartialEntryAssumed]);
    assert_eq!(regions.len(), 2);
    assert!(regions.iter().any(|region| matches!(region,
        SketchProfileRegion::Trimmed { outer_boundary, .. }
            if outer_boundary.iter().any(|use_| use_.entity == *entities[1].id()))));
    assert!(regions.iter().all(|region| matches!(region,
        SketchProfileRegion::Trimmed { outer_boundary, .. }
            if outer_boundary.iter().all(|use_| use_.entity != *entities[6].id()))));
}

#[test]
fn a_split_identity_without_a_boundary_piece_stays_native() {
    let entities = split_rectangle();
    assert!(
        select_candidates(&entities, &[vec![0], vec![1], vec![2], vec![3], vec![6, 7]],).is_none()
    );
}

#[test]
fn an_entry_without_decoded_pieces_stays_native() {
    let entities = divided_rectangle();
    assert!(
        select_candidates(&entities, &[vec![0], vec![1], vec![2], vec![3], Vec::new()],).is_none()
    );
}

#[test]
fn a_reversed_coincident_boundary_curve_uses_the_certified_interval() {
    let mut entities = divided_rectangle();
    entities.push(SketchEntity::new(
        SketchEntityId::mint("synthetic:test:curve#right-alias").unwrap(),
        xy_sketch().id,
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(2.0, 1.0),
            end: Point2::new(2.0, 0.0),
        })
        .unwrap(),
    ));
    let (regions, losses) =
        select_candidates(&entities, &[vec![0], vec![1], vec![2], vec![3], vec![6]])
            .expect("coincident boundary");
    assert_eq!(
        losses,
        vec![
            F3dLossCode::FeatureProfilePartialEntryAssumed,
            F3dLossCode::FeatureProfileCoincidentBoundaryAssumed,
        ]
    );
    assert_eq!(regions.len(), 2);
}

#[test]
fn counted_member_boundary_certificate_keeps_an_unsampled_conic() {
    use cadmpeg_ir::features::SketchProfileBoundaryUse;
    use cadmpeg_ir::geometry::DirectedParameterRange;
    use cadmpeg_ir::scalar::{Angle, Length};

    let sketch = xy_sketch();
    let entities = [
        SketchEntity::new(
            SketchEntityId::mint("synthetic:test:curve#parabola").unwrap(),
            sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Parabola {
                vertex: Point2::new(0.0, 0.0),
                axis_angle: Angle::new(0.0).unwrap(),
                focal_length: Length::new(1.0).unwrap(),
                bounds: Some([-2.0, 2.0]),
            })
            .unwrap(),
        ),
        SketchEntity::new(
            SketchEntityId::mint("synthetic:test:curve#chord").unwrap(),
            sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(1.0, 2.0),
                end: Point2::new(1.0, -2.0),
            })
            .unwrap(),
        ),
    ];
    let boundary = entities
        .iter()
        .zip([[-2.0, 2.0], [0.0, 1.0]])
        .map(|(entity, range)| SketchProfileBoundaryUse {
            entity: entity.id().clone(),
            parameter_range: DirectedParameterRange::new(range).unwrap(),
            reversed: true,
        })
        .collect::<Vec<_>>();
    let candidate = PlanarProfileRef::sketch_regions(
        sketch.id.clone(),
        vec![SketchProfileRegion::trimmed(boundary.clone(), Vec::new()).unwrap()],
    )
    .unwrap();
    let named = entities
        .iter()
        .map(|entity| vec![entity.id().clone()])
        .collect::<Vec<_>>();
    crate::test_support::with_decode_context(|ctx| {
        assert!(
            crate::design::geometry::boundary_use_samples(&boundary[0], &entities, 8, ctx,)
                .unwrap()
                .is_none()
        );
        let work = ProfileGeometryWork::new();
        let resolution = ScopedExtrudeProfileResolution {
            entities: &entities,
            spatial_entities: &[],
            histories: &[],
            linear_tolerance: TOLERANCE,
            angular_tolerance: TOLERANCE,
            profile_work: &work,
            ctx,
        };
        let (selected, losses) = super::certify_boundaries(candidate.clone(), &named, resolution)
            .expect("each counted member bounds the certified region");
        assert_eq!(selected, candidate);
        assert!(losses.is_empty());
    });
}
