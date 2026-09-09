// SPDX-License-Identifier: Apache-2.0
//! Project exact Design assembly alignments into neutral joints.

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{Feature, FeatureDefinition};
use cadmpeg_ir::products::{
    AssemblyJoint, ExternalDocumentReference, JointConnector, JointLimits, JointOperand,
    PairedJointKind,
};

use crate::ids::native_stream;
use crate::records::feature::{
    DesignAssemblyAxialOperandTarget, DesignAssemblyLimitKind, DesignAssemblyOperandQualifier,
    DesignComponentOccurrence, DesignParameterScope,
};

/// One exact generation of the legacy 421-byte `As-built` alignment grammar.
///
/// The scope class pair is the admission key. All other fields are part of
/// that key's grammar and must not be inferred from a neighboring generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyAsBuilt421Generation {
    Class364,
    Class420,
    Class417,
    Class457,
}

/// Operand-frame form for a non-axial `Assemble` scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssemblyOperandFrameVariant {
    Standard,
    Compact,
    Axial,
    LegacyClass388,
}

/// Whether a scope belongs to the variable-reference `Assemble` generation.
///
/// This generation keeps the standard operand and locator prefix while its
/// owner and reference trailers grow with additional placement and limit
/// groups. The class pair, rather than the total frame length, admits it.
pub(crate) fn variable_reference_assembly_generation(
    class_tag: &str,
    paired_class_tag: &str,
) -> bool {
    matches!(
        (class_tag, paired_class_tag),
        ("283", "264") | ("347", "260")
    )
}

/// The operand, owner-lane, and locator layout of one assembly scope.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AssemblyScopeGeneration {
    operand_frame: Option<AssemblyOperandFrameVariant>,
    alignment: AssemblyAlignmentLanes,
    locator_offsets: Option<[usize; 2]>,
}

#[derive(Debug, Clone, Copy)]
enum AssemblyAlignmentLanes {
    Fixed(Option<(usize, usize, usize)>),
    Variable(Option<(usize, usize, usize)>),
}

impl AssemblyScopeGeneration {
    /// Classify the scope's three coupled layout projections.
    pub(crate) fn new(frame_length: u64, class_tag: &str, paired_class_tag: &str) -> Self {
        use AssemblyOperandFrameVariant::{Axial, Compact, LegacyClass388, Standard};
        let (operand_frame, lanes, locator_offsets) = match frame_length {
            399 => (None, Some((4, 0, 4)), Some([51, 62])),
            604 => (None, Some((8, 4, 8)), None),
            627 | 637 | 692 => (Some(Standard), Some((4, 0, 4)), Some([366, 377])),
            633 => (Some(Compact), Some((4, 0, 4)), Some([362, 373])),
            671 => ((class_tag == "406" && paired_class_tag == "261").then_some(Standard), Some((6, 4, 6)), Some([
                crate::layout::assembly_class_406_261_scope_671::FIRST_LOCATOR_REFERENCE,
                crate::layout::assembly_class_406_261_scope_671::SECOND_LOCATOR_REFERENCE,
            ])),
            705 => (Some(Axial), Some((6, 4, 6)), None),
            732 => (Some(Compact), Some((8, 4, 8)), Some([362, 373])),
            744 => ((class_tag == "430" && paired_class_tag == "262").then_some(Compact), Some((8, 4, 8)), Some([362, 373])),
            748 => ((class_tag == "430" && paired_class_tag == "262").then_some(Standard), Some((8, 4, 8)), Some([366, 377])),
            772 => (Some(Axial), Some((10, 8, 10)), None),
            length if length == crate::layout::assembly_class_388_266_scope_968::LEN as u64 => (
                (class_tag == "388" && paired_class_tag == "266").then_some(LegacyClass388), Some((28, 4, 8)), Some([
                    crate::layout::assembly_class_388_266_scope_968::OPERAND_PATH_LOCATOR_REFERENCES,
                    crate::layout::assembly_class_388_266_scope_968::OPERAND_PATH_LOCATOR_REFERENCES + 11,
                ])),
            length if length == crate::layout::assembly_class_383_258_scope_1011::LEN as u64 => (
                (class_tag == "383" && paired_class_tag == "258").then_some(Standard), Some((20, 8, 12)), None),
            _ => (None, None, None),
        };
        if variable_reference_assembly_generation(class_tag, paired_class_tag) {
            Self {
                operand_frame: Some(Standard),
                alignment: AssemblyAlignmentLanes::Variable(lanes),
                locator_offsets: Some([366, 377]),
            }
        } else {
            Self {
                operand_frame,
                alignment: AssemblyAlignmentLanes::Fixed(lanes),
                locator_offsets,
            }
        }
    }

