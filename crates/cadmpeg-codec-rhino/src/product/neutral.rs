// SPDX-License-Identifier: Apache-2.0
//! Prototype-local product ownership and placed instance references.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    ExternalDocument, Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind,
    PrototypeReference,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{CadIr, SourceProvenance};

use crate::container::Scan;
use crate::instances::{DefinitionKind, InstanceReference, LinkSource};
use crate::loss::RhinoLossCode;
use crate::settings::UnitBinding;
use crate::wire::Uuid;

const MAX_OCCURRENCES: usize = 1 << 20;
const MAX_INSTANCE_DEPTH: usize = 64;

fn product_id(id: Uuid) -> Result<ProductDefinitionId, CodecError> {
    ProductDefinitionId::mint(format!("rhino:model:product#{id}")).map_err(CodecError::malformed)
}

struct Projection<'a> {
    scan: &'a Scan<'a>,
    object_orders: HashMap<Uuid, Vec<usize>>,
    references: BTreeMap<usize, InstanceReference>,
    occurrences: Vec<Occurrence>,
    visited_orders: BTreeSet<usize>,
    losses: Vec<LossNote>,
}

impl Projection<'_> {
    fn note(&mut self, order: usize, message: impl std::fmt::Display) {
        let offset = self
            .scan
            .objects
            .get(order)
            .map_or(0, |object| object.range().start);
        self.losses.push(
            RhinoLossCode::ProductRecordUnprojected
                .note(format!("object record {order}: {message}"))
                .with_provenance(SourceProvenance::root(
                    "rhino",
                    cadmpeg_core::decode::u64_from_index(offset),
                )),
        );
    }

    fn visit(
        &mut self,
        ctx: &DecodeContext<'_>,
        order: usize,
        parent: Option<&OccurrenceId>,
        ordinal: u32,
        path: &str,
        stack: &mut Vec<Uuid>,
    ) -> Result<(), CodecError> {
        if self.occurrences.len() >= MAX_OCCURRENCES || stack.len() >= MAX_INSTANCE_DEPTH {
            self.note(order, "placed occurrence tree exceeds its local projection bound; typed native relationship remains held");
            return Ok(());
        }
        let Some(reference) = self.references.get(&order).cloned() else {
            return Ok(());
        };
        let Some(object) = self.scan.objects[order].framed() else {
            return Ok(());
        };
        let binding = UnitBinding::from_units(self.scan.metadata.settings.units.as_ref());
        let UnitBinding::Millimeters(scale) = binding else {
            self.note(
                order,
                format!(
                    "occurrence placement has no physical millimeter binding ({})",
                    binding.label()
                ),
            );
            return Ok(());
        };
        let Some(transform) = crate::instances::scale_translation(reference.transform(), scale)
        else {
            self.note(
                order,
                "occurrence translation overflows canonical millimeters",
            );
            return Ok(());
        };
        let definition = self
            .scan
            .definitions
            .definitions()
            .iter()
            .find(|definition| definition.id() == reference.definition_id());
        let ambiguous = self
            .scan
            .definitions
            .is_ambiguous(reference.definition_id());
        let prototype = if ambiguous {
            self.note(
                order,
                format!("prototype {} is ambiguous", reference.definition_id()),
            );
            PrototypeReference::Unresolved {}
        } else if let Some(definition) = definition {
            if matches!(definition.kind, DefinitionKind::Linked) && definition.members.is_empty() {
                PrototypeReference::External {
                    document: external_document(&definition.link),
                    object: Some(format!("{}", definition.id())),
                }
            } else {
                PrototypeReference::Local {
                    definition: product_id(definition.id())?,
                }
            }
        } else {
            self.note(
                order,
                format!("prototype {} is missing", reference.definition_id()),
            );
            PrototypeReference::Unresolved {}
        };
        let identity = &object.identity;
        let key = if path.is_empty() {
            format!("{order}")
        } else {
            format!("{path}.{order}")
        };
        let id = OccurrenceId::mint(format!("rhino:model:occurrence#{key}"))
            .map_err(CodecError::malformed)?;
        let native_key = if identity.object_id.is_nil()
            || self
                .object_orders
                .get(&identity.object_id)
                .is_some_and(|orders| orders.len() != 1)
        {
            format!("record-{order:06}")
        } else {
            format!("{}", identity.object_id)
        };
        let native_ref = format!("rhino:product:occurrence#{native_key}");
        ctx.charge_entities(1, "rhino_product_occurrences")?;
        self.visited_orders.insert(order);
        self.occurrences.push(Occurrence {
            id: id.clone(),
            prototype,
            parent: parent.map_or(OccurrenceParent::Root {}, |parent| {
                OccurrenceParent::Occurrence {
                    occurrence: parent.clone(),
                }
            }),
            ordinal,
            transform: Some(transform),
            linked_prototype: None,
            name: (!identity.name.is_empty()).then(|| identity.name.clone()),
            visible: object
                .attributes
                .parsed()
                .map(|attributes| attributes.visible),
            link: None,
            native_ref: Some(native_ref.clone()),
        });
        if ambiguous {
            return Ok(());
        }
        let Some(definition) = definition else {
            return Ok(());
        };
        if stack.contains(&definition.id()) {
            self.note(order, format!("recursive prototype {} closes a definition cycle; the closing occurrence is retained and further unfolding stops",definition.id()));
            return Ok(());
        }
        stack.push(definition.id());
        let mut seen = BTreeSet::new();
        for (member_ordinal, member) in definition.members.iter().enumerate() {
            if !seen.insert(*member) {
                self.note(
                    order,
                    format!("definition {} repeats member {member}", definition.id()),
                );
                continue;
            }
            let child = match self.object_orders.get(member).map(Vec::as_slice) {
                Some([child]) => *child,
                None => {
                    self.note(
                        order,
                        format!("definition {} member {member} is missing", definition.id()),
                    );
                    continue;
                }
                Some(_) => {
                    self.note(
                        order,
                        format!(
                            "definition {} member {member} is ambiguous",
                            definition.id()
                        ),
                    );
                    continue;
                }
            };
            if self.references.contains_key(&child) {
                let ordinal = u32::try_from(member_ordinal)
                    .map_err(|_| CodecError::malformed("Rhino occurrence ordinal exceeds u32"))?;
                self.visit(ctx, child, Some(&id), ordinal, &key, stack)?;
            }
        }
        stack.pop();
        Ok(())
    }
}

