// SPDX-License-Identifier: Apache-2.0
//! Read and write `SolidWorks` `.sldprt` part documents.
//!
//! [`SldprtCodec`] decodes B-rep topology, analytic and NURBS geometry,
//! tessellation, appearances, selected document attributes, feature history,
//! and feature-input records into [`cadmpeg_ir::CadIr`]. It preserves source
//! blocks and records provenance so supported edits can retain native data.
//!
//! Support level: [L4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder)
//! on the cadmpeg support ladder.
//!
//! # Decode
//!
//! ```
//! use std::io::Cursor;
//!
//! use cadmpeg_codec_sldprt::SldprtCodec;
//! use cadmpeg_ir::{Codec, DecodeOptions};
//!
//! # fn decode(bytes: Vec<u8>) -> Result<(), cadmpeg_ir::CodecError> {
//! let decoded = SldprtCodec.decode(
//!     &mut Cursor::new(bytes),
//!     &DecodeOptions::default(),
//! )?;
//! println!("{} faces", decoded.ir.model.faces.len());
//! for loss in &decoded.report.losses {
//!     eprintln!("{:?}: {}", loss.severity, loss.message);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Decode reports can accompany a usable model. Untyped support carriers become
//! opaque geometry linked to retained bytes, while their resolvable topology
//! remains in the IR. Failure to build a Parasolid graph yields a metadata-only
//! IR with blocking diagnostics. Set [`DecodeOptions::container_only`] to request
//! that result without attempting geometry.
//!
//! [`Codec::inspect`] inventories the outer blocks, section directory, cache
//! cells, payload families, and Parasolid schemas. It does not build model
//! geometry.
//!
//! # Format and units
//!
//! The outer container uses an 8-byte header, CRC-validated raw-DEFLATE blocks,
//! a fixed-cell section index, and a tail directory. Embedded Parasolid
//! `partition` and `deltas` streams supply the B-rep record graph. Parasolid
//! lengths are metres; decoded `CadIr` coordinates are millimetres. Directions,
//! normals, and ratios remain dimensionless.
//!
//! # Encode
//!
//! [`SldprtCodec`] implements [`Encoder`]. Unchanged decoded IR replays its
//! retained source image byte for byte. Supported geometry edits can patch the
//! native partition when the entity graph and provenance remain stable.
//! Otherwise the writer regenerates supported semantic records and returns
//! [`CodecError::NotImplemented`] for an unsupported IR shape.
//!
//! The semantic writer supports solid bodies with multiple regions and shells,
//! sheet bodies with one shell per region, analytic and non-periodic NURBS carriers, selected
//! metadata and feature records, base colors, and sequential triangle-strip
//! tessellation. It bakes right-handed rigid body transforms into geometry.
//!
//! [`Codec::inspect`]: cadmpeg_ir::Codec::inspect
//! [`CodecError::NotImplemented`]: cadmpeg_ir::CodecError::NotImplemented
//! [`DecodeOptions::container_only`]: cadmpeg_ir::DecodeOptions::container_only

mod annotations;
mod appearance;
pub mod brep;
mod classification;
pub mod container;
pub mod decode;
mod history;
mod metadata;
mod native;
pub mod parasolid;
mod pmi;
pub mod records;
mod resolved_features;
mod tessellation;
mod writer;
mod writer_patch;
mod writer_transform;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult, Encoder, ReadSeek,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{Annotations, Check, Finding, Severity, SourceFidelity};
use std::io::Write;

/// Codec for `SolidWorks` `.sldprt` part documents.
#[derive(Debug, Default, Clone, Copy)]
pub struct SldprtCodec;

