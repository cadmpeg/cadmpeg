// SPDX-License-Identifier: Apache-2.0

use super::{complete_cap_envelope_body, withhold_unresolved_cap_envelopes, CAP_SUPPORT_IMAGES};
use crate::feature::rows::FeatureRow;
use crate::feature::schema::SchemaClass;
use crate::surface::{
    InlineSurfaceCarrier, InlineType26MinorRadius, PositionalCylinderFrame, SurfaceBodyBoundary,
    SurfaceKind, SurfaceParameterCarrier, SurfaceParameterRecord,
};

fn scalar(value: f64) -> Vec<u8> {
    let bytes = value.to_be_bytes();
    let token = match bytes[0] {
        0x3f => 0x28,
        0x40 => 0x2d,
        0xc0 => 0x46,
        _ => panic!("fixture requires an explicit scalar form"),
    };
    std::iter::once(token)
        .chain(bytes[1..].iter().copied())
        .collect()
}

fn body(tail: bool) -> Vec<u8> {
    let control = [0x6b, 0xe3, 1, 2, 3, 4, 5];
    let mut bytes = control.to_vec();
    bytes.push(0x18);
    bytes.extend(control);
    bytes.extend(control);
    for value in [-7.0, -4.0, 2.0, 4.0, 5.0, 6.0] {
        bytes.extend(scalar(value));
    }
    if tail {
        bytes.push(0xe3);
        bytes.extend(CAP_SUPPORT_IMAGES[0]);
        for value in [2.0, 3.0, 6.0, 0.75] {
            bytes.extend(scalar(value));
        }
    }
    bytes
}

