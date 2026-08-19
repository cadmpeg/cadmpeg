// SPDX-License-Identifier: Apache-2.0
//! Project exact local component operations into neutral product structure.

use std::collections::BTreeMap;

use cadmpeg_ir::features::{Feature, FeatureDefinition};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};

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
        if let Some(paths) = scope
            .assembly_alignment
            .as_ref()
            .and_then(|alignment| alignment.operand_paths.as_ref())
        {
            for path in paths {
                let Some(root) = path
                    .occurrence_guids
                    .first()
                    .and_then(|guid| native_by_guid.get(&guid.to_ascii_lowercase()))
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
                    root.transform.unwrap_or([
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, 1.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                        [0.0, 0.0, 0.0, 1.0],
                    ]),
                );
            }
        }
        if let Some(operation) = &scope.copy_paste_component_operation {
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
        let Some((instances, component_occurrences)) = scope
            .rectangular_pattern_construction
            .as_ref()
            .and_then(|construction| construction.instances.as_ref())
            .and_then(|instances| Some((instances, instances.component_occurrences.as_ref()?)))
        else {
            continue;
        };
        if instances.transforms.len()
            != component_occurrences
                .generated_occurrence_guids
                .len()
                .saturating_add(1)
        {
            continue;
        }
        project_occurrence(
            &mut components,
            &mut occurrences,
            &native_by_guid,
            &component_occurrences.component_guid,
            &component_occurrences.seed_occurrence_guid,
            instances.transforms[0],
        );
        for (occurrence_guid, transform) in component_occurrences
            .generated_occurrence_guids
            .iter()
            .zip(&instances.transforms[1..])
        {
            project_occurrence(
                &mut components,
                &mut occurrences,
                &native_by_guid,
                &component_occurrences.component_guid,
                occurrence_guid,
                *transform,
            );
        }
    }

    let mut occurrences = occurrences.into_values().collect::<Vec<_>>();
    for (ordinal, occurrence) in occurrences.iter_mut().enumerate() {
        occurrence.ordinal = u32::try_from(ordinal).unwrap_or(u32::MAX);
    }
    (components.into_values().collect(), occurrences)
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
        let Some(construction) = scope.component_insert_construction.as_ref() else {
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
            transform: neutral_transform(construction.transform),
            prototype_transform: cadmpeg_ir::transform::Transform::identity(),
            scale: [1.0; 3],
            name: Some(construction.neutron_role.clone()),
            linked_subelements: Vec::new(),
            visible: None,
            element_component: None,
            claim_child: None,
            copy_on_change: None,
            copy_on_change_source: None,
            copy_on_change_group: None,
            copy_on_change_touched: None,
            link_transform: None,
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
            prototype_transform: cadmpeg_ir::transform::Transform::identity(),
            scale: [1.0; 3],
            name: None,
            linked_subelements: Vec::new(),
            visible: None,
            element_component: None,
            claim_child: None,
            copy_on_change: None,
            copy_on_change_source: None,
            copy_on_change_group: None,
            copy_on_change_touched: None,
            link_transform: None,
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
    cadmpeg_ir::transform::Transform { rows: transform }
}

#[cfg(test)]
mod tests {
    use crate::records::{
        DesignComponentOccurrence, DesignCopyPasteComponentOperation, DesignParameterScope,
    };
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
                transform_offset: None,
            }
        };
        let native_occurrences = [occurrence(100, 700, SOURCE), occurrence(101, 701, COPY)];
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#10",
            "CopyPaste",
            10,
        );
        scope.copy_paste_component_operation = Some(DesignCopyPasteComponentOperation {
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

    fn identity_matrix() -> [[f64; 4]; 4] {
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }
}
