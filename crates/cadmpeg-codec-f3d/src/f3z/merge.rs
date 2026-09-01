// SPDX-License-Identifier: Apache-2.0
//! Occurrence-scoped merge of decoded F3Z member graphs.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::{EntityRewrite, Model};
use cadmpeg_ir::{Native, NativeRecord};
use serde::{de::DeserializeOwned, Serialize};
use serde_value::Value;

use super::archive::{ArchiveSession, ClassifiedMember};
use crate::container::ContainerScan;
use crate::loss::F3dLossCode;
use crate::records::XrefReference;
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
pub(super) fn make_sibling_ordinals_unique(occurrences: &mut [cadmpeg_ir::products::Occurrence]) {
    use std::collections::{HashMap, HashSet};

    let mut used = HashMap::<Option<String>, HashSet<u32>>::new();
    for occurrence in occurrences {
        let parent = match &occurrence.parent {
            cadmpeg_ir::products::OccurrenceParent::Root => None,
            cadmpeg_ir::products::OccurrenceParent::Occurrence { occurrence } => {
                Some(occurrence.0.clone())
            }
        };
        let siblings = used.entry(parent).or_default();
        if !siblings.insert(occurrence.ordinal) {
            occurrence.ordinal = (0..=u32::MAX)
                .find(|ordinal| siblings.insert(*ordinal))
                .expect("an in-memory occurrence population cannot exhaust u32 ordinals");
        }
    }
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
            let component = match crate::decode::decode_archive_member(self.ctx, member_scan) {
                Ok(component) => component,
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
            if component.ir.units != parent_ir.units {
                parent_report
                    .losses
                    .push(F3dLossCode::XrefUnitsMismatch.note(format!(
                        "xref {label}: component units differ from the containing document; the occurrence was not merged"
                    )));
                continue;
            }
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
                apply_occurrence_transform(&mut component_ir.model, transform);
            }
            append_feature_history(&parent_ir.model, &mut component_ir.model)?;
            let mut scope = OccurrenceScope {
                occurrence: &occurrence,
            };
            parent_ir
                .model
                .extend_rewritten(component_ir.model, &mut scope)?;
            extend_native(&mut parent_ir.native, component_ir.native, &occurrence);
            merge_annotations(
                &mut parent_fidelity.annotations,
                component_fidelity.annotations,
                &occurrence,
            )?;
            merged += descendants + 1;
            if component_report.transfer.geometry_transferred() {
                parent_report.transfer = cadmpeg_ir::DecodeTransfer::full(true);
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

fn merge_annotations(
    target: &mut cadmpeg_ir::annotations::Annotations,
    source: cadmpeg_ir::annotations::Annotations,
    occurrence: &str,
) -> Result<(), CodecError> {
    let mut stream_map = Vec::with_capacity(source.streams.len());
    for stream in source.streams {
        let index = if let Some(index) = target
            .streams
            .iter()
            .position(|candidate| candidate == &stream)
        {
            index
        } else {
            target.streams.push(stream);
            target.streams.len() - 1
        };
        stream_map.push(u32::try_from(index).map_err(|_| {
            CodecError::Malformed("merged F3Z annotation stream count exceeds u32::MAX".into())
        })?);
    }
    for (id, mut provenance) in source.provenance {
        let stream = usize::try_from(provenance.stream)
            .ok()
            .and_then(|index| stream_map.get(index))
            .copied()
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "component annotation {id} references missing stream {}",
                    provenance.stream
                ))
            })?;
        provenance.stream = stream;
        target
            .provenance
            .insert(remap_id_text(&id, occurrence), provenance);
    }
    target.exactness.extend(
        source
            .exactness
            .into_iter()
            .map(|(id, note)| (remap_id_text(&id, occurrence), note)),
    );
    Ok(())
}

fn remap_id_text(text: &str, occurrence: &str) -> String {
    text.strip_prefix("f3d:").map_or_else(
        || text.to_owned(),
        |rest| format!("f3d:xref/{occurrence}/{rest}"),
    )
}

fn occurrence_key(reference: &XrefReference) -> String {
    let role = if reference.neutron_role.is_empty() {
        format!("ordinal-{}", reference.ordinal)
    } else {
        reference.neutron_role.clone()
    };
    format!("{role}/occurrence-{}", reference.occurrence_ordinal)
}

