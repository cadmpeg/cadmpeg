// SPDX-License-Identifier: Apache-2.0
//! Dependency worklists and source-graph ownership.

use super::super::*;
use crate::test_support::test_b5::{b5_linear_pcurve_payload, b5_object_ref, b5_plane_payload};

fn record(object_id: u32, class: u8, payload: Vec<u8>) -> B5RecordBuf {
    B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class,
        object_id,
        payload,
    }
}

fn supported(object_id: u32, carrier: u32, supports: [u32; 2], pcurves: [u32; 2]) -> B5RecordBuf {
    let mut payload = vec![0x85];
    for id in [carrier, supports[0], supports[1], pcurves[0], pcurves[1]] {
        payload.extend(b5_object_ref(id));
    }
    payload.extend([0x09, 0x01, 0x01, 0x05, 0x05, 0x0d]);
    for scalar in [101.6_f64, 20.0] {
        payload.extend(scalar.to_le_bytes());
    }
    record(object_id, 0x3b, payload)
}

fn alias(object_id: u32, terminal: u32) -> B5RecordBuf {
    let mut payload = vec![0x81];
    payload.extend(b5_object_ref(terminal));
    record(object_id, 0x2e, payload)
}

// These fixtures create at most twelve ordered entries per input record.
// Each insertion admits up to three node passes. At most four candidate
// visits per record add queue insertions/removals; 64 passes per binary
// height bound cover their node work. The final term covers the fixed-size
// payload comparisons, copies, hash indexes, and record traversals.
fn session_work_ceiling(record_count: usize) -> u64 {
    let pointer = std::mem::size_of::<usize>();
    let surface_node =
        11 * (std::mem::size_of::<u32>() + std::mem::size_of::<B5Surface>()) + 18 * pointer;
    let queue_node = 11 * std::mem::size_of::<(u8, usize)>() + 18 * pointer;
    let height = usize::try_from(usize::BITS - record_count.leading_zeros())
        .expect("binary height fits usize");
    u64_from_index(record_count * (36 * surface_node + 64 * height * queue_node + 4_096))
}

#[test]
fn b5_supported_worklist_resolves_reverse_chains_and_late_aliases_in_linear_visits() {
    const COUNT: u32 = 1_024;
    const ALIASES: u32 = 256;
    let plane = COUNT + 1;
    let pcurves = [COUNT + 2, COUNT + 3];
    let mut owned: Vec<_> = (0..ALIASES)
        .map(|index| alias(COUNT + 4 + index, 1))
        .collect();
    owned.extend((1..=COUNT).map(|id| supported(id, id + 1, [plane; 2], pcurves)));
    owned.push(record(plane, 0x27, b5_plane_payload([0.0; 3])));
    for id in pcurves {
        owned.push(record(
            id,
            0x21,
            b5_linear_pcurve_payload(
                u16::try_from(plane).expect("small plane identity"),
                [0.0, 0.0],
                [1.0, 0.0],
            ),
        ));
    }
    let cycle = COUNT + ALIASES + 4;
    owned.extend([alias(cycle, cycle + 1), alias(cycle + 1, cycle)]);
    for preload_opaque in [false, true] {
        if preload_opaque {
            for id in 1..=COUNT {
                let mut payload = vec![0x81];
                payload.extend(b5_object_ref(id));
                owned.push(record(cycle + 2 + id, 0x5f, payload));
            }
        }
        let records: Vec<_> = owned.iter().map(B5RecordBuf::record).collect();
        // Initial visits, dependency wakeups, and memoized alias walks are linear.
        let ceiling = usize::try_from(COUNT + ALIASES).expect("small local count") * 4;
        let budget = WorkBudget::new(ceiling);
        crate::test_support::with_work_limit(session_work_ceiling(records.len()), |ctx| {
            let graph = parse_from_records_budgeted(
                ctx,
                &[],
                &records,
                &[],
                false,
                Some(&budget),
                &mut crate::nurbs::LaneRefusals::new(),
            )?
            .expect("reverse dependency chain fits a linear local ceiling");
            assert!(!budget.exhausted());
            assert_eq!(
                graph.supported_surfaces.len(),
                usize::try_from(COUNT).expect("small count")
            );
            let expected = graph.surfaces.get(&plane).expect("plane carrier");
            for id in 1..=COUNT {
                assert_eq!(graph.surfaces.get(&id), Some(expected));
                assert_eq!(graph.supported_surfaces[&id].carrier_surface, id + 1);
                assert_eq!(graph.supported_surfaces[&id].support_pcurves, pcurves);
            }
            for index in 0..ALIASES {
                assert_eq!(graph.surfaces.get(&(COUNT + 4 + index)), Some(expected));
            }
            assert!(!graph.surfaces.contains_key(&cycle));
            assert!(!graph.surfaces.contains_key(&(cycle + 1)));
            Ok::<_, CodecError>(())
        })
        .expect("indexed work fits the core node-cost ceiling");
    }
}

