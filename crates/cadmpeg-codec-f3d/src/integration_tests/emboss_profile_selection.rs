// SPDX-License-Identifier: Apache-2.0
//! Emboss sketch-profile operands bind neutral Wrap profiles.

use std::collections::HashMap;

use cadmpeg_ir::features::{
    FaceSelection, Feature, FeatureDefinition, FeatureOperation, PlanarProfileRef, WrapMode,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchGeometry, SketchPlacement};
use serde_json::json;

use crate::design::geometry::ProfileGeometryWork;
use crate::design::profile_select::emboss::bind_emboss_profile_selections;
use crate::design::profile_select::{ExtrudeProfileResolution, SketchCurveSelectionResolution};
use crate::loss::F3dLossCode;
use crate::records::feature::emboss::{DesignEmbossConstruction, DesignEmbossControlWords};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayload};
use crate::records::identity::{Located, ReferenceRun};
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::sketch_placement::{
    DesignSketchFrame, DesignSketchFrameForm, DesignSketchPlacement,
};
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;

const STREAM: &str = "f3d:Design1/BulkStream.dat";
const REGION_TOLERANCE: f64 = 1.0e-7;
const ANGULAR_TOLERANCE: f64 = 1.0e-9;

/// A circle of radius 0.5 inside a 2 x 2 square, and a 4 x 1 bar whose long
/// sides cross the square: loops `[circle]`, `[square]` and `[bar]`.
fn geometry() -> Vec<serde_json::Value> {
    vec![
        json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 0.5}),
        json!({"kind": "line", "start": {"u": -1.0, "v": -1.0}, "end": {"u": 1.0, "v": -1.0}}),
        json!({"kind": "line", "start": {"u": 1.0, "v": -1.0}, "end": {"u": 1.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": 1.0, "v": 1.0}, "end": {"u": -1.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": -1.0, "v": 1.0}, "end": {"u": -1.0, "v": -1.0}}),
    ]
}

fn placement() -> DesignSketchPlacement {
    let matrix = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 10.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
    .try_into()
    .unwrap();
    DesignSketchPlacement {
        id: format!("{STREAM}:sketch-placement#0"),
        scope_record_index: Some(30),
        entity_id: "0_42".to_owned().try_into().unwrap(),
        display_name: None,
        visibility: None,
        class_tag: "305".to_owned().try_into().unwrap(),
        record_index: 50,
        paired_class_tag: "306".to_owned().try_into().unwrap(),
        frame: DesignSketchFrame::new(0, DesignSketchFrameForm::ScopeExplicit(matrix)).unwrap(),
    }
}