    /// Operand-frame grammar admitted by the scope classes.
    pub(crate) fn operand_frame_variant(self) -> Option<AssemblyOperandFrameVariant> {
        self.operand_frame
    }

    /// Half-open alignment range for the supplied owner count.
    pub(crate) fn alignment_lane_bounds(self, owner_count: usize) -> Option<(usize, usize)> {
        let fallback = match self.alignment {
            AssemblyAlignmentLanes::Variable(_)
                if owner_count >= 12 && matches!((owner_count - 12) % 4, 0 | 2) =>
            {
                return Some((8, 12))
            }
            AssemblyAlignmentLanes::Fixed(fallback)
            | AssemblyAlignmentLanes::Variable(fallback) => fallback,
        };
        let (count, start, end) = fallback?;
        (owner_count == count).then_some((start, end))
    }

    /// Marker offsets of the two ordered operand-path locators.
    pub(crate) fn operand_path_locator_offsets(self) -> Option<[usize; 2]> {
        self.locator_offsets
    }
}

/// Admit the legacy 383/258 assembly scope only as its exact generation.
pub(crate) fn legacy_class_383_258_scope(
    frame_length: u64,
    class_tag: &str,
    paired_class_tag: &str,
) -> bool {
    frame_length == crate::layout::assembly_class_383_258_scope_1011::LEN as u64
        && class_tag == "383"
        && paired_class_tag == "258"
}

impl LegacyAsBuilt421Generation {
    /// Owner-frame primary class for the six scalar lanes.
    pub(crate) const fn owner_class_tag(self) -> &'static str {
        match self {
            Self::Class364 => "293",
            Self::Class420 => "378",
            Self::Class417 => "318",
            Self::Class457 => "418",
        }
    }

    /// Owner-frame paired class.
    pub(crate) const fn owner_paired_class_tag(self) -> &'static str {
        match self {
            Self::Class364 => "272",
            Self::Class420 => "262",
            Self::Class417 => "263",
            Self::Class457 => "258",
        }
    }

    /// Solved connector-frame primary class named by reference-table entry 8.
    pub(crate) const fn frame_class_tag(self) -> &'static str {
        match self {
            Self::Class364 => "376",
            Self::Class420 => "327",
            Self::Class417 => "448",
            Self::Class457 => "297",
        }
    }

    /// Solved connector-frame paired class.
    pub(crate) const fn frame_paired_class_tag(self) -> &'static str {
        self.owner_paired_class_tag()
    }

    /// Byte length from the solved frame primary header to its paired header.
    pub(crate) const fn frame_length(self) -> usize {
        match self {
            Self::Class364 => 389,
            Self::Class420 | Self::Class417 => 390,
            Self::Class457 => 385,
        }
    }

    /// Offset of the four-byte marker immediately before the solved matrix.
    pub(crate) const fn matrix_prefix(self) -> usize {
        match self {
            Self::Class420 | Self::Class417 => 46,
            Self::Class364 | Self::Class457 => 45,
        }
    }

    /// Offset of the first f64 in the solved row-major matrix.
    pub(crate) const fn matrix_offset(self) -> usize {
        match self {
            Self::Class420 | Self::Class417 => 50,
            Self::Class364 | Self::Class457 => 49,
        }
    }

    /// Domain of the two limit lanes.
    pub(crate) const fn limit_kind(self) -> DesignAssemblyLimitKind {
        match self {
            Self::Class364 => DesignAssemblyLimitKind::Angular,
            Self::Class420 | Self::Class417 | Self::Class457 => DesignAssemblyLimitKind::Linear,
        }
    }

    /// Whether source limit lanes are stored maximum then minimum.
    pub(crate) const fn reverse_limit_order(self) -> bool {
        matches!(self, Self::Class420 | Self::Class417)
    }
}

/// Admit one exact 421-byte `As-built` scope generation.
pub(crate) fn legacy_as_built_421_generation(
    frame_length: u64,
    class_tag: &str,
    paired_class_tag: &str,
) -> Option<LegacyAsBuilt421Generation> {
    if frame_length != 421 {
        return None;
    }
    match (class_tag, paired_class_tag) {
        ("364", "272") => Some(LegacyAsBuilt421Generation::Class364),
        ("420", "262") => Some(LegacyAsBuilt421Generation::Class420),
        ("417", "263") => Some(LegacyAsBuilt421Generation::Class417),
        ("457", "258") => Some(LegacyAsBuilt421Generation::Class457),
        _ => None,
    }
}

