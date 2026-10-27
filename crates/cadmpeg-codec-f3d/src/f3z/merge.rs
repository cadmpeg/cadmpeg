// SPDX-License-Identifier: Apache-2.0
//! Shared product definitions and instance trees for decoded F3Z members.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::document::{EntityRewrite, Model};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::SourceFidelity;
use cadmpeg_ir::{Native, NativeRecord};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::archive::{ArchiveSession, ClassifiedMember};
use crate::container::ContainerScan;
use crate::loss::F3dLossCode;
use crate::records::xref::XrefReference;
use crate::xref::{self, XrefTable};

/// Merges the root document's recursively reachable members in archive scope.
pub(super) fn merge_archive(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    archive: &ArchiveSession<'_>,
    model_root: String,
    ir: &mut cadmpeg_ir::CadIr,
    report: &mut cadmpeg_ir::codec::DecodeBody,
    fidelity: &mut cadmpeg_ir::SourceFidelity,
) -> Result<usize, CodecError> {
    let table = xref_table_from_ir(ctx, ir)?;

    let stack = vec![model_root];
    MergeSession {
        ctx,
        scan,
        archive,
        stack,
        members: std::collections::BTreeMap::new(),
    }
    .merge(ir, report, fidelity, &table)
}

/// Reassigns only repeated sibling ordinals after independent document graphs
/// have been combined.
pub(super) fn make_sibling_ordinals_unique(
    occurrences: &mut [cadmpeg_ir::products::Occurrence],
) -> Result<(), CodecError> {
    use std::collections::{HashMap, HashSet};

    // Every ordinal below a parent's floor is in use, so the search for the
    // smallest free ordinal resumes there and each parent's search is linear
    // over the whole pass.
    let mut used = HashMap::<Option<&cadmpeg_ir::ids::OccurrenceId>, (HashSet<u32>, u64)>::new();
    for occurrence in occurrences {
        let parent = match &occurrence.parent {
            cadmpeg_ir::products::OccurrenceParent::Root {} => None,
            cadmpeg_ir::products::OccurrenceParent::Occurrence { occurrence } => Some(occurrence),
        };
        let (siblings, floor) = used.entry(parent).or_default();
        let ordinal = if siblings.contains(&occurrence.ordinal) {
            let free = (*floor..=u64::from(u32::MAX))
                .filter_map(|candidate| u32::try_from(candidate).ok())
                .find(|candidate| !siblings.contains(candidate))
                .ok_or_else(|| {
                    CodecError::malformed(
                        "F3Z sibling occurrence population exhausts the u32 ordinal space",
                    )
                })?;
            *floor = u64::from(free) + 1;
            free
        } else {
            occurrence.ordinal
        };
        siblings.insert(ordinal);
        occurrence.ordinal = ordinal;
    }
    Ok(())
}

fn xref_table_from_ir(
    ctx: &DecodeContext<'_>,
    ir: &cadmpeg_ir::CadIr,
) -> Result<XrefTable, CodecError> {
    fn load_arena<T: DeserializeOwned>(
        ctx: &DecodeContext<'_>,
        namespace: &cadmpeg_ir::NativeNamespace,
        name: &str,
    ) -> Result<Vec<T>, CodecError> {
        match namespace.arena_as_for_decode(ctx, name) {
            Ok(records) => Ok(records),
            Err(error) => match CodecError::from(error) {
                error @ CodecError::ResourceLimit(_) => Err(error),
                CodecError::Malformed(message) => Err(CodecError::malformed(format_args!(
                    "invalid F3D native data: {message}"
                ))),
                error => Err(error),
            },
        }
    }
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(XrefTable::default());
    };
    Ok(XrefTable {
        designs: load_arena(ctx, namespace, "xref_designs")?,
        references: load_arena(ctx, namespace, "xref_references")?,
        placement_failures: Vec::new(),
        placement_overrides: Vec::new(),
        placement_metadata_missing: false,
    })
}

struct MemberPlacement<'a> {
    reference: &'a XrefReference,
    parent: cadmpeg_ir::ids::OccurrenceId,
    scope: String,
}

