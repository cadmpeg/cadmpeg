// SPDX-License-Identifier: Apache-2.0
//! Registry-driven validation of every typed entity reference.

use super::record_finding;
use crate::document::CadIr;
use crate::index::identities::IdentitySet;
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use crate::schema::{ArenaRecord, EntitySchema};

/// Whether a reference of the visited id type can name a record of an arena
/// the CADIR wire omits.
fn targets_omitted_arena() -> bool {
    use crate::features::{ConfigurationId, FeatureId, ParameterId};
    use crate::ids::{FeatureInputTopologyId, FeatureResultTopologyId, HistoryBodyId};
    use crate::products::JointId;
    use crate::sketches::{
        SketchConstraintId, SketchEntityId, SketchId, SpatialSketchEntityId, SpatialSketchId,
    };
    use crate::spreadsheets::SpreadsheetId;
    use std::any::type_name;
    let omitted = [
        type_name::<FeatureId>(),
        type_name::<FeatureInputTopologyId>(),
        type_name::<FeatureResultTopologyId>(),
        type_name::<ConfigurationId>(),
        type_name::<ParameterId>(),
        type_name::<SketchId>(),
        type_name::<SketchEntityId>(),
        type_name::<SketchConstraintId>(),
        type_name::<SpatialSketchId>(),
        type_name::<SpatialSketchEntityId>(),
        type_name::<SpreadsheetId>(),
        type_name::<JointId>(),
        type_name::<HistoryBodyId>(),
    ];
    crate::schema::rewrite::typed::visited_id_type()
        .is_some_and(|id_type| omitted.contains(&id_type))
}

/// The identities of the records of the `design` home.
fn design_identities(ir: &CadIr) -> std::collections::HashSet<&str> {
    macro_rules! collect {
        (@arena $ids:ident, design, $field:ident) => {
            $ids.extend(ir.design.$field.iter().map(ArenaRecord::record_identity));
        };
        (@arena $ids:ident, $home:ident, $field:ident) => {};
        ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {{
            let mut ids = std::collections::HashSet::new();
            $(collect!(@arena ids, $home, $field);)*
            ids
        }};
    }
    crate::document::every_arena_registry!(collect)
}

/// The positions of the referential-integrity findings of each owner, kept
/// up to date with the findings list on each read.
#[derive(Default)]
struct OwnerFindings {
    by_owner: std::collections::HashMap<String, Vec<usize>>,
    seen: usize,
}

impl OwnerFindings {
    /// Whether a referential-integrity finding of `owner` names `target`.
    fn names(&mut self, findings: &[Finding], owner: &str, target: &str) -> bool {
        for (position, finding) in findings.iter().enumerate().skip(self.seen) {
            if finding.check != Check::ReferentialIntegrity {
                continue;
            }
            if let Some(entity) = &finding.entity {
                self.by_owner
                    .entry(entity.clone())
                    .or_default()
                    .push(position);
            }
        }
        self.seen = findings.len();
        self.by_owner.get(owner).is_some_and(|positions| {
            positions
                .iter()
                .any(|&position| findings[position].message.contains(target))
        })
    }
}

