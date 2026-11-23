// SPDX-License-Identifier: Apache-2.0
use super::{MainLine, MainLineError, Replay, ReplayError, StateNode, TransmitError};
use crate::records::node_type::{BODY, GROUP, POINT};
use crate::records::{DeltasRecords, NodeFields, PrimaryRecords};
use crate::test_support::replay::{
    embedded_partition, partition, DeltasStream, Mark, TestLimit, TestNode,
};
use crate::test_support::with_decode_context;

const C: Mark = Mark {
    xmt: 100,
    preceding: 101,
    identifier: 9,
};
const B: Mark = Mark {
    xmt: 101,
    preceding: 102,
    identifier: 6,
};
const A: Mark = Mark {
    xmt: 102,
    preceding: 1,
    identifier: 3,
};

/// The state at the current mark C. B held P10 with its first image and P13;
/// the change from B to C rewrote P10, created P11, and deleted P13. The
/// change from the empty initial mark A to B created P10, G12, and P13.
fn current() -> [TestNode; 3] {
    [
        TestNode::point(10, 11, 3),
        TestNode::point(11, 1, 1),
        TestNode::group(12, 1),
    ]
}

fn delta_c(stream: DeltasStream) -> DeltasStream {
    stream.delta(
        &[TestNode::point(13, 1, 7)],
        &[(11, POINT)],
        &[TestNode::point(10, 1, 1)],
    )
}

fn delta_b(stream: DeltasStream) -> DeltasStream {
    stream.delta(&[], &[(10, POINT), (12, GROUP), (13, POINT)], &[])
}

/// The `SolidWorks` order: the current delta, then the main line down to the
/// initial mark, then the end of the stream.
fn solidworks() -> Vec<u8> {
    delta_b(delta_c(DeltasStream::new(C)).marks(&[B]))
        .marks(&[A])
        .terminator()
        .finish()
}

/// NX order: the current delta, then the main line up from the initial mark,
/// whose section also holds the next mark, then a final terminator.
fn nx() -> Vec<u8> {
    delta_b(delta_c(DeltasStream::new(C)).marks(&[A, B]))
        .terminator()
        .finish()
}

type Image = (u32, u16, u32, f64);

fn image(node: &impl NodeFields) -> Image {
    let value = match node.kind() {
        POINT => node.vector("pvec").unwrap()[0],
        _ => f64::from(node.byte("type").unwrap()),
    };
    (
        node.index(),
        node.kind(),
        node.pointer("next").unwrap(),
        value,
    )
}

fn state_image(node: &StateNode<'_>) -> Image {
    match node {
        StateNode::Partition(node) => image(node),
        StateNode::Delta(node) => image(node),
    }
}

fn images(replay: &Replay<'_, '_>) -> Vec<Image> {
    replay.state().nodes().map(state_image).collect()
}

/// The identifier and node images of every state, from the current mark.
fn walk(partition_bytes: &[u8], deltas: &[u8]) -> Vec<(u32, Vec<Image>)> {
    let records = DeltasRecords::read(deltas).unwrap();
    let line = MainLine::read(&records).unwrap();
    let partition = PrimaryRecords::read(partition_bytes).unwrap().unwrap();
    let mut replay = line.replay(&partition).unwrap();
    let mut out = vec![(replay.mark().identifier(), images(&replay))];
    assert_eq!(replay.state().dangling().count(), 0);
    while let Some(mark) = replay.step().unwrap() {
        assert_eq!(mark, replay.mark());
        assert_eq!(replay.state().dangling().count(), 0);
        out.push((mark.identifier(), images(&replay)));
    }
    assert!(replay.state().is_empty());
    out
}

fn expected_states() -> Vec<(u32, Vec<Image>)> {
    vec![
        (
            9,
            vec![
                (10, POINT, 11, 3.0),
                (11, POINT, 1, 1.0),
                (12, GROUP, 1, 0.0),
            ],
        ),
        (
            6,
            vec![
                (10, POINT, 1, 1.0),
                (12, GROUP, 1, 0.0),
                (13, POINT, 1, 7.0),
            ],
        ),
        (3, vec![]),
    ]
}

