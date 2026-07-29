// SPDX-License-Identifier: Apache-2.0
//! Project exact local component operations into neutral product structure.

use std::collections::BTreeMap;

use cadmpeg_ir::products::{Component, ComponentKind, ComponentReference, Occurrence};

use crate::records::{DesignComponentOccurrence, DesignParameterScope};

/// Project components and occurrences proven by local component history operations.
pub(crate) fn project_local_components(
    scopes: &[DesignParameterScope],
    native_occurrences: &[DesignComponentOccurrence],
) -> (Vec<Component>, Vec<Occurrence>) {
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

    (
        components.into_values().collect(),
        occurrences.into_values().collect(),
    )
}

fn project_occurrence(
    components: &mut BTreeMap<String, Component>,
    occurrences: &mut BTreeMap<String, Occurrence>,
    native_by_guid: &BTreeMap<String, Option<&DesignComponentOccurrence>>,
    component_guid: &str,
    occurrence_guid: &str,
    transform: [[f64; 4]; 4],
) {
    let component_id = crate::ids::neutral_component_id(component_guid);
    let transform = neutral_transform(transform);
    components
        .entry(component_id.0.clone())
        .or_insert_with(|| Component {
            id: component_id.clone(),
            kind: ComponentKind::Part,
            source_name: None,
            label: None,
            description: None,
            part_number: None,
            bom_properties: BTreeMap::new(),
            parent: None,
            local_transform: identity_transform(),
            resolved_transform: identity_transform(),
            components: Vec::new(),
            occurrences: Vec::new(),
            native_ref: None,
        });
    let occurrence_id = crate::ids::neutral_component_occurrence_id(occurrence_guid);
    occurrences
        .entry(occurrence_id.0.clone())
        .or_insert_with(|| Occurrence {
            id: occurrence_id,
            prototype: ComponentReference::Local {
                component: component_id,
            },
            parent: None,
            array_index: None,
            local_transform: transform,
            prototype_transform: identity_transform(),
            resolved_transform: transform,
            scale: [1.0; 3],
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

fn identity_transform() -> [[f64; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn neutral_transform(mut transform: [[f64; 4]; 4]) -> [[f64; 4]; 4] {
    for row in &mut transform[..3] {
        row[3] *= 10.0;
    }
    transform
}

#[cfg(test)]
mod tests {
    #[test]
    fn local_component_placement_scales_only_translation_to_millimetres() {
        let transform = [
            [0.0, -1.0, 0.0, 1.25],
            [1.0, 0.0, 0.0, -2.5],
            [0.0, 0.0, 1.0, 3.75],
            [0.0, 0.0, 0.0, 1.0],
        ];
        assert_eq!(
            super::neutral_transform(transform),
            [
                [0.0, -1.0, 0.0, 12.5],
                [1.0, 0.0, 0.0, -25.0],
                [0.0, 0.0, 1.0, 37.5],
                [0.0, 0.0, 0.0, 1.0],
            ]
        );
    }
}
