// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    SpatialSketch, SpatialSketchEntity, SpatialSketchEntityUse, SpatialSketchGeometry,
    SpatialSketchGeometryDefinition, SpatialSketchId, SpatialSketchProfile,
};
use serde_json::json;

use super::{
    certified_profiles, distinct_profiles, stored_region_profiles, SpatialFrame,
    SpatialFrameProfiles,
};
use crate::ids::neutral_spatial_sketch_curve_id;
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;

const STREAM: &str = "f3d:Design/BulkStream.dat";
const TOLERANCE: f64 = 1.0e-6;
const OWNER: u32 = 42;

/// A curve as its persistent identity pair and geometry.
type Curve = ((u64, u64), SpatialSketchGeometryDefinition);

fn line(start: (f64, f64), end: (f64, f64)) -> SpatialSketchGeometryDefinition {
    SpatialSketchGeometryDefinition::Line {
        start: Point3::new(start.0, start.1, 0.0),
        end: Point3::new(end.0, end.1, 0.0),
    }
}

fn circle(center: (f64, f64), radius: f64) -> SpatialSketchGeometryDefinition {
    SpatialSketchGeometryDefinition::Circle {
        center: Point3::new(center.0, center.1, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        reference_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: Length::new(radius).unwrap(),
    }
}

/// A square of curves 1 through 4, a circle 5 and a coincident circle 6
/// drawn again, and four projected lines with secondary identities that bound
/// no profile. Profiles: the square, circle 5, circle 6.
fn fixture() -> (
    SpatialSketch,
    Vec<SpatialSketchEntity>,
    Vec<SketchCurveIdentity>,
) {
    let id = SpatialSketchId::mint("f3d:model:spatial-sketch#fixture").unwrap();
    let curves: Vec<Curve> = vec![
        ((1, 0), line((0.0, 0.0), (10.0, 0.0))),
        ((2, 0), line((10.0, 0.0), (10.0, 10.0))),
        ((3, 0), line((10.0, 10.0), (0.0, 10.0))),
        ((4, 0), line((0.0, 10.0), (0.0, 0.0))),
        ((5, 0), circle((20.0, 5.0), 2.0)),
        ((6, 0), circle((20.0, 5.0), 2.0)),
        ((7, 70), line((2.0, 2.0), (4.0, 2.0))),
        ((8, 80), line((4.0, 2.0), (4.0, 4.0))),
        ((9, 90), line((4.0, 4.0), (2.0, 4.0))),
        ((10, 100), line((2.0, 4.0), (2.0, 2.0))),
    ];
    let mut entities = Vec::new();
    let mut identities = Vec::new();
    for ((primary, secondary), geometry) in curves {
        entities.push(SpatialSketchEntity::new(
            neutral_spatial_sketch_curve_id(&id, primary, secondary),
            id.clone(),
            SpatialSketchGeometry::try_from(geometry).unwrap(),
        ));
        identities.push(SketchCurveIdentity {
            id: format!("{STREAM}:curve#{primary}"),
            record_index: u32::try_from(primary).unwrap(),
            owner_reference: Some(OWNER),
            geometry_group: None,
            class_tag: "300".to_owned().try_into().unwrap(),
            byte_offset: 0,
            geometry_offset: 0,
            entity_genesis: None,
            primary_id: std::num::NonZeroU64::new(primary).unwrap(),
            secondary_id: secondary,
            geometry: None,
            planar_z_noise: None,
            tail_marker: None,
        });
    }
    let profile = |primaries: &[u64]| {
        SpatialSketchProfile::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            primaries
                .iter()
                .map(|primary| SpatialSketchEntityUse {
                    entity: neutral_spatial_sketch_curve_id(&id, *primary, 0),
                    reversed: false,
                })
                .collect(),
        )
        .unwrap()
    };
    let sketch = SpatialSketch {
        id: id.clone(),
        name: None,
        configuration: None,
        visible: None,
        native_ref: None,
        profiles: vec![profile(&[1, 2, 3, 4]), profile(&[5]), profile(&[6])],
    };
    (sketch, entities, identities)
}

