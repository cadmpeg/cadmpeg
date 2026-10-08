// SPDX-License-Identifier: Apache-2.0
//! Linear B5 identity indexes and scoped selection storage.

use super::super::*;

#[test]
fn b5_alias_terminal_index_shares_long_suffixes_and_keeps_cycles() {
    let count = 4_096_u32;
    let alias = |id, target| {
        let mut payload = vec![0x81];
        payload.extend(crate::test_support::test_b5::b5_object_ref(target));
        B5RecordBuf {
            offset: 0,
            family: 0xb5,
            class: 0x2e,
            object_id: id,
            payload,
        }
    };
    let mut owned: Vec<_> = (1..=count).map(|id| alias(id, id + 1)).collect();
    owned.extend([alias(count + 10, count + 11), alias(count + 11, count + 10)]);
    let records: Vec<_> = owned.iter().map(B5RecordBuf::record).collect();
    let aliases: Vec<_> = records.iter().collect();
    let by_id: HashMap<_, _> = records
        .iter()
        .map(|record| (record.object_id, record))
        .collect();
    crate::test_support::with_work_limit(u64::from(count) * 1_000, |ctx| {
        let mut scratch = ctx.reserve_scoped(0, "test_alias_index")?;
        let terminals = surface_alias_terminals(ctx, &aliases, &by_id, None, &mut scratch)?
            .expect("no local ceiling");
        assert_eq!(terminals.get(&1), Some(&Some(count + 1)));
        assert_eq!(terminals.get(&count), Some(&Some(count + 1)));
        assert_eq!(terminals.get(&(count + 10)), Some(&None));
        assert_eq!(terminals.get(&(count + 11)), Some(&None));
        Ok::<_, CodecError>(())
    })
    .expect("linear terminal closure fits the work bound");
}

#[test]
fn isolated_geometry_queries_do_not_replay_topology_roots() {
    let count = 4_096;
    let mut bytes = Vec::new();
    let mut runs = Vec::new();
    for index in 0..count {
        let start = bytes.len();
        crate::test_support::test_b5::append_b5_record(
            &mut bytes,
            0x5f,
            u32::try_from(index).expect("small identity"),
            &[0],
        );
        runs.push(IndexedObjectRun {
            stream_index: 0,
            range: start..bytes.len(),
            frame_range: index..index + 1,
            topology: true,
        });
    }
    crate::test_support::with_work_limit(u64::try_from(count * 64).expect("small work"), |ctx| {
        let mut storage = ctx.reserve_scoped(0, "test_isolated_index")?;
        let index = index_isolated_geometry(ctx, &runs, |_| &bytes, &mut storage)?;
        for selected in 0..count {
            let mut query_storage = ctx.reserve_scoped(0, "test_isolated_query")?;
            assert!(
                isolated_geometry_runs(ctx, &runs, selected, &[], &index, &mut query_storage)?
                    .is_empty()
            );
        }
        Ok::<_, CodecError>(())
    })
    .expect("one global scan and constant empty queries");
}

#[test]
fn isolated_geometry_index_preserves_first_equal_frame_and_conflicts() {
    let mut bytes = Vec::new();
    let mut runs = Vec::new();
    for (id, payload) in [
        (1, &[0_u8][..]),
        (1, &[0_u8][..]),
        (2, &[0_u8][..]),
        (2, &[1_u8][..]),
    ] {
        let start = bytes.len();
        crate::test_support::test_b5::append_b5_record(&mut bytes, 0x27, id, payload);
        runs.push(IndexedObjectRun {
            stream_index: 0,
            range: start..bytes.len(),
            frame_range: 0..1,
            topology: false,
        });
    }
    crate::test_support::with_retained_limit(0, |ctx| {
        let mut storage = ctx
            .reserve_scoped(0, "test_isolated_index")
            .expect("scoped reservation");
        let index =
            index_isolated_geometry(ctx, &runs, |_| &bytes, &mut storage).expect("scoped index");
        assert_eq!(index.get(&(0, 1)), Some(&Some(0)));
        assert_eq!(index.get(&(0, 2)), Some(&None));
    });
}

#[test]
fn selected_population_indexes_stay_scoped_through_record_readers() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    crate::test_support::with_retained_limit(0, |ctx| {
        for _ in 0..4 {
            let mut storage = ctx
                .reserve_scoped(0, "test_selection_storage")
                .expect("selection reservation");
            let selection = select_object_stream_population(
                ctx,
                std::slice::from_ref(&bytes),
                None,
                &mut storage,
            )
            .expect("selection is scratch");
            assert!(selection.selected());
            assert_eq!(selection.source(), bytes);
            assert!(!selection.records().is_empty());
            assert!(!selection.census_records().is_empty());
        }
    });
}

#[test]
fn alias_terminal_index_observes_late_carrier_construction() {
    let mut payload = vec![0x81];
    payload.extend(crate::test_support::test_b5::b5_object_ref(2));
    let owned = B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x2e,
        object_id: 1,
        payload,
    };
    let record = owned.record();
    let aliases = [&record];
    let by_id = HashMap::from([(1, &record)]);
    crate::test_support::with_service_context(|ctx| {
        let mut storage = ctx.reserve_scoped(0, "test_alias_index")?;
        let terminals = surface_alias_terminals(ctx, &aliases, &by_id, None, &mut storage)?
            .expect("no local ceiling");
        let mut surfaces = BTreeMap::new();
        assert_eq!(
            resolved_surface_alias_terminal(ctx, 1, &terminals, &surfaces)?,
            None
        );
        surfaces.insert(
            2,
            B5Surface::Plane {
                origin: crate::test_support::test_b5::point([0.0; 3]),
                frame: crate::test_support::test_b5::plane_frame([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                direction_v: crate::test_support::test_b5::exact_unit([0.0, 1.0, 0.0]),
                u_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
                v_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
            },
        );
        assert_eq!(
            resolved_surface_alias_terminal(ctx, 1, &terminals, &surfaces)?,
            Some(2)
        );
        Ok::<_, CodecError>(())
    })
    .expect("current carrier lookup with unchanged terminals");
}
