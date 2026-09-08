//! Carrier admission tests for shape binders.

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn distinguishes_absent_and_malformed_shape_binder_carriers() {
    fn document(mutation: Option<(&str, &str, &str)>) -> String {
        let choose = |object: &str, name: &str, default: &str| {
            mutation
                .filter(|(target_object, target_name, _)| {
                    *target_object == object && *target_name == name
                })
                .map_or_else(
                    || default.to_owned(),
                    |(_, _, replacement)| replacement.to_owned(),
                )
        };
        let shape_binder = [
            r#"<Property name="Support" type="App::PropertyLinkSubListGlobal"><LinkSubList count="1"><Link obj="Source" sub="Face1"/></LinkSubList></Property>"#.to_owned(),
            choose(
                "ShapeBind",
                "TraceSupport",
                r#"<Property name="TraceSupport" type="App::PropertyBool"><Bool value="true"/></Property>"#,
            ),
        ]
        .join("");
        let subshape_binder = [
            r#"<Property name="Support" type="App::PropertyXLinkSubList"><XLinkSubList count="1"><XLink name="Source" sub="Edge1"/></XLinkSubList></Property>"#.to_owned(),
            choose(
                "SubBind",
                "Fuse",
                r#"<Property name="Fuse" type="App::PropertyBool"><Bool value="true"/></Property>"#,
            ),
            choose(
                "SubBind",
                "MakeFace",
                r#"<Property name="MakeFace" type="App::PropertyBool"><Bool value="false"/></Property>"#,
            ),
            choose(
                "SubBind",
                "Offset",
                r#"<Property name="Offset" type="App::PropertyFloat"><Float value="-2.5"/></Property>"#,
            ),
            choose(
                "SubBind",
                "OffsetJoinType",
                r#"<Property name="OffsetJoinType" type="App::PropertyEnumeration"><Integer value="2"/></Property>"#,
            ),
            choose(
                "SubBind",
                "OffsetFill",
                r#"<Property name="OffsetFill" type="App::PropertyBool"><Bool value="true"/></Property>"#,
            ),
            choose(
                "SubBind",
                "OffsetOpenResult",
                r#"<Property name="OffsetOpenResult" type="App::PropertyBool"><Bool value="true"/></Property>"#,
            ),
            choose(
                "SubBind",
                "OffsetIntersection",
                r#"<Property name="OffsetIntersection" type="App::PropertyBool"><Bool value="true"/></Property>"#,
            ),
            choose(
                "SubBind",
                "ClaimChildren",
                r#"<Property name="ClaimChildren" type="App::PropertyBool"><Bool value="true"/></Property>"#,
            ),
            choose(
                "SubBind",
                "Relative",
                r#"<Property name="Relative" type="App::PropertyBool"><Bool value="false"/></Property>"#,
            ),
            choose(
                "SubBind",
                "BindMode",
                r#"<Property name="BindMode" type="App::PropertyEnumeration"><Integer value="1"/></Property>"#,
            ),
            choose(
                "SubBind",
                "PartialLoad",
                r#"<Property name="PartialLoad" type="App::PropertyBool"><Bool value="true"/></Property>"#,
            ),
            choose(
                "SubBind",
                "BindCopyOnChange",
                r#"<Property name="BindCopyOnChange" type="App::PropertyEnumeration"><Integer value="2"/></Property>"#,
            ),
            choose(
                "SubBind",
                "Refine",
                r#"<Property name="Refine" type="App::PropertyBool"><Bool value="false"/></Property>"#,
            ),
        ]
        .join("");
        format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="Part::Box" name="Source" id="1"/>
 <Object type="PartDesign::ShapeBinder" name="ShapeBind" id="2"/>
 <Object type="PartDesign::SubShapeBinder" name="SubBind" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Source"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="20"/></Property><Property name="Width" type="App::PropertyLength"><Float value="20"/></Property><Property name="Height" type="App::PropertyLength"><Float value="5"/></Property></Properties></Object>
 <Object name="ShapeBind"><Properties Count="{shape_count}">{shape_binder}</Properties></Object>
 <Object name="SubBind"><Properties Count="{subshape_count}">{subshape_binder}</Properties></Object>
</ObjectData></Document>"#,
            shape_count = shape_binder.matches("<Property ").count(),
            subshape_count = subshape_binder.matches("<Property ").count(),
        )
    }

    fn decode(document: &str) -> cadmpeg_ir::codec::DecodeResult {
        FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("shape binder carriers")
    }

    fn definition<'a>(
        result: &'a cadmpeg_ir::codec::DecodeResult,
        name: &str,
    ) -> &'a FeatureDefinition {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    }

    fn assert_native(result: &cadmpeg_ir::codec::DecodeResult, name: &str, kind: &str) {
        let actual = match definition(result, name) {
            FeatureDefinition::PostProcess { operation, .. } => operation.as_ref(),
            other => other,
        };
        assert!(
            matches!(
                actual,
                FeatureDefinition::Native { kind: value, .. } if value.as_str() == kind
            ),
            "{name} expected native {kind}, got {actual:?}"
        );
        assert_eq!(result.report().losses.len(), 1);
        assert!(result.report().losses.iter().all(|loss| {
            loss.code.namespace() == "fcstd"
                && loss.code.local_code() == "feature.native-kind-retained"
                && loss.severity == cadmpeg_ir::Severity::Blocking
        }));
    }

    let shape_absent = decode(&document(Some(("ShapeBind", "TraceSupport", ""))));
    assert!(matches!(
        definition(&shape_absent, "ShapeBind"),
        FeatureDefinition::Binder {
            construction: cadmpeg_ir::features::BinderConstruction::Shape {
                trace_support: false
            },
            ..
        }
    ));
    assert!(shape_absent.report().losses.is_empty());

    for name in [
        "Fuse",
        "MakeFace",
        "Offset",
        "OffsetJoinType",
        "OffsetFill",
        "OffsetOpenResult",
        "OffsetIntersection",
        "ClaimChildren",
        "Relative",
        "BindMode",
        "PartialLoad",
        "BindCopyOnChange",
        "Refine",
    ] {
        let result = decode(&document(Some(("SubBind", name, ""))));
        let operation = match definition(&result, "SubBind") {
            FeatureDefinition::PostProcess { operation, .. } => operation.as_ref(),
            FeatureDefinition::Binder { .. } => definition(&result, "SubBind"),
            _ => panic!("subshape binder definition"),
        };
        let FeatureDefinition::Binder { construction, .. } = operation else {
            panic!("subshape binder")
        };
        let cadmpeg_ir::features::BinderConstruction::SubShape {
            lifecycle,
            placement,
            copy_on_change,
            claim_children,
            fuse,
            make_face,
            partial_load,
            refine,
            offset,
            ..
        } = construction
        else {
            panic!("subshape binder construction")
        };
        assert_eq!(
            *lifecycle,
            if name == "BindMode" {
                cadmpeg_ir::features::BinderLifecycle::Synchronized
            } else {
                cadmpeg_ir::features::BinderLifecycle::Frozen
            }
        );
        assert_eq!(
            *placement,
            if name == "Relative" {
                cadmpeg_ir::features::BinderPlacement::Relative
            } else {
                cadmpeg_ir::features::BinderPlacement::Global
            }
        );
        assert_eq!(
            *copy_on_change,
            if name == "BindCopyOnChange" {
                cadmpeg_ir::features::BinderCopyOnChange::Disabled
            } else {
                cadmpeg_ir::features::BinderCopyOnChange::Mutated
            }
        );
        assert_eq!(*claim_children, name != "ClaimChildren");
        assert_eq!(*fuse, name != "Fuse");
        assert_eq!(*make_face, name == "MakeFace");
        assert_eq!(*partial_load, name != "PartialLoad");
        assert_eq!(*refine, name == "Refine");
        if name == "Offset" {
            assert!(offset.is_none());
        } else {
            let offset = offset.as_ref().expect("selected offset");
            assert!((offset.distance.0 + 2.5).abs() <= f64::EPSILON);
            assert_eq!(
                offset.join,
                if name == "OffsetJoinType" {
                    cadmpeg_ir::features::BinderOffsetJoin::Arcs
                } else {
                    cadmpeg_ir::features::BinderOffsetJoin::Intersection
                }
            );
            assert_eq!(offset.fill, name != "OffsetFill");
            assert_eq!(offset.open_result, name != "OffsetOpenResult");
            assert_eq!(offset.intersection, name != "OffsetIntersection");
        }
        assert!(result.report().losses.is_empty(), "{name}");
    }

    let malformed_bools = [
        ("App::PropertyString", r#"<String value="true"/>"#),
        ("App::PropertyInteger", r#"<Integer value="1"/>"#),
        ("App::PropertyBool", r#"<Bool value="bad"/>"#),
        (
            "App::PropertyBool",
            r#"<Wrapper><Bool value="true"/></Wrapper>"#,
        ),
        (
            "App::PropertyBool",
            r#"<Bool value="false"/><Bool value="true"/>"#,
        ),
        ("App::PropertyBool", r#"<Bool value="1"/>"#),
        ("App::PropertyBool", r#"<Bool value="2"/>"#),
    ];
    for object in ["ShapeBind", "SubBind"] {
        let names = if object == "ShapeBind" {
            vec!["TraceSupport"]
        } else {
            vec![
                "Fuse",
                "MakeFace",
                "OffsetFill",
                "OffsetOpenResult",
                "OffsetIntersection",
                "ClaimChildren",
                "Relative",
                "PartialLoad",
                "Refine",
            ]
        };
        for name in names {
            for (type_name, value) in malformed_bools {
                let replacement =
                    format!(r#"<Property name="{name}" type="{type_name}">{value}</Property>"#);
                let result = decode(&document(Some((object, name, &replacement))));
                assert_native(
                    &result,
                    if object == "ShapeBind" {
                        "ShapeBind"
                    } else {
                        "SubBind"
                    },
                    if object == "ShapeBind" {
                        "PartDesign::ShapeBinder"
                    } else {
                        "PartDesign::SubShapeBinder"
                    },
                );
            }
        }
    }

    let malformed_enumerations = [
        ("App::PropertyInteger", r#"<Integer value="1"/>"#),
        ("App::PropertyEnumeration", r#"<Integer value="bad"/>"#),
        (
            "App::PropertyEnumeration",
            r#"<Wrapper><Integer value="1"/></Wrapper>"#,
        ),
        (
            "App::PropertyEnumeration",
            r#"<Integer value="0"/><Integer value="1"/>"#,
        ),
        ("App::PropertyEnumeration", r#"<Integer value="-1"/>"#),
        ("App::PropertyEnumeration", r#"<Integer value="99"/>"#),
    ];
    for name in ["BindMode", "BindCopyOnChange", "OffsetJoinType"] {
        for (type_name, value) in malformed_enumerations {
            let replacement =
                format!(r#"<Property name="{name}" type="{type_name}">{value}</Property>"#);
            let result = decode(&document(Some(("SubBind", name, &replacement))));
            assert_native(&result, "SubBind", "PartDesign::SubShapeBinder");
        }
    }

    let malformed_offset = [
        ("App::PropertyLength", r#"<Float value="-2.5"/>"#),
        ("App::PropertyFloat", r#"<Float value="bad"/>"#),
        ("App::PropertyFloat", r#"<Float value="NaN"/>"#),
        (
            "App::PropertyFloat",
            r#"<Wrapper><Float value="-2.5"/></Wrapper>"#,
        ),
        (
            "App::PropertyFloat",
            r#"<Float value="-2.5"/><Float value="-1.0"/>"#,
        ),
    ];
    for (type_name, value) in malformed_offset {
        let replacement =
            format!(r#"<Property name="Offset" type="{type_name}">{value}</Property>"#);
        let result = decode(&document(Some(("SubBind", "Offset", &replacement))));
        assert_native(&result, "SubBind", "PartDesign::SubShapeBinder");
    }
}
