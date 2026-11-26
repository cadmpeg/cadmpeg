// SPDX-License-Identifier: Apache-2.0
//! Solid Edge document decode.

use cadmpeg_core::decode::{ArenaClass, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::SourceMeta;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::products::ExternalDocument;
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::unknown::part::{Cause, DecodeParts, GivenUpRecord, Part};
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
    let mut fidelity = SourceFidelity::new(Sha256Digest::digest(root.window()));
    body.losses.extend(
        snapshot.deviations().map(|deviation| {
            SolidEdgeLossCode::ContainerNoncanonical.note(deviation.description())
        }),
    );
    if !ctx.container_only() {
        let mut found = false;
        let mut kernels = Vec::new();
        let mut given_up = Vec::new();
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
            let source = match ctx.run_part(|| snapshot.open(ctx, entry))? {
                Ok(source) => source,
                Err(error) => {
                    let stream = cadmpeg_ir::ids::Identity::compose(
                        &cadmpeg_ir::identity_namespace!("solidedge", "container", "stream"),
                        cadmpeg_ir::ids::IdentityKey::encode_key_text(entry.path()),
                    );
                    body.losses.push(
                        GivenUpRecord::new(
                            stream,
                            Cause::Unreadable.of(&error),
                            format_args!("partition stream does not open: {error}"),
                        )
                        .loss(),
                    );
                    continue;
                }
            };
            let mut cause = Cause::Unreadable;
            let result = ctx.run_part(|| {
                let payload = partition::payload(source).map_err(CodecError::from)?;
                let Some(records) = PrimaryRecords::read(payload.window())? else {
                    cause = Cause::Undecoded;
                    return Err(CodecError::Malformed(
                        "unsupported Parasolid partition header".into(),
                    ));
                };
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
                let stream = u32::try_from(ordinal).map_err(|_| {
                    cause = Cause::Undecoded;
                    CodecError::Malformed("too many partition streams".into())
                })?;
                let mut candidate = brep::project(ctx, &records, stream, &mut partition_losses)?;
                candidate.finalize();
                // A partition is admitted as a document of its own; the decoded
                // document states one root over the bodies of every partition.
                candidate.state_root_definition(
                    ctx,
                    root_definition_id(),
                    ExternalDocument::default(),
                )?;
                // Past the projection, the decoder cannot tell a source defect
                // from a form it does not decode.
                cause = Cause::Undecoded;
                let report =
                    cadmpeg_ir::validate::validate_neutral_for_decode(ctx, &candidate, Vec::new())?;
                candidate
                    .model
                    .product_definitions
                    .retain(|definition| !definition.is_root());
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
            })?;
            match result {
                Ok(()) => body.losses.extend(partition_losses),
                Err(error) => {
                    let part = Part::at(
                        "solidedge",
                        "partition",
                        0,
                        source.window(),
                        [ArenaClass::Model],
                    )
                    .in_stream(cadmpeg_ir::stream_name!("solidedge:").with_suffix(entry.path()));
                    given_up.push(ctx.give_up_part(part, cause, error)?);
                }
            }
        }
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
        ir.state_root_definition(ctx, root_definition_id(), ExternalDocument::default())?;
        for part in given_up {
            body.losses.push(part.commit(ctx, &mut ir, &mut fidelity)?);
        }
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
        source_fidelity: fidelity,
    })
}

fn root_definition_id() -> ProductDefinitionId {
    ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("solidedge", "model", "product-definition"),
        cadmpeg_ir::identity_key!("root"),
    )
}
