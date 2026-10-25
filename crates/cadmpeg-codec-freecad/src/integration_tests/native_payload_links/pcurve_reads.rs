// SPDX-License-Identifier: Apache-2.0
//! Actual multi-input neutral Pcurve read ownership, #214 §3.0.
use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_test_support::framing::{freecad::brep::frame_brep_member, zip};

#[test]
fn every_public_binary_pcurve_owns_real_curve_topology_and_surface_input_reads() {
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/binary_exact_shape.FCStd"
    ));
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded.ir().model.pcurves.is_empty());
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let payloads = crate::brep::records::payloads_untracked(namespace).unwrap();
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let mut frames = Vec::new();
    let zip_entries = zip::frame(&ctx, root, &mut frames).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    for payload in &payloads {
        let entry = entries
            .iter()
            .find(|entry| entry.id() == payload.entry)
            .unwrap();
        let zip_entry = zip_entries
            .iter()
            .find(|candidate| candidate.name().window() == entry.name().as_bytes())
            .unwrap();
        let source = archive.open(&ctx, entry.name()).unwrap();
        frame_brep_member(
            &ctx,
            source,
            zip_entry.name(),
            zip_entry.frame_index(),
            &mut frames,
        )
        .unwrap();
    }
    let layouts = decoded.source_fidelity().record_layout();
    for pcurve in &decoded.ir().model.pcurves {
        let layout = &layouts[pcurve.id.as_str()];
        let payload = payloads
            .iter()
            .find(|payload| Some(payload.id.as_str()) == pcurve.native_ref.as_deref())
            .unwrap();
        let home = &layouts[&payload.id];
        assert_eq!(layout.stream(), home.stream());
        assert!(home.offset() <= layout.offset());
        assert!(layout.offset() + layout.byte_len() <= home.offset() + home.byte_len());
        let reads = &layout.reads()[layout.stream()];
        let roots = frames
            .iter()
            .filter(|frame| {
                frame.level().as_str() == "freecad.brep_binary_root"
                    && frame.capture().stream() == layout.stream()
            })
            .collect::<Vec<_>>();
        assert!(
            !roots.is_empty(),
            "independent actual Root-use frame exists"
        );
        assert!(
            roots.iter().any(|root| reads.iter().any(|read| read.start
                < root.capture().offset() + root.capture().byte_len()
                && root.capture().offset() < read.end)),
            "{} has no actual Root admission getter",
            pcurve.id
        );
        let mut tables = vec![
            b"Curve2ds".as_slice(),
            b"Surfaces".as_slice(),
            b"TShapes".as_slice(),
        ];
        let edges = decoded
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.pcurves.iter().any(|usage| usage.pcurve == pcurve.id))
            .filter_map(|coedge| {
                decoded
                    .ir()
                    .model
                    .edges
                    .iter()
                    .find(|edge| edge.id == coedge.edge)
            })
            .collect::<Vec<_>>();
        if !edges.is_empty() && edges.iter().all(|edge| edge.curve().is_some()) {
            tables.push(b"Curves".as_slice());
        }
        for table in tables {
            let rows = frames
                .iter()
                .filter(|frame| {
                    frame.level().as_str() == "freecad.brep_binary_record"
                        && frame.capture().stream() == layout.stream()
                        && frame.external_dispatch().first().is_some_and(|marker| {
                            marker
                                .window()
                                .split(|byte| byte.is_ascii_whitespace())
                                .next()
                                == Some(table)
                        })
                })
                .collect::<Vec<_>>();
            assert!(!rows.is_empty(), "independent {table:?} rows exist");
            assert!(
                rows.iter().any(|row| reads.iter().any(|read| read.start
                    < row.capture().offset() + row.capture().byte_len()
                    && row.capture().offset() < read.end)),
                "{} has no actual {table:?} getter",
                pcurve.id
            );
            if table == b"Curve2ds" {
                assert!(
                    rows.iter()
                        .any(|row| row.capture().offset() == layout.offset()
                            && row.capture().byte_len() == layout.byte_len()),
                    "primary Pcurve scope is its complete actual Curve2d source row"
                );
            }
        }
    }
}