fn sketch() -> (Sketch, Vec<SketchEntity>, Vec<SketchCurveIdentity>) {
    let sketch_id = crate::ids::neutral_sketch_id(&placement());
    let mut entities = Vec::new();
    let mut curves = Vec::new();
    for (index, geometry) in geometry().into_iter().enumerate() {
        let primary = u32::try_from(index + 1).unwrap();
        entities.push(SketchEntity::new(
            crate::ids::neutral_sketch_curve_id(&sketch_id, u64::from(primary), 0),
            sketch_id.clone(),
            serde_json::from_value::<SketchGeometry>(geometry).unwrap(),
        ));
        curves.push(SketchCurveIdentity {
            id: format!("{STREAM}:curve#{primary}"),
            record_index: primary,
            owner_reference: Some(42),
            geometry_group: None,
            class_tag: "300".to_owned().try_into().unwrap(),
            byte_offset: 0,
            geometry_offset: 0,
            entity_genesis: None,
            primary_id: std::num::NonZeroU64::new(u64::from(primary)).unwrap(),
            secondary_id: 0,
            geometry: None,
            planar_z_noise: None,
            tail_marker: None,
        });
    }
    let loops: [&[usize]; 2] = [&[0], &[1, 2, 3, 4]];
    let profiles = loops
        .iter()
        .map(|indices| {
            indices
                .iter()
                .map(|index| json!({"entity": entities[*index].id(), "reversed": false}))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 10.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: serde_json::from_value(json!(profiles)).unwrap(),
        native_ref: None,
    };
    (sketch, entities, curves)
}

/// Record identities of one fixed Emboss scope. The ordered reference table
/// holds them in increasing order: `41, 44, 48, 51, 60, 63, 66, 69, 72`.
#[derive(Clone, Copy)]
struct Records {
    profile_group: u32,
    face_group: u32,
    auxiliary_group: u32,
    /// Depth, Horizontal Distance, Vertical Distance, Rotation Angle.
    owners: [u32; 4],
}

impl Records {
    fn profile_member(self) -> u32 {
        Self::TABLE[Self::ordinal(self.profile_group) + 1]
    }

    fn ordinal(record: u32) -> usize {
        Self::TABLE
            .iter()
            .position(|value| *value == record)
            .unwrap()
    }

    const TABLE: [u32; 9] = [41, 44, 48, 51, 60, 63, 66, 69, 72];
}

/// The selection groups precede the scalar owners.
const PROFILE_FIRST: Records = Records {
    profile_group: 41,
    face_group: 48,
    auxiliary_group: 72,
    owners: [60, 63, 66, 69],
};

/// The scalar owners and the auxiliary group precede the selection groups.
const OWNER_FIRST: Records = Records {
    profile_group: 63,
    face_group: 69,
    auxiliary_group: 60,
    owners: [41, 44, 48, 51],
};

fn scope(members: &[u32]) -> DesignParameterScope {
    scope_in(PROFILE_FIRST, members)
}

/// The fixed Emboss scope with `records`, whose sketch-profile operand
/// selects `members`.
fn scope_in(records: Records, members: &[u32]) -> DesignParameterScope {
    let members = members
        .iter()
        .map(|primary| {
            json!({
                "kind": 3, "kind_offset": 0, "curve_primary_id": primary,
                "curve_primary_id_offset": 4,
                "incidence_words": [0, 0, 0, 0, 1, 1, 0, 0],
                "incidence_words_offset": 8,
            })
        })
        .collect::<Vec<_>>();
    let operand: DesignSketchProfileOperand = serde_json::from_value(json!({
        "scope_reference_ordinal": Records::ordinal(records.profile_member()),
        "record_index": records.profile_member(), "byte_offset": 10,
        "class_tag": "301", "asset_id": "01234567-89ab-cdef-0123-456789abcdef",
        "asset_id_offset": 20, "entity_id": "0_42", "entity_suffix": 42,
        "entity_reference_offset": 100, "paired_class_tag": "259", "paired_byte_offset": 200,
        "region_selection": {
            "record_index": 103, "byte_offset": 300, "class_tag": "302", "region_count_offset": 336,
            "regions": [{"member_count_offset": 340, "members": members}],
            "companion_class_tag": "260", "companion_byte_offset": 700,
        }
    }))
    .unwrap();
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#0"),
        DesignFeatureKind::Emboss,
        40,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 430;
            draft.paired_byte_offset = 430;
            draft.reference_count_offset = 235;
            draft.reference_members = ReferenceRun::unlocated(Records::TABLE.to_vec());
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
            let [depth, horizontal, vertical, rotation] = records.owners;
            draft.payload = DesignScopePayload::Emboss(Some(DesignEmbossConstruction {
                control_words: DesignEmbossControlWords::Two([1, 0]),
                native_pair: Some([17, 23]),
                additional_native_pairs: None,
                depth_owner: Located {
                    value: depth,
                    offset: 29,
                },
                auxiliary_group: Located {
                    value: records.auxiliary_group,
                    offset: 40,
                },
                horizontal_owner: Located {
                    value: horizontal,
                    offset: 51,
                },
                rotation_owner: Located {
                    value: rotation,
                    offset: 82,
                },
                face_group: Located {
                    value: records.face_group,
                    offset: 93,
                },
                vertical_owner: Located {
                    value: vertical,
                    offset: 104,
                },
                profile_group: Located {
                    value: records.profile_group,
                    offset: 119,
                },
                sketch_profile: Some(operand),
            }));
        })
        .unwrap();
    assert!(scope.emboss_operand_construction().is_some());
    scope
}

