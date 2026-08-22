// SPDX-License-Identifier: Apache-2.0
//! Project exact Design assembly alignments into neutral joints.

use std::collections::BTreeMap;

use cadmpeg_ir::features::{Feature, FeatureDefinition};
use cadmpeg_ir::products::{
    AssemblyJoint, ExternalDocumentReference, ExternalResolution, JointKind, JointLimits,
    JointOperand,
};

use crate::ids::native_stream;
use crate::records::{
    DesignAssemblyAxialOperandTarget, DesignAssemblyLegacyOperand, DesignAssemblyLimitKind,
    DesignAssemblyOperandPath, DesignComponentOccurrence, DesignParameterScope,
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

/// Select the exact operand-frame grammar admitted for an `Assemble` scope.
///
/// The class-430 generation is keyed by both class tags because its 744- and
/// 748-byte spans are also used by other scope families with different
/// payloads. Those spans must not become a frame-length-only admission.
/// The 671-byte generation is likewise keyed to class-406 paired with
/// class-261; its standard payload is not a generic length variant.
pub(crate) fn operand_frame_variant(
    frame_length: u64,
    class_tag: &str,
    paired_class_tag: &str,
) -> Option<AssemblyOperandFrameVariant> {
    match frame_length {
        length
            if length == crate::layout::assembly_class_388_266_scope_968::LEN as u64
                && class_tag == "388"
                && paired_class_tag == "266" =>
        {
            Some(AssemblyOperandFrameVariant::LegacyClass388)
        }
        length
            if length == crate::layout::assembly_class_383_258_scope_1011::LEN as u64
                && class_tag == "383"
                && paired_class_tag == "258" =>
        {
            Some(AssemblyOperandFrameVariant::Standard)
        }
        627 | 637 | 692 => Some(AssemblyOperandFrameVariant::Standard),
        671 if class_tag == "406" && paired_class_tag == "261" => {
            Some(AssemblyOperandFrameVariant::Standard)
        }
        633 | 732 => Some(AssemblyOperandFrameVariant::Compact),
        744 if class_tag == "430" && paired_class_tag == "262" => {
            Some(AssemblyOperandFrameVariant::Compact)
        }
        748 if class_tag == "430" && paired_class_tag == "262" => {
            Some(AssemblyOperandFrameVariant::Standard)
        }
        705 | 772 => Some(AssemblyOperandFrameVariant::Axial),
        _ => None,
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

/// Return the half-open owner-lane range that carries assembly alignment.
///
/// The serialized frame length fixes both the Cartesian/axial form and the
/// number of placement lanes that precede the alignment values.
pub(crate) const fn alignment_lane_bounds(
    frame_length: u64,
    owner_count: usize,
) -> Option<(usize, usize)> {
    match (frame_length, owner_count) {
        (length, 28) if length == crate::layout::assembly_class_388_266_scope_968::LEN as u64 => {
            Some((4, 8))
        }
        (length, 20) if length == crate::layout::assembly_class_383_258_scope_1011::LEN as u64 => {
            Some((8, 12))
        }
        (399 | 627 | 633 | 637 | 692, 4) => Some((0, 4)),
        (671, 6) => Some((4, 6)),
        (604 | 732 | 744 | 748, 8) => Some((4, 8)),
        (705, 6) => Some((4, 6)),
        (772, 10) => Some((8, 10)),
        _ => None,
    }
}

/// Return the scope-relative marker offsets of the two ordered operand-path
/// locator references carried by a non-axial assembly frame.
pub(crate) const fn operand_path_locator_offsets(frame_length: u64) -> Option<[usize; 2]> {
    match frame_length {
        399 => Some([51, 62]),
        627 | 637 | 692 | 748 => Some([366, 377]),
        671 => Some([
            crate::layout::assembly_class_406_261_scope_671::FIRST_LOCATOR_REFERENCE,
            crate::layout::assembly_class_406_261_scope_671::SECOND_LOCATOR_REFERENCE,
        ]),
        633 | 732 | 744 => Some([362, 373]),
        _ => None,
    }
}

/// Project assembly scopes whose connector frames and operand qualifiers are complete.
pub(crate) fn project_assembly_joints(
    scopes: &[DesignParameterScope],
    native_occurrences: &[DesignComponentOccurrence],
    features: &[Feature],
) -> Vec<AssemblyJoint> {
    let mut occurrences = BTreeMap::new();
    for occurrence in native_occurrences {
        let Some(stream) = native_stream(&occurrence.id) else {
            continue;
        };
        occurrences
            .entry((stream, occurrence.occurrence_guid.to_ascii_lowercase()))
            .and_modify(|candidate| *candidate = None)
            .or_insert(Some(occurrence));
    }
    let mut joints = BTreeMap::new();
    for scope in scopes {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(alignment) = &scope.assembly_alignment else {
            continue;
        };
        let Some(frames) = &alignment.operand_frames else {
            continue;
        };
        let operands = if let Some(carriers) = alignment.legacy_operand_carriers.as_ref() {
            project_legacy_operands(carriers)
        } else {
            match (
                alignment.operand_paths.as_ref(),
                alignment.axial_operand_targets.as_ref(),
            ) {
                (Some(paths), None) => project_path_operands(paths, stream, &occurrences),
                (None, Some(targets)) => project_axial_operands(targets, stream, scopes, features),
                _ => None,
            }
        };
        let Some(operands) = operands else {
            continue;
        };
        let (angular_limits, linear_limits) = match alignment.limits.as_ref() {
            Some(limits) => {
                let projected = JointLimits {
                    minimum: Some(limits.minimum),
                    maximum: Some(limits.maximum),
                };
                match limits.kind {
                    DesignAssemblyLimitKind::Angular => (Some(projected), None),
                    DesignAssemblyLimitKind::Linear => (None, Some(projected)),
                }
            }
            None => (None, None),
        };
        let id = crate::ids::neutral_assembly_joint_id(scope);
        joints.entry(id.0.clone()).or_insert_with(|| AssemblyJoint {
            id,
            kind: JointKind::Fixed,
            operands,
            frames: frames
                .iter()
                .map(|frame| neutral_transform(frame.transform))
                .collect(),
            offset_frames: Vec::new(),
            suppressed: false,
            detached: [false; 2],
            angle: Some(alignment.angle),
            translation_offset: Some(alignment.offset.map(|value| value * 10.0)),
            distance: None,
            distance2: None,
            angular_limits,
            linear_limits,
            properties: BTreeMap::new(),
            native_ref: Some(scope.id.clone()),
        });
    }
    joints.into_values().collect()
}

fn project_legacy_operands(
    carriers: &[DesignAssemblyLegacyOperand; 2],
) -> Option<Vec<JointOperand>> {
    carriers
        .iter()
        .map(|carrier| {
            Some(JointOperand {
                occurrence: None,
                external_document: None,
                object: Some(crate::ids::neutral_assembly_legacy_object_id(
                    &carrier.selection,
                )),
                subelements: Vec::new(),
            })
        })
        .collect()
}

fn project_path_operands(
    paths: &[DesignAssemblyOperandPath; 2],
    stream: &str,
    occurrences: &BTreeMap<(&str, String), Option<&DesignComponentOccurrence>>,
) -> Option<Vec<JointOperand>> {
    paths
        .iter()
        .map(|path| {
            let root_guid = path.occurrence_guids.first()?;
            let occurrence = occurrences
                .get(&(stream, root_guid.to_ascii_lowercase()))
                .copied()
                .flatten();
            if occurrence.is_none() && path.class_tag != "386" {
                return None;
            }
            Some(JointOperand {
                occurrence: occurrence
                    .map(|_| crate::ids::neutral_component_occurrence_id(root_guid)),
                external_document: occurrence.is_none().then(|| ExternalDocumentReference {
                    path: None,
                    document_id: path.identity_guids.first().cloned(),
                    resolution: ExternalResolution::Unresolved,
                }),
                object: Some(root_guid.to_ascii_lowercase()),
                subelements: path.occurrence_guids[1..]
                    .iter()
                    .map(|guid| guid.to_ascii_lowercase())
                    .collect(),
            })
        })
        .collect()
}

fn project_axial_operands(
    targets: &[DesignAssemblyAxialOperandTarget; 2],
    stream: &str,
    scopes: &[DesignParameterScope],
    features: &[Feature],
) -> Option<Vec<JointOperand>> {
    targets
        .iter()
        .map(|target| match target {
            DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
                component_insert_scope_record_index,
                selectors,
                ..
            } => {
                let target_scope = unique_scope(
                    scopes,
                    stream,
                    *component_insert_scope_record_index,
                    "Component Insert",
                )?;
                let feature = unique_feature(features, &target_scope.id)?;
                let FeatureDefinition::InsertComponent { occurrence } = &feature.definition else {
                    return None;
                };
                Some(JointOperand {
                    occurrence: Some(occurrence.clone()),
                    external_document: None,
                    object: Some(crate::ids::neutral_assembly_axial_object_id(&selectors[0])),
                    subelements: Vec::new(),
                })
            }
            DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin { scope_record_index } => {
                let target_scope =
                    unique_scope(scopes, stream, *scope_record_index, "JointOrigin")?;
                if let Some(feature) = unique_feature(features, &target_scope.id) {
                    if !matches!(
                        feature.definition,
                        FeatureDefinition::DatumCoordinateSystem { .. }
                    ) {
                        return None;
                    }
                } else if target_scope.joint_origin_transform.is_none() {
                    return None;
                }
                Some(JointOperand {
                    occurrence: None,
                    external_document: None,
                    object: Some(crate::ids::neutral_feature_id(target_scope).0),
                    subelements: Vec::new(),
                })
            }
        })
        .collect()
}

fn unique_scope<'a>(
    scopes: &'a [DesignParameterScope],
    stream: &str,
    record_index: u32,
    kind: &str,
) -> Option<&'a DesignParameterScope> {
    let mut matches = scopes.iter().filter(|scope| {
        native_stream(&scope.id) == Some(stream)
            && scope.record_index == record_index
            && scope.kind == kind
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

fn neutral_transform(mut transform: [[f64; 4]; 4]) -> cadmpeg_ir::transform::Transform {
    for row in &mut transform[..3] {
        row[3] *= 10.0;
    }
    cadmpeg_ir::transform::Transform { rows: transform }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
    use cadmpeg_ir::ids::OccurrenceId;
    use cadmpeg_ir::math::{Point3, Vector3};

    use crate::records::{
        DesignAssemblyAxialOperandTarget, DesignAssemblyAxialSelectorIdentity,
        DesignAssemblyLimitKind, DesignParameterScope,
    };

    fn selector() -> DesignAssemblyAxialSelectorIdentity {
        DesignAssemblyAxialSelectorIdentity {
            axis_record_index: 10,
            axis_class_tag: "316".into(),
            axis_byte_offset: 100,
            axis_paired_class_tag: "261".into(),
            axis_paired_byte_offset: 120,
            selector_record_index: 13,
            selector_class_tag: "277".into(),
            selector_byte_offset: 200,
            selector_paired_class_tag: "261".into(),
            selector_paired_byte_offset: 560,
            nested_record_index: 16,
            nested_record_index_offset: 223,
            selector_asset_id: "abcdefab-cdef-4abc-8def-abcdefabcdef".into(),
            selector_asset_id_offset: 241,
            selector_context_id: "bcdefabc-defa-4bcd-8efa-bcdefabcdefa".into(),
            selector_context_id_offset: 317,
            occurrence_reference: 1_001,
            occurrence_reference_offset: 402,
            external_object_reference: 2_001,
            external_object_reference_offset: 417,
            external_segment: 7,
            external_segment_offset: 426,
            external_asset_id: "abcdefab-cdef-4abc-8def-abcdefabcdef".into(),
            external_asset_id_offset: 434,
            external_link_name: "component-link".into(),
            external_link_name_offset: 511,
            external_property_key: None,
            external_property_key_offset: None,
            external_version_urn: None,
            external_version_urn_offset: None,
            role_record_index: 18,
            role_class_tag: "298".into(),
            role_byte_offset: 600,
            occurrence_role: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".into(),
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
            id: FeatureId(format!("test:model:feature#{native_ref}")),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: Some(native_ref.into()),
        }
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
            super::neutral_transform(transform).rows,
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
                super::alignment_lane_bounds(frame_length, owner_count),
                Some(expected)
            );
        }
        for (frame_length, owner_count) in [(627, 6), (732, 6), (705, 8), (772, 8), (604, 4)] {
            assert_eq!(
                super::alignment_lane_bounds(frame_length, owner_count),
                None
            );
        }
    }

    #[test]
    fn operand_path_locator_offsets_follow_the_frame_layout() {
        for frame_length in [627, 637, 692, 748] {
            assert_eq!(
                super::operand_path_locator_offsets(frame_length),
                Some([366, 377])
            );
        }
        for frame_length in [633, 732, 744] {
            assert_eq!(
                super::operand_path_locator_offsets(frame_length),
                Some([362, 373])
            );
        }
        assert_eq!(super::operand_path_locator_offsets(671), Some([388, 399]));
        for frame_length in [604, 705, 772] {
            assert_eq!(super::operand_path_locator_offsets(frame_length), None);
        }
    }

    #[test]
    fn operand_frames_are_scoped_by_class_pair() {
        assert_eq!(
            super::operand_frame_variant(
                crate::layout::assembly_class_388_266_scope_968::LEN as u64,
                "388",
                "266"
            ),
            Some(super::AssemblyOperandFrameVariant::LegacyClass388)
        );
        assert_eq!(
            super::operand_frame_variant(
                crate::layout::assembly_class_388_266_scope_968::LEN as u64,
                "388",
                "258"
            ),
            None
        );
        assert_eq!(
            super::operand_frame_variant(
                crate::layout::assembly_class_383_258_scope_1011::LEN as u64,
                "383",
                "258"
            ),
            Some(super::AssemblyOperandFrameVariant::Standard)
        );
        assert_eq!(
            super::operand_frame_variant(
                crate::layout::assembly_class_383_258_scope_1011::LEN as u64,
                "383",
                "261"
            ),
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
            super::operand_frame_variant(744, "430", "262"),
            Some(super::AssemblyOperandFrameVariant::Compact)
        );
        assert_eq!(
            super::operand_frame_variant(748, "430", "262"),
            Some(super::AssemblyOperandFrameVariant::Standard)
        );
        assert_eq!(
            super::operand_frame_variant(671, "406", "261"),
            Some(super::AssemblyOperandFrameVariant::Standard)
        );
        assert_eq!(super::operand_frame_variant(671, "406", "258"), None);
        assert_eq!(super::operand_frame_variant(671, "430", "261"), None);
        assert_eq!(super::operand_frame_variant(744, "327", "262"), None);
        assert_eq!(super::operand_frame_variant(748, "430", "261"), None);
    }

    #[test]
    fn axial_operands_project_component_and_document_root_qualifiers() {
        let component_scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:component-insert#200",
            "Component Insert",
            200,
        );
        let origin_scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:joint-origin#80",
            "JointOrigin",
            80,
        );
        let mut origin_scope = origin_scope;
        origin_scope.joint_origin_transform = Some([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]);
        let occurrence = OccurrenceId("test:model:occurrence#component".into());
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
                    origin: Point3::new(0.0, 0.0, 0.0),
                    x_axis: Vector3::new(1.0, 0.0, 0.0),
                    y_axis: Vector3::new(0.0, 1.0, 0.0),
                    z_axis: Vector3::new(0.0, 0.0, 1.0),
                },
            ),
        ];
        let targets = [
            DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
                component_insert_scope_record_index: 200,
                construction_record_index: 70,
                construction_class_tag: "305".into(),
                construction_byte_offset: 1_300,
                construction_transform_offset: 1_348,
                axis_record_index_offsets: [1_493, 1_509],
                construction_paired_class_tag: "261".into(),
                construction_paired_byte_offset: 1_680,
                selectors: Box::new([selector(), second_selector()]),
            },
            DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
                scope_record_index: 80,
            },
        ];

        let scopes = vec![component_scope, origin_scope.clone()];
        let operands = super::project_axial_operands(
            &targets,
            "f3d:Design/BulkStream.dat",
            &scopes,
            &features,
        )
        .expect("complete axial operands");

        assert_eq!(operands[0].occurrence, Some(occurrence));
        assert!(operands[0].external_document.is_none());
        assert!(operands[0]
            .object
            .as_deref()
            .is_some_and(|object| object.starts_with("f3d:feature-input:connector#")));
        assert!(operands[1].occurrence.is_none());
        assert!(operands[1].external_document.is_none());
        assert_eq!(
            operands[1].object.as_deref(),
            Some(crate::ids::neutral_feature_id(&origin_scope).0.as_str())
        );

        let unlisted_operands = super::project_axial_operands(
            &targets,
            "f3d:Design/BulkStream.dat",
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
        second.selector_asset_id.make_ascii_uppercase();
        second.selector_context_id.make_ascii_uppercase();
        second.external_asset_id.make_ascii_uppercase();
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