#[test]
fn backward_deltas_restore_older_images_and_remove_created_nodes() {
    with_decode_context(|_ctx| {
        assert_eq!(
            walk(&partition(&current()), &solidworks()),
            expected_states()
        );
    });
}

#[test]
fn both_section_orders_assign_each_delta_to_its_mark() {
    with_decode_context(|_ctx| {
        let partition = partition(&current());
        assert_eq!(walk(&partition, &nx()), walk(&partition, &solidworks()));
        for stream in [solidworks(), nx()] {
            let records = DeltasRecords::read(&stream).unwrap();
            let line = MainLine::read(&records).unwrap();
            let identifiers: Vec<_> = line.marks().iter().map(|mark| mark.identifier()).collect();
            assert_eq!(identifiers, [9, 6, 3]);
            assert_eq!(line.off_line(), 0);
            assert_eq!(line.deltas().len(), 2);
            assert_eq!(line.deltas()[0].mark().identifier(), 9);
            assert_eq!(line.deltas()[0].target().identifier(), 6);
            assert_eq!(line.deltas()[0].restored().len(), 1);
            assert_eq!(line.deltas()[0].older().len(), 1);
            assert_eq!(line.deltas()[1].created().len(), 3);
        }
    });
}

#[test]
fn restored_and_older_images_come_from_the_deltas_stream() {
    with_decode_context(|_ctx| {
        let deltas = solidworks();
        let records = DeltasRecords::read(&deltas).unwrap();
        let line = MainLine::read(&records).unwrap();
        let partition_bytes = partition(&current());
        let partition = PrimaryRecords::read(&partition_bytes).unwrap().unwrap();
        let mut replay = line.replay(&partition).unwrap();
        assert!(replay
            .state()
            .nodes()
            .all(|node| node.as_partition().is_some()));
        replay.step().unwrap();
        let state = replay.state();
        for (xmt, from_deltas) in [(10, true), (12, false), (13, true)] {
            let node = state.get(xmt).unwrap();
            assert_eq!(node.as_delta().is_some(), from_deltas, "node {xmt}");
            assert_eq!(matches!(node, StateNode::Partition(_)), !from_deltas);
        }
        assert_eq!(state.get(11).map(StateNode::kind), None);
        assert_eq!(state.count(POINT), 2);
        assert!(state.is_empty_part());
        assert_eq!(
            state.get(10).unwrap().pointer_targets(),
            Some(vec![1, 1, 1, 1])
        );
    });
}

#[test]
fn a_group_joins_the_delta_whose_change_creates_it() {
    with_decode_context(|_ctx| {
        let deltas = nx();
        let records = DeltasRecords::read(&deltas).unwrap();
        let line = MainLine::read(&records).unwrap();
        assert_eq!(line.deltas()[0].created_groups().count(), 0);
        assert_eq!(line.deltas()[1].created_groups().collect::<Vec<_>>(), [12]);
        let creating: Vec<_> = line
            .deltas_creating(12)
            .map(|delta| (delta.mark().identifier(), delta.target().identifier()))
            .collect();
        assert_eq!(creating, [(6, 3)]);
        assert_eq!(line.deltas_creating(99).count(), 0);
    });
}

#[test]
fn one_delta_can_create_the_nodes_of_several_changes() {
    let merged = DeltasStream::new(Mark {
        preceding: A.xmt,
        ..C
    })
    .delta(&[], &[(10, POINT), (11, POINT), (12, GROUP)], &[])
    .marks(&[A])
    .terminator()
    .finish();
    with_decode_context(|_ctx| {
        let states = walk(&partition(&current()), &merged);
        assert_eq!(states.len(), 2);
        assert_eq!(states[0], expected_states()[0]);
        assert_eq!(states[1], (3, vec![]));
        let records = DeltasRecords::read(&merged).unwrap();
        let line = MainLine::read(&records).unwrap();
        assert_eq!(line.deltas()[0].created_groups().collect::<Vec<_>>(), [12]);
    });
}

