// SPDX-License-Identifier: Apache-2.0
//! Project exact Design assembly alignments into neutral joints.

use cadmpeg_core::decode::u64_from_index;

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::products::{
    AssemblyJoint, ExternalDocument, JointConnector, JointLimits, JointOperand, PairedJointKind,
};
use cadmpeg_ir::report::loss::LossNote;

use crate::design::components::ComponentInserts;
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::{
    assembly::{
        DesignAssemblyAxialOperandTarget, DesignAssemblyLimitKind, DesignAssemblyOperandQualifier,
    },
    assembly_features::DesignComponentOccurrence,
    scope::DesignParameterScope,
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
            length if length == u64_from_index(crate::layout::assembly_class_388_266_scope_968::LEN) => (
                (class_tag == "388" && paired_class_tag == "266").then_some(LegacyClass388), Some((28, 4, 8)), Some([
                    crate::layout::assembly_class_388_266_scope_968::OPERAND_PATH_LOCATOR_REFERENCES,
                    crate::layout::assembly_class_388_266_scope_968::OPERAND_PATH_LOCATOR_REFERENCES + 11,
                ])),
            length if length == u64_from_index(crate::layout::assembly_class_383_258_scope_1011::LEN) => (
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
pub(super) fn legacy_class_383_258_scope(
    frame_length: u64,
    class_tag: &str,
    paired_class_tag: &str,
) -> bool {
    frame_length == u64_from_index(crate::layout::assembly_class_383_258_scope_1011::LEN)
        && class_tag == "383"
        && paired_class_tag == "258"
}

impl LegacyAsBuilt421Generation {
    /// Owner-frame primary class for the six scalar lanes.
    pub(super) const fn owner_class_tag(self) -> &'static str {
        match self {
            Self::Class364 => "293",
            Self::Class420 => "378",
            Self::Class417 => "318",
            Self::Class457 => "418",
        }
    }

    /// Owner-frame paired class.
    const fn owner_paired_class_tag(self) -> &'static str {
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
    pub(super) const fn frame_paired_class_tag(self) -> &'static str {
        self.owner_paired_class_tag()
    }

    /// Byte length from the solved frame primary header to its paired header.
    pub(super) const fn frame_length(self) -> usize {
        match self {
            Self::Class364 => 389,
            Self::Class420 | Self::Class417 => 390,
            Self::Class457 => 385,
        }
    }

    /// Offset of the four-byte marker immediately before the solved matrix.
    pub(super) const fn matrix_prefix(self) -> usize {
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

/// Assembly joints and the losses of the joint scopes that do not project.
#[derive(Debug, Default)]
pub(crate) struct AssemblyJointProjection {
    pub(crate) joints: Vec<AssemblyJoint>,
    pub(crate) losses: Vec<LossNote>,
}

/// Project assembly scopes whose connector frames and operand qualifiers are complete.
///
/// Joints are product structure. They read scopes, local occurrence carriers,
/// and the `Component Insert` occurrences, and no features. A joint scope
/// whose operands, frames, limits, or alignment values do not project is
/// skipped and counted in one loss.
pub(crate) fn project_assembly_joints(
    scopes: &[DesignParameterScope],
    native_occurrences: &[DesignComponentOccurrence],
    inserts: &ComponentInserts,
) -> AssemblyJointProjection {
    let mut occurrences = BTreeMap::new();
    for occurrence in native_occurrences {
        let Some(stream) = native_stream(&occurrence.id) else {
            continue;
        };
        let key = (
            stream,
            occurrence.occurrence_guid.as_str().to_ascii_lowercase(),
        );
        match occurrences.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Some(occurrence));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                *entry.get_mut() = None;
            }
        }
    }
    let mut joints = BTreeMap::<String, AssemblyJoint>::new();
    let mut unprojected = 0_usize;
    for scope in scopes {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        match project_assembly_joint(scope, stream, &occurrences, scopes, inserts) {
            Ok(Some(joint)) => {
                joints.entry(joint.id.as_str().to_owned()).or_insert(joint);
            }
            Ok(None) => {}
            Err(_) => unprojected += 1,
        }
    }
    let mut losses = Vec::new();
    if unprojected != 0 {
        losses.push(F3dLossCode::AssemblyJointUnprojected.note(format!(
            "{unprojected} Design assembly joint scope(s) were retained natively: their operands, connector frames, limits, or alignment values do not project."
        )));
    }
    AssemblyJointProjection {
        joints: joints.into_values().collect(),
        losses,
    }
}