#[test]
fn b5_source_graph_storage_stays_scoped_through_topology_readers() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream_with_native_vertex_chain();
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..3 {
            let mut storage = ctx
                .reserve_scoped(0, "test_source_graph_storage")
                .expect("graph owner");
            let graph = storage
                .with_storage(|| parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new()))
                .expect("source graph is scratch")
                .expect("closed triangle");
            assert!(graph.complete);
            assert_eq!(graph.faces.len(), 1);
            assert_eq!(graph.pcurves.len(), 3);
            assert_eq!(graph.loops.len(), 1);
            let referenced = storage
                .with_storage(|| graph.referenced_edge_vertex_references(ctx))
                .expect("topology lookup")
                .expect("closed topology");
            assert_eq!(referenced.len(), 3);
        }
    });
}

#[test]
fn b5_extrusion_worklist_retries_only_context_dependents() {
    const UNRELATED: u32 = 512;
    let mut directrix = vec![0x81];
    directrix.extend(b5_object_ref(3));
    directrix.extend([0x81, 0x01]);
    for scalar in [-3.0_f64, 4.0, 0.0] {
        directrix.extend(scalar.to_le_bytes());
    }
    directrix.push(0x01);
    let mut offset_directrix = vec![0x81];
    offset_directrix.extend(b5_object_ref(2));
    for scalar in [-3.0_f64, 4.0] {
        offset_directrix.extend(scalar.to_le_bytes());
    }
    offset_directrix.push(0x05);
    for scalar in [-1.5_f64, 0.0, 0.0, 1.0, -5.0, 6.0] {
        offset_directrix.extend(scalar.to_le_bytes());
    }
    let carrier = |id, directrix_id, bounds: [f64; 4], controls: [u8; 2]| {
        let mut payload = vec![0x81];
        payload.extend(b5_object_ref(directrix_id));
        for scalar in [
            0.0_f64, 0.0, 1.0, bounds[0], bounds[1], 1.0, 0.0, bounds[2], bounds[3],
        ] {
            payload.extend(scalar.to_le_bytes());
        }
        payload.extend(controls);
        record(id, 0x2c, payload)
    };
    let mut offset = vec![0x82];
    offset.extend(b5_object_ref(8));
    offset.extend(b5_object_ref(10));
    offset.extend((-1.5_f64).to_le_bytes());
    offset.push(0x21);
    for scalar in [-5.0_f64, 6.0, 2.0, 9.0] {
        offset.extend(scalar.to_le_bytes());
    }
    let mut pcurve = vec![0x81];
    pcurve.extend(b5_object_ref(7));
    pcurve.push(0x05);
    for scalar in [0.0_f64, -3.0, 4.0] {
        pcurve.extend(scalar.to_le_bytes());
    }
    let mut owned = vec![carrier(8, 4, [2.0, 9.0, 35.0, 7.0], [0x01, 0x09])];
    owned.extend((0..UNRELATED).map(|index| record(1_000 + index, 0x2c, vec![0x80])));
    owned.extend([
        carrier(10, 2, [0.0, 1.0, -3.0, 4.0], [0x05, 0x05]),
        record(2, 0x24, directrix),
        record(4, 0x14, offset_directrix),
        record(3, 0x18, pcurve),
        record(7, 0x27, b5_plane_payload([0.0; 3])),
        record(11, 0x30, offset),
    ]);
    let records: Vec<_> = owned.iter().map(B5RecordBuf::record).collect();
    // Each initial extrusion, one context retry, and one offset candidate.
    let budget = WorkBudget::new(usize::try_from(UNRELATED).expect("small local count") + 8);
    crate::test_support::with_service_context(|ctx| {
        let graph = parse_from_records_budgeted(
            ctx,
            &[],
            &records,
            &[],
            false,
            Some(&budget),
            &mut crate::nurbs::LaneRefusals::new(),
        )?
        .expect("unrelated candidates do not replay");
        assert!(!budget.exhausted());
        assert_eq!(graph.extrusion_surfaces.len(), 2);
        assert_eq!(
            graph.extrusion_surfaces[&8]
                .parameter_bounds
                .map(IncreasingParameterInterval::endpoints),
            [[2.0, 9.0], [-5.0, 6.0]]
        );
        assert_eq!(
            graph.extrusion_surfaces[&10]
                .parameter_bounds
                .map(IncreasingParameterInterval::endpoints),
            [[0.0, 1.0], [-3.0, 4.0]]
        );
        assert!(matches!(
            graph.extrusion_surfaces[&8].directrix,
            B5ExtrusionDirectrix::Offset { object_id: 4, .. }
        ));
        assert_eq!(graph.offset_surfaces[&11].carrier_surface, 8);
        Ok::<_, CodecError>(())
    })
    .expect("indexed extrusion resolution");
}

