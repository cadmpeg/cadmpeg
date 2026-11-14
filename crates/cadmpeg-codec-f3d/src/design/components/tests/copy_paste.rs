// SPDX-License-Identifier: Apache-2.0

use super::{
    carrier, identity_matrix, placed, BTreeMap, DesignComponentOccurrence,
    DesignComponentOccurrencePlacement, DesignParameterScope, Feature, FeatureDefinition,
    FeatureId, FeatureOperation, Occurrence, DOCUMENT_ROOT, STREAM,
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
    scope.occurrence_transfer_intent = Some(
        crate::records::feature::occurrence_transfer::DesignOccurrenceTransferIntent {
            mode: crate::records::feature::occurrence_transfer::DesignOccurrenceTransferMode::Copy,
            relation_record_index: Some(346),
            occurrence_record_index: Some(345),
            component_record_index: Some(159),
            source_owner_component_record_index: None,
            target_owner_component_record_index: Some(159),
            source_transform: Some(target.try_into().unwrap()),
            target_transform: Some(target.try_into().unwrap()),
        },
    );
    scope
}

fn strict_copy_scope(target: [[f64; 4]; 4]) -> DesignParameterScope {
    let mut scope = copy_scope(target);
    scope.occurrence_transfer = Some(DesignOccurrenceTransfer {
        relation_record_index: 346,
        occurrence_record_index: 345,
        component_record_index: 159,
        target_owner: DesignPasteOwner {
            collection: 27,
            context: 26,
            component: 159,
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

fn cut_scope(target: [[f64; 4]; 4]) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#348"),
        crate::records::feature::scope::DesignFeatureKind::try_from("CutPaste".to_owned()).unwrap(),
        348,
    );
    scope.occurrence_transfer_intent = Some(
        crate::records::feature::occurrence_transfer::DesignOccurrenceTransferIntent {
            mode:
                crate::records::feature::occurrence_transfer::DesignOccurrenceTransferMode::Reparent,
            relation_record_index: Some(346),
            occurrence_record_index: Some(345),
            component_record_index: Some(159),
            source_owner_component_record_index: Some(160),
            target_owner_component_record_index: Some(159),
            source_transform: None,
            target_transform: Some(target.try_into().unwrap()),
        },
    );
    scope
}

fn source() -> DesignComponentOccurrence {
    carrier(
        253,
        SOURCE,
        DesignComponentOccurrencePlacement::Base,
        DOCUMENT_ROOT,
    )
}

fn copy() -> DesignComponentOccurrence {
    carrier(345, COPY, placed(2, translated(6.0)), DOCUMENT_ROOT)
}

fn product_occurrence(carrier: &DesignComponentOccurrence) -> Occurrence {
    serde_json::from_value(serde_json::json!({
        "id": crate::ids::neutral_component_occurrence_id(&carrier.occurrence_guid),
        "native_ref": carrier.id.clone(),
        "prototype": {"scope": "local", "definition": crate::ids::neutral_component_id(&carrier.component_guid)},
        "parent": {"kind": "root"},
        "ordinal": 0,
        "transform": [[1.0, 0.0, 0.0, 60.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
        "scale": [1.0, 1.0, 1.0],
    }))
    .unwrap()
}

fn native() -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Native {
        kind: "CopyPaste".into(),
        parameters: BTreeMap::new(),
    })
}

fn project(
    scope: &DesignParameterScope,
    carriers: &[DesignComponentOccurrence],
    projected: &[Occurrence],
) -> (FeatureDefinition, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let mut definitions = Vec::new();
    let mut component_definitions_by_record = BTreeMap::new();
    for carrier in carriers {
        let Some(stream) = crate::ids::native_stream(&carrier.id) else {
            continue;
        };
        let mut matching = projected
            .iter()
            .filter(|occurrence| occurrence.native_ref.as_deref() == Some(carrier.id.as_str()));
        let (Some(occurrence), None) = (matching.next(), matching.next()) else {
            continue;
        };
        if let cadmpeg_ir::products::PrototypeReference::Local { definition } =
            &occurrence.prototype
        {
            if !definitions
                .iter()
                .any(|candidate: &super::super::ProductDefinition| {
                    candidate.id.as_str() == definition.as_str()
                })
            {
                definitions.push(super::super::component_definition(
                    definition.clone(),
                    Vec::new(),
                ));
            }
            component_definitions_by_record.insert(
                (stream.to_owned(), carrier.component_record_index),
                definition.clone(),
            );
        }
    }
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
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(native()),
        native_ref: Some(scope.id.clone()),
    };
    let losses = super::super::project_copy_paste_transfer_features(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(scope),
        carriers,
        projected,
        &definitions,
        &component_definitions_by_record,
    );
    (feature.evaluation.definition().clone(), losses)
}

#[test]
fn a_strict_copy_transfer_retains_history_when_its_occurrence_resolves() {
    let projected = [product_occurrence(&copy())];
    let (definition, losses) = project(
        &strict_copy_scope(translated(6.0)),
        &[source(), copy()],
        &projected,
    );
    let FeatureDefinition::Operation(FeatureOperation::ComponentTransfer {
        mode,
        occurrence,
        component,
        target_owner,
        source_transform,
        target_transform,
        ..
    }) = definition
    else {
        panic!("a fully joined CopyPaste retains its transfer history");
    };
    assert_eq!(mode, cadmpeg_ir::features::ComponentTransferMode::Copy);
    assert_eq!(occurrence, Some(projected[0].id.clone()));
    assert!(component.is_some());
    assert_eq!(component, target_owner);
    assert_eq!(source_transform, target_transform);
    assert_eq!(source_transform.unwrap().rows()[0][3], 60.0);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureOccurrenceTransferSourceUnresolved.kind()
    );
}

