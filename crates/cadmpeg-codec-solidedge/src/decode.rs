// SPDX-License-Identifier: Apache-2.0
//! Solid Edge document decode.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::SourceMeta;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition};
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::{CadIr, SourceFidelity};

use crate::{brep, container, dialect, loss::SolidEdgeLossCode, partition};
use cadmpeg_container::compound::CompoundEntry;
use cadmpeg_parasolid::records::PrimaryRecords;

pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
    let snapshot = container::open(ctx, root)?;
    let mut ir = CadIr::empty();
    ir.source = Some(SourceMeta::classified(
        dialect::layers(),
        std::collections::BTreeMap::default(),
    ));
    let mut body = DecodeBody::new(DecodeTransfer::at(ctx.depth(), false));
    body.losses.extend(
        snapshot.deviations().map(|deviation| {
            SolidEdgeLossCode::ContainerNoncanonical.note(deviation.description())
        }),
    );
    if !ctx.container_only() {
        let mut found = false;
        let mut kernels = Vec::new();
        for (ordinal, entry) in snapshot.entries().iter().enumerate() {
            let CompoundEntry::Stream(entry) = entry else {
                continue;
            };
            if !partition::is_primary(entry.path()) {
                continue;
            }
            found = true;
            // The losses of a partition that does not decode leave no body
            // to describe, so they join the report only with the partition.
            let mut partition_losses = vec![SolidEdgeLossCode::PrimaryStateAssumed.note(
                "primary partition supplies the saved body state; delta selection is unresolved",
            )];
            let result = (|| {
                let source = snapshot.open(ctx, entry)?;
                let payload = partition::payload(source).map_err(CodecError::from)?;
                let records = PrimaryRecords::read(payload.window())?.ok_or_else(|| {
                    CodecError::Malformed("unsupported Parasolid partition header".into())
                })?;
                kernels.push((
                    partition::schema(payload).map_err(CodecError::from)?,
                    cadmpeg_parasolid::Carrier::new(entry.path().to_owned()),
                ));
                if !records.is_terminated() {
                    partition_losses.push(SolidEdgeLossCode::PartitionIncomplete.note(format!(
                        "partition node framing stopped at {}",
                        records.stop_offset().unwrap_or(0)
                    )));
                }
                let stream = u32::try_from(ordinal)
                    .map_err(|_| CodecError::Malformed("too many partition streams".into()))?;
                let mut candidate = brep::project(ctx, &records, stream, &mut partition_losses)?;
                // One partition is a fragment of the document: the document
                // states its root definition, not the partition.
                candidate.fragment = true;
                candidate.finalize();
                let report =
                    cadmpeg_ir::validate::validate_neutral_for_decode(ctx, &candidate, Vec::new())?;
                if !report.is_ok() {
                    return Err(CodecError::Malformed(
                        report
                            .findings
                            .iter()
                            .filter(|finding| {
                                finding.severity >= cadmpeg_ir::report::Severity::Error
                            })
                            .map(|finding| finding.message.as_str())
                            .collect::<Vec<_>>()
                            .join("; "),
                    ));
                }
                ctx.charge_entities(
                    cadmpeg_core::decode::u64_from_index(candidate.geometry.entity_count()),
                    "admit Solid Edge partition entities",
                )?;
                if !candidate.geometry.bodies.is_empty()
                    && (ir.geometry.bodies.is_empty()
                        || candidate.tolerances.linear > ir.tolerances.linear)
                {
                    ir.tolerances.linear = candidate.tolerances.linear;
                }
                ir.try_append(
                    candidate.geometry,
                    candidate.model,
                    candidate.native,
                    |_| Ok(Ok::<_, CodecError>(())),
                )??;
                Ok::<_, CodecError>(())
            })();
            match result {
                Ok(()) => body.losses.extend(partition_losses),
                Err(error) => {
                    if matches!(error, CodecError::ResourceLimit(_)) {
                        return Err(error);
                    }
                    body.losses.push(
                        SolidEdgeLossCode::PartitionUnreadable
                            .note(format!("partition could not be decoded: {error}")),
                    );
                }
            }
        }
        // The document is one product definition that owns every body.
        let mut root = ProductDefinition::root(root_definition(), ExternalDocument::default());
        root.bodies = ir
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect();
        ir.model.product_definitions.push(root);
        if !found {
            body.losses.push(
                SolidEdgeLossCode::BodiesPending.note("document has no primary partition stream"),
            );
        }
        let mut layers = dialect::layers();
        for message in cadmpeg_parasolid::push_extras(
            &mut layers,
            cadmpeg_parasolid::extra_layers(kernels, &[]),
        ) {
            body.losses
                .push(SolidEdgeLossCode::KernelUnverified.note(message));
        }
        for matched in layers.iter() {
            if let Some(message) = cadmpeg_parasolid::unverified_message(matched) {
                body.losses
                    .push(SolidEdgeLossCode::KernelUnverified.note(message));
            }
        }
        ir.source = Some(SourceMeta::classified(
            layers,
            std::collections::BTreeMap::default(),
        ));
        body.losses.push(
            SolidEdgeLossCode::AttributesPending
                .note("body names, colours, and visibility remain undecoded"),
        );
        body.losses.push(
            SolidEdgeLossCode::FeaturesPending
                .note("feature records and assembly placements remain undecoded"),
        );
    }
    body.transfer = DecodeTransfer::at(ctx.depth(), !ir.geometry.bodies.is_empty());
    Ok(Decoded {
        ir,
        body,
        source_fidelity: SourceFidelity::new(Sha256Digest::digest(root.window())),
    })
}

fn root_definition() -> ProductDefinitionId {
    ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("solidedge", "model", "product-definition"),
        cadmpeg_ir::identity_key!("root"),
    )
}