fn member_placements<'a>(
    model: &Model,
    table: &'a XrefTable,
    containing_scope: Option<&str>,
) -> Vec<MemberPlacement<'a>> {
    use cadmpeg_ir::products::PrototypeReference;

    let mut placements = Vec::new();
    for reference in &table.references {
        for occurrence in model.occurrences.iter().filter(|occurrence| {
            matches!(occurrence.prototype, PrototypeReference::External { .. })
                && occurrence.native_ref.as_deref() == Some(reference.id.as_str())
        }) {
            let key = occurrence_key(reference);
            let mut scope = containing_scope
                .map_or_else(|| key.clone(), |parent| format!("{parent}/xref/{key}"));
            let neutral = crate::ids::neutral_xref_occurrence_id(
                reference.ordinal,
                reference.occurrence_ordinal,
            );
            let expected = containing_scope
                .and_then(|scope| rescope(neutral.as_str(), scope))
                .unwrap_or_else(|| neutral.as_str().to_owned());
            if occurrence.id.as_str() != expected {
                scope = format!(
                    "{scope}/instance-{}",
                    EscapedOccurrenceComponent(occurrence.id.as_str())
                );
            }
            placements.push(MemberPlacement {
                reference,
                parent: occurrence.id.clone(),
                scope,
            });
        }
    }
    placements
}

#[derive(Clone)]
struct MemberDefinition {
    definition: cadmpeg_ir::ids::ProductDefinitionId,
    scope: String,
    occurrences: Vec<cadmpeg_ir::products::Occurrence>,
    joints: Vec<cadmpeg_ir::products::AssemblyJoint>,
    table: XrefTable,
}

/// State shared by recursive F3Z reference traversal.
struct MergeSession<'r, 'a> {
    ctx: &'r DecodeContext<'a>,
    scan: &'r ContainerScan<'a>,
    archive: &'r ArchiveSession<'a>,
    stack: Vec<String>,
    members: std::collections::BTreeMap<String, MemberDefinition>,
}

impl MergeSession<'_, '_> {
    fn merge(
        &mut self,
        parent_ir: &mut cadmpeg_ir::CadIr,
        parent_report: &mut cadmpeg_ir::codec::DecodeBody,
        parent_fidelity: &mut cadmpeg_ir::SourceFidelity,
        table: &XrefTable,
    ) -> Result<usize, CodecError> {
        let placements = member_placements(&parent_ir.model, table, None);
        let mut member_roots = std::collections::BTreeMap::new();
        let merged = self.merge_placements(
            parent_ir,
            parent_report,
            parent_fidelity,
            table,
            placements,
            &mut member_roots,
        )?;
        rebind_joint_descent(
            self.ctx,
            parent_ir,
            &member_roots,
            &mut parent_report.losses,
        )?;
        parent_report
            .losses
            .retain(|loss| loss.code != F3dLossCode::SavedFreeEdgeBodyUnowned.kind());
        parent_report
            .losses
            .extend(crate::decode::saved_free_edge_bodies_loss(&parent_ir.model));
        Ok(merged)
    }

