// SPDX-License-Identifier: Apache-2.0
//! `SolidWorks` Keywords XML feature history.

mod bind;
mod classify;
mod configuration;
mod encode;
mod hash;
mod literals;
mod parameters;
mod project;
mod selections;
mod write;

pub(crate) use bind::*;
pub(crate) use classify::*;
pub(crate) use configuration::*;
pub(crate) use hash::*;
pub(crate) use literals::*;
pub(crate) use parameters::*;
pub(crate) use project::*;
pub(crate) use selections::*;
pub(crate) use write::*;

use crate::container::ContainerScan;
use crate::records::{Configuration, Feature, FeatureContent, FeatureHistory, HistoryContent};
#[allow(unused_imports)]
use cadmpeg_ir::annotations::Annotations;
#[allow(unused_imports)]
use cadmpeg_ir::attributes::AttributeValue;
#[allow(unused_imports)]
use cadmpeg_ir::features::{
    Angle, AngularTermination, BooleanOp, ChamferSpec, ConfigurationBodies, ConfigurationId,
    CosmeticThreadExtent, DatumPlaneReference, DesignConfiguration, DesignParameter,
    DimensionDisplay, EdgeSelection, ExtrudeExtent, ExtrudeSide, FaceSelection, FeatureDefinition,
    FeatureId, FeatureSourceContent, FeatureTreeNodeRole, HoleBottom, HoleKind, Length,
    LinearTermination, ParameterId, ParameterValue, PathRef, ProfileRef, RadiusSpec,
    RevolutionConstruction, RevolveExtent, RibConstruction, SplitFaceTool,
};
#[allow(unused_imports)]
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
#[allow(unused_imports)]
use cadmpeg_ir::math::{Point3, Vector3};
#[allow(unused_imports)]
use cadmpeg_ir::topology::Face;
use cadmpeg_ir::Exactness;
#[allow(unused_imports)]
use std::collections::{BTreeMap, HashMap, HashSet};