/// Project assembly scopes whose connector frames and operand qualifiers are complete.
pub(crate) fn project_assembly_joints(
    scopes: &[DesignParameterScope],
    native_occurrences: &[DesignComponentOccurrence],
    features: &[Feature],
) -> Result<Vec<AssemblyJoint>, CodecError> {
    let mut occurrences = BTreeMap::new();
    for occurrence in native_occurrences {
        let Some(stream) = native_stream(&occurrence.id) else {
            continue;
        };
        occurrences
            .entry((
                stream,
                occurrence.occurrence_guid.as_str().to_ascii_lowercase(),
            ))
            .and_modify(|candidate| *candidate = None)
            .or_insert(Some(occurrence));
    }
    let mut joints = BTreeMap::new();
    for scope in scopes {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(alignment) = scope.assembly_alignment() else {
            continue;
        };
        let (frames, operands, limits) = match alignment.form.as_ref() {
            Some(crate::records::feature::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                carriers,
                solved_frame,
                limits,
                ..
            }) => (
                carriers
                    .frames(solved_frame)
                    .map_err(cadmpeg_core::CodecError::NotImplemented)?,
                carriers.selections().map(|selection| {
                    JointOperand::root(
                        crate::ids::neutral_assembly_legacy_object_id(selection),
                        Vec::new(),
                    )
                }),
                limits.as_ref(),
            ),
            Some(crate::records::feature::DesignAssemblyAlignmentForm::Qualified(operands)) => {
                let Some(projected) = project_qualified_operands(
                    operands.each_ref().map(|operand| &operand.qualifier),
                    stream,
                    &occurrences,
                    scopes,
                    features,
                ) else {
                    continue;
                };
                (
                    operands.each_ref().map(|operand| operand.frame.clone()),
                    projected,
                    None,
                )
            }
            _ => continue,
        };
        let (angular_limits, linear_limits) = match limits {
            Some(limits) => {
                let projected = JointLimits::new(Some(limits.minimum()), Some(limits.maximum()))
                    .ok_or_else(|| {
                        CodecError::Malformed(
                            "joint limits minimum/maximum must be finite and ordered".into(),
                        )
                    })?;
                match limits.kind {
                    DesignAssemblyLimitKind::Angular => (Some(projected), None),
                    DesignAssemblyLimitKind::Linear => (None, Some(projected)),
                }
            }
            None => (None, None),
        };
        let id = crate::ids::neutral_assembly_joint_id(scope);
        let [first_operand, second_operand] = operands;
        let first_frame = super::components::neutral_transform(frames[0].transform)?;
        let second_frame = super::components::neutral_transform(frames[1].transform)?;
        let angle = cadmpeg_ir::scalar::FiniteReal::new(alignment.angle())
            .ok_or_else(|| CodecError::Malformed("joint angle must be finite".into()))?;
        let [x, y, z] = alignment.offset().map(|value| {
            cadmpeg_ir::scalar::FiniteReal::new(value * 10.0).ok_or_else(|| {
                CodecError::Malformed("joint translation_offset must be finite".into())
            })
        });
        let translation_offset = [x?, y?, z?];
        joints.entry(id.as_str().to_owned()).or_insert_with(|| {
            let mut joint = AssemblyJoint::paired(
                id,
                PairedJointKind::Fixed {
                    angle: Some(angle),
                    translation_offset: Some(translation_offset),
                    angular_limits,
                    linear_limits,
                },
                [
                    JointConnector {
                        operand: first_operand,
                        frame: first_frame,
                        detached: false,
                    },
                    JointConnector {
                        operand: second_operand,
                        frame: second_frame,
                        detached: false,
                    },
                ],
                None,
            );
            joint.native_ref = Some(scope.id.clone());
            joint
        });
    }
    Ok(joints.into_values().collect())
}

