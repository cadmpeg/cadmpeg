// SPDX-License-Identifier: Apache-2.0
//! Parameter synthesis exercises owned, user and legacy semantic records.

use super::*;
use crate::records::parameters::{
    DesignParameterContent, DesignParameterDiscriminator, DesignParameterSource,
};

#[test]
fn semantic_parameter_families_encode_without_source_locations() {
    for (tag, kind, owner, discriminator, unit, value) in [
        (
            "256",
            "User Parameter",
            None,
            Some(DesignParameterDiscriminator::Code0),
            Some("mm"),
            2.5,
        ),
        (
            "258",
            "AlongDistance",
            Some(9),
            Some(DesignParameterDiscriminator::Code3),
            Some("cm"),
            -3.0,
        ),
        (
            "259",
            "Linear Dimension-2",
            Some(12),
            Some(DesignParameterDiscriminator::Code6),
            Some("mm"),
            4.0,
        ),
        ("260", "TaperAngle", Some(15), None, Some("rad"), 0.25),
        ("287", "Linear Dimension", Some(18), None, Some("mm"), 5.0),
    ] {
        let original = DesignParameter::authored(DesignParameterContent {
            id: "f3d:design:design-parameter#1".into(),
            class_tag: tag.to_owned().try_into().unwrap(),
            record_index: 1,
            source_ordinal: 0,
            source: DesignParameterSource::authored(kind.into(), owner, discriminator).unwrap(),
            expression: "d1 + 2".into(),
            unit: unit.map(str::to_owned),
            name: "d2".into(),
            evaluated_value: value,
        })
        .unwrap();
        assert_eq!(original.byte_offset(), None);
        let bytes = encode_document_parameters(std::slice::from_ref(&original)).unwrap();
        let parsed =
            crate::design::decode::parameters::parse_design_parameter_record(&bytes).unwrap();
        assert_eq!(parsed.class_tag, original.class_tag);
        assert_eq!(parsed.record_index, original.record_index);
        assert_eq!(parsed.source_ordinal, original.source_ordinal);
        assert_eq!(parsed.source_kind(), original.source_kind());
        assert_eq!(parsed.owner_record_index(), original.owner_record_index());
        assert_eq!(parsed.expression(), original.expression());
        assert_eq!(
            parsed.unit().map(|value| value.value.as_str()),
            original.unit().map(|value| value.value.as_str())
        );
        assert_eq!(parsed.name(), original.name());
        assert_eq!(parsed.evaluated_value(), original.evaluated_value());
    }
}
