// SPDX-License-Identifier: Apache-2.0
//! Inventor sketch record parsing tests.

use super::*;

fn legacy_constraint_header(index: u32, parameter: u32) -> Vec<u8> {
    let mut bytes = content(index);
    bytes.extend_from_slice(&(-1i32).to_le_bytes());
    bytes.extend_from_slice(&0x8000_000cu32.to_le_bytes());
    bytes.extend_from_slice(&parameter.to_le_bytes());
    bytes
}

#[test]
fn parses_generated_planar_geometry_branches() {
    let point = point_bytes(1, 3, [1.25, -2.5]);
    let parsed = parse(&point, |ctx, source| {
        parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point")
    });
    assert!(matches!(
        parsed.kind,
        PmDcSketchEntityKind::Point {
            position,
            ..
        } if position.map(FiniteReal::get) == [1.25, -2.5]
    ));

    let line = line_bytes(2, 3, [4, 5]);
    let parsed = parse(&line, |ctx, source| {
        parse_entity(ctx, SketchEntityTag::Line, source, 22).expect("line")
    });
    assert!(matches!(
        parsed.kind,
        PmDcSketchEntityKind::Line { ref points, .. }
            if points.references().iter().map(|value| value.index()).collect::<Vec<_>>() == [4, 5]
    ));

    let mut circle = entity_prefix(3, 3, 0);
    circle.extend(list(2, &[]));
    circle.extend(list(2, &[]));
    circle.extend_from_slice(&4u32.to_le_bytes());
    circle.extend_from_slice(&2.5f64.to_le_bytes());
    circle.push(1);
    let parsed = parse(&circle, |ctx, source| {
        parse_entity(ctx, SketchEntityTag::Circle, source, 22).expect("circle")
    });
    assert!(matches!(
        parsed.kind,
        PmDcSketchEntityKind::Circle { radius, .. } if radius.get() == 2.5
    ));

    let mut ellipse = entity_prefix(4, 3, 0);
    ellipse.extend(list(2, &[]));
    ellipse.extend(list(2, &[]));
    ellipse.extend_from_slice(&4u32.to_le_bytes());
    ellipse.extend_from_slice(&1.0f64.to_le_bytes());
    ellipse.extend_from_slice(&0.0f64.to_le_bytes());
    ellipse.extend_from_slice(&3.0f64.to_le_bytes());
    ellipse.extend_from_slice(&2.0f64.to_le_bytes());
    ellipse.push(0);
    let parsed = parse(&ellipse, |ctx, source| {
        parse_entity(ctx, SketchEntityTag::Ellipse, source, 22).expect("ellipse")
    });
    assert!(matches!(
        parsed.kind,
        PmDcSketchEntityKind::Ellipse {
            major_radius,
            minor_radius,
            ..
        } if major_radius.get() == 3.0 && minor_radius.get() == 2.0
    ));
}

#[test]
fn constraint_map_counts_refuse_truncated_payload_before_admission() {
    for scalar in [true, false] {
        let width = if scalar {
            super::super::MIN_SCALAR_MAP_ENTRY_BYTES
        } else {
            super::super::MIN_REFERENCE_MAP_ENTRY_BYTES
        };
        for count in [1_u32, u32::MAX] {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&6_u16.to_le_bytes());
            bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            bytes.extend_from_slice(&[0; 8]);
            bytes.resize(bytes.len() + width - 1, 0);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_work_units = 0;
            let (ctx, view) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
            let mut cursor = crate::pmdc::Cursor::new(view);
            let result = if scalar {
                super::super::reference_scalar_map(&ctx, &mut cursor).map(|map| map.entries().len())
            } else {
                super::super::reference_pair_map(&ctx, &mut cursor).map(|map| map.entries().len())
            };
            assert!(matches!(result, Err(CodecError::Malformed(_))));
        }
    }
}