fn external_document(link: &LinkSource) -> ExternalDocument {
    match link {
        LinkSource::None => ExternalDocument::Missing {},
        LinkSource::Structured(reference) => {
            ExternalDocument::path(if reference.full_path.is_empty() {
                reference.relative_path.clone()
            } else {
                reference.full_path.clone()
            })
        }
        LinkSource::LegacyFull(path) => ExternalDocument::path(path.as_str()),
        LinkSource::LegacyRelative { relative_path, .. } => {
            ExternalDocument::path(relative_path.as_str())
        }
    }
}

fn object_bodies(ir: &CadIr, order: usize) -> Vec<BodyId> {
    let Some(id) = ir
        .native
        .namespace("rhino")
        .and_then(|namespace| namespace.arenas().get("unknowns"))
        .and_then(|records| records.get(order))
        .map(|record| record.id().to_owned())
    else {
        return Vec::new();
    };
    ir.model
        .bodies
        .iter()
        .filter(|body| body.native_ref.as_deref() == Some(id.as_str()))
        .map(|body| body.id.clone())
        .collect()
}

/// Project native definitions and references after local carriers are decoded.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_>,
    ir: &mut CadIr,
) -> Result<Vec<LossNote>, CodecError> {
    let mut projection = Projection {
        scan,
        object_orders: HashMap::new(),
        references: BTreeMap::new(),
        occurrences: Vec::new(),
        visited_orders: BTreeSet::new(),
        losses: Vec::new(),
    };
    for (order, object) in scan.objects.iter().enumerate() {
        if let Some(identity) = object.identity() {
            projection
                .object_orders
                .entry(identity.object_id)
                .or_default()
                .push(order);
        }
        let Some(object) = object.framed() else {
            continue;
        };
        if crate::instances::is_reference_class(object.class_uuid) {
            if let Ok(reference) =
                crate::instances::parse_reference(scan.data, object.class_data_range.clone())
            {
                projection.references.insert(order, reference);
            }
        }
    }
    for definition in scan.definitions.definitions() {
        if scan.definitions.is_ambiguous(definition.id()) {
            continue;
        }
        let mut bodies = Vec::new();
        for member in &definition.members {
            let Some([order]) = projection.object_orders.get(member).map(Vec::as_slice) else {
                continue;
            };
            let owned = object_bodies(ir, *order);
            if owned.is_empty() && !projection.references.contains_key(order) {
                projection.note(*order, format!("definition {} member {member} has no decoded Body ownership carrier; its local geometry and typed native membership remain held", definition.id()));
            }
            bodies.extend(owned);
        }
        bodies.sort();
        bodies.dedup();
        let id = product_id(definition.id())?;
        let native_ref = super::definition_id(definition.id());
        ctx.charge_entities(1, "rhino_product_definitions")?;
        ir.model.product_definitions.push(ProductDefinition {
            id: id.clone(),
            kind: ProductDefinitionKind::Part,
            source_name: (!definition.name.is_empty()).then(|| definition.name.clone()),
            label: None,
            description: (!definition.description.is_empty())
                .then(|| definition.description.clone()),
            part_number: None,
            bom_properties: BTreeMap::new(),
            bodies,
            sketches: Vec::new(),
            native_ref: Some(native_ref.clone()),
        });
    }
    let roots = projection
        .references
        .keys()
        .copied()
        .filter(|order| {
            scan.objects[*order]
                .identity()
                .is_none_or(|identity| !scan.definitions.contains_member(identity.object_id))
        })
        .collect::<Vec<_>>();
    for (ordinal, order) in roots.into_iter().enumerate() {
        projection.visit(
            ctx,
            order,
            None,
            u32::try_from(ordinal)
                .map_err(|_| CodecError::malformed("Rhino root occurrence ordinal exceeds u32"))?,
            "",
            &mut Vec::new(),
        )?;
    }
    // An unattached recursive native graph still needs a named projection loss.
    for order in projection.references.keys().copied().collect::<Vec<_>>() {
        let held = projection.visited_orders.contains(&order);
        if !held
            && scan.objects[order]
                .identity()
                .is_some_and(|identity| scan.definitions.contains_member(identity.object_id))
        {
            projection.note(order,"definition-member reference has no finite placed use reachable from a root occurrence; source relationship remains typed native content");
        }
    }
    ir.model.occurrences.extend(projection.occurrences);
    Ok(projection.losses)
}

