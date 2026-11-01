// SPDX-License-Identifier: Apache-2.0
//! Split feature property grammars and the solid-split projection.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    BodySelection, DistinctMembers, FaceSelection, Feature, FeatureContent, FeatureDefinition,
    FeatureEvaluation, FeatureId, FeatureOperation, FeatureResultTopology, SplitBodyTool,
};
use cadmpeg_ir::native::bytes::NativeBytes;

use crate::pmdc::{content_header, Cursor, PmDcReference};

use super::{
    parse_enumeration, property, ClassId, PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureLabel,
    PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeaturePropertyPayload,
    PmDcFeatureReferenceFamily, ProjectionIndex, PropertyParser,
};

pub(crate) const CLASS_ID: ClassId = ClassId(NativeBytes::new([
    0xba, 0xa8, 0x71, 0x83, 0xd3, 0x11, 0x34, 0x7b, 0xc0, 0x00, 0xe3, 0x95, 0x45, 0xdf, 0x72, 0x4f,
]));

/// Slot-0 mode that splits a solid into every resulting side.
const SPLIT_SOLID: u16 = 3;
/// Slot-3 tool value that selects the slot-5 work plane.
const TOOL_WORK_PLANE: u16 = 2;
/// Slot-3 tool value that selects the slot-8 surface-body wrapper.
const TOOL_SURFACE: u16 = 3;
/// Slot of the two-body result collection.
const RESULT_SLOT: usize = 9;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeReason {
    Slots,
    Mode(u16),
    Control(usize),
    Selection(usize),
    Tool,
    Result,
}

impl NativeReason {
    fn note(self) -> cadmpeg_ir::report::loss::LossNote {
        let reason = match self {
            Self::Slots => "property vector is not the ten-slot type-2 form".to_owned(),
            Self::Mode(value) => {
                format!("operation mode {value} has unresolved construction operands")
            }
            Self::Control(slot) => format!("slot {slot} differs from the solid-split control form"),
            Self::Selection(slot) => format!("slot {slot} has unresolved selection operands"),
            Self::Tool => "tool identity, frame, or history position is unresolved".to_owned(),
            Self::Result => "result-body topology is unresolved".to_owned(),
        };
        crate::loss::InventorLossCode::FeatureSplitOperandsNative
            .note(format!("Retained Inventor Split natively: {reason}."))
    }
}