#[test]
fn b5_surface_worklist_keeps_stable_opaque_cycle_merges() {
    let plane = 3;
    let pcurves = [4, 5];
    let mut owned = vec![
        supported(1, 2, [plane; 2], pcurves),
        supported(2, 1, [plane; 2], pcurves),
        record(plane, 0x27, b5_plane_payload([0.0; 3])),
    ];
    for id in pcurves {
        owned.push(record(
            id,
            0x21,
            b5_linear_pcurve_payload(3, [0.0, 0.0], [1.0, 0.0]),
        ));
    }
    for id in [1, 2] {
        let mut payload = vec![0x81];
        payload.extend(b5_object_ref(id));
        owned.push(record(id + 5, 0x5f, payload));
    }
    let records: Vec<_> = owned.iter().map(B5RecordBuf::record).collect();
    let budget = WorkBudget::new(8);
    crate::test_support::with_service_context(|ctx| {
        let graph = parse_from_records_budgeted(
            ctx,
            &[],
            &records,
            &[],
            false,
            Some(&budget),
            &mut crate::nurbs::LaneRefusals::new(),
        )?
        .expect("stable opaque cycle");
        let expected = B5Surface::Unknown {
            family: 0xb5,
            class: 0x3b,
            payload: owned[1].payload.clone(),
        };
        assert_eq!(graph.surfaces.get(&1), Some(&expected));
        assert_eq!(graph.surfaces.get(&2), Some(&expected));
        assert!(graph.supported_surfaces.is_empty());
        assert!(!budget.exhausted());
        Ok::<_, CodecError>(())
    })
    .expect("source order selects the second opaque carrier");
}

#[test]
fn b5_surface_merge_reports_only_stored_changes_and_keeps_conflict_tombstones() {
    let triangle = crate::test_support::test_b5::b5_closed_triangle_stream();
    let plane = records(&triangle)
        .into_iter()
        .find_map(|record| parse_surface(&record))
        .expect("plane");
    let mut surfaces = BTreeMap::from([
        (1, plane.clone()),
        (
            2,
            B5Surface::Unknown {
                family: 0xb5,
                class: 0x3b,
                payload: vec![0x81],
            },
        ),
    ]);
    crate::test_support::with_service_context(|ctx| {
        let mut storage = ctx.reserve_scoped(0, "test_surface_conflicts")?;
        let mut conflicts = HashSet::new();
        assert_eq!(
            merge_surface_from(ctx, &mut surfaces, (&mut conflicts, &mut storage), 1, 2)?,
            Some(false)
        );
        assert_eq!(surfaces.get(&1), Some(&plane));
        let mut conflicting = plane.clone();
        let B5Surface::Plane { origin, .. } = &mut conflicting else {
            panic!("plane fixture")
        };
        *origin = crate::test_support::test_b5::point([1.0, 0.0, 0.0]);
        surfaces.insert(2, conflicting);
        assert_eq!(
            merge_surface_from(ctx, &mut surfaces, (&mut conflicts, &mut storage), 1, 2)?,
            None
        );
        assert!(!surfaces.contains_key(&1));
        assert!(conflicts.contains(&1));
        surfaces.insert(2, plane.clone());
        assert_eq!(
            merge_surface_from(ctx, &mut surfaces, (&mut conflicts, &mut storage), 1, 2)?,
            None
        );
        assert!(!surfaces.contains_key(&1));
        Ok::<_, CodecError>(())
    })
    .expect("resolved carriers and conflict tombstones remain stable");
}