#[test]
fn a_zero_length_delta_keeps_the_state() {
    let d = Mark {
        xmt: 103,
        preceding: C.xmt,
        identifier: 12,
    };
    let stream =
        delta_b(delta_c(DeltasStream::new(d).delta(&[], &[], &[]).marks(&[C])).marks(&[B]))
            .marks(&[A])
            .terminator()
            .finish();
    with_decode_context(|_ctx| {
        let states = walk(&partition(&current()), &stream);
        let mut expected = expected_states();
        expected.insert(0, (12, expected[0].1.clone()));
        assert_eq!(states, expected);
        let records = DeltasRecords::read(&stream).unwrap();
        assert!(MainLine::read(&records).unwrap().deltas()[0].is_empty());
    });
}

#[test]
fn a_stream_at_its_initial_mark_has_one_state() {
    let stream = DeltasStream::new(A)
        .delta(&[], &[], &[])
        .terminator()
        .finish();
    with_decode_context(|_ctx| {
        let records = DeltasRecords::read(&stream).unwrap();
        let line = MainLine::read(&records).unwrap();
        assert_eq!(line.marks().len(), 1);
        assert!(line.deltas().is_empty());
        let partition_bytes = partition(&[]);
        let partition = PrimaryRecords::read(&partition_bytes).unwrap().unwrap();
        let mut replay = line.replay(&partition).unwrap();
        assert_eq!(replay.step(), Ok(None));
        assert_eq!(replay.mark().identifier(), 3);
    });
}

#[test]
fn marks_off_the_main_line_are_counted_and_not_applied() {
    let branch = Mark {
        xmt: 104,
        preceding: B.xmt,
        identifier: 7,
    };
    let stream = delta_b(delta_c(DeltasStream::new(C)).marks(&[B]))
        .marks(&[branch])
        .delta(&[TestNode::point(40, 1, 0)], &[], &[])
        .marks(&[A])
        .terminator()
        .finish();
    with_decode_context(|_ctx| {
        assert_eq!(walk(&partition(&current()), &stream), expected_states());
        let records = DeltasRecords::read(&stream).unwrap();
        assert_eq!(MainLine::read(&records).unwrap().off_line(), 1);
    });
}

/// Restored images, created entries, older images, and the refused step.
type Case<'a> = (
    &'a [TestNode],
    &'a [(u32, u16)],
    &'a [TestNode],
    ReplayError,
);

/// B as the initial mark, for streams that end at B.
const INITIAL_B: Mark = Mark { preceding: 1, ..B };

fn first_step(stream: &[u8]) -> (Result<(), ReplayError>, usize, u32) {
    let records = DeltasRecords::read(stream).unwrap();
    let line = MainLine::read(&records).unwrap();
    let partition_bytes = partition(&current());
    let partition = PrimaryRecords::read(&partition_bytes).unwrap().unwrap();
    let mut replay = line.replay(&partition).unwrap();
    let result = replay.step().map(|_| ());
    (result, replay.state().len(), replay.mark().identifier())
}

#[test]
fn an_inconsistent_delta_is_refused_and_the_state_stays_at_its_mark() {
    let cases: [Case<'_>; 10] = [
        (
            &[],
            &[(14, POINT)],
            &[],
            ReplayError::RemovedAbsent { mark: 9, xmt: 14 },
        ),
        (
            &[TestNode::point(14, 1, 0), TestNode::point(14, 1, 1)],
            &[],
            &[],
            ReplayError::RestoredPresent { mark: 9, xmt: 14 },
        ),
        (
            &[],
            &[(11, POINT), (11, POINT)],
            &[],
            ReplayError::RemovedAbsent { mark: 9, xmt: 11 },
        ),
        (
            &[],
            &[(12, POINT)],
            &[],
            ReplayError::RemovedKind {
                mark: 9,
                xmt: 12,
                expected: POINT,
                found: GROUP,
            },
        ),
        (
            &[TestNode::point(10, 1, 0)],
            &[],
            &[],
            ReplayError::RestoredPresent { mark: 9, xmt: 10 },
        ),
        (
            &[TestNode::group(11, 1)],
            &[(11, POINT)],
            &[],
            ReplayError::RestoredPresent { mark: 9, xmt: 11 },
        ),
        (
            &[],
            &[(11, POINT)],
            &[TestNode::point(11, 1, 0)],
            ReplayError::OlderAbsent { mark: 9, xmt: 11 },
        ),
        (
            &[TestNode::point(14, 1, 0)],
            &[],
            &[TestNode::point(14, 1, 1)],
            ReplayError::OlderAbsent { mark: 9, xmt: 14 },
        ),
        (
            &[],
            &[],
            &[TestNode::point(10, 1, 1), TestNode::point(10, 1, 2)],
            ReplayError::OlderRepeated { mark: 9, xmt: 10 },
        ),
        (
            &[],
            &[],
            &[TestNode::group(10, 1)],
            ReplayError::OlderKind {
                mark: 9,
                xmt: 10,
                expected: GROUP,
                found: POINT,
            },
        ),
    ];
    with_decode_context(|_ctx| {
        for (restored, created, older, error) in cases {
            let stream = DeltasStream::new(C)
                .delta(restored, created, older)
                .marks(&[INITIAL_B])
                .terminator()
                .finish();
            assert_eq!(first_step(&stream), (Err(error), 3, 9), "{error}");
        }
    });
}

