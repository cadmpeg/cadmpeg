// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::design::profile_select::{
    bind_extrude_profile_selections, SketchCurveSelectionResolution,
};
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef,
};
use serde_json::json;

#[test]
fn compact_members_with_missing_geometry_retain_the_selection_for_every_profile_count() {
    for profile_count in 0..=2 {
        let sketch_id = SketchId::mint("synthetic:test:sketch#compact-members").unwrap();
        let profiles = (0..profile_count)
            .map(|i| {
                vec![SketchEntityUse {
                    entity: SketchEntityId::mint(format!("synthetic:test:entity#{i}")).unwrap(),
                    reversed: false,
                }]
            })
            .collect::<Vec<_>>();
        let sketch = Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::Unresolved {},
            profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(profiles).unwrap(),
            native_ref: None,
        };
        let scope =
            DesignParameterScope::empty("stream:scope#7", DesignScopePayload::Extrude(None), 7);
        let mut selection = selection(10, 0, 99, true);
        selection.groups[0].region[0].entries[0].primary_id_offset -= 40;
        selection.identity_end_offset -= 40;
        assert!(selection.compact_references());
        assert!(selection.valid_layout());
        let selections = [selection];
        let mut feature: Feature = serde_json::from_value(json!({
            "id": "synthetic:test:feature#compact", "ordinal": 0, "suppressed": false,
            "native_ref": scope.id,
            "definition": {
                "definition": "extrude", "profile": {"kind":"sketch", "value":sketch_id},
                "direction":{"kind":"profile_normal"}, "start":{"kind":"profile_plane"},
                "extent":{"kind":"one_sided","side":{"termination":{"kind":"blind","length":1.0}}},
                "op":"new_body", "solid":true
            }
        }))
        .unwrap();
        crate::test_support::with_decode_context(|ctx| {
            let sketches = [sketch];
            let scopes = [scope];
            let work = ProfileGeometryWork::new();
            bind_extrude_profile_selections(
                std::slice::from_mut(&mut feature),
                &scopes,
                &[],
                &[],
                &sketches,
                &SketchCurveSelectionResolution {
                    scopes: &scopes,
                    groups: &[],
                    operands: &[],
                    profile_curves: &selections,
                    placements: &[],
                    curve_identities: &[],
                    sketches: &sketches,
                    sketch_entities: &[],
                    spatial_sketches: &[],
                    spatial_sketch_entities: &[],
                },
                ExtrudeProfileResolution {
                    entities: &[],
                    spatial_sketches: &[],
                    spatial_entities: &[],
                    histories: &[],
                    scope_histories: &HashMap::new(),
                    linear_tolerance: 1.0e-7,
                    angular_tolerance: 1.0e-9,
                    profile_work: &work,
                    ctx,
                },
            )
            .unwrap();
        });
        let FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::SketchSelection { sketch, selections }),
            ..
        }) = feature.evaluation.definition()
        else {
            panic!("native compact selection")
        };
        assert_eq!(sketch, &sketch_id);
        assert_eq!(selections.as_slice(), &["stream:scope#7".to_owned()]);
    }
}

#[test]
fn a_complete_compact_loop_can_name_the_single_sketch_profile() {
    use crate::records::feature::scope::{DesignExtrudeScope, DesignScopePayload};
    use crate::records::topology::sketch_profile::DesignSketchProfileOperand;

    let placements = [placement()];
    let sketch_id = neutral_sketch_id(&placements[0]);
    let curves = [curve(31, 11, 0), curve(32, 12, 0)];
    let entities = [(11, 1.0), (12, 2.0)].map(|(primary, radius)| {
        SketchEntity::new(
            neutral_sketch_curve_id(&sketch_id, primary, 0),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length::new(radius).unwrap(),
            })
            .unwrap(),
        )
    });
    let sketches = [Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        native_ref: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity: entities[0].id().clone(),
            reversed: false,
        }]])
        .unwrap(),
    }];
    let operand: DesignSketchProfileOperand = serde_json::from_value(json!({
        "scope_reference_ordinal": 0, "record_index": 10, "byte_offset": 10,
        "class_tag": "343", "asset_id": "01234567-89ab-cdef-0123-456789abcdef",
        "asset_id_offset": 20, "entity_id": "Sketch_42", "entity_suffix": 42,
        "entity_reference_offset": 100, "paired_class_tag": "265", "paired_byte_offset": 200,
        "compact_tail_flags": [false, false]
    }))
    .unwrap();
    let scope = DesignParameterScope::empty(
        "stream:scope#7",
        DesignScopePayload::Extrude(Some(DesignExtrudeScope {
            extrude_profile: Some(operand),
            ..Default::default()
        })),
        7,
    );
    let mut group = group();
    group
        .try_set_members(group.members()[..1].to_vec())
        .unwrap();
    group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::PROFILE,
        );
    let groups = [group];
    let mut compact = selection(10, 0, 11, true);
    compact.groups[0].region[0].entries[0].primary_id_offset -= 40;
    compact.identity_end_offset -= 40;
    let resolve = |selections: &[DesignProfileCurveSelection]| {
        super::super::super::compact_profile_loops(
            &scope,
            &sketches[0],
            &SketchCurveSelectionResolution {
                scopes: &[],
                groups: &groups,
                operands: &[],
                profile_curves: selections,
                placements: &placements,
                curve_identities: &curves,
                sketches: &sketches,
                sketch_entities: &entities,
                spatial_sketches: &[],
                spatial_sketch_entities: &[],
            },
        )
    };
    assert_eq!(
        resolve(std::slice::from_ref(&compact)),
        Some(ProfileRef::Planar(
            PlanarProfileRef::sketch_profiles(sketch_id, vec![0]).unwrap()
        ))
    );
    compact.groups[0].region[0].entries[0].primary_id = 12;
    assert!(resolve(std::slice::from_ref(&compact)).is_none());
    compact.groups[0].region[0].entries[0].primary_id = 11;
    compact.groups[0].region[0].entries[0].entry_fields[1] = 2;
    assert!(resolve(std::slice::from_ref(&compact)).is_none());
}
