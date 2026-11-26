// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::design::profile_select::{
    bind_extrude_profile_selections, SketchCurveSelectionResolution,
};
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use serde_json::json;

/// The profile that an Extrude with a native profile on the fixture group
/// binds from `selections`, and its count of
/// `feature.profile-member-reference-lost` notes.
fn bind(selections: &[DesignProfileCurveSelection]) -> (ProfileRef, usize) {
    let (group, placements, _, curves, entities, sketches) = two_circle_profile_fixture();
    let scope = DesignParameterScope::empty("stream:scope#7", DesignScopePayload::Extrude(None), 7);
    let mut feature: Feature = cadmpeg_core::serde_erase::json::from_value(json!({
        "id": "synthetic:test:feature#lost-member", "ordinal": 0, "suppressed": false,
        "native_ref": scope.id,
        "definition": {"kind": "operation", "value": {"kind": "extrude", "value": {
            "profile": {"kind": "planar", "value": {"kind": "native", "value": group.id}},
            "direction": {"kind": "profile_normal"}, "start": {"kind": "profile_plane"},
            "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 1.0}}},
            "op": "new_body", "solid": true}}},
        "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}))
    .unwrap();
    let groups = [group];
    let scopes = [scope];
    let losses = crate::test_support::with_decode_context(|ctx| {
        let work = ProfileGeometryWork::new();
        bind_extrude_profile_selections(
            std::slice::from_mut(&mut feature),
            &scopes,
            &[],
            &[],
            &sketches,
            &SketchCurveSelectionResolution {
                scopes: &scopes,
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
        .unwrap()
    });
    let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) =
        feature.evaluation.definition()
    else {
        panic!("extrude");
    };
    let lost = losses
        .iter()
        .filter(|loss| {
            loss.code == crate::loss::F3dLossCode::FeatureProfileMemberReferenceLost.kind()
        })
        .count();
    (profile.clone(), lost)
}

fn regions(profiles: &[u32]) -> ProfileRef {
    let (_, placements, ..) = two_circle_profile_fixture();
    ProfileRef::Planar(
        PlanarProfileRef::sketch_regions(
            neutral_sketch_id(&placements[0]),
            profiles
                .iter()
                .map(|&profile| SketchProfileRegion::loops(profile, vec![]).unwrap())
                .collect(),
        )
        .unwrap(),
    )
}

#[test]
fn a_lost_member_joins_the_complete_group_before_the_live_members() {
    // Both members name Sketch curves: the complete group resolves and keeps
    // the lost member, which carries its note.
    let mut selections = [selection(10, 0, 11, true), selection(11, 1, 12, true)];
    selections[1].lost_reference_records = vec![120];
    assert_eq!(bind(&selections), (regions(&[0, 1]), 1));

    // The lost member names an absent curve: the live member alone resolves.
    selections[1].groups[0].region[0].entries[0].primary_id = 99;
    assert_eq!(bind(&selections), (regions(&[0]), 1));

    // Every member is lost and names an absent curve: the profile stays
    // native, and each member carries its note.
    selections[0].groups[0].region[0].entries[0].primary_id = 98;
    selections[0].lost_reference_records = vec![121];
    let (profile, lost) = bind(&selections);
    assert!(matches!(
        profile,
        ProfileRef::Planar(PlanarProfileRef::Native(_))
    ));
    assert_eq!(lost, 2);

    // A live member that does not resolve leaves the profile native, and the
    // lost member still carries its note.
    selections[0].lost_reference_records.clear();
    let (profile, lost) = bind(&selections);
    assert!(matches!(
        profile,
        ProfileRef::Planar(PlanarProfileRef::Native(_))
    ));
    assert_eq!(lost, 1);
}
