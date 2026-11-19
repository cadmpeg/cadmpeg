// SPDX-License-Identifier: Apache-2.0
use crate::design::ids::{OperationId, SetupId, ToolId};
use crate::manufacturing::{
    ManufacturingOperation, ManufacturingParameter, ManufacturingSource, ManufacturingStrategy,
    ManufacturingTool,
};
use crate::CadIr;

#[test]
fn cam_manufacturing_wire_keeps_exact_parameter_text() {
    let parameter = ManufacturingParameter {
        name: "tool_diameter".into(),
        value: "6.000".into(),
        unit: Some("mm".into()),
        kind: Some("float".into()),
        native_xml: "<Parameter name='tool_diameter' value='6.000'/>".into(),
    };
    let mut ir = CadIr::empty();
    ir.manufacturing.tools.push(ManufacturingTool {
        id: ToolId::mint("test:cam:tool#1").unwrap(),
        number: Some("1".into()),
        tool_type: Some("flat end mill".into()),
        parameters: vec![parameter],
        holder: Vec::new(),
        vendor: None,
        product: None,
        source: ManufacturingSource {
            exactness: crate::provenance::Exactness::ByteExact,
            member: "Toolpath/Iron.Document/synthetic.irondoc".into(),
            start: 0,
            end: 7,
            native_xml: "<tool/>".into(),
        },
    });
    let encoded = serde_json::to_string(&ir).unwrap();
    assert_eq!(serde_json::from_str::<CadIr>(&encoded).unwrap(), ir);
    assert_eq!(ir.ir_version(), "11");
    assert!(encoded.contains("6.000"));
    assert!(encoded.contains("manufacturing"));
}

#[test]
fn cam_empty_section_preserves_existing_wire() {
    let encoded = serde_json::to_value(CadIr::empty()).unwrap();
    assert!(encoded.get("manufacturing").is_none());
    assert!(serde_json::from_value::<CadIr>(encoded.clone())
        .unwrap()
        .manufacturing
        .is_empty());
    let mut explicit_null = encoded;
    explicit_null["manufacturing"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<CadIr>(explicit_null).is_err());
}

#[test]
fn cam_dangling_setup_is_a_reference_finding() {
    let mut ir = CadIr::empty();
    ir.manufacturing.operations.push(ManufacturingOperation {
        id: OperationId::mint("test:cam:operation#1").unwrap(),
        setup: SetupId::mint("test:cam:setup#missing").unwrap(),
        order: 0,
        name: None,
        strategy: "parallel".into(),
        family: ManufacturingStrategy::Parallel,
        tool: None,
        parameters: Vec::new(),
        geometry: Vec::new(),
        computed_path_present: false,
        computed_path_valid: None,
        source: ManufacturingSource {
            exactness: crate::provenance::Exactness::ByteExact,
            member: "synthetic".into(),
            start: 0,
            end: 12,
            native_xml: "<operation/>".into(),
        },
    });
    let report = crate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(report.findings.iter().any(|finding| finding.check
        == crate::report::check::Check::ReferentialIntegrity
        && finding.entity.as_deref() == Some("test:cam:operation#1")));
    assert_eq!(ir.entity_count(), 1);
    assert!(!crate::diff::diff(&CadIr::empty(), &ir).is_empty());
}
