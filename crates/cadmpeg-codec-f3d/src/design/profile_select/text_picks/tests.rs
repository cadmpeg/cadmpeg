// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::features::PlanarProfileRef;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::loss::LossKind;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchFontWeight, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};

use super::text_glyph_selection;
use crate::design::geometry::ProfileGeometryWork;
use crate::design::profile_select::ScopedExtrudeProfileResolution;
use crate::loss::F3dLossCode;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::references::DesignClassTag;
use crate::records::sketch_relations::SketchRelationOperand;
use crate::records::topology::extrude_selection::{
    DesignExtrudeSelectionGroup, DesignExtrudeSelectionGroupWire, DesignExtrudeSelectionMember,
    DesignExtrudeSelectionMemberDraft,
};

const TOLERANCE: f64 = 1.0e-6;
const GROUP_RECORD: u32 = 9;

fn sketch() -> Sketch {
    Sketch {
        id: SketchId::mint("synthetic:test:sketch#text-picks").unwrap(),
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

fn entity(name: &str, definition: SketchGeometryDefinition) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:entity#{name}")).unwrap(),
        sketch().id,
        SketchGeometry::try_from(definition).unwrap(),
    )
}

/// A plate from the origin to `(10, 5)` with a hole of radius 1 at
/// `(8, 2.5)`, which bound two regions, and a text `content` on the plate.
fn plate_with_text(content: &str, construction_text: bool) -> Vec<SketchEntity> {
    let mut entities: Vec<_> = [
        ("bottom", (0.0, 0.0), (10.0, 0.0)),
        ("right", (10.0, 0.0), (10.0, 5.0)),
        ("top", (10.0, 5.0), (0.0, 5.0)),
        ("left", (0.0, 5.0), (0.0, 0.0)),
    ]
    .iter()
    .map(|(name, start, end)| {
        entity(
            name,
            SketchGeometryDefinition::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            },
        )
    })
    .collect();
    entities.push(entity(
        "hole",
        SketchGeometryDefinition::Circle {
            center: Point2::new(8.0, 2.5),
            radius: Length::new(1.0).unwrap(),
        },
    ));
    let mut text = entity(
        "text",
        SketchGeometryDefinition::Text {
            text: NonBlankString::new(content).unwrap(),
            font_family: NonBlankString::new("sans").unwrap(),
            font_weight: SketchFontWeight::Regular,
            height: Length::new(2.0).unwrap(),
            width_factor: None,
            placement: None,
            horizontal_alignment: None,
            vertical_alignment: None,
        },
    );
    text.construction = construction_text;
    entities.push(text);
    entities
}

fn group(count: u32) -> DesignExtrudeSelectionGroup {
    let members: Vec<u32> = (0..count).map(|ordinal| 100 + ordinal).collect();
    let member_offsets: Vec<u64> = (0..u64::from(count))
        .map(|ordinal| 37 + 11 * ordinal)
        .collect();
    let opaque_index_offset = 36 + 11 * u64::from(count);
    DesignExtrudeSelectionGroup::try_from(DesignExtrudeSelectionGroupWire {
        id: format!("f3d:Design/BulkStream.dat:selection-group#{GROUP_RECORD}"),
        scope_record_index: 7,
        scope_reference_ordinal: 0,
        record_index: GROUP_RECORD,
        byte_offset: 0,
        class_tag: "322".to_owned(),
        member_count_offset: 32,
        members,
        member_offsets,
        opaque_index: 1,
        opaque_index_offset,
        opaque_scalar: 0.0,
        opaque_scalar_offset: opaque_index_offset + 4,
        variant: Some(false),
        typed_references: false,
        paired_class_tag: "263".to_owned(),
        paired_byte_offset: opaque_index_offset + 53,
    })
    .unwrap()
}

fn members(count: u32) -> Vec<DesignExtrudeSelectionMember> {
    (0..count)
        .map(|ordinal| {
            let record_index = 100 + ordinal;
            DesignExtrudeSelectionMember::try_new(DesignExtrudeSelectionMemberDraft {
                id: format!("f3d:Design/BulkStream.dat:selection-member#{record_index}"),
                group_record_index: GROUP_RECORD,
                group_member_ordinal: ordinal,
                record_index,
                byte_offset: 0,
                class_tag: DesignClassTag::try_from("299".to_owned()).unwrap(),
                local_id: u64::from(40 + ordinal),
                local_id_offset: 21,
                asset_id: DesignRelaxedGuidText::try_from(
                    "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 33,
                context_id: DesignRelaxedGuidText::try_from(
                    "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
                )
                .unwrap(),
                context_id_offset: 109,
                tail_slot_present: false,
                tail_slot_offset: 0,
                resolved_geometry: None,
                operand_identity_ids: Vec::new(),
                historical: None,
                scope_tail: None,
                next_record_index: record_index + 1,
                next_byte_offset: 190,
            })
            .unwrap()
        })
        .collect()
}

fn select(
    entities: &[SketchEntity],
    group: &DesignExtrudeSelectionGroup,
    members: &[DesignExtrudeSelectionMember],
) -> Option<(PlanarProfileRef, LossKind)> {
    crate::test_support::with_decode_context(|ctx| {
        let work = ProfileGeometryWork::new();
        let scoped = ScopedExtrudeProfileResolution {
            entities,
            spatial_entities: &[],
            histories: &[],
            linear_tolerance: TOLERANCE,
            angular_tolerance: TOLERANCE,
            profile_work: &work,
            ctx,
        };
        text_glyph_selection("feature#1", &sketch(), &[group], members, scoped)
    })
    .unwrap()
    .map(|(profile, note)| (profile, note.code))
}

#[test]
fn one_pick_per_glyph_beyond_the_carried_regions_names_the_text() {
    let entities = plate_with_text("NO BREAD", false);
    let (profile, code) = select(&entities, &group(7), &members(7)).expect("text profile");
    assert_eq!(
        profile,
        PlanarProfileRef::sketch_entities(sketch().id, vec![entities[5].id().clone()]).unwrap()
    );
    assert_eq!(code, F3dLossCode::FeatureProfileTextGlyphsAssumed.kind());
}

#[test]
fn picks_beyond_the_carried_regions_that_miss_the_glyph_count_stay_native() {
    let entities = plate_with_text("NO BREAD", false);
    let group = group(3);
    let (profile, code) = select(&entities, &group, &members(3)).expect("native profile");
    assert_eq!(profile, PlanarProfileRef::Native(group.id.clone()));
    assert_eq!(code, F3dLossCode::FeatureProfileTextGlyphsNative.kind());
}

#[test]
fn picks_within_the_carried_regions_leave_the_selection_to_the_region_rules() {
    let entities = plate_with_text("NO BREAD", false);
    assert!(select(&entities, &group(2), &members(2)).is_none());
}

#[test]
fn construction_text_bounds_no_pick() {
    let entities = plate_with_text("NO BREAD", true);
    assert!(select(&entities, &group(7), &members(7)).is_none());
}

#[test]
fn a_curve_pick_leaves_the_selection_to_the_curve_rules() {
    let entities = plate_with_text("NO BREAD", false);
    let mut members = members(7);
    members[0].resolved_geometry = Some(SketchRelationOperand::Curve {
        record_index: 1,
        primary_id: 2,
        secondary_id: 0,
    });
    assert!(select(&entities, &group(7), &members).is_none());
}