    fn merge_placements(
        &mut self,
        parent_ir: &mut cadmpeg_ir::CadIr,
        parent_report: &mut cadmpeg_ir::codec::DecodeBody,
        parent_fidelity: &mut cadmpeg_ir::SourceFidelity,
        table: &XrefTable,
        placements: Vec<MemberPlacement<'_>>,
        member_roots: &mut std::collections::BTreeMap<
            cadmpeg_ir::ids::OccurrenceId,
            cadmpeg_ir::ids::OccurrenceId,
        >,
    ) -> Result<usize, CodecError> {
        use cadmpeg_ir::products::PrototypeReference;

        let mut merged = 0usize;
        for placement in placements {
            let reference = placement.reference;
            let occurrence = placement.scope;
            let label = xref::design_for(table, reference)
                .map_or(reference.relative_path.as_str(), |design| {
                    design.display_name.as_str()
                });
            if self
                .stack
                .iter()
                .any(|path| path == reference.relative_path.as_str())
            {
                super::push_loss(
                    &mut parent_report.losses,
                    F3dLossCode::XrefCycle,
                    format_args!(
                        "xref {label}: reference cycle through {}; the occurrence was not resolved",
                        reference.relative_path
                    ),
                );
                continue;
            }
            let ctx = self.ctx;
            let _depth = ctx.enter_nested("f3z member reference")?;
            let member = if let Some(member) = self.members.get(reference.relative_path.as_str()) {
                self.ctx.charge_entities(
                    cadmpeg_core::decode::u64_from_index(
                        member
                            .occurrences
                            .len()
                            .checked_add(member.joints.len())
                            .ok_or_else(|| {
                                CodecError::malformed("F3Z repeated member entity count overflows")
                            })?,
                    ),
                    "F3Z repeated member occurrences and joints",
                )?;
                member.clone()
            } else {
                let Some(member) = self.archive.members.get(reference.relative_path.as_str())
                else {
                    if self.scan.entry_view(&reference.relative_path).is_some() {
                        super::push_loss(&mut parent_report.losses, F3dLossCode::XrefMemberUndecoded, format_args!(
                            "xref {label}: member {} is not an F3D document member; the occurrence was not resolved",
                            reference.relative_path
                        ));
                    } else {
                        super::push_loss(&mut parent_report.losses, F3dLossCode::XrefMemberMissing, format_args!(
                            "xref {label}: member {} is not present in the archive; the occurrence was not resolved",
                            reference.relative_path
                        ));
                    }
                    continue;
                };
                let member_scan = match member {
                    ClassifiedMember::Scanned(member_scan) => member_scan,
                    ClassifiedMember::Unreadable(_) => continue,
                };
                let component = match crate::decode::decode_archive_member(
                    self.ctx,
                    member_scan,
                    &self.archive.layers,
                ) {
                    Ok(component) => component.into_decoded(),
                    Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                    Err(error) => {
                        super::push_loss(&mut parent_report.losses, F3dLossCode::XrefMemberUndecoded, format_args!(
                            "xref {label}: member {} failed to decode ({error}); the occurrence was not resolved",
                            reference.relative_path
                        ));
                        continue;
                    }
                };
                let mut child_table = xref_table_from_ir(self.ctx, &component.ir)?;
                let cadmpeg_ir::codec::Decoded {
                    ir: mut component_ir,
                    body: mut component_report,
                    source_fidelity: component_fidelity,
                } = component;
                scope_document_parameters(self.ctx, &mut component_ir.model)?;
                let definition = define_member(self.ctx, &mut component_ir.model, label)?;
                append_feature_history(&parent_ir.model, &mut component_ir.model)?;
                let mut scope = OccurrenceScope {
                    ctx: self.ctx,
                    occurrence: &occurrence,
                };
                let occurrence_start = parent_ir.model.occurrences.len();
                let joint_start = parent_ir.model.assembly_joints.len();
                parent_ir
                    .model
                    .extend_rewritten(component_ir.model, &mut scope)?;
                let member = MemberDefinition {
                    definition: scope.rewrite(definition)?,
                    scope: occurrence.clone(),
                    occurrences: parent_ir
                        .model
                        .occurrences
                        .drain(occurrence_start..)
                        .collect(),
                    joints: parent_ir
                        .model
                        .assembly_joints
                        .drain(joint_start..)
                        .collect(),
                    table: {
                        for reference in &mut child_table.references {
                            reference.id =
                                rescope(&reference.id, &occurrence).ok_or_else(|| {
                                    CodecError::malformed("invalid F3Z reference identity")
                                })?;
                        }
                        child_table
                    },
                };
                extend_native(
                    self.ctx,
                    &mut parent_ir.native,
                    component_ir.native,
                    &occurrence,
                )?;
                parent_fidelity.append(rescope_fidelity(component_fidelity, &occurrence)?)?;
                if component_report.transfer.geometry_transferred() {
                    parent_report.transfer = cadmpeg_ir::report::decode::DecodeTransfer::at(
                        parent_report.transfer.depth(),
                        true,
                    );
                }
                for loss in &mut component_report.losses {
                    loss.message = format!("xref {label}: {}", loss.message);
                }
                parent_report.losses.append(&mut component_report.losses);
                self.members
                    .insert(reference.relative_path.as_str().to_owned(), member.clone());
                member
            };
            let mut instance = member.instantiate(self.ctx, &occurrence, &placement.parent)?;
            let child_placements = member_placements(&instance, &member.table, Some(&occurrence));
            for root in &instance.occurrences {
                if matches!(root.parent, cadmpeg_ir::products::OccurrenceParent::Root {})
                    && matches!(root.prototype, PrototypeReference::Local { .. })
                    && root.native_ref.is_none()
                {
                    member_roots.insert(root.id.clone(), placement.parent.clone());
                }
            }
            reparent_component_roots(&mut instance.occurrences, &placement.parent)?;
            parent_ir.model.occurrences.extend(instance.occurrences);
            parent_ir
                .model
                .assembly_joints
                .extend(instance.assembly_joints);
            let placing = parent_ir
                .model
                .occurrences
                .iter_mut()
                .find(|item| item.id == placement.parent)
                .ok_or_else(|| CodecError::malformed("missing F3Z placing occurrence"))?;
            placing.prototype = PrototypeReference::Local {
                definition: member.definition,
            };
            self.stack.push(reference.relative_path.as_str().to_owned());
            let descendants = self.merge_placements(
                parent_ir,
                parent_report,
                parent_fidelity,
                &member.table,
                child_placements,
                member_roots,
            )?;
            self.stack.pop();
            merged += descendants + 1;
            parent_report.notes.push(format!(
                "xref {label}: merged {} as occurrence {occurrence} (shared definition; {descendants} nested occurrence(s))",
                reference.relative_path
            ));
        }
        Ok(merged)
    }
}