#[test]
fn table_slots_follow_the_record_order_of_the_reference_table() {
    for (records, slots) in [
        (PROFILE_FIRST, [0, 1, 2, 3, 8]),
        (OWNER_FIRST, [5, 6, 7, 8, 4]),
    ] {
        let slots_found = scope_in(records, &[1]).emboss_table_slots().unwrap();
        assert_eq!(
            [
                slots_found.profile_group,
                slots_found.profile_member,
                slots_found.face_group,
                slots_found.face_member,
                slots_found.auxiliary_group,
            ],
            slots
        );
    }
}

#[test]
fn an_owner_first_table_binds_the_selected_loop() {
    let (profile, losses) = bound(scope_in(OWNER_FIRST, &[1]), &[placement()]);
    let PlanarProfileRef::SketchProfiles { profiles, .. } = &profile else {
        panic!("sketch profiles: {profile:?}");
    };
    assert_eq!(profiles.iter().copied().collect::<Vec<_>>(), [0]);
    assert!(losses.is_empty(), "{losses:?}");
}

fn bound(
    scope: DesignParameterScope,
    placements: &[DesignSketchPlacement],
) -> (PlanarProfileRef, Vec<LossNote>) {
    let (sketch, entities, curves) = sketch();
    let definition = FeatureDefinition::Operation(FeatureOperation::Wrap {
        profile: PlanarProfileRef::Native(scope.id.clone()),
        face: FaceSelection::Native(scope.id.clone()),
        mode: WrapMode::Emboss {
            depth: PositiveLength::new(2.0).unwrap(),
        },
    });
    let mut feature: Feature = serde_json::from_value(json!({
        "id": "synthetic:test:feature#emboss", "ordinal": 0, "suppressed": false,
        "native_ref": scope.id,
        "definition": serde_json::to_value(definition).unwrap(),
        "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}))
    .unwrap();
    let work = ProfileGeometryWork::new();
    let losses = crate::test_support::with_decode_context(|ctx| {
        let sketches = [sketch];
        let scopes = [scope];
        bind_emboss_profile_selections(
            std::slice::from_mut(&mut feature),
            &SketchCurveSelectionResolution {
                scopes: &scopes,
                groups: &[],
                operands: &[],
                profile_curves: &[],
                placements,
                curve_identities: &curves,
                sketches: &sketches,
                sketch_entities: &entities,
                spatial_sketches: &[],
                spatial_sketch_entities: &[],
            },
            ExtrudeProfileResolution {
                entities: &entities,
                spatial_sketches: &[],
                spatial_entities: &[],
                histories: &[],
                scope_histories: &HashMap::new(),
                linear_tolerance: REGION_TOLERANCE,
                angular_tolerance: ANGULAR_TOLERANCE,
                profile_work: &work,
                ctx,
            },
        )
    })
    .unwrap();
    let FeatureDefinition::Operation(FeatureOperation::Wrap { profile, .. }) =
        feature.evaluation.into_parts().0
    else {
        panic!("Wrap");
    };
    (profile, losses)
}

#[test]
fn a_selected_loop_binds_the_matching_sketch_profile() {
    let (profile, losses) = bound(scope(&[1]), &[placement()]);
    let PlanarProfileRef::SketchProfiles { sketch, profiles } = &profile else {
        panic!("sketch profiles: {profile:?}");
    };
    assert_eq!(*sketch, crate::ids::neutral_sketch_id(&placement()));
    assert_eq!(profiles.iter().copied().collect::<Vec<_>>(), [0]);
    assert!(losses.is_empty(), "{losses:?}");

    let (profile, _) = bound(scope(&[2, 3, 4, 5]), &[placement()]);
    let PlanarProfileRef::SketchProfiles { profiles, .. } = &profile else {
        panic!("sketch profiles: {profile:?}");
    };
    assert_eq!(profiles.iter().copied().collect::<Vec<_>>(), [1]);
}

#[test]
fn an_unmatched_selection_keeps_the_native_profile_with_its_loss() {
    for (members, placements) in [(&[99][..], vec![placement()]), (&[1][..], Vec::new())] {
        let (profile, losses) = bound(scope(members), &placements);
        assert!(
            matches!(profile, PlanarProfileRef::Native(_)),
            "{profile:?}"
        );
        let [loss] = losses.as_slice() else {
            panic!("expected one loss, got {losses:?}");
        };
        assert_eq!(loss.code, F3dLossCode::EmbossProfileSelectionNative.kind());
    }
}
