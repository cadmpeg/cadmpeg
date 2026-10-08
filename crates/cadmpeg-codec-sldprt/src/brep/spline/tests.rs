// SPDX-License-Identifier: Apache-2.0
//! Spline carrier parsing and admission.

    use super::{
        expanded_knots, scan_arrays, scan_curve_carriers, scan_surface_carriers,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};

    #[test]
    fn spline_source_tables_release_storage_when_no_wrapper_uses_them() {
        let bytes = crate::test_support::parasolid::f64_array(0x2d, 12, &[0.0, 1.0, 2.0]);
        for curve in [true, false] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = 4096;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut refusals = Vec::new();
            if curve {
                assert!(scan_curve_carriers(&ctx, &bytes, &mut refusals)
                    .unwrap()
                    .is_empty());
            } else {
                assert!(scan_surface_carriers(&ctx, &bytes, &mut refusals)
                    .unwrap()
                    .is_empty());
            }
            assert!(refusals.is_empty());
            let _storage = ctx
                .reserve_scoped(
                    policy.limits.max_materialized_bytes,
                    "test released spline source storage",
                )
                .expect("no source table storage remains charged");
        }
    }

    #[test]
    fn unused_spline_arrays_need_no_materialized_storage() {
        let bytes = crate::test_support::parasolid::f64_array(0x2d, 12, &[0.0, 1.0, 2.0]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut refusals = Vec::new();
        assert!(scan_curve_carriers(&ctx, &bytes, &mut refusals)
            .unwrap()
            .is_empty());
        assert!(scan_surface_carriers(&ctx, &bytes, &mut refusals)
            .unwrap()
            .is_empty());
        assert!(refusals.is_empty());
        ctx.finish_session().unwrap();
    }

    #[test]
    fn spline_wrappers_do_not_decode_unreferenced_scalar_arrays() {
        for curve in [true, false] {
            let mut bytes = if curve {
                crate::test_support::parasolid::nurbs_curve_carrier(170, 171)
            } else {
                crate::test_support::parasolid::nurbs_surface_carrier(180, 181, 10)
            };
            let operation = if curve {
                "collect Parasolid curve carriers"
            } else {
                "collect Parasolid surface carriers"
            };
            let refusal = |bytes: &[u8]| {
                let error = cadmpeg_test_support::refusal::resource_limit_at(
                    ResourceDimension::CollectionItems,
                    operation,
                    |cap| {
                        let arena = DecodeArena::new();
                        let mut policy = DecodePolicy::service();
                        policy.limits.max_collection_items = cap;
                        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                        let mut refusals = Vec::new();
                        if curve {
                            scan_curve_carriers(&ctx, bytes, &mut refusals).map(|out| out.len())
                        } else {
                            scan_surface_carriers(&ctx, bytes, &mut refusals).map(|out| out.len())
                        }
                    },
                );
                let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                    panic!("carrier insertion refusal");
                };
                limit
            };
            let original = refusal(&bytes);
            let mut prefix =
                crate::test_support::parasolid::f64_array(0x2d, 65000, &vec![0.25; 4096]);
            prefix.extend(bytes);
            bytes = prefix;
            let extended = refusal(&bytes);
            assert_eq!(extended.used, original.used);
            assert_eq!(extended.additional, original.additional);
        }
    }

    #[test]
    fn surface_pole_traversal_charges_rows_and_poles_once() {
        let valid = crate::test_support::parasolid::nurbs_surface_carrier(180, 181, 10);
        let control = crate::test_support::parasolid::f64_array(
            0x2d,
            182,
            &[0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.5],
        );
        let start = valid
            .windows(control.len())
            .position(|bytes| bytes == control)
            .unwrap();
        let mut invalid = valid.clone();
        let values_at = super::array_body(&invalid, start, 0x2d).unwrap()
            + crate::layout::bspline_array_header::LEN;
        invalid[values_at..values_at + 8].copy_from_slice(&f64::NAN.to_be_bytes());
        let mut zero_weight =
            crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10);
        let control_at = (0..zero_weight.len())
            .find_map(|off| {
                let body = super::array_body(&zero_weight, off, 0x2d)?;
                (cadmpeg_core::decode::View::u16_be_at(
                    &zero_weight,
                    body + crate::layout::bspline_array_header::ATTR,
                ) == Some(182))
                .then_some(body + crate::layout::bspline_array_header::LEN)
            })
            .unwrap();
        zero_weight[control_at + 24..control_at + 32].copy_from_slice(&0.0_f64.to_be_bytes());
        for bytes in [&valid, &zero_weight] {
            for operation in [
                "scan Parasolid surface poles",
                "scan Parasolid surface row poles",
            ] {
                let error = crate::test_support::work_refusal_at(operation, |ctx| {
                    scan_surface_carriers(ctx, bytes, &mut Vec::new())
                });
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.operation == operation && limit.additional == 1)
                );
            }
        }
        let ctx = cadmpeg_test_support::service_decode_context();
        assert!(scan_surface_carriers(&ctx, &invalid, &mut Vec::new())
            .unwrap()
            .is_empty());
        assert!(scan_surface_carriers(&ctx, &zero_weight, &mut Vec::new())
            .unwrap()
            .is_empty());
        assert!(scan_surface_carriers(&ctx, &valid, &mut Vec::new())
            .unwrap()
            .contains_key(&180));
    }

    #[test]
    fn rejected_spline_candidates_do_not_consume_retained_storage() {
        let mut curve = crate::test_support::parasolid::nurbs_curve_carrier(170, 171);
        let curve_control = crate::test_support::parasolid::f64_array(
            0x2d,
            172,
            &[0.0, 0.0, 0.0, 0.5, 1.0, 0.0, 1.0, 0.0, 0.0],
        );
        let curve_at = curve
            .windows(curve_control.len())
            .position(|window| window == curve_control)
            .unwrap();
        let curve_values_at = super::array_body(&curve, curve_at, 0x2d).unwrap()
            + crate::layout::bspline_array_header::LEN;
        curve[curve_values_at..curve_values_at + 8].copy_from_slice(&f64::NAN.to_be_bytes());

        let mut surface = crate::test_support::parasolid::nurbs_surface_carrier(180, 181, 10);
        let surface_control = crate::test_support::parasolid::f64_array(
            0x2d,
            182,
            &[0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.5],
        );
        let surface_at = surface
            .windows(surface_control.len())
            .position(|window| window == surface_control)
            .unwrap();
        let surface_values_at = super::array_body(&surface, surface_at, 0x2d).unwrap()
            + crate::layout::bspline_array_header::LEN;
        surface[surface_values_at..surface_values_at + 8]
            .copy_from_slice(&f64::NAN.to_be_bytes());

        for (bytes, curve) in [(&curve, true), (&surface, false)] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
            let empty = if curve {
                scan_curve_carriers(&ctx, bytes, &mut Vec::new()).unwrap().is_empty()
            } else {
                scan_surface_carriers(&ctx, bytes, &mut Vec::new()).unwrap().is_empty()
            };
            assert!(empty);
            ctx.finish_session().unwrap();
        }
    }

    #[test]
    fn curve_missing_dependency_skips_large_control_payload() {
        let mut bytes = crate::test_support::parasolid::nurbs_curve_carrier(170, 171);
        let descriptor_at = bytes
            .windows(2)
            .position(|window| window == [0x00, 0x88])
            .expect("curve descriptor");
        let pole_count = 4096_u32;
        bytes[descriptor_at + 6..descriptor_at + 10].copy_from_slice(&pole_count.to_be_bytes());
        bytes[descriptor_at + 23..descriptor_at + 25].copy_from_slice(&0x7ffe_u16.to_be_bytes());
        let old_control = crate::test_support::parasolid::f64_array(
            0x2d,
            172,
            &[0.0, 0.0, 0.0, 0.5, 1.0, 0.0, 1.0, 0.0, 0.0],
        );
        let at = bytes
            .windows(old_control.len())
            .position(|window| window == old_control)
            .expect("curve control array");
        let control_count = usize::try_from(pole_count).unwrap() * 3;
        let large_control = crate::test_support::parasolid::f64_array(
            0x2d,
            172,
            &vec![0.25; control_count],
        );
        bytes.splice(at..at + old_control.len(), large_control);

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "decode Parasolid scalar array values",
            None,
        );
        assert!(scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
            .unwrap()
            .is_empty());
        ctx.finish_session().unwrap();
    }

    #[test]
    fn unmatched_surface_knot_header_skips_large_payload() {
        let mut bytes = crate::test_support::parasolid::nurbs_surface_carrier(180, 181, 10);
        let old_knots = crate::test_support::parasolid::f64_array(0x80, 186, &[0.0, 1.0]);
        let at = bytes
            .windows(old_knots.len())
            .position(|window| window == old_knots)
            .expect("v-axis knot array");
        let large_knots = crate::test_support::parasolid::f64_array(0x80, 186, &vec![0.25; 8192]);
        bytes.splice(at..at + old_knots.len(), large_knots);

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        policy.limits.max_materialized_bytes = u64::MAX;
        let additional = u64::try_from(
            (8192 / 2) * std::mem::size_of::<f64>(),
        )
        .expect("bounded fixture growth");
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::MaterializedBytes,
            "decode Parasolid scalar array values",
            Some(additional),
        );
        assert!(scan_surface_carriers(&ctx, &bytes, &mut Vec::new())
            .unwrap()
            .is_empty());
        ctx.finish_session().unwrap();
    }

    #[test]
    fn compact_array_duplicates_release_candidate_storage() {
        let values = [0.25_f64; 12];
        let mut bytes = Vec::new();
        let mut compact_arrays = Vec::new();
        for _ in 0..64 {
            let offset = bytes.len();
            bytes.extend(crate::test_support::parasolid::compact_f64_array(12, &values));
            compact_arrays.push(super::CompactArray {
                offset,
                count: values.len(),
            });
        }
        let compact = std::collections::BTreeMap::from([(12, compact_arrays)]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The 12-value read grows to capacity 16. While comparing the second
        // duplicate, both 16-value buffers and the prior 8-value buffer during
        // the final reallocation remain live, plus four ArrayCandidate slots.
        let metadata = 4 * std::mem::size_of::<super::ArrayCandidate<'_, '_, f64>>();
        let value_storage = 40 * std::mem::size_of::<f64>();
        policy.limits.max_materialized_bytes = u64::try_from(metadata + value_storage)
            .expect("bounded fixture storage");
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let framed = std::collections::BTreeMap::new();
        let mut storage = ctx.reserve_scoped(0, "hold test array candidates").unwrap();
        let candidates = super::array_candidates(
            &ctx,
            &mut storage,
            &bytes,
            &framed,
            &compact,
            12,
            |_| Ok(true),
            |at| View::f64_be_at(&bytes, at),
        )
        .unwrap();
        assert_eq!(candidates.len(), 1);
        drop(candidates);
        drop(storage);
        drop(framed);
        ctx.finish_session().unwrap();
    }

    #[test]
    fn curve_pole_scan_stops_at_the_first_invalid_pole() {
        let mut bytes = crate::test_support::parasolid::nurbs_curve_carrier(170, 171);
        let control = crate::test_support::parasolid::f64_array(
            0x2d,
            172,
            &[0.0, 0.0, 0.0, 0.5, 1.0, 0.0, 1.0, 0.0, 0.0],
        );
        let at = bytes
            .windows(control.len())
            .position(|window| window == control)
            .expect("curve control array");
        let values_at = super::array_body(&bytes, at, 0x2d).unwrap()
            + crate::layout::bspline_array_header::LEN;
        bytes[values_at..values_at + 8].copy_from_slice(&f64::NAN.to_be_bytes());

        let error = crate::test_support::work_refusal_at("scan Parasolid curve poles", |ctx| {
            scan_curve_carriers(ctx, &bytes, &mut Vec::new())
        });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "scan Parasolid curve poles" && limit.additional == 1));
        let ctx = cadmpeg_test_support::service_decode_context();
        assert!(scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn framed_array_payload_is_cached_with_its_scoped_storage() {
        let bytes = crate::test_support::parasolid::f64_array(0x2d, 12, &[0.0, 1.0, 2.0]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 4096;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (arrays, nodes) = ctx
            .with_scoped_storage("test framed array index", || {
                scan_arrays(&ctx, &bytes, None, None)
            })
            .unwrap();
        let array = arrays.f64s.get(&12).unwrap();
        assert!(array.values.get().is_none());
        assert_eq!(
            array.read(&ctx, |at| View::f64_be_at(&bytes, at)).unwrap(),
            [0.0, 1.0, 2.0]
        );
        {
            let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                ResourceDimension::WorkUnits,
                "decode Parasolid scalar array values",
                None,
            );
            assert_eq!(
                array.read(&ctx, |at| View::f64_be_at(&bytes, at)).unwrap(),
                [0.0, 1.0, 2.0]
            );
        }
        drop(arrays);
        drop(nodes);
        let released = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "test released framed values",
            )
            .unwrap();
        drop(released);
        ctx.finish_session().unwrap();
    }

    #[test]
    fn duplicate_curve_wrapper_does_not_read_its_unused_large_payload() {
        let mut bytes = crate::test_support::parasolid::nurbs_curve_carrier(170, 171);
        let mut duplicate = crate::test_support::parasolid::nurbs_curve_carrier(170, 400);
        let descriptor_at = duplicate
            .windows(2)
            .position(|window| window == [0x00, 0x88])
            .expect("duplicate curve descriptor");
        let pole_count = 43_690_u32;
        duplicate[descriptor_at + 6..descriptor_at + 10]
            .copy_from_slice(&pole_count.to_be_bytes());
        let control_count = usize::try_from(pole_count).unwrap() * 3;
        let control = crate::test_support::parasolid::f64_array(
            0x2d,
            401,
            &[0.0, 0.0, 0.0, 0.5, 1.0, 0.0, 1.0, 0.0, 0.0],
        );
        let at = duplicate
            .windows(control.len())
            .position(|window| window == control)
            .unwrap();
        let unused = crate::test_support::parasolid::f64_array(
            0x2d,
            401,
            &vec![0.25; control_count],
        );
        duplicate.splice(at..at + control.len(), unused);
        bytes.extend(duplicate);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 128 * 1024;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let decoded = scan_curve_carriers(&ctx, &bytes, &mut Vec::new()).unwrap();
        assert_eq!(decoded.len(), 1);
        assert!(decoded.contains_key(&170));
        ctx.finish_session().unwrap();
    }

    #[test]
    fn scalar_array_read_stops_at_first_missing_cell() {
        use std::cell::Cell;
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let calls = Cell::new(0);
        assert_eq!(
            super::read_array(&ctx, 100, "read missing scalar cell", |index| {
                calls.set(calls.get() + 1);
                (index < 2).then_some(index)
            })
            .unwrap(),
            None
        );
        assert_eq!(calls.get(), 3);
        crate::test_support::work_refusal_at("read missing scalar cell", |ctx| {
            super::read_array(ctx, 100, "read missing scalar cell", |index| {
                (index < 2).then_some(index)
            })
        });
    }

    #[test]
    fn parasolid_scalar_array_values_refuse_collection_limit_before_value_insertion() {
        let bytes = crate::test_support::parasolid::f64_array(0x2d, 12, &[0.0, 1.0, 2.0]);
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "decode Parasolid scalar array values",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
                let arrays = scan_arrays(&ctx, &bytes, None, None)?;
                let Some(array) =
                    ctx.get_btree_map(&arrays.f64s, &12, "test framed array lookup")?
                else {
                    panic!("framed array is present");
                };
                array
                    .read(&ctx, |at| View::f64_be_at(&bytes, at))
                    .map(<[f64]>::len)
            },
        );
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid scalar array values"));

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert_eq!(
            scan_arrays(&ctx, &bytes, None, None)
                .expect("service scan")
                .f64s
                .get(&12)
                .map(|array| array
                    .read(&ctx, |at| View::f64_be_at(&bytes, at))
                    .expect("service values")
                    .len()),
            Some(3)
        );
    }

    #[test]
    fn parasolid_integer_array_values_refuse_collection_limit_before_value_insertion() {
        let bytes = crate::test_support::parasolid::u16_array(12, &[1, 2, 3]);
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "decode Parasolid integer array values",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
                let arrays = scan_arrays(&ctx, &bytes, None, None)?;
                let Some(array) =
                    ctx.get_btree_map(&arrays.u16s, &12, "test framed array lookup")?
                else {
                    panic!("framed array is present");
                };
                array
                    .read(&ctx, |at| View::u16_be_at(&bytes, at))
                    .map(<[u16]>::len)
            },
        );
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid integer array values"));
    }

    #[test]
    fn parasolid_knot_expansion_refuses_collection_limit_before_allocation() {
        let bytes = [0_u8; 1];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = expanded_knots(&ctx, &[0.0, 1.0], &[2, 2], 4)
            .expect_err("four knots exceed three items");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "expand Parasolid knots"));

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert_eq!(
            expanded_knots(&ctx, &[0.0, 1.0], &[2, 2], 4).expect("service expansion"),
            Some(vec![0.0, 0.0, 1.0, 1.0])
        );
    }

    #[test]
    fn parasolid_curve_poles_refuse_collection_limit_before_allocation() {
        let bytes = crate::test_support::parasolid::nurbs_curve_carrier(170, 171);
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "decode Parasolid curve poles",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)?;
                scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
            },
        );
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid curve poles"));

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert!(scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
            .expect("service scan")
            .contains_key(&170));
    }

    #[test]
    fn parasolid_curve_weights_refuse_collection_limit_before_allocation() {
        let bytes = crate::test_support::parasolid::rational_linear_nurbs_curve_carrier(170, 171);
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "decode Parasolid curve weights",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
                scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
            },
        );
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid curve weights"));
    }

    #[test]
    fn parasolid_curve_descriptors_refuse_collection_limit_before_insertion() {
        let bytes = crate::test_support::parasolid::nurbs_curve_carrier(170, 171);
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "collect Parasolid curve descriptors",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
                scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
            },
        );
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "collect Parasolid curve descriptors"));
    }

    #[test]
    fn parasolid_curve_carriers_refuse_collection_limit_before_insertion() {
        let bytes = crate::test_support::parasolid::nurbs_curve_carrier(170, 171);
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "collect Parasolid curve carriers",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
                scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
            },
        );
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "collect Parasolid curve carriers"));
    }

    #[test]
    fn parasolid_curve_weighted_poles_refuse_collection_limit_before_pairing() {
        let bytes = crate::test_support::parasolid::rational_linear_nurbs_curve_carrier(170, 171);
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "IR NURBS paired poles",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
                scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
            },
        );
        assert!(
            matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "IR NURBS paired poles"),
            "{error:?}"
        );
    }

    #[test]
    fn parasolid_curve_poles_refuse_collection_limit_before_admission() {
        let bytes = crate::test_support::parasolid::rational_linear_nurbs_curve_carrier(170, 171);
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "IR NURBS admitted poles",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)?;
                scan_curve_carriers(&ctx, &bytes, &mut Vec::new())
            },
        );
        assert!(
            matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "IR NURBS admitted poles"),
            "{error:?}"
        );
    }

    #[test]
    fn parasolid_surface_poles_refuse_collection_limit_before_allocation() {
        let bytes = crate::test_support::parasolid::nurbs_surface_carrier(180, 181, 10);
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "decode Parasolid surface poles",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
                scan_surface_carriers(&ctx, &bytes, &mut Vec::new())
            },
        );
        assert!(
            matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid surface poles"),
            "{error:?}"
        );

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert!(scan_surface_carriers(&ctx, &bytes, &mut Vec::new())
            .expect("service scan")
            .contains_key(&180));
    }

    #[test]
    fn parasolid_surface_weights_refuse_collection_limit_before_allocation() {
        let bytes = crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10);
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "decode Parasolid surface weights",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
                scan_surface_carriers(&ctx, &bytes, &mut Vec::new())
            },
        );
        assert!(
            matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode Parasolid surface weights"),
            "{error:?}"
        );
    }

    macro_rules! surface_collection_boundary {
        ($name:ident, $bytes:expr, $operation:literal) => {
            #[test]
            fn $name() {
                let bytes = $bytes;
                let arena = DecodeArena::new();
                let error = cadmpeg_test_support::refusal::resource_limit_at(
                    ResourceDimension::CollectionItems,
                    $operation,
                    |cap| {
                        let mut policy = DecodePolicy::service();
                        policy.limits.max_collection_items = cap;
                        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)?;
                        let result = scan_surface_carriers(&ctx, &bytes, &mut Vec::new());
                        if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                        }
                        result
                    },
                );
                assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
            }
        };
    }

    macro_rules! plain_surface_boundary {
        ($name:ident, $operation:literal) => {
            surface_collection_boundary!(
                $name,
                crate::test_support::parasolid::nurbs_surface_carrier(180, 181, 10),
                $operation
            );
        };
    }

    plain_surface_boundary!(
        parasolid_surface_descriptors_refuse_before_insertion,
        "collect Parasolid surface descriptors"
    );
    plain_surface_boundary!(
        parasolid_surface_array_references_refuse_before_collection,
        "collect Parasolid surface array references"
    );
    plain_surface_boundary!(
        parasolid_compact_arrays_refuse_before_insertion,
        "scan Parasolid compact arrays"
    );
    plain_surface_boundary!(
        parasolid_scalar_arrays_refuse_before_insertion,
        "collect Parasolid scalar arrays"
    );
    plain_surface_boundary!(
        parasolid_integer_arrays_refuse_before_insertion,
        "collect Parasolid integer arrays"
    );
    plain_surface_boundary!(
        parasolid_array_candidates_refuse_before_insertion,
        "collect Parasolid array candidates"
    );
    plain_surface_boundary!(
        parasolid_compact_values_refuse_before_allocation,
        "decode Parasolid compact values"
    );
    plain_surface_boundary!(
        parasolid_knot_multiplicity_groups_refuse_before_insertion,
        "group Parasolid knot multiplicities"
    );
    plain_surface_boundary!(
        parasolid_distinct_knots_refuse_before_copy,
        "copy Parasolid distinct knots"
    );
    plain_surface_boundary!(
        parasolid_knot_multiplicities_refuse_before_copy,
        "copy Parasolid knot multiplicities"
    );
    plain_surface_boundary!(
        parasolid_knot_candidates_refuse_before_insertion,
        "collect Parasolid knot candidates"
    );
    plain_surface_boundary!(
        parasolid_surface_pole_rows_refuse_before_partition,
        "partition Parasolid surface pole rows"
    );
    plain_surface_boundary!(
        parasolid_surface_pole_rows_refuse_before_admission,
        "IR NURBS admitted grid rows"
    );
    plain_surface_boundary!(
        parasolid_surface_poles_refuse_before_admission,
        "IR NURBS admitted poles"
    );
    plain_surface_boundary!(
        parasolid_surface_carriers_refuse_before_insertion,
        "collect Parasolid surface carriers"
    );
    surface_collection_boundary!(
        parasolid_surface_weight_rows_refuse_before_partition,
        crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10),
        "partition Parasolid surface weight rows"
    );
    surface_collection_boundary!(
        parasolid_weighted_surface_rows_refuse_before_pairing,
        crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10),
        "IR NURBS paired grid rows"
    );
    surface_collection_boundary!(
        parasolid_weighted_surface_poles_refuse_before_pairing,
        crate::test_support::parasolid::rational_nurbs_surface_carrier(180, 181, 10),
        "IR NURBS paired poles"
    );