fn rebind_joint_descent(
    ctx: &DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
    member_roots: &std::collections::BTreeMap<
        cadmpeg_ir::ids::OccurrenceId,
        cadmpeg_ir::ids::OccurrenceId,
    >,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    use crate::records::feature::assembly_features::DesignComponentOccurrence;
    use cadmpeg_ir::products::{OccurrenceParent, OperandContainer};

    if !ir.model.assembly_joints.iter().any(|joint| {
        joint.connectors().any(|connector| {
            matches!(
                connector.operand.container,
                OperandContainer::Occurrence { .. }
            ) && !connector.operand.subelements.is_empty()
        })
    }) {
        return Ok(());
    }
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(());
    };
    let carriers = namespace
        .arena_iter_as_for_decode::<DesignComponentOccurrence>(ctx, "design_component_occurrences")
        .map(|carrier| carrier.map(|carrier| (carrier.id, carrier.occurrence_guid)))
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
    let mut children = std::collections::BTreeMap::new();
    for occurrence in &ir.model.occurrences {
        let Some(key) = occurrence
            .native_ref
            .as_deref()
            .and_then(|id| carriers.get(id))
        else {
            continue;
        };
        let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent else {
            continue;
        };
        let mut parent = parent;
        for _ in 0..=member_roots.len() {
            children
                .entry((parent.clone(), key.as_str().to_ascii_lowercase()))
                .and_modify(|selected| *selected = None)
                .or_insert(Some(&occurrence.id));
            let Some(outer) = member_roots.get(parent) else {
                break;
            };
            parent = outer;
        }
    }
    for joint in &mut ir.model.assembly_joints {
        let joint_id = joint.id.clone();
        for connector in joint.connectors_mut() {
            let operand = &mut connector.operand;
            // An operand that already names a nested document path is not
            // rebound: its path is relative to the current container.
            let OperandContainer::Occurrence {
                occurrence,
                occurrence_path,
            } = &operand.container
            else {
                continue;
            };
            if !occurrence_path.is_empty() {
                continue;
            }
            let mut selected = occurrence;
            let mut consumed = 0;
            let mut ambiguous = false;
            for key in &operand.subelements {
                match children.get(&(selected.clone(), key.to_ascii_lowercase())) {
                    Some(Some(child)) => {
                        selected = child;
                        consumed += 1;
                    }
                    Some(None) => {
                        ambiguous = true;
                        losses.push(F3dLossCode::AssemblyOperandDescentAmbiguous.note(format!(
                            "joint {joint_id}: operand descent key {key} selects multiple merged child occurrences; the operand is unchanged"
                        )));
                        break;
                    }
                    None => break,
                }
            }
            if consumed != 0 && !ambiguous {
                operand.container = OperandContainer::Occurrence {
                    occurrence: selected.clone(),
                    occurrence_path: Vec::new(),
                };
                operand.subelements.drain(..consumed);
            }
        }
    }
    Ok(())
}

