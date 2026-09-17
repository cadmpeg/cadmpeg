// SPDX-License-Identifier: Apache-2.0
//! Occurrence-scoped merge of decoded F3Z member graphs.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::document::{EntityRewrite, Model};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::SourceFidelity;
use cadmpeg_ir::{Native, NativeRecord};
use serde::de::DeserializeOwned;
use serde::Serialize;
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
    let table = xref_table_from_ir(ir)?;
    MergeSession {
        ctx,
        scan,
        archive,
        stack: vec![model_root],
    }
    .merge(ir, report, fidelity, &table)
}

/// Reassigns only repeated sibling ordinals after independent document graphs
/// have been combined.
pub(super) fn make_sibling_ordinals_unique(
    occurrences: &mut [cadmpeg_ir::products::Occurrence],
) -> Result<(), CodecError> {
    use std::collections::{HashMap, HashSet};

    let mut used = HashMap::<Option<String>, HashSet<u32>>::new();
    for occurrence in occurrences {
        let parent = match &occurrence.parent {
            cadmpeg_ir::products::OccurrenceParent::Root {} => None,
            cadmpeg_ir::products::OccurrenceParent::Occurrence { occurrence } => {
                Some(occurrence.as_str().to_owned())
            }
        };
        let siblings = used.entry(parent).or_default();
        if !siblings.insert(occurrence.ordinal) {
            occurrence.ordinal = (0..=u32::MAX)
                .find(|ordinal| siblings.insert(*ordinal))
                .ok_or_else(|| {
                    CodecError::malformed(
                        "F3Z sibling occurrence population exhausts the u32 ordinal space",
                    )
                })?;
        }
    }
    Ok(())
}

fn xref_table_from_ir(ir: &cadmpeg_ir::CadIr) -> Result<XrefTable, CodecError> {
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(XrefTable::default());
    };
    let invalid = |error| CodecError::malformed(format_args!("invalid F3D native data: {error}"));
    Ok(XrefTable {
        designs: namespace.arena_as("xref_designs").map_err(invalid)?,
        references: namespace.arena_as("xref_references").map_err(invalid)?,
        placement_failures: Vec::new(),
        placement_overrides: Vec::new(),
    })
}

/// State shared by recursive F3Z reference traversal.
struct MergeSession<'r, 'a> {
    ctx: &'r DecodeContext<'a>,
    scan: &'r ContainerScan<'a>,
    archive: &'r ArchiveSession<'a>,
    stack: Vec<String>,
}

impl MergeSession<'_, '_> {
    /// Resolves outgoing references and merges each usable component graph.
    fn merge(
        &mut self,
        parent_ir: &mut cadmpeg_ir::CadIr,
        parent_report: &mut cadmpeg_ir::codec::DecodeBody,
        parent_fidelity: &mut cadmpeg_ir::SourceFidelity,
        table: &XrefTable,
    ) -> Result<usize, CodecError> {
        let mut merged = 0usize;
        for reference in &table.references {
            let occurrence = occurrence_key(reference);
            let label = xref::design_for(table, reference).map_or_else(
                || reference.relative_path.clone(),
                |design| design.display_name.clone(),
            );
            if self.stack.contains(&reference.relative_path) {
                parent_report
                    .losses
                    .push(F3dLossCode::XrefCycle.note(format!(
                        "xref {label}: reference cycle through {}; the occurrence was not resolved",
                        reference.relative_path
                    )));
                continue;
            }
            let Some(member) = self.archive.members.get(&reference.relative_path) else {
                let (code, message) = if self.scan.entry_view(&reference.relative_path).is_some() {
                    (
                        F3dLossCode::XrefMemberUndecoded,
                        format!(
                            "xref {label}: member {} is not an F3D document member; the occurrence was not resolved",
                            reference.relative_path
                        ),
                    )
                } else {
                    (
                        F3dLossCode::XrefMemberMissing,
                        format!(
                            "xref {label}: member {} is not present in the archive; the occurrence was not resolved",
                            reference.relative_path
                        ),
                    )
                };
                parent_report.losses.push(code.note(message));
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
                Err(error) => {
                    parent_report
                        .losses
                        .push(F3dLossCode::XrefMemberUndecoded.note(format!(
                            "xref {label}: member {} failed to decode ({error}); the occurrence was not resolved",
                            reference.relative_path
                        )));
                    continue;
                }
            };
            let child_table = xref_table_from_ir(&component.ir)?;
            let cadmpeg_ir::codec::Decoded {
                ir: mut component_ir,
                body: mut component_report,
                source_fidelity: mut component_fidelity,
            } = component;
            self.stack.push(reference.relative_path.clone());
            let descendants = self.merge(
                &mut component_ir,
                &mut component_report,
                &mut component_fidelity,
                &child_table,
            )?;
            self.stack.pop();
            if let Some(transform) = reference.transform {
                apply_occurrence_transform(&mut component_ir.model, transform.rows())?;
            }
            append_feature_history(&parent_ir.model, &mut component_ir.model)?;
            let occurrence_start = parent_ir.model.occurrences.len();
            let mut scope = OccurrenceScope {
                occurrence: &occurrence,
            };
            parent_ir
                .model
                .extend_rewritten(component_ir.model, &mut scope)?;
            reparent_component_roots(
                &mut parent_ir.model.occurrences[occurrence_start..],
                &crate::ids::neutral_xref_occurrence_id(
                    reference.ordinal,
                    reference.occurrence_ordinal,
                ),
            );
            extend_native(&mut parent_ir.native, component_ir.native, &occurrence)?;
            parent_fidelity.append(rescope_fidelity(component_fidelity, &occurrence)?)?;
            merged += descendants + 1;
            if component_report.transfer.geometry_transferred() {
                parent_report.transfer = cadmpeg_ir::report::DecodeTransfer::full(true);
            }
            parent_report
                .losses
                .extend(component_report.losses.into_iter().map(|mut loss| {
                    loss.message = format!("xref {label}: {}", loss.message);
                    loss
                }));
            let placement = if reference.transform.is_some() {
                "Design occurrence transform"
            } else {
                "identity placement"
            };
            parent_report.notes.push(format!(
                "xref {label}: merged {} as occurrence {occurrence} ({placement}; {descendants} nested occurrence(s))",
                reference.relative_path
            ));
        }
        Ok(merged)
    }
}

