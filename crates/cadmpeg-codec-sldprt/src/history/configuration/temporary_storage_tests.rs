// SPDX-License-Identifier: Apache-2.0

use super::{inherit_configuration_reference_plane_definition, ConfigurationDefinitions};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DatumPlaneReference, DistinctMembers, Feature, FeatureContent, FeatureDefinition,
    FeatureEvaluation, FeatureId, FeatureOperation, PrincipalPlane,
};
use cadmpeg_ir::scalar::Length;
use std::collections::BTreeMap;

fn feature(id: FeatureId, definition: FeatureDefinition) -> Feature {
    Feature {
        id,
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(definition),
        native_ref: None,
    }
}

fn fixture() -> [Feature; 2] {
    let plane = FeatureId::mint(format!("synthetic:test:id#{}", "p".repeat(512))).unwrap();
    [
        feature(
            plane.clone(),
            FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane {
                plane: PrincipalPlane::Top,
            }),
        ),
        feature(
            FeatureId::mint("synthetic:test:id#offset").unwrap(),
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::Feature { feature: plane }),
                distance: Length::new(2.0).unwrap(),
            }),
        ),
    ]
}

fn first_table_storage<T>() -> u64 {
    // Core's first hash growth uses four buckets, sixteen trailing control
    // bytes and max(alignment, 16)-1 padding bytes.
    u64::try_from(4 * std::mem::size_of::<T>() + 4 + 16 + std::mem::align_of::<T>().max(16) - 1)
        .unwrap()
}

fn definition_storage() -> u64 {
    first_table_storage::<(&FeatureId, &FeatureDefinition)>()
}

fn output_peak(features: &[Feature; 2]) -> u64 {
    // Two exact identity copies survive: the reference and the dependency.
    // The first dependency Vec growth retains four FeatureId slots. The
    // definitions table is still live; the visited set has already expired.
    definition_storage()
        + 2 * u64::try_from(features[0].id.as_str().len()).unwrap()
        + u64::try_from(4 * std::mem::size_of::<FeatureId>()).unwrap()
}

fn inherit(ctx: &DecodeContext<'_>, features: &[Feature; 2]) -> Result<(), CodecError> {
    let base = ConfigurationDefinitions::new(ctx, features)?;
    let mut output_storage = ctx.reserve_scoped(0, "SLDPRT configuration feature projection")?;
    let mut definition = FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
        reference: None,
        distance: Length::new(7.0).unwrap(),
    });
    let mut dependencies = DistinctMembers::default();
    output_storage.with_storage(|| {
        inherit_configuration_reference_plane_definition(
            ctx,
            &features[1].id,
            &mut definition,
            &mut dependencies,
            &base,
        )
    })?;
    assert_eq!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature {
                feature: features[0].id.clone(),
            }),
            distance: Length::new(7.0).unwrap(),
        }),
    );
    assert_eq!(dependencies.as_slice(), &[features[0].id.clone()]);
    Ok(())
}

#[test]
fn configuration_datum_traversal_expires_before_scoped_output_copy() {
    let features = fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = output_peak(&features);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    inherit(&ctx, &features).unwrap();
    assert!(ctx.resource_refusal().is_none());
}

fn assert_refusal(cap: u64, operation: &'static str) {
    let features = fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = inherit(&ctx, &features).unwrap_err() else {
        panic!("expected configuration materialized refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.limit, cap);
    assert_eq!(ctx.resource_refusal().as_ref(), Some(&limit));
    let CodecError::ResourceLimit(replayed) = inherit(&ctx, &features).unwrap_err() else {
        panic!("expected original configuration refusal");
    };
    assert_eq!(replayed, limit);
}

#[test]
fn configuration_datum_scoped_output_refuses_one_byte_below_peak() {
    assert_refusal(
        output_peak(&fixture()) - 1,
        "retain SLDPRT configuration datum dependency",
    );
}

#[test]
fn configuration_datum_traversal_refuses_its_actual_live_table_peak() {
    assert_refusal(
        definition_storage() + first_table_storage::<&FeatureId>() - 1,
        "resolve SLDPRT configuration datum frame",
    );
}
