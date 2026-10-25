// SPDX-License-Identifier: Apache-2.0
//! Framing follows exact stored mappings without becoming semantic read credit.

use super::*;
use cadmpeg_core::decode::{ByteRange, ExpandSpec};

fn inventory(ctx: &DecodeContext<'_>) -> Inventory {
    let mut inventory = Inventory(BTreeMap::new());
    for source in ctx.source_streams().unwrap() {
        inventory.registered(ctx, &source).unwrap();
    }
    inventory
}

#[test]
fn stored_xml_framing_and_whitespace_leave_no_physical_alias_gap_or_extra_read() {
    let xml = "<Document FileVersion='1'>\n<Objects Count='0'/>\n</Document>";
    let prefix = b"prefix!";
    let mut bytes = prefix.to_vec();
    bytes.extend_from_slice(xml.as_bytes());
    bytes.extend_from_slice(b"suffix!");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let member = ctx
        .register_named_slice(
            root,
            ByteRange {
                start: prefix.len() as u64,
                end: (prefix.len() + xml.len()) as u64,
            },
            "stored.xml",
        )
        .unwrap();
    let source = crate::xml_framing::XmlSource::new(&ctx, member).unwrap();
    let record = ctx
        .record("fcstd:native:document#0", member, || {
            Ok(crate::native::DocumentFacts {
                id: "fcstd:native:document#0".into(),
                root_name: source.read_name(0)?.to_owned(),
                file_version: source.attribute(&ctx, 0, "FileVersion")?.unwrap().into(),
                program_version: None,
                object_count: 0,
                domains: Vec::new(),
            })
        })
        .unwrap();
    let mut ir = CadIr::empty();
    ir.native
        .namespace_mut("fcstd")
        .set_arena(&ctx, "document", &[record])
        .unwrap();
    let mut inventory = inventory(&ctx);
    let mut framed = Vec::new();
    for element in source.elements() {
        for field in element.framing() {
            framed.push(field.start()..field.end());
            inventory.range(&ctx, field, false).unwrap();
        }
    }
    let whitespace = source.insignificant(&ctx).unwrap();
    assert_eq!(whitespace.len(), 2);
    for view in whitespace {
        framed.push(view.start()..view.end());
        inventory.range(&ctx, view, true).unwrap();
    }
    let mut fidelity = SourceFidelity::default();
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    let original_reads = fidelity.record_layout()["fcstd:native:document#0"]
        .reads()
        .clone();
    let mut losses = Vec::new();
    inventory
        .finalize(&ctx, &ir, &mut fidelity, &mut losses)
        .unwrap();
    assert_eq!(
        fidelity.record_layout()["fcstd:native:document#0"].reads(),
        &original_reads
    );
    assert_eq!(
        original_reads
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["file", "stored.xml"]
    );
    for range in framed {
        for (stream, start, end) in [
            ("stored.xml", range.start as u64, range.end as u64),
            (
                "file",
                (prefix.len() + range.start) as u64,
                (prefix.len() + range.end) as u64,
            ),
        ] {
            assert!(
                fidelity.b1_spans()[stream].spans().iter().all(|span| {
                    let b1 = span.range();
                    b1.end <= start || end <= b1.start
                }),
                "framing/whitespace {stream}:{start}..{end} became B1"
            );
        }
    }
    // Count metadata is neither retained semantic content nor Appendix C framing.
    let count = xml.find("Count").unwrap() as u64;
    for (stream, at) in [("stored.xml", count), ("file", count + prefix.len() as u64)] {
        assert!(fidelity.b1_spans()[stream].spans().iter().any(|span| {
            let range = span.range();
            range.start <= at && at < range.end
        }));
    }
}

#[test]
fn composite_framing_maps_only_its_exact_segments_and_stops_at_expansion() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"01234567ZIP!cdef", &arena, &DecodePolicy::service())
            .unwrap();
    let stored = ctx
        .register_named_slice(root, ByteRange { start: 0, end: 4 }, "stored")
        .unwrap();
    let mut writer = ctx.begin_expand(ExpandSpec::Exact(4)).unwrap();
    writer.write(b"wxyz").unwrap();
    let expanded = writer
        .finalize_from(root.child(8, 12).unwrap(), "expanded")
        .unwrap();
    let combined = ctx
        .concat_named_views(
            &[stored.child(1, 3).unwrap(), expanded.child(1, 4).unwrap()],
            "combined",
        )
        .unwrap();
    let mut inventory = inventory(&ctx);
    inventory
        .range(&ctx, combined.child(1, 4).unwrap(), false)
        .unwrap();
    inventory
        .range(&ctx, combined.child(0, 1).unwrap(), true)
        .unwrap();
    let mut actual = ctx.source_streams().unwrap();
    actual.sort_unstable_by(|left, right| left.name().cmp(right.name()));
    inventory.project_alias_ranges(&ctx, &actual).unwrap();
    assert_eq!(inventory.0["combined"].framing, [1..4]);
    assert_eq!(inventory.0["stored"].framing, [2..3]);
    assert_eq!(inventory.0["expanded"].framing, [1..3]);
    assert_eq!(inventory.0["file"].framing, [2..3]);
    assert_eq!(inventory.0["stored"].insignificant, [1..2]);
    assert_eq!(inventory.0["file"].insignificant, [1..2]);
    assert!(inventory.0["file"]
        .framing
        .iter()
        .all(|range| range.end <= 8 || 12 <= range.start));
    assert!(ctx.take_record_captures().unwrap().is_empty());
}
