// SPDX-License-Identifier: Apache-2.0
use crate::design::feature_project::scope_properties;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

#[test]
fn scope_properties_record_reference_members() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::Combine,
        10,
    );

    let properties = scope_properties(&scope, "f3d:Design/BulkStream.dat", &[], &[]).unwrap();
    assert_eq!(properties.len(), 1);
    assert_eq!(
        properties[&cadmpeg_core::nonblank_literal!("reference:0")],
        "10"
    );
}

#[test]
fn counted_assembly_owners_keep_source_parameter_expressions() {
    use crate::design::decode::parameters::parse_design_parameter_record;
    use crate::design::test_support::parameter_record;
    use crate::records::feature::assembly::{DesignAssemblyAlignment, DesignAssemblyOwnerRun};
    use crate::records::feature::scope::DesignScopePayloadMut;
    use crate::records::identity::Located;

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::Assemble,
        10,
    );
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(94),
        "-7.5 mm",
        "SlideMinimum",
        Some("mm"),
        "d8",
        -0.75,
    ))
    .unwrap();
    let mut alignment = DesignAssemblyAlignment::try_new(0.0, [0.0; 3], Vec::new(), None).unwrap();
    alignment.additional_owner_run = Some(DesignAssemblyOwnerRun {
        count: Located {
            value: 1,
            offset: 273,
        },
        references: vec![Located {
            value: 95,
            offset: 277,
        }],
    });
    if let DesignScopePayloadMut::Assemble(slot) = scope.payload_mut() {
        *slot = Some(alignment);
    }
    let properties =
        scope_properties(&scope, "f3d:Design/BulkStream.dat", &[], &[(4, &parameter)]).unwrap();
    assert_eq!(
        properties.get("parameter:d8").map(String::as_str),
        Some("-7.5 mm")
    );
    if let DesignScopePayloadMut::Assemble(slot) = scope.payload_mut() {
        slot.as_mut().unwrap().additional_owner_run = None;
    }
    let properties =
        scope_properties(&scope, "f3d:Design/BulkStream.dat", &[], &[(4, &parameter)]).unwrap();
    assert!(!properties.contains_key("parameter:d8"));
    if let DesignScopePayloadMut::Assemble(slot) = scope.payload_mut() {
        slot.as_mut().unwrap().additional_tail_reference = Some(Located {
            value: 63,
            offset: 290,
        });
    }
    let properties =
        scope_properties(&scope, "f3d:Design/BulkStream.dat", &[], &[(4, &parameter)]).unwrap();
    assert_eq!(
        properties.get("parameter:d8").map(String::as_str),
        Some("-7.5 mm")
    );
}

#[test]
fn hole_properties_retain_expressions_controls_cache_and_sizing_names() {
    use crate::design::decode::parameters::parse_design_parameter_record;
    use crate::design::test_support::parameter_record;
    use crate::records::feature::hole::{DesignHoleExtent, DesignHoleStandard};
    use crate::records::identity::Located;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        slot.extent = Some(DesignHoleExtent {
            hole_type: Some(Located {
                value: 2,
                offset: 5,
            }),
            leading_word: Some(Located {
                value: 2,
                offset: 1,
            }),
            extent_control: Some(1),
            saved_non_distance_depth: Some(Located {
                value: cadmpeg_ir::scalar::FiniteReal::new(0.75).unwrap(),
                offset: 2,
            }),
            direction_control: Some(Located {
                value: 0,
                offset: 3,
            }),
            standard: Some(DesignHoleStandard {
                offset: 4,
                standard: "ISO".into(),
                fit: "Close".into(),
                size: "M3".into(),
                fastener: "Socket".into(),
            }),
        });
    }
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(94),
        "10 mm",
        "HoleDepth",
        Some("mm"),
        "d4",
        1.0,
    ))
    .unwrap();
    let properties =
        scope_properties(&scope, "f3d:Design/BulkStream.dat", &[], &[(0, &parameter)]).unwrap();
    for (key, value) in [
        ("reference:0", "32"),
        ("parameter:d4", "10 mm"),
        ("hole:extent_leading_word", "2"),
        ("hole:type", "2"),
        ("hole:extent_control", "1"),
        ("hole:saved_non_distance_depth_cm", "0.75"),
        ("hole:direction_control", "0"),
        ("hole:standard", "ISO"),
        ("hole:fit", "Close"),
        ("hole:size", "M3"),
        ("hole:fastener", "Socket"),
    ] {
        assert_eq!(properties.get(key).map(String::as_str), Some(value));
    }
    assert_eq!(properties.len(), 11);
}

#[test]
fn a_duplicate_hole_parameter_name_keeps_both_expressions() {
    use crate::design::decode::parameters::parse_design_parameter_record;
    use crate::design::test_support::parameter_record;
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    let mut first = parse_design_parameter_record(&parameter_record(
        Some(94),
        "10 mm",
        "HoleDepth",
        Some("mm"),
        "d4",
        1.0,
    ))
    .unwrap();
    let mut second = parse_design_parameter_record(&parameter_record(
        Some(104),
        "4 mm",
        "HoleDiameter",
        Some("mm"),
        "d4",
        0.4,
    ))
    .unwrap();
    first.id = "synthetic:first".into();
    second.id = "synthetic:second".into();
    let properties = scope_properties(
        &scope,
        "f3d:Design/BulkStream.dat",
        &[],
        &[(0, &first), (1, &second)],
    )
    .unwrap();
    assert_eq!(
        properties.get("parameter:d4").map(String::as_str),
        Some("10 mm")
    );
    assert_eq!(
        properties
            .get("parameter-native:synthetic:second")
            .map(String::as_str),
        Some("4 mm")
    );
}
