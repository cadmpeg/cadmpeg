// SPDX-License-Identifier: Apache-2.0
//! Occurrence grounding from the assembly state reader.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::scope::{DesignScope, OccurrenceDesign};
use cadmpeg_ir::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{AssemblyJoint, JointOperands, OperandContainer, Storage};
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

pub(crate) fn project(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    if ctx.depth() < cadmpeg_core::decode::DecodeDepth::Design {
        ir.model.assembly_joints.clear();
        return Ok(());
    }
    let grounded = grounded(&ir.model.assembly_joints);
    if grounded.is_empty() {
        return Ok(());
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
    ctx.charge_entities(1, "project Inventor assembly design scope")?;
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
    let unproduced = spanned_bodies(ir, &root.id);
    ir.design.scopes.push(DesignScope {
        definition: root.id.clone(),
        roots: Vec::new(),
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences,
        current: Vec::new(),
        unproduced,
    });
    ir.model.assembly_joints.clear();
    Ok(())
}

/// The bodies of the definitions that a scope without members spans (§5.1),
/// which no member produces (§6.7).
fn spanned_bodies(ir: &CadIr, scope: &ProductDefinitionId) -> BTreeSet<BodyId> {
    let definitions = ir
        .model
        .product_definitions
        .iter()
        .map(|definition| (&definition.id, definition))
        .collect::<BTreeMap<_, _>>();
    let prototypes = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| (&occurrence.id, &occurrence.prototype))
        .collect::<BTreeMap<_, _>>();
    let scoped = ir
        .design
        .scopes
        .iter()
        .map(|scope| &scope.definition)
        .collect::<BTreeSet<_>>();
    let open = |id: &ProductDefinitionId| {
        definitions.get(id).is_some_and(|definition| {
            matches!(definition.storage, Storage::Embedded) && !scoped.contains(id)
        })
    };
    let mut spanned = BTreeSet::from([scope]);
    let mut pending = vec![scope];
    if definitions
        .get(scope)
        .is_some_and(|root| matches!(root.storage, Storage::File { .. }))
    {
        let placed = prototypes.values().copied().collect::<BTreeSet<_>>();
        for id in definitions.keys().copied() {
            if open(id) && !placed.contains(id) && spanned.insert(id) {
                pending.push(id);
            }
        }
    }
    while let Some(id) = pending.pop() {
        let Some(definition) = definitions.get(id) else {
            continue;
        };
        for occurrence in &definition.occurrences {
            if let Some(&prototype) = prototypes.get(occurrence) {
                if open(prototype) && spanned.insert(prototype) {
                    pending.push(prototype);
                }
            }
        }
    }
    spanned
        .into_iter()
        .filter_map(|id| definitions.get(id))
        .flat_map(|definition| definition.bodies.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use cadmpeg_ir::products::JointId;
    use cadmpeg_ir::products::{JointConnector, JointOperand};
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

    #[test]
    fn grounding_moves_to_the_owning_scope_occurrence() {
        let (mut ir, id) = document();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        project(&ctx, &mut ir).expect("projection");
        assert!(ir.model.assembly_joints.is_empty());
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
        project(&ctx, &mut ir).expect("projection");
        assert_eq!(
            ir.design.scopes[0].unproduced,
            BTreeSet::from([body("embedded"), body("root")])
        );
    }
    #[test]
    fn geometry_depth_omits_grounding_design_records() {
        let (mut ir, _) = document();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes_at(
            &[],
            &arena,
            &policy,
            cadmpeg_core::decode::DecodeDepth::Geometry,
        )
        .expect("context");
        project(&ctx, &mut ir).expect("projection");
        assert!(ir.model.assembly_joints.is_empty());
        assert!(ir.design.scopes.is_empty());
    }
}