fn record(bytes: Vec<u8>) -> SurfaceParameterRecord {
    let frame = PositionalCylinderFrame::new(
        [8.0, -9.0, 2.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        0.75,
        Some(4.0),
    )
    .expect("finite fixture frame");
    SurfaceParameterRecord {
        surface_id: 17,
        body: bytes,
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        carrier: SurfaceParameterCarrier::Resolved(InlineSurfaceCarrier::Cylinder {
            frame,
            split_bounds: None,
        }),
        type26_minor_radius: InlineType26MinorRadius::Absent,
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 30,
        body_offset: 38,
    }
}

fn feature(class: u32) -> FeatureRow {
    FeatureRow {
        feature_id: 6,
        root_schema_class: Some(SchemaClass::from(class)),
        stream_offset: 0,
        body: vec![0xeb, 4].try_into().expect("complete feature header"),
        body_offset: 0,
        offset: 0,
    }
}

#[test]
fn cap_envelope_withholding_requires_complete_framing() {
    let complete = body(true);
    assert!(complete_cap_envelope_body(&complete).is_some());
    assert!(complete_cap_envelope_body(&body(false)).is_some());
    for support in CAP_SUPPORT_IMAGES {
        let mut alternate = complete.clone();
        let offset = body(false).len() + 1;
        alternate[offset..offset + support.len()].copy_from_slice(&support);
        assert!(complete_cap_envelope_body(&alternate).is_some());
    }
    let mut partial = body(false);
    partial.push(0xe3);
    partial.extend(CAP_SUPPORT_IMAGES[0]);
    partial.push(0x0d);
    partial.extend([0, 4, 0xdc]);
    partial.extend(scalar(6.0));
    partial.push(0x18);
    assert!(complete_cap_envelope_body(&partial).is_some());
    let mut trailing = complete.clone();
    trailing.push(0);
    assert!(complete_cap_envelope_body(&trailing).is_none());
    assert!(complete_cap_envelope_body(&complete[..complete.len() - 1]).is_none());
    assert!(complete_cap_envelope_body(&complete[..8]).is_none());
    let mut unrelated = complete;
    unrelated[7] = 0x0f;
    assert!(complete_cap_envelope_body(&unrelated).is_none());
}

#[test]
fn cap_envelope_withholding_preserves_raw_body_without_numeric_claims() {
    let bytes = body(true);
    let mut scan = crate::container::scan_bytes_ok(crate::test_support::build_prt("cap", &[]));
    scan.features.rows = vec![feature(932)];
    scan.surfaces.rows = vec![crate::decode::tests::surface_row(
        17,
        6,
        SurfaceKind::Cylinder,
    )];
    scan.surfaces.parameters = vec![record(bytes.clone())];
    crate::decode::with_service_context(|ctx| {
        withhold_unresolved_cap_envelopes(ctx, &mut scan).expect("bounded opaque body");
    });
    let retained = &scan.surfaces.parameters[0];
    assert_eq!(retained.body, bytes);
    assert!(scan.surfaces.withheld_cap_surface_ids.contains(&17));
    assert!(retained.positional_cylinder_frame().is_none());
    assert!(retained.scalar_tokens.is_empty());
    assert!(retained.scalar_frames.is_empty());
    assert_eq!(retained.opaque_spans.len(), 1);
    assert_eq!(retained.opaque_spans[0].offset, 0);
    assert_eq!(retained.opaque_spans[0].raw.as_slice(), retained.body);
}

#[test]
fn cap_envelope_withholding_requires_unambiguous_cylinder_owner() {
    let mut scan = crate::container::scan_bytes_ok(crate::test_support::build_prt("cap", &[]));
    scan.surfaces.rows = vec![crate::decode::tests::surface_row(
        17,
        6,
        SurfaceKind::Cylinder,
    )];
    scan.surfaces.parameters = vec![record(body(true))];
    for classes in [vec![], vec![feature(913)], vec![feature(932), feature(913)]] {
        scan.features.rows = classes;
        let before = scan.surfaces.parameters.clone();
        crate::decode::with_service_context(|ctx| {
            withhold_unresolved_cap_envelopes(ctx, &mut scan).expect("unmatched owner");
        });
        assert_eq!(scan.surfaces.parameters, before);
    }
    scan.features.rows = vec![feature(932)];
    scan.surfaces.rows.push(scan.surfaces.rows[0].clone());
    let before = scan.surfaces.parameters.clone();
    crate::decode::with_service_context(|ctx| {
        withhold_unresolved_cap_envelopes(ctx, &mut scan).expect("duplicate owner");
    });
    assert_eq!(scan.surfaces.parameters, before);
}

#[test]
fn cap_envelope_withholding_blocks_feature_membership_circle_fallback() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows = vec![feature(932)];
    scan.surfaces.rows = vec![crate::decode::tests::surface_row(
        17,
        6,
        SurfaceKind::Cylinder,
    )];
    scan.surfaces.parameters = vec![record(body(true))];
    scan.features
        .entity_tables
        .push(crate::feature::entity::FeatureEntityTable::new(
            6,
            80,
            vec![
                crate::feature::entity::dummy_table_entry(22),
                crate::feature::entity::dummy_table_entry(23),
            ],
            &std::collections::BTreeSet::new(),
            0,
        ));
    let circle = |id, z| {
        crate::reference::ReferenceCircle::try_new(
            id,
            crate::reference::ReferenceCircleCenter::Stored(
                cadmpeg_ir::features::FinitePoint3::new([2.0, 3.0, z].into())
                    .expect("finite center"),
            ),
            cadmpeg_ir::scalar::PositiveLength::new(0.75).expect("positive radius"),
            cadmpeg_ir::units::UnitVector3::new([0.0, 0.0, 1.0].into()).expect("unit axis"),
            [
                cadmpeg_ir::features::FinitePoint3::new([2.75, 3.0, z].into())
                    .expect("finite start"),
                cadmpeg_ir::features::FinitePoint3::new([2.0, 3.75, z].into()).expect("finite end"),
            ],
            0,
        )
        .expect("stored circle")
    };
    scan.references.circles = vec![circle(22, 2.0), circle(23, 6.0)];
    assert!(
        crate::decode::surfaces::cylinders::reference_circle_pair_cylinder_frame(&[
            &scan.references.circles[0],
            &scan.references.circles[1]
        ])
        .is_some()
    );
    crate::decode::with_service_context(|ctx| {
        withhold_unresolved_cap_envelopes(ctx, &mut scan).expect("withheld complete body");
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let result = crate::decode::surfaces::cylinders::transfer_positional_cylinders(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
        .expect("bounded source identities");
        assert_eq!(result.transferred, 0);
        assert!(ir.model.surfaces.is_empty());
    });
}