fn project_qualified_operands(
    qualifiers: [&DesignAssemblyOperandQualifier; 2],
    stream: &str,
    occurrences: &BTreeMap<(&str, String), Option<&DesignComponentOccurrence>>,
    scopes: &[DesignParameterScope],
    features: &[Feature],
) -> Option<[JointOperand; 2]> {
    let projected = qualifiers.map(|qualifier| match qualifier {
        DesignAssemblyOperandQualifier::OccurrencePath { path } => {
            let root_guid = &path.occurrence_guids().first()?.value;
            let occurrence = occurrences
                .get(&(stream, root_guid.as_str().to_ascii_lowercase()))
                .copied()
                .flatten();
            if occurrence.is_none() && !matches!(path.class_tag().as_str(), "330" | "386") {
                return None;
            }
            let object = root_guid.as_str().to_ascii_lowercase();
            let subelements = path.occurrence_guids()[1..]
                .iter()
                .map(|guid| guid.value.as_str().to_ascii_lowercase())
                .collect();
            Some(match occurrence {
                Some(_) => JointOperand::occurrence(
                    crate::ids::neutral_component_occurrence_id(root_guid.as_str()),
                    object,
                    subelements,
                ),
                None => JointOperand::external(
                    ExternalDocumentReference::document_id(
                        path.identity_guids().first()?.value.clone(),
                    ),
                    object,
                    subelements,
                ),
            })
        }
        DesignAssemblyOperandQualifier::AxialTarget { target } => match target {
            DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
                component_insert_scope_record_index,
                selectors,
                ..
            } => {
                let target_scope = unique_scope(
                    scopes,
                    stream,
                    *component_insert_scope_record_index,
                    &crate::records::feature::DesignFeatureKind::ComponentInsert,
                )?;
                let feature = unique_feature(features, &target_scope.id)?;
                let FeatureDefinition::InsertComponent { occurrence } =
                    feature.evaluation.definition()
                else {
                    return None;
                };
                Some(JointOperand::occurrence(
                    occurrence.clone(),
                    crate::ids::neutral_assembly_axial_object_id(&selectors[0]),
                    Vec::new(),
                ))
            }
            DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin { scope_record_index } => {
                project_joint_origin_operand(*scope_record_index, stream, scopes, features)
            }
        },
        DesignAssemblyOperandQualifier::JointOrigin {
            scope_record_index, ..
        } => project_joint_origin_operand(*scope_record_index, stream, scopes, features),
    });
    let [first, second] = projected;
    Some([first?, second?])
}

fn project_joint_origin_operand(
    scope_record_index: u32,
    stream: &str,
    scopes: &[DesignParameterScope],
    features: &[Feature],
) -> Option<JointOperand> {
    let target_scope = unique_scope(
        scopes,
        stream,
        scope_record_index,
        &crate::records::feature::DesignFeatureKind::JointOrigin,
    )?;
    if let Some(feature) = unique_feature(features, &target_scope.id) {
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::DatumCoordinateSystem { .. }
        ) {
            return None;
        }
    } else if target_scope.joint_origin_transform().is_none() {
        return None;
    }
    Some(JointOperand::root(
        crate::ids::neutral_feature_id(target_scope).as_str(),
        Vec::new(),
    ))
}

fn unique_scope<'a>(
    scopes: &'a [DesignParameterScope],
    stream: &str,
    record_index: u32,
    kind: &crate::records::feature::DesignFeatureKind,
) -> Option<&'a DesignParameterScope> {
    let mut matches = scopes.iter().filter(|scope| {
        native_stream(&scope.id) == Some(stream)
            && scope.record_index == record_index
            && scope.kind() == *kind
    });
    let scope = matches.next()?;
    matches.next().is_none().then_some(scope)
}

fn unique_feature<'a>(features: &'a [Feature], native_ref: &str) -> Option<&'a Feature> {
    let mut matches = features
        .iter()
        .filter(|feature| feature.native_ref.as_deref() == Some(native_ref));
    let feature = matches.next()?;
    matches.next().is_none().then_some(feature)
}

#[cfg(test)]
mod tests {
    use crate::records::feature::DesignAssemblyOperandQualifier;
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
    use cadmpeg_ir::ids::OccurrenceId;
    use cadmpeg_ir::math::{Point3, Vector3};

    use crate::records::feature::{
        DesignAssemblyAxialOperandTarget, DesignAssemblyAxialSelectorIdentity,
        DesignAssemblyLimitKind, DesignParameterScope,
    };

