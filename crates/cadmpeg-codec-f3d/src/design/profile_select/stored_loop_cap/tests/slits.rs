// SPDX-License-Identifier: Apache-2.0

use std::f64::consts::TAU;

use super::super::super::ScopedExtrudeProfileResolution;
use super::super::stored_entry_losses;
use super::{divided_rectangle, select_candidates, xy_sketch, TOLERANCE};
use crate::design::geometry::ProfileGeometryWork;
use crate::loss::F3dLossCode;
use cadmpeg_ir::features::profiles::{SketchProfileBoundaryUse, SketchProfileRegion};
use cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition,
};

fn line(name: &str, start: (f64, f64), end: (f64, f64)) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:curve#{name}")).unwrap(),
        xy_sketch().id,
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        })
        .unwrap(),
    )
}

fn circle(name: &str, radius: f64) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:curve#{name}")).unwrap(),
        xy_sketch().id,
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(radius).unwrap(),
        })
        .unwrap(),
    )
}

fn full_turn(entity: &SketchEntity, reversed: bool) -> SketchProfileBoundaryUse {
    SketchProfileBoundaryUse {
        entity: entity.id().clone(),
        parameter_range: DirectedParameterRange::new([0.0, TAU]).unwrap(),
        reversed,
    }
}

/// The losses the stored entries `stored` charge against the two atomic
/// regions of two concentric circles of radius 3 and 1.75: the inner disk
/// and the ring between the circles, which has the inner circle as its hole.
/// `entities` holds the outer circle, the inner circle, and a line.
fn concentric_entry_losses(
    entities: &[SketchEntity],
    stored: &[usize],
) -> Option<Vec<F3dLossCode>> {
    let regions = vec![
        SketchProfileRegion::trimmed(vec![full_turn(&entities[1], false)], Vec::new()).unwrap(),
        SketchProfileRegion::trimmed(
            vec![full_turn(&entities[0], false)],
            vec![vec![full_turn(&entities[1], true)]],
        )
        .unwrap(),
    ];
    let stored = stored
        .iter()
        .map(|index| vec![entities[*index].id().clone()])
        .collect::<Vec<_>>();
    crate::test_support::with_decode_context(|ctx| {
        let work = ProfileGeometryWork::new();
        let resolution = ScopedExtrudeProfileResolution {
            entities,
            spatial_entities: &[],
            histories: &[],
            linear_tolerance: TOLERANCE,
            angular_tolerance: TOLERANCE,
            profile_work: &work,
            ctx,
        };
        stored_entry_losses(&regions, &stored, resolution)
    })
    .unwrap()
}

/// The stored loops of a selection of the whole outer disk: the outer circle
/// with the line, the inner circle, and the outer circle, the inner circle,
/// and the line.
const WHOLE_DISK_LOOPS: [usize; 6] = [0, 2, 1, 0, 1, 2];

#[test]
fn a_line_that_ends_inside_the_inner_circle_is_a_slit_of_the_selected_disk() {
    let entities = [
        circle("outer", 3.0),
        circle("inner", 1.75),
        line("slit", (0.0, -5.2), (0.0, 1.5)),
    ];
    assert_eq!(
        concentric_entry_losses(&entities, &WHOLE_DISK_LOOPS),
        Some(vec![F3dLossCode::FeatureProfilePartialEntryAssumed])
    );
}

#[test]
fn a_line_outside_both_concentric_circles_selects_nothing() {
    let entities = [
        circle("outer", 3.0),
        circle("inner", 1.75),
        line("outside", (0.0, -5.2), (0.0, -4.0)),
    ];
    assert_eq!(concentric_entry_losses(&entities, &WHOLE_DISK_LOOPS), None);
}

#[test]
fn a_stored_line_that_ends_inside_the_certified_cap_is_a_slit() {
    let mut entities = divided_rectangle();
    entities.push(line("spur", (0.5, -0.5), (0.5, 0.5)));
    let (regions, losses) =
        select_candidates(&entities, &[vec![0], vec![1], vec![2], vec![3], vec![6]])
            .expect("certified regions");
    assert_eq!(losses, vec![F3dLossCode::FeatureProfilePartialEntryAssumed]);
    assert_eq!(regions.len(), 2);
    let spur = entities[6].id();
    assert!(regions.iter().all(|region| matches!(region,
        SketchProfileRegion::Trimmed { outer_boundary, hole_boundaries }
            if hole_boundaries.is_empty()
                && outer_boundary.iter().all(|use_| use_.entity != *spur))));
}
