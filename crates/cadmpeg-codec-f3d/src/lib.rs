// SPDX-License-Identifier: Apache-2.0
//! Read and write Autodesk Fusion `.f3d` archives.
//!
//! [`F3dCodec`] implements [`Codec`] and [`Encoder`]. Decoding produces a
//! [`CadIr`] document with B-rep topology, analytic and cached NURBS geometry,
//! body transforms, design and sketch records, construction history, and
//! appearances. Encoding replays an unchanged decoded archive byte for byte,
//! applies supported semantic edits to retained source data, or creates an
//! archive from the supported source-less profile.
//!
//! Support level: [L4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder)
//! on the cadmpeg support ladder.
//!
//! # Decode
//!
//! ```no_run
//! use cadmpeg_codec_f3d::F3dCodec;
//! use cadmpeg_ir::{Codec, DecodeOptions};
//! use std::fs::File;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.f3d")?;
//! let result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;
//! for loss in &result.report.losses {
//!     eprintln!("{:?}: {}", loss.severity, loss.message);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [`Codec::inspect`] classifies the ZIP entries and reads ASM B-rep headers
//! without building geometry. `DecodeOptions::container_only` provides the
//! corresponding metadata-only `CadIr`.
//!
//! # Encode
//!
//! ```no_run
//! use cadmpeg_codec_f3d::F3dCodec;
//! use cadmpeg_ir::{Codec, DecodeOptions, Encoder};
//! use std::fs::File;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.f3d")?;
//! let mut result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;
//! // Edit supported fields in result.ir.
//! let mut output = File::create("part-edited.f3d")?;
//! F3dCodec.encode(&result.ir, &mut output)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Data flow
//!
//! [`container`] selects the authoritative `.smbh` B-rep, or the first `.smb`
//! construction snapshot when no `.smbh` exists. [`sab`] frames its active
//! record slice. [`brep`] builds the topology chain from bodies through
//! vertices and points, while [`nurbs`] decodes cached spline carriers.
//! [`design`], [`history`], and [`materials`] populate source-native records and
//! appearance bindings.
//!
//! ASM model-space lengths become millimetres. Directions, ratios, angles,
//! knots, weights, and UV parameters retain their native scale.
//!
//! Inspect [`cadmpeg_ir::report::DecodeReport::losses`] before consuming a
//! decode. A stream that cannot produce geometry returns container metadata,
//! retained source data, and blocking geometry and topology losses. Referenced
//! carrier bytes needed for passthrough remain available as
//! [`cadmpeg_ir::unknown::UnknownRecord`] values.

mod act;
pub mod asm_header;
pub mod brep;
pub mod container;
pub mod decode;
pub mod design;
pub mod history;
mod history_records;
pub mod materials;
mod native;
pub mod nurbs;
pub mod records;
pub mod sab;
mod writer;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult, Encoder, ReadSeek,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::{Check, Finding, Severity};
use std::io::Write;

/// The ZIP local-file-header magic.
const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

/// The Autodesk Fusion `.f3d` container codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct F3dCodec;

