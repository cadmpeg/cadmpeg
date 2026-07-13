// SPDX-License-Identifier: Apache-2.0
//! Read ZIP-packaged `FreeCAD` `.FCStd` documents.

mod container;
mod native;

use std::collections::BTreeMap;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult, ReadSeek,
};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::Units;

/// Input-only `FCStd` codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct FcstdCodec;

impl Codec for FcstdCodec {
    fn id(&self) -> &'static str {
        "fcstd"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if !prefix.starts_with(b"PK\x03\x04") {
            return Confidence::No;
        }
        if contains(prefix, b"Document.xml") {
            Confidence::High
        } else {
            Confidence::Low
        }
    }

    fn inspect(&self, reader: &mut dyn ReadSeek) -> Result<ContainerSummary, CodecError> {
        container::scan(reader).map(|scan| container::summarize(&scan))
    }

    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError> {
        let scan = container::scan(reader)?;
        let mut attributes = BTreeMap::new();
        attributes.insert(
            "schema_version".into(),
            scan.document.schema_version.clone(),
        );
        attributes.insert("file_version".into(), scan.document.file_version.clone());
        attributes.insert("document_root".into(), scan.document.root_name.clone());
        attributes.insert(
            "object_count".into(),
            scan.document.object_count.to_string(),
        );
        attributes.insert("archive_entry_count".into(), scan.entries.len().to_string());
        attributes.insert(
            "physical_ledger_spans".into(),
            scan.ledger.len().to_string(),
        );
        if let Some(value) = &scan.document.program_version {
            attributes.insert("program_version".into(), value.clone());
        }
        if let Some(thumbnail) = scan
            .data
            .get("thumbnails/Thumbnail.png")
            .or_else(|| scan.data.get("Thumbnail.png"))
        {
            attributes.insert("thumbnail_bytes".into(), thumbnail.len().to_string());
        }
        let mut ir = CadIr::empty(Units::default());
        ir.source = Some(SourceMeta {
            format: "fcstd".into(),
            attributes,
        });
        let namespace = ir.native.namespace_mut("fcstd");
        namespace.version = native::VERSION;
        namespace.set_arena("document", std::slice::from_ref(&scan.document))?;
        namespace.set_arena("physical_ledger", &scan.ledger)?;
        let losses = if options.container_only {
            Vec::new()
        } else {
            vec![LossNote {
                category: LossCategory::Geometry,
                severity: Severity::Blocking,
                message: "FCStd persistence and exact-shape decoding are not implemented yet"
                    .into(),
                provenance: None,
            }]
        };
        Ok(DecodeResult::new(
            ir,
            DecodeReport {
                format: "fcstd".into(),
                container_only: options.container_only,
                geometry_transferred: false,
                losses,
                notes: container::summarize(&scan).notes,
            },
        ))
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests;