impl MemberDefinition {
    fn instantiate(
        &self,
        ctx: &DecodeContext<'_>,
        scope: &str,
        parent: &cadmpeg_ir::ids::OccurrenceId,
    ) -> Result<Model, CodecError> {
        let prefix = format!("f3d:xref/{}/", self.scope);
        let ids = self
            .occurrences
            .iter()
            .map(|occurrence| occurrence.id.as_str())
            .chain(self.joints.iter().map(|joint| joint.id.as_str()))
            .map(|id| {
                let tail = id
                    .strip_prefix(&prefix)
                    .ok_or_else(|| CodecError::malformed("invalid F3Z member instance scope"))?;
                Ok((id, format!("f3d:xref/{scope}/{tail}")))
            })
            .collect::<Result<std::collections::BTreeMap<_, _>, CodecError>>()?;
        let rewrite =
            |id: &str| Ok::<_, CodecError>(ids.get(id).cloned().unwrap_or_else(|| id.to_owned()));
        let occurrences = self
            .occurrences
            .iter()
            .map(|occurrence| {
                cadmpeg_ir::schema::rewrite::identities(ctx, occurrence.clone(), rewrite)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let assembly_joints = self
            .joints
            .iter()
            .map(|joint| {
                let mut joint =
                    cadmpeg_ir::schema::rewrite::identities(ctx, joint.clone(), rewrite)?;
                for connector in joint.connectors_mut() {
                    if matches!(
                        connector.operand.container,
                        cadmpeg_ir::products::OperandContainer::Root {}
                    ) {
                        connector.operand.container =
                            cadmpeg_ir::products::OperandContainer::Occurrence {
                                occurrence: parent.clone(),
                                occurrence_path: Vec::new(),
                            };
                    }
                }
                Ok(joint)
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        let mut instance = Model::default();
        instance.occurrences = occurrences;
        instance.assembly_joints = assembly_joints;
        Ok(instance)
    }
}

fn define_member(
    ctx: &DecodeContext<'_>,
    model: &mut Model,
    label: &str,
) -> Result<cadmpeg_ir::ids::ProductDefinitionId, CodecError> {
    use cadmpeg_ir::products::{ProductDefinition, ProductDefinitionKind};

    let owned = model
        .product_definitions
        .iter()
        .flat_map(|definition| &definition.bodies)
        .collect::<std::collections::BTreeSet<_>>();
    let bodies = model
        .bodies
        .iter()
        .filter(|body| !owned.contains(&body.id))
        .map(|body| body.id.clone())
        .collect();
    ctx.charge_entities(1, "F3Z member product definition")?;
    let id = crate::ids::archive_product_definition_id();
    model.product_definitions.push(ProductDefinition {
        id: id.clone(),
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: (!label.trim().is_empty()).then(|| label.to_owned()),
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::new(),
        bodies,
        sketches: Vec::new(),
        native_ref: None,
    });
    Ok(id)
}

/// Places every root-level occurrence from a merged member inside the
/// occurrence that owns that member. Child occurrence parents already carry
/// the member-local hierarchy and are left unchanged.
fn reparent_component_roots(
    occurrences: &mut [cadmpeg_ir::products::Occurrence],
    parent: &cadmpeg_ir::ids::OccurrenceId,
) -> Result<(), CodecError> {
    for occurrence in occurrences {
        if matches!(
            occurrence.parent,
            cadmpeg_ir::products::OccurrenceParent::Root {}
        ) {
            let parent_id = String::from_utf8(parent.as_str().as_bytes().to_vec())
                .map_err(CodecError::malformed)?;
            occurrence.parent = cadmpeg_ir::products::OccurrenceParent::Occurrence {
                occurrence: cadmpeg_ir::ids::OccurrenceId::mint(parent_id)
                    .map_err(CodecError::malformed)?,
            };
        }
    }
    Ok(())
}

/// Gives document parameters an equation container before occurrence rescoping.
/// The container is an entity the member decode did not admit, so it is
/// charged here.
fn scope_document_parameters(ctx: &DecodeContext<'_>, model: &mut Model) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{
        DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation,
        FeatureOperation, FeatureTreeNodeRole, TreeChildren,
    };

    if model
        .parameters
        .iter()
        .all(|parameter| parameter.owner.is_some())
    {
        return Ok(());
    }
    ctx.charge_entities(1, "F3Z document parameter scope")?;
    let owner = crate::ids::document_parameter_scope_id();
    let first = model
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .min()
        .unwrap_or(0);
    for feature in &mut model.features {
        feature.ordinal = feature
            .ordinal
            .checked_sub(first)
            .and_then(|ordinal| ordinal.checked_add(1))
            .ok_or_else(|| CodecError::malformed("merged F3Z feature ordinal exceeds u64::MAX"))?;
    }
    for parameter in &mut model.parameters {
        if parameter.owner.is_none() {
            parameter.owner = Some(owner.clone());
        }
    }
    model.features.push(Feature {
        id: owner,
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::Equations,
                children: TreeChildren::default(),
            },
        )),
        native_ref: None,
    });
    Ok(())
}

/// Places one component's feature history after the histories already merged.
fn append_feature_history(parent: &Model, component: &mut Model) -> Result<(), CodecError> {
    let Some(component_minimum) = component
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .min()
    else {
        return Ok(());
    };

    let next = parent
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .max()
        .map_or(Ok(0), |ordinal| {
            ordinal.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("merged F3Z feature ordinal exceeds u64::MAX".into())
            })
        })?;

    for feature in &mut component.features {
        feature.ordinal = feature
            .ordinal
            .checked_sub(component_minimum)
            .and_then(|ordinal| ordinal.checked_add(next))
            .ok_or_else(|| {
                CodecError::Malformed("merged F3Z feature ordinal exceeds u64::MAX".into())
            })?;
    }
    Ok(())
}

