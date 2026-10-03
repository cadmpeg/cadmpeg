// SPDX-License-Identifier: Apache-2.0
//! Registry-driven validation of every typed entity reference.

use crate::document::CadIr;
use crate::index::identities::BorrowedIdentities;
use super::record_finding;
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use crate::schema::EntitySchema;

pub(super) fn check_typed_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    index: &BorrowedIdentities<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), cadmpeg_core::CodecError> {
    macro_rules! check_arenas {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
            $(for entity in &ir.model.$field {
                ctx.charge_work(1, "typed reference entity scan")?;
                let owner = entity.identity();
                let walk = entity.visit_references(ctx, &mut |target| {
                    if index.contains(ctx, target)? { return Ok(()); }
                    for finding in findings.iter() {
                        ctx.charge_work(1, "typed reference finding scan")?;
                        if finding.check != Check::ReferentialIntegrity { continue; }
                        ctx.charge_work(cadmpeg_core::decode::u64_from_index(owner.len()), "compare typed reference finding owner")?;
                        if finding.entity.as_deref() != Some(owner) { continue; }
                        ctx.charge_work(cadmpeg_core::decode::u64_from_index(finding.message.len()), "scan typed reference finding message")?;
                        ctx.charge_work(cadmpeg_core::decode::u64_from_index(target.len()), "scan typed reference finding target")?;
                        if finding.message.contains(target) { return Ok(()); }
                    }
                    record_finding(ctx, findings, Check::ReferentialIntegrity, Severity::Error,
                        Some(owner), format_args!("unresolved typed reference {target}"))
                });
                match walk {
                    Ok(()) => {}
                    Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Err(error),
                    Err(error) => record_finding(ctx, findings, Check::ReferentialIntegrity, Severity::Error,
                        Some(owner), format_args!("entity schema cannot state its typed references: {error}"))?,
                }
            })*
        };
    }
    crate::document::arena_registry!(check_arenas);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_typed_references;
    use crate::assets::AssetId;
    use crate::examples::unit_cube;
    use crate::index::ModelIndex;
    use crate::math::Point3;
    use crate::report::{check::Check, Severity};
    use crate::tessellation::{Tessellation, TessellationMesh, TessellationTextureAssignment};

    #[test]
    fn unresolved_asset_reference_is_reported() {
        let missing = AssetId::mint("synthetic:test:asset#missing").expect("valid identity");
        let tessellation = Tessellation::new(
            crate::tessellation::TessellationId::mint("synthetic:test:tessellation#textured")
                .expect("valid identity"),
            TessellationMesh::List {
                vertices: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .expect("valid tessellation")
        .with_texture_assignments(vec![TessellationTextureAssignment {
            source_id: None,
            texture: missing.clone(),
            triangles: vec![0],
        }])
        .expect("valid local texture assignment");
        let owner = tessellation.id.clone();
        let mut ir = unit_cube().expect("valid unit cube fixture");
        ir.model.tessellations.push(tessellation);
        let mut findings = Vec::new();
        let ctx = cadmpeg_test_support::service_decode_context();
        let index = ModelIndex::new(&ir, crate::index::StandardIndex);
        let identities = super::BorrowedIdentities::build(&ctx, |add| {
            for id in index.identities(&ctx) { add(id?, ())?; }
            Ok(())
        }).unwrap();
        check_typed_references(&ctx, &ir, &identities, &mut findings).unwrap();
        assert!(findings.iter().any(|finding| {
            finding.check == Check::ReferentialIntegrity
                && finding.severity == Severity::Error
                && finding.entity.as_deref() == Some(owner.as_str())
                && finding.message == format!("unresolved typed reference {missing}")
        }));
    }

    #[test]
    fn typed_reference_validation_preserves_outer_resource_refusals() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let mut ir = crate::CadIr::empty();
        ir.model.vertices.push(crate::topology::Vertex {
            id: "test:model:vertex#owner".try_into().unwrap(),
            point: "test:model:point#missing".try_into().unwrap(), tolerance: None,
        });
        let source = cadmpeg_test_support::service_decode_context();
        let identities = super::BorrowedIdentities::build(&source, |_| Ok(())).unwrap();
        for dimension in [ResourceDimension::WorkUnits, ResourceDimension::RecursionDepth,
            ResourceDimension::CollectionItems, ResourceDimension::RetainedBytes] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut findings = Vec::new();
            let Err(CodecError::ResourceLimit(limit)) = check_typed_references(&ctx, &ir, &identities, &mut findings) else { panic!("typed reference validation must refuse"); };
            assert_eq!(limit.dimension, dimension);
            assert!(findings.is_empty());
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
        }
    }

    #[test]
    fn typed_reference_validation_borrows_targets_and_keeps_duplicate_suppression() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let mut ir = crate::CadIr::empty();
        let target = "test:model:body#missing";
        let owner = "test:model:product#owner";
        ir.model.product_definitions.push(crate::products::ProductDefinition {
            id: owner.try_into().unwrap(), kind: crate::products::ProductDefinitionKind::Part,
            source_name: Some("test:model:name#plain".into()), label: None,
            description: None, part_number: None, bom_properties: std::collections::BTreeMap::new(),
            bodies: vec![target.try_into().unwrap(), target.try_into().unwrap()], native_ref: None,
        });
        let source = cadmpeg_test_support::service_decode_context();
        let identities = super::BorrowedIdentities::build(&source, |_| Ok(())).unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        check_typed_references(&ctx, &ir, &identities, &mut findings).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].check, Check::ReferentialIntegrity);
        assert_eq!(findings[0].severity, Severity::Error);
        assert_eq!(findings[0].entity.as_deref(), Some(owner));
        assert_eq!(findings[0].message, format!("unresolved typed reference {target}"));
        check_typed_references(&ctx, &ir, &identities, &mut findings).unwrap();
        assert_eq!(findings.len(), 1);
        ctx.finish_session().unwrap();
    }

}
