// SPDX-License-Identifier: Apache-2.0
//! Transfer of exact CATIA reference history nodes.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, PrincipalPlane, SketchSpace};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement};

use crate::native::{CatiaDesignObject, CatiaNative, CatiaObjectRecord};
use crate::object_graph::{PayloadField, PayloadSubtype};

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DesignFeatureTransfer {
    pub(crate) feature_ids: HashMap<String, FeatureId>,
    pub(crate) native_operation_records: HashSet<String>,
    pub(crate) principal_plane_records: HashSet<String>,
    pub(crate) sketch_owner_records: HashSet<String>,
}

impl DesignFeatureTransfer {
    pub(crate) fn consumed_records(&self) -> HashSet<String> {
        self.principal_plane_records
            .union(&self.sketch_owner_records)
            .chain(self.native_operation_records.iter())
            .cloned()
            .collect()
    }

    /// Bind parameters to a transferred feature only through their exact
    /// entity-record and object-record ownership chain.
    pub(crate) fn assign_parameter_owners(&self, ir: &mut CadIr, native: &CatiaNative) {
        let entities = native
            .entity_records
            .iter()
            .map(|entity| (entity.id.as_str(), entity))
            .collect::<HashMap<_, _>>();
        let object_records = native
            .object_graphs
            .iter()
            .flat_map(|graph| &graph.records)
            .map(|record| (record.id.as_str(), record))
            .collect::<HashMap<_, _>>();
        let design_objects = native
            .design_objects
            .iter()
            .map(|object| (object.id.as_str(), object))
            .collect::<HashMap<_, _>>();

        for parameter in &mut ir.model.parameters {
            if parameter.owner.is_some() {
                continue;
            }
            let Some(native_ref) = parameter.native_ref.as_deref() else {
                continue;
            };
            let Some(entity) = entities.get(native_ref) else {
                continue;
            };
            let Some(object_record) = object_records.get(entity.object_record.as_str()) else {
                continue;
            };
            let Some(design_object) = object_record.design_object.as_deref() else {
                continue;
            };
            let Some(feature_id) =
                feature_owner_for_design_object(design_object, &design_objects, &self.feature_ids)
            else {
                continue;
            };
            parameter.owner = Some(feature_id);
        }
    }

    /// Bind a neutral feature to a transferred structural parent.
    ///
    /// The object graph records an exact owner-design-object incidence. It is
    /// a feature parent only when both endpoint design objects independently
    /// satisfy a neutral feature transfer. Do not use a field relation here:
    /// those relations are typed incidences, but their operation roles remain
    /// unresolved. A malformed owner cycle is omitted as a whole rather than
    /// creating a cyclic neutral history.
    pub(crate) fn assign_feature_parents(&self, ir: &mut CadIr, native: &CatiaNative) {
        let design_objects = native
            .design_objects
            .iter()
            .map(|object| (object.id.as_str(), object))
            .collect::<HashMap<_, _>>();
        let parents = ir
            .model
            .features
            .iter()
            .filter_map(|feature| {
                let native_ref = feature.native_ref.as_deref()?;
                let object = design_objects.get(native_ref)?;
                let parent_object = object.owner_design_object.as_deref()?;
                let parent = self.feature_ids.get(parent_object)?;
                (parent != &feature.id).then(|| (feature.id.clone(), parent.clone()))
            })
            .collect::<HashMap<_, _>>();

        for feature in &mut ir.model.features {
            if feature.parent.is_some() {
                continue;
            }
            let Some(parent) = parents.get(&feature.id) else {
                continue;
            };
            if feature_parent_chain_is_acyclic(&feature.id, &parents) {
                feature.parent = Some(parent.clone());
            }
        }
    }
}

fn feature_parent_chain_is_acyclic(
    feature_id: &FeatureId,
    parents: &HashMap<FeatureId, FeatureId>,
) -> bool {
    let mut current = Some(feature_id);
    let mut visited = HashSet::new();
    while let Some(id) = current {
        if !visited.insert(id) {
            return false;
        }
        current = parents.get(id);
    }
    true
}