fn project_assembly_joint(
    scope: &DesignParameterScope,
    stream: &str,
    occurrences: &BTreeMap<(&str, String), Option<&DesignComponentOccurrence>>,
    scopes: &[DesignParameterScope],
    inserts: &ComponentInserts,
) -> Result<Option<AssemblyJoint>, CodecError> {
    let Some(alignment) = scope.assembly_alignment() else {
        return Ok(None);
    };
    let (frames, operands, limits) = match alignment.form.as_ref() {
        Some(
            crate::records::feature::assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                carriers,
                solved_frame,
                limits,
                ..
            },
        ) => (
            carriers
                .frames(solved_frame)
                .map_err(cadmpeg_core::CodecError::NotImplemented)?,
            {
                let [first, second] = carriers.selections().map(|selection| {
                    JointOperand::root(
                        crate::ids::neutral_assembly_legacy_object_id(selection),
                        Vec::new(),
                    )
                });
                [first, second]
            },
            limits.as_ref(),
        ),
        Some(crate::records::feature::assembly::DesignAssemblyAlignmentForm::Qualified(
            operands,
        )) => {
            let projected = project_qualified_operands(
                operands.each_ref().map(|operand| &operand.qualifier),
                stream,
                occurrences,
                scopes,
                inserts,
            )
            .ok_or_else(|| {
                CodecError::NotImplemented(
                    "assembly joint operand names no projected occurrence or origin".into(),
                )
            })?;
            (
                operands.each_ref().map(|operand| operand.frame.clone()),
                projected,
                None,
            )
        }
        _ => return Ok(None),
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
    let [first_operand, second_operand] = operands;
    let first_frame = super::components::neutral_transform(frames[0].transform)?;
    let second_frame = super::components::neutral_transform(frames[1].transform)?;
    let angle = cadmpeg_ir::scalar::FiniteReal::new(alignment.angle())
        .ok_or_else(|| CodecError::Malformed("joint angle must be finite".into()))?;
    let [x, y, z] = alignment.offset().map(|value| {
        cadmpeg_ir::scalar::FiniteReal::new(value * 10.0)
            .ok_or_else(|| CodecError::Malformed("joint translation_offset must be finite".into()))
    });
    let translation_offset = [x?, y?, z?];
    let mut joint = AssemblyJoint::paired(
        crate::ids::neutral_assembly_joint_id(scope),
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
    Ok(Some(joint))
}

/// Bind each native `Assemble` feature to the joint that product projection
/// emitted for its scope.
pub(crate) fn bind_assembly_joint_features(features: &mut [Feature], joints: &[AssemblyJoint]) {
    let joints = joints
        .iter()
        .filter_map(|joint| Some((joint.native_ref.as_deref()?, &joint.id)))
        .collect::<BTreeMap<_, _>>();
    for feature in features {
        let Some(joint) = feature
            .native_ref
            .as_deref()
            .and_then(|scope| joints.get(scope))
        else {
            continue;
        };
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ) {
            continue;
        }
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::AssemblyJoint {
                    joint: (*joint).clone(),
                },
            ));
    }
}

