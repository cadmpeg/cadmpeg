// SPDX-License-Identifier: Apache-2.0
//! Kernel producers own only their real XML controls and consumed binary rows.

use super::*;
use crate::native::entry_contents::EntryContents;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartState;

fn entries(result: &cadmpeg_ir::codec::DecodeResult) -> Vec<crate::native::EntryRecord> {
    result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as("entries")
        .unwrap()
}

#[test]
fn kernel_neutral_layouts_read_their_real_counts_rows_and_only_used_controls() {
    let bytes = source();
    let result = FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let mesh_stream = ctx
        .capture_part_extent(archive.open(&ctx, "MeshKernel.bms").unwrap())
        .unwrap();
    let cloud_stream = ctx
        .capture_part_extent(archive.open(&ctx, "Cloud").unwrap())
        .unwrap();
    let document = archive.open(&ctx, "Document.xml").unwrap();
    let document_stream = ctx.capture_part_extent(document).unwrap();
    let document_text = std::str::from_utf8(document.window()).unwrap();
    let mtrx = "1 0 0 10 0 1 0 20 0 0 1 30 0 0 0 1";
    let mtrx_key = u64::try_from(document_text.find("mtrx='").unwrap()).unwrap();
    let mtrx_start = mtrx_key + 6;
    let layouts = result.source_fidelity().record_layout();
    let mesh = &result.ir().model.tessellations[0];
    assert_eq!(
        layouts[mesh.id.as_str()].reads()[mesh_stream.stream()],
        [0..8, 264..320]
    );
    assert_eq!(result.ir().model.points.len(), 2);
    for (ordinal, point) in result.ir().model.points.iter().enumerate() {
        let layout = &layouts[point.id.as_str()];
        let expected = if ordinal == 0 {
            vec![0..16]
        } else {
            vec![0..4, 16..28]
        };
        assert_eq!(layout.reads()[cloud_stream.stream()], expected);
        let xml_reads = &layout.reads()[document_stream.stream()];
        for expected in [
            mtrx_key..mtrx_key + 4,
            mtrx_start..mtrx_start + u64::try_from(mtrx.len()).unwrap(),
        ] {
            assert!(
                xml_reads
                    .iter()
                    .any(|read| read.start <= expected.start && expected.end <= read.end),
                "point {ordinal} lacks its actual matrix field {expected:?}: {xml_reads:?}"
            );
        }
    }
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    crate::fidelity::b1::assert_classified_spans(&result);
}

#[test]
fn application_and_gui_aliases_share_one_physical_kernel_schedule_and_entry_contents() {
    use std::io::Read;
    let original = source();
    let mut archive = zip::ZipArchive::new(Cursor::new(&original)).unwrap();
    let mut members = Vec::new();
    for name in ["Document.xml", "MeshKernel.bms", "Cloud"] {
        let mut bytes = Vec::new();
        archive
            .by_name(name)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        members.push((name, bytes));
    }
    let gui = b"<Document SchemaVersion='1'><ViewProviderData Count='2'><ViewProvider name='Mesh'><Properties Count='1'><Property name='MeshData' type='Mesh::PropertyMeshKernel'><Mesh file='MeshKernel.bms'/></Property></Properties></ViewProvider><ViewProvider name='Cloud'><Properties Count='1'><Property name='PointsData' type='Points::PropertyPointKernel'><Points file='Cloud' mtrx='1 0 0 10 0 1 0 20 0 0 1 30 0 0 0 1'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let source = archive_entries(&[
        (members[0].0, members[0].1.as_slice()),
        (members[1].0, members[1].1.as_slice()),
        (members[2].0, members[2].1.as_slice()),
        ("GuiDocument.xml", gui),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let native = result.ir().native.namespace("fcstd").unwrap();
    let app = native
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let gui = native
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .unwrap();
    for name in ["MeshKernel.bms", "Cloud"] {
        let all_entries = entries(&result);
        let entry = all_entries
            .iter()
            .find(|entry| entry.name() == name)
            .unwrap();
        assert_eq!(entry.contents().len(), 1);
        assert!(matches!(
            (name, &entry.contents()[0]),
            ("MeshKernel.bms", EntryContents::Mesh { .. })
                | ("Cloud", EntryContents::Points { .. })
        ));
        let app = app
            .iter()
            .find(|property| property.side_entries().iter().any(|file| file == name))
            .unwrap();
        let gui = gui
            .iter()
            .find(|property| property.side_entries.iter().any(|file| file == name))
            .unwrap();
        assert_eq!(
            entry
                .referenced_by()
                .iter()
                .collect::<std::collections::BTreeSet<_>>(),
            [&app.id, &gui.id].into_iter().collect()
        );
        let stream = ctx
            .capture_part_extent(archive.open(&ctx, name).unwrap())
            .unwrap();
        let parts = result
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| part.stream() == stream.stream())
            .collect::<Vec<_>>();
        let expected: &[(&str, usize)] = if name == "Cloud" {
            &[
                ("freecad.point_kernel_entry", 1),
                ("freecad.point_kernel_point", 2),
            ]
        } else {
            &[
                ("freecad.mesh_kernel_entry", 1),
                ("freecad.mesh_kernel_header", 1),
                ("freecad.mesh_point", 3),
                ("freecad.mesh_facet", 1),
                ("freecad.mesh_bounding_box", 1),
            ]
        };
        for (level, count) in expected {
            assert_eq!(
                parts
                    .iter()
                    .filter(|part| part.level().as_str() == *level)
                    .count(),
                *count,
                "{name} {level}"
            );
        }
    }
    assert!(result
        .source_fidelity()
        .parts()
        .values()
        .all(|part| part.state() != &(PartState::GivenUp {})));
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    crate::fidelity::b1::assert_classified_spans(&result);
}

#[test]
fn a_point_member_cut_keeps_known_arity_and_the_complete_point_without_future_parts() {
    let original = source();
    let before = FcstdCodec
        .decode(&mut Cursor::new(&original), &DecodeOptions::default())
        .unwrap();
    let bytes = source_with_cuts(None, Some(21));
    let after = FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let all_entries = entries(&after);
    let cloud = all_entries
        .iter()
        .find(|entry| entry.name() == "Cloud")
        .unwrap();
    let points = cloud
        .contents()
        .iter()
        .find_map(|contents| match contents {
            EntryContents::Points { points } => Some(points),
            _ => None,
        })
        .unwrap();
    assert_eq!(points.len(), 2);
    assert_eq!(
        points[0].unwrap().get(),
        cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
    );
    assert!(points[1].is_none());
    assert_eq!(after.ir().model.points.len(), 1);
    assert_eq!(after.ir().model.points[0], before.ir().model.points[0]);
    assert_eq!(
        after.ir().model.tessellations,
        before.ir().model.tessellations
    );
    let failed = after
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &(PartState::GivenUp {}))
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].level().as_str(), "freecad.point_kernel_point");
    assert_eq!((failed[0].offset(), failed[0].byte_len()), (16, 5));
    assert_eq!(
        after
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| part.level().as_str() == "freecad.point_kernel_point")
            .count(),
        2
    );
    super::cuts::compare(&before, &after, &original, &bytes);
}
