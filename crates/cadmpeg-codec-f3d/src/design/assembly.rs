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
    DesignAssemblyAxialOperandTarget, DesignAssemblyOperandPath, DesignComponentOccurrence,
    DesignParameterScope,
};

/// Return whether a 421-byte As-built scope uses one of the admitted legacy
/// generation pairs.
pub(crate) fn is_legacy_as_built_421(
    frame_length: u64,
    class_tag: &str,
    paired_class_tag: &str,
) -> bool {
    frame_length == 421
        && matches!(
            (class_tag, paired_class_tag),
            ("364", "272") | ("420", "262")
        )
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
        (399 | 627 | 633 | 637 | 692, 4) => Some((0, 4)),
        (604 | 732, 8) => Some((4, 8)),
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
        627 | 637 | 692 => Some([366, 377]),
        633 | 732 => Some([362, 373]),
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
        let operands = match (
            alignment.operand_paths.as_ref(),
            alignment.axial_operand_targets.as_ref(),
        ) {
            (Some(paths), None) => project_path_operands(paths, stream, &occurrences),
            (None, Some(targets)) => project_axial_operands(targets, stream, scopes, features),
            _ => None,
        };
        let Some(operands) = operands else {
            continue;
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
            angular_limits: alignment.angular_limits.as_ref().map(|limits| JointLimits {
                minimum: Some(limits.minimum),
                maximum: Some(limits.maximum),
            }),
            linear_limits: None,
            properties: BTreeMap::new(),
            native_ref: Some(scope.id.clone()),
        });
    }
    joints.into_values().collect()
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
        DesignAssemblyAxialOperandTarget, DesignAssemblyAxialSelectorIdentity, DesignParameterScope,
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
    fn alignment_lane_bounds_require_the_exact_frame_and_owner_count() {
        for (frame_length, owner_count, expected) in [
            (627, 4, (0, 4)),
            (633, 4, (0, 4)),
            (637, 4, (0, 4)),
            (692, 4, (0, 4)),
            (604, 8, (4, 8)),
            (732, 8, (4, 8)),
            (705, 6, (4, 6)),
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
        for frame_length in [627, 637, 692] {
            assert_eq!(
                super::operand_path_locator_offsets(frame_length),
                Some([366, 377])
            );
        }
        for frame_length in [633, 732] {
            assert_eq!(
                super::operand_path_locator_offsets(frame_length),
                Some([362, 373])
            );
        }
        for frame_length in [604, 705, 772] {
            assert_eq!(super::operand_path_locator_offsets(frame_length), None);
        }
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
