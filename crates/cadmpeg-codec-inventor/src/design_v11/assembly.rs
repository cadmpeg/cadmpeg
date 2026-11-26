// SPDX-License-Identifier: Apache-2.0
//! Occurrence grounding from the assembly state reader.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{ArenaClass, DecodeContext, DecodeDepth};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::design::scope::{DesignScope, OccurrenceDesign};
use cadmpeg_ir::ids::{BodyId, OccurrenceId};
use cadmpeg_ir::products::{AssemblyJoint, JointOperands, OperandContainer};
use cadmpeg_ir::CadIr;

pub(crate) fn grounded(joints: &[AssemblyJoint]) -> BTreeSet<OccurrenceId> {
    joints
        .iter()
        .filter_map(|joint| {
            let JointOperands::Grounded { connector, .. } = joint.operands() else {
                return None;
            };
            let OperandContainer::Occurrence {
                occurrence,
                occurrence_path,
            } = &connector.operand.container
            else {
                return None;
            };
            occurrence_path.is_empty().then(|| occurrence.clone())
        })
        .collect()
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    body: &mut DecodeBody,
) -> Result<(), CodecError> {
    if ir.model.assembly_joints.is_empty() {
        return Ok(());
    }
    if ctx.depth() < DecodeDepth::Design {
        body.source_holds(ArenaClass::Design);
        ir.model.assembly_joints.clear();
        return Ok(());
    }
    let scope = scope(ir, &ir.model.assembly_joints)?;
    body.losses.push(crate::loss::InventorLossCode::AssemblyGroundingJointDropped.note(format!(
        "{} grounding joints are stated as grounded occurrences; their joint ids and native records are dropped.",
        ir.model.assembly_joints.len()
    )));
    ir.model.assembly_joints.clear();
    if let Some(scope) = scope {
        ctx.charge_entities(1, "project Inventor assembly design scope")?;
        ir.design.scopes.push(scope);
    }
    Ok(())
}

