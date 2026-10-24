// SPDX-License-Identifier: Apache-2.0
//! Split feature property grammars.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::pmdc::{content_header, Cursor};

use super::{
    parse_enumeration, property, PmDcFeatureEnumFamily, PmDcFeaturePropertyKind,
    PmDcFeaturePropertyPayload, PropertyParser,
};

const SPLIT_ENUM_TYPE: [u8; 16] = [
    0x19, 0x90, 0xe1, 0xb8, 0xd3, 0x11, 0x46, 0x7b, 0xc0, 0x00, 0xe3, 0x95, 0x45, 0xdf, 0x72, 0x4f,
];
const TOOL_ENUM_TYPE: [u8; 16] = [
    0x17, 0x90, 0xe1, 0xb8, 0xd3, 0x11, 0x46, 0x7b, 0xc0, 0x00, 0xe3, 0x95, 0x45, 0xdf, 0x72, 0x4f,
];
const SPLIT_BOOLEAN_TYPE: [u8; 16] = [
    0x9f, 0x8f, 0x2a, 0x40, 0xd2, 0x11, 0x20, 0xe6, 0x60, 0x00, 0xaa, 0xab, 0xc8, 0x43, 0x3c, 0xb0,
];

pub(super) fn property_parser(type_id: [u8; 16]) -> Option<PropertyParser> {
    match type_id {
        SPLIT_ENUM_TYPE => Some(parse_split_enum),
        TOOL_ENUM_TYPE => Some(parse_tool_enum),
        SPLIT_BOOLEAN_TYPE => Some(parse_split_boolean),
        _ => None,
    }
}

fn parse_split_boolean(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let name_count = cursor.u32("split Boolean name count")?;
    let name_value = cursor.u32("split Boolean name value")?;
    if name_count != 0 || name_value != 0 {
        return Err(CodecError::malformed(
            "Inventor PmDc split Boolean requires an empty name and zero name value",
        ));
    }
    let raw = cursor.u8("split Boolean value")?;
    if raw > 1 {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc split Boolean value is {raw}"
        )));
    }
    cursor.finish("split Boolean")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value,
            value: raw != 0,
        },
    ))
}

fn parse_split_enum(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    checked_enumeration(source, version, 3)
}

fn parse_tool_enum(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    checked_enumeration(source, version, 4)
}

fn checked_enumeration(
    source: View<'_>,
    version: u8,
    expected: i16,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let property = parse_enumeration(source, version, PmDcFeatureEnumFamily::Auxiliary)?;
    if !matches!(
        property.kind,
        PmDcFeaturePropertyKind::Enumeration { type_value, .. } if type_value == expected
    ) {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc split enumeration requires type value {expected}"
        )));
    }
    Ok(property)
}

#[cfg(test)]
mod tests {
    use super::{SPLIT_BOOLEAN_TYPE, SPLIT_ENUM_TYPE, TOOL_ENUM_TYPE};
    use crate::feature::{
        feature_property_parser, PmDcFeatureEnumFamily, PmDcFeaturePropertyKind,
        PmDcFeaturePropertyPayload,
    };
    use crate::test_support::test_fixtures::{content_at, parse};
    use cadmpeg_core::CodecError;

    fn header(version: u8) -> Vec<u8> {
        let stamps: &[i32] = match version {
            15..=22 => &[],
            23 => &[-1],
            _ => &[-1, 7],
        };
        content_at(9, version, stamps)
    }