fn main_line_error(stream: &[u8]) -> MainLineError {
    let records = DeltasRecords::read(stream).unwrap();
    MainLine::read(&records).unwrap_err()
}

#[test]
fn a_broken_main_line_is_a_typed_error() {
    let missing = delta_c(DeltasStream::new(C)).terminator().finish();
    let duplicate = delta_b(delta_c(DeltasStream::new(C)).marks(&[B]))
        .marks(&[B])
        .terminator()
        .finish();
    let cycle = delta_c(DeltasStream::new(C))
        .marks(&[Mark {
            preceding: C.xmt,
            ..B
        }])
        .delta(&[], &[], &[])
        .terminator()
        .finish();
    let mut incomplete = delta_c(DeltasStream::new(C))
        .marks(&[B])
        .terminator()
        .finish();
    incomplete.push(0);
    let initial = delta_b(delta_c(DeltasStream::new(C)).marks(&[B]))
        .marks(&[A])
        .nodes(&[TestNode::point(20, 1, 0)])
        .terminator()
        .finish();
    let forward = delta_b(delta_c(DeltasStream::new(C)).forward_marks(&[B]))
        .marks(&[A])
        .terminator()
        .finish();
    let cut_downward = delta_c(DeltasStream::new(C))
        .marks(&[B])
        .terminator()
        .finish();
    let cut_upward = delta_c(DeltasStream::new(C))
        .marks(&[A, B])
        .terminator()
        .finish();
    with_decode_context(|_ctx| {
        assert_eq!(
            main_line_error(&missing),
            MainLineError::MissingMark { xmt: 101 }
        );
        assert_eq!(
            main_line_error(&duplicate),
            MainLineError::DuplicateMark { xmt: 101 }
        );
        assert_eq!(main_line_error(&cycle), MainLineError::Cycle { xmt: 100 });
        assert_eq!(
            main_line_error(&incomplete),
            MainLineError::IncompleteDelta { identifier: 6 }
        );
        assert_eq!(
            main_line_error(&initial),
            MainLineError::InitialDelta { identifier: 3 }
        );
        assert_eq!(
            main_line_error(&forward),
            MainLineError::ForwardDelta { identifier: 6 }
        );
        for cut in [cut_downward, cut_upward] {
            assert_eq!(
                main_line_error(&cut),
                MainLineError::IncompleteDelta { identifier: 6 }
            );
        }
        assert_eq!(main_line_error(b"XX"), MainLineError::MissingPrefix);
    });
}

#[test]
fn a_partition_that_does_not_frame_whole_starts_no_replay() {
    let mut truncated = partition(&current());
    truncated.truncate(truncated.len() - 4);
    let duplicate = partition(&[TestNode::point(10, 1, 0), TestNode::point(10, 1, 1)]);
    with_decode_context(|_ctx| {
        let stream = solidworks();
        let records = DeltasRecords::read(&stream).unwrap();
        let line = MainLine::read(&records).unwrap();
        for (bytes, error) in [
            (truncated, ReplayError::PartitionIncomplete),
            (duplicate, ReplayError::DuplicateNode { xmt: 10 }),
        ] {
            let partition = PrimaryRecords::read(&bytes).unwrap().unwrap();
            assert_eq!(line.replay(&partition).unwrap_err(), error);
        }
    });
}

