// SPDX-License-Identifier: Apache-2.0
//! Transfer of exact CATIA reference and sketch history nodes.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, PrincipalPlane, SketchSpace};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement};

use crate::native::{CatiaDesignObject, CatiaNative, CatiaObjectRecord};
use crate::object_graph::{PayloadField, PayloadSubtype};

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DesignFeatureTransfer {
    pub(crate) declaration_records: HashSet<String>,
    pub(crate) placement_records: HashSet<String>,
    pub(crate) principal_plane_records: HashSet<String>,
    pub(crate) features_by_design_object: HashMap<String, FeatureId>,
}

impl DesignFeatureTransfer {
    pub(crate) fn consumed_records(&self) -> HashSet<String> {
        self.declaration_records
            .union(&self.placement_records)
            .chain(&self.principal_plane_records)
            .cloned()
            .collect()
    }
}

/// Transfer exact owner-bound reference and sketch history nodes.
pub(crate) fn transfer_design_features(
    ir: &mut CadIr,
    native: &CatiaNative,
) -> DesignFeatureTransfer {
    let records = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .map(|record| (record.id.as_str(), record))
        .collect::<HashMap<_, _>>();
    let candidates = native
        .design_objects
        .iter()
        .filter_map(|object| design_feature_candidate(object, &records))
        .collect::<Vec<_>>();
    let mut transfer = DesignFeatureTransfer::default();

    for candidate in candidates {
        let object = candidate.object;
        let feature_id = FeatureId(format!("{}:feature", object.id));
        let parent = object
            .owner_design_object
            .as_deref()
            .and_then(|owner| transfer.features_by_design_object.get(owner))
            .cloned();
        match candidate.definition {
            DesignFeatureDefinition::PrincipalPlane {
                declarations,
                plane,
                declaration_class,
            } => {
                ir.model.features.push(Feature {
                    id: feature_id.clone(),
                    ordinal: ir.model.features.len() as u64,
                    name: None,
                    suppressed: None,
                    parent,
                    dependencies: Vec::new(),
                    source_properties: BTreeMap::new(),
                    source_tag: Some(declaration_class.to_string()),
                    source_text: None,
                    source_content: Vec::new(),
                    outputs: Vec::new(),
                    definition: FeatureDefinition::DatumPrincipalPlane { plane },
                    native_ref: Some(object.id.clone()),
                });
                transfer
                    .principal_plane_records
                    .extend(declarations.into_iter().map(|record| record.id.clone()));
            }
            DesignFeatureDefinition::Sketch {
                declarations,
                declaration_class,
            } => {
                let plane_declarations = object
                    .fields
                    .iter()
                    .filter_map(|field| records.get(field.as_str()).copied())
                    .filter(|record| {
                        matches!(
                            record.class_name.as_deref(),
                            Some("xy-plane" | "yz-plane" | "zx-plane")
                        )
                    })
                    .collect::<Vec<_>>();
                let resolved_placement = complete_principal_plane(
                    &plane_declarations,
                    &object.id,
                    object.owner_entity_id,
                );
                let placement = resolved_placement.unwrap_or(SketchPlacement::Unresolved);

                let sketch_id = SketchId(format!("{}:sketch", object.id));
                ir.model.sketches.push(Sketch {
                    id: sketch_id.clone(),
                    name: None,
                    configuration: None,
                    placement,
                    profiles: Vec::new(),
                    native_ref: Some(object.id.clone()),
                });
                ir.model.features.push(Feature {
                    id: feature_id.clone(),
                    ordinal: ir.model.features.len() as u64,
                    name: None,
                    suppressed: None,
                    parent,
                    dependencies: Vec::new(),
                    source_properties: BTreeMap::new(),
                    source_tag: Some(declaration_class.to_string()),
                    source_text: None,
                    source_content: Vec::new(),
                    outputs: Vec::new(),
                    definition: FeatureDefinition::Sketch {
                        space: SketchSpace::Planar,
                        sketch: Some(sketch_id),
                    },
                    native_ref: Some(object.id.clone()),
                });
                transfer.declaration_records.extend(
                    declarations
                        .into_iter()
                        .filter(|declaration| {
                            complete_empty_declaration(
                                declaration,
                                &object.id,
                                object.owner_entity_id,
                            )
                        })
                        .map(|declaration| declaration.id.clone()),
                );
                if resolved_placement.is_some() {
                    transfer.placement_records.extend(
                        plane_declarations
                            .into_iter()
                            .map(|declaration| declaration.id.clone()),
                    );
                }
            }
        }
        transfer
            .features_by_design_object
            .insert(object.id.clone(), feature_id);
    }

    transfer
}