    fn enumeration(version: u8, type_value: i16, value: u16) -> Vec<u8> {
        let mut bytes = header(version);
        bytes.extend_from_slice(&type_value.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes
    }

    #[test]
    fn split_enumerations_keep_values_and_versioned_headers() {
        for version in [15, 16, 17, 18, 19, 20, 21, 22, 23, 25, 28, 29, 30, 31] {
            for (record_type, type_value) in [(SPLIT_ENUM_TYPE, 3), (TOOL_ENUM_TYPE, 4)] {
                let parser =
                    feature_property_parser(record_type).expect("registered enumeration parser");
                for value in [0, 1, 2, 3, u16::MAX] {
                    let bytes = enumeration(version, type_value, value);
                    let property = parse(&bytes, |ctx, view| parser(ctx, view, version))
                        .expect("valid enumeration");
                    assert_eq!(property.save_version_major, version);
                    assert_eq!(property.header.source_index, 9);
                    assert_eq!(
                        property.header.flags_extension,
                        (version >= 28).then_some(0)
                    );
                    let stamps: &[i32] = match version {
                        15..=22 => &[],
                        23 => &[-1],
                        _ => &[-1, 7],
                    };
                    assert_eq!(property.header.stamps, stamps);
                    assert_eq!(
                        property.kind,
                        PmDcFeaturePropertyKind::Enumeration {
                            family: PmDcFeatureEnumFamily::Auxiliary,
                            type_value,
                            value,
                        }
                    );
                    let wire = serde_json::to_value(&property).expect("serialize property");
                    assert_eq!(
                        serde_json::from_value::<PmDcFeaturePropertyPayload>(wire)
                            .expect("deserialize property"),
                        property
                    );
                }
            }
        }
    }

    #[test]
    fn split_enumerations_reject_wrong_types_truncation_and_trailing_bytes() {
        for (record_type, expected) in [(SPLIT_ENUM_TYPE, 3), (TOOL_ENUM_TYPE, 4)] {
            let parser =
                feature_property_parser(record_type).expect("registered enumeration parser");
            for version in [19, 23, 25, 28, 31] {
                for type_value in [i16::MIN, -expected, 0, 3, 4, i16::MAX] {
                    if type_value == expected {
                        continue;
                    }
                    let wrong = enumeration(version, type_value, 1);
                    assert!(matches!(
                        parse(&wrong, |ctx, view| parser(ctx, view, version)),
                        Err(CodecError::Malformed(_))
                    ));
                }
                let mut bytes = enumeration(version, expected, 1);
                for end in 0..bytes.len() {
                    let error = parse(&bytes, |ctx, view| {
                        parser(ctx, view.child(0, end).expect("record prefix"), version)
                    })
                    .expect_err("truncated enumeration");
                    assert!(matches!(error, CodecError::Truncated { .. }));
                }
                bytes.push(0);
                assert!(parse(&bytes, |ctx, view| parser(ctx, view, version)).is_err());
            }
        }
    }

    #[test]
    fn split_boolean_requires_the_empty_named_boolean_form() {
        let parser =
            feature_property_parser(SPLIT_BOOLEAN_TYPE).expect("registered Boolean parser");
        for version in [15, 22, 23, 25, 28, 31] {
            for raw in 0..=u8::MAX {
                let mut bytes = header(version);
                bytes.extend_from_slice(&0u32.to_le_bytes());
                bytes.extend_from_slice(&0u32.to_le_bytes());
                bytes.push(raw);
                let parsed = parse(&bytes, |ctx, view| parser(ctx, view, version));
                if raw > 1 {
                    assert!(parsed.is_err());
                    continue;
                }
                let property = parsed.expect("valid Boolean");
                assert_eq!(property.save_version_major, version);
                assert_eq!(property.header.source_index, 9);
                assert_eq!(
                    property.header.flags_extension,
                    (version >= 28).then_some(0)
                );
                let stamps: &[i32] = match version {
                    15..=22 => &[],
                    23 => &[-1],
                    _ => &[-1, 7],
                };
                assert_eq!(property.header.stamps, stamps);
                assert_eq!(
                    property.kind,
                    PmDcFeaturePropertyKind::Boolean {
                        name: String::new(),
                        name_value: 0,
                        value: raw == 1,
                    }
                );
                bytes.pop();
                assert!(matches!(
                    parse(&bytes, |ctx, view| parser(ctx, view, version)),
                    Err(CodecError::Truncated { .. })
                ));
                bytes.extend_from_slice(&[raw, 0]);
                assert!(parse(&bytes, |ctx, view| parser(ctx, view, version)).is_err());
            }
        }
    }

    #[test]
    fn split_boolean_rejects_names_and_nonzero_name_values() {
        let parser =
            feature_property_parser(SPLIT_BOOLEAN_TYPE).expect("registered Boolean parser");
        for version in [15, 22, 23, 25, 28, 31] {
            for (name_count, name_value) in [(1u32, 0u32), (u32::MAX, 0), (0, 1), (0, u32::MAX)] {
                let mut bytes = header(version);
                bytes.extend_from_slice(&name_count.to_le_bytes());
                bytes.extend_from_slice(&name_value.to_le_bytes());
                bytes.push(1);
                assert!(matches!(
                    parse(&bytes, |ctx, view| parser(ctx, view, version)),
                    Err(CodecError::Malformed(_))
                ));
            }
        }
        for (name, name_value) in [("a", 0u32), ("", 1), ("a", 1)] {
            let mut bytes = header(19);
            bytes.extend_from_slice(
                &u32::try_from(name.len())
                    .expect("short synthetic name")
                    .to_le_bytes(),
            );
            for unit in name.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            bytes.extend_from_slice(&name_value.to_le_bytes());
            bytes.push(1);
            assert!(parse(&bytes, |ctx, view| parser(ctx, view, 19)).is_err());
        }
    }

    #[test]
    fn split_boolean_truncation_keeps_the_source_location() {
        let parser =
            feature_property_parser(SPLIT_BOOLEAN_TYPE).expect("registered Boolean parser");
        for version in [15, 22, 23, 25, 28, 31] {
            let mut bytes = vec![0; 7];
            bytes.extend_from_slice(&header(version));
            let body = bytes.len();
            bytes.extend_from_slice(&[0; 9]);
            for (offset, width, operation) in [
                (0, 4, "split Boolean name count"),
                (4, 4, "split Boolean name value"),
                (8, 1, "split Boolean value"),
            ] {
                for available in 0..width {
                    parse(&bytes, |ctx, view| {
                        let expected = view.location_at(body + offset);
                        let source = view
                            .child(7, body + offset + available)
                            .expect("body prefix");
                        assert!(matches!(
                            parser(ctx, source, version),
                            Err(CodecError::Truncated { location, operation: field })
                                if location == expected && field == operation
                        ));
                    });
                }
            }
        }
    }
}