fn rescope_fidelity(
    source: SourceFidelity,
    occurrence: &str,
) -> Result<SourceFidelity, CodecError> {
    let (mut annotations, records) = source.into_parts();
    annotations
        .map_ids(|id| match rescope(id, occurrence) {
            Some(id) => Ok(id),
            None => Ok(id.to_string()),
        })?
        .map_err(CodecError::from)?;
    // The occurrence is one owner component. Escape its separators so two
    // different occurrences cannot share an owner by shifting a path boundary.
    let owner = cadmpeg_ir::StreamName::try_from(format!(
        "f3d:xref/{}/",
        EscapedOccurrenceComponent(occurrence)
    ))
    .map_err(CodecError::malformed)?;
    let provenance = std::mem::take(&mut annotations.provenance);
    let mut builder = AnnotationBuilder::resume(annotations);
    for (id, provenance) in provenance {
        let stream =
            cadmpeg_ir::StreamName::try_from(format!("{}{}", owner.as_str(), provenance.stream()))
                .map_err(CodecError::malformed)?;
        let stream = StreamHandle::new(stream);
        builder.note(&id, &stream, provenance.offset, provenance.tag.as_deref());
    }
    let mut rescoped = SourceFidelity::with_annotations(builder.build());
    for (id, record) in records {
        let id_text = match rescope(id.as_str(), occurrence) {
            Some(id) => id,
            None => {
                format!("{id}")
            }
        };
        let id = UnknownId::mint(id_text).map_err(|error| {
            CodecError::malformed(format_args!("F3Z retained record {id}: {error}"))
        })?;
        let stream =
            cadmpeg_ir::StreamName::try_from(format!("{}{}", owner.as_str(), record.stream()))
                .map_err(CodecError::malformed)?;

        rescoped.insert_retained_record(id, record.with_owner(stream))?;
    }
    Ok(rescoped)
}

fn occurrence_key(reference: &XrefReference) -> String {
    let role = EscapedOccurrenceComponent(&reference.neutron_role);
    // `occurrence_ordinal` restarts for each Redirections reference. Keep the
    // source reference ordinal in the scope so two admitted rows carrying the
    // same role cannot merge their model or fidelity identities.
    format!(
        "role-{role}/reference-{}/occurrence-{}",
        reference.ordinal, reference.occurrence_ordinal
    )
}

struct EscapedOccurrenceComponent<'a>(&'a str);

impl std::fmt::Display for EscapedOccurrenceComponent<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for character in self.0.chars() {
            if matches!(character, ':' | '#' | '%' | '/') || character.is_whitespace() {
                let mut bytes = [0; 4];
                for byte in character.encode_utf8(&mut bytes).as_bytes() {
                    write!(formatter, "%{byte:02X}")?;
                }
            } else {
                write!(formatter, "{character}")?;
            }
        }
        Ok(())
    }
}

fn rescope(text: &str, occurrence: &str) -> Option<String> {
    text.strip_prefix("f3d:")
        .map(|rest| format!("f3d:xref/{occurrence}/{rest}"))
}

/// Rewrites every `f3d:` identity in one model entity into occurrence scope.
struct OccurrenceScope<'r, 'a> {
    ctx: &'r DecodeContext<'a>,
    occurrence: &'r str,
}

impl EntityRewrite for OccurrenceScope<'_, '_> {
    type Error = CodecError;

    fn rewrite<T: cadmpeg_ir::schema::rewrite::typed::RewriteIdentities>(
        &mut self,
        entity: T,
    ) -> Result<T, CodecError> {
        cadmpeg_ir::schema::rewrite::identities(self.ctx, entity, |id: &str| {
            match rescope(id, self.occurrence) {
                Some(rewritten) => Ok(rewritten),
                None => Ok(id.to_owned()),
            }
        })
    }
}

/// Appends all known component-native arenas after occurrence-local rescoping.
fn extend_native(
    ctx: &DecodeContext<'_>,
    root: &mut Native,
    mut component: Native,
    occurrence: &str,
) -> Result<(), CodecError> {
    let Some(mut source) = component.0.remove("f3d") else {
        return Ok(());
    };
    let target = root.namespace_mut("f3d");
    for name in crate::native::F3D_ARENA_NAMES
        .iter()
        .copied()
        .chain(std::iter::once("unknowns"))
    {
        let Some(records) = source.arenas_mut().remove(name) else {
            continue;
        };
        if records.is_empty() {
            continue;
        }
        let arena = target.arenas_mut().entry(name.to_string()).or_default();

        for record in records {
            arena.push(rescope_record(ctx, &record, name, occurrence)?);
        }
    }
    Ok(())
}

