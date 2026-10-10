// SPDX-License-Identifier: Apache-2.0

use crate::feature::definitions::{
    DecodedField, FeatureSavedEntity, FeatureSavedSection, FeatureSavedSpline,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn spline() -> FeatureSavedEntity {
    FeatureSavedEntity::Spline(FeatureSavedSpline {
        entity_id: None,
        declared_point_count: Some(2),
        interpolation_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        interpolation_points_body: Vec::new(),
        endpoint_tangents: Some(DecodedField {
            value: [[1.0, 0.0, 0.0]; 2],
            body: Vec::new(),
        }),
        parameters: Some(DecodedField {
            value: vec![0.0, 1.0],
            body: Vec::new(),
        }),
        offset: 64,
    })
}

#[test]
fn saved_identity_spline_geometry_is_released_before_the_next_record() {
    let mut definition = super::definition(None);
    definition.saved_section = Some(FeatureSavedSection {
        entities: vec![spline()],
        offset: 0,
    });
    let run = |definition: &crate::feature::definitions::FeatureDefinition, cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let ids =
            super::super::materialized_saved_section_external_ids(&ctx, definition, &mut refusal)?;
        assert!(ids.is_empty());
        assert!(refusal.take_records_checked()?.is_empty());
        Ok::<_, cadmpeg_core::CodecError>(ids)
    };
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        None,
        |cap| run(&definition, cap),
    );
    definition
        .saved_section
        .as_mut()
        .expect("saved section")
        .entities = vec![spline(); 16];
    assert!(run(&definition, cap)
        .expect("only one spline geometry is live at a time")
        .is_empty());
}
