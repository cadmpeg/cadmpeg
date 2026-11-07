// SPDX-License-Identifier: Apache-2.0

use super::{
    carrier, identity_matrix, placed, BTreeMap, DesignComponentOccurrencePlacement,
    DesignParameterScope, Feature, FeatureDefinition, FeatureId, FeatureOperation, DOCUMENT_ROOT,
    STREAM,
};
use crate::records::feature::occurrence_transfer::{
    DesignOccurrenceTransfer, DesignOccurrenceTransferSource, DesignPasteOwner,
};

const SOURCE: &str = "00000000-0000-4000-8000-000000000253";
const COPY: &str = "00000000-0000-4000-8000-000000000345";

fn translated(x: f64) -> [[f64; 4]; 4] {
    let mut transform = identity_matrix();
    transform[0][3] = x;
    transform
}

fn copy_scope(target: [[f64; 4]; 4]) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#348"),
        crate::records::feature::scope::DesignFeatureKind::CopyPaste,
        348,
    );
    scope.occurrence_transfer = Some(DesignOccurrenceTransfer {
        relation_record_index: 346,
        occurrence_record_index: 345,
        component_record_index: 159,
        target_owner: DesignPasteOwner {
            collection: 27,
            context: 26,
            component: 3,
        },
        target_transform: target.try_into().unwrap(),
        target_transform_offset: Some(41288),
        source: DesignOccurrenceTransferSource::Copy {
            source_transform: target.try_into().unwrap(),
            source_transform_offset: 41132,
        },
    });
    scope
}

fn project(
    scope: &DesignParameterScope,
    carriers: &[crate::records::feature::assembly_features::DesignComponentOccurrence],
) -> FeatureDefinition {
    let mut feature = Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:model:feature#copy").expect("identity grammar"),
        ordinal: 3,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "CopyPaste".into(),
                parameters: BTreeMap::new(),
            }),
        ),
        native_ref: Some(scope.id.clone()),
    };
    super::super::project_copy_paste_transfer_features(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(scope),
        carriers,
    );
    feature.evaluation.definition().clone()
}

#[test]
fn a_copy_transfer_inserts_the_carrier_its_relation_names() {
    let moved = translated(6.0);
    let carriers = [
        carrier(
            253,
            SOURCE,
            DesignComponentOccurrencePlacement::Base,
            DOCUMENT_ROOT,
        ),
        carrier(345, COPY, placed(2, moved), DOCUMENT_ROOT),
    ];
    assert_eq!(
        project(&copy_scope(moved), &carriers),
        FeatureDefinition::Operation(FeatureOperation::InsertComponent {
            occurrence: crate::ids::neutral_component_occurrence_id(&carriers[1].occurrence_guid),
        })
    );
}

#[test]
fn a_copy_transfer_stays_native_without_a_source_of_its_definition_or_its_placement() {
    let moved = translated(6.0);
    let source = || {
        carrier(
            253,
            SOURCE,
            DesignComponentOccurrencePlacement::Base,
            DOCUMENT_ROOT,
        )
    };
    let copy = || carrier(345, COPY, placed(2, moved), DOCUMENT_ROOT);
    let native = FeatureDefinition::Operation(FeatureOperation::Native {
        kind: "CopyPaste".into(),
        parameters: BTreeMap::new(),
    });

    let mut new_definition = copy();
    new_definition.component_record_index = 160;
    assert_eq!(
        project(&copy_scope(moved), &[source(), new_definition]),
        native
    );
    assert_eq!(project(&copy_scope(moved), &[copy()]), native);
    assert_eq!(
        project(&copy_scope(translated(5.0)), &[source(), copy()]),
        native
    );
}