    fn selector() -> DesignAssemblyAxialSelectorIdentity {
        DesignAssemblyAxialSelectorIdentity {
            axis_record_index: 10,
            axis_class_tag: crate::records::DesignClassTag::try_from("316".to_owned()).unwrap(),
            axis_byte_offset: 100,
            axis_paired_class_tag: crate::records::DesignClassTag::try_from("261".to_owned())
                .unwrap(),
            axis_paired_byte_offset: 120,
            selector_record_index: 13,
            selector_class_tag: crate::records::DesignClassTag::try_from("277".to_owned()).unwrap(),
            selector_byte_offset: 200,
            selector_paired_class_tag: crate::records::DesignClassTag::try_from("261".to_owned())
                .unwrap(),
            selector_paired_byte_offset: 560,
            nested_record_index: 16,
            nested_record_index_offset: 223,
            selector_asset_id: "abcdefab-cdef-4abc-8def-abcdefabcdef"
                .to_owned()
                .try_into()
                .expect("GUID"),
            selector_asset_id_offset: 241,
            selector_context_id: "bcdefabc-defa-4bcd-8efa-bcdefabcdefa"
                .to_owned()
                .try_into()
                .expect("GUID"),
            selector_context_id_offset: 317,
            occurrence_reference: 1_001,
            occurrence_reference_offset: 402,
            external_object_reference: 2_001,
            external_object_reference_offset: 417,
            external_segment: 7,
            external_segment_offset: 426,
            external_asset_id: "abcdefab-cdef-4abc-8def-abcdefabcdef"
                .to_owned()
                .try_into()
                .expect("GUID"),
            external_asset_id_offset: 434,
            external_link_name: "component-link".into(),
            external_link_name_offset: 511,
            external_version: None,
            role_record_index: 18,
            role_class_tag: crate::records::DesignClassTag::try_from("298".to_owned()).unwrap(),
            role_byte_offset: 600,
            occurrence_role: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
                .to_owned()
                .try_into()
                .expect("GUID"),
            occurrence_role_offset: 629,
        }
    }

    fn second_selector() -> DesignAssemblyAxialSelectorIdentity {
        let mut selector = selector();
        selector.axis_record_index = 30;
        selector.axis_byte_offset = 720;
        selector.axis_paired_byte_offset = 740;
        selector.selector_record_index = 33;
        selector.selector_byte_offset = 760;
        selector.selector_paired_byte_offset = 1_120;
        selector.nested_record_index = 36;
        selector.nested_record_index_offset = 783;
        selector.selector_asset_id_offset = 801;
        selector.selector_context_id_offset = 877;
        selector.occurrence_reference = 1_002;
        selector.occurrence_reference_offset = 962;
        selector.external_object_reference_offset = 977;
        selector.external_segment_offset = 986;
        selector.external_asset_id_offset = 994;
        selector.external_link_name_offset = 1_071;
        selector.role_record_index = 38;
        selector.role_byte_offset = 1_140;
        selector.occurrence_role_offset = 1_169;
        selector
    }

    fn feature(native_ref: &str, definition: FeatureDefinition) -> Feature {
        Feature {
            id: FeatureId::mint(format!(
                "test:model:feature#{}",
                native_ref.replace('#', ":")
            ))
            .expect("identity grammar"),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
            native_ref: Some(native_ref.into()),
        }
    }