/// The states from the current mark to the initial mark, or the first
/// refusal.
fn replayed(partition: &PrimaryRecords<'_>, deltas: &[u8]) -> Option<Vec<(u32, Vec<Image>)>> {
    let records = DeltasRecords::read(deltas).ok()?;
    let line = MainLine::read(&records).ok()?;
    let mut replay = line.replay(partition).ok()?;
    let mut out = vec![(replay.mark().identifier(), images(&replay))];
    while let Some(mark) = replay.step().ok()? {
        out.push((mark.identifier(), images(&replay)));
    }
    Some(out)
}

#[test]
fn a_truncated_stream_gives_its_true_states_or_a_typed_error() {
    let partition_bytes = partition(&current());
    with_decode_context(|_ctx| {
        let partition = PrimaryRecords::read(&partition_bytes).unwrap().unwrap();
        for stream in [solidworks(), nx()] {
            for end in 0..stream.len() {
                if let Some(states) = replayed(&partition, &stream[..end]) {
                    assert_eq!(states, expected_states(), "cut at {end}");
                }
            }
        }
    });
}

#[test]
fn a_corrupted_stream_ends_in_a_state_or_a_typed_error() {
    let partition_bytes = partition(&current());
    with_decode_context(|_ctx| {
        let partition = PrimaryRecords::read(&partition_bytes).unwrap().unwrap();
        for stream in [solidworks(), nx()] {
            for at in 0..stream.len() {
                for flip in [0x01, 0x80, 0xff] {
                    let mut bytes = stream.clone();
                    bytes[at] ^= flip;
                    let Ok(records) = DeltasRecords::read(&bytes) else {
                        continue;
                    };
                    let Ok(line) = MainLine::read(&records) else {
                        continue;
                    };
                    let Ok(mut replay) = line.replay(&partition) else {
                        continue;
                    };
                    while let Ok(Some(_)) = replay.step() {}
                    replay.state().dangling().for_each(drop);
                }
            }
        }
    });
}

/// The nodes a written partition frames, in node-index order.
fn written_images(bytes: &[u8]) -> Vec<Image> {
    let records = PrimaryRecords::read(bytes).unwrap().unwrap();
    assert!(records.is_terminated());
    let mut out: Vec<Image> = records.nodes().map(|node| image(&node)).collect();
    out.sort_by_key(|image| image.0);
    out
}

#[test]
fn every_state_writes_as_a_partition_that_reads_back_to_its_nodes() {
    let partition_bytes = partition(&current());
    with_decode_context(|_ctx| {
        for stream in [solidworks(), nx()] {
            let records = DeltasRecords::read(&stream).unwrap();
            let line = MainLine::read(&records).unwrap();
            let partition = PrimaryRecords::read(&partition_bytes).unwrap().unwrap();
            let mut replay = line.replay(&partition).unwrap();
            assert_eq!(replay.transmit().unwrap(), partition_bytes);
            while replay.step().unwrap().is_some() {
                assert_eq!(written_images(&replay.transmit().unwrap()), images(&replay));
            }
        }
    });
}

#[test]
fn a_restored_hvec_image_writes_the_rows_a_partition_stores() {
    let rows = vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
    let stream = DeltasStream::new(C)
        .limits(&[TestLimit {
            xmt: 14,
            rows: rows.clone(),
        }])
        .terminator()
        .map(&[(11, POINT)])
        .nodes(&[TestNode::point(10, 1, 1)])
        .terminator()
        .marks(&[INITIAL_B])
        .terminator()
        .finish();
    let partition_bytes = partition(&current());
    with_decode_context(|_ctx| {
        let records = DeltasRecords::read(&stream).unwrap();
        let line = MainLine::read(&records).unwrap();
        let partition = PrimaryRecords::read(&partition_bytes).unwrap().unwrap();
        let mut replay = line.replay(&partition).unwrap();
        replay.step().unwrap();
        let state = replay.state().get(14).unwrap().as_delta().unwrap();
        assert_eq!(state.hvec_points("hvec"), Some(rows.clone()));
        let bytes = replay.transmit().unwrap();
        let written = PrimaryRecords::read(&bytes).unwrap().unwrap();
        let limit = written.nodes().find(|node| node.index() == 14).unwrap();
        assert_eq!(limit.hvec_points("hvec"), Some(rows));
    });
}