/// Validate Fusion native design-record relationships and exact sketch frames.
pub fn validate_native(ir: &CadIr) -> Vec<Finding> {
    use std::collections::HashSet;

    let Some(namespace) = ir.native.namespace("f3d") else {
        return Vec::new();
    };
    if namespace.version != native::F3D_NATIVE_VERSION {
        let version = namespace.version;
        return vec![Finding {
            check: Check::Version,
            severity: Severity::Error,
            message: format!("unsupported Fusion native namespace version {version}"),
            entity: None,
        }];
    }
    let Ok(native) = native::F3dNative::load(namespace) else {
        return vec![Finding {
            check: Check::NativeLinks,
            severity: Severity::Error,
            message: "Fusion native namespace does not match schema version 1".into(),
            entity: None,
        }];
    };
    let mut findings = Vec::new();
    let record_indices = native
        .design_record_headers
        .iter()
        .map(|record| record.record_index)
        .collect::<HashSet<_>>();
    let mut parameter_indices = HashSet::new();
    let mut parameter_ordinals = HashSet::new();
    let parameters_by_index = native
        .design_parameters
        .iter()
        .map(|parameter| (parameter.record_index, parameter))
        .collect::<std::collections::HashMap<_, _>>();
    let owners_by_index = native
        .design_parameter_owners
        .iter()
        .map(|owner| (owner.record_index, owner))
        .collect::<std::collections::HashMap<_, _>>();
    let scopes_by_index = native
        .design_parameter_scopes
        .iter()
        .map(|scope| (scope.record_index, scope))
        .collect::<std::collections::HashMap<_, _>>();
    let entities_by_suffix = native
        .design_entity_headers
        .iter()
        .map(|entity| (entity.entity_suffix, entity))
        .collect::<std::collections::HashMap<_, _>>();
    let mut scope_indices = HashSet::new();
    for scope in &native.design_parameter_scopes {
        let unique_index = scope_indices.insert(scope.record_index);
        let entity_link = match (
            scope.entity_id.as_deref(),
            scope.entity_suffix,
            scope.entity_reference_offset,
        ) {
            (None, None, None) => None,
            (Some(entity_id), Some(entity_suffix), Some(offset)) => Some(
                entities_by_suffix
                    .get(&entity_suffix)
                    .is_some_and(|entity| {
                        entity.entity_id == entity_id
                            && offset > scope.byte_offset
                            && offset < scope.paired_byte_offset
                    }),
            ),
            _ => Some(false),
        };
        let valid = scope.class_tag.len() == 3
            && scope.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && scope.paired_class_tag.len() == 3
            && scope
                .paired_class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && !scope.kind.is_empty()
            && scope.frame_length > 89
            && scope.paired_byte_offset == scope.byte_offset.saturating_add(scope.frame_length)
            && scope.kind_offset > scope.byte_offset
            && scope.kind_offset < scope.paired_byte_offset.saturating_sub(78)
            && record_indices.contains(&scope.record_index)
            && entity_link.unwrap_or(scope.kind != "Sketch")
            && unique_index;
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design parameter scope has an invalid paired frame".into(),
                entity: Some(scope.id.clone()),
            });
        }
    }
    let mut owner_indices = HashSet::new();
    let mut owner_ordinals = HashSet::new();
    let mut owner_local_ordinals = HashSet::new();
    for owner in &native.design_parameter_owners {
        let unique_index = owner_indices.insert(owner.record_index);
        let unique_ordinal = owner_ordinals.insert(owner.owned_ordinal);
        let unique_local_ordinal =
            owner_local_ordinals.insert((owner.scope_record_index, owner.local_ordinal));
        let parameter = parameters_by_index.get(&owner.parameter_record_index);
        let valid = owner.class_tag.len() == 3
            && owner.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && owner.variant <= 1
            && owner.evaluated_value.is_finite()
            && owner.evaluated_value_offset == owner.byte_offset + 40
            && owner.parameter_record_index == owner.record_index.saturating_add(1)
            && owner.companion_record_index == owner.record_index.saturating_add(2)
            && scopes_by_index.contains_key(&owner.scope_record_index)
            && record_indices.contains(&owner.parameter_record_index)
            && record_indices.contains(&owner.companion_record_index)
            && parameter.is_some_and(|parameter| {
                parameter.owner_record_index == Some(owner.record_index)
                    && parameter.evaluated_value.to_bits() == owner.evaluated_value.to_bits()
            })
            && unique_index
            && unique_ordinal
            && unique_local_ordinal;
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design parameter owner has an invalid frame or indexed link"
                    .into(),
                entity: Some(owner.id.clone()),
            });
        }
    }
    for parameter in &native.design_parameters {
        let unique_index = parameter_indices.insert(parameter.record_index);
        let unique_ordinal = parameter_ordinals.insert(parameter.source_ordinal);
        let expected_kind = if parameter.source_kind == "User Parameter" {
            records::DesignParameterKind::User
        } else if parameter.source_kind.contains("Dimension") {
            records::DesignParameterKind::Dimension
        } else {
            records::DesignParameterKind::Feature
        };
        let owner_shape_valid = match parameter.kind {
            records::DesignParameterKind::User => parameter.owner_record_index.is_none(),
            records::DesignParameterKind::Dimension | records::DesignParameterKind::Feature => {
                parameter
                    .owner_record_index
                    .is_some_and(|owner| owners_by_index.contains_key(&owner))
            }
        };
        let offsets_ordered = parameter.byte_offset < parameter.expression_offset
            && parameter.expression_offset < parameter.source_kind_offset
            && parameter.source_kind_offset
                < parameter.unit_offset.unwrap_or(parameter.name_offset)
            && parameter
                .unit_offset
                .is_none_or(|offset| offset < parameter.name_offset)
            && parameter.name_offset < parameter.evaluated_value_offset;
        let valid = parameter.class_tag.len() == 3
            && parameter
                .class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && !parameter.expression.is_empty()
            && !parameter.source_kind.is_empty()
            && !parameter.name.is_empty()
            && parameter.unit.as_ref().is_none_or(|unit| !unit.is_empty())
            && parameter.unit.is_some() == parameter.unit_offset.is_some()
            && parameter.evaluated_value.is_finite()
            && parameter.kind == expected_kind
            && owner_shape_valid
            && offsets_ordered
            && unique_index
            && unique_ordinal;
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design parameter has an invalid frame, family, or owner".into(),
                entity: Some(parameter.id.clone()),
            });
        }
    }
    for header in &native.design_entity_headers {
        let count_matches = header
            .declared_reference_count
            .is_none_or(|count| count as usize == header.reference_indices.len());
        let references_resolve = header
            .reference_indices
            .iter()
            .all(|index| record_indices.contains(index));
        if !count_matches || !references_resolve {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: "Fusion design entity has an invalid reference run".into(),
                entity: Some(header.entity_id.clone()),
            });
        }
    }
    let sketch_owners = native
        .design_entity_headers
        .iter()
        .filter(|header| header.object_kind == Some(records::DesignObjectKind::Sketch))
        .map(|header| header.entity_suffix as u32)
        .collect::<HashSet<_>>();
    let sketch_owner_ids = native
        .design_entity_headers
        .iter()
        .filter(|header| header.object_kind == Some(records::DesignObjectKind::Sketch))
        .map(|header| (header.entity_suffix as u32, header.entity_id.as_str()))
        .collect::<std::collections::HashMap<_, _>>();
    for relation in &native.sketch_relations {
        let (constraint_kinds, unknown_constraint_bits) =
            design::decode_constraint_kinds(relation.state);
        let offsets_fit = relation
            .member_offsets
            .iter()
            .chain(&relation.auxiliary_reference_offsets)
            .chain(std::iter::once(&relation.owner_reference_offset))
            .chain(&relation.return_member_offsets)
            .all(|offset| {
                usize::try_from(*offset)
                    .ok()
                    .and_then(|offset| offset.checked_add(4))
                    .is_some_and(|end| end <= relation.raw_bytes.len())
            });
        let valid = sketch_owners.contains(&relation.owner_reference)
            && sketch_owner_ids.get(&relation.owner_reference).copied()
                == Some(relation.owner_entity_id.as_str())
            && relation.raw_bytes.len() >= 24
            && relation.members.len() == relation.member_offsets.len()
            && relation.auxiliary_references.len() == relation.auxiliary_reference_offsets.len()
            && relation.return_members.len() == relation.return_member_offsets.len()
            && offsets_fit
            && relation.unknown_constraint_bits == unknown_constraint_bits
            && relation.constraint_kinds == constraint_kinds;
        if !valid {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: "Fusion sketch relation has an invalid owner or byte frame".into(),
                entity: Some(relation.id.clone()),
            });
        }
    }
    for point in &native.sketch_points {
        if !point.coordinates.u.is_finite() || !point.coordinates.v.is_finite() {
            findings.push(Finding {
                check: Check::Bounds,
                severity: Severity::Error,
                message: "Fusion sketch point contains a non-finite coordinate".into(),
                entity: Some(point.id.clone()),
            });
        }
    }
    let typed_sketch_records = native
        .sketch_points
        .iter()
        .map(|point| point.record_index)
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|curve| curve.record_index),
        )
        .collect::<std::collections::HashSet<_>>();
    let sketch_operands = native
        .sketch_points
        .iter()
        .map(|point| {
            (
                point.record_index,
                records::SketchRelationOperand::Point {
                    record_index: point.record_index,
                    persistent_id: point.persistent_id,
                },
            )
        })
        .chain(native.sketch_curve_identities.iter().map(|curve| {
            (
                curve.record_index,
                records::SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id,
                    secondary_id: curve.secondary_id,
                },
            )
        }))
        .collect::<std::collections::HashMap<_, _>>();
    let mut relation_owners = std::collections::HashMap::new();
    for relation in &native.sketch_relations {
        let resolve = |indices: &[u32]| {
            indices
                .iter()
                .map(|record_index| {
                    sketch_operands.get(record_index).cloned().unwrap_or(
                        records::SketchRelationOperand::Record {
                            record_index: *record_index,
                        },
                    )
                })
                .collect::<Vec<_>>()
        };
        if relation.resolved_members != resolve(&relation.members)
            || relation.resolved_return_members != resolve(&relation.return_members)
        {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message:
                    "Fusion sketch relation typed operands disagree with its indexed references"
                        .into(),
                entity: Some(relation.id.clone()),
            });
        }
        for member in relation.members.iter().chain(&relation.return_members) {
            if !typed_sketch_records.contains(member) {
                continue;
            }
            if relation_owners
                .insert(*member, relation.owner_reference)
                .is_some_and(|owner| owner != relation.owner_reference)
            {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "Fusion sketch member belongs to multiple sketch owners".into(),
                    entity: Some(relation.id.clone()),
                });
            }
        }
    }
    for (id, record_index, owner_reference) in native
        .sketch_points
        .iter()
        .map(|point| (&point.id, point.record_index, point.owner_reference))
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|curve| (&curve.id, curve.record_index, curve.owner_reference)),
        )
    {
        if relation_owners.get(&record_index).copied() != owner_reference {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion sketch geometry owner disagrees with its relation graph".into(),
                entity: Some(id.clone()),
            });
        }
    }
    let body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<HashSet<_>>();
    let face_ids = ir
        .model
        .faces
        .iter()
        .map(|face| &face.id)
        .collect::<HashSet<_>>();
    let edge_ids = ir
        .model
        .edges
        .iter()
        .map(|edge| &edge.id)
        .collect::<HashSet<_>>();
    let mut body_links = std::collections::BTreeMap::new();
    for link in &native.persistent_design_links {
        let target_key = match &link.target {
            cadmpeg_ir::attributes::AttributeTarget::Body(id) if body_ids.contains(id) => {
                Some(id.0.clone())
            }
            _ => None,
        };
        let valid = target_key.is_some()
            && link.entity_kind == 3
            && !link.design_id.is_empty()
            && link.design_id.bytes().all(|byte| byte.is_ascii_digit());
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion persistent body link has an invalid target or group payload"
                    .into(),
                entity: Some(link.id.clone()),
            });
            continue;
        }
        body_links
            .entry(target_key.expect("validated body target"))
            .or_insert_with(Vec::new)
            .push(link);
    }
    for links in body_links.values_mut() {
        links.sort_by_key(|link| link.ordinal);
        if links.iter().enumerate().any(|(ordinal, link)| {
            link.ordinal != ordinal as u32 || link.is_current != (ordinal + 1 == links.len())
        }) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion persistent body links have noncanonical history ordering".into(),
                entity: links.first().map(|link| link.id.clone()),
            });
        }
    }
    let mut subentity_tags = std::collections::BTreeMap::new();
    for tag in &native.persistent_subentity_tags {
        let target_key = match &tag.target {
            cadmpeg_ir::attributes::AttributeTarget::Face(id) if face_ids.contains(id) => {
                Some(format!("face:{}", id.0))
            }
            cadmpeg_ir::attributes::AttributeTarget::Edge(id) if edge_ids.contains(id) => {
                Some(format!("edge:{}", id.0))
            }
            _ => None,
        };
        if target_key.is_none() || tag.token.is_empty() || tag.design_references.is_empty() {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion persistent subentity tag has an invalid target or group payload"
                    .into(),
                entity: Some(tag.id.clone()),
            });
            continue;
        }
        subentity_tags
            .entry(target_key.expect("validated subentity target"))
            .or_insert_with(Vec::new)
            .push(tag);
    }
    for tags in subentity_tags.values_mut() {
        tags.sort_by_key(|tag| tag.ordinal);
        if tags
            .iter()
            .enumerate()
            .any(|(ordinal, tag)| tag.ordinal != ordinal as u32)
        {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion persistent subentity tags have noncanonical group ordering".into(),
                entity: tags.first().map(|tag| tag.id.clone()),
            });
        }
    }
    for history in &native.asm_histories {
        if !history::graph_is_coherent(history) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion ASM history graph is not a coherent doubly linked state chain"
                    .into(),
                entity: Some(history.id.clone()),
            });
        }
    }
    findings
}

