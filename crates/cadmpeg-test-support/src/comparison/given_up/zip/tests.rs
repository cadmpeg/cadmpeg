// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::comparison::{assert_decodes_agree, Mode};
use crate::zip_fixture::{archive, RawEntry};
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_core::dialect::{DialectLayers, FormatIdentity};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::ids::{PointId, UnknownId};
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};
use cadmpeg_ir::source_fidelity::RetainedSourceRecord;
use cadmpeg_ir::{CadIr, CodecFormat, SourceFidelity};
use serde_json::json;
use std::io::Write;

mod b1;
mod recovered;
mod suffix;

fn payload() -> Vec<u8> {
    format!("#1=CARTESIAN_POINT('{}',(1,0,0));", "a".repeat(256)).into_bytes()
}
fn deflated(name: &str, bytes: &[u8]) -> RawEntry {
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    RawEntry::compressed(name, 8, &encoder.finish().unwrap(), bytes)
}
fn fixture(target: &[u8], peer: &[u8], name: &str) -> Vec<u8> {
    archive(&[deflated(name, target), RawEntry::stored("peer.step", peer)])
}
fn read_point(view: View<'_>) -> Result<FinitePoint3, CodecError> {
    let text = std::str::from_utf8(view.window()).map_err(CodecError::malformed)?;
    let at = text
        .rfind(",(")
        .ok_or_else(|| CodecError::malformed("missing coordinate tuple"))?
        + 2;
    let end = text
        .get(at..)
        .and_then(|text| text.find(')'))
        .ok_or_else(|| CodecError::malformed("cut coordinate tuple"))?
        + at;
    let mut values = Vec::new();
    let mut offset = view.start() + at;
    for raw in text[at..end].split(',') {
        let field = view
            .read_slice(offset..offset + raw.len())
            .ok_or_else(|| CodecError::malformed("coordinate field escaped source"))?;
        values.push(
            std::str::from_utf8(field)
                .map_err(CodecError::malformed)?
                .parse::<f64>()
                .map_err(CodecError::malformed)?,
        );
        offset += raw.len() + 1;
    }
    let [x, y, z]: [f64; 3] = values
        .try_into()
        .map_err(|_| CodecError::malformed("point needs three coordinates"))?;
    FinitePoint3::new(cadmpeg_ir::math::Point3::new(x, y, z))
        .ok_or_else(|| CodecError::malformed("nonfinite coordinate"))
}
fn decode(bytes: &[u8], root_failed: bool) -> (CadIr, SourceFidelity, DecodeReport) {
    decode_records(bytes, root_failed, false)
}
fn decode_records(
    bytes: &[u8],
    root_failed: bool,
    split: bool,
) -> (CadIr, SourceFidelity, DecodeReport) {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let mut output = PartOutput::new(CodecFormat::Step);
    ctx.decode_part(
        &mut output,
        root,
        PartSpec::new(PartLevel::parse("zip.archive").unwrap(), vec![]),
        |stage| {
            if root_failed {
                return Err(CodecError::malformed("cut archive"));
            }
            let mut frames = Vec::new();
            let (entries, incomplete) =
                match zip::scan(&ctx, root, &mut frames).map_err(CodecError::malformed)? {
                    zip::ZipScan::Complete { entries } => (
                        entries
                            .into_iter()
                            .map(|entry| (entry.name(), entry.frame_index()))
                            .collect::<Vec<_>>(),
                        None,
                    ),
                    zip::ZipScan::Local { entries, stop }
                        if stop.reason() == zip::RecoveryReason::HeaderIncomplete =>
                    {
                        (
                            entries
                                .into_iter()
                                .map(|entry| (entry.name(), entry.frame_index()))
                                .collect(),
                            stop.frame_index(),
                        )
                    }
                    _ => {
                        return Err(CodecError::malformed(
                            "point fixture has no bounded local schedule",
                        ))
                    }
                };
            let source = cadmpeg_container::ArchiveSnapshot::new(&ctx, root)?;
            for (index, entry) in entries.iter().enumerate() {
                let container = &frames[entry.1];
                let local_header = frames
                    .iter()
                    .find(|part| {
                        part.level().as_str() == "zip.local_header"
                            && part.view().end() == container.view().start()
                    })
                    .ok_or_else(|| {
                        CodecError::malformed("point fixture member has no local header")
                    })?;
                ctx.decode_part(
                    stage,
                    local_header.view(),
                    PartSpec::new(local_header.level(), local_header.framing().to_vec()),
                    |_| Ok(()),
                )?;
                ctx.decode_part(
                    stage,
                    container.view(),
                    PartSpec::new(container.level(), container.framing().to_vec()),
                    |_| Ok(()),
                )?;
                let parent = stage.parent(&ctx, container.view(), "zip.entry")?;
                let member = source.open(
                    &ctx,
                    std::str::from_utf8(entry.0.window()).map_err(CodecError::malformed)?,
                )?;
                let mut start = 0;
                let ends = if split {
                    let mut ends = member
                        .window()
                        .iter()
                        .enumerate()
                        .filter_map(|(index, byte)| (*byte == b';').then_some(index + 1))
                        .collect::<Vec<_>>();
                    if ends.last().copied() != Some(member.end()) {
                        ends.push(member.end());
                    }
                    ends
                } else {
                    vec![member.end()]
                };
                for (ordinal, end) in ends.into_iter().enumerate() {
                    let extent = member
                        .child(start, end)
                        .ok_or_else(|| CodecError::malformed("fixture record escapes member"))?;
                    let id = PointId::mint(if split {
                        format!("step:model:point#{index}:{ordinal}")
                    } else {
                        format!("step:model:point#{index}")
                    })
                    .map_err(CodecError::malformed)?;
                    ctx.decode_part(
                        stage,
                        extent,
                        PartSpec::new(
                            PartLevel::parse("step.simple_entity_instance").unwrap(),
                            if split {
                                let mut framing =
                                    vec![u64_from_index(start)..u64_from_index(start + 3)];
                                if extent.window().ends_with(b";") {
                                    framing.push(u64_from_index(end - 1)..u64_from_index(end));
                                }
                                framing
                            } else {
                                vec![]
                            },
                        )
                        .with_parent(&parent),
                        |record_stage| {
                            ctx.record(id.as_str(), extent, || {
                                if !extent.window().ends_with(b";") {
                                    return Err(CodecError::malformed("cut entity terminator"));
                                }
                                let point = read_point(extent)?;
                                record_stage.insert_model(
                                    &ctx,
                                    cadmpeg_ir::topology::Point::new(id.clone(), point, None),
                                )
                            })
                        },
                    )?;
                    start = end;
                }
            }
            if let Some(index) = incomplete {
                let header = &frames[index];
                ctx.decode_part(
                    stage,
                    header.view(),
                    PartSpec::new(header.level(), header.framing().to_vec()),
                    |_| source.read_recovery_header(&ctx),
                )?;
            }
            Ok(())
        },
    )
    .unwrap();
    let (model, native, mut fidelity, mut losses) = output.into_parts(&ctx).unwrap().into_parts();
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    if !root_failed {
        fidelity
            .insert_retained_record(
                UnknownId::try_from("step:source:image#archive").unwrap(),
                RetainedSourceRecord::whole("file", bytes.to_vec()),
            )
            .unwrap();
    }
    let mut ir = CadIr::empty();
    ir.model = model;
    ir.native = native;
    if split {
        let partitions = ctx
            .source_streams()
            .unwrap()
            .iter()
            .map(|stream| {
                let parts = fidelity
                    .parts()
                    .values()
                    .filter(|part| part.stream() == stream.name());
                let framing = parts
                    .clone()
                    .flat_map(|part| part.framing_bytes().iter().cloned())
                    .collect::<Vec<_>>();
                let failures = parts
                    .filter(|part| part.state() == &(PartState::GivenUp {}))
                    .map(|part| part.offset()..part.offset() + part.byte_len())
                    .collect::<Vec<_>>();
                cadmpeg_ir::source_fidelity::b1::partition_stream(
                    &ctx,
                    stream.name(),
                    stream.byte_len(),
                    fidelity.record_layout(),
                    &framing,
                    &[],
                    &failures,
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        let index =
            cadmpeg_ir::source_fidelity::b1::classification::ClassificationIndex::new(&ctx, &ir)
                .unwrap();
        fidelity
            .finalize_b1(&ctx, &index, "step", &partitions, &mut losses)
            .unwrap();
    }
    let report = serde_json::from_value(json!({
        "identity": FormatIdentity::<DialectLayers>::unclassified("step"),
        "transfer": DecodeTransfer { scope: ir.scope, geometry_transferred: false },
        "losses": losses, "notes": [],
    }))
    .unwrap();
    (ir, fidelity, report)
}
fn allowances(
    old_bytes: &[u8],
    new_bytes: &[u8],
    before: &(CadIr, SourceFidelity, DecodeReport),
    after: &(CadIr, SourceFidelity, DecodeReport),
) -> Result<Allowances, String> {
    let old_arena = DecodeArena::new();
    let (old_ctx, old_root) =
        DecodeContext::from_root_bytes(old_bytes, &old_arena, &DecodePolicy::service()).unwrap();
    let old_root = old_ctx.name_stream(old_root, "file").unwrap();
    let new_arena = DecodeArena::new();
    let (new_ctx, new_root) =
        DecodeContext::from_root_bytes(new_bytes, &new_arena, &DecodePolicy::service()).unwrap();
    let new_root = new_ctx.name_stream(new_root, "file").unwrap();
    Allowances::given_up_zip(
        DecodeSnapshot::new(&before.0, &before.1),
        DecodeSnapshot::new(&after.0, &after.1),
        &after.2,
        (&old_ctx, old_root),
        (&new_ctx, new_root),
    )
}

#[test]
fn changed_compressed_length_uses_actual_member_correspondence_and_keeps_peer_exact() {
    let good = payload();
    let bad = vec![0xff; good.len()];
    let peer = b"#2=CARTESIAN_POINT('',(2,0,0));";
    let old = fixture(&good, peer, "target.step");
    let new = fixture(&bad, peer, "target.step");
    assert_ne!(old.len(), new.len());
    let before = decode(&old, false);
    let after = decode(&new, false);
    let allowed = allowances(&old, &new, &before, &after).unwrap();
    assert_decodes_agree(
        &serde_json::to_value(&before.0).unwrap(),
        &serde_json::to_value(&after.0).unwrap(),
        &allowed,
        Mode::Same,
    );
    assert_eq!(after.0.model.points.len(), 1);
    assert_eq!(after.0.model.points[0].id.as_str(), "step:model:point#1");
}

#[test]
fn shortened_member_requires_a_real_prefix_and_its_actual_cut_containing_home() {
    let good = payload();
    let peer = b"#2=CARTESIAN_POINT('',(2,0,0));";
    let old = fixture(&good, peer, "target.step");
    let new = fixture(&good[..good.len() - 5], peer, "target.step");
    let before = decode(&old, false);
    let after = decode(&new, false);
    let allowed = allowances(&old, &new, &before, &after).unwrap();
    assert_decodes_agree(
        &serde_json::to_value(&before.0).unwrap(),
        &serde_json::to_value(&after.0).unwrap(),
        &allowed,
        Mode::Same,
    );
    let wrong = fixture(b"unrelated shorter bytes", peer, "target.step");
    let wrong_after = decode(&wrong, false);
    assert!(allowances(&old, &wrong, &before, &wrong_after).is_err());
}

#[test]
fn wrong_root_member_identity_and_unrelated_member_edits_grant_no_permission() {
    let good = payload();
    let bad = vec![0xff; good.len()];
    let peer = b"#2=CARTESIAN_POINT('',(2,0,0));";
    let old = fixture(&good, peer, "target.step");
    let before = decode(&old, false);
    let bad_source = fixture(&bad, peer, "target.step");
    let after = decode(&bad_source, false);
    assert!(allowances(&old, &old, &before, &after).is_err());
    let renamed = fixture(&bad, peer, "another.step");
    let renamed_after = decode(&renamed, false);
    assert!(allowances(&old, &renamed, &before, &renamed_after).is_err());
    let changed_peer = fixture(&bad, b"#2=CARTESIAN_POINT('',(9,0,0));", "target.step");
    let changed_after = decode(&changed_peer, false);
    assert!(allowances(&old, &changed_peer, &before, &changed_after).is_err());
}

#[test]
fn whole_archive_prefix_cut_requires_its_actual_failed_root_and_matches_retained_bytes() {
    let old = fixture(
        &payload(),
        b"#2=CARTESIAN_POINT('',(2,0,0));",
        "target.step",
    );
    let new = &old[..old.len() / 2];
    let before = decode(&old, false);
    let mut after = decode(new, true);
    after
        .0
        .set_scope(cadmpeg_ir::document::ContentScope::container())
        .unwrap();
    let mut report = serde_json::to_value(&after.2).unwrap();
    report["transfer"]["scope"] = serde_json::to_value(after.0.scope).unwrap();
    after.2 = serde_json::from_value(report).unwrap();
    let allowed = allowances(&old, new, &before, &after).unwrap();
    assert_decodes_agree(
        &serde_json::to_value(&before.0).unwrap(),
        &serde_json::to_value(&after.0).unwrap(),
        &allowed,
        Mode::Same,
    );
    assert!(allowed
        .lost_classes
        .contains(&cadmpeg_ir::document::ArenaClass::Model));
    assert!(allowed
        .lost_classes
        .contains(&cadmpeg_ir::document::ArenaClass::Design));
    let unrelated = vec![0; new.len()];
    let unrelated_after = decode(&unrelated, true);
    assert!(allowances(&old, &unrelated, &before, &unrelated_after).is_err());
}

#[test]
fn a_member_failure_does_not_prove_loss_of_an_empty_document_class() {
    let good = payload();
    let old = fixture(&good, b"#2=CARTESIAN_POINT('',(2,0,0));", "target.step");
    let new = fixture(
        &vec![0xff; good.len()],
        b"#2=CARTESIAN_POINT('',(2,0,0));",
        "target.step",
    );
    let before = decode(&old, false);
    let mut after = decode(&new, false);
    after
        .0
        .set_scope(cadmpeg_ir::document::ContentScope::geometry())
        .unwrap();
    let mut report = serde_json::to_value(&after.2).unwrap();
    report["transfer"]["scope"] = serde_json::to_value(after.0.scope).unwrap();
    after.2 = serde_json::from_value(report).unwrap();
    let allowed = allowances(&old, &new, &before, &after).unwrap();
    assert!(allowed.lost_classes.is_empty());
    assert!(crate::comparison::agree(
        &serde_json::to_value(&before.0).unwrap(),
        &serde_json::to_value(&after.0).unwrap(),
        &allowed,
        Mode::Same,
    )
    .is_err());
}