/// Checks that every typed reference of a `geometry` or `model` record
/// resolves. On a document read from CADIR JSON, which holds no record of the
/// arenas the wire omits, a reference of an id type of those arenas resolves
/// against the `design` records that restate them, and one that resolves
/// nowhere is a Warning: the omitted arena may hold its target.
pub(super) fn check_typed_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    index: &dyn IdentitySet,
    findings: &mut Vec<Finding>,
) -> Result<(), cadmpeg_core::CodecError> {
    let not_read = (!ir.model.omitted_arenas_held()).then(|| {
        let arenas = crate::document::ArenaName::WIRE_OMITTED
            .iter()
            .map(|arena| arena.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        (arenas, design_identities(ir))
    });
    let mut owned = OwnerFindings::default();
    macro_rules! check_arenas {
        ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
            $(for entity in &ir.$home.$field {
                let owner = entity.identity();
                let walk = entity.visit_references(ctx, &mut |target| {
                    if index.contains(target) { return Ok(()); }
                    if owned.names(findings, owner, target) { return Ok(()); }
                    match &not_read {
                        Some((omitted, design)) if targets_omitted_arena() => {
                            if !design.contains(target) {
                                record_finding(findings, Check::ReferentialIntegrity, Severity::Warning, Some(owner), crate::rule!("referential_integrity.unresolved-typed-reference-document-read-cadir", "unresolved typed reference {target}; the document was read from CADIR, which does not carry the arenas {omitted}, and one of them may hold it"));
                            }
                        }
                        _ => record_finding(findings, Check::ReferentialIntegrity, Severity::Error, Some(owner), crate::rule!("referential_integrity.unresolved-typed-reference", "unresolved typed reference {target}")),
                    }
                    Ok(())
                });
                match walk {
                    Ok(()) => {}
                    Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Err(error),
                    Err(error) => record_finding(findings, Check::ReferentialIntegrity, Severity::Error, Some(owner), crate::rule!("referential_integrity.entity-schema-cannot-state-typed-references", "entity schema cannot state its typed references: {error}")),
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
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
        let index = ModelIndex::build(&ir);
        let identities = crate::index::identities::BorrowedIdentities::build(|add| {
            for id in index.identities(crate::index::StandardIndex) {
                add(id, ());
            }
        });
        check_typed_references(&ctx, &ir, &identities, &mut findings).unwrap();
        assert!(findings.iter().any(|finding| {
            finding.check == Check::ReferentialIntegrity
                && finding.severity == Severity::Error
                && finding.entity.as_deref() == Some(owner.as_str())
                && finding.message == format!("unresolved typed reference {missing}")
        }));
    }

    /// The findings of the typed-reference check of `ir` against no held
    /// identity.
    fn unresolved(ir: &crate::CadIr) -> Vec<crate::report::check::Finding> {
        let identities = crate::index::identities::BorrowedIdentities::<()>::new();
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
        let mut findings = Vec::new();
        check_typed_references(&ctx, ir, &identities, &mut findings).unwrap();
        findings
    }

    /// A document read from CADIR JSON, which holds no record of the arenas
    /// the wire omits.
    fn read_from_cadir() -> crate::CadIr {
        let ir =
            crate::CadIr::from_json(&crate::CadIr::empty().to_canonical_json().unwrap()).unwrap();
        assert!(!ir.model.omitted_arenas_held());
        ir
    }

    #[test]
    fn a_dangling_geometry_reference_of_a_cadir_read_document_is_an_error() {
        let mut ir = read_from_cadir();
        ir.geometry.vertices.push(crate::topology::Vertex {
            id: "test:model:vertex#owner".try_into().unwrap(),
            point: "test:model:point#missing".try_into().unwrap(),
            tolerance: None,
            native_ref: None,
        });
        let findings = unresolved(&ir);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].severity, Severity::Error);
        assert_eq!(
            findings[0].message,
            "unresolved typed reference test:model:point#missing"
        );
    }

    #[test]
    fn a_reference_into_an_omitted_arena_of_a_cadir_read_document_is_a_warning() {
        let mut ir = read_from_cadir();
        let mut definition: crate::products::ProductDefinition =
            cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
                "id": "test:model:product#owner", "bom_properties": {},
                "storage": {"kind": "embedded"}, "bodies": [], "occurrences": [],
                "instance_bodies": [], "representations": []
            }))
            .unwrap();
        definition.sketches = vec!["test:model:sketch#omitted".try_into().unwrap()];
        ir.model.product_definitions.push(definition.clone());
        let findings = unresolved(&ir);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].severity, Severity::Warning);
        assert!(findings[0]
            .message
            .starts_with("unresolved typed reference test:model:sketch#omitted; "));

        let mut held = crate::CadIr::empty();
        held.model.product_definitions.push(definition);
        let findings = unresolved(&held);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].severity, Severity::Error);
    }

    #[test]
    fn typed_reference_validation_propagates_depth_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let mut ir = crate::CadIr::empty();
        ir.geometry.vertices.push(crate::topology::Vertex {
            id: "test:model:vertex#owner".try_into().unwrap(),
            point: "test:model:point#missing".try_into().unwrap(),
            tolerance: None,
            native_ref: None,
        });
        let identities = crate::index::identities::BorrowedIdentities::<()>::new();
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
                sketches: Vec::default(),

                id: owner.try_into().unwrap(),
                kind: Some(crate::products::ProductDefinitionKind::Part),
                source_name: Some("test:model:name#plain".into()),
                label: None,
                description: None,
                part_number: None,
                bom_properties: std::collections::BTreeMap::new(),
                bodies: vec![target.try_into().unwrap(), target.try_into().unwrap()],
                native_ref: None,
                generated_from: None,
                storage: crate::products::Storage::Embedded,
                occurrences: Vec::new(),
                instance_bodies: Vec::new(),
                representations: Vec::new(),
                units: None,
                general_tolerance: None,
                active: None,
            });
        let identities = crate::index::identities::BorrowedIdentities::<()>::new();
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