/// Resolve one unique transferred feature on a complete structural owner chain.
///
/// An immediate field group is not always the semantic feature object. CATIA
/// may store a feature's parameter in a child design object. The parent links
/// are exact object-graph incidences, so following them is safe when the chain
/// is complete, has no non-reflexive cycle, and reaches exactly one transferred
/// feature.
fn feature_owner_for_design_object(
    design_object_id: &str,
    design_objects: &HashMap<&str, &CatiaDesignObject>,
    feature_ids: &HashMap<String, FeatureId>,
) -> Option<FeatureId> {
    let mut current = Some(design_object_id);
    let mut visited = HashSet::new();
    let mut feature = None;

    while let Some(current_id) = current {
        if !visited.insert(current_id) {
            return None;
        }
        let object = design_objects.get(current_id).copied()?;
        if let Some(candidate) = feature_ids.get(current_id) {
            if feature.replace(candidate.clone()).is_some() {
                return None;
            }
        }
        current = object
            .owner_design_object
            .as_deref()
            .filter(|parent| *parent != current_id);
    }

    feature
}

/// Transfer exact owner-bound reference history nodes.
pub(crate) fn transfer_design_features(
    ir: &mut CadIr,
    native: &CatiaNative,
    graph_scope: Option<&HashSet<String>>,
) -> DesignFeatureTransfer {
    let records = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .map(|record| (record.id.as_str(), record))
        .collect::<HashMap<_, _>>();
    let mut transfer = DesignFeatureTransfer::default();

    for object in native
        .design_objects
        .iter()
        .filter(|object| graph_scope.is_none_or(|scope| scope.contains(object.parent.as_str())))
    {
        let plane_candidate = principal_plane_candidate(object, &records);
        let sketch_owner = sketch_candidate(object, &records);
        let native_operation = native_operation_candidate(object, &records);
        match (plane_candidate, sketch_owner, native_operation) {
            (Some(candidate), None, None) => {
                transfer_principal_plane(ir, &mut transfer, candidate);
            }
            (None, Some(owner_record), None) => {
                transfer_sketch(ir, &mut transfer, object, owner_record);
            }
            (None, None, Some(candidate)) => {
                transfer_native_operation(ir, &mut transfer, &candidate);
            }
            (Some(_), Some(_), _) | (Some(_), None, Some(_)) | (None, Some(_), Some(_)) => {
                // One object cannot safely occupy two neutral feature identities.
                // Leave all declarations unresolved so the feature-id map cannot
                // overwrite one transfer with another.
            }
            (None, None, None) => {}
        }
    }

    transfer.assign_feature_parents(ir, native);
    transfer
}

fn transfer_principal_plane(
    ir: &mut CadIr,
    transfer: &mut DesignFeatureTransfer,
    candidate: PrincipalPlaneCandidate<'_>,
) {
    let object = candidate.object;
    let feature_id = FeatureId(format!("{}:feature", object.id));
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: object.first_field_byte_offset,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: Some(candidate.declaration_class.to_string()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::DatumPrincipalPlane {
            plane: candidate.plane,
        },
        native_ref: Some(object.id.clone()),
    });
    transfer.feature_ids.insert(object.id.clone(), feature_id);
    transfer.principal_plane_records.extend(
        candidate
            .declarations
            .into_iter()
            .map(|record| record.id.clone()),
    );
}

fn transfer_sketch(
    ir: &mut CadIr,
    transfer: &mut DesignFeatureTransfer,
    object: &CatiaDesignObject,
    owner_record: &CatiaObjectRecord,
) {
    let sketch_id = SketchId(format!("{}:sketch", object.id));
    let feature_id = FeatureId(format!("{}:feature", object.id));
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        placement: SketchPlacement::Unresolved,
        profiles: Vec::new(),
        native_ref: Some(object.id.clone()),
    });
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: object.first_field_byte_offset,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: Some("Sketch".to_string()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: SketchSpace::Unresolved,
            sketch: Some(sketch_id),
        },
        native_ref: Some(object.id.clone()),
    });
    transfer.feature_ids.insert(object.id.clone(), feature_id);
    transfer
        .sketch_owner_records
        .insert(owner_record.id.clone());
}

struct NativeOperationCandidate<'a> {
    object: &'a CatiaDesignObject,
    owner_record: &'a CatiaObjectRecord,
    kind: &'a str,
}