impl F3dCodec {
    /// Write a decoded F3D document, replaying its source bytes when its
    /// semantic content is unchanged.
    ///
    /// Supported edits regenerate affected records within the retained archive.
    /// The method returns [`CodecError::NotImplemented`] when `ir` has no F3D
    /// semantic baseline or retained source image.
    pub fn write_preserved(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<(), CodecError> {
        let expected = ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("semantic_sha256"))
            .ok_or_else(|| CodecError::NotImplemented("IR has no F3D semantic baseline".into()))?;
        let unknowns = ir.native_unknowns("f3d")?;
        let record = unknowns
            .iter()
            .find(|record| record.id.0 == "f3d:file:source-image#0")
            .ok_or_else(|| {
                CodecError::NotImplemented("IR has no retained F3D source image".into())
            })?;
        let data = record.data.as_ref().ok_or_else(|| {
            CodecError::Malformed("retained F3D source image has no bytes".into())
        })?;
        let hash = sha256_hex(data);
        if data.len() as u64 != record.byte_len || hash != record.sha256 {
            return Err(CodecError::Malformed(
                "retained F3D source image failed integrity validation".into(),
            ));
        }
        if decode::semantic_hash(ir) != *expected {
            return writer::write_semantic(ir, data, writer);
        }
        writer.write_all(data)?;
        Ok(())
    }
}

