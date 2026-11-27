// SPDX-License-Identifier: Apache-2.0

use super::super::region_members::resolve_later_curve_boundary;
use super::super::ScopedExtrudeProfileResolution;
use super::{curve, placement, profile_region_member};
use crate::design::geometry::ProfileGeometryWork;
use crate::ids::{neutral_sketch_curve_id, neutral_sketch_id};
use crate::records::topology::sketch_profile::{
    DesignSketchProfileOperand, DesignSketchProfileOperandDraft, DesignSketchProfileRegion,
    DesignSketchProfileRegionSelection,
};
use cadmpeg_ir::features::profiles::PlanarProfileRef;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchGeometry, SketchGeometryDefinition, SketchPlacement, SketchProfiles,
};

const STREAM: &str = "stream";

/// What the later curve of the fixture looks like to the resolver.
struct Later {
    primary_id: u64,
    owner: u32,
    tail_marker: Option<u8>,
}

impl Default for Later {
    fn default() -> Self {
        Self {
            primary_id: 30,
            owner: 42,
            tail_marker: None,
        }
    }
}

fn operand(named: &[u32]) -> DesignSketchProfileOperand {
    let tag =
        |text: &str| crate::records::references::DesignClassTag::try_from(text.to_owned()).unwrap();
    DesignSketchProfileOperand::try_new(DesignSketchProfileOperandDraft {
        sketch_absent: false,
        scope_reference_ordinal: 0,
        record_index: 10,
        byte_offset: 0,
        class_tag: tag("300"),
        asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
        )
        .unwrap(),
        asset_id_offset: 32,
        entity_id: placement().entity_id.clone(),
        entity_reference_offset: 80,
        compact_tail_flags: None,
        region_selection: Some(DesignSketchProfileRegionSelection {
            atomic_regions: Vec::new(),
            record_index: 13,
            byte_offset: 0,
            class_tag: tag("303"),
            region_count_offset: 0,
            regions: vec![DesignSketchProfileRegion {
                member_count_offset: 0,
                region_ordinal: 0,
                hole: false,
                members: named
                    .iter()
                    .map(|primary| profile_region_member(*primary))
                    .collect(),
            }],
            companion_class_tag: tag("304"),
            companion_byte_offset: 0,
        }),
        paired_class_tag: tag("301"),
        paired_byte_offset: 160,
    })
    .unwrap()
}

/// A square of curves 21 to 24 whose right side stops at height one,
/// closed by the later curve, resolved for a member list naming `named`.
fn resolve(named: &[u32], later: Later) -> (Option<PlanarProfileRef>, Vec<String>) {
    let placement = placement();
    let sketch = Sketch {
        id: neutral_sketch_id(&placement),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::default(),
        native_ref: None,
    };
    let specs = [
        (21u64, (0.0, 0.0), (2.0, 0.0)),
        (22, (2.0, 0.0), (2.0, 1.0)),
        (23, (2.0, 2.0), (0.0, 2.0)),
        (24, (0.0, 2.0), (0.0, 0.0)),
        (later.primary_id, (2.0, 1.0), (2.0, 2.0)),
    ];
    let mut curves = Vec::new();
    let mut entities = Vec::new();
    for (index, (primary, start, end)) in specs.iter().enumerate() {
        let mut identity = curve(u32::try_from(index).unwrap() + 100, *primary, 0);
        if index == 4 {
            identity.owner_reference = Some(later.owner);
            identity.tail_marker = later.tail_marker;
        }
        let mut entity = SketchEntity::new(
            neutral_sketch_curve_id(&sketch.id, *primary, 0),
            sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            })
            .unwrap(),
        );
        entity.native_ref = Some(identity.id.clone());
        curves.push(identity);
        entities.push(entity);
    }
    let profile = operand(named);
    crate::test_support::with_decode_context(|ctx| {
        let work = ProfileGeometryWork::new();
        let resolution = ScopedExtrudeProfileResolution {
            entities: &entities,
            spatial_entities: &[],
            histories: &[],
            linear_tolerance: 1.0e-7,
            angular_tolerance: 1.0e-9,
            profile_work: &work,
            ctx,
        };
        let selected =
            resolve_later_curve_boundary(STREAM, &profile, &sketch, &curves, &entities, resolution)
                .unwrap();
        work.finish_selection();
        let losses = work
            .losses()
            .into_iter()
            .map(|loss| loss.code.to_string())
            .collect();
        (selected, losses)
    })
}

fn assumed_loss() -> String {
    crate::loss::F3dLossCode::FeatureProfileLaterCurveBoundaryAssumed
        .kind()
        .to_string()
}

#[test]
fn the_face_with_a_later_curve_is_selected_and_reports_the_assumption() {
    let (selected, losses) = resolve(&[21, 22, 23, 24], Later::default());
    let Some(PlanarProfileRef::SketchRegions { regions, .. }) = selected else {
        panic!("sketch regions");
    };
    assert_eq!(regions.len(), 1);
    assert_eq!(losses, vec![assumed_loss()]);
}

#[test]
fn a_curve_with_an_identity_below_a_named_identity_is_not_later() {
    let (selected, losses) = resolve(
        &[21, 22, 23, 24],
        Later {
            primary_id: 5,
            ..Later::default()
        },
    );
    assert!(selected.is_none());
    assert!(losses.is_empty());
}

#[test]
fn a_curve_of_another_owner_is_not_later() {
    let (selected, losses) = resolve(
        &[21, 22, 23, 24],
        Later {
            owner: 7,
            ..Later::default()
        },
    );
    assert!(selected.is_none());
    assert!(losses.is_empty());
}

#[test]
fn a_tail_marker_one_curve_is_not_later() {
    let (selected, losses) = resolve(
        &[21, 22, 23, 24],
        Later {
            tail_marker: Some(1),
            ..Later::default()
        },
    );
    assert!(selected.is_none());
    assert!(losses.is_empty());
}

#[test]
fn a_member_with_no_curve_identity_leaves_the_selection_native() {
    let (selected, losses) = resolve(&[21, 22, 23, 99], Later::default());
    assert!(selected.is_none());
    assert!(losses.is_empty());
}

#[test]
fn a_list_naming_the_later_curve_has_nothing_unnamed_to_assume() {
    let (selected, losses) = resolve(&[21, 22, 23, 24, 30], Later::default());
    assert!(selected.is_none());
    assert!(losses.is_empty());
}