pub fn histories(scan: &ContainerScan, annotations: &mut Annotations) -> Vec<FeatureHistory> {
    scan.sections()
        .filter_map(|section| {
            let source = section.ordinal();
            let text = crate::container::xml_text(section.payload())?;
            let doc = roxmltree::Document::parse(&text).ok()?;
            let root = doc.root_element();
            if !root.tag_name().name().contains("Keywords") {
                return None;
            }
            let stream = section.display_name();
            let parent = format!("sldprt:history:feature-history#{source}");
            let configurations = root
                .children()
                .filter(|node| node.is_element() && node.tag_name().name() == "Configuration")
                .enumerate()
                .map(|(ordinal, node)| {
                    let id = format!("sldprt:history:configuration#{source}:{ordinal}");
                    crate::annotations::note(
                        annotations,
                        id.clone(),
                        stream.clone(),
                        node.range().start as u64,
                        "Configuration",
                        Exactness::ByteExact,
                    );
                    Configuration {
                        id,
                        parent: parent.clone(),
                        ordinal: ordinal as u32,
                        source_index: node
                            .attribute("SourceIndex")
                            .and_then(|value| value.parse().ok()),
                        name: node.attribute("Name").unwrap_or("").into(),
                        material: node
                            .attribute("Material")
                            .filter(|value| !value.is_empty())
                            .map(str::to_string),
                        properties: node
                            .attributes()
                            .filter(|attribute| {
                                !matches!(attribute.name(), "Name" | "Material" | "SourceIndex")
                            })
                            .map(|attribute| {
                                (attribute.name().to_string(), attribute.value().to_string())
                            })
                            .collect(),
                    }
                })
                .collect();
            let feature_nodes = root
                .descendants()
                .filter(|node| {
                    node.is_element()
                        && !matches!(
                            node.tag_name().name(),
                            "Keywords" | "Configuration" | "Dimension"
                        )
                })
                .collect::<Vec<_>>();
            let feature_ids = feature_nodes
                .iter()
                .enumerate()
                .map(|(ordinal, node)| {
                    (
                        node.range().start,
                        format!("sldprt:history:feature#{source}:{ordinal}"),
                    )
                })
                .collect::<HashMap<_, _>>();
            let features = feature_nodes
                .into_iter()
                .enumerate()
                .map(|(ordinal, node)| {
                    let id = feature_ids[&node.range().start].clone();
                    crate::annotations::note(
                        annotations,
                        id.clone(),
                        stream.clone(),
                        node.range().start as u64,
                        node.tag_name().name(),
                        Exactness::ByteExact,
                    );
                    Feature {
                        id,
                        parent: parent.clone(),
                        xml_tag: node.tag_name().name().into(),
                        tree_parent: node
                            .ancestors()
                            .skip(1)
                            .find_map(|ancestor| feature_ids.get(&ancestor.range().start).cloned()),
                        source_id: node
                            .attribute("id")
                            .filter(|value| !value.is_empty())
                            .map(str::to_string),
                        parent_source_id: node
                            .ancestors()
                            .skip(1)
                            .find(|ancestor| feature_ids.contains_key(&ancestor.range().start))
                            .and_then(|parent| parent.attribute("id"))
                            .map(str::to_string),
                        ordinal: ordinal as u32,
                        name: node.attribute("Name").unwrap_or("").into(),
                        kind: node
                            .attribute("Type")
                            .unwrap_or_else(|| node.tag_name().name())
                            .into(),
                        input_class: None,
                        suppressed: node
                            .attribute("Suppressed")
                            .is_some_and(|value| matches!(value, "1" | "true" | "True")),
                        parameters: node
                            .children()
                            .filter(|child| {
                                child.is_element() && child.tag_name().name() == "Dimension"
                            })
                            .filter_map(|dimension| {
                                Some((
                                    dimension.attribute("Name")?.into(),
                                    dimension.text().unwrap_or_default().trim().into(),
                                ))
                            })
                            .collect::<BTreeMap<_, _>>(),
                        dimension_properties: node
                            .children()
                            .filter(|child| {
                                child.is_element() && child.tag_name().name() == "Dimension"
                            })
                            .filter_map(|dimension| {
                                let name = dimension.attribute("Name")?;
                                let properties = dimension
                                    .attributes()
                                    .filter(|attribute| attribute.name() != "Name")
                                    .map(|attribute| {
                                        (
                                            attribute.name().to_string(),
                                            attribute.value().to_string(),
                                        )
                                    })
                                    .collect::<BTreeMap<_, _>>();
                                (!properties.is_empty()).then(|| (name.into(), properties))
                            })
                            .collect(),
                        properties: node
                            .attributes()
                            .filter(|attribute| {
                                !matches!(attribute.name(), "id" | "Name" | "Type" | "Suppressed")
                            })
                            .map(|attribute| {
                                (attribute.name().to_string(), attribute.value().to_string())
                            })
                            .collect(),
                        text: (!node.children().any(|child| child.is_element()))
                            .then(|| node.text().map(str::trim).unwrap_or_default().to_string())
                            .filter(|value| !value.is_empty()),
                        content: node
                            .children()
                            .filter_map(|child| {
                                if child.is_text() {
                                    let value = child.text()?.trim();
                                    return (!value.is_empty())
                                        .then(|| FeatureContent::Text(value.into()));
                                }
                                if !child.is_element() {
                                    return None;
                                }
                                if child.tag_name().name() == "Dimension" {
                                    return child
                                        .attribute("Name")
                                        .map(|name| FeatureContent::Dimension(name.into()));
                                }
                                feature_ids
                                    .get(&child.range().start)
                                    .cloned()
                                    .map(FeatureContent::Feature)
                            })
                            .collect(),
                    }
                })
                .collect::<Vec<_>>();
            let configuration_ids = root
                .children()
                .filter(|node| node.is_element() && node.tag_name().name() == "Configuration")
                .enumerate()
                .map(|(ordinal, node)| {
                    (
                        node.range().start,
                        format!("sldprt:history:configuration#{source}:{ordinal}"),
                    )
                })
                .collect::<HashMap<_, _>>();
            let content = root
                .children()
                .filter_map(|child| {
                    if child.is_text() {
                        let value = child.text()?.trim();
                        return (!value.is_empty()).then(|| HistoryContent::Text(value.into()));
                    }
                    if !child.is_element() {
                        return None;
                    }
                    configuration_ids
                        .get(&child.range().start)
                        .cloned()
                        .map(HistoryContent::Configuration)
                        .or_else(|| {
                            feature_ids
                                .get(&child.range().start)
                                .cloned()
                                .map(HistoryContent::Feature)
                        })
                })
                .collect();
            let id = parent;
            crate::annotations::note(
                annotations,
                id.clone(),
                stream,
                0,
                "Keywords",
                Exactness::ByteExact,
            );
            Some(FeatureHistory {
                id,
                part_name: root
                    .attribute("Name")
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
                properties: root
                    .attributes()
                    .filter(|attribute| attribute.name() != "Name")
                    .map(|attribute| (attribute.name().to_string(), attribute.value().to_string()))
                    .collect(),
                content,
                configurations,
                features,
            })
        })
        .collect()
}

