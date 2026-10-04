//! Resource admission for surface sweep profile projection.

use super::super::project_surface_sweep_profiles;
use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::history::resolved_feature_classes_with_ids;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureOperation, PlanarProfileRef, SweepMode, SweepSection, SweepShape,
};
use std::io::Cursor;

fn surface_sweep_projection_error(policy: DecodePolicy) -> cadmpeg_core::CodecError {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
        <Feature Name="Helix1" Type="Helix/Spiral" id="119"/>
        <Feature Name="Surface-Sweep1" Type="Surface-Sweep" id="137"/>
    </Keywords>"#,
    ));
    let mut resolved = resolved_feature_classes_with_ids(&[
        ("moHelix_c", "Helix1", 119),
        ("moSweepRefSurface_c", "Surface-Sweep1", 137),
    ]);
    resolved.extend_from_slice(&[0xdd, 0x94, 0xff, 0xff, 1, 0]);
    let class = b"moCompReferenceCurve_c";
    resolved.extend_from_slice(
        &u16::try_from(class.len())
            .expect("class name length fits u16")
            .to_le_bytes(),
    );
    resolved.extend_from_slice(class);
    let prefix = resolved.len();
    resolved.resize(prefix + 133, 0);
    resolved[prefix..prefix + 10].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0]);
    resolved[prefix + 45..prefix + 61].fill(0xff);
    let reference = prefix + 81;
    resolved[reference..reference + 4].copy_from_slice(&119u32.to_le_bytes());
    resolved[reference + 4..reference + 8].copy_from_slice(&0x5edf_5674u32.to_le_bytes());
    resolved[reference + 16..reference + 20].copy_from_slice(&0x65u32.to_le_bytes());
    resolved[reference + 24..reference + 28].fill(0xff);
    for offset in [reference + 32, reference + 36, reference + 40] {
        resolved[offset..offset + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    }
    resolved[reference + 48..reference + 52].copy_from_slice(&[0xf8, 0x2a, 0, 0]);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native =
        crate::native::SldprtNative::load(decoded.ir().native.namespace("sldprt").unwrap())
            .unwrap();
    let mut features = decoded.ir().model.features.clone();
    for feature in &mut features {
        if matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sweep { .. })
        ) {
            feature.evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) =
                    definition
                {
                    *shape = SweepShape::sheet_sections(
                        SweepMode::Surface {},
                        SweepSection::Unresolved(None),
                        Vec::new(),
                    );
                }
            });
            feature.dependencies.clear();
        }
    }
    let arena = DecodeArena::new();
    let (service, _) =
        DecodeContext::from_root_bytes(&resolved, &arena, &DecodePolicy::service()).unwrap();
    let mut admitted = features.clone();
    project_surface_sweep_profiles(
        &service,
        &mut admitted,
        &native.feature_histories,
        &native.feature_input_lanes,
    )
    .unwrap();
    let helix = admitted
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Helix1"))
        .unwrap();
    let sweep = admitted
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Surface-Sweep1"))
        .unwrap();
    assert!(
        matches!(sweep.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. })
        if matches!(shape.referenced_profile(), Some(PlanarProfileRef::Feature(id)) if id == &helix.id))
    );
    assert!(sweep.dependencies.contains(&helix.id));
    let (ctx, _) = DecodeContext::from_root_bytes(&resolved, &arena, &policy).unwrap();
    project_surface_sweep_profiles(
        &ctx,
        &mut features,
        &native.feature_histories,
        &native.feature_input_lanes,
    )
    .unwrap_err()
}

#[test]
fn surface_sweep_projection_refuses_collection_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    assert!(
        matches!(surface_sweep_projection_error(policy), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems)
    );
}

#[test]
fn surface_sweep_projection_refuses_retained_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    assert!(
        matches!(surface_sweep_projection_error(policy), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes)
    );
}

#[test]
fn surface_sweep_projection_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    assert!(
        matches!(surface_sweep_projection_error(policy), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits)
    );
}