/// Places every root-level occurrence from a merged member inside the
/// occurrence that owns that member. Child occurrence parents already carry
/// the member-local hierarchy and are left unchanged.
pub(super) fn reparent_component_roots(
    occurrences: &mut [cadmpeg_ir::products::Occurrence],
    parent: &cadmpeg_ir::ids::OccurrenceId,
) {
    for occurrence in occurrences {
        if matches!(
            occurrence.parent,
            cadmpeg_ir::products::OccurrenceParent::Root {}
        ) {
            occurrence.parent = cadmpeg_ir::products::OccurrenceParent::Occurrence {
                occurrence: parent.clone(),
            };
        }
    }
}

/// Places one component's feature history after the histories already merged.
pub(super) fn append_feature_history(
    parent: &Model,
    component: &mut Model,
) -> Result<(), CodecError> {
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
    annotations.map_ids(|id| rescope(id, occurrence).unwrap_or_else(|| id.to_owned()))?;
    // The occurrence is one owner component. Escape its separators so two
    // different occurrences cannot share an owner by shifting a path boundary.
    let owner = cadmpeg_ir::stream_name!("f3d:xref/")
        .with_suffix(crate::ids::identity_key_component(occurrence).replace('/', "%2F"))
        .with_suffix("/");
    let provenance = std::mem::take(&mut annotations.provenance);
    let mut builder = AnnotationBuilder::resume(annotations);
    let mut streams = std::collections::BTreeMap::new();
    for (id, provenance) in provenance {
        let stream = streams
            .entry(provenance.stream().to_owned())
            .or_insert_with(|| StreamHandle::new(owner.clone().with_suffix(provenance.stream())));
        let note = builder.note(id, stream, provenance.offset);
        if let Some(tag) = provenance.tag {
            note.tag(tag);
        }
    }
    let mut rescoped = SourceFidelity::with_annotations(builder.build());
    for (id, record) in records {
        let id = UnknownId::mint(
            rescope(id.as_str(), occurrence).unwrap_or_else(|| id.as_str().to_owned()),
        )
        .map_err(|error| {
            CodecError::malformed(format_args!("F3Z retained record {id}: {error}"))
        })?;
        let stream = owner.clone().with_suffix(record.stream());
        rescoped.insert_retained_record(id, record.with_owner(stream))?;
    }
    Ok(rescoped)
}

pub(super) fn occurrence_key(reference: &XrefReference) -> String {
    if reference.neutron_role.is_empty() {
        return format!(
            "ordinal-{}/occurrence-{}",
            reference.ordinal, reference.occurrence_ordinal
        );
    }
    let role = crate::ids::identity_key_component(&reference.neutron_role).replace('/', "%2F");
    // `occurrence_ordinal` restarts for each Redirections reference. Keep the
    // source reference ordinal in the scope so two admitted rows carrying the
    // same role cannot merge their model or fidelity identities.
    format!(
        "role-{role}/reference-{}/occurrence-{}",
        reference.ordinal, reference.occurrence_ordinal
    )
}