pub(crate) fn enrich_scene_classes(
    histories: &mut [FeatureHistory],
    scene_classes: &crate::tessellation::SceneFeatureClasses,
) {
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        let Some(source) = feature.source_id.as_deref() else {
            continue;
        };
        if feature.input_class.is_none() && classless_builtin_node(feature) {
            feature.input_class = scene_classes.by_source.get(source).cloned();
        }
    }
}

#[cfg(test)]
mod literal_tests {
    use super::*;

    #[test]
    fn native_scalar_literals_are_compact_and_bit_exact() {
        for value in [
            0.0,
            -0.0,
            0.125,
            -42.5,
            7.745_183_829_698_638e-127,
            -5.486_124_068_793_69e307,
        ] {
            let literal = format_f64_literal(value);
            let parsed = literal.parse::<f64>().expect("required invariant");
            assert_eq!(parsed.to_bits(), value.to_bits(), "{literal}");
        }
        assert_eq!(format_f64_literal(0.125), "0.125");
        assert_eq!(
            format_f64_literal(7.745_183_829_698_638e-127),
            "7.745183829698638e-127"
        );
    }

    #[test]
    fn solidworks_length_units_convert_to_millimeters() {
        for (literal, expected) in [
            ("1A", 1.0e-7),
            ("1Å", 1.0e-7),
            ("1nm", 1.0e-6),
            ("1um", 1.0e-3),
            ("1µm", 1.0e-3),
            ("1μm", 1.0e-3),
            ("1mm", 1.0),
            ("1cm", 10.0),
            ("1m", 1000.0),
            ("1uin", 25.4e-6),
            ("1mil", 0.0254),
            ("1in", 25.4),
            ("1ft", 304.8),
        ] {
            assert_eq!(parse_length_mm(literal), Some(expected), "{literal}");
        }
    }

    #[test]
    fn diameter_display_literals_participate_in_expressions() {
        let aliases = std::collections::HashMap::new();
        let values = std::collections::HashMap::new();
        assert_eq!(
            ParameterExpressionParser::new_flat("<MOD-DIAM>4mm / 2", &aliases, &values).parse(),
            Some(ParameterValue::Length(cadmpeg_ir::features::Length(2.0)))
        );
        assert_eq!(
            ParameterExpressionParser::new_flat("<MOD-DIAM>4 + 1mm", &aliases, &values).parse(),
            Some(ParameterValue::Length(cadmpeg_ir::features::Length(5.0)))
        );
        assert_eq!(
            ParameterExpressionParser::new_flat("&lt;MOD-DIAM&gt;4mm / 2", &aliases, &values,)
                .parse(),
            Some(ParameterValue::Length(cadmpeg_ir::features::Length(2.0)))
        );
        assert_eq!(
            parse_parameter_literal("&lt;MOD-DIAM&gt;4.917"),
            Some(ParameterValue::Length(cadmpeg_ir::features::Length(4.917)))
        );
    }

