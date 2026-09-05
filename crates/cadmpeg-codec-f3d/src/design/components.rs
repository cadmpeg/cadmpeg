// SPDX-License-Identifier: Apache-2.0
//! Project exact local component operations into neutral product structure.

use std::collections::BTreeMap;

use cadmpeg_ir::features::{Feature, FeatureDefinition};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};

use crate::records::DesignAssemblyOperandQualifier;
use crate::records::{DesignComponentOccurrence, DesignParameterScope};

/// Project components and occurrences proven by local component history operations.
pub(crate) fn project_local_components(
    scopes: &[DesignParameterScope],
    native_occurrences: &[DesignComponentOccurrence],
) -> (Vec<ProductDefinition>, Vec<Occurrence>) {
    let mut components = BTreeMap::new();
    let mut occurrences = BTreeMap::new();
    let mut native_by_guid = BTreeMap::new();
    for occurrence in native_occurrences {
        native_by_guid
            .entry(occurrence.occurrence_guid.to_ascii_lowercase())
            .and_modify(|candidate| *candidate = None)
            .or_insert(Some(occurrence));
    }

    for scope in scopes {
        if let Some(qualifiers) = scope
            .assembly_alignment()
            .and_then(|alignment| alignment.operand_qualifiers())
        {
            for qualifier in &qualifiers {
                let DesignAssemblyOperandQualifier::OccurrencePath { path } = qualifier else {
                    continue;
                };
                let Some(root) = path
                    .occurrence_guids
                    .first()
                    .and_then(|guid| native_by_guid.get(&guid.value.to_ascii_lowercase()))
                    .copied()
                    .flatten()
                else {
                    continue;
                };
                project_occurrence(
                    &mut components,
                    &mut occurrences,
                    &native_by_guid,
                    &root.component_guid,
                    &root.occurrence_guid,
                    root.transform.map(|frame| frame.value).unwrap_or([
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, 1.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                        [0.0, 0.0, 0.0, 1.0],
                    ]),
                );
            }
        }
        if let Some(operation) = scope.copy_paste_component_operation() {
            project_occurrence(
                &mut components,
                &mut occurrences,
                &native_by_guid,
                &operation.component_guid,
                &operation.source_occurrence_guid,
                operation.source_transform,
            );
            project_occurrence(
                &mut components,
                &mut occurrences,
                &native_by_guid,
                &operation.component_guid,
                &operation.copied_occurrence_guid,
                operation.copied_transform,
            );
        }
        if let Some(construction) = scope.derived_instance_construction() {
            project_occurrence(
                &mut components,
                &mut occurrences,
                &native_by_guid,
                &construction.component_guid,
                &construction.occurrence_guid,
                construction.transform,
            );
        }
        let Some(crate::records::DesignRectangularPatternInstances::Components { component_guid, seed, generated }) = scope
            .rectangular_pattern_construction()
            .and_then(|construction| construction.instances.as_ref())
        else {
            continue;
        };
        for occurrence in std::iter::once(seed).chain(generated) {
            project_occurrence(
                &mut components,
                &mut occurrences,
                &native_by_guid,
                component_guid,
                &occurrence.occurrence_guid,
                occurrence.instance.transform.value,
            );
        }
    }

    let mut occurrences = occurrences.into_values().collect::<Vec<_>>();
    for (ordinal, occurrence) in occurrences.iter_mut().enumerate() {
        occurrence.ordinal = u32::try_from(ordinal).unwrap_or(u32::MAX);
    }
    (components.into_values().collect(), occurrences)
}

/// Project a proven local occurrence into a `DerivedInstance` feature.
pub(crate) fn project_derived_instance_features(
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
) {
    for scope in scopes {
        let Some(construction) = scope.derived_instance_construction() else {
            continue;
        };
        let Some(feature) = features
            .iter_mut()
            .find(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()))
        else {
            continue;
        };
        if !matches!(feature.definition, FeatureDefinition::Native { .. }) {
            continue;
        }
        feature.definition = FeatureDefinition::InsertComponent {
            occurrence: crate::ids::neutral_component_occurrence_id(&construction.occurrence_guid),
        };
    }
}

