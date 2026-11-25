// SPDX-License-Identifier: Apache-2.0
//! `IronCAD` scene decode.

use cadmpeg_asm::dialect::KernelHeaderRef;
use cadmpeg_container::compound::{CompoundEntry, CompoundSnapshot};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::{DialectLayerError, DialectLayers, LayerInstance};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::SourceMeta;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{CadIr, SourceFidelity};

use crate::container::BREP_DATA;
use crate::loss::IronCadLossCode;
use crate::tree::{Kernel, KernelRecord};
use crate::{container, dialect, frame, kernel, tree};

pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
    let snapshot = container::open(ctx, root)?;
    let mut layers = dialect::layers();
    let mut body = DecodeBody::new(DecodeTransfer::at(ctx.depth(), false));
    body.losses.extend(
        snapshot
            .deviations()
            .map(|deviation| IronCadLossCode::ContainerNoncanonical.note(deviation.description())),
    );
    if !ctx.container_only() {
        if let Some(records) = records(ctx, &snapshot, &mut body.losses)? {
            classify(&records, &mut layers, &mut body.losses);
        }
        scene_streams(&snapshot, &mut body.losses);
    }
    let mut ir = CadIr::empty();
    ir.source = Some(SourceMeta::classified(
        layers,
        std::collections::BTreeMap::default(),
    ));
    Ok(Decoded {
        ir,
        body,
        source_fidelity: SourceFidelity::new(Sha256Digest::digest(root.window())),
    })
}

/// Reads the kernel records of `Brep Data`, or charges the reason they are absent.
fn records<'a>(
    ctx: &DecodeContext<'a>,
    snapshot: &CompoundSnapshot<'a>,
    losses: &mut Vec<LossNote>,
) -> Result<Option<Vec<KernelRecord<'a>>>, CodecError> {
    let Some(entry) = snapshot.stream(BREP_DATA) else {
        losses.push(IronCadLossCode::FrameUnreadable.note("`Brep Data` is not a stream"));
        return Ok(None);
    };
    let payload = match snapshot
        .open(ctx, entry)
        .and_then(|stream| frame::payload(ctx, stream))
    {
        Ok(payload) => payload,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(error) => {
            losses.push(
                IronCadLossCode::FrameUnreadable
                    .note(format!("`Brep Data` frame could not be read: {error}")),
            );
            return Ok(None);
        }
    };
    match tree::read(payload) {
        Ok(records) => Ok(Some(records)),
        Err(error) => {
            losses.push(IronCadLossCode::RecordTreeUnreadable.note(format!(
                "`Brep Data` record tree could not be read: {}",
                CodecError::from(error)
            )));
            Ok(None)
        }
    }
}

/// Adds a dialect layer for every kernel record and charges each pending body.
fn classify(records: &[KernelRecord<'_>], layers: &mut DialectLayers, losses: &mut Vec<LossNote>) {
    let acis_count = records
        .iter()
        .filter(|record| record.kernel == Kernel::Acis)
        .count();
    let acis_instance = if acis_count > 1 {
        LayerInstance::Tagged
    } else {
        LayerInstance::Sole
    };
    let mut parasolid = Vec::new();
    for (ordinal, record) in records.iter().enumerate() {
        let carrier = format!("{BREP_DATA}/record {ordinal}");
        if !record.canonical_header {
            losses.push(
                IronCadLossCode::RecordHeaderUnrecognized
                    .note(format!("{carrier}: header words differ from 1, 1, 1, 0")),
            );
        }
        let name = match record.kernel {
            Kernel::Parasolid => {
                match kernel::parasolid_schema(record.bytes) {
                    Ok(schema) => {
                        parasolid.push((schema, cadmpeg_parasolid::Carrier::new(carrier.clone())));
                    }
                    Err(error) => losses.push(
                        IronCadLossCode::KernelPrologueUnreadable
                            .note(format!("{carrier}: {error}")),
                    ),
                }
                "Parasolid"
            }
            Kernel::Acis => {
                match kernel::acis_header(record.bytes).and_then(|header| {
                    cadmpeg_asm::dialect::classify_layer(
                        KernelHeaderRef::Acis(&header),
                        &carrier,
                        acis_instance,
                    )
                    .map_err(kernel::PrologueError::from)
                }) {
                    Ok(matched) => {
                        if let Err(error) = layers.insert(matched) {
                            losses.push(
                                IronCadLossCode::KernelUnverified.note(collision(&error, &carrier)),
                            );
                        }
                    }
                    Err(error) => losses.push(
                        IronCadLossCode::KernelPrologueUnreadable
                            .note(format!("{carrier}: {error}")),
                    ),
                }
                "ACIS"
            }
            Kernel::Other(tag) => {
                losses.push(IronCadLossCode::KernelUnrecognized.note(format!(
                    "{carrier}: kernel tag {tag:#010x} is not ACIS or Parasolid"
                )));
                continue;
            }
        };
        losses.push(IronCadLossCode::BodiesPending.note(format!(
            "{carrier}: {name} body of {} bytes at nesting depth {} is not decoded",
            record.bytes.window().len(),
            record.depth
        )));
    }
    for message in
        cadmpeg_parasolid::push_extras(layers, cadmpeg_parasolid::extra_layers(parasolid, &[]))
    {
        losses.push(IronCadLossCode::KernelUnverified.note(message));
    }
    for matched in layers.iter() {
        let message = cadmpeg_parasolid::unverified_message(matched)
            .or_else(|| cadmpeg_asm::dialect::unverified_message("ACIS record", matched));
        if let Some(message) = message {
            losses.push(IronCadLossCode::KernelUnverified.note(message));
        }
    }
}

fn collision(error: &DialectLayerError, carrier: &str) -> String {
    match error {
        DialectLayerError::Duplicate(rejected) => format!(
            "the scene produced a duplicate {} dialect layer at carrier {carrier}; \
             the later classification was omitted",
            rejected.format()
        ),
        DialectLayerError::MissingInstance(rejected) => format!(
            "the scene produced an untagged {} dialect layer at carrier {carrier}; \
             the later classification was omitted",
            rejected.format()
        ),
    }
}

/// Charges every scene stream other than `Brep Data` that holds bytes.
fn scene_streams(snapshot: &CompoundSnapshot<'_>, losses: &mut Vec<LossNote>) {
    for entry in snapshot.entries() {
        let CompoundEntry::Stream(stream) = entry else {
            continue;
        };
        if stream.path().eq_ignore_ascii_case(BREP_DATA) || stream.logical_size() == 0 {
            continue;
        }
        losses.push(IronCadLossCode::SceneStreamsPending.note(format!(
            "stream `{}` of {} bytes is not decoded",
            stream.path(),
            stream.logical_size()
        )));
    }
}