    #[test]
    fn radius_display_literals_participate_in_expressions() {
        let aliases = std::collections::HashMap::new();
        let values = std::collections::HashMap::new();
        assert_eq!(
            ParameterExpressionParser::new_flat("<MOD-RHO>4mm / 2", &aliases, &values).parse(),
            Some(ParameterValue::Length(cadmpeg_ir::features::Length(2.0)))
        );
        assert_eq!(
            ParameterExpressionParser::new_flat("&lt;MOD-RHO&gt;4 + 1mm", &aliases, &values,)
                .parse(),
            Some(ParameterValue::Length(cadmpeg_ir::features::Length(5.0)))
        );
        assert_eq!(
            parse_parameter_literal("<MOD-RHO>0.5"),
            Some(ParameterValue::Length(cadmpeg_ir::features::Length(0.5)))
        );
        assert_eq!(
            dimension_display("&lt;MOD-RHO&gt;0.5"),
            Some(DimensionDisplay::Radius)
        );
    }

    #[test]
    fn dimension_decorations_preserve_the_nominal_scalar() {
        let aliases = std::collections::HashMap::new();
        let values = std::collections::HashMap::new();
        for (expression, expected, display) in [
            ("2X<MOD-DIAM>1.2", 1.2, DimensionDisplay::Diameter),
            ("6XR2", 2.0, DimensionDisplay::Radius),
            ("<MOD-DIAM>15H7", 15.0, DimensionDisplay::Diameter),
            ("3x &lt;MOD-RHO&gt;0.5", 0.5, DimensionDisplay::Radius),
        ] {
            assert_eq!(
                parse_parameter_literal(expression),
                Some(ParameterValue::Length(cadmpeg_ir::features::Length(
                    expected
                ))),
                "{expression}"
            );
            assert_eq!(dimension_display(expression), Some(display), "{expression}");
            assert_eq!(
                ParameterExpressionParser::new_flat(expression, &aliases, &values).parse(),
                Some(ParameterValue::Length(cadmpeg_ir::features::Length(
                    expected
                ))),
                "{expression}"
            );
        }
        assert_eq!(parse_parameter_literal("x2"), None);
        assert_eq!(parse_parameter_literal("15mmH7"), None);
        assert_eq!(parse_parameter_literal("<MOD-DIAM>15H"), None);
    }

    #[test]
    fn bare_native_text_is_distinct_from_scalar_expressions_and_references() {
        for text in ["M16x2.0", "740四件等高", "plain text"] {
            assert_eq!(
                bare_text_parameter_literal(text),
                Some(ParameterValue::String(text.into()))
            );
        }
        for expression in ["", "1 +", "width/2", "\"D1@Sketch1\"", "D12"] {
            assert_eq!(
                bare_text_parameter_literal(expression),
                None,
                "{expression}"
            );
        }
    }

    #[test]
    fn formatted_text_dimensions_are_strings_only_for_txd_parameters() {
        let text = "4X <MOD-DIAM> 12 <HOLE-DEPTH> 40<MOD-PM>.2";
        for (name, text) in [
            ("TXD5", text),
            ("TXD2", "30X <MOD-DIAM> 14<HOLE-SINK><MOD-DIAM> 20 X 90°"),
            ("TXD3", "<BORDER><MOD-DIAM>10 </BORDER>"),
            ("TXD7", "4X M12x1.75 <HOLE-DEPTH> 25<MOD-PM>.25"),
        ] {
            assert_eq!(
                formatted_text_dimension_literal(name, text),
                Some(ParameterValue::String(text.into())),
                "{name}"
            );
        }
        for name in ["D5", "TXD", "TXD5-extra"] {
            assert_eq!(formatted_text_dimension_literal(name, text), None, "{name}");
        }
        for malformed in ["1 +", "<MOD-DIAM", "MOD-DIAM>", "<>", "< >", "<<TAG>"] {
            assert_eq!(
                formatted_text_dimension_literal("TXD5", malformed),
                None,
                "{malformed}"
            );
        }
    }