fn native_operation_candidate<'a>(
    object: &'a CatiaDesignObject,
    records: &HashMap<&str, &'a CatiaObjectRecord>,
) -> Option<NativeOperationCandidate<'a>> {
    let owner_class = object.owner_class.as_ref()?;
    is_admitted_native_operation_class(&owner_class.name).then_some(())?;
    let owner_record_id = object.owner_record.as_deref()?;
    let owner_record = records.get(owner_record_id).copied()?;
    (owner_record.class_name.as_deref() == Some(owner_class.name.as_str())
        && owner_record.class_entry.as_deref() == Some(owner_class.entry.as_str())
        && owner_record.entity_id == Some(object.owner_entity_id)
        && owner_record.design_object.as_deref() == object.owner_design_object.as_deref())
    .then_some(NativeOperationCandidate {
        object,
        owner_record,
        kind: owner_class.name.as_str(),
    })
}

fn is_admitted_native_operation_class(name: &str) -> bool {
    matches!(
        name,
        "EdgeFillet"
            | "Prism_ThickThin1"
            | "Prism_ThickThin2"
            | "Revol_ThickThin1"
            | "Sweep_ThickThin1"
    )
}

fn transfer_native_operation(
    ir: &mut CadIr,
    transfer: &mut DesignFeatureTransfer,
    candidate: &NativeOperationCandidate<'_>,
) {
    let object = candidate.object;
    let kind = candidate.kind.to_string();
    let feature_id = FeatureId(format!("{}:feature", object.id));
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: object.first_field_byte_offset,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: Some(kind.clone()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind,
            parameters: BTreeMap::new(),
            properties: BTreeMap::new(),
        },
        native_ref: Some(object.id.clone()),
    });
    transfer.feature_ids.insert(object.id.clone(), feature_id);
    transfer
        .native_operation_records
        .insert(candidate.owner_record.id.clone());
}

struct PrincipalPlaneCandidate<'a> {
    object: &'a CatiaDesignObject,
    declarations: Vec<&'a CatiaObjectRecord>,
    plane: PrincipalPlane,
    declaration_class: &'a str,
}

fn principal_plane_candidate<'a>(
    object: &'a CatiaDesignObject,
    records: &HashMap<&str, &'a CatiaObjectRecord>,
) -> Option<PrincipalPlaneCandidate<'a>> {
    object.owner_record.as_ref()?;
    let declarations = object
        .fields
        .iter()
        .map(|field| records.get(field.as_str()).copied())
        .collect::<Option<Vec<_>>>()?;
    let first = declarations.first()?;
    let class_name = first.class_name.as_deref()?;
    let class_entry = first.class_entry.as_deref()?;
    let plane = principal_plane(class_name)?;
    declarations
        .iter()
        .all(|record| {
            record.class_name.as_deref() == Some(class_name)
                && record.class_entry.as_deref() == Some(class_entry)
                && complete_empty_declaration(record, &object.id, object.owner_entity_id)
        })
        .then_some(PrincipalPlaneCandidate {
            object,
            declarations,
            plane,
            declaration_class: class_name,
        })
}

fn sketch_candidate<'a>(
    object: &CatiaDesignObject,
    records: &HashMap<&str, &'a CatiaObjectRecord>,
) -> Option<&'a CatiaObjectRecord> {
    let owner_class = object.owner_class.as_ref()?;
    (owner_class.name == "Sketch").then_some(())?;
    let owner_record_id = object.owner_record.as_deref()?;
    let owner_record = records.get(owner_record_id).copied()?;
    (owner_record.class_name.as_deref() == Some("Sketch")
        && owner_record.class_entry.as_deref() == Some(owner_class.entry.as_str())
        && owner_record.design_object.as_deref() == object.owner_design_object.as_deref())
    .then_some(owner_record)
}

fn principal_plane(class_name: &str) -> Option<PrincipalPlane> {
    match class_name {
        "xy-plane" => Some(PrincipalPlane::Top),
        "yz-plane" => Some(PrincipalPlane::Right),
        "zx-plane" => Some(PrincipalPlane::Front),
        _ => None,
    }
}

fn bound_declaration(
    record: &CatiaObjectRecord,
    design_object: &str,
    owner_entity_id: u32,
) -> bool {
    record.design_object.as_deref() == Some(design_object)
        && record.owner_entity_id() == Some(owner_entity_id)
}