#[test]
fn scalar_map_formats_its_index_only_for_a_nonfinite_error() {
    for value in [0.5_f64, f64::NAN] {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&6_u16.to_le_bytes());
        bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, view) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        let result = super::super::reference_scalar_map(&ctx, &mut crate::pmdc::Cursor::new(view));
        if value.is_finite() {
            assert_eq!(
                result.expect("no temporary diagnostic text").entries()[0]
                    .1
                    .get(),
                value
            );
        } else {
            assert!(matches!(result, Err(CodecError::Malformed(detail))
                if detail == "Inventor PmDc constraint scalar-map value 0 is not finite"));
        }
    }
}

#[test]
fn nonfinite_first_scalar_map_value_does_not_prepay_the_tail() {
    for count in [1_u32, 512] {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&6_u16.to_le_bytes());
        bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&f64::NAN.to_le_bytes());
        let entries = usize::try_from(count).expect("fixture count");
        bytes.resize(16 + entries * super::super::MIN_SCALAR_MAP_ENTRY_BYTES, 0);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("scalar map context");
        assert!(matches!(
            super::super::reference_scalar_map(&ctx, &mut crate::pmdc::Cursor::new(view)),
            Err(CodecError::Malformed(detail))
                if detail == "Inventor PmDc constraint scalar-map value 0 is not finite"
        ));
        ctx.finish_session().expect("unread scalar-map values use no work");
    }
}

#[test]
fn reference_maps_admit_each_pair_and_the_end_probe() {
    for count in [0_usize, 1, 512] {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&6_u16.to_le_bytes());
        bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(count).expect("fixture count").to_le_bytes());
        if count != 0 {
            bytes.extend_from_slice(&[0; 8]);
        }
        for _ in 0..count {
            bytes.extend_from_slice(&0x8000_0001_u32.to_le_bytes());
            bytes.extend_from_slice(&2_u32.to_le_bytes());
        }
        for end_probe in [0_u64, 1] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_work_units =
                cadmpeg_core::decode::u64_from_index(count) + end_probe;
            let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("reference map context");
            let result = super::super::reference_pair_map(&ctx, &mut crate::pmdc::Cursor::new(view));
            if end_probe == 0 {
                let error = result.err().expect("pair-map end probe refuses");
                assert!(matches!(&error, CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::WorkUnits
                        && limit.operation == "read Inventor sketch constraint reference map"
                        && limit.used == cadmpeg_core::decode::u64_from_index(count)
                        && limit.additional == 1));
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit))
                    if matches!(&error, CodecError::ResourceLimit(original) if original == &limit)));
            } else {
                let map = result.expect("pairs and end probe fit");
                assert_eq!(map.entries().len(), count);
                if count != 0 {
                    for pair in [map.entries()[0], map.entries()[count - 1]] {
                        assert_eq!(pair.0.index(), 1);
                        assert!(pair.0.qualified());
                        assert_eq!(pair.1.index(), 2);
                        assert!(!pair.1.qualified());
                    }
                }
                ctx.finish_session().expect("reference-map work fits exactly");
            }
        }
    }
}

#[test]
fn planar_geometry_uses_static_diagnostic_fields() {
    let point = point_bytes(1, 3, [1.25, -2.5]);
    let line = line_bytes(2, 3, [4, 5]);
    for (tag, bytes) in [
        (SketchEntityTag::Point, point),
        (SketchEntityTag::Line, line),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, view) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        assert!(parse_entity(&ctx, tag, view, 22).is_ok());
    }
    let mut bytes = content(0);
    bytes.extend_from_slice(&[0; 36]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    assert!(parse_direction(view, 22).is_ok());
    ctx.finish_session().expect("fixed direction needs no scratch");
}