fn apply_occurrence_transform(
    model: &mut Model,
    source_rows: [[f64; 4]; 4],
) -> Result<(), CodecError> {
    if source_rows[3] != [0.0, 0.0, 0.0, 1.0] {
        return Err(CodecError::malformed(format_args!(
            "F3Z occurrence translation is not a finite affine transform"
        )));
    }
    let mut rows = [source_rows[0], source_rows[1], source_rows[2]];
    for row in &mut rows {
        row[3] *= 10.0;
    }
    let occurrence = cadmpeg_ir::transform::Transform::affine(rows).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "F3Z occurrence translation is not a finite affine transform"
        ))
    })?;
    for body in &mut model.bodies {
        body.transform = Some(match body.transform {
            Some(local) => compose_transforms(occurrence, local)?,
            None => occurrence,
        });
    }
    Ok(())
}

/// Composes a component-local transform after its archive occurrence transform.
pub(super) fn compose_transforms(
    outer: cadmpeg_ir::transform::Transform,
    inner: cadmpeg_ir::transform::Transform,
) -> Result<cadmpeg_ir::transform::Transform, CodecError> {
    outer.compose(inner).map_err(|error| {
        CodecError::malformed(format_args!(
            "F3Z occurrence composition is not a finite affine transform: {error}"
        ))
    })
}

fn rescope(text: &str, occurrence: &str) -> Option<String> {
    text.strip_prefix("f3d:")
        .map(|rest| format!("f3d:xref/{occurrence}/{rest}"))
}

/// Rewrites every `f3d:` identity in one model entity into occurrence scope.
pub(super) struct OccurrenceScope<'a> {
    pub(super) occurrence: &'a str,
}

impl EntityRewrite for OccurrenceScope<'_> {
    type Error = CodecError;

    fn rewrite<T: Serialize + DeserializeOwned>(&mut self, entity: T) -> Result<T, CodecError> {
        let rewritten = cadmpeg_ir::schema::rewrite::identities(&entity, |id| {
            rescope(id, self.occurrence).unwrap_or_else(|| id.to_owned())
        });
        let value = serde_value::to_value(rewritten).map_err(|error| {
            CodecError::malformed(format_args!("model serialization failed: {error}"))
        })?;
        crate::value_tree::from_value(value).map_err(|error| {
            CodecError::malformed(format_args!("merged model round-trip failed: {error}"))
        })
    }
}

/// Appends all known component-native arenas after occurrence-local rescoping.
pub(super) fn extend_native(
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
        arena.reserve(records.len());
        for record in records {
            arena.push(rescope_record(&record, name, occurrence)?);
        }
    }
    Ok(())
}

/// Rescopes one native record's identity and every identity it references.
pub(super) fn rescope_record(
    record: &NativeRecord,
    arena: &str,
    occurrence: &str,
) -> Result<NativeRecord, cadmpeg_ir::native::NativeConvertError> {
    let mut fields = typed_fields(record, arena, occurrence)?;
    rescope_native_reference_fields(arena, &mut fields, occurrence);
    let id = rescope(record.id(), occurrence).unwrap_or_else(|| record.id().to_owned());
    NativeRecord::new(id, fields)
}