/// Rescopes one native record's identity and every identity it references.
fn rescope_record(
    ctx: &DecodeContext<'_>,
    record: &NativeRecord,
    arena: &str,
    occurrence: &str,
) -> Result<NativeRecord, CodecError> {
    let mut fields = typed_fields(ctx, record, arena, occurrence)?;
    rescope_native_reference_fields(ctx, arena, &mut fields, occurrence)?;
    let id = rescope(record.id(), occurrence).unwrap_or_else(|| record.id().to_string());
    NativeRecord::new(
        cadmpeg_ir::ids::Identity::new(id).map_err(CodecError::malformed)?,
        fields,
    )
    .map_err(CodecError::from)
}

/// Rewrite typed identity markers before JSON erases their ownership.
///
/// Most F3D native records carry source text and numeric stream facts. The
/// records listed here also carry an IR identity marker inside a structured
/// field. Going through the typed owner keeps that marker distinct from an
/// ordinary `String` while the field-specific pass below handles native text
/// references that have not yet gained a newtype.
fn typed_fields(
    ctx: &DecodeContext<'_>,
    record: &NativeRecord,
    arena: &str,
    occurrence: &str,
) -> Result<Map<String, Value>, CodecError> {
    macro_rules! typed {
        ($type:path) => {
            record.rewrite_fields::<$type, _>(ctx, |id| match rescope(id, occurrence) {
                Some(rewritten) => Ok(rewritten),
                None => Ok(id.to_owned()),
            })?
        };
    }

    Ok(match arena {
        "body_visibilities" => typed!(crate::records::bodies::BodyVisibility),
        "creation_timestamps" => typed!(crate::records::recipes::CreationTimestamp),
        "design_body_bindings" => typed!(crate::records::bodies::DesignBodyBinding),
        "design_body_recipe_operands" => {
            typed!(crate::records::topology::body_recipe::DesignBodyRecipeOperand)
        }
        "design_dimension_recipe_records" => {
            typed!(crate::records::dimensions::DesignDimensionRecipeRecord)
        }
        "design_edge_operands" => {
            typed!(crate::records::topology::edge_identity::DesignEdgeOperand)
        }
        "design_edge_treatment_vertex_operands" => {
            typed!(crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand)
        }
        "design_face_operands" => typed!(crate::records::topology::face::DesignFaceOperand),
        "design_mesh_features" => typed!(crate::records::mesh::DesignMeshFeature),
        "design_parameter_scopes" => typed!(crate::records::feature::scope::DesignParameterScope),
        "persistent_design_links" => typed!(crate::records::sketch_links::PersistentDesignLink),
        "persistent_subentity_tags" => typed!(crate::records::sketch_links::PersistentSubentityTag),
        "sketch_curve_links" => typed!(crate::records::sketch_links::SketchCurveLink),
        _ => record.copy_fields(ctx)?,
    })
}