#[test]
fn parses_generated_constraint_branches() {
    for (type_id, tail, expected) in [
        (SketchConstraintTag::Coincident, vec![4, 5], "coincident"),
        (SketchConstraintTag::Parallel, vec![4, 5, 0], "parallel"),
        (
            SketchConstraintTag::Perpendicular,
            vec![4, 5, 0],
            "perpendicular",
        ),
        (SketchConstraintTag::Tangent, vec![4, 5, 0], "tangent"),
    ] {
        let mut bytes = constraint_header(9, 0);
        for (index, value) in tail.into_iter().enumerate() {
            if index == 2 && matches!(expected, "parallel" | "perpendicular") {
                bytes.extend_from_slice(
                    &(u16::try_from(value).expect("fixture value fits u16")).to_le_bytes(),
                );
            } else {
                bytes.extend_from_slice(
                    &(u32::try_from(value).expect("fixture value fits u32")).to_le_bytes(),
                );
            }
        }
        let parsed = parse(&bytes, |ctx, source| {
            parse_constraint(ctx, type_id, source, 22).expect(expected)
        });
        assert_eq!(
            match parsed.kind {
                PmDcSketchConstraintKind::Coincident { .. } => "coincident",
                PmDcSketchConstraintKind::Parallel { .. } => "parallel",
                PmDcSketchConstraintKind::Perpendicular { .. } => "perpendicular",
                PmDcSketchConstraintKind::Tangent { .. } => "tangent",
                _ => "unexpected",
            },
            expected
        );
    }

    for (type_id, expected) in [
        (SketchConstraintTag::Horizontal, true),
        (SketchConstraintTag::Vertical, false),
    ] {
        let mut bytes = constraint_header(10, 0);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.push(1);
        let parsed = parse(&bytes, |ctx, source| {
            parse_constraint(ctx, type_id, source, 22).expect("axis constraint")
        });
        assert_eq!(
            matches!(parsed.kind, PmDcSketchConstraintKind::Horizontal { .. }),
            expected
        );
    }
}

#[test]
fn parses_generated_legacy_constraint_header_without_maps() {
    let mut bytes = legacy_constraint_header(9, 0);
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&5u32.to_le_bytes());
    let parsed = parse(&bytes, |ctx, source| {
        parse_constraint(ctx, SketchConstraintTag::Coincident, source, 16)
            .expect("legacy coincident")
    });
    assert!(parsed.header.scalar_map.entries().is_empty());
    assert!(parsed.header.reference_map.entries().is_empty());
    assert!(matches!(
        parsed.kind,
        PmDcSketchConstraintKind::Coincident { first, second }
            if first.index() == 4 && second.index() == 5
    ));
}

#[test]
fn parses_generated_dimensional_constraint_branches() {
    for type_id in [
        SketchConstraintTag::HorizontalDistance,
        SketchConstraintTag::VerticalDistance,
    ] {
        let mut bytes = constraint_header(11, 0);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&5u32.to_le_bytes());
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        parse(&bytes, |ctx, source| {
            parse_constraint(ctx, type_id, source, 22).expect("distance constraint")
        });
    }
    let mut radius = constraint_header(12, 13);
    radius.extend_from_slice(&0u32.to_le_bytes());
    radius.extend_from_slice(&4u32.to_le_bytes());
    radius.extend_from_slice(&[0; 16]);
    parse(&radius, |ctx, source| {
        parse_constraint(ctx, SketchConstraintTag::Radius, source, 22).expect("radius")
    });

    let mut diameter = constraint_header(13, 14);
    diameter.extend_from_slice(&0u32.to_le_bytes());
    diameter.extend_from_slice(&4u32.to_le_bytes());
    diameter.extend_from_slice(&[0; 16]);
    parse(&diameter, |ctx, source| {
        parse_constraint(ctx, SketchConstraintTag::Diameter, source, 22).expect("diameter")
    });

    for type_id in [
        SketchConstraintTag::CircleCenter,
        SketchConstraintTag::EqualRadius,
    ] {
        let mut bytes = constraint_header(14, 0);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&5u32.to_le_bytes());
        parse(&bytes, |ctx, source| {
            parse_constraint(ctx, type_id, source, 22).expect("circle relation")
        });
    }
}