/// Validate `SolidWorks` native feature-input byte references.
pub fn validate_native(ir: &CadIr) -> Vec<Finding> {
    const MARKER: &[u8] = &[0xff, 0xff, 0x1f, 0x00, 0x03];

    let Some(namespace) = ir.native.namespace("sldprt") else {
        return Vec::new();
    };
    if namespace.version != native::SLDPRT_NATIVE_VERSION {
        let version = namespace.version;
        return vec![Finding {
            check: Check::Version,
            severity: Severity::Error,
            message: format!("unsupported SolidWorks native namespace version {version}"),
            entity: None,
        }];
    }
    let native = match native::SldprtNative::load(namespace) {
        Ok(native) => native,
        Err(error) => {
            return vec![Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: format!("invalid SolidWorks native namespace: {error}"),
                entity: None,
            }]
        }
    };
    let mut findings = Vec::new();
    for history in &native.feature_histories {
        if let Err(error) = crate::writer::validate_feature_graph(&history.features) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: error.to_string(),
                entity: Some(history.id.clone()),
            });
        }
        let mut feature_ordinals = std::collections::HashSet::new();
        for feature in &history.features {
            if !feature_ordinals.insert(feature.ordinal) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!(
                        "SolidWorks history repeats feature ordinal {}",
                        feature.ordinal
                    ),
                    entity: Some(feature.id.clone()),
                });
            }
        }
        let mut configuration_ordinals = std::collections::HashSet::new();
        for configuration in &history.configurations {
            if !configuration_ordinals.insert(configuration.ordinal) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!(
                        "SolidWorks history repeats configuration ordinal {}",
                        configuration.ordinal
                    ),
                    entity: Some(configuration.id.clone()),
                });
            }
        }
        if !history.content.is_empty() {
            let configurations = history
                .configurations
                .iter()
                .map(|configuration| configuration.id.as_str())
                .collect::<std::collections::HashSet<_>>();
            let root_features = history
                .features
                .iter()
                .filter(|feature| {
                    feature.tree_parent.is_none() && feature.parent_source_id.is_none()
                })
                .map(|feature| feature.id.as_str())
                .collect::<std::collections::HashSet<_>>();
            let all_features = history
                .features
                .iter()
                .map(|feature| feature.id.as_str())
                .collect::<std::collections::HashSet<_>>();
            let mut seen_configurations = std::collections::HashSet::new();
            let mut seen_features = std::collections::HashSet::new();
            for item in &history.content {
                let error = match item {
                    crate::records::HistoryContent::Configuration(id) => {
                        if !configurations.contains(id.as_str()) {
                            Some(format!(
                                "SolidWorks history root references missing configuration {id}"
                            ))
                        } else if !seen_configurations.insert(id.as_str()) {
                            Some(format!(
                                "SolidWorks history root repeats configuration {id}"
                            ))
                        } else {
                            None
                        }
                    }
                    crate::records::HistoryContent::Feature(id) => {
                        if !all_features.contains(id.as_str()) {
                            Some(format!(
                                "SolidWorks history root references missing feature {id}"
                            ))
                        } else if !root_features.contains(id.as_str()) {
                            Some(format!(
                                "SolidWorks history root references nested feature {id}"
                            ))
                        } else if !seen_features.insert(id.as_str()) {
                            Some(format!("SolidWorks history root repeats feature {id}"))
                        } else {
                            None
                        }
                    }
                    crate::records::HistoryContent::Text(_) => None,
                };
                if let Some(message) = error {
                    findings.push(Finding {
                        check: Check::NativeLinks,
                        severity: Severity::Error,
                        message,
                        entity: Some(history.id.clone()),
                    });
                }
            }
            for missing in configurations.difference(&seen_configurations) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!("SolidWorks history root omits configuration {missing}"),
                    entity: Some(history.id.clone()),
                });
            }
            for missing in root_features.difference(&seen_features) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!("SolidWorks history root omits feature {missing}"),
                    entity: Some(history.id.clone()),
                });
            }
        }
    }
    for lane in &native.feature_input_lanes {
        let expected_classes =
            crate::resolved_features::class_declarations(&lane.native_payload, &lane.id);
        if lane.classes != expected_classes {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "SolidWorks feature-input class index does not match its native payload"
                    .into(),
                entity: Some(lane.id.clone()),
            });
        }
        let expected_names = crate::resolved_features::object_names(&lane.native_payload, &lane.id);
        if lane.names.len() != expected_names.len()
            || lane
                .names
                .iter()
                .zip(&expected_names)
                .any(|(actual, expected)| {
                    actual.id != expected.id
                        || actual.parent != expected.parent
                        || actual.ordinal != expected.ordinal
                        || actual.offset != expected.offset
                })
        {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message:
                    "SolidWorks feature-input name structure does not match its native payload"
                        .into(),
                entity: Some(lane.id.clone()),
            });
        }
        let mut expected_lane = lane.clone();
        expected_lane.scalars =
            crate::resolved_features::named_scalars(&lane.native_payload, &lane.id, &lane.names);
        expected_lane.relation_bindings = crate::resolved_features::relation_bindings(
            &lane.id,
            &lane.classes,
            &expected_lane.scalars,
        );
        expected_lane.references =
            crate::resolved_features::reference_cells(&expected_lane.scalars);
        crate::resolved_features::bind_scalar_operands(
            &native.feature_histories,
            std::slice::from_mut(&mut expected_lane),
        );
        if !crate::resolved_features::scalar_indices_match(&lane.scalars, &expected_lane.scalars) {
            let detail = lane
                .scalars
                .iter()
                .zip(&expected_lane.scalars)
                .find(|(actual, expected)| {
                    !crate::resolved_features::scalar_indices_match(
                        std::slice::from_ref(actual),
                        std::slice::from_ref(expected),
                    )
                })
                .map_or_else(
                    || {
                        format!(
                            "count {} != {}",
                            lane.scalars.len(),
                            expected_lane.scalars.len()
                        )
                    },
                    |(actual, expected)| format!("{actual:?} != {expected:?}"),
                );
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: format!(
                    "SolidWorks feature-input scalar index does not match its native payload: {detail}"
                ),
                entity: Some(lane.id.clone()),
            });
        }
        if lane.relation_bindings != expected_lane.relation_bindings {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message:
                    "SolidWorks feature-input relation bindings do not match the native payload"
                        .into(),
                entity: Some(lane.id.clone()),
            });
        }
        if lane.relation_instances != expected_lane.relation_instances {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message:
                    "SolidWorks feature-input relation instances do not match the native payload"
                        .into(),
                entity: Some(lane.id.clone()),
            });
        }
        if lane.references != expected_lane.references {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message:
                    "SolidWorks feature-input reference index does not match its native payload"
                        .into(),
                entity: Some(lane.id.clone()),
            });
        }
        let expected_offsets = lane
            .native_payload
            .windows(MARKER.len())
            .enumerate()
            .filter_map(|(offset, bytes)| (bytes == MARKER).then_some(offset as u64))
            .collect::<std::collections::HashSet<_>>();
        let mut ordinals = std::collections::HashSet::new();
        let mut offsets = std::collections::HashSet::new();
        let mut previous_offset = None;
        for (index, entity) in lane.sketch_entities.iter().enumerate() {
            if entity.ordinal != index as u32 {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!(
                        "SolidWorks feature-input lane expects entity ordinal {index}, found {}",
                        entity.ordinal
                    ),
                    entity: Some(entity.id.clone()),
                });
            }
            if previous_offset.is_some_and(|offset| entity.offset <= offset) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "SolidWorks feature-input entities are not in stream order".into(),
                    entity: Some(entity.id.clone()),
                });
            }
            previous_offset = Some(entity.offset);
            if !ordinals.insert(entity.ordinal) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!(
                        "SolidWorks feature-input lane repeats entity ordinal {}",
                        entity.ordinal
                    ),
                    entity: Some(entity.id.clone()),
                });
            }
            if !offsets.insert(entity.offset) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!(
                        "SolidWorks feature-input lane repeats entity offset {}",
                        entity.offset
                    ),
                    entity: Some(entity.id.clone()),
                });
            }
            let valid = usize::try_from(entity.offset).ok().is_some_and(|offset| {
                offset
                    .checked_add(MARKER.len())
                    .and_then(|end| lane.native_payload.get(offset..end))
                    == Some(MARKER)
                    && offset
                        .checked_add(21)
                        .is_some_and(|end| end <= lane.native_payload.len())
            });
            if !valid {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "feature-input entity is outside its native payload".into(),
                    entity: Some(lane.id.clone()),
                });
            }
            if usize::try_from(entity.offset).ok().is_some_and(|offset| {
                entity.local_id
                    != crate::resolved_features::marker_local_id(&lane.native_payload, offset)
            }) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message:
                        "SolidWorks feature-input local object id does not match its native payload"
                            .into(),
                    entity: Some(entity.id.clone()),
                });
            }
        }
        for offset in expected_offsets.difference(&offsets) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: format!("SolidWorks feature-input lane omits marker at offset {offset}"),
                entity: Some(lane.id.clone()),
            });
        }
    }
    findings
}