/// Rewrite only fields whose owners document an identity relationship.
///
/// Native records intentionally retain arbitrary source strings, including
/// configuration extensions and names that can happen to begin with `f3d:`.
/// Field names are therefore the admission boundary: this list is assembled
/// from the native record definitions and their readers, and no map key or
/// unowned string is traversed as an identity.
fn rescope_native_reference_fields(
    ctx: &DecodeContext<'_>,
    arena: &str,
    fields: &mut Map<String, Value>,
    occurrence: &str,
) -> Result<(), CodecError> {
    let direct_fields: &[&str] = match arena {
        "asm_bulletin_boards"
        | "asm_delta_states"
        | "asm_entity_changes"
        | "asm_face_boundaries"
        | "asm_history_records" => &["parent"],
        "body_native_keys" => &["body"],
        "edge_continuities" => &["edge"],
        "edge_ownerships" => &["edge", "owner_coedge"],
        "face_native_keys" | "face_sidedness" => &["face"],
        "design_body_bounds" => &["body_binding_ids"],
        "design_body_recipe_operands" | "design_edge_operands" | "design_face_operands" => {
            &["recipe_id"]
        }
        "design_dimension_recipe_records" => &["recipe_id", "matching_edge_operand_ids"],
        "design_construction_operand_groups" => &["lost_edge_references"],
        "design_extrude_selection_members" => &["operand_identity_ids"],
        "design_edge_identity_operands" => &["resolution_identity_id"],
        "design_parameter_companions" => &["owned_recipe_ids"],
        "mesh_surface_sentinels" => &["surface"],
        "tolerant_coedge_parameters" => &["coedge"],
        "tolerant_edge_tails" => &["edge"],
        "tolerant_vertex_tails" => &["vertex"],
        "transform_hints" => &["body"],
        "unknowns" => &["links"],
        "vertex_ownerships" => &["owning_edge", "vertex"],
        "wire_topologies" => &["edges", "free_vertex", "shell"],
        _ => &[],
    };
    for field in direct_fields {
        if let Some(value) = fields.get_mut(*field) {
            scope_identity_value(value, occurrence);
        }
    }

    // These records carry history-qualified selection proofs as nested
    // structs. Their `history_id` is a native record identity; the adjacent
    // historical slots are numeric source facts and remain unchanged.
    if matches!(
        arena,
        "design_entity_selection_operands"
            | "design_edge_identity_operands"
            | "design_extrude_selection_members"
    ) {
        scope_named_fields(ctx, fields, &["history_id"], occurrence)?;
    }

    // WorkPoint, mesh and edge-operand records contain native identities as ordinary strings
    // inside their nested envelopes. Their surrounding payloads also carry
    // source text, so the walker is restricted to the field names owned by the
    // corresponding native relations.
    match arena {
        "design_edge_operands" => {
            scope_named_fields(
                ctx,
                fields,
                &[
                    "result_face_boundary_ids",
                    "preceding_face_boundary_ids",
                    "preceding_support_face_boundary_ids",
                ],
                occurrence,
            )?;
        }
        "design_edge_treatment_vertex_operands" => {
            scope_named_fields(ctx, fields, &["recipe_id"], occurrence)?;
        }
        "design_mesh_features" => {
            scope_named_fields(ctx, fields, &["tessellation_id"], occurrence)?;
        }
        "xref_references" => {
            scope_named_fields(ctx, fields, &["stream"], occurrence)?;
        }
        "design_parameter_scopes" => {
            scope_named_fields(
                ctx,
                fields,
                &["history_id", "operand_id", "point_native_id", "recipe_id"],
                occurrence,
            )?;
        }
        _ => {}
    }
    Ok(())
}

fn scope_identity_value(value: &mut Value, occurrence: &str) {
    match value {
        Value::String(text) => {
            if let Some(rescoped) = rescope(text, occurrence) {
                *text = rescoped;
            }
        }
        Value::Array(items) => {
            for item in items {
                if let Value::String(text) = item {
                    if let Some(rescoped) = rescope(text, occurrence) {
                        *text = rescoped;
                    }
                }
            }
        }
        // `AttributeTarget` is the only selected object field. Its `id` is a
        // typed identity and all other members are its discriminator/value.
        Value::Object(fields) => {
            if let Some(Value::String(text)) = fields.get_mut("id") {
                if let Some(rescoped) = rescope(text, occurrence) {
                    *text = rescoped;
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn scope_named_fields(
    ctx: &DecodeContext<'_>,
    fields: &mut Map<String, Value>,
    names: &[&str],
    occurrence: &str,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("walk F3Z native identity fields")?;
    for (name, value) in fields {
        if names.contains(&name.as_str()) {
            scope_identity_value(value, occurrence);
        } else {
            scope_named_values(ctx, value, names, occurrence)?;
        }
    }
    Ok(())
}

fn scope_named_values(
    ctx: &DecodeContext<'_>,
    value: &mut Value,
    names: &[&str],
    occurrence: &str,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("walk F3Z native identity values")?;
    match value {
        Value::Object(fields) => scope_named_fields(ctx, fields, names, occurrence)?,
        Value::Array(items) => {
            for item in items {
                scope_named_values(ctx, item, names, occurrence)?;
            }
        }
        Value::String(_) | Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    mod fidelity;
    mod occurrence;

    use super::define_member;

    #[test]
    fn member_definition_keeps_a_finite_body_transform_without_preplacement() {
        let transform = cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, f64::MAX],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap();
        let mut model = cadmpeg_ir::examples::unit_cube().unwrap().model;
        model.product_definitions.clear();
        model.bodies[0].transform = Some(transform);
        let ctx = cadmpeg_test_support::service_decode_context();
        define_member(&ctx, &mut model, "member").unwrap();
        assert_eq!(model.bodies[0].transform, Some(transform));
        assert_eq!(
            model.product_definitions[0].bodies,
            [model.bodies[0].id.clone()]
        );
    }

    #[test]
    fn member_definition_obeys_the_entity_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_entities = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut model = cadmpeg_ir::document::Model::default();
        let error = define_member(&ctx, &mut model, "member").unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
        assert!(model.product_definitions.is_empty());
    }
}
