// SPDX-License-Identifier: Apache-2.0
//! Registry-driven validation of every typed entity reference.

use super::record_finding;
use crate::document::CadIr;
use crate::index::identities::BorrowedIdentities;
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use crate::schema::EntitySchema;

pub(super) fn check_typed_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    index: &BorrowedIdentities<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), cadmpeg_core::CodecError> {
    macro_rules! check_arenas {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
            $(for entity in &ir.model.$field {
                let owner = entity.identity();
                let walk = entity.visit_references(ctx, &mut |target| {
                    if index.contains(target) { return Ok(()); }
                    for finding in findings.iter() {
                        if finding.check != Check::ReferentialIntegrity { continue; }
                        if finding.entity.as_deref() != Some(owner) { continue; }
                        if finding.message.contains(target) { return Ok(()); }
                    }
                    record_finding(findings, Check::ReferentialIntegrity, Severity::Error, Some(owner), format_args!("unresolved typed reference {target}"));
                    Ok(())
                });
                match walk {
                    Ok(()) => {}
                    Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Err(error),
                    Err(error) => record_finding(findings, Check::ReferentialIntegrity, Severity::Error, Some(owner), format_args!("entity schema cannot state its typed references: {error}")),
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
        let index = ModelIndex::build(&ir);
        let identities = super::BorrowedIdentities::build(|add| {
            for id in index.identities(crate::index::StandardIndex) {
                add(id, ())?;
            }
            Ok(())
        })
        .unwrap();
        check_typed_references(&ctx, &ir, &identities, &mut findings).unwrap();
        assert!(findings.iter().any(|finding| {
            finding.check == Check::ReferentialIntegrity
                && finding.severity == Severity::Error
                && finding.entity.as_deref() == Some(owner.as_str())
                && finding.message == format!("unresolved typed reference {missing}")
        }));
    }

    #[test]
    fn typed_reference_validation_propagates_depth_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let mut ir = crate::CadIr::empty();
        ir.model.vertices.push(crate::topology::Vertex {
            id: "test:model:vertex#owner".try_into().unwrap(),
            point: "test:model:point#missing".try_into().unwrap(),
            tolerance: None,
        });
        let identities = super::BorrowedIdentities::new();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        let Err(CodecError::ResourceLimit(limit)) =
            check_typed_references(&ctx, &ir, &identities, &mut findings)
        else {
            panic!("typed reference validation must refuse");
        };
        assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
        assert!(findings.is_empty());
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }

    #[test]
    fn typed_reference_validation_borrows_targets_and_keeps_duplicate_suppression() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let mut ir = crate::CadIr::empty();
        let target = "test:model:body#missing";
        let owner = "test:model:product#owner";
        ir.model
            .product_definitions
            .push(crate::products::ProductDefinition {
                id: owner.try_into().unwrap(),
                kind: crate::products::ProductDefinitionKind::Part,
                source_name: Some("test:model:name#plain".into()),
                label: None,
                description: None,
                part_number: None,
                bom_properties: std::collections::BTreeMap::new(),
                bodies: vec![target.try_into().unwrap(), target.try_into().unwrap()],
                native_ref: None,
            });
        let identities = super::BorrowedIdentities::new();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        check_typed_references(&ctx, &ir, &identities, &mut findings).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].check, Check::ReferentialIntegrity);
        assert_eq!(findings[0].severity, Severity::Error);
        assert_eq!(findings[0].entity.as_deref(), Some(owner));
        assert_eq!(
            findings[0].message,
            format!("unresolved typed reference {target}")
        );
        check_typed_references(&ctx, &ir, &identities, &mut findings).unwrap();
        assert_eq!(findings.len(), 1);
        ctx.finish_session().unwrap();
    }
}
