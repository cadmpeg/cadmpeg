// SPDX-License-Identifier: Apache-2.0
//! Actual source callback results govern dispatch after a child allowance fails.

use std::fmt::Write as _;

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
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("the fixture is a compound file");
    let metadata_source = snapshot
        .open(
            &ctx,
            snapshot
                .stream("RSeStorage/Mdc")
                .expect("the fixture holds the metadata stream"),
        )
        .expect("the metadata stream opens");
    let bulk_source = snapshot
        .open(
            &ctx,
            snapshot
                .stream("RSeStorage/Bdc")
                .expect("the fixture holds the bulk stream"),
        )
        .expect("the bulk stream opens");
    let SegmentMetaState::Parsed(mut meta) =
        super::super::parse_meta_stream(&ctx, metadata_source).expect("the metadata stream parses")
    else {
        panic!("actual metadata frame")
    };
    let bulk = super::super::parse_bulk_stream(&ctx, bulk_source).expect("the bulk stream parses");
    let registry_source = snapshot
        .open(
            &ctx,
            snapshot
                .stream("RSeStorage/RSeSegInfo")
                .expect("the fixture holds the segment registry stream"),
        )
        .expect("the segment registry stream opens");
    let registry = crate::database::parse_registry(&ctx, registry_source.window())
        .expect("the segment registry parses");
    let version = registry
        .entries
        .iter()
        .find(|entry| entry.segment_id == meta.segment_id)
        .expect("the registry lists the metadata segment")
        .version
        .major;
    assert_eq!(version, 18);
    let frame = crate::records::frame_type_table(meta.body)
        .expect("the metadata body holds a type table frame");
    let mut output = PartOutput::new(CodecFormat::Inventor);
    let slots = ctx
        .decode_part(
            &mut output,
            meta.source,
            part_spec("cfb.stream", meta.source, stream).expect("the stream part spec is valid"),
            |member| {
                ctx.decode_part(
                    member,
                    meta.compressed,
                    part_spec("inventor.meta_body", meta.compressed, stream)
                        .expect("the meta body part spec is valid"),
                    |body| {
                        ctx.decode_part(
                            body,
                            frame.extent,
                            part_spec("inventor.meta_forward_section", frame.extent, expanded)
                                .expect("the forward section part spec is valid"),
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
                                            .expect("the type descriptor part spec is valid"),
                                        |record| {
                                            let value = crate::records::parse_type_descriptor(
                                                extent,
                                                u32::try_from(index)
                                                    .expect("the descriptor index fits u32"),
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
                                                    type_id: value.id.iter().fold(
                                                        String::new(),
                                                        |mut out, byte| {
                                                            write!(out, "{byte:02x}").expect(
                                                                "writing to a String succeeds",
                                                            );
                                                            out
                                                        },
                                                    ),
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
        .expect("the source part decodes")
        .flatten()
        .flatten()
        .expect("the type table part is formed");
    assert_eq!(slots.len(), 3);
    assert_eq!(slots[0].expect("descriptor 0 is formed").id, [0x11; 16]);
    assert!(slots[1].is_none());
    assert_eq!(slots[2].expect("descriptor 2 is formed").id, [0x33; 16]);
    meta.source_types = Some(slots);
    for descriptor in &mut meta.tables.types {
        descriptor.id = [0xff; 16];
    }
    let (table, source_count) = super::stage(&ctx, &mut output, &bulk, &meta, "dc", Some(version))
        .expect("the dependent stage succeeds")
        .expect("the stage forms a type table");
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
        .expect("the inventor namespace exists")
        .arena_as::<MetaTypeRecord>("meta_types")
        .expect("the meta types arena holds meta type records");
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