#[test]
fn a_copy_transfer_keeps_decoded_placement_when_occurrence_join_is_missing() {
    let scope = copy_scope(translated(6.0));
    let (definition, losses) = project(&scope, &[source()], &[]);
    let FeatureDefinition::Operation(FeatureOperation::ComponentTransfer {
        mode,
        occurrence,
        source_transform,
        target_transform,
        ..
    }) = definition
    else {
        panic!("unresolved CopyPaste must retain its transfer operation");
    };
    assert_eq!(mode, cadmpeg_ir::features::ComponentTransferMode::Copy);
    assert_eq!(occurrence, None);
    assert_eq!(source_transform, target_transform);
    assert_eq!(target_transform.unwrap().rows()[0][3], 60.0);
    let codes = losses
        .iter()
        .map(|loss| loss.code.local_code())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"feature.occurrence-transfer-occurrence-unresolved"));
    assert!(codes.contains(&"feature.occurrence-transfer-owner-unresolved"));
    assert!(codes.contains(&"feature.occurrence-transfer-source-unresolved"));
    assert!(losses.iter().all(|loss| loss.message.contains(&scope.id)));
}

#[test]
fn unreadable_copy_placements_emit_the_placement_loss() {
    let mut scope = copy_scope(translated(6.0));
    let intent = scope.occurrence_transfer_intent.as_mut().unwrap();
    intent.source_transform = None;
    intent.target_transform = None;
    let projected = [product_occurrence(&copy())];
    let (definition, losses) = project(&scope, &[source(), copy()], &projected);
    let FeatureDefinition::Operation(FeatureOperation::ComponentTransfer {
        source_transform,
        target_transform,
        ..
    }) = definition
    else {
        panic!("an unreadable CopyPaste placement keeps its transfer operation");
    };
    assert_eq!(source_transform, None);
    assert_eq!(target_transform, None);
    assert!(losses.iter().any(|loss| {
        loss.code.local_code() == "feature.occurrence-transfer-placement-unresolved"
    }));
    assert!(losses
        .iter()
        .any(|loss| { loss.code.local_code() == "feature.occurrence-transfer-source-unresolved" }));
}

#[test]
fn a_copy_transfer_uses_the_stream_qualified_projected_occurrence_id() {
    let primary = copy();
    let mut other_stream = copy();
    other_stream.id = "f3d:Other/BulkStream.dat:design-component-occurrence#345".into();
    other_stream.component_guid = super::guid("2ad641b3-9ea3-4e5f-a4da-926c3faeccad");
    let mut projected_primary = product_occurrence(&primary);
    projected_primary.id =
        crate::ids::stream_component_occurrence_id(STREAM, &primary.occurrence_guid);
    let allocated_definition =
        cadmpeg_ir::ids::ProductDefinitionId::mint("f3d:model:product-definition#allocated")
            .unwrap();
    projected_primary.prototype = cadmpeg_ir::products::PrototypeReference::Local {
        definition: allocated_definition.clone(),
    };
    let mut projected_other = product_occurrence(&other_stream);
    projected_other.id = crate::ids::stream_component_occurrence_id(
        "f3d:Other/BulkStream.dat",
        &other_stream.occurrence_guid,
    );
    assert_ne!(projected_primary.id, projected_other.id);
    let (definition, _) = project(
        &copy_scope(translated(6.0)),
        &[primary.clone(), other_stream.clone()],
        &[projected_primary.clone(), projected_other],
    );
    let FeatureDefinition::Operation(FeatureOperation::ComponentTransfer {
        occurrence,
        component,
        ..
    }) = definition
    else {
        panic!("a partially joined CopyPaste must retain its operation");
    };
    assert_eq!(occurrence, Some(projected_primary.id));
    assert_eq!(component, Some(allocated_definition.clone()));
    assert_ne!(
        component,
        Some(crate::ids::neutral_component_id(&primary.component_guid))
    );
    assert_ne!(
        allocated_definition,
        crate::ids::neutral_component_id(&other_stream.component_guid)
    );
}

#[test]
fn a_cut_transfer_projects_when_its_source_owner_does_not_resolve() {
    let projected = [product_occurrence(&copy())];
    let (definition, losses) = project(&cut_scope(translated(6.0)), &[copy()], &projected);
    let FeatureDefinition::Operation(FeatureOperation::ComponentTransfer {
        mode,
        occurrence,
        component,
        source_owner,
        target_owner,
        source_transform,
        target_transform,
    }) = definition
    else {
        panic!("CutPaste must project as a component transfer");
    };
    assert_eq!(mode, cadmpeg_ir::features::ComponentTransferMode::Reparent);
    assert_eq!(occurrence, Some(projected[0].id.clone()));
    assert!(component.is_some());
    assert_eq!(source_owner, None);
    assert_eq!(target_owner, component);
    assert_eq!(source_transform, None);
    assert_eq!(target_transform.unwrap().rows()[0][3], 60.0);
    assert!(losses
        .iter()
        .any(|loss| { loss.code.local_code() == "feature.occurrence-transfer-owner-unresolved" }));
}