#[cfg(test)]
mod tests {
    use crate::chunks::ArchiveVersion;
    use crate::test_support::test_dump as support;
    use cadmpeg_ir::products::{OccurrenceParent, PrototypeReference};

    fn placement(x: f64, y: f64) -> [[f64; 4]; 4] {
        [
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, y],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }

    #[test]
    fn two_placed_uses_share_one_local_body() {
        let archive = ArchiveVersion::V5;
        let definition_id = [0x51; 16];
        let member_id = [0x61; 16];
        let definition = support::definition_record(
            archive,
            &support::v5_definition_payload(archive, 6, definition_id, &[member_id], false),
        );
        let objects = [
            support::object_record_with_payload(
                archive,
                1,
                support::POINT_CLASS,
                &support::point_payload([2.0, 3.0, 4.0]),
            ),
            support::object_record_with_payload(
                archive,
                0x1000,
                support::INSTANCE_REFERENCE_CLASS,
                &support::instance_reference_payload(definition_id, placement(10.0, 0.0)),
            ),
            support::object_record_with_payload(
                archive,
                0x1000,
                support::INSTANCE_REFERENCE_CLASS,
                &support::instance_reference_payload(definition_id, placement(20.0, 0.0)),
            ),
        ];
        let bytes = support::document_with_definitions("50", archive, &[definition], &objects);
        crate::decode::with_expand_bytes(&bytes, |expand| {
            let mut scan = crate::container::scan(expand.ctx(), &bytes).expect("authored archive");
            support::set_identity(&mut scan, 0, member_id, "member", None, true);
            support::set_identity(&mut scan, 1, [0x71; 16], "first", None, true);
            support::set_identity(&mut scan, 2, [0x72; 16], "second", None, true);
            support::set_test_units(&mut scan, 1.0);
            let decoded = crate::decode::decode_for_test(&scan);
            let ir = decoded.ir();
            assert_eq!(ir.model.bodies.len(), 1);
            assert_eq!(ir.model.points.len(), 1);
            assert_eq!(
                ir.model.points[0].position().get(),
                cadmpeg_ir::math::Point3::new(2.0, 3.0, 4.0)
            );
            assert_eq!(ir.model.product_definitions.len(), 1);
            assert_eq!(
                ir.model.product_definitions[0].bodies,
                vec![ir.model.bodies[0].id.clone()]
            );
            assert_eq!(ir.model.occurrences.len(), 2);
            assert_eq!(
                ir.model.occurrences[0]
                    .transform
                    .expect("source occurrence placement")
                    .rows(),
                placement(10.0, 0.0)
            );
            assert_eq!(
                ir.model.occurrences[1]
                    .transform
                    .expect("source occurrence placement")
                    .rows(),
                placement(20.0, 0.0)
            );
            assert!(ir.model.occurrences.iter().all(|occurrence|matches!(&occurrence.prototype,PrototypeReference::Local {definition} if *definition==ir.model.product_definitions[0].id)));
            assert!(ir.model.bodies[0].transform.is_none());
        });
    }