#[test]
fn b5_surface_worklist_seeds_an_opaque_cycle_once_before_incoming_chains() {
    const TAIL: u32 = 512;
    const CYCLE: u32 = 128;
    let count = TAIL + CYCLE;
    let plane = count + 1;
    let pcurves = [count + 2, count + 3];
    let mut owned: Vec<_> = (1..=count)
        .map(|id| {
            supported(
                id,
                if id == count { TAIL + 1 } else { id + 1 },
                [plane; 2],
                pcurves,
            )
        })
        .collect();
    owned.push(record(plane, 0x27, b5_plane_payload([0.0; 3])));
    for id in pcurves {
        owned.push(record(
            id,
            0x21,
            b5_linear_pcurve_payload(
                u16::try_from(plane).expect("small plane identity"),
                [0.0, 0.0],
                [1.0, 0.0],
            ),
        ));
    }
    for id in 1..=count {
        let mut payload = vec![0x81];
        payload.extend(b5_object_ref(id));
        owned.push(record(count + 3 + id, 0x5f, payload));
    }
    let expected = B5Surface::Unknown {
        family: 0xb5,
        class: 0x3b,
        // The first cycle construction reads the second cycle carrier.
        payload: owned[usize::try_from(TAIL + 1).expect("small source index")]
            .payload
            .clone(),
    };
    let records: Vec<_> = owned.iter().map(B5RecordBuf::record).collect();
    // Initial visits, one bounded cycle walk, and dependency wakeups are linear.
    let budget = WorkBudget::new(4 * usize::try_from(count).expect("small local count"));
    crate::test_support::with_work_limit(session_work_ceiling(records.len()), |ctx| {
        let graph = parse_from_records_budgeted(
            ctx,
            &[],
            &records,
            &[],
            false,
            Some(&budget),
            &mut crate::nurbs::LaneRefusals::new(),
        )?
        .expect("one cycle seed settles the incoming chain");
        for id in 1..=count {
            assert_eq!(graph.surfaces.get(&id), Some(&expected));
        }
        assert!(graph.supported_surfaces.is_empty());
        assert!(!budget.exhausted());
        Ok::<_, CodecError>(())
    })
    .expect("cycle and chain work fits the core node-cost ceiling");
}

#[test]
fn b5_surface_worklist_accepts_self_carriers_without_cycle_search() {
    let plane = 2;
    let pcurves = [3, 4];
    let mut owned = vec![
        supported(1, 1, [plane; 2], pcurves),
        record(plane, 0x27, b5_plane_payload([0.0; 3])),
    ];
    for id in pcurves {
        owned.push(record(
            id,
            0x21,
            b5_linear_pcurve_payload(2, [0.0, 0.0], [1.0, 0.0]),
        ));
    }
    let mut face = vec![0x81];
    face.extend(b5_object_ref(1));
    owned.push(record(5, 0x5f, face));
    let records: Vec<_> = owned.iter().map(B5RecordBuf::record).collect();
    let budget = WorkBudget::new(1);
    crate::test_support::with_service_context(|ctx| {
        let graph = parse_from_records_budgeted(
            ctx,
            &[],
            &records,
            &[],
            false,
            Some(&budget),
            &mut crate::nurbs::LaneRefusals::new(),
        )?
        .expect("self carrier needs only its initial candidate visit");
        assert_eq!(
            graph.surfaces.get(&1),
            Some(&B5Surface::Unknown {
                family: 0xb5,
                class: 0x3b,
                payload: owned[0].payload.clone(),
            })
        );
        assert!(graph.supported_surfaces.is_empty());
        assert!(!budget.exhausted());
        Ok::<_, CodecError>(())
    })
    .expect("self carrier is fixed work within the candidate visit");
}

