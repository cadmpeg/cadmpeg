// SPDX-License-Identifier: Apache-2.0
//! Actual source callback results govern dispatch after a child allowance fails.

use crate::decode::part_spec;
use crate::native::MetaTypeRecord;
use crate::rse::SegmentMetaState;
use crate::test_support::given_up_records;
use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};
use cadmpeg_ir::CodecFormat;

fn stream() -> String {
    "RSeStorage/Mdc".into()
}

fn expanded() -> String {
    "RSeStorage/Mdc:expanded".into()
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
    let mut output = PartOutput::new(CodecFormat::Inventor);
    let slots = ctx
        .decode_part(
            &mut output,
            meta.source,
            part_spec("cfb.stream", meta.source, stream).unwrap(),
            |member| {
                ctx.decode_part(
                    member,
                    meta.compressed,
                    part_spec("inventor.meta_body", meta.compressed, stream).unwrap(),
                    |body| {
                        ctx.decode_part(
                            body,
                            frame.extent,
                            part_spec("inventor.meta_forward_section", frame.extent, expanded)
                                .unwrap(),
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
                                        part_spec("inventor.type_descriptor", extent, expanded)
                                            .unwrap(),
                                        |record| {
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
    let (_, native, fidelity, losses) = output.into_parts();
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
    // The record of ordinal 1 needs the given-up descriptor 1, so it is
    // not formed and is not given up on its own.
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.to_string(), "decode/record.unreadable");
    assert_eq!(given_up_records(&fidelity).len(), 1);
}