fn complete_empty_declaration(
    record: &CatiaObjectRecord,
    design_object: &str,
    owner_entity_id: u32,
) -> bool {
    bound_declaration(record, design_object, owner_entity_id)
        && record.storage_ref.is_none()
        && record.references.is_empty()
        && record.subtype == PayloadSubtype::Empty
        && record.payload.size == 1
        && record.payload.fields == [PayloadField::Terminator]
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::units::Units;

    use crate::native::{CatiaDesignClass, CatiaObjectGraph, CatiaObjectOwner};
    use crate::object_graph::ObjectPayload;

    fn design_object(id: &str, owner_design_object: Option<&str>) -> CatiaDesignObject {
        CatiaDesignObject {
            id: id.to_string(),
            parent: "graph".to_string(),
            ordinal: 0,
            first_field_byte_offset: 0,
            owner_entity_id: 0,
            owner_record: None,
            owner_design_object: owner_design_object.map(str::to_string),
            owner_class: None,
            owner_storage_ref: None,
            fields: Vec::new(),
            field_classes: Vec::new(),
            definition_values: Vec::new(),
            definition_chain_values: Vec::new(),
            relations: Vec::new(),
            parallel_reference_table: None,
        }
    }

    fn feature(id: &str, native_ref: &str) -> Feature {
        Feature {
            id: FeatureId::from(id),
            ordinal: 0,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::StoredGeometry,
            native_ref: Some(native_ref.to_string()),
        }
    }

    fn native_operation_object(
        id: &str,
        owner_design_object: Option<&str>,
        owner_entity_id: u32,
        owner_record: &str,
        class_name: &str,
        class_entry: &str,
    ) -> CatiaDesignObject {
        let mut object = design_object(id, owner_design_object);
        object.owner_entity_id = owner_entity_id;
        object.owner_record = Some(owner_record.to_string());
        object.owner_class = Some(CatiaDesignClass {
            entry: class_entry.to_string(),
            name: class_name.to_string(),
        });
        object
    }

    fn object_record(
        id: &str,
        design_object: Option<&str>,
        entity_id: Option<u32>,
        owner: Option<u32>,
        class_name: Option<&str>,
        class_entry: Option<&str>,
    ) -> CatiaObjectRecord {
        CatiaObjectRecord {
            id: id.to_string(),
            parent: "graph".to_string(),
            design_object: design_object.map(str::to_string),
            entity_record: None,
            entity_id,
            ordinal: 0,
            byte_offset: 0,
            byte_len: 0,
            lead: 0,
            head: Vec::new(),
            inline_body: None,
            owner: owner.map(CatiaObjectOwner::Entity),
            class_ref: None,
            class_name: class_name.map(str::to_string),
            class_entry: class_entry.map(str::to_string),
            storage_ref: None,
            storage_record: None,
            storage_design_object: None,
            payload: ObjectPayload {
                size: 1,
                fields: vec![PayloadField::Terminator],
            },
            repeated_reference_suffix: None,
            repeated_reference_schema_selection: None,
            subtype: PayloadSubtype::Empty,
            references: Vec::new(),
        }
    }

    #[test]
    fn assigns_parent_from_an_exact_transferred_owner_chain() {
        let native = CatiaNative {
            design_objects: vec![
                design_object("parent-object", None),
                design_object("child-object", Some("parent-object")),
            ],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty(Units::default());
        ir.model
            .features
            .push(feature("parent-feature", "parent-object"));
        ir.model
            .features
            .push(feature("child-feature", "child-object"));
        let transfer = DesignFeatureTransfer {
            feature_ids: HashMap::from([
                (
                    "parent-object".to_string(),
                    FeatureId::from("parent-feature"),
                ),
                ("child-object".to_string(), FeatureId::from("child-feature")),
            ]),
            ..DesignFeatureTransfer::default()
        };

        transfer.assign_feature_parents(&mut ir, &native);

        assert_eq!(ir.model.features[0].parent, None);
        assert_eq!(
            ir.model.features[1].parent,
            Some(FeatureId::from("parent-feature"))
        );
    }

    #[test]
    fn does_not_assign_a_self_parent() {
        let native = CatiaNative {
            design_objects: vec![design_object("feature-object", Some("feature-object"))],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty(Units::default());
        ir.model.features.push(feature("feature", "feature-object"));
        let transfer = DesignFeatureTransfer {
            feature_ids: HashMap::from([(
                "feature-object".to_string(),
                FeatureId::from("feature"),
            )]),
            ..DesignFeatureTransfer::default()
        };

        transfer.assign_feature_parents(&mut ir, &native);

        assert_eq!(ir.model.features[0].parent, None);
    }

    #[test]
    fn omits_all_parents_in_an_owner_cycle() {
        let native = CatiaNative {
            design_objects: vec![
                design_object("first-object", Some("second-object")),
                design_object("second-object", Some("first-object")),
            ],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty(Units::default());
        ir.model
            .features
            .push(feature("first-feature", "first-object"));
        ir.model
            .features
            .push(feature("second-feature", "second-object"));
        let transfer = DesignFeatureTransfer {
            feature_ids: HashMap::from([
                ("first-object".to_string(), FeatureId::from("first-feature")),
                (
                    "second-object".to_string(),
                    FeatureId::from("second-feature"),
                ),
            ]),
            ..DesignFeatureTransfer::default()
        };

        transfer.assign_feature_parents(&mut ir, &native);

        assert!(ir
            .model
            .features
            .iter()
            .all(|feature| feature.parent.is_none()));
    }

    #[test]
    fn transfers_admitted_native_operations_with_exact_parentage() {
        let mut parent = native_operation_object(
            "parent-object",
            None,
            1,
            "parent-record",
            "Prism_ThickThin1",
            "parent-entry",
        );
        parent.first_field_byte_offset = 10;
        let mut child = native_operation_object(
            "child-object",
            Some("parent-object"),
            2,
            "child-record",
            "EdgeFillet",
            "child-entry",
        );
        child.first_field_byte_offset = 20;
        let native = CatiaNative {
            design_objects: vec![parent, child],
            object_graphs: vec![CatiaObjectGraph {
                id: "graph".to_string(),
                byte_offset: 0,
                byte_len: 0,
                finjpl_segment: None,
                outer_container: None,
                catalog_byte_offset: None,
                catalog: None,
                records: vec![
                    object_record(
                        "parent-record",
                        None,
                        Some(1),
                        None,
                        Some("Prism_ThickThin1"),
                        Some("parent-entry"),
                    ),
                    object_record(
                        "child-record",
                        Some("parent-object"),
                        Some(2),
                        Some(1),
                        Some("EdgeFillet"),
                        Some("child-entry"),
                    ),
                ],
            }],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty(Units::default());

        let transfer = transfer_design_features(&mut ir, &native, None);

        assert_eq!(ir.model.features.len(), 2);
        assert_eq!(ir.model.features[0].ordinal, 10);
        assert_eq!(ir.model.features[1].ordinal, 20);
        assert_eq!(
            ir.model.features[1].parent,
            Some(FeatureId::from("parent-object:feature"))
        );
        for (feature, kind) in ir
            .model
            .features
            .iter()
            .zip(["Prism_ThickThin1", "EdgeFillet"])
        {
            let FeatureDefinition::Native {
                kind: actual_kind,
                parameters,
                properties,
            } = &feature.definition
            else {
                panic!("expected an opaque native operation");
            };
            assert_eq!(actual_kind, kind);
            assert!(parameters.is_empty());
            assert!(properties.is_empty());
        }
        assert_eq!(
            transfer.native_operation_records,
            HashSet::from(["parent-record".to_string(), "child-record".to_string()])
        );
        assert_eq!(
            transfer.consumed_records(),
            transfer.native_operation_records
        );
    }

    #[test]
    fn does_not_promote_an_unadmitted_helper_owner_class() {
        let object = native_operation_object(
            "helper-object",
            None,
            1,
            "helper-record",
            "Prism_EndLimit_Length",
            "helper-entry",
        );
        let native = CatiaNative {
            design_objects: vec![object],
            object_graphs: vec![CatiaObjectGraph {
                id: "graph".to_string(),
                byte_offset: 0,
                byte_len: 0,
                finjpl_segment: None,
                outer_container: None,
                catalog_byte_offset: None,
                catalog: None,
                records: vec![object_record(
                    "helper-record",
                    None,
                    Some(1),
                    None,
                    Some("Prism_EndLimit_Length"),
                    Some("helper-entry"),
                )],
            }],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty(Units::default());

        let transfer = transfer_design_features(&mut ir, &native, None);

        assert!(ir.model.features.is_empty());
        assert!(transfer.native_operation_records.is_empty());
    }
}
