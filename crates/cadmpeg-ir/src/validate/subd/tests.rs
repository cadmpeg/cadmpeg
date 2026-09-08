// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::geometry::{Curve, CurveGeometry};
use crate::ids::CurveId;
use crate::provenance::SourceObjectAssociation;
use crate::topology::Color;
use crate::validate::validate_neutral;
use crate::CadIr;

#[test]
fn source_association_is_a_free_carrier_root() {
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: CurveId::mint("synthetic:source:curve#0").expect("valid identity"),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: Some(SourceObjectAssociation {
            format: crate::CodecFormat::Rhino,
            object_id: "00000000-0000-0000-0000-000000000000".into(),
            name: Some("curve".into()),
            color: None,
            visible: Some(true),
            layer: Some("layer-0".into()),
            instance_path: Vec::new(),
        }),
    });
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.is_ok(), "{:?}", report.findings);
    let parsed = CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap();
    assert_eq!(parsed, ir);
}

#[test]
fn source_association_rejects_out_of_range_color() {
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: CurveId::mint("synthetic:source:curve#color").expect("valid identity"),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: Some(SourceObjectAssociation {
            format: crate::CodecFormat::Rhino,
            object_id: "object".into(),
            name: None,
            color: Some(Color {
                r: 1.1,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }),
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        }),
    });
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("outside [0, 1]")));
}