#[test]
fn b5_extrusion_worklist_waits_for_all_context_sources_before_scanning_offsets() {
    const COUNT: u32 = 256;
    let mut directrix = vec![0x81];
    directrix.extend(b5_object_ref(3));
    directrix.extend([0x81, 0x01]);
    for scalar in [-3.0_f64, 4.0, 0.0] {
        directrix.extend(scalar.to_le_bytes());
    }
    directrix.push(0x01);
    let mut offset_directrix = vec![0x81];
    offset_directrix.extend(b5_object_ref(2));
    for scalar in [-3.0_f64, 4.0] {
        offset_directrix.extend(scalar.to_le_bytes());
    }
    offset_directrix.push(0x05);
    for scalar in [-1.5_f64, 0.0, 0.0, 1.0, -5.0, 6.0] {
        offset_directrix.extend(scalar.to_le_bytes());
    }
    let carrier = |id, directrix_id, bounds: [f64; 4], controls: [u8; 2]| {
        let mut payload = vec![0x81];
        payload.extend(b5_object_ref(directrix_id));
        for scalar in [
            0.0_f64, 0.0, 1.0, bounds[0], bounds[1], 1.0, 0.0, bounds[2], bounds[3],
        ] {
            payload.extend(scalar.to_le_bytes());
        }
        payload.extend(controls);
        record(id, 0x2c, payload)
    };
    let mut owned = vec![carrier(8, 4, [2.0, 9.0, 35.0, 7.0], [0x01, 0x09])];
    owned.extend(
        (0..COUNT).map(|index| carrier(100 + index, 2, [0.0, 1.0, -3.0, 4.0], [0x05, 0x05])),
    );
    let mut pcurve = vec![0x81];
    pcurve.extend(b5_object_ref(7));
    pcurve.push(0x05);
    for scalar in [0.0_f64, -3.0, 4.0] {
        pcurve.extend(scalar.to_le_bytes());
    }
    owned.extend([
        record(2, 0x24, directrix),
        record(4, 0x14, offset_directrix),
        record(3, 0x18, pcurve),
        record(7, 0x27, b5_plane_payload([0.0; 3])),
    ]);
    for index in 0..COUNT {
        let mut offset = vec![0x82];
        offset.extend(b5_object_ref(8));
        offset.extend(b5_object_ref(100 + index));
        offset.extend((-1.5_f64).to_le_bytes());
        offset.push(0x21);
        for scalar in [-5.0_f64, 6.0, 2.0, 9.0] {
            offset.extend(scalar.to_le_bytes());
        }
        owned.push(record(1_000 + index, 0x30, offset));
    }
    let records: Vec<_> = owned.iter().map(B5RecordBuf::record).collect();
    // Initial extrusions, one ready-context retry, initial offsets, and one
    // carrier wakeup consume at most 2*COUNT+4 candidate visits.
    let budget = WorkBudget::new(2 * usize::try_from(COUNT).expect("small local count") + 4);
    crate::test_support::with_work_limit(session_work_ceiling(records.len()), |ctx| {
        let graph = parse_from_records_budgeted(
            ctx,
            &[],
            &records,
            &[],
            false,
            Some(&budget),
            &mut crate::nurbs::LaneRefusals::new(),
        )?
        .expect("context waits for every source before scanning its offsets");
        assert_eq!(
            graph.extrusion_surfaces.len(),
            usize::try_from(COUNT + 1).expect("small count")
        );
        assert_eq!(
            graph.offset_surfaces.len(),
            usize::try_from(COUNT).expect("small count")
        );
        assert_eq!(
            graph.extrusion_surfaces[&8]
                .parameter_bounds
                .map(IncreasingParameterInterval::endpoints),
            [[2.0, 9.0], [-5.0, 6.0]]
        );
        assert!(!budget.exhausted());
        Ok::<_, CodecError>(())
    })
    .expect("one complete context scan fits the core node-cost ceiling");
}