    #[test]
    fn writer_rejects_cadir_assembly_translation_overflow() {
        use crate::records::feature::{
            DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyOperandFrame,
            DesignFeatureKind, DesignScopePayload,
        };
        let mut scopes = Vec::new();
        for index in [1, 2] {
            let mut scope = DesignParameterScope::empty(
                &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{index}"),
                DesignFeatureKind::JointOrigin,
                index,
            );
            scope.with_joint_origin_transform(crate::records::SketchPlacementMatrix::IDENTITY);
            scopes.push(scope);
        }
        let mut rows = cadmpeg_ir::transform::Transform::identity().rows();
        rows[0][3] = f64::MAX;
        let frames = [1, 2].map(|index| DesignAssemblyOperandFrame {
            reference_record_index: index,
            reference_offset: 0,
            transform: rows.try_into().unwrap(),
            transform_offset: 0,
        });
        let qualifiers =
            [1, 2].map(
                |scope_record_index| DesignAssemblyOperandQualifier::AxialTarget {
                    target: DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
                        scope_record_index,
                    },
                },
            );
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#3",
            DesignFeatureKind::Assemble,
            3,
        );
        scope
            .try_edit(|draft| {
                draft.payload = DesignScopePayload::Assemble(Some(
                    DesignAssemblyAlignment::try_new(
                        0.0,
                        [0.0; 3],
                        Vec::new(),
                        Some(DesignAssemblyAlignmentForm::qualified(frames, qualifiers)),
                    )
                    .unwrap(),
                ));
            })
            .unwrap();
        scopes.push(scope);
        let native = crate::native::F3dNative {
            design_parameter_scopes: scopes,
            ..Default::default()
        };
        let native = serde_json::from_value(serde_json::to_value(native).unwrap()).unwrap();
        let result = crate::writer::primitives::validate_assembly_projection(
            &cadmpeg_ir::document::CadIr::empty(),
            Some(&native),
        );
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::NotImplemented(m))
                if m.contains("finite affine transform")
        ));
    }

    #[test]
    fn affine_projection_rejects_nonfinite_cadir_coefficients() {
        let mut transform = cadmpeg_ir::transform::Transform::identity().rows();
        transform[0][0] = f64::NAN;
        assert!(matches!(
            crate::design::components::neutral_transform(transform),
            Err(cadmpeg_core::CodecError::NotImplemented(_))
        ));
    }

    #[test]
    fn affine_projection_rejects_translation_overflow() {
        let mut transform = cadmpeg_ir::transform::Transform::identity().rows();
        transform[0][3] = f64::MAX;
        assert!(matches!(
            crate::design::components::neutral_transform(transform),
            Err(cadmpeg_core::CodecError::NotImplemented(_))
        ));
    }

    #[test]
    fn assembly_frame_conversion_scales_only_translation() {
        let transform = [
            [0.0, -1.0, 0.0, 1.25],
            [1.0, 0.0, 0.0, -2.5],
            [0.0, 0.0, 1.0, 3.75],
            [0.0, 0.0, 0.0, 1.0],
        ];
        assert_eq!(
            crate::design::components::neutral_transform(transform)
                .unwrap()
                .rows(),
            [
                [0.0, -1.0, 0.0, 12.5],
                [1.0, 0.0, 0.0, -25.0],
                [0.0, 0.0, 1.0, 37.5],
                [0.0, 0.0, 0.0, 1.0],
            ]
        );
    }

    #[test]
    fn legacy_as_built_421_generation_map_is_exact() {
        for (
            scope_class,
            scope_paired_class,
            owner_class,
            owner_paired_class,
            frame_class,
            frame_paired_class,
            frame_length,
            matrix_prefix,
            matrix_offset,
            limit_kind,
            reverse_limit_order,
        ) in [
            (
                "364",
                "272",
                "293",
                "272",
                "376",
                "272",
                389,
                45,
                49,
                DesignAssemblyLimitKind::Angular,
                false,
            ),
            (
                "420",
                "262",
                "378",
                "262",
                "327",
                "262",
                390,
                46,
                50,
                DesignAssemblyLimitKind::Linear,
                true,
            ),
            (
                "417",
                "263",
                "318",
                "263",
                "448",
                "263",
                390,
                46,
                50,
                DesignAssemblyLimitKind::Linear,
                true,
            ),
            (
                "457",
                "258",
                "418",
                "258",
                "297",
                "258",
                385,
                45,
                49,
                DesignAssemblyLimitKind::Linear,
                false,
            ),
        ] {
            let generation =
                super::legacy_as_built_421_generation(421, scope_class, scope_paired_class)
                    .expect("generation is admitted");
            assert_eq!(generation.owner_class_tag(), owner_class);
            assert_eq!(generation.owner_paired_class_tag(), owner_paired_class);
            assert_eq!(generation.frame_class_tag(), frame_class);
            assert_eq!(generation.frame_paired_class_tag(), frame_paired_class);
            assert_eq!(generation.frame_length(), frame_length);
            assert_eq!(generation.matrix_prefix(), matrix_prefix);
            assert_eq!(generation.matrix_offset(), matrix_offset);
            assert_eq!(generation.limit_kind(), limit_kind);
            assert_eq!(generation.reverse_limit_order(), reverse_limit_order);
        }
        assert!(super::legacy_as_built_421_generation(420, "364", "272").is_none());
        assert!(super::legacy_as_built_421_generation(421, "364", "262").is_none());
        assert!(super::legacy_as_built_421_generation(421, "999", "272").is_none());
    }

    #[test]
    fn alignment_lane_bounds_require_the_exact_frame_and_owner_count() {
        for (frame_length, owner_count, expected) in [
            (
                crate::layout::assembly_class_388_266_scope_968::LEN as u64,
                28,
                (4, 8),
            ),
            (
                crate::layout::assembly_class_383_258_scope_1011::LEN as u64,
                20,
                (8, 12),
            ),
            (627, 4, (0, 4)),
            (633, 4, (0, 4)),
            (637, 4, (0, 4)),
            (692, 4, (0, 4)),
            (604, 8, (4, 8)),
            (732, 8, (4, 8)),
            (744, 8, (4, 8)),
            (748, 8, (4, 8)),
            (705, 6, (4, 6)),
            (671, 6, (4, 6)),
            (772, 10, (8, 10)),
        ] {
            assert_eq!(
                super::AssemblyScopeGeneration::new(frame_length, "", "")
                    .alignment_lane_bounds(owner_count),
                Some(expected)
            );
        }
        for (frame_length, owner_count) in [(627, 6), (732, 6), (705, 8), (772, 8), (604, 4)] {
            assert_eq!(
                super::AssemblyScopeGeneration::new(frame_length, "", "")
                    .alignment_lane_bounds(owner_count),
                None
            );
        }
        for (class_tag, paired_class_tag) in [("283", "264"), ("347", "260")] {
            for owner_count in [12, 14, 16, 20, 22, 36, 38, 44, 60] {
                assert_eq!(
                    super::AssemblyScopeGeneration::new(
                        800 + owner_count as u64,
                        class_tag,
                        paired_class_tag
                    )
                    .alignment_lane_bounds(owner_count),
                    Some((8, 12))
                );
            }
            for owner_count in [0, 10, 13, 15] {
                assert_eq!(
                    super::AssemblyScopeGeneration::new(
                        800 + owner_count as u64,
                        class_tag,
                        paired_class_tag
                    )
                    .alignment_lane_bounds(owner_count),
                    None
                );
            }
        }
        assert_eq!(
            super::AssemblyScopeGeneration::new(869, "283", "260").alignment_lane_bounds(12),
            None
        );
    }

    #[test]
    fn operand_path_locator_offsets_follow_the_frame_layout() {
        let class_388_length = crate::layout::assembly_class_388_266_scope_968::LEN as u64;
        assert_eq!(
            super::AssemblyScopeGeneration::new(class_388_length, "388", "266")
                .operand_path_locator_offsets(),
            Some([366, 377])
        );
        for frame_length in [627, 637, 692, 748] {
            assert_eq!(
                super::AssemblyScopeGeneration::new(frame_length, "", "")
                    .operand_path_locator_offsets(),
                Some([366, 377])
            );
        }
        for frame_length in [633, 732, 744] {
            assert_eq!(
                super::AssemblyScopeGeneration::new(frame_length, "", "")
                    .operand_path_locator_offsets(),
                Some([362, 373])
            );
        }
        assert_eq!(
            super::AssemblyScopeGeneration::new(671, "406", "261").operand_path_locator_offsets(),
            Some([388, 399])
        );
        for frame_length in [604, 705, 772] {
            assert_eq!(
                super::AssemblyScopeGeneration::new(frame_length, "", "")
                    .operand_path_locator_offsets(),
                None
            );
        }
        assert_eq!(
            super::AssemblyScopeGeneration::new(869, "283", "264").operand_path_locator_offsets(),
            Some([366, 377])
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(843, "347", "260").operand_path_locator_offsets(),
            Some([366, 377])
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(869, "283", "260").operand_path_locator_offsets(),
            None
        );
    }

    #[test]
    fn operand_frames_are_scoped_by_class_pair() {
        assert_eq!(
            super::AssemblyScopeGeneration::new(
                crate::layout::assembly_class_388_266_scope_968::LEN as u64,
                "388",
                "266"
            )
            .operand_frame_variant(),
            Some(super::AssemblyOperandFrameVariant::LegacyClass388)
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(
                crate::layout::assembly_class_388_266_scope_968::LEN as u64,
                "388",
                "258"
            )
            .operand_frame_variant(),
            None
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(
                crate::layout::assembly_class_383_258_scope_1011::LEN as u64,
                "383",
                "258"
            )
            .operand_frame_variant(),
            Some(super::AssemblyOperandFrameVariant::Standard)
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(
                crate::layout::assembly_class_383_258_scope_1011::LEN as u64,
                "383",
                "261"
            )
            .operand_frame_variant(),
            None
        );
        assert!(super::legacy_class_383_258_scope(
            crate::layout::assembly_class_383_258_scope_1011::LEN as u64,
            "383",
            "258"
        ));
        assert!(!super::legacy_class_383_258_scope(
            crate::layout::assembly_class_383_258_scope_1011::LEN as u64 - 1,
            "383",
            "258"
        ));
        assert_eq!(
            super::AssemblyScopeGeneration::new(744, "430", "262").operand_frame_variant(),
            Some(super::AssemblyOperandFrameVariant::Compact)
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(748, "430", "262").operand_frame_variant(),
            Some(super::AssemblyOperandFrameVariant::Standard)
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(671, "406", "261").operand_frame_variant(),
            Some(super::AssemblyOperandFrameVariant::Standard)
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(671, "406", "258").operand_frame_variant(),
            None
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(671, "430", "261").operand_frame_variant(),
            None
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(744, "327", "262").operand_frame_variant(),
            None
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(748, "430", "261").operand_frame_variant(),
            None
        );
    }

    #[test]
    fn axial_operands_project_component_and_document_root_qualifiers() {
        let component_scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:component-insert#200",
            crate::records::feature::DesignFeatureKind::ComponentInsert,
            200,
        );
        let origin_scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:joint-origin#80",
            crate::records::feature::DesignFeatureKind::JointOrigin,
            80,
        );
        let mut origin_scope = origin_scope;
        origin_scope.with_joint_origin_transform(
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
            .try_into()
            .unwrap(),
        );
        let occurrence =
            OccurrenceId::mint("test:model:occurrence#component").expect("identity grammar");
        let features = [
            feature(
                &component_scope.id,
                FeatureDefinition::InsertComponent {
                    occurrence: occurrence.clone(),
                },
            ),
            feature(
                &origin_scope.id,
                FeatureDefinition::DatumCoordinateSystem {
                    frame: cadmpeg_ir::features::FeatureCoordinateFrame::new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        Vector3::new(0.0, 1.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                    )
                    .unwrap(),
                },
            ),
        ];
        let targets = [
            DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
                component_insert_scope_record_index: 200,
                construction_record_index: 70,
                construction_class_tag: crate::records::DesignClassTag::try_from("305".to_owned())
                    .unwrap(),
                construction_byte_offset: 1_300,
                construction_transform_offset: 1_348,
                axis_record_index_offsets: [1_493, 1_509],
                construction_paired_class_tag: crate::records::DesignClassTag::try_from(
                    "261".to_owned(),
                )
                .unwrap(),
                construction_paired_byte_offset: 1_680,
                selectors: Box::new([selector(), second_selector()]),
            },
            DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
                scope_record_index: 80,
            },
        ];

        let qualifiers =
            targets.map(|target| DesignAssemblyOperandQualifier::AxialTarget { target });
        let scopes = vec![component_scope, origin_scope.clone()];
        let operands = super::project_qualified_operands(
            qualifiers.each_ref(),
            "f3d:Design/BulkStream.dat",
            &BTreeMap::new(),
            &scopes,
            &features,
        )
        .expect("complete axial operands");

        assert_eq!(
            operands[0].container,
            cadmpeg_ir::OperandContainer::Occurrence(occurrence)
        );
        assert!(operands[0]
            .object
            .starts_with("f3d:feature-input:connector#"));
        assert_eq!(operands[1].container, cadmpeg_ir::OperandContainer::Root);
        assert_eq!(
            operands[1].object,
            crate::ids::neutral_feature_id(&origin_scope).as_str()
        );

        let unlisted_operands = super::project_qualified_operands(
            qualifiers.each_ref(),
            "f3d:Design/BulkStream.dat",
            &BTreeMap::new(),
            &scopes,
            &features[..1],
        )
        .expect("frame-resolved unlisted root JointOrigin");
        assert_eq!(unlisted_operands[1].object, operands[1].object);
    }

    #[test]
    fn axial_connector_identity_excludes_axis_specific_occurrence_references() {
        let first = selector();
        let mut second = first.clone();
        second.occurrence_reference += 1;
        second.occurrence_reference_offset += 100;
        second.selector_asset_id = second
            .selector_asset_id
            .as_str()
            .to_ascii_uppercase()
            .try_into()
            .expect("GUID");
        second.selector_context_id = second
            .selector_context_id
            .as_str()
            .to_ascii_uppercase()
            .try_into()
            .expect("GUID");
        second.external_asset_id = second
            .external_asset_id
            .as_str()
            .to_ascii_uppercase()
            .try_into()
            .expect("GUID");
        assert!(first.selects_same_object(&second));
        assert_eq!(
            crate::ids::neutral_assembly_axial_object_id(&first),
            crate::ids::neutral_assembly_axial_object_id(&second)
        );

        second.external_object_reference += 1;
        assert_ne!(
            crate::ids::neutral_assembly_axial_object_id(&first),
            crate::ids::neutral_assembly_axial_object_id(&second)
        );
    }
}