impl SldprtCodec {
    /// Write a decoded document with its retained source-fidelity sidecar.
    pub fn write_preserved_with_source_fidelity(
        &self,
        ir: &CadIr,
        source_fidelity: &SourceFidelity,
        writer: &mut dyn Write,
    ) -> Result<(), CodecError> {
        let records = source_records(ir, source_fidelity)?;
        Self::write_preserved_with_annotations(ir, &source_fidelity.annotations, &records, writer)
    }

    fn write_preserved_with_annotations(
        ir: &CadIr,
        annotations: &Annotations,
        records: &[UnknownRecord],
        writer: &mut dyn Write,
    ) -> Result<(), CodecError> {
        let expected = ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("semantic_sha256"));
        if expected.is_none_or(|expected| decode::semantic_hash(ir) != *expected) {
            return writer::write_semantic_with_records(ir, annotations, records, writer);
        }
        let record = records
            .iter()
            .find(|record| record.id.0 == "sldprt:file:source-image#0")
            .ok_or_else(|| {
                CodecError::NotImplemented("IR has no retained SLDPRT source image".into())
            })?;
        let data = record.data.as_ref().ok_or_else(|| {
            CodecError::Malformed("retained SLDPRT source image has no bytes".into())
        })?;
        let hash = sha256_hex(data);
        if data.len() as u64 != record.byte_len || hash != record.sha256 {
            return Err(CodecError::Malformed(
                "retained SLDPRT source image failed integrity validation".into(),
            ));
        }
        writer.write_all(data)?;
        Ok(())
    }
}

