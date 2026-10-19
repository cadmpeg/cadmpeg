// SPDX-License-Identifier: Apache-2.0

use super::{pcurve_geometry, pcurve_loss, PcurveGeometryError};
use crate::brep::TextCurve2d;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::EdgeId;
use cadmpeg_ir::scalar::FiniteReal;

#[test]
fn pcurve_loss_message_names_the_payload_and_index() {
    let payload_id = "fcstd:native:shape#Source";
    assert_eq!(
        pcurve_loss(payload_id, 2, None).message,
        "payload fcstd:native:shape#Source curve2ds index 2 could not enter neutral geometry"
    );
}

#[test]
fn pcurve_malformed_loss_message_carries_the_geometry_error() {
    let payload_id = "fcstd:native:shape#Source";
    let error = PcurveGeometryError::Nurbs(cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
        "knots must be non-decreasing".into(),
    ));
    assert_eq!(pcurve_loss(payload_id, 2, Some(&error)).message,
        "payload fcstd:native:shape#Source curve2ds index 2 could not enter neutral geometry: knots must be non-decreasing");
}

#[test]
fn pcurve_nested_basis_refuses_at_depth_limit() {
    let curve = TextCurve2d::Offset {
        distance: FiniteReal::ONE,
        basis: crate::brep::NestedCurve2d::try_new(TextCurve2d::Line {
            origin: cadmpeg_ir::units::FinitePoint2::ZERO,
            direction: cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(
                1.0, 0.0,
            ))
            .expect("finite direction"),
        })
        .expect("one nested basis"),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(matches!(pcurve_geometry(&ctx, &curve),
        Err(PcurveGeometryError::Resource(CodecError::ResourceLimit(refusal)))
            if refusal.operation == "FreeCAD pcurve geometry nesting"));
}

#[test]
fn polygon_curve_id_spelling_is_preserved() {
    let edge = EdgeId::mint("fcstd:model:edge#Payload:1").unwrap();
    assert_eq!(
        super::Builder::polygon_curve_id(&edge, 0, false)
            .unwrap()
            .as_str(),
        "fcstd:model:edge#Payload:1:polygon:1"
    );
    assert_eq!(
        super::Builder::polygon_curve_id(&edge, 0, true)
            .unwrap()
            .as_str(),
        "fcstd:model:edge#Payload:1:polygon:1:secondary"
    );
}
