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