fn apply_occurrence_transform(model: &mut Model, source_rows: [[f64; 4]; 4]) {
    let mut occurrence = cadmpeg_ir::transform::Transform { rows: source_rows };
    for row in 0..3 {
        occurrence.rows[row][3] *= 10.0;
    }
    for body in &mut model.bodies {
        body.transform = Some(match body.transform {
            Some(local) => compose_transforms(occurrence, local),
            None => occurrence,
        });
    }
}

/// Composes a component-local transform after its archive occurrence transform.
pub(super) fn compose_transforms(
    outer: cadmpeg_ir::transform::Transform,
    inner: cadmpeg_ir::transform::Transform,
) -> cadmpeg_ir::transform::Transform {
    let mut rows = [[0.0; 4]; 4];
    for (row, values) in rows.iter_mut().enumerate() {
        for (column, value) in values.iter_mut().enumerate() {
            *value = (0..4)
                .map(|index| outer.rows[row][index] * inner.rows[index][column])
                .sum();
        }
    }
    cadmpeg_ir::transform::Transform { rows }
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
        let mut value = serde_value::to_value(entity).map_err(|error| {
            CodecError::malformed(format_args!("model serialization failed: {error}"))
        })?;
        remap_ids(&mut value, self.occurrence);
        crate::value_tree::from_value(value).map_err(|error| {
            CodecError::malformed(format_args!("merged model round-trip failed: {error}"))
        })
    }
}

fn remap_ids(value: &mut Value, occurrence: &str) {
    match value {
        Value::String(text) => {
            if let Some(rescoped) = rescope(text, occurrence) {
                *text = rescoped;
            }
        }
        Value::Seq(items) => {
            for item in items {
                remap_ids(item, occurrence);
            }
        }
        Value::Map(fields) => {
            let entries = std::mem::take(fields);
            for (mut key, mut item) in entries {
                remap_ids(&mut key, occurrence);
                remap_ids(&mut item, occurrence);
                fields.insert(key, item);
            }
        }
        Value::Option(Some(item)) | Value::Newtype(item) => remap_ids(item, occurrence),
        _ => {}
    }
}

/// Appends all known component-native arenas after occurrence-local rescoping.
pub(super) fn extend_native(root: &mut Native, mut component: Native, occurrence: &str) {
    let Some(mut source) = component.0.remove("f3d") else {
        return;
    };
    let target = root.namespace_mut("f3d");
    for name in crate::native::F3D_ARENA_NAMES
        .iter()
        .copied()
        .chain(std::iter::once("unknowns"))
    {
        let Some(records) = source.arenas.remove(name) else {
            continue;
        };
        if records.is_empty() {
            continue;
        }
        let arena = target.arenas.entry(name.to_string()).or_default();
        arena.reserve(records.len());
        for record in records {
            arena.push(rescope_record(&record, occurrence));
        }
    }
}

/// Rescopes one native record's identity and every identity it references.
pub(super) fn rescope_record(record: &NativeRecord, occurrence: &str) -> NativeRecord {
    let mut fields = record.fields();
    rescope_json_fields(&mut fields, occurrence);
    let id = rescope(record.id(), occurrence).unwrap_or_else(|| record.id().to_owned());
    NativeRecord::new(id, fields)
}

fn rescope_json(value: &mut serde_json::Value, occurrence: &str) {
    match value {
        serde_json::Value::String(text) => {
            if let Some(rescoped) = rescope(text, occurrence) {
                *text = rescoped;
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                rescope_json(item, occurrence);
            }
        }
        serde_json::Value::Object(fields) => rescope_json_fields(fields, occurrence),
        _ => {}
    }
}

fn rescope_json_fields(fields: &mut serde_json::Map<String, serde_json::Value>, occurrence: &str) {
    if fields.keys().any(|key| key.starts_with("f3d:")) {
        for (key, mut value) in std::mem::take(fields) {
            rescope_json(&mut value, occurrence);
            fields.insert(rescope(&key, occurrence).unwrap_or(key), value);
        }
        return;
    }
    for value in fields.values_mut() {
        rescope_json(value, occurrence);
    }
}
