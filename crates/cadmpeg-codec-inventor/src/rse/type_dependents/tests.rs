// SPDX-License-Identifier: Apache-2.0
//! Actual source callback results govern dispatch after a child allowance fails.

use crate::native::MetaTypeRecord;
use crate::rse::SegmentMetaState;
use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};
use cadmpeg_ir::CodecFormat;

fn level(name: &str) -> PartLevel {
    PartLevel::parse(name).unwrap()
}

#[test]
fn successful_source_descriptor_facts_keep_their_indices_and_exact_dependent_peers() {
    let bytes = crate::test_support::test_fixtures::primary_envelope_fixture_with_design(
        &[],
        &[
            ([0x11; 16], Vec::new()),
            ([0x22; 16], Vec::new()),
            ([0x33; 16], Vec::new()),
        ],
    );
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let snapshot = CompoundSnapshot::new(&ctx, root).unwrap();
    let metadata_source = snapshot
        .open(&ctx, snapshot.stream("RSeStorage/Mdc").unwrap())
        .unwrap();
    let bulk_source = snapshot
        .open(&ctx, snapshot.stream("RSeStorage/Bdc").unwrap())
        .unwrap();
    let SegmentMetaState::Parsed(mut meta) =
        super::super::parse_meta_stream(&ctx, metadata_source).unwrap()
    else {
        panic!("actual metadata frame")
    };
    let bulk = super::super::parse_bulk_stream(&ctx, bulk_source).unwrap();
    let registry_source = snapshot
        .open(&ctx, snapshot.stream("RSeStorage/RSeSegInfo").unwrap())
        .unwrap();
    let registry = crate::database::parse_registry(&ctx, registry_source.window()).unwrap();
    let version = registry
        .entries
        .iter()
        .find(|entry| entry.segment_id == meta.segment_id)
        .unwrap()
        .version
        .major;
    assert_eq!(version, 18);
    let frame = crate::records::frame_type_table(meta.body).unwrap();
    let count = ctx.capture_part_extent(frame.count_source).unwrap();
    let span = ctx.capture_part_extent(frame.span_source).unwrap();
    let mut output = PartOutput::new(CodecFormat::Inventor);
    let slots = ctx
        .decode_part(
            &mut output,
            meta.source,
            PartSpec::new(level("cfb.stream"), Vec::new()),
            |member| {
                ctx.decode_part(
                    member,
                    meta.compressed,
                    PartSpec::new(level("inventor.meta_body"), Vec::new()),
                    |body| {
                        ctx.decode_part(
                            body,
                            frame.extent,
                            PartSpec::new(
                                level("inventor.meta_forward_section"),
                                vec![
                                    count.offset()..count.offset() + count.byte_len(),
                                    span.offset()..span.offset() + span.byte_len(),
                                ],
                            ),
                            |table| {
                                let mut values = ctx.alloc_filled(
                                    frame.count,
                                    None,
                                    "test source descriptor slots",
                                )?;
                                let mut source = frame.payload;
                                for (index, value) in values.iter_mut().enumerate() {
                                    let extent = crate::reader::take_child(
                                        &mut source,
                                        28,
                                        "type descriptor",
                                    )?;
                                    let id = format!("inventor:rse:meta-type#dc-{index}");
                                    *value = ctx.decode_part(
                                        table,
                                        extent,
                                        PartSpec::new(
                                            level("inventor.type_descriptor"),
                                            Vec::new(),
                                        ),
                                        |record| {
                                            ctx.record(&id, extent, || {
                                                let value = crate::records::parse_type_descriptor(
                                                    extent,
                                                    u32::try_from(index).unwrap(),
                                                )?;
                                                if value.index == 1 {
                                                    return Err(ctx.refuse_part_limit(
                                                        ResourceDimension::RetainedBytes,
                                                        0,
                                                        0,
                                                        1,
                                                        "test source descriptor allowance",
                                                    ));
                                                }
                                                record.insert_native(
                                                    &ctx,
                                                    "meta_types",
                                                    MetaTypeRecord {
                                                        id: id.clone(),
                                                        token: "dc".into(),
                                                        index: value.index,
                                                        type_id: value
                                                            .id
                                                            .iter()
                                                            .map(|byte| format!("{byte:02x}"))
                                                            .collect(),
                                                        fields: value.fields,
                                                    },
                                                )?;
                                                Ok(value)
                                            })
                                        },
                                    )?;
                                }
                                Ok(values)
                            },
                        )
                    },
                )
            },
        )
        .unwrap()
        .flatten()
        .flatten()
        .unwrap();
    assert_eq!(slots.len(), 3);
    assert_eq!(slots[0].unwrap().id, [0x11; 16]);
    assert!(slots[1].is_none());
    assert_eq!(slots[2].unwrap().id, [0x33; 16]);
    meta.source_types = Some(slots);
    for descriptor in &mut meta.tables.types {
        descriptor.id = [0xff; 16];
    }
    let (table, source_count) = super::stage(&ctx, &mut output, &bulk, &meta, "dc", Some(version))
        .unwrap()
        .unwrap();
    assert_eq!(source_count, 3);
    assert_eq!(table.records.len(), 2);
    assert_eq!(table.records[0].ordinal, 0);
    assert_eq!(table.records[0].selector, 0);
    assert_eq!(table.records[0].type_id, [0x11; 16]);
    assert_eq!(table.records[1].ordinal, 2);
    assert_eq!(table.records[1].selector, 2);
    assert_eq!(table.records[1].type_id, [0x33; 16]);
    assert_eq!(table.stream_trailer.window(), &u32::MAX.to_le_bytes());
    let (_, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let records = native
        .namespace("inventor")
        .unwrap()
        .arena_as::<MetaTypeRecord>("meta_types")
        .unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].index, 0);
    assert_eq!(records[0].type_id, "11111111111111111111111111111111");
    assert_eq!(records[1].index, 2);
    assert_eq!(records[1].type_id, "33333333333333333333333333333333");
    // #215 §4.5 rules 2-3: the record of ordinal 1 needs the given-up
    // descriptor 1, so it is dropped with it and the descriptor's one loss
    // names it; it is not a part of its own.
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.to_string(), "decode/record.unreadable");
    assert!(losses[0].message.contains("inventor:rse:meta-type#dc-1"));
    assert!(losses[0].message.contains("inventor:rse:record#dc-1"));
    assert!(!fidelity
        .parts()
        .values()
        .any(|part| part.level().as_str() == "inventor.rse_record_body"));
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 2);
    assert!(captures
        .iter()
        .any(|capture| capture.id() == "inventor:rse:meta-type#dc-0"));
    assert!(captures
        .iter()
        .any(|capture| capture.id() == "inventor:rse:meta-type#dc-2"));
    assert!(!captures
        .iter()
        .any(|capture| capture.id() == "inventor:rse:meta-type#dc-1"));
}
