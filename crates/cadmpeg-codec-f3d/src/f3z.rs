// SPDX-License-Identifier: Apache-2.0
//! Decode a multi-document `.f3z` archive
//! ([spec §1.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#15-multi-document-archives-f3z)).
//!
//! A `.f3z` holds `Manifest.json` (naming its root document),
//! `DesignDescription.json`, and one member per document. [`decode`] decodes a
//! 3D root or the sole derived 3D model of a drawing root, recursively resolves
//! each 3D member's outgoing XREFs, and merges the component models with every
//! occurrence-local Design placement applied from child to ancestor.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_value::Value;
use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::{CodecError, ContainerSummary};
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::document::{EntityRewrite, Model};
use cadmpeg_ir::{LossNote, Native, NativeRecord};

use crate::container::ContainerScan;
use crate::loss::F3dLossCode;
use crate::records::XrefReference;
use crate::xref::{self, XrefTable};

/// The archive-level member naming the root document.
pub const MANIFEST_ENTRY: &str = "Manifest.json";
/// The archive-level design-graph member.
pub const DESIGN_DESCRIPTION_ENTRY: &str = "DesignDescription.json";

#[derive(Deserialize)]
struct ManifestJson {
    root: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesignDescriptionJson {
    design_description: DesignDescription,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesignDescription {
    design_graphs: Vec<DesignGraph>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesignGraph {
    root_ids: Vec<u64>,
    design_objects: Vec<DesignObject>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesignObject {
    id: u64,
    relative_path: String,
    content_type: String,
    references: Vec<DesignObjectReference>,
}

#[derive(Deserialize)]
struct DesignObjectReference {
    #[serde(rename = "type")]
    reference_type: String,
    ids: Vec<u64>,
}

/// Inspect every document member under the F3Z archive identity.
pub(crate) fn inspect<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<ContainerSummary, CodecError> {
    let manifest: ManifestJson = serde_json::from_slice(scan.entry_bytes(MANIFEST_ENTRY)?)
        .map_err(|error| {
            CodecError::malformed(format_args!("{MANIFEST_ENTRY} is not valid JSON: {error}"))
        })?;
    let (model_root, _) = model_root_member(scan, &manifest.root)?;
    scan.entry_view(&model_root).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "f3z root member {model_root} is not present in the archive"
        ))
    })?;
    let classified = classify_archive_members(ctx, scan)?;
    let member_count = scan
        .entries
        .iter()
        .filter(|entry| crate::container::is_f3d_name(&entry.name))
        .count();
    let mut notes = vec![format!(
        "f3z archive: {member_count} document member(s); model root {model_root}"
    )];
    notes.extend(
        classified
            .losses
            .iter()
            .map(|loss| format!("archive classification loss: {}", loss.message)),
    );
    Ok(ContainerSummary::classified(
        classified.layers,
        "zip",
        scan.entries.clone(),
        notes,
    ))
}

/// Decode a scanned `.f3z` archive into one merged document.
pub fn decode<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<DecodeResult, CodecError> {
    let manifest: ManifestJson = serde_json::from_slice(scan.entry_bytes(MANIFEST_ENTRY)?)
        .map_err(|error| {
            CodecError::malformed(format_args!("{MANIFEST_ENTRY} is not valid JSON: {error}"))
        })?;
    let (model_root, omitted_drawing_root) = model_root_member(scan, &manifest.root)?;
    let outer = classify_archive_members(ctx, scan)?;
    let root_scan = outer.member_scan(&model_root)?;
    let (mut ir, mut report, mut fidelity) =
        crate::decode::decode_archive_member(ctx, root_scan)?.into_parts();
    fidelity
        .retained_records
        .retain(|record| record.id != crate::ids::FILE_SOURCE_IMAGE_ID);
    fidelity.retain_unknown_records("f3d", [crate::decode::preserve_source_image(scan)]);
    if let Some(drawing_root) = omitted_drawing_root {
        report
            .losses
            .push(F3dLossCode::DrawingDocumentOmitted.note(format!(
            "drawing root {drawing_root} is omitted; decoded its unambiguous derived model {model_root}"
        )));
    }
    let member_count = scan
        .entries
        .iter()
        .filter(|entry| crate::container::is_f3d_name(&entry.name))
        .count();
    report.notes.push(format!(
        "f3z archive: {member_count} document member(s); root {model_root}"
    ));
    if ctx.container_only() {
        return finalize_result(ir, classify_outer_report(report, outer), fidelity);
    }

    let table = xref_table_from_ir(&ir)?;
    let merged = MergeSession {
        ctx,
        scan,
        archive: &outer,
        stack: vec![model_root],
    }
    .merge(&mut ir, &mut report, &mut fidelity, &table)?;
    if merged > 0 {
        fidelity
            .retained_records
            .retain(|record| record.id != crate::ids::FILE_SOURCE_IMAGE_ID);
        report.notes.push(format!(
            "{merged} merged component(s) retain occurrence-scoped model entities and native \
             records; member source streams remain archive-local"
        ));
    }
    report.notes.push(format!(
        "merged {merged} external occurrence(s) from the f3z archive"
    ));
    make_sibling_ordinals_unique(&mut ir.model.occurrences);
    finalize_result(ir, classify_outer_report(report, outer), fidelity)
}