/// Checks the complete solid-split operand form for projection and validation.
pub(crate) fn definition<'a>(
    source: &PmDcFeature,
    resolve: impl Fn(u32) -> Option<&'a PmDcFeatureProperty>,
    plane: impl Fn(u32) -> Option<FeatureId>,
) -> Result<FeatureDefinition, NativeReason> {
    if source.properties.marker() != 2 {
        return Err(NativeReason::Slots);
    }
    let [mode, solid_control, unused, tool_kind, patches, work_plane, side_control, inputs, surface, results] =
        source.properties.references()
    else {
        return Err(NativeReason::Slots);
    };
    let enumeration =
        |reference: &PmDcReference, type_value: i16| match resolve(reference.index())?.kind {
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Auxiliary,
                type_value: actual,
                value,
            } if actual == type_value => Some(value),
            _ => None,
        };
    let mode = enumeration(mode, 3).ok_or(NativeReason::Control(0))?;
    if mode != SPLIT_SOLID {
        return Err(NativeReason::Mode(mode));
    }
    if !matches!(
        resolve(solid_control.index()).map(|property| &property.kind),
        Some(PmDcFeaturePropertyKind::Boolean { value: false, .. })
    ) {
        return Err(NativeReason::Control(1));
    }
    if unused.index() != 0 {
        return Err(NativeReason::Control(2));
    }
    if !matches!(
        resolve(side_control.index()).map(|property| &property.kind),
        Some(PmDcFeaturePropertyKind::Boolean { .. })
    ) {
        return Err(NativeReason::Control(6));
    }
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::BoundaryPatch,
        items: patches,
    } = &resolve(patches.index())
        .ok_or(NativeReason::Selection(4))?
        .kind
    else {
        return Err(NativeReason::Selection(4));
    };
    if !patches.references().is_empty() {
        return Err(NativeReason::Selection(4));
    }
    let body = |reference: PmDcReference| {
        let property = resolve(reference.index())?;
        matches!(property.kind, PmDcFeaturePropertyKind::SurfaceBody { .. }).then(|| property.id())
    };
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::BodySelection,
        items: targets,
    } = &resolve(inputs.index())
        .ok_or(NativeReason::Selection(7))?
        .kind
    else {
        return Err(NativeReason::Selection(7));
    };
    let [target] = targets.references() else {
        return Err(NativeReason::Selection(7));
    };
    let target = body(*target).ok_or(NativeReason::Selection(7))?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::ObjectCollection,
        items: outputs,
    } = &resolve(results.index())
        .ok_or(NativeReason::Selection(9))?
        .kind
    else {
        return Err(NativeReason::Selection(9));
    };
    let [first, second] = outputs.references() else {
        return Err(NativeReason::Selection(9));
    };
    if body(*first).ok_or(NativeReason::Selection(9))?
        == body(*second).ok_or(NativeReason::Selection(9))?
    {
        return Err(NativeReason::Selection(9));
    }
    let tool = match enumeration(tool_kind, 4).ok_or(NativeReason::Control(3))? {
        TOOL_WORK_PLANE if surface.index() == 0 => SplitBodyTool::Plane {
            plane: plane(work_plane.record_ordinal().ok_or(NativeReason::Tool)?)
                .ok_or(NativeReason::Tool)?,
        },
        TOOL_SURFACE if work_plane.index() == 0 => {
            let surface = body(*surface).ok_or(NativeReason::Tool)?;
            if surface == target {
                return Err(NativeReason::Tool);
            }
            SplitBodyTool::Faces(FaceSelection::Native(surface))
        }
        _ => return Err(NativeReason::Tool),
    };
    Ok(FeatureDefinition::Operation(FeatureOperation::SplitBody {
        targets: BodySelection::Native(target),
        tool,
    }))
}

/// The datum-plane feature of a work plane whose frame admits and whose
/// history position precedes the split at `split`.
fn plane_feature(
    token: &str,
    ordinal: u32,
    split: u32,
    index: &ProjectionIndex<'_>,
    history: &super::history::History,
) -> Option<FeatureId> {
    let record = index.work_planes.get(&(token, ordinal))?.as_ref()?;
    let placement = history.placement(ordinal)?;
    if placement.kind != super::history::HistoryNodeKind::WorkPlane
        || placement.ordinal >= history.placement(split)?.ordinal
    {
        return None;
    }
    super::work_plane::frame(record)?;
    super::history_feature_id(&record.identity).ok()
}

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
    history: &super::history::History,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    let token = source.identity.segment_token.as_str();
    let definition = match definition(
        source,
        |reference| super::resolve_property(token, reference, index),
        |ordinal| {
            plane_feature(
                token,
                ordinal,
                source.identity.record_ordinal,
                index,
                history,
            )
        },
    ) {
        Ok(definition) => definition,
        Err(reason) => {
            losses.push(reason.note());
            return None;
        }
    };
    let mut dependencies = DistinctMembers::default();
    if let FeatureDefinition::Operation(FeatureOperation::SplitBody {
        tool: SplitBodyTool::Plane { plane },
        ..
    }) = &definition
    {
        dependencies.insert(plane.clone());
    }
    let Some(result) = super::feature_result(ctx, source, RESULT_SLOT, index) else {
        losses.push(NativeReason::Result.note());
        return None;
    };
    let (id, result) = match result {
        Ok(result) => result,
        Err(error) => return Some(Err(error)),
    };
    if let Err(error) = ctx.charge_entities(1, "project Inventor feature") {
        return Some(Err(error));
    }
    Some(Ok((
        Feature {
            product_definition: None,
            id,
            ordinal: u64::from(label.index),
            name: label.name().map(str::to_owned),
            suppressed: None,
            needs_recompute: false,
            dependencies,
            source_properties: super::boolean_properties(source, &[6], index),
            source_tag: Some("split_body".into()),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(definition),
            native_ref: Some(source.id()),
        },
        result,
    )))
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
pub(crate) mod tests {
    use super::{definition, SPLIT_BOOLEAN_TYPE, SPLIT_ENUM_TYPE, TOOL_ENUM_TYPE};
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