    #[test]
    fn nested_reference_preserves_local_pose_and_computes_world_pose_on_request() {
        let archive = ArchiveVersion::V5;
        let inner_id = [0x51; 16];
        let outer_id = [0x52; 16];
        let member_id = [0x61; 16];
        let child_id = [0x62; 16];
        let definitions = [
            support::definition_record(
                archive,
                &support::v5_definition_payload(archive, 6, inner_id, &[member_id], false),
            ),
            support::definition_record(
                archive,
                &support::v5_definition_payload(archive, 6, outer_id, &[child_id], false),
            ),
        ];
        let objects = [
            support::object_record_with_payload(
                archive,
                1,
                support::POINT_CLASS,
                &support::point_payload([2.0, 3.0, 4.0]),
            ),
            support::object_record_with_payload(
                archive,
                0x1000,
                support::INSTANCE_REFERENCE_CLASS,
                &support::instance_reference_payload(inner_id, placement(0.0, 7.0)),
            ),
            support::object_record_with_payload(
                archive,
                0x1000,
                support::INSTANCE_REFERENCE_CLASS,
                &support::instance_reference_payload(outer_id, placement(10.0, 0.0)),
            ),
        ];
        let bytes = support::document_with_definitions("50", archive, &definitions, &objects);
        crate::decode::with_expand_bytes(&bytes, |expand| {
            let mut scan = crate::container::scan(expand.ctx(), &bytes).expect("authored archive");
            support::set_identity(&mut scan, 0, member_id, "member", None, true);
            support::set_identity(&mut scan, 1, child_id, "child", None, true);
            support::set_identity(&mut scan, 2, [0x71; 16], "root", None, true);
            support::set_test_units(&mut scan, 1.0);
            let decoded = crate::decode::decode_for_test(&scan);
            let ir = decoded.ir();
            assert_eq!(ir.model.bodies.len(), 1);
            assert_eq!(ir.model.occurrences.len(), 2);
            let parent = &ir.model.occurrences[0];
            let child = &ir.model.occurrences[1];
            assert_eq!(
                child.transform.expect("source occurrence placement").rows(),
                placement(0.0, 7.0)
            );
            assert_eq!(
                child.parent,
                OccurrenceParent::Occurrence {
                    occurrence: parent.id.clone()
                }
            );
            assert_eq!(
                cadmpeg_ir::products::AssemblyGraph::new(&ir.model.occurrences)
                    .expect("valid placed tree")
                    .effective_transform(&child.id)
                    .expect("world pose")
                    .rows(),
                placement(10.0, 7.0)
            );
        });
    }
}