fn classify_outer_report(
    mut report: cadmpeg_ir::DecodeReport,
    outer: ArchiveSession<'_>,
) -> cadmpeg_ir::DecodeReport {
    // The archive pass owns member identity and its losses, including members
    // no XREF traversal reached. Intermediate member reports are unclassified.
    report.losses.extend(outer.losses);
    cadmpeg_ir::DecodeReport::classified(
        outer.layers,
        report.transfer(),
        report.coverage,
        report.losses,
        report.notes,
        report.transfer_ledger,
    )
}

fn finalize_result(
    mut ir: cadmpeg_ir::CadIr,
    report: cadmpeg_ir::DecodeReport,
    fidelity: cadmpeg_ir::SourceFidelity,
) -> Result<DecodeResult, CodecError> {
    // The decoded model root is an unclassified intermediate member. The final
    // document is the containing F3Z archive, whose report primary is the sole
    // authority for source identity. Preserve member-derived attributes before
    // DecodeResult mirrors the archive primary.
    if let Some(source) = ir.source.as_mut() {
        *source = cadmpeg_ir::SourceMeta::unclassified(
            report.format(),
            std::mem::take(&mut source.attributes),
        );
    }
    let mut result = DecodeResult::new(ir, report, fidelity)?;
    let hash = crate::decode::document_local_sha256(result.ir());
    if let Some(source) = &mut result.ir_mut().source {
        source.attributes.insert(
            cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE.into(),
            hash,
        );
    }
    Ok(result)
}

/// Classify every F3D document member and attach its layers to its archive path.
struct ArchiveSession<'a> {
    members: BTreeMap<String, ClassifiedMember<'a>>,
    layers: DialectLayers,
    losses: Vec<LossNote>,
}

enum ClassifiedMember<'a> {
    Scanned(Box<ContainerScan<'a>>),
    Unreadable(String),
}

impl ArchiveSession<'_> {
    fn member_scan(&self, path: &str) -> Result<&ContainerScan<'_>, CodecError> {
        match self.members.get(path) {
            Some(ClassifiedMember::Scanned(scan)) => Ok(scan),
            Some(ClassifiedMember::Unreadable(message)) => Err(CodecError::malformed(
                format_args!("f3z document member {path} could not be scanned: {message}"),
            )),
            None => Err(CodecError::malformed(format_args!(
                "f3z document member {path} is not present in the archive"
            ))),
        }
    }
}

