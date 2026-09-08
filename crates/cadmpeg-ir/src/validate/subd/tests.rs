// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::geometry::{Curve, CurveGeometry};
use crate::ids::CurveId;
use crate::provenance::SourceObjectAssociation;
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
            object_id: crate::products::NonEmptyString::new("00000000-0000-0000-0000-000000000000")
                .expect("nonempty source identity"),
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