/// Project the occurrence side of an external `Component Insert` when its
/// target reference table is not present in this container.
///
/// The operation still has a complete local placement. The prototype remains
/// explicitly unresolved because the source does not provide a target
/// document or a target object that this decoder can identify. Keeping the
/// occurrence in the product graph lets the feature retain its operation and
/// transform while native storage retains the exact external-reference role.
pub(crate) fn project_unresolved_component_insert_occurrences(
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    ordinal_start: usize,
) -> Vec<Occurrence> {
    let mut occurrences = Vec::new();
    for scope in scopes {
        let Some(construction) = scope.component_insert_construction() else {
            continue;
        };
        let Some(feature) = features
            .iter_mut()
            .find(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()))
        else {
            continue;
        };
        if !matches!(feature.definition, FeatureDefinition::Native { .. }) {
            continue;
        }

        let occurrence_id = crate::ids::neutral_component_insert_occurrence_id(scope);
        feature.definition = FeatureDefinition::InsertComponent {
            occurrence: occurrence_id.clone(),
        };
        occurrences.push(Occurrence {
            id: occurrence_id,
            prototype: PrototypeReference::Unresolved,
            parent: OccurrenceParent::Root,
            ordinal: u32::try_from(ordinal_start.saturating_add(occurrences.len()))
                .unwrap_or(u32::MAX),
            transform: neutral_transform(*construction.transform()),
            linked_prototype: None,
            scale: [1.0; 3],
            name: Some(construction.neutron_role.clone()),
            visible: None,
            link: None,
            native_ref: Some(scope.id.clone()),
        });
    }
    occurrences
}