struct DesignFeatureCandidate<'a> {
    object: &'a CatiaDesignObject,
    definition: DesignFeatureDefinition<'a>,
}

enum DesignFeatureDefinition<'a> {
    PrincipalPlane {
        declarations: Vec<&'a CatiaObjectRecord>,
        plane: PrincipalPlane,
        declaration_class: &'a str,
    },
    Sketch {
        declarations: Vec<&'a CatiaObjectRecord>,
        declaration_class: &'a str,
    },
}

fn design_feature_candidate<'a>(
    object: &'a CatiaDesignObject,
    records: &HashMap<&str, &'a CatiaObjectRecord>,
) -> Option<DesignFeatureCandidate<'a>> {
    principal_plane_candidate(object, records).or_else(|| sketch_candidate(object, records))
}

fn principal_plane_candidate<'a>(
    object: &'a CatiaDesignObject,
    records: &HashMap<&str, &'a CatiaObjectRecord>,
) -> Option<DesignFeatureCandidate<'a>> {
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
        .then_some(DesignFeatureCandidate {
            object,
            definition: DesignFeatureDefinition::PrincipalPlane {
                declarations,
                plane,
                declaration_class: class_name,
            },
        })
}

fn sketch_candidate<'a>(
    object: &'a CatiaDesignObject,
    records: &HashMap<&str, &'a CatiaObjectRecord>,
) -> Option<DesignFeatureCandidate<'a>> {
    object.owner_record.as_ref()?;
    let declarations = object
        .fields
        .iter()
        .filter_map(|field| records.get(field.as_str()).copied())
        .filter(|record| matches!(record.class_name.as_deref(), Some("PRTSketch" | "Sketch")))
        .collect::<Vec<_>>();
    let (declaration_class, declaration_entry) = declarations.first().and_then(|record| {
        record
            .class_name
            .as_deref()
            .zip(record.class_entry.as_deref())
    })?;
    declarations
        .iter()
        .all(|record| {
            record.class_name.as_deref() == Some(declaration_class)
                && record.class_entry.as_deref() == Some(declaration_entry)
                && bound_declaration(record, &object.id, object.owner_entity_id)
        })
        .then_some(DesignFeatureCandidate {
            object,
            definition: DesignFeatureDefinition::Sketch {
                declarations,
                declaration_class,
            },
        })
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
        && record.owner_entity_id == Some(owner_entity_id)
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

fn complete_principal_plane(
    declarations: &[&CatiaObjectRecord],
    design_object: &str,
    owner_entity_id: u32,
) -> Option<SketchPlacement> {
    let first = declarations.first()?;
    let class_name = first.class_name.as_deref()?;
    let class_entry = first.class_entry.as_deref()?;
    if declarations.iter().any(|record| {
        record.class_name.as_deref() != Some(class_name)
            || record.class_entry.as_deref() != Some(class_entry)
            || !complete_empty_declaration(record, design_object, owner_entity_id)
    }) {
        return None;
    }
    let (normal, u_axis) = match class_name {
        "xy-plane" => (
            Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            Vector3 {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
        ),
        "yz-plane" => (
            Vector3 {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
        ),
        "zx-plane" => (
            Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        ),
        _ => return None,
    };
    Some(SketchPlacement::Resolved {
        origin: Point3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        normal,
        u_axis,
    })
}