fn classify_archive_members<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<ArchiveSession<'a>, CodecError> {
    let mut members = BTreeMap::new();
    let mut layers = DialectLayers::of(scan.dialect.clone());
    let mut losses = Vec::new();
    for member_path in scan
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .filter(|name| crate::container::is_f3d_name(name))
    {
        let member_view = scan.entry_view(member_path).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "f3z document member {member_path} is not readable"
            ))
        })?;
        let member_scan = match crate::container::scan(ctx, member_view) {
            Ok(member_scan) => member_scan,
            Err(error) => {
                let message = error.to_string();
                losses.push(F3dLossCode::XrefMemberUndecoded.note(format!(
                    "xref {member_path}: member could not be scanned as an F3D document ({message}); its source bytes remain retained"
                )));
                members.insert(
                    member_path.to_owned(),
                    ClassifiedMember::Unreadable(message),
                );
                continue;
            }
        };
        let classification = crate::report::classify_document(&member_scan);
        let (member_layers, member_losses) = classification.into_parts();
        losses.extend(member_losses.into_iter().map(|mut loss| {
            loss.message = format!("archive member {member_path}: {}", loss.message);
            loss
        }));
        losses.extend(merge_member_layers(
            &mut layers,
            &member_layers,
            member_path,
        ));
        members.insert(
            member_path.to_owned(),
            ClassifiedMember::Scanned(Box::new(member_scan)),
        );
    }
    losses.extend(crate::report::dialect_losses(&layers));
    Ok(ArchiveSession {
        members,
        layers,
        losses,
    })
}

/// Attach one archive member's identity and nested layers to its archive path.
fn merge_member_layers(
    target: &mut DialectLayers,
    member: &DialectLayers,
    member_path: &str,
) -> Vec<LossNote> {
    let mut losses = Vec::new();
    for matched in member.iter().cloned() {
        let instance = matched.instance().map_or_else(
            || member_path.to_owned(),
            |nested| format!("{member_path}/{nested}"),
        );
        let mut declared = matched.declared().clone();
        declared.insert(
            crate::dialect::DECLARED_ARCHIVE_MEMBER.to_owned(),
            member_path.to_owned(),
        );
        let matched = matched.with_declared(declared).with_instance(instance);
        let format = matched.format().to_owned();
        let instance = matched
            .instance()
            .expect("attached member layers have instances")
            .to_owned();
        if target.try_push(matched).is_err() {
            losses.push(F3dLossCode::DialectLayerCollision.note(format!(
                "archive member {member_path} produced a duplicate {format} dialect layer at instance {instance}; the later layer was omitted",
            )));
        }
    }
    losses
}

fn model_root_member(
    scan: &ContainerScan<'_>,
    archive_root: &str,
) -> Result<(String, Option<String>), CodecError> {
    if crate::container::is_f3d_name(archive_root) {
        return Ok((archive_root.to_owned(), None));
    }

    let description: DesignDescriptionJson =
        serde_json::from_slice(scan.entry_bytes(DESIGN_DESCRIPTION_ENTRY)?).map_err(|error| {
            CodecError::malformed(format_args!(
                "{DESIGN_DESCRIPTION_ENTRY} is not valid JSON: {error}"
            ))
        })?;
    let mut candidates = Vec::new();
    for graph in description.design_description.design_graphs {
        let Some(root) = graph.design_objects.iter().find(|object| {
            graph.root_ids.contains(&object.id) && object.relative_path == archive_root
        }) else {
            continue;
        };
        let derived_ids = root
            .references
            .iter()
            .filter(|reference| reference.reference_type == "DERIVED")
            .flat_map(|reference| reference.ids.iter().copied())
            .collect::<Vec<_>>();
        for object in &graph.design_objects {
            if derived_ids.contains(&object.id)
                && object.content_type.eq_ignore_ascii_case("f3d")
                && crate::container::is_f3d_name(&object.relative_path)
                && scan.entry_view(&object.relative_path).is_some()
            {
                candidates.push(object.relative_path.clone());
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    match candidates.as_slice() {
        [model_root] => Ok((model_root.clone(), Some(archive_root.to_owned()))),
        _ => Err(CodecError::malformed(format_args!(
            "f3z root member {archive_root} is not an f3d document and has {} unambiguous derived f3d model members",
            candidates.len()
        ))),
    }
}

fn make_sibling_ordinals_unique(occurrences: &mut [cadmpeg_ir::products::Occurrence]) {
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

/// Read the two external-reference arenas out of a decoded document.
///
/// Only those two arenas are parsed. A member's retained source population
/// reaches hundreds of thousands of records, and deserializing all of them to
/// find its outgoing references costs far more than the archive traversal it
/// serves.
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
    /// Resolve `table`'s outgoing references against the archive and merge each
    /// resolved member into `parent`, returning the number of merged occurrences.
    ///
    /// `parent` accumulates in its own document form: a member's arenas are
    /// appended entity by entity and record by record as it is resolved, and the
    /// member is dropped before the next one is decoded. Nothing beyond the
    /// neutral and native model population of the merged document and one decoded
    /// member is resident at any point. The lightweight container scans retained
    /// by [`ArchiveSession`] borrow archive storage and hold classification facts.
    ///
    /// A reference that cannot be resolved -- a cycle, an absent member, a member
    /// that fails to decode, or one whose units differ -- is recorded as a loss and
    /// skipped, leaving the rest of the archive to merge.
    fn merge(
        &mut self,
        parent_ir: &mut cadmpeg_ir::CadIr,
        parent_report: &mut cadmpeg_ir::DecodeReport,
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
                        "xref {label}: member {} failed to decode ({error}); the occurrence was \
                     not resolved",
                        reference.relative_path
                    )));
                    continue;
                }
            };
            if component.ir().units != parent_ir.units {
                parent_report
                    .losses
                    .push(F3dLossCode::XrefUnitsMismatch.note(format!(
                        "xref {label}: component units differ from the containing document; the \
                 occurrence was not merged"
                    )));
                continue;
            }
            let child_table = xref_table_from_ir(component.ir())?;
            let (mut component_ir, mut component_report, mut component_fidelity) =
                component.into_parts();
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
            if component_report.geometry_transferred() {
                parent_report.mark_geometry_transferred();
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
            "xref {label}: merged {} as occurrence {occurrence} ({placement}; {descendants} nested \
             occurrence(s))",
            reference.relative_path
        ));
        }
        Ok(merged)
    }
}