/// A frame whose nested selection has, per selected region, the rings of
/// `(primary, secondary)` members.
fn operand(regions: &[Vec<Vec<(u32, u32)>>]) -> DesignSketchProfileOperand {
    let mut loops = Vec::new();
    for (ordinal, rings) in regions.iter().enumerate() {
        for ring in rings {
            let members: Vec<_> = ring
                .iter()
                .map(|(primary, secondary)| {
                    json!({
                        "kind": 3,
                        "kind_offset": 0,
                        "curve_primary_id": primary,
                        "curve_primary_id_offset": 4,
                        "incidence_words": [0, secondary, 0, 0, 1, 1, 0, 0],
                        "incidence_words_offset": 8
                    })
                })
                .collect();
            loops.push(json!({
                "member_count_offset": 340,
                "members": members,
                "region_ordinal": ordinal
            }));
        }
    }
    json::from_value(json!({
        "scope_reference_ordinal": 0,
        "record_index": 100,
        "byte_offset": 10,
        "class_tag": "317",
        "asset_id": "01234567-89ab-cdef-0123-456789abcdef",
        "asset_id_offset": 20,
        "entity_id": format!("Sketch_{OWNER}"),
        "entity_suffix": OWNER,
        "entity_reference_offset": 100,
        "paired_class_tag": "268",
        "paired_byte_offset": 200,
        "region_selection": {
            "record_index": 103,
            "byte_offset": 300,
            "class_tag": "310",
            "region_count_offset": 336,
            "regions": loops,
            "companion_class_tag": "257",
            "companion_byte_offset": 700
        }
    }))
    .unwrap()
}

fn frame<'a>(
    operand: &'a DesignSketchProfileOperand,
    sketch: &'a SpatialSketch,
    entities: &'a [SpatialSketchEntity],
    identities: &'a [SketchCurveIdentity],
) -> SpatialFrame<'a> {
    SpatialFrame {
        stream: STREAM,
        operand,
        sketch,
        curve_identities: identities,
        entities,
        histories: &[],
        state_ids: None,
        linear_tolerance: TOLERANCE,
        angular_tolerance: 1.0e-9,
    }
}

#[test]
fn a_ring_that_names_one_profile_boundary_selects_that_profile() {
    let (sketch, entities, identities) = fixture();
    let operand = operand(&[
        vec![vec![(3, 0), (1, 0), (4, 0), (2, 0)]],
        vec![vec![(6, 0)]],
    ]);
    let frame = frame(&operand, &sketch, &entities, &identities);
    assert_eq!(stored_region_profiles(&frame), Some(vec![0, 2]));
}

#[test]
fn a_selected_region_with_a_hole_ring_stays_unresolved() {
    let (sketch, entities, identities) = fixture();
    // The square with the projected inner square as its hole.
    let operand = operand(&[vec![
        vec![(1, 0), (2, 0), (3, 0), (4, 0)],
        vec![(7, 70), (8, 80), (9, 90), (10, 100)],
    ]]);
    let frame = frame(&operand, &sketch, &entities, &identities);
    assert_eq!(stored_region_profiles(&frame), None);
}

#[test]
fn a_ring_of_curves_in_no_profile_stays_unresolved() {
    let (sketch, entities, identities) = fixture();
    let operand = operand(&[vec![vec![(7, 70), (8, 80), (9, 90), (10, 100)]]]);
    let frame = frame(&operand, &sketch, &entities, &identities);
    assert_eq!(stored_region_profiles(&frame), None);
}

#[test]
fn a_member_names_a_curve_by_its_primary_and_secondary_identity() {
    let (sketch, entities, identities) = fixture();
    // Curve 7 has secondary identity 70; a member that stores 71 names no curve,
    // and a member that stores zero names the curve by its primary identity.
    let operand = operand(&[vec![vec![(7, 71), (8, 80), (9, 0)]]]);
    let members = &operand.region_selection.as_ref().unwrap().regions[0].members;
    let named = |index: usize| {
        super::super::spatial_profile_member_entity(
            STREAM,
            OWNER,
            &members[index],
            &sketch,
            &identities,
            &entities,
        )
        .map(|entity| entity.id().clone())
    };
    assert_eq!(named(0), None);
    assert_eq!(
        named(1),
        Some(neutral_spatial_sketch_curve_id(&sketch.id, 8, 80))
    );
    assert_eq!(
        named(2),
        Some(neutral_spatial_sketch_curve_id(&sketch.id, 9, 90))
    );
}

#[test]
fn a_ring_of_coincident_circles_names_the_profile_of_its_first_member() {
    let (sketch, entities, identities) = fixture();
    let operand = operand(&[vec![vec![(6, 0), (5, 0)]], vec![vec![(5, 0), (6, 0)]]]);
    let frame = frame(&operand, &sketch, &entities, &identities);
    assert_eq!(stored_region_profiles(&frame), Some(vec![2, 1]));
    assert_eq!(distinct_profiles(&frame, &[2, 1]), Some(vec![2]));
}

#[test]
fn a_ring_that_names_part_of_a_profile_boundary_stays_unresolved() {
    let (sketch, entities, identities) = fixture();
    let operand = operand(&[vec![vec![(1, 0), (2, 0), (3, 0)]]]);
    let frame = frame(&operand, &sketch, &entities, &identities);
    assert_eq!(stored_region_profiles(&frame), None);
}

