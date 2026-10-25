// SPDX-License-Identifier: Apache-2.0
//! Standalone projected carriers require a real typed native owner.

use crate::examples::unit_cube;
use crate::ids::Identity;
use crate::{CadIr, NativeRecord};

fn linked_carriers() -> CadIr {
    let mut ir = unit_cube().unwrap();
    let mut point = ir.model.points[0].clone();
    point.id = "test:model:point#standalone".try_into().unwrap();
    point.source_object = None;
    point.native_ref = Some("test:native:record#point".into());
    ir.model.points.push(point);
    let mut curve = ir.model.curves[0].clone();
    curve.id = "test:model:curve#standalone".try_into().unwrap();
    curve.source_object = None;
    curve.native_ref = Some("test:native:record#curve".into());
    ir.model.curves.push(curve);
    let mut surface = ir.model.surfaces[0].clone();
    surface.id = "test:model:surface#standalone".try_into().unwrap();
    surface.source_object = None;
    surface.native_ref = Some("test:native:record#surface".into());
    ir.model.surfaces.push(surface);
    ir
}

fn findings(ir: &CadIr) -> Vec<crate::report::check::Finding> {
    let mut findings = Vec::new();
    super::super::check_carrier_reachability(
        &cadmpeg_test_support::service_decode_context(),
        crate::native::view::NativeView::new(ir, None),
        &mut findings,
    )
    .unwrap();
    findings
}

#[test]
fn actual_typed_native_producers_root_standalone_points_curves_and_surfaces() {
    let mut ir = linked_carriers();
    let records = ["point", "curve", "surface"]
        .into_iter()
        .map(|kind| {
            NativeRecord::new(
                Identity::new(format!("test:native:record#{kind}")).unwrap(),
                serde_json::Map::from_iter([("value".into(), 1.into())]),
            )
            .unwrap()
        })
        .collect();
    ir.native
        .namespace_mut("test")
        .arenas_mut()
        .insert("records".into(), records);
    assert!(findings(&ir).is_empty());
}

#[test]
fn missing_and_opaque_native_references_do_not_root_standalone_carriers() {
    let mut ir = linked_carriers();
    for opaque in [false, true] {
        if opaque {
            let records = ["point", "curve", "surface"]
                .into_iter()
                .map(|kind| crate::unknown::NativeUnknownRecord {
                    id: crate::ids::UnknownId::mint(format!("test:native:record#{kind}")).unwrap(),
                    links: Vec::new(),
                })
                .collect::<Vec<_>>();
            ir.set_native_unknowns(
                &cadmpeg_test_support::service_decode_context(),
                "test",
                &records,
            )
            .unwrap();
        }
        let findings = findings(&ir);
        assert_eq!(findings.len(), 3, "opaque={opaque}: {findings:?}");
        for kind in ["point", "curve", "surface"] {
            assert!(findings.iter().any(|finding| finding.entity.as_deref()
                == Some(format!("test:model:{kind}#standalone").as_str())));
        }
    }
}
