// SPDX-License-Identifier: Apache-2.0
use super::super::*;
use crate::wire::records::consolidated_records_in_sources;

fn frame(bytes: &mut Vec<u8>, class: u8, data: &[u8]) {
    bytes.extend([
        0xb2,
        3,
        class,
        u8::try_from(data.len()).expect("short frame"),
        5,
    ]);
    bytes.extend(data);
}
fn backward(distance: usize) -> u8 {
    u8::try_from(4 * distance + 1).expect("compact distance")
}
fn square() -> (Vec<u8>, usize, usize, usize) {
    let mut bytes = Vec::new();
    let mut plane = vec![0x84, 0];
    for value in [
        0.0_f64, 0.0, 2.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0,
    ] {
        plane.extend(value.to_le_bytes());
    }
    frame(&mut bytes, 0x27, &plane);
    for _ in 0..4 {
        frame(&mut bytes, 0x5d, &[1]);
    }
    let root = bytes.len();
    bytes.extend(super::governing_roots::group(3));
    let shell = bytes.len();
    frame(&mut bytes, 0x61, &[0x81, 3, 0, 0x13, 3]);
    let face = bytes.len();
    frame(&mut bytes, 0x5f, &[0x82, backward(7), 3, 3]);
    let mut owner = vec![0x89];
    for i in 0..4 {
        owner.extend([4 * (2 * i) + 3, 4 * (2 * i + 1) + 3]);
    }
    owner.extend([backward(8), 0x84, 0x41, 0, 0, 1]);
    frame(&mut bytes, 0x62, &owner);
    let corners = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    for i in 0..4 {
        let a = corners[i];
        let b = corners[(i + 1) % 4];
        let mut line = vec![5, 0];
        for value in [0.0_f64, 1.0, a[0], a[1], b[0] - a[0], b[1] - a[1]] {
            line.extend(value.to_le_bytes());
        }
        frame(&mut bytes, 0x18, &line);
        let edge = 10 + 2 * i;
        frame(
            &mut bytes,
            0x5e,
            &[
                5,
                backward(edge - (1 + i)),
                backward(edge - (1 + (i + 1) % 4)),
                1,
                1,
                1,
            ],
        );
    }
    (bytes, root, shell, face)
}
fn run(data: &[u8], sources: &[Vec<SourceExtent>], source: usize, selected: bool) -> Sheets {
    let records = consolidated_records_in_sources(data, sources).expect("source records");
    let selection = crate::container::body_selection::BodySelection {
        source,
        allocation_ends: vec![6],
    };
    crate::test_support::with_service_context(|ctx| {
        patches(
            ctx,
            &CadIr::empty(),
            &Stream {
                data,
                sources,
                records: &records,
                selection: selected.then_some(&selection),
            },
            &BTreeMap::new(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("sheet decode")
}
#[test]
fn governing_support_before_group_reaches_a_bounded_face_across_extents() {
    let (logical, root, shell, face) = square();
    for split in [1, root, shell + 2, face + 6, logical.len() - 1] {
        let mut data = logical[..split].to_vec();
        data.extend([0xff; 7]);
        data.extend(&logical[split..]);
        let sources = vec![vec![
            SourceExtent::within(&data, 0, split).expect("first extent"),
            SourceExtent::within(&data, split + 7, data.len()).expect("second extent"),
        ]];
        let sheets = run(&data, &sources, 0, true);
        assert_eq!(
            sheets.patches.len(),
            1,
            "split {split}, selected {}, members {:?}",
            sheets.selection.is_some(),
            sheets.members
        );
        let patch = &sheets.patches[0];
        let physical = |offset| offset + if offset >= split { 7 } else { 0 };
        assert_eq!(patch.face, physical(face));
        assert_eq!(patch.body, (0, physical(shell)));
        assert!(sheets.roots[&patch.body].contains(&physical(root)));
        assert_eq!(patch.sides.len(), 4);
        for side in &patch.sides {
            for (_, p) in side.vertices {
                assert_eq!(p.z, 2.0);
                assert!([0.0, 1.0].contains(&p.x));
                assert!([0.0, 1.0].contains(&p.y));
            }
        }
    }
    let sources = vec![vec![
        SourceExtent::within(&logical, 0, logical.len()).expect("source")
    ]];
    assert!(run(&logical, &sources, 0, false).patches.is_empty());
}
#[test]
fn governing_support_scope_stays_in_the_selected_source() {
    let (logical, _, _, face) = square();
    let mut data = vec![0xb2, 3, 0x27, 0, 5];
    let offset = data.len();
    data.extend(&logical);
    let sources = vec![
        vec![SourceExtent::within(&data, 0, offset).expect("other source")],
        vec![SourceExtent::within(&data, offset, data.len()).expect("selected source")],
    ];
    let sheets = run(&data, &sources, 1, true);
    assert_eq!(sheets.patches.len(), 1);
    assert_eq!(sheets.patches[0].body.0, 1);
    assert_eq!(sheets.patches[0].face, offset + face);
}
#[test]
fn governing_scope_rejects_mixed_disagreeing_anchor_directions() {
    let mut bytes = vec![0xb2, 3, 0x27, 0, 9, 0xb2, 0x13, 0x5b, 0, 5];
    let end = bytes.len();
    frame(&mut bytes, 0x61, &[0x82, 3, 7, 0, 0x33, 3]);
    frame(&mut bytes, 0x5f, &[0x82, backward(3), 3, 5]);
    frame(&mut bytes, 0x5f, &[0x82, 3, 3, 5]);
    frame(&mut bytes, 0x27, &[]);
    let local = Allocations::read(&bytes, end, Some(bytes.len()));
    let full = Allocations::read(&bytes, 0, Some(bytes.len()));
    let members = local.faces(&bytes, &[0], true).expect("two members");
    assert!(full
        .members_with_preceding_supports(&bytes, &local, end, &members)
        .is_none());
    let first = full
        .members_with_preceding_supports(&bytes, &local, end, &members[..1])
        .expect("preceding support");
    assert!(first.end_anchored);
    assert!(first.inferred);
}
#[test]
fn governing_roster_selects_members_without_claiming_unlisted_faces() {
    let mut bytes = vec![0xb2, 3, 0x61, 5, 5, 0x81, 3, 0, 0x13, 3];
    frame(&mut bytes, 0x5f, &[]);
    frame(&mut bytes, 0x5f, &[]);
    let alloc = Allocations::read(&bytes, 0, Some(bytes.len()));
    assert!(alloc.faces(&bytes, &[0], false).is_none());
    assert_eq!(alloc.faces(&bytes, &[0], true), Some(vec![(0, 1, true)]));
    bytes[6] = 11;
    assert!(Allocations::read(&bytes, 0, Some(bytes.len()))
        .faces(&bytes, &[0], true)
        .is_none());
}

#[test]
fn governing_support_loads_an_uncached_tensor_before_the_group() {
    let (bytes, _, _, _) = square();
    let first =
        crate::wire::records::parse_consolidated_record(&bytes, 0, bytes.len()).expect("plane");
    let mut data = crate::test_support::test_a5a8::a5_surface_stream();
    data[7] = 5;
    data.extend(&bytes[first.source_range().end..]);
    let sources = vec![vec![
        SourceExtent::within(&data, 0, data.len()).expect("source")
    ]];
    let sheets = run(&data, &sources, 0, true);
    assert_eq!(sheets.patches.len(), 1);
    let expected = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 1.0],
        [2.0, 1.0, 0.0],
        [3.0, 1.0, 1.0],
    ];
    for side in &sheets.patches[0].sides {
        for (_, point) in side.vertices {
            assert!(expected
                .iter()
                .any(|p| point.distance(Point3::from(*p)) < 1e-10));
        }
    }
}
