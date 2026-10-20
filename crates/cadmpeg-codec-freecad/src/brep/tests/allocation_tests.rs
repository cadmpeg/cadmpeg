// SPDX-License-Identifier: Apache-2.0
//! Input-sized BREP parser allocation tests.

use super::super::{
    append_text_curve, append_text_surface, CurveTransfer, SurfaceTransfer, TextCurve, TextSurface,
};
use crate::native::{PropertyBody, PropertyFamily, PropertyRecord, RetainedXml};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::SourceObjectAssociation;

#[test]
fn transferred_nurbs_curve_keeps_one_geometry_copy() {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    let nurbs = NurbsCurve::from_finite_lanes(
        1,
        vec![
            FiniteReal::ZERO,
            FiniteReal::ZERO,
            FiniteReal::ONE,
            FiniteReal::ONE,
        ],
        vec![FinitePoint3::ZERO, FinitePoint3::ZERO],
        None,
        false,
    )
    .unwrap();
    let curve = TextCurve::Nurbs(nurbs.clone());
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut transfer = CurveTransfer::default();
    let index = append_text_curve(
        &ctx,
        &curve,
        CurveId::mint("fcstd:model:curve#Nurbs").unwrap(),
        &source_association(),
        &mut transfer,
    )
    .expect("one geometry copy");
    assert_eq!(transfer.curves.len(), 1);
    assert_eq!(
        transfer.curves[index].geometry,
        cadmpeg_ir::geometry::CurveGeometry::Solved(
            cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(nurbs)
        )
    );
}

#[test]
fn transferred_nurbs_surface_keeps_one_geometry_copy() {
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    let axis = || {
        NurbsSurfaceAxis::new(
            1,
            vec![
                FiniteReal::ZERO,
                FiniteReal::ZERO,
                FiniteReal::ONE,
                FiniteReal::ONE,
            ],
            false,
        )
    };
    let nurbs = NurbsSurface::from_finite_lanes(
        axis(),
        axis(),
        NurbsSurfaceLanes::new(
            vec![
                vec![FinitePoint3::ZERO, FinitePoint3::ZERO],
                vec![FinitePoint3::ZERO, FinitePoint3::ZERO],
            ],
            None,
        ),
        false,
    )
    .unwrap();
    let surface = TextSurface::Nurbs(nurbs.clone());
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut curves = CurveTransfer::default();
    let mut transfer = SurfaceTransfer::default();
    let index = append_text_surface(
        &ctx,
        &surface,
        SurfaceId::mint("fcstd:model:surface#Nurbs").unwrap(),
        &source_association(),
        &mut curves,
        &mut transfer,
    )
    .expect("one geometry copy");
    assert_eq!(transfer.surfaces.len(), 1);
    assert_eq!(
        transfer.surfaces[index].geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(
            cadmpeg_ir::geometry::SolvedSurfaceGeometry::Nurbs(nurbs)
        )
    );
}

fn shape_property(xml: &str) -> PropertyRecord {
    PropertyRecord {
        id: "fcstd:native:property#Owner:Shape".into(),
        owner: "fcstd:native:object#Owner".into(),
        name: "Shape".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: PropertyFamily::Geometry,
        status: None,
        body: PropertyBody::Transient,
        order: 0,
        xml: RetainedXml::from_text(xml.into(), 0).expect("test XML span"),
    }
}

#[test]
fn uppercase_binary_shape_extension_selects_binary_reader() {
    let property = shape_property("<Property><Part file=\"Body.BIN\"/></Property>");
    let entry = crate::test_support::entry_record(
        "fcstd:native:entry#Body.BIN".into(),
        "Body.BIN".into(),
        cadmpeg_core::container::ContainerRole::Brep,
        Vec::new(),
        b"garbage\n".to_vec(),
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is within policy");
    let error = super::super::parse_payloads(&ctx, &[property], &[entry])
        .expect_err("malformed binary payload");
    assert!(error
        .to_string()
        .contains("unsupported binary B-rep header"));
}

fn source_association() -> SourceObjectAssociation {
    SourceObjectAssociation {
        geometry_role: None,
        format: cadmpeg_ir::CodecFormat::Fcstd,
        object_id: cadmpeg_core::text::NonBlankString::new("Owner").expect("nonblank object"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    }
}
