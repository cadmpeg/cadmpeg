// SPDX-License-Identifier: Apache-2.0
//! A known bad cache cannot claim a complete typed control; missing caches retain identity.

use super::parameter_dimension;
use crate::history::tests::feature;
use cadmpeg_ir::features::{Dimension, ParameterValue};
use cadmpeg_ir::scalar::{Length, PositiveLength};
use std::collections::BTreeMap;

#[test]
fn missing_cache_retains_the_named_parameter_binding() {
    let mut source = feature("extrusion", Some("1"), 0);
    source
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Depth"), "Width * 2".into());
    let binding = parameter_dimension::<PositiveLength>(&source, &["Depth"]);
    assert!(matches!(binding, Some(Dimension::Parameter(_))));
    source.dimension_properties.insert(
        "Depth".into(),
        BTreeMap::from([(cadmpeg_core::nonblank_literal!("Value"), "10mm".into())]),
    );
    assert_eq!(
        parameter_dimension::<PositiveLength>(&source, &["Depth"]),
        binding
    );
}

#[test]
fn inadmissible_source_cache_does_not_complete_a_positive_length() {
    let mut source = feature("extrusion", Some("1"), 0);
    source
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Depth"), "Width * 2".into());
    for cache in ["0mm", "-1mm", "NaNmm", "infmm"] {
        source.dimension_properties.insert(
            "Depth".into(),
            BTreeMap::from([(cadmpeg_core::nonblank_literal!("Value"), cache.into())]),
        );
        assert!(
            parameter_dimension::<PositiveLength>(&source, &["Depth"]).is_none(),
            "{cache}"
        );
    }
    source.dimension_properties.clear();
    for (literal, value) in [("0mm", 0.0), ("-1mm", -1.0)] {
        source
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("Depth"), literal.into());
        assert!(parameter_dimension::<PositiveLength>(&source, &["Depth"]).is_none());
        assert_eq!(
            crate::history::parameters::source_parameter_value(&source, "Depth"),
            Some(ParameterValue::Length(
                Length::new(value).expect("finite source length")
            ))
        );
    }
    for literal in ["NaNmm", "infmm", "-infrad"] {
        source
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("Depth"), literal.into());
        assert!(
            parameter_dimension::<PositiveLength>(&source, &["Depth"]).is_none(),
            "{literal}"
        );
        assert_eq!(source.parameters["Depth"], literal);
    }
}