/// Places one component's feature history after every feature already merged.
///
/// Feature ordinals are document-global in CADIR. Source documents number their
/// histories independently, so an assembly merge translates the component's
/// minimum ordinal to the next available ordinal and preserves every relative
/// ordinal difference within that history.
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

/// The id-prefix key for one occurrence: its role string, or its ordinal when
/// the role is absent.
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

fn compose_transforms(
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

/// Rescope one `f3d:`-namespaced identity to `f3d:xref/<occurrence>/<rest>`,
/// and leave every other string alone.
///
/// Model entity ids, their cross-references, and native record ids all carry
/// the `f3d:` prefix, so applying the rewrite to every string of an entity or
/// record keeps each occurrence's graph internally consistent and disjoint from
/// the root's without knowing which strings are identities.
fn rescope(text: &str, occurrence: &str) -> Option<String> {
    text.strip_prefix("f3d:")
        .map(|rest| format!("f3d:xref/{occurrence}/{rest}"))
}

/// Rescopes one model entity at a time while its arena is appended onto the
/// root model's.
struct OccurrenceScope<'a> {
    occurrence: &'a str,
}

impl EntityRewrite for OccurrenceScope<'_> {
    type Error = CodecError;

    /// Model entities are rescoped through [`serde_value`] rather than through
    /// JSON: their coordinate fields are `f64`, and JSON cannot carry a
    /// non-finite one.
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

/// Rescope every string of one serialized model entity, map keys included.
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

/// Append every typed and reserved `f3d` native arena of `component` onto the
/// root's, rescoping each record into `occurrence`.
///
/// Records are moved across one at a time in their stored JSON form. The whole
/// merged population is never held as a parsed value tree, and neither is the
/// whole component's: only the record being rescoped is parsed.
fn extend_native(root: &mut Native, mut component: Native, occurrence: &str) {
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

/// Rescope one native record's identity and every identity it references.
fn rescope_record(record: &NativeRecord, occurrence: &str) -> NativeRecord {
    let mut fields = record.fields();
    rescope_json_fields(&mut fields, occurrence);
    let id = rescope(record.id(), occurrence).unwrap_or_else(|| record.id().to_owned());
    NativeRecord::new(id, fields)
}

/// Rescope every string of one native record field value.
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

/// Rescope one JSON object's values, and its keys when a record keys a map by
/// identity. Rescoping a key can move it, so the map is rebuilt in that case
/// and left in place otherwise.
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

#[cfg(test)]
mod tests;