#[test]
fn an_embedded_schema_state_declares_each_type_at_its_first_written_node() {
    let [p10, p11, g12] = current();
    let partition_bytes = embedded_partition(&[g12, p11, p10]);
    let deltas = delta_b(delta_c(DeltasStream::embedded(C)).marks(&[B]))
        .marks(&[A])
        .terminator()
        .finish();
    with_decode_context(|_ctx| {
        let records = DeltasRecords::read(&deltas).unwrap();
        let line = MainLine::read(&records).unwrap();
        let partition = PrimaryRecords::read(&partition_bytes).unwrap().unwrap();
        let mut replay = line.replay(&partition).unwrap();
        assert_eq!(replay.transmit().unwrap(), partition_bytes);
        let mut states = vec![(replay.mark().identifier(), images(&replay))];
        while let Some(mark) = replay.step().unwrap() {
            assert_eq!(written_images(&replay.transmit().unwrap()), images(&replay));
            states.push((mark.identifier(), images(&replay)));
        }
        assert_eq!(states, expected_states());
    });
}

#[test]
fn a_pointer_to_a_removed_node_dangles() {
    let stream = DeltasStream::new(C)
        .delta(&[], &[(11, POINT)], &[])
        .marks(&[INITIAL_B])
        .terminator()
        .finish();
    let partition_bytes = partition(&current());
    with_decode_context(|_ctx| {
        let records = DeltasRecords::read(&stream).unwrap();
        let line = MainLine::read(&records).unwrap();
        let partition = PrimaryRecords::read(&partition_bytes).unwrap().unwrap();
        let mut replay = line.replay(&partition).unwrap();
        assert_eq!(replay.state().dangling().count(), 0);
        replay.step().unwrap();
        assert_eq!(replay.state().dangling().collect::<Vec<_>>(), [(10, 11)]);
    });
}

#[test]
fn a_state_that_holds_a_body_is_not_an_empty_part() {
    let partition_bytes =
        embedded_partition(&[TestNode::bare(BODY, 15), TestNode::point(10, 1, 0)]);
    let stream = DeltasStream::new(C)
        .delta(&[], &[(15, BODY)], &[])
        .marks(&[INITIAL_B])
        .terminator()
        .finish();
    with_decode_context(|_ctx| {
        let records = DeltasRecords::read(&stream).unwrap();
        let line = MainLine::read(&records).unwrap();
        let partition = PrimaryRecords::read(&partition_bytes).unwrap().unwrap();
        let mut replay = line.replay(&partition).unwrap();
        assert_eq!(replay.state().count(BODY), 1);
        assert!(!replay.state().is_empty_part());
        replay.step().unwrap();
        assert_eq!(replay.state().len(), 1);
        assert!(replay.state().is_empty_part());
    });
}

#[test]
fn a_state_that_does_not_write_as_a_partition_is_a_typed_error() {
    let mismatch = DeltasStream::embedded(C)
        .delta(&[TestNode::bare(POINT, 13)], &[], &[])
        .marks(&[INITIAL_B])
        .terminator()
        .finish();
    let misframed = DeltasStream::embedded(C)
        .delta(&[TestNode::group(14, 1)], &[], &[])
        .marks(&[INITIAL_B])
        .terminator()
        .finish();
    let partition_bytes = partition(&[TestNode::point(10, 1, 0)]);
    with_decode_context(|_ctx| {
        let partition = PrimaryRecords::read(&partition_bytes).unwrap().unwrap();
        for (stream, error) in [
            (mismatch, TransmitError::SchemaMismatch { kind: POINT }),
            (misframed, TransmitError::Reframe),
        ] {
            let records = DeltasRecords::read(&stream).unwrap();
            let line = MainLine::read(&records).unwrap();
            let mut replay = line.replay(&partition).unwrap();
            assert_eq!(replay.transmit().unwrap(), partition_bytes);
            replay.step().unwrap();
            assert_eq!(replay.transmit(), Err(error));
        }
    });
}