    pub(crate) mod solid {
        use cadmpeg_ir::features::{
            BodySelection, FaceSelection, FeatureDefinition, FeatureId, FeatureOperation,
            SplitBodyTool,
        };

        use super::definition;
        use crate::feature::tests::{reference, test_feature, test_property};
        use crate::feature::{
            PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureProperty, PmDcFeaturePropertyKind,
            PmDcFeatureReferenceFamily,
        };

        const PLANE: u32 = 50;

        fn list(values: &[u32]) -> crate::pmdc::PmDcReferenceList {
            crate::pmdc::PmDcReferenceList::new(
                2,
                (!values.is_empty()).then_some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                values
                    .iter()
                    .copied()
                    .map(crate::pmdc::PmDcReference::from_packed)
                    .collect(),
            )
            .expect("synthetic packed reference list")
        }

        fn enumeration(type_value: i16, value: u16) -> PmDcFeaturePropertyKind {
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::Auxiliary,
                type_value,
                value,
            }
        }

        fn boolean(value: bool) -> PmDcFeaturePropertyKind {
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value,
            }
        }

        fn body() -> PmDcFeaturePropertyKind {
            PmDcFeaturePropertyKind::SurfaceBody { body: reference(0) }
        }

        fn properties(tool: u16) -> Vec<PmDcFeatureProperty> {
            properties_at(0, tool)
        }

        /// Ordinals `base` + 1 to 10: mode, solid control, tool kind,
        /// patches, side control, input selection, result collection, target
        /// body, and the two result bodies. Ordinal `base` + 11 is a surface
        /// tool.
        pub(crate) fn properties_at(base: u32, tool: u16) -> Vec<PmDcFeatureProperty> {
            let packed =
                |values: &[u32]| list(&values.iter().map(|value| value + base).collect::<Vec<_>>());
            vec![
                test_property(base + 1, enumeration(3, 3)),
                test_property(base + 2, boolean(false)),
                test_property(base + 3, enumeration(4, tool)),
                test_property(
                    base + 4,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::BoundaryPatch,
                        items: list(&[]),
                    },
                ),
                test_property(base + 5, boolean(true)),
                test_property(
                    base + 6,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::BodySelection,
                        items: packed(&[9]),
                    },
                ),
                test_property(
                    base + 7,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::ObjectCollection,
                        items: packed(&[10, 11]),
                    },
                ),
                test_property(base + 8, body()),
                test_property(base + 9, body()),
                test_property(base + 10, body()),
                test_property(base + 11, body()),
            ]
        }

        fn plane_split() -> PmDcFeature {
            plane_split_at(0, 0, PLANE)
        }

        /// A solid split at `ordinal` over [`properties_at`] with `base`,
        /// using the work plane at ordinal `plane`.
        pub(crate) fn plane_split_at(ordinal: u32, base: u32, plane: u32) -> PmDcFeature {
            test_feature(
                ordinal,
                10,
                &[
                    (0, base + 1),
                    (1, base + 2),
                    (3, base + 3),
                    (4, base + 4),
                    (5, plane),
                    (6, base + 5),
                    (7, base + 6),
                    (9, base + 7),
                ],
            )
        }

        fn surface_split() -> PmDcFeature {
            test_feature(
                0,
                10,
                &[
                    (0, 1),
                    (1, 2),
                    (3, 3),
                    (4, 4),
                    (6, 5),
                    (7, 6),
                    (8, 11),
                    (9, 7),
                ],
            )
        }

        fn plane_id() -> FeatureId {
            FeatureId::compose(
                &cadmpeg_ir::identity_namespace!("inventor", "design", "feature"),
                cadmpeg_ir::identity_key!("plane"),
            )
        }

        fn admit(
            source: &PmDcFeature,
            properties: &[PmDcFeatureProperty],
        ) -> Result<FeatureDefinition, crate::feature::split::NativeReason> {
            definition(
                source,
                |index| {
                    properties
                        .iter()
                        .find(|property| property.identity.record_ordinal + 1 == index)
                },
                |ordinal| (ordinal == PLANE).then(plane_id),
            )
        }

        fn decode(
            source: &PmDcFeature,
            properties: &[PmDcFeatureProperty],
        ) -> Option<FeatureDefinition> {
            admit(source, properties).ok()
        }

        #[test]
        fn solid_split_by_a_work_plane_or_a_surface_keeps_every_side() {
            let properties = properties(2);
            assert_eq!(
                decode(&plane_split(), &properties),
                Some(FeatureDefinition::Operation(FeatureOperation::SplitBody {
                    targets: BodySelection::Native(properties[7].id()),
                    tool: SplitBodyTool::Plane { plane: plane_id() },
                }))
            );
            let properties = self::properties(3);
            assert_eq!(
                decode(&surface_split(), &properties),
                Some(FeatureDefinition::Operation(FeatureOperation::SplitBody {
                    targets: BodySelection::Native(properties[7].id()),
                    tool: SplitBodyTool::Faces(FaceSelection::Native(properties[10].id())),
                }))
            );
            let mut properties = self::properties(2);
            properties[4] = test_property(5, boolean(false));
            assert!(decode(&plane_split(), &properties).is_some());
        }

        fn projection_index(
            properties: &[PmDcFeatureProperty],
        ) -> crate::feature::ProjectionIndex<'_> {
            crate::feature::ProjectionIndex {
                properties: properties
                    .iter()
                    .map(|record| {
                        (
                            (
                                record.identity.segment_token.as_str(),
                                record.identity.record_ordinal,
                            ),
                            Some(record),
                        )
                    })
                    .collect(),
                ..crate::feature::ProjectionIndex::default()
            }
        }

        #[test]
        fn admitted_surface_split_emits_no_native_operand_loss() {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )
            .expect("decode context");
            let properties = properties(3);
            let source = surface_split();
            let label =
                crate::feature::tests::test_label(0, 1, crate::feature::split::CLASS_ID, &[]);
            let mut losses = Vec::new();
            let projected = crate::feature::split::project(
                &ctx,
                &source,
                &label,
                &projection_index(&properties),
                &crate::feature::history::History::default(),
                &mut losses,
            )
            .expect("admitted Split")
            .expect("projection");
            assert_eq!(
                projected.0.evaluation.definition(),
                &FeatureDefinition::Operation(FeatureOperation::SplitBody {
                    targets: BodySelection::Native(properties[7].id()),
                    tool: SplitBodyTool::Faces(FaceSelection::Native(properties[10].id())),
                }),
            );
            assert!(losses.is_empty());
        }

        #[test]
        fn native_modes_keep_both_boolean_values_and_emit_the_pinned_loss() {
            use crate::feature::history::History;
            use crate::feature::split::{project, NativeReason};

            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )
            .expect("decode context");
            for mode in [1, 2] {
                for first in [false, true] {
                    for second in [false, true] {
                        let mut properties = properties(2);
                        properties[0] = test_property(1, enumeration(3, mode));
                        properties[1] = test_property(2, boolean(first));
                        properties[4] = test_property(5, boolean(second));
                        let mut slots =
                            vec![(0, 1), (1, 2), (3, 3), (4, 4), (5, PLANE), (6, 5), (7, 6)];
                        if mode == 2 {
                            properties.push(test_property(
                                12,
                                PmDcFeaturePropertyKind::References {
                                    family: PmDcFeatureReferenceFamily::FaceCollection,
                                    items: list(&[14]),
                                },
                            ));
                            slots.push((2, 12));
                        }
                        let source = test_feature(0, 10, &slots);
                        let index = projection_index(&properties);
                        assert_eq!(admit(&source, &properties), Err(NativeReason::Mode(mode)));
                        let label = crate::feature::tests::test_label(
                            0,
                            1,
                            crate::feature::split::CLASS_ID,
                            &[],
                        );
                        let mut losses = Vec::new();
                        assert!(project(
                            &ctx,
                            &source,
                            &label,
                            &index,
                            &History::default(),
                            &mut losses
                        )
                        .is_none());
                        assert_eq!(losses.len(), 1);
                        assert_eq!(
                            losses[0].code,
                            crate::loss::InventorLossCode::FeatureSplitOperandsNative
                                .note("test")
                                .code
                        );
                        assert_eq!(losses[0].code.namespace(), "inventor");
                        assert_eq!(losses[0].code.local_code(), "feature.split-operands-native");
                        assert!(losses[0]
                            .message
                            .contains(&format!("operation mode {mode}")));
                    }
                }
            }
        }

        #[test]
        fn solid_split_refuses_other_modes_tools_and_operands() {
            for (position, kind) in [
                (0, enumeration(3, 1)),
                (0, enumeration(3, 2)),
                (0, enumeration(4, 3)),
                (1, boolean(true)),
                (2, enumeration(4, 3)),
                (2, enumeration(4, 1)),
                (
                    3,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::BoundaryPatch,
                        items: list(&[8]),
                    },
                ),
                (4, enumeration(3, 3)),
                (
                    5,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::BodySelection,
                        items: list(&[]),
                    },
                ),
                (
                    5,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::BodySelection,
                        items: list(&[9, 10]),
                    },
                ),
                (
                    5,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::ObjectCollection,
                        items: list(&[9]),
                    },
                ),
                (
                    6,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::ObjectCollection,
                        items: list(&[10]),
                    },
                ),
                (
                    6,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::ObjectCollection,
                        items: list(&[10, 10]),
                    },
                ),
                (
                    6,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::ObjectCollection,
                        items: list(&[10, 2]),
                    },
                ),
                (7, boolean(true)),
            ] {
                let mut properties = properties(2);
                let ordinal = properties[position].identity.record_ordinal;
                properties[position] = test_property(ordinal, kind);
                let reason = admit(&plane_split(), &properties).expect_err("unsupported operand");
                assert_eq!(
                    reason.note().code.local_code(),
                    "feature.split-operands-native",
                    "{position}"
                );
            }
        }

        #[test]
        fn solid_split_requires_the_tool_reference_its_enumeration_selects() {
            let properties = properties(2);
            let mut source = plane_split();
            let mut references = source.properties.references().to_vec();
            references[5] = reference(PLANE + 2);
            let unknown_plane = test_feature(0, 10, &[]);
            let rebuilt = |references: Vec<crate::pmdc::PmDcReference>| {
                let mut feature = unknown_plane.clone();
                let mut payload = (*feature).clone();
                payload.properties = crate::pmdc::PmDcReferenceList::new(
                    2,
                    Some(crate::pmdc::PmDcListMetadata::U32([10, 0])),
                    references,
                )
                .expect("ten references");
                feature = crate::record_identity::Located::new(
                    payload,
                    feature.identity.type_id.clone(),
                    feature.identity.segment_token.clone(),
                    feature.identity.record_ordinal,
                );
                feature
            };
            assert!(decode(&rebuilt(references.clone()), &properties).is_none());
            references[5] = source.properties.references()[5];
            references[8] = reference(12);
            assert!(decode(&rebuilt(references.clone()), &properties).is_none());
            references[8] = reference(0);
            references[2] = reference(1);
            assert!(decode(&rebuilt(references), &properties).is_none());

            let properties = self::properties(3);
            source = surface_split();
            let mut references = source.properties.references().to_vec();
            references[5] = reference(PLANE + 1);
            assert!(decode(&rebuilt(references.clone()), &properties).is_none());
            references[5] = reference(0);
            references[8] = reference(9);
            assert!(decode(&rebuilt(references.clone()), &properties).is_none());
            references[8] = reference(2);
            assert!(decode(&rebuilt(references), &properties).is_none());
            assert!(decode(&plane_split(), &properties).is_none());
            assert!(decode(&test_feature(0, 9, &[(0, 1)]), &properties).is_none());
        }
    }
}