/// The design scope of the root assembly definition that `joints` ground:
/// its occurrences with their grounding, and as unproduced bodies the bodies
/// of the definitions the scope spans (§5.1), which no member produces
/// (§6.7). No joint grounds an occurrence: no scope.
pub(crate) fn scope(
    ir: &CadIr,
    joints: &[AssemblyJoint],
) -> Result<Option<DesignScope>, CodecError> {
    let grounded = grounded(joints);
    if grounded.is_empty() {
        return Ok(None);
    }
    let Some(root) = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
    else {
        return Err(CodecError::malformed(
            "Inventor assembly grounding has no root definition",
        ));
    };
    let occurrences = root
        .occurrences
        .iter()
        .map(|id| {
            (
                id.clone(),
                OccurrenceDesign {
                    configuration: None,
                    arrangements: Vec::new(),
                    grounded: grounded.contains(id).then_some(true),
                    flexible: None,
                    parameters: BTreeMap::new(),
                },
            )
        })
        .collect();
    let spanned = ir.spanned_definitions(&root.id);
    let unproduced = ir
        .model
        .product_definitions
        .iter()
        .filter(|definition| spanned.contains(definition.id.as_str()))
        .flat_map(|definition| definition.bodies.iter().cloned())
        .collect::<BTreeSet<BodyId>>();
    Ok(Some(DesignScope {
        definition: root.id.clone(),
        roots: Vec::new(),
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences,
        current: Vec::new(),
        unproduced,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use cadmpeg_ir::ids::ProductDefinitionId;
    use cadmpeg_ir::products::{JointConnector, JointOperand};
    use cadmpeg_ir::products::{JointId, Storage};
    use cadmpeg_ir::report::decode::DecodeTransfer;
    use cadmpeg_ir::transform::Transform;

    fn document() -> (CadIr, OccurrenceId) {
        let mut ir = crate::test_support::root_part_ir();
        let id = OccurrenceId::mint("inventor:assembly:occurrence#synthetic").expect("id");
        ir.model.product_definitions[0].occurrences.push(id.clone());
        ir.model.assembly_joints.push(AssemblyJoint::grounded(
            JointId::mint("inventor:assembly:joint#synthetic").expect("id"),
            JointConnector {
                operand: JointOperand::occurrence(id.clone(), "component", Vec::new()),
                frame: Transform::identity(),
                detached: false,
            },
            None,
        ));
        (ir, id)
    }

    fn decode_body(depth: DecodeDepth) -> DecodeBody {
        DecodeBody::new(DecodeTransfer::at(depth, true))
    }

    #[test]
    fn grounding_moves_to_the_owning_scope_occurrence() {
        let (mut ir, id) = document();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut body = decode_body(DecodeDepth::Full);
        project(&ctx, &mut ir, &mut body).expect("projection");
        assert!(ir.model.assembly_joints.is_empty());
        assert_eq!(
            body.losses
                .iter()
                .map(|loss| loss.code.clone())
                .collect::<Vec<_>>(),
            [crate::loss::InventorLossCode::AssemblyGroundingJointDropped.kind()]
        );
        assert_eq!(ir.design.scopes.len(), 1);
        assert_eq!(
            ir.design.scopes[0].definition,
            ir.model.product_definitions[0].id
        );
        assert_eq!(ir.design.scopes[0].occurrences[&id].grounded, Some(true));
        assert!(ir.design.scopes[0].roots.is_empty());
        assert!(ir.design.scopes[0].current.is_empty());
        assert!(ir.design.scopes[0].unproduced.is_empty());
    }

    #[test]
    fn bodies_of_spanned_embedded_definitions_are_unproduced() {
        let (mut ir, id) = document();
        let body = |key: &str| BodyId::mint(format!("inventor:brep:body#{key}")).expect("body");
        let mut embedded = ir.model.product_definitions[0].clone();
        embedded.id =
            ProductDefinitionId::mint("inventor:product:definition#embedded").expect("id");
        embedded.storage = Storage::Embedded;
        embedded.occurrences = Vec::new();
        embedded.bodies = vec![body("embedded")];
        let mut external = embedded.clone();
        external.id =
            ProductDefinitionId::mint("inventor:product:definition#external").expect("id");
        external.storage = ir.model.product_definitions[0].storage.clone();
        external.bodies = vec![body("external")];
        ir.model.product_definitions[0].bodies = vec![body("root")];
        let occurrence =
            cadmpeg_ir::products::Occurrence::new(id.clone(), embedded.id.clone(), None);
        ir.model.occurrences.push(occurrence);
        ir.model.product_definitions.push(embedded);
        ir.model.product_definitions.push(external);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        project(&ctx, &mut ir, &mut decode_body(DecodeDepth::Full)).expect("projection");
        assert_eq!(
            ir.design.scopes[0].unproduced,
            BTreeSet::from([body("embedded"), body("root")])
        );
    }
    #[test]
    fn a_spanned_body_dropped_from_unproduced_fails_native_validation() {
        let (mut ir, id) = document();
        let body = |key: &str| BodyId::mint(format!("inventor:brep:body#{key}")).expect("body");
        let mut embedded = ir.model.product_definitions[0].clone();
        embedded.id =
            ProductDefinitionId::mint("inventor:product:definition#embedded").expect("id");
        embedded.storage = Storage::Embedded;
        embedded.occurrences = Vec::new();
        embedded.bodies = vec![body("embedded")];
        ir.model
            .occurrences
            .push(cadmpeg_ir::products::Occurrence::new(
                id,
                embedded.id.clone(),
                None,
            ));
        ir.model.product_definitions.push(embedded);
        ir.geometry.bodies.push(
            cadmpeg_core::serde_erase::json::from_value(
                serde_json::json!({"id": "inventor:brep:body#embedded", "kind": "solid"}),
            )
            .expect("body"),
        );
        let joints = ir.model.assembly_joints.clone();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        project(&ctx, &mut ir, &mut decode_body(DecodeDepth::Full)).expect("projection");
        let design_errors = |ir: &CadIr| {
            cadmpeg_ir::validate::validate_neutral(ir, Vec::new())
                .expect("validation")
                .findings
                .into_iter()
                .filter(|finding| {
                    finding.check == cadmpeg_ir::report::check::Check::Design
                        && finding.severity == cadmpeg_ir::report::Severity::Error
                })
                .map(|finding| finding.message)
                .collect::<Vec<_>>()
        };
        assert_eq!(design_errors(&ir), Vec::<String>::new());
        assert!(crate::validate::assembly_scope_findings(&ir, &joints).is_empty());
        ir.design.scopes[0].unproduced.remove(&body("embedded"));
        let errors = design_errors(&ir);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].contains("belongs to no current state or unproduced set of that scope"),
            "{errors:?}"
        );
        assert_eq!(
            crate::validate::assembly_scope_findings(&ir, &joints).len(),
            1
        );
    }

    #[test]
    fn a_geometry_decode_charges_the_undelivered_design_for_grounding_joints() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) =
            DecodeContext::from_root_bytes_at(&[], &arena, &policy, DecodeDepth::Geometry)
                .expect("context");
        let (mut ir, _) = document();
        let mut body = decode_body(DecodeDepth::Geometry);
        project(&ctx, &mut ir, &mut body).expect("projection");
        assert!(ir.model.assembly_joints.is_empty());
        assert!(ir.design.scopes.is_empty());
        assert_eq!(body.losses.len(), 1, "{:#?}", body.losses);
        assert_eq!(
            body.losses[0].code.to_string(),
            "decode/depth.not-delivered"
        );
        assert!(
            body.losses[0]
                .message
                .contains("the source holds design records that a geometry-depth decode"),
            "{}",
            body.losses[0].message
        );

        let (mut ir, _) = document();
        ir.model.assembly_joints.clear();
        let mut body = decode_body(DecodeDepth::Geometry);
        project(&ctx, &mut ir, &mut body).expect("projection");
        assert!(body.losses.is_empty(), "{:#?}", body.losses);
    }
}