fn project_occurrence(
    components: &mut BTreeMap<String, ProductDefinition>,
    occurrences: &mut BTreeMap<String, Occurrence>,
    native_by_guid: &BTreeMap<String, Option<&DesignComponentOccurrence>>,
    component_guid: &str,
    occurrence_guid: &str,
    transform: [[f64; 4]; 4],
) {
    let component_id = crate::ids::neutral_component_id(component_guid);
    let transform = neutral_transform(transform);
    project_component(components, component_guid);
    let occurrence_id = crate::ids::neutral_component_occurrence_id(occurrence_guid);
    occurrences
        .entry(occurrence_id.0.clone())
        .or_insert_with(|| Occurrence {
            id: occurrence_id,
            prototype: PrototypeReference::Local {
                definition: component_id,
            },
            parent: OccurrenceParent::Root,
            ordinal: 0,
            transform,
            linked_prototype: None,
            scale: [1.0; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: native_by_guid
                .get(&occurrence_guid.to_ascii_lowercase())
                .copied()
                .flatten()
                .map(|occurrence| occurrence.id.clone()),
        });
}

fn project_component(components: &mut BTreeMap<String, ProductDefinition>, component_guid: &str) {
    let component_id = crate::ids::neutral_component_id(component_guid);
    components
        .entry(component_id.0.clone())
        .or_insert_with(|| ProductDefinition {
            id: component_id,
            kind: ProductDefinitionKind::Part,
            source_name: None,
            label: None,
            description: None,
            part_number: None,
            bom_properties: BTreeMap::new(),
            bodies: Vec::new(),
            native_ref: None,
        });
}

fn neutral_transform(mut transform: [[f64; 4]; 4]) -> cadmpeg_ir::transform::Transform {
    for row in &mut transform[..3] {
        row[3] *= 10.0;
    }
    cadmpeg_ir::transform::Transform::from_rows(transform).expect("affine transform")
}

#[cfg(test)]
mod tests {
    use crate::records::{
        DesignComponentOccurrence, DesignCopyPasteComponentOperation,
        DesignDerivedInstanceConstruction, DesignParameterScope,
    };
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
    use cadmpeg_ir::products::PrototypeReference;

    #[test]
    fn local_component_placement_scales_only_translation_to_millimetres() {
        let transform = [
            [0.0, -1.0, 0.0, 1.25],
            [1.0, 0.0, 0.0, -2.5],
            [0.0, 0.0, 1.0, 3.75],
            [0.0, 0.0, 0.0, 1.0],
        ];
        assert_eq!(
            super::neutral_transform(transform).rows(),
            [
                [0.0, -1.0, 0.0, 12.5],
                [1.0, 0.0, 0.0, -25.0],
                [0.0, 0.0, 1.0, 37.5],
                [0.0, 0.0, 0.0, 1.0],
            ]
        );
    }

    #[test]
    fn equal_component_guids_share_one_definition_across_carrier_references() {
        const COMPONENT: &str = "11111111-2222-4333-8444-555555555555";
        const SOURCE: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
        const COPY: &str = "aaaaaaaa-bbbb-4ccc-8ddd-ffffffffffff";
        let occurrence = |record_index: u32, component_record_index: u64, occurrence_guid: &str| {
            DesignComponentOccurrence {
                id: format!("f3d:Design/BulkStream.dat:design-component-occurrence#{record_index}"),
                class_tag: "256".into(),
                record_index,
                byte_offset: u64::from(record_index),
                component_record_index,
                component_guid: COMPONENT.into(),
                component_guid_offset: 48,
                occurrence_guid: occurrence_guid.into(),
                occurrence_guid_offset: 124,
                occurrence_ordinal: 1,
                transform: None,
            }
        };
        let native_occurrences = [occurrence(100, 700, SOURCE), occurrence(101, 701, COPY)];
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#10",
            crate::records::DesignFeatureKind::CopyPaste,
            10,
        );
        if let crate::records::DesignScopePayload::CopyPaste(slot) = &mut scope.payload {
            *slot = Some(DesignCopyPasteComponentOperation {
                relation_record_index: 20,
                source_occurrence_record_index: 100,
                copied_occurrence_record_index: 101,
                component_guid: COMPONENT.into(),
                source_occurrence_guid: SOURCE.into(),
                copied_occurrence_guid: COPY.into(),
                source_transform: identity_matrix(),
                source_transform_offset: 0,
                copied_transform: identity_matrix(),
                copied_transform_offset: 0,
            });
        }

        let (definitions, occurrences) =
            super::project_local_components(&[scope], &native_occurrences);

        assert_eq!(definitions.len(), 1);
        assert_eq!(occurrences.len(), 2);
        let definition = definitions[0].id.clone();
        assert!(occurrences.iter().all(|occurrence| matches!(
            &occurrence.prototype,
            PrototypeReference::Local { definition: actual } if actual == &definition
        )));
    }

    #[test]
    fn derived_instance_projects_its_joined_local_occurrence() {
        const COMPONENT: &str = "11111111-2222-4333-8444-555555555555";
        const OCCURRENCE: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#385",
            crate::records::DesignFeatureKind::DerivedInstance,
            385,
        );
        if let crate::records::DesignScopePayload::DerivedInstance(slot) = &mut scope.payload {
            *slot = Some(DesignDerivedInstanceConstruction {
                reference_record_index: 305,
                relation_record_index: 383,
                carrier_record_index: 382,
                component_guid: COMPONENT.into(),
                occurrence_guid: OCCURRENCE.into(),
                transform: identity_matrix(),
                transform_offset: 473,
            });
        }
        let native_occurrence = DesignComponentOccurrence {
            id: "f3d:Design/BulkStream.dat:design-component-occurrence#382".into(),
            class_tag: "380".into(),
            record_index: 382,
            byte_offset: 0,
            component_record_index: 305,
            component_guid: COMPONENT.into(),
            component_guid_offset: 0,
            occurrence_guid: OCCURRENCE.into(),
            occurrence_guid_offset: 0,
            occurrence_ordinal: 1,
            transform: Some(crate::records::Located { value: identity_matrix(), offset: 209 }),
        };
        let (definitions, occurrences) =
            super::project_local_components(&[scope.clone()], &[native_occurrence]);
        assert_eq!(definitions.len(), 1);
        assert_eq!(occurrences.len(), 1);
        assert_eq!(
            occurrences[0].id,
            crate::ids::neutral_component_occurrence_id(OCCURRENCE)
        );
        assert!(matches!(
            &occurrences[0].prototype,
            PrototypeReference::Local { definition }
                if definition == &crate::ids::neutral_component_id(COMPONENT)
        ));

        let mut feature = Feature::new(
            FeatureId("f3d:model:feature#derived".into()),
            1,
            FeatureDefinition::Native {
                kind: "DerivedInstance".into(),
                parameters: std::collections::BTreeMap::new(),
            },
        );
        feature.native_ref = Some(scope.id.clone());
        super::project_derived_instance_features(std::slice::from_mut(&mut feature), &[scope]);
        assert_eq!(
            feature.definition,
            FeatureDefinition::InsertComponent {
                occurrence: crate::ids::neutral_component_occurrence_id(OCCURRENCE),
            }
        );
    }

    fn identity_matrix() -> [[f64; 4]; 4] {
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }
}
