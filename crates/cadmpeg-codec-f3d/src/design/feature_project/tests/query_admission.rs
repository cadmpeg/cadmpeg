// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn revolve_scope_index_refuses_actual_operations_and_releases_storage() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use cadmpeg_ir::features::{
        Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation,
        PartialRevolveConstruction, PlanarProfileRef, RevolveConstruction,
    };
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#100",
        DesignFeatureKind::Revolve,
        100,
    );
    let feature = Feature {
        id: crate::ids::neutral_feature_id(&scope),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Revolve {
                construction: RevolveConstruction::Unresolved(PartialRevolveConstruction::Axis {
                    profile: PlanarProfileRef::Native("f3d:test:profile#1".into()),
                    extent: None,
                    solid: None,
                    face_maker: None,
                    fuse_order: None,
                    allow_multi_profile_faces: None,
                }),
                op: cadmpeg_ir::features::BooleanOp::Join,
            },
        )),
        native_ref: Some(scope.id.clone()),
    };
    let scopes = [
        DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#99",
            DesignFeatureKind::Sketch,
            99,
        ),
        scope,
    ];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let mut features = [feature.clone()];
        super::super::bind_revolve_face_axes(
            ctx,
            &mut features,
            &scopes,
            &[],
            &[],
            &[],
            &[],
            &[],
        )?;
        assert_eq!(features[0], feature);
        Ok::<_, CodecError>(())
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1 << 20;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        run(ctx).unwrap();
        let _released = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "released Revolve scope index",
            )
            .unwrap();
        assert!(ctx.resource_refusal().is_none());
    });
    for operation in [
        "visit F3D Revolve axis feature",
        "f3d Revolve scope index scan",
        "f3d Revolve scope index lookup",
        "f3d Revolve scope index insertion",
        "f3d Revolve scope lookup",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            run,
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
    }
}