fn project_qualified_operands(
    qualifiers: [&DesignAssemblyOperandQualifier; 2],
    stream: &str,
    occurrences: &BTreeMap<(&str, String), Option<&DesignComponentOccurrence>>,
    scopes: &[DesignParameterScope],
    inserts: &ComponentInserts,
) -> Option<[JointOperand; 2]> {
    let project = |qualifier: &DesignAssemblyOperandQualifier| -> Option<JointOperand> {
        match qualifier {
            DesignAssemblyOperandQualifier::OccurrencePath { path } => {
                let root_guid = &path.occurrence_guids().first()?.value;
                let lookup_guid = root_guid.as_str().to_ascii_lowercase();
                let occurrence = occurrences.get(&(stream, lookup_guid)).copied().flatten();
                if occurrence.is_none() && !matches!(path.class_tag().as_str(), "330" | "386") {
                    return None;
                }
                let object = root_guid.as_str().to_ascii_lowercase();
                let subelement_guids = &path.occurrence_guids()[1..];

                let mut subelements = Vec::new();
                for guid in subelement_guids {
                    subelements.push(guid.value.as_str().to_ascii_lowercase());
                }
                Some(match occurrence {
                    Some(_) => JointOperand::occurrence(
                        crate::ids::neutral_component_occurrence_id(root_guid),
                        object,
                        subelements,
                    ),
                    None => JointOperand::external(
                        ExternalDocument::document_id(
                            path.identity_guids().first()?.value.as_str().to_owned(),
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
                        &crate::records::feature::scope::DesignFeatureKind::ComponentInsert,
                    )?;
                    Some(JointOperand::occurrence(
                        inserts.occurrence(&target_scope.id)?.clone(),
                        crate::ids::neutral_assembly_axial_object_id(&selectors[0]),
                        Vec::new(),
                    ))
                }
                DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
                    scope_record_index,
                } => project_joint_origin_operand(*scope_record_index, stream, scopes),
            },
            DesignAssemblyOperandQualifier::JointOrigin {
                scope_record_index, ..
            } => project_joint_origin_operand(*scope_record_index, stream, scopes),
        }
    };
    Some([project(qualifiers[0])?, project(qualifiers[1])?])
}

/// The current-document-root operand named by a `JointOrigin` scope whose
/// solved frame projects.
fn project_joint_origin_operand(
    scope_record_index: u32,
    stream: &str,
    scopes: &[DesignParameterScope],
) -> Option<JointOperand> {
    let target_scope = unique_scope(
        scopes,
        stream,
        scope_record_index,
        &crate::records::feature::scope::DesignFeatureKind::JointOrigin,
    )?;
    crate::design::feature_project::joint_origin_frame(target_scope).map(|_| {
        JointOperand::root(
            crate::ids::neutral_feature_id(target_scope).into_string(),
            Vec::new(),
        )
    })
}

fn unique_scope<'a>(
    scopes: &'a [DesignParameterScope],
    stream: &str,
    record_index: u32,
    kind: &crate::records::feature::scope::DesignFeatureKind,
) -> Option<&'a DesignParameterScope> {
    let mut matches = scopes.iter().filter(|scope| {
        native_stream(&scope.id) == Some(stream)
            && scope.record_index == record_index
            && scope.kind() == *kind
    });
    let scope = matches.next()?;
    matches.next().is_none().then_some(scope)
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::u64_from_index;

    use crate::records::feature::assembly::DesignAssemblyOperandQualifier;
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    use crate::records::feature::{
        assembly::{
            DesignAssemblyAxialOperandTarget, DesignAssemblyAxialSelectorIdentity,
            DesignAssemblyLimitKind,
        },
        scope::DesignParameterScope,
    };

    fn selector() -> DesignAssemblyAxialSelectorIdentity {
        DesignAssemblyAxialSelectorIdentity {
            axis_record_index: 10,
            axis_class_tag: crate::records::references::DesignClassTag::try_from("316".to_owned())
                .unwrap(),
            axis_byte_offset: 100,
            axis_paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            axis_paired_byte_offset: 120,
            selector_record_index: 13,
            selector_class_tag: crate::records::references::DesignClassTag::try_from(
                "277".to_owned(),
            )
            .unwrap(),
            selector_byte_offset: 200,
            selector_paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
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
            role_class_tag: crate::records::references::DesignClassTag::try_from("298".to_owned())
                .unwrap(),
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
    fn a_joint_frame_translation_overflow_skips_the_joint_with_a_loss() {
        use crate::records::feature::{
            assembly::{
                DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyOperandFrame,
            },
            scope::{DesignFeatureKind, DesignScopePayload},
        };
        let mut scopes = Vec::new();
        for index in [1, 2] {
            let mut scope = DesignParameterScope::empty(
                &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{index}"),
                DesignFeatureKind::JointOrigin,
                index,
            );
            scope.with_joint_origin_transform(
                crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
            );
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

        let projection = super::project_assembly_joints(
            &scopes,
            &[],
            &crate::design::components::ComponentInserts::default(),
        );
        assert!(projection.joints.is_empty());
        assert_eq!(
            projection
                .losses
                .iter()
                .map(|loss| loss.code.clone())
                .collect::<Vec<_>>(),
            [crate::loss::F3dLossCode::AssemblyJointUnprojected.kind()]
        );

        let native = crate::native::F3dNative {
            design_parameter_scopes: scopes,
            ..Default::default()
        };
        let native = serde_json::from_value(serde_json::to_value(native).unwrap()).unwrap();
        crate::writer::primitives::validate_assembly_projection(
            &cadmpeg_ir::document::CadIr::empty(),
            Some(&native),
        )
        .expect("the writer re-projects the same skipped joint");
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
                u64_from_index(crate::layout::assembly_class_388_266_scope_968::LEN),
                28,
                (4, 8),
            ),
            (
                u64_from_index(crate::layout::assembly_class_383_258_scope_1011::LEN),
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
                        800 + u64_from_index(owner_count),
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
                        800 + u64_from_index(owner_count),
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
        let class_388_length = u64_from_index(crate::layout::assembly_class_388_266_scope_968::LEN);
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
                u64_from_index(crate::layout::assembly_class_388_266_scope_968::LEN),
                "388",
                "266"
            )
            .operand_frame_variant(),
            Some(super::AssemblyOperandFrameVariant::LegacyClass388)
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(
                u64_from_index(crate::layout::assembly_class_388_266_scope_968::LEN),
                "388",
                "258"
            )
            .operand_frame_variant(),
            None
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(
                u64_from_index(crate::layout::assembly_class_383_258_scope_1011::LEN),
                "383",
                "258"
            )
            .operand_frame_variant(),
            Some(super::AssemblyOperandFrameVariant::Standard)
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(
                u64_from_index(crate::layout::assembly_class_383_258_scope_1011::LEN),
                "383",
                "261"
            )
            .operand_frame_variant(),
            None
        );
        assert!(super::legacy_class_383_258_scope(
            u64_from_index(crate::layout::assembly_class_383_258_scope_1011::LEN),
            "383",
            "258"
        ));
        assert!(!super::legacy_class_383_258_scope(
            u64_from_index(crate::layout::assembly_class_383_258_scope_1011::LEN) - 1,
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

    fn joint_origin_scope(record_index: u32, translation_x: f64) -> DesignParameterScope {
        let mut scope = DesignParameterScope::empty(
            &format!("f3d:Design/BulkStream.dat:joint-origin#{record_index}"),
            crate::records::feature::scope::DesignFeatureKind::JointOrigin,
            record_index,
        );
        scope.with_joint_origin_transform(
            [
                [1.0, 0.0, 0.0, translation_x],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
            .try_into()
            .unwrap(),
        );
        scope
    }

    #[test]
    fn axial_operands_project_component_and_document_root_qualifiers() {
        let mut component_scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:component-insert#200",
            crate::records::feature::scope::DesignFeatureKind::ComponentInsert,
            200,
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::ComponentInsert(slot) =
            component_scope.payload_mut()
        {
            *slot = Some(
                crate::records::feature::assembly_features::DesignComponentInsertConstruction {
                    relation_record_index: 201,
                    carrier_record_index: 202,
                    occurrence_identity: None,
                    neutron_role: "component-role".into(),
                    neutron_role_offset: 0,
                    placement: None,
                },
            );
        }
        let origin_scope = joint_origin_scope(80, 0.0);
        let targets = [
            DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
                component_insert_scope_record_index: 200,
                construction_record_index: 70,
                construction_class_tag: crate::records::references::DesignClassTag::try_from(
                    "305".to_owned(),
                )
                .unwrap(),
                construction_byte_offset: 1_300,
                construction_transform_offset: 1_348,
                axis_record_index_offsets: [1_493, 1_509],
                construction_paired_class_tag:
                    crate::records::references::DesignClassTag::try_from("261".to_owned()).unwrap(),
                construction_paired_byte_offset: 1_680,
                selectors: Box::new([selector(), second_selector()]),
            },
            DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
                scope_record_index: 80,
            },
        ];

        let qualifiers =
            targets.map(|target| DesignAssemblyOperandQualifier::AxialTarget { target });
        let scopes = vec![component_scope.clone(), origin_scope.clone()];
        let inserts = crate::design::components::ComponentInserts::decide(
            &scopes,
            &crate::design::test_support::synthetic_feature_timelines(std::slice::from_ref(
                &component_scope,
            )),
            &[],
        );
        let project =
            |scopes: &[DesignParameterScope],
             inserts: &crate::design::components::ComponentInserts| {
                super::project_qualified_operands(
                    qualifiers.each_ref(),
                    "f3d:Design/BulkStream.dat",
                    &BTreeMap::new(),
                    scopes,
                    inserts,
                )
            };
        let operands = project(&scopes, &inserts).expect("complete axial operands");

        assert_eq!(
            operands[0].container,
            cadmpeg_ir::OperandContainer::Occurrence {
                occurrence: crate::ids::neutral_component_insert_occurrence_id(&component_scope)
            }
        );
        assert!(operands[0]
            .object
            .starts_with("f3d:feature-input:connector#"));
        assert_eq!(operands[1].container, cadmpeg_ir::OperandContainer::Root {});
        assert_eq!(
            operands[1].object,
            crate::ids::neutral_feature_id(&origin_scope).as_str()
        );

        assert!(
            project(
                &scopes,
                &crate::design::components::ComponentInserts::default()
            )
            .is_none(),
            "an insert without a projected occurrence is no operand"
        );
        let unframed = [component_scope, joint_origin_scope(80, 1.0e308)];
        assert!(
            project(&unframed, &inserts).is_none(),
            "a JointOrigin whose frame does not project is no operand"
        );
    }

    #[test]
    fn assemble_features_bind_only_to_projected_joints() {
        let native = FeatureDefinition::Operation(FeatureOperation::Native {
            kind: "Assemble".into(),
            parameters: BTreeMap::new(),
        });
        let scope = "f3d:Design/BulkStream.dat:design-parameter-scope#3";
        let mut features = [
            feature(scope, native.clone()),
            feature(
                "f3d:Design/BulkStream.dat:design-parameter-scope#4",
                native.clone(),
            ),
        ];
        let mut joint = cadmpeg_ir::products::AssemblyJoint::paired(
            crate::ids::neutral_assembly_joint_id(&DesignParameterScope::empty(
                scope,
                crate::records::feature::scope::DesignFeatureKind::Assemble,
                3,
            )),
            cadmpeg_ir::products::PairedJointKind::Fixed {
                angle: None,
                translation_offset: None,
                angular_limits: None,
                linear_limits: None,
            },
            [0, 1].map(|index| cadmpeg_ir::products::JointConnector {
                operand: cadmpeg_ir::products::JointOperand::root(
                    format!("origin-{index}"),
                    Vec::new(),
                ),
                frame: cadmpeg_ir::transform::Transform::identity(),
                detached: false,
            }),
            None,
        );
        joint.native_ref = Some(scope.into());

        super::bind_assembly_joint_features(&mut features, std::slice::from_ref(&joint));

        assert_eq!(
            *features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::AssemblyJoint {
                joint: joint.id.clone()
            })
        );
        assert_eq!(*features[1].evaluation.definition(), native);
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
