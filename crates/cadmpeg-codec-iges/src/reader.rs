// SPDX-License-Identifier: Apache-2.0
//! Physical graph to CADIR native preservation and loss reporting.

use crate::{byte_ledger, card, directory, entities, global, graph, native, parameter};
use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult, ReadSeek};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::{CadIr, SourceFidelity, SourceMeta};
use std::collections::{BTreeMap, BTreeSet};

fn source_meta(global: &global::Global) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert("representation".into(), "fixed-ascii".into());
    attributes.insert(
        "parameter_delimiter".into(),
        char::from(global.parameter_delimiter).to_string(),
    );
    attributes.insert(
        "record_delimiter".into(),
        char::from(global.record_delimiter).to_string(),
    );
    if let Some(value) = global.version() {
        attributes.insert("iges_version".into(), value.into());
    }
    if let Some(value) = global.version_flag() {
        attributes.insert("iges_version_flag".into(), value.to_string());
    }
    if let Some(value) = global.units_name() {
        attributes.insert("native_units".into(), value);
    }
    if let Some(value) = global.sender_product() {
        attributes.insert("sender_product".into(), value);
    }
    if let Some(value) = global.native_file_name() {
        attributes.insert("native_file_name".into(), value);
    }
    SourceMeta {
        format: "iges".into(),
        attributes,
    }
}

pub(crate) fn decode(
    reader: &mut dyn ReadSeek,
    options: DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = card::scan(reader)?;
    let global = global::parse(&scan)?;
    if global.version() != Some("5.3") {
        return Err(CodecError::NotImplemented(format!(
            "IGES Fixed ASCII version {} decode; target envelope is 5.3",
            global.version().unwrap_or("unrecognized")
        )));
    }
    let directory = directory::parse(&scan)?;
    let parameters = parameter::assemble(&scan, &directory, &global)?;
    let references = graph::build(&directory);
    let byte_ledger = byte_ledger::build(&scan, &global, &parameters);
    let mut source_fidelity = SourceFidelity {
        byte_ledger: byte_ledger.clone(),
        ..SourceFidelity::default()
    };

    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(source_meta(&global));
    let projection = if options.container_only {
        entities::geometry::Projection {
            handled: BTreeSet::default(),
            decoded: BTreeSet::default(),
            losses: Vec::new(),
        }
    } else {
        entities::geometry::project_geometry(&mut ir, &directory, &parameters, &global)
    };
    let product_occurrences_truncated = native::store(
        &mut ir,
        &scan,
        &directory,
        &parameters,
        &references,
        &global,
        &mut source_fidelity,
    )?;
    source_fidelity.finalize();

    let geometry_transferred = !projection.decoded.is_empty();
    let mut losses = projection.losses;
    if product_occurrences_truncated {
        losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: "IGES product occurrence expansion reached its configured output limit".into(),
            provenance: None,
        });
    }
    if !options.container_only {
        losses.extend(
            directory
                .iter()
                .filter(|entry| {
                    entry.entity_type != 0
                        && (!crate::profile::envelope_a_admits(entry.entity_type, entry.form)
                            || !projection.handled.contains(&entry.sequence))
                })
                .map(|entry| LossNote {
                    category: LossCategory::Other,
                    severity: Severity::Warning,
                    message: if crate::profile::envelope_a_admits(entry.entity_type, entry.form) {
                        format!(
                            "IGES entity type {} form {} retained without neutral projection",
                            entry.entity_type, entry.form
                        )
                    } else {
                        format!(
                            "IGES entity type {} form {} is outside the Fixed ASCII mechanical/document envelope",
                            entry.entity_type, entry.form
                        )
                    },
                    provenance: None,
                }),
        );
    }
    let mut notes = directory::summary_notes(&directory);
    notes.extend(parameter::summary_notes(&parameters));
    notes.extend(graph::summary_notes(&references));
    Ok(DecodeResult::with_source_fidelity(
        ir,
        DecodeReport {
            format: "iges".into(),
            container_only: options.container_only,
            geometry_transferred,
            losses,
            notes,
        },
        source_fidelity,
    ))
}