#[test]
fn a_profile_drawn_twice_is_named_once() {
    let (sketch, entities, identities) = fixture();
    let operand = operand(&[vec![vec![(1, 0), (2, 0), (3, 0), (4, 0)]]]);
    let frame = frame(&operand, &sketch, &entities, &identities);
    assert_eq!(distinct_profiles(&frame, &[0, 1, 2]), Some(vec![0, 1]));
    assert_eq!(distinct_profiles(&frame, &[2, 1, 0]), Some(vec![2, 0]));
}

#[test]
fn a_stored_ring_without_a_transition_is_stated_as_assumed() {
    let (sketch, entities, identities) = fixture();
    let operand = operand(&[vec![vec![(1, 0), (2, 0), (3, 0), (4, 0)]]]);
    let frame = frame(&operand, &sketch, &entities, &identities);
    assert_eq!(
        certified_profiles(&frame, None),
        Some(SpatialFrameProfiles::Assumed(vec![0]))
    );
}

#[test]
fn stored_groups_without_a_transition_are_stated_as_assumed() {
    let (sketch, entities, identities) = fixture();
    let mut operand = operand(&[]);
    operand.region_selection = None;
    let frame = frame(&operand, &sketch, &entities, &identities);
    // Profile 2 traces the same circle as profile 1 and is named once.
    assert_eq!(
        certified_profiles(&frame, Some(vec![1, 2])),
        Some(SpatialFrameProfiles::Assumed(vec![1]))
    );
}

#[test]
fn an_unresolved_ring_without_a_transition_stays_native() {
    let (sketch, entities, identities) = fixture();
    let operand = operand(&[vec![vec![(1, 0), (2, 0), (3, 0)]]]);
    let frame = frame(&operand, &sketch, &entities, &identities);
    assert_eq!(certified_profiles(&frame, None), None);
}

#[test]
fn the_one_profile_of_a_sketch_without_a_transition_is_stated_as_assumed() {
    let (mut sketch, entities, identities) = fixture();
    sketch.profiles.truncate(1);
    let mut operand = operand(&[]);
    operand.region_selection = None;
    let frame = frame(&operand, &sketch, &entities, &identities);
    assert_eq!(
        certified_profiles(&frame, None),
        Some(SpatialFrameProfiles::Assumed(vec![0]))
    );
}

#[test]
fn every_profile_of_a_sketch_without_a_transition_stays_native() {
    let (sketch, entities, identities) = fixture();
    let mut operand = operand(&[]);
    operand.region_selection = None;
    let frame = frame(&operand, &sketch, &entities, &identities);
    assert_eq!(certified_profiles(&frame, None), None);
}

#[test]
fn assumed_spatial_transitions_report_one_loss_counting_selections() {
    use crate::loss::F3dLossCode;
    let work = crate::design::geometry::ProfileGeometryWork::new();
    work.note_spatial_transition_assumed();
    work.finish_selection();
    work.finish_selection();
    work.note_spatial_transition_assumed();
    work.finish_selection();
    let losses = work.losses();
    let [loss] = losses.as_slice() else {
        panic!("expected one loss, got {losses:?}");
    };
    assert_eq!(
        loss.code,
        F3dLossCode::FeatureSpatialProfileTransitionAssumed.kind()
    );
    assert!(loss.message.starts_with("2 extrude profile selection(s)"));
}

#[test]
fn a_selected_region_with_more_than_one_loop_has_holes() {
    let one_loop = operand(&[
        vec![vec![(1, 0), (2, 0), (3, 0), (4, 0)]],
        vec![vec![(5, 0)]],
    ]);
    assert!(super::regions_without_holes(
        one_loop.region_selection.as_ref().unwrap()
    ));
    let holed = operand(&[vec![
        vec![(1, 0), (2, 0), (3, 0), (4, 0)],
        vec![(7, 70), (8, 80), (9, 90), (10, 100)],
    ]]);
    assert!(!super::regions_without_holes(
        holed.region_selection.as_ref().unwrap()
    ));
}

#[test]
fn only_one_distinct_profile_leaves_a_single_candidate() {
    let (mut sketch, entities, identities) = fixture();
    let operand = operand(&[vec![vec![(5, 0)]]]);
    // The square and the circle drawn twice are two distinct regions.
    let both = frame(&operand, &sketch, &entities, &identities);
    assert_eq!(distinct_profiles(&both, &[0, 1, 2]), Some(vec![0, 1]));
    // The circle drawn twice alone is one region.
    sketch.profiles.remove(0);
    let circles = frame(&operand, &sketch, &entities, &identities);
    assert_eq!(distinct_profiles(&circles, &[0, 1]), Some(vec![0]));
}