    #[test]
    fn solidworks_sign_function_is_three_way() {
        for (argument, expected) in [(-2, -1), (0, 0), (2, 1)] {
            assert_eq!(
                apply_parameter_function("sgn", &ParameterValue::Integer(argument)),
                Some(ParameterValue::Integer(expected))
            );
        }
    }

    #[test]
    fn integer_function_preserves_discrete_integer_values() {
        for value in [i64::MIN, -(1_i64 << 53) - 1, (1_i64 << 53) + 1, i64::MAX] {
            assert_eq!(
                apply_parameter_function("int", &ParameterValue::Integer(value)),
                Some(ParameterValue::Integer(value))
            );
        }
        assert_eq!(
            apply_parameter_function("int", &ParameterValue::Real(-3.75)),
            Some(ParameterValue::Integer(-3))
        );
    }

    #[test]
    fn integer_powers_preserve_exact_exponent_parity() {
        let odd = ParameterValue::Integer((1_i64 << 53) + 1);
        assert_eq!(
            exponentiate_parameter_value(&ParameterValue::Integer(-1), &odd),
            Some(ParameterValue::Integer(-1))
        );
        assert_eq!(
            exponentiate_parameter_value(
                &ParameterValue::Integer(-1),
                &ParameterValue::Integer(-((1_i64 << 53) + 1)),
            ),
            Some(ParameterValue::Real(-1.0))
        );
        assert_eq!(
            exponentiate_parameter_value(&ParameterValue::Integer(2), &ParameterValue::Integer(-3),),
            Some(ParameterValue::Real(0.125))
        );
    }

    #[test]
    fn non_finite_parameter_literals_have_no_evaluated_value() {
        for literal in ["NaN", "inf", "-inf"] {
            assert_eq!(parse_parameter_literal(literal), None, "{literal}");
        }
    }

    #[test]
    fn bare_binary_digits_are_integer_parameters() {
        assert_eq!(
            parse_parameter_literal("0"),
            Some(ParameterValue::Integer(0))
        );
        assert_eq!(
            parse_parameter_literal("1"),
            Some(ParameterValue::Integer(1))
        );
        assert_eq!(
            parse_parameter_literal("true"),
            Some(ParameterValue::Boolean(true))
        );
        assert_eq!(
            parse_parameter_literal("false"),
            Some(ParameterValue::Boolean(false))
        );
    }

    #[test]
    fn native_scalars_accept_only_exact_integer_values() {
        let largest_consecutive = 1_i64 << 53;
        assert_eq!(exact_integer_f64(largest_consecutive), Some(2_f64.powi(53)));
        assert_eq!(exact_integer_f64(largest_consecutive + 1), None);
        assert_eq!(exact_integer_f64(i64::MIN), Some(i64::MIN as f64));
        assert_eq!(exact_integer_f64(i64::MAX), None);
    }

    #[test]
    fn mixed_numeric_comparisons_preserve_integer_identity() {
        let integer = ParameterValue::Integer((1_i64 << 53) + 1);
        let rounded_real = ParameterValue::Real(2_f64.powi(53));
        assert_eq!(
            compare_parameter_values(&integer, &rounded_real, "="),
            Some(false)
        );
        assert_eq!(
            compare_parameter_values(&integer, &rounded_real, ">"),
            Some(true)
        );
        assert_eq!(
            compare_parameter_values(&rounded_real, &integer, "<"),
            Some(true)
        );

        assert_eq!(
            compare_parameter_values(
                &ParameterValue::Integer(-3),
                &ParameterValue::Real(-3.5),
                ">",
            ),
            Some(true)
        );
        assert_eq!(
            compare_parameter_values(
                &ParameterValue::Integer(i64::MAX),
                &ParameterValue::Real(-(i64::MIN as f64)),
                "<",
            ),
            Some(true)
        );
    }

    #[test]
    fn expression_rewrite_quotes_hyphenated_identifiers() {
        let aliases = std::collections::HashMap::from([("Width".into(), "Wall-Gauge".into())]);
        assert_eq!(
            rewrite_parameter_expression("Width * 2", &aliases).as_deref(),
            Some("\"Wall-Gauge\" * 2")
        );
    }
}

#[cfg(test)]
mod tests;