impl Codec for F3dCodec {
    fn id(&self) -> &'static str {
        "f3d"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if !prefix.starts_with(ZIP_MAGIC) {
            return Confidence::No;
        }
        // A ZIP alone is a weak signal (many formats are ZIPs). An f3d marker
        // string in the prefix — entry names are stored in cleartext in ZIP
        // local headers — makes it conclusive.
        if container::DETECT_MARKERS
            .iter()
            .any(|m| contains_subslice(prefix, m))
        {
            Confidence::High
        } else {
            Confidence::Low
        }
    }

    fn inspect(&self, reader: &mut dyn ReadSeek) -> Result<ContainerSummary, CodecError> {
        let scan = container::scan(reader)?;
        Ok(container::summarize(&scan))
    }

    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError> {
        decode::decode(reader, options)
    }
}

impl Encoder for F3dCodec {
    fn id(&self) -> &'static str {
        "f3d"
    }

    fn encode(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<ExportReport, CodecError> {
        let replay = ir
            .native_unknowns("f3d")?
            .into_iter()
            .any(|record| record.id.0 == "f3d:file:source-image#0");
        if replay {
            self.write_preserved(ir, writer)?;
        } else {
            writer::write_new(ir, writer)?;
        }
        let validation = cadmpeg_ir::validate(ir, Vec::new());
        let total_entities = validation.entity_counts.values().sum();
        Ok(ExportReport {
            format: "f3d".into(),
            entity_counts: validation.entity_counts,
            total_entities,
            losses: Vec::new(),
            notes: vec![
                if replay {
                    "preserved source container replayed verbatim"
                } else {
                    "source container regenerated from IR"
                }
                .into(),
                "entity counts are derived from the IR".into(),
            ],
        })
    }
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests;