impl Codec for SldprtCodec {
    fn id(&self) -> &'static str {
        "sldprt"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if container::looks_like_sldprt(prefix) {
            Confidence::High
        } else {
            Confidence::No
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

impl Encoder for SldprtCodec {
    fn id(&self) -> &'static str {
        "sldprt"
    }

    fn encode(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<ExportReport, CodecError> {
        Self::encode_with_annotations(ir, &Annotations::default(), &[], writer)
    }

    fn encode_with_source_fidelity(
        &self,
        ir: &CadIr,
        source_fidelity: Option<&SourceFidelity>,
        writer: &mut dyn Write,
    ) -> Result<ExportReport, CodecError> {
        match source_fidelity {
            Some(value) => Self::encode_with_fidelity(ir, value, writer),
            None => Self::encode_with_annotations(ir, &Annotations::default(), &[], writer),
        }
    }
}

impl SldprtCodec {
    fn encode_with_fidelity(
        ir: &CadIr,
        source_fidelity: &SourceFidelity,
        writer: &mut dyn Write,
    ) -> Result<ExportReport, CodecError> {
        let records = source_records(ir, source_fidelity)?;
        Self::encode_with_annotations(ir, &source_fidelity.annotations, &records, writer)
    }

    fn encode_with_annotations(
        ir: &CadIr,
        annotations: &Annotations,
        records: &[UnknownRecord],
        writer: &mut dyn Write,
    ) -> Result<ExportReport, CodecError> {
        let replay = records
            .iter()
            .any(|record| record.id.0 == "sldprt:file:source-image#0");
        Self::write_preserved_with_annotations(ir, annotations, records, writer)?;
        let validation = cadmpeg_ir::validate(ir, Vec::new());
        let total_entities = validation.entity_counts.values().sum();
        Ok(ExportReport {
            format: "sldprt".into(),
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

fn source_records(
    ir: &CadIr,
    source_fidelity: &SourceFidelity,
) -> Result<Vec<UnknownRecord>, CodecError> {
    let mut records = source_fidelity.native_unknown_records(ir, "sldprt")?;
    if let Some(source) = source_fidelity.retained_record("sldprt:file:source-image#0") {
        records.push(UnknownRecord {
            id: source.id.clone().into(),
            offset: source.offset,
            byte_len: source.byte_len,
            sha256: source.sha256.clone(),
            data: source.data.clone(),
            links: Vec::new(),
        });
    }
    Ok(records)
}

#[cfg(test)]
mod tests;