/// Rewrite typed identity markers before JSON erases their ownership.
///
/// Most F3D native records carry source text and numeric stream facts. The
/// records listed here also carry an IR identity marker inside a structured
/// field. Going through the typed owner keeps that marker distinct from an
/// ordinary `String` while the field-specific pass below handles native text
/// references that have not yet gained a newtype.
fn typed_fields(
    record: &NativeRecord,
    arena: &str,
    occurrence: &str,
) -> Result<Map<String, Value>, cadmpeg_ir::native::NativeConvertError> {
    let typed_error = |error: serde_json::Error| {
        cadmpeg_ir::native::NativeConvertError::InvalidCollection(
            format_args!(
                "F3D native arena `{arena}` record `{}` typed admission: {error}",
                record.id()
            )
            .to_string(),
        )
    };
    macro_rules! typed {
        ($type:path) => {{
            let mut value = Value::Object(record.fields());
            let Value::Object(fields) = &mut value else {
                return Err(cadmpeg_ir::native::NativeConvertError::NonObject);
            };
            fields.insert("id".into(), Value::String(record.id().into()));
            let typed: $type = serde_json::from_value(value).map_err(typed_error)?;
            let rewritten = cadmpeg_ir::schema::rewrite::identities(&typed, |id| {
                rescope(id, occurrence).unwrap_or_else(|| id.to_owned())
            });
            let Value::Object(mut fields) = serde_json::to_value(rewritten).map_err(typed_error)?
            else {
                return Err(cadmpeg_ir::native::NativeConvertError::NonObject);
            };
            fields.remove("id");
            fields
        }};
    }

    Ok(match arena {
        "body_visibilities" => typed!(crate::records::bodies::BodyVisibility),
        "creation_timestamps" => typed!(crate::records::recipes::CreationTimestamp),
        "design_body_bindings" => typed!(crate::records::bodies::DesignBodyBinding),
        "design_body_recipe_operands" => typed!(crate::records::topology::DesignBodyRecipeOperand),
        "design_dimension_recipe_records" => {
            typed!(crate::records::dimensions::DesignDimensionRecipeRecord)
        }
        "design_edge_operands" => typed!(crate::records::topology::DesignEdgeOperand),
        "design_edge_treatment_vertex_operands" => {
            typed!(crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand)
        }
        "design_face_operands" => typed!(crate::records::topology::DesignFaceOperand),
        "design_mesh_features" => typed!(crate::records::mesh::DesignMeshFeature),
        "design_parameter_scopes" => typed!(crate::records::feature::scope::DesignParameterScope),
        "persistent_design_links" => typed!(crate::records::sketch_links::PersistentDesignLink),
        "persistent_subentity_tags" => typed!(crate::records::sketch_links::PersistentSubentityTag),
        "sketch_curve_links" => typed!(crate::records::sketch_links::SketchCurveLink),
        _ => record.fields(),
    })
}

/// Rewrite only fields whose owners document an identity relationship.
///
/// Native records intentionally retain arbitrary source strings, including
/// configuration extensions and names that can happen to begin with `f3d:`.
/// Field names are therefore the admission boundary: this list is assembled
/// from the native record definitions and their readers, and no map key or
/// unowned string is traversed as an identity.
fn rescope_native_reference_fields(arena: &str, fields: &mut Map<String, Value>, occurrence: &str) {
    let direct_fields: &[&str] = match arena {
        "asm_bulletin_boards"
        | "asm_delta_states"
        | "asm_entity_changes"
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
        scope_named_fields(fields, &["history_id"], occurrence);
    }

    // WorkPoint and mesh records contain native identities as ordinary strings
    // inside their nested envelopes. Their surrounding payloads also carry
    // source text, so the walker is restricted to the field names owned by the
    // corresponding native relations.
    match arena {
        "design_edge_treatment_vertex_operands" => {
            scope_named_fields(fields, &["recipe_id"], occurrence);
        }
        "design_mesh_features" => {
            scope_named_fields(fields, &["tessellation_id"], occurrence);
        }
        "design_parameter_scopes" => {
            scope_named_fields(
                fields,
                &["history_id", "operand_id", "point_native_id", "recipe_id"],
                occurrence,
            );
        }
        _ => {}
    }
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

fn scope_named_fields(fields: &mut Map<String, Value>, names: &[&str], occurrence: &str) {
    for (name, value) in fields {
        if names.contains(&name.as_str()) {
            scope_identity_value(value, occurrence);
        } else {
            scope_named_values(value, names, occurrence);
        }
    }
}

fn scope_named_values(value: &mut Value, names: &[&str], occurrence: &str) {
    match value {
        Value::Object(fields) => scope_named_fields(fields, names, occurrence),
        Value::Array(items) => {
            for item in items {
                scope_named_values(item, names, occurrence);
            }
        }
        Value::String(_) | Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests {
    mod fidelity;

    use super::*;

    #[test]
    fn occurrence_translation_overflow_is_rejected() {
        let rows = [
            [1.0, 0.0, 0.0, f64::MAX],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let source = cadmpeg_ir::transform::Transform::affine(rows).unwrap();
        let error = apply_occurrence_transform(&mut Model::default(), source.rows()).unwrap_err();
        assert!(error
            .to_string()
            .contains("F3Z occurrence translation is not a finite affine transform"));
    }

    #[test]
    fn occurrence_composition_overflow_is_rejected() {
        let transform = cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, f64::MAX],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap();
        let error = compose_transforms(transform, transform).unwrap_err();
        assert!(error
            .to_string()
            .contains("F3Z occurrence composition is not a finite affine transform"));
    }
}
