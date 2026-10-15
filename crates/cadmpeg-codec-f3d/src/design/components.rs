// SPDX-License-Identifier: Apache-2.0
//! Project exact local component operations into neutral product structure.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::report::loss::LossNote;

use crate::design::decode::components::{ComponentDefinition, StreamComponents};
use crate::loss::F3dLossCode;
use crate::records::bodies::DesignBodyBinding;
use crate::records::feature::{
    assembly::DesignAssemblyOperandQualifier, assembly_features::DesignComponentOccurrence,
    scope::DesignParameterScope,
};
use crate::records::mesh::DesignRelaxedGuidText;

const IDENTITY: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// Decoded records that define and place local component occurrences.
pub(crate) struct LocalComponentSources<'a> {
    /// Design parameter scopes, including component operations.
    pub(crate) scopes: &'a [DesignParameterScope],
    /// Local component-occurrence carriers.
    pub(crate) occurrences: &'a [DesignComponentOccurrence],
    /// Component definitions and root component of each Design stream.
    pub(crate) components: &'a BTreeMap<String, StreamComponents>,
    /// Design body-map pairs.
    pub(crate) body_bindings: &'a [DesignBodyBinding],
    /// Solved model bodies, or `None` when bodies are projected after product structure.
    pub(crate) bodies: Option<&'a BTreeSet<BodyId>>,
    /// First free ordinal among root occurrences already in the model.
    pub(crate) root_ordinal_start: usize,
}

/// Local product structure projected from component occurrences.
#[derive(Debug)]
pub(crate) struct LocalComponentProjection {
    pub(crate) definitions: Vec<ProductDefinition>,
    pub(crate) occurrences: Vec<Occurrence>,
    pub(crate) losses: Vec<LossNote>,
}

/// Project local component definitions and their placed occurrences.
///
/// Component operations supply the occurrences and placements they prove.
/// Every other carrier contained by the root component is placed at its own
/// transform. A carrier whose container or definition the source does not
/// settle stays unplaced and is reported.
pub(crate) fn project_local_components(
    sources: &LocalComponentSources<'_>,
) -> Result<LocalComponentProjection, cadmpeg_core::CodecError> {
    let mut projector = Projector::new(sources);
    for scope in sources.scopes {
        if let Some(qualifiers) = scope.assembly_alignment().and_then(
            super::super::records::feature::assembly::DesignAssemblyAlignment::operand_qualifiers,
        ) {
            for qualifier in &qualifiers {
                let DesignAssemblyOperandQualifier::OccurrencePath { path } = qualifier else {
                    continue;
                };
                let Some(guid) = path.occurrence_guids().first() else {
                    continue;
                };
                let key = guid.value.as_str().to_ascii_lowercase();
                let root = projector.native_by_guid.get(&key).copied().flatten();
                let Some(root) = root else {
                    continue;
                };
                projector.place(
                    &root.component_guid,
                    &root.occurrence_guid,
                    root.transform()
                        .map_or(IDENTITY, |frame| frame.value.into()),
                )?;
            }
        }
        if let Some(operation) = scope.copy_paste_component_operation() {
            projector.place(
                &operation.component_guid,
                &operation.source_occurrence_guid,
                operation.source_transform,
            )?;
            projector.place(
                &operation.component_guid,
                &operation.copied_occurrence_guid,
                operation.copied_transform,
            )?;
        }
        if let Some(construction) = scope.derived_instance_construction() {
            projector.place(
                &construction.component_guid,
                &construction.occurrence_guid,
                construction.transform,
            )?;
        }
        let Some(
            crate::records::feature::patterns::DesignRectangularPatternInstances::Components {
                component_guid,
                seed,
                generated,
            },
        ) = scope
            .rectangular_pattern_construction()
            .and_then(|construction| construction.instances())
        else {
            continue;
        };
        for occurrence in std::iter::once(seed).chain(generated) {
            projector.place(
                component_guid,
                &occurrence.occurrence_guid,
                occurrence.instance.transform.value,
            )?;
        }
    }
    projector.place_root_contained_carriers()?;
    projector.attach_root_occurrences();
    projector.finish()
}

/// Reason a local occurrence carrier stays unplaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Unplaced {
    RepeatedGuid,
    NoRootComponent,
    NestedContainer,
    DefinitionUnbound,
    BodyOutsideComponents,
    BodiesPending,
}

impl Unplaced {
    fn reason(self) -> &'static str {
        match self {
            Self::RepeatedGuid => "the occurrence GUID repeats in its Design stream",
            Self::NoRootComponent => {
                "the Design stream has no unique root instance bound to a component naming space"
            }
            Self::NestedContainer => {
                "the containing component is not the root component; nested placement is not projected"
            }
            Self::DefinitionUnbound => {
                "the component definition record stores no naming-space binding"
            }
            Self::BodyOutsideComponents => {
                "a model body has no component definition, so a product structure would omit it"
            }
            Self::BodiesPending => {
                "the document projects its bodies after product structure, so component body ownership is not known"
            }
        }
    }
}

struct Projector<'a> {
    sources: &'a LocalComponentSources<'a>,
    native_by_guid: BTreeMap<String, Option<&'a DesignComponentOccurrence>>,
    definitions: BTreeMap<String, ProductDefinition>,
    occurrences: BTreeMap<String, (Occurrence, Option<&'a DesignComponentOccurrence>)>,
    unplaced: BTreeMap<Unplaced, usize>,
}

impl<'a> Projector<'a> {
    fn new(sources: &'a LocalComponentSources<'a>) -> Self {
        let mut native_by_guid = BTreeMap::new();
        for occurrence in sources.occurrences {
            match native_by_guid.entry(occurrence.occurrence_guid.as_str().to_ascii_lowercase()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Some(occurrence));
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    *entry.get_mut() = None;
                }
            }
        }
        Self {
            sources,
            native_by_guid,
            definitions: BTreeMap::new(),
            occurrences: BTreeMap::new(),
            unplaced: BTreeMap::new(),
        }
    }

    fn stream_components(
        &self,
        carrier: &DesignComponentOccurrence,
    ) -> Option<(&'a str, &'a StreamComponents)> {
        let (stream, components) = self
            .sources
            .components
            .get_key_value(crate::ids::native_stream(&carrier.id)?)?;
        Some((stream.as_str(), components))
    }

    fn carrier_definition(
        &self,
        carrier: &DesignComponentOccurrence,
    ) -> Option<(&'a str, &'a ComponentDefinition)> {
        let (stream, components) = self.stream_components(carrier)?;
        Some((
            stream,
            components.roots.get(&carrier.component_record_index)?,
        ))
    }

    /// Place one occurrence once. The first placement of an occurrence wins.
    fn place(
        &mut self,
        component_guid: &DesignRelaxedGuidText,
        occurrence_guid: &DesignRelaxedGuidText,
        transform: impl Into<[[f64; 4]; 4]>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let occurrence_id = crate::ids::neutral_component_occurrence_id(occurrence_guid);
        if self.occurrences.contains_key(occurrence_id.as_str()) {
            return Ok(());
        }
        let transform = neutral_transform(transform)?;
        let carrier = self
            .native_by_guid
            .get(&occurrence_guid.as_str().to_ascii_lowercase())
            .copied()
            .flatten();
        let definition = match carrier.and_then(|carrier| self.carrier_definition(carrier)) {
            Some((stream, root)) => self.naming_space_definition(stream, root),
            None => self.guid_definition(component_guid),
        };
        let key = occurrence_id.as_str().to_owned();
        self.occurrences.insert(
            key,
            (
                Occurrence {
                    id: occurrence_id,
                    prototype: PrototypeReference::Local { definition },
                    parent: OccurrenceParent::Root {},
                    ordinal: 0,
                    transform,
                    linked_prototype: None,
                    scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
                    name: None,
                    visible: None,
                    link: None,
                    native_ref: carrier.map(|carrier| carrier.id.clone()),
                },
                carrier,
            ),
        );
        Ok(())
    }

    /// Place every carrier that the root component contains and that no
    /// component operation placed, at the carrier's own transform.
    fn place_root_contained_carriers(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        let bodies_owned = match self.sources.bodies {
            None => Err(Unplaced::BodiesPending),
            Some(_) if !self.every_body_has_a_definition() => Err(Unplaced::BodyOutsideComponents),
            Some(_) => Ok(()),
        };
        for carrier in self.sources.occurrences {
            let occurrence_id =
                crate::ids::neutral_component_occurrence_id(&carrier.occurrence_guid);
            if self.occurrences.contains_key(occurrence_id.as_str()) {
                continue;
            }
            let unplaced = if self
                .native_by_guid
                .get(&carrier.occurrence_guid.as_str().to_ascii_lowercase())
                .copied()
                .flatten()
                .is_none()
            {
                Some(Unplaced::RepeatedGuid)
            } else {
                match self.stream_components(carrier).and_then(|(_, components)| {
                    components
                        .root_component
                        .and_then(|root| components.roots.get(&root))
                }) {
                    None => Some(Unplaced::NoRootComponent),
                    Some(root) if root.root_entity != carrier.owner_entity => {
                        Some(Unplaced::NestedContainer)
                    }
                    Some(_) if self.carrier_definition(carrier).is_none() => {
                        Some(Unplaced::DefinitionUnbound)
                    }
                    Some(_) => bodies_owned.err(),
                }
            };
            if let Some(reason) = unplaced {
                *self.unplaced.entry(reason).or_default() += 1;
                continue;
            }
            self.place(
                &carrier.component_guid,
                &carrier.occurrence_guid,
                carrier
                    .transform()
                    .map_or(IDENTITY, |frame| frame.value.into()),
            )?;
        }
        Ok(())
    }

    /// Place the occurrences contained by each stream's root component inside
    /// one identity root occurrence of that component.
    fn attach_root_occurrences(&mut self) {
        for (stream, components) in self.sources.components {
            let Some(root) = components
                .root_component
                .and_then(|root| components.roots.get(&root))
            else {
                continue;
            };
            let root_occurrence = crate::ids::neutral_root_occurrence_id(&root.context_uuid);
            let mut contained = false;
            for (occurrence, carrier) in self.occurrences.values_mut() {
                if carrier.is_some_and(|carrier| {
                    crate::ids::native_stream(&carrier.id) == Some(stream.as_str())
                        && carrier.owner_entity == root.root_entity
                }) {
                    occurrence.parent = OccurrenceParent::Occurrence {
                        occurrence: root_occurrence.clone(),
                    };
                    contained = true;
                }
            }
            if !contained {
                continue;
            }
            let definition = self.naming_space_definition(stream, root);
            let key = root_occurrence.as_str().to_owned();
            self.occurrences.insert(
                key,
                (
                    Occurrence {
                        id: root_occurrence,
                        prototype: PrototypeReference::Local { definition },
                        parent: OccurrenceParent::Root {},
                        ordinal: 0,
                        transform: cadmpeg_ir::transform::Transform::identity(),
                        linked_prototype: None,
                        scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
                        name: None,
                        visible: None,
                        link: None,
                        native_ref: None,
                    },
                    None,
                ),
            );
        }
    }

    fn finish(self) -> Result<LocalComponentProjection, cadmpeg_core::CodecError> {
        let overflow =
            || cadmpeg_core::CodecError::malformed("Fusion Design occurrence ordinal exceeds u32");
        let mut occurrences = self.occurrences.into_values().collect::<Vec<_>>();
        occurrences.sort_by(|(left, left_carrier), (right, right_carrier)| {
            (
                left_carrier.map(DesignComponentOccurrence::byte_offset),
                left.id.as_str(),
            )
                .cmp(&(
                    right_carrier.map(DesignComponentOccurrence::byte_offset),
                    right.id.as_str(),
                ))
        });
        let mut next_ordinal = BTreeMap::<Option<OccurrenceId>, u32>::new();
        let root_start = u32::try_from(self.sources.root_ordinal_start).map_err(|_| overflow())?;
        for (occurrence, _) in &mut occurrences {
            let parent = match &occurrence.parent {
                OccurrenceParent::Root {} => None,
                OccurrenceParent::Occurrence { occurrence } => Some(occurrence.clone()),
            };
            let start = if parent.is_none() { root_start } else { 0 };
            let ordinal = next_ordinal.entry(parent).or_insert(start);
            occurrence.ordinal = *ordinal;
            *ordinal = ordinal.checked_add(1).ok_or_else(overflow)?;
        }
        let mut occurrences = occurrences
            .into_iter()
            .map(|(occurrence, _)| occurrence)
            .collect::<Vec<_>>();
        occurrences.sort_by(|left, right| left.id.cmp(&right.id));
        let losses = self
            .unplaced
            .into_iter()
            .map(|(reason, count)| {
                F3dLossCode::LocalOccurrenceUnplaced.note(format!(
                    "{count} local component occurrence(s) are not placed: {}",
                    reason.reason()
                ))
            })
            .collect();
        Ok(LocalComponentProjection {
            definitions: self.definitions.into_values().collect(),
            occurrences,
            losses,
        })
    }

    /// Definition identified by a component naming space; it owns the bodies
    /// of that component's entity cluster.
    fn naming_space_definition(
        &mut self,
        stream: &str,
        root: &ComponentDefinition,
    ) -> ProductDefinitionId {
        let id = crate::ids::neutral_component_id(&root.context_uuid);
        if !self.definitions.contains_key(id.as_str()) {
            let bodies = self.cluster_bodies(stream, root.root_entity);
            self.definitions.insert(
                id.as_str().to_owned(),
                component_definition(id.clone(), bodies),
            );
        }
        id
    }

    /// Definition identified by a carrier GUID when no naming space binds the
    /// component record.
    fn guid_definition(&mut self, component_guid: &DesignRelaxedGuidText) -> ProductDefinitionId {
        let id = crate::ids::neutral_component_id(component_guid);
        if !self.definitions.contains_key(id.as_str()) {
            self.definitions.insert(
                id.as_str().to_owned(),
                component_definition(id.clone(), Vec::new()),
            );
        }
        id
    }

    /// Solved bodies whose body-map entity suffix lies in the entity cluster
    /// that starts at `root_entity`.
    fn cluster_bodies(&self, stream: &str, root_entity: u64) -> Vec<BodyId> {
        let end = self
            .sources
            .components
            .get(stream)
            .and_then(|components| {
                components
                    .root_entities
                    .range((
                        std::ops::Bound::Excluded(root_entity),
                        std::ops::Bound::Unbounded,
                    ))
                    .next()
            })
            .copied();
        let mut bodies = self
            .sources
            .body_bindings
            .iter()
            .filter(|binding| {
                crate::ids::native_stream(binding.id()) == Some(stream)
                    && binding.entity_suffix >= root_entity
                    && end.is_none_or(|end| binding.entity_suffix < end)
            })
            .filter_map(|binding| binding.body.clone())
            .filter(|body| {
                self.sources
                    .bodies
                    .is_none_or(|bodies| bodies.contains(body))
            })
            .collect::<Vec<_>>();
        bodies.sort();
        bodies.dedup();
        bodies
    }

    /// Whether every solved body lies in the cluster of a component definition.
    fn every_body_has_a_definition(&self) -> bool {
        let mut owned = BTreeSet::new();
        for (stream, components) in self.sources.components {
            for root in components.roots.values() {
                owned.extend(self.cluster_bodies(stream, root.root_entity));
            }
        }
        self.sources
            .bodies
            .is_some_and(|bodies| bodies.iter().all(|body| owned.contains(body)))
    }
}

fn component_definition(id: ProductDefinitionId, bodies: Vec<BodyId>) -> ProductDefinition {
    ProductDefinition {
        id,
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: None,
        description: None,
        part_number: None,
        bom_properties: BTreeMap::new(),
        bodies,
        native_ref: None,
    }
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
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ) {
            continue;
        }
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::InsertComponent {
                    occurrence: crate::ids::neutral_component_occurrence_id(
                        &construction.occurrence_guid,
                    ),
                },
            ));
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
) -> Result<Vec<Occurrence>, cadmpeg_core::CodecError> {
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
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ) {
            continue;
        }

        let occurrence_id = crate::ids::neutral_component_insert_occurrence_id(scope);
        let feature_occurrence_id = occurrence_id.clone();
        let name = construction.neutron_role.clone();
        let native_ref = scope.id.clone();
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::InsertComponent {
                    occurrence: feature_occurrence_id,
                },
            ));
        occurrences.push(Occurrence {
            id: occurrence_id,
            prototype: PrototypeReference::Unresolved {},
            parent: OccurrenceParent::Root {},
            ordinal: u32::try_from(ordinal_start.checked_add(occurrences.len()).ok_or_else(
                || {
                    cadmpeg_core::CodecError::malformed(
                        "Fusion Design occurrence ordinal exceeds u32",
                    )
                },
            )?)
            .map_err(|_| {
                cadmpeg_core::CodecError::malformed("Fusion Design occurrence ordinal exceeds u32")
            })?,
            transform: neutral_transform(*construction.transform())?,
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: Some(name),
            visible: None,
            link: None,
            native_ref: Some(native_ref),
        });
    }
    Ok(occurrences)
}

/// A millimetre placement projected from source centimetres.
pub(crate) fn neutral_transform(
    transform: impl Into<[[f64; 4]; 4]>,
) -> Result<cadmpeg_ir::transform::Transform, cadmpeg_core::CodecError> {
    let source = transform.into();
    if source[3] != [0.0, 0.0, 0.0, 1.0] {
        return Err(cadmpeg_core::CodecError::NotImplemented(
            "F3D placement must project to a finite affine transform".into(),
        ));
    }
    let mut rows = [source[0], source[1], source[2]];
    for row in &mut rows {
        row[3] *= 10.0;
    }
    cadmpeg_ir::transform::Transform::affine(rows).ok_or_else(|| {
        cadmpeg_core::CodecError::NotImplemented(
            "F3D placement must project to a finite affine transform".into(),
        )
    })
}

#[cfg(test)]
mod tests {
    use crate::design::test_support::identity_matrix;
    use crate::records::feature::{
        assembly_features::{
            DesignComponentOccurrence, DesignCopyPasteComponentOperation,
            DesignDerivedInstanceConstruction,
        },
        scope::DesignParameterScope,
    };

    use std::collections::{BTreeMap, BTreeSet};

    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::products::{
        Occurrence, OccurrenceParent, ProductDefinition, PrototypeReference,
    };

    use crate::design::decode::components::{ComponentDefinition, StreamComponents};
    use crate::records::bodies::DesignBodyBinding;
    use crate::records::feature::assembly_features::{
        DesignComponentOccurrenceDraft, DesignComponentOccurrencePlacement,
    };

    const STREAM: &str = "f3d:Design/BulkStream.dat";
    const COMPONENT_CONTEXT: &str = "3df3a877-70e6-45e7-bf55-7d4d5ce42a20";
    const ROOT_CONTEXT: &str = "103b35af-2d83-4986-88b5-95c7fdce0b17";
    const COMPONENT_ROOT: u64 = 160;
    const DOCUMENT_ROOT: u64 = 277;
    const COMPONENT_BODY: &str = "f3d:brep/component.smbh/brep:entity#1";
    const ROOT_BODY: &str = "f3d:brep/root.smbh/brep:entity#1";

    fn project_without_naming_spaces(
        scopes: &[DesignParameterScope],
        occurrences: &[DesignComponentOccurrence],
    ) -> (Vec<ProductDefinition>, Vec<Occurrence>) {
        let components = BTreeMap::new();
        let bodies = BTreeSet::new();
        let projection = super::project_local_components(&super::LocalComponentSources {
            scopes,
            occurrences,
            components: &components,
            body_bindings: &[],
            bodies: Some(&bodies),
            root_ordinal_start: 0,
        })
        .unwrap();
        (projection.definitions, projection.occurrences)
    }

    fn guid(text: &str) -> crate::records::mesh::DesignRelaxedGuidText {
        text.to_owned().try_into().expect("GUID")
    }

    /// One component definition record 159 rooted at entity 160, and the root
    /// component record 276 rooted at entity 277 and named by the root instance.
    fn two_component_stream(root_component: Option<u64>) -> BTreeMap<String, StreamComponents> {
        BTreeMap::from([(
            STREAM.to_owned(),
            StreamComponents {
                root_entities: BTreeSet::from([COMPONENT_ROOT, DOCUMENT_ROOT]),
                roots: BTreeMap::from([
                    (
                        159,
                        ComponentDefinition {
                            root_entity: COMPONENT_ROOT,
                            context_uuid: guid(COMPONENT_CONTEXT),
                        },
                    ),
                    (
                        276,
                        ComponentDefinition {
                            root_entity: DOCUMENT_ROOT,
                            context_uuid: guid(ROOT_CONTEXT),
                        },
                    ),
                ]),
                root_component,
            },
        )])
    }

    fn binding(offset: u64, entity_suffix: u64, body: &str) -> DesignBodyBinding {
        serde_json::from_value(serde_json::json!({
            "id": format!("{STREAM}:design-body-binding#{offset}"),
            "stream": "Design/BulkStream.dat",
            "pair_count": 1,
            "pair_ordinal": 0,
            "asm_body_key": 11,
            "asm_body_key_offset": offset,
            "entity_suffix": entity_suffix,
            "entity_suffix_offset": offset + 8,
            "blob_name": "BREP.",
            "blob_name_offset": offset + 32,
            "body": body,
        }))
        .unwrap()
    }

    fn carrier(
        byte_offset: u64,
        occurrence_guid: &str,
        placement: DesignComponentOccurrencePlacement,
        owner_entity: u64,
    ) -> DesignComponentOccurrence {
        DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
            id: format!("{STREAM}:design-component-occurrence#{byte_offset}"),
            class_tag: crate::records::references::DesignClassTag::try_from("339".to_owned())
                .unwrap(),
            record_index: u32::try_from(byte_offset).unwrap(),
            byte_offset,
            component_record_index: 159,
            component_guid: guid("2ad641b3-9ea3-4e5f-a4da-926c3faeccae"),
            occurrence_guid: guid(occurrence_guid),
            placement,
            owner_entity,
        })
        .unwrap()
    }

    fn placed(ordinal: u32, transform: [[f64; 4]; 4]) -> DesignComponentOccurrencePlacement {
        DesignComponentOccurrencePlacement::Explicit {
            ordinal: std::num::NonZeroU32::new(ordinal).unwrap(),
            transform: transform.try_into().unwrap(),
        }
    }

    fn project_carriers(
        carriers: &[DesignComponentOccurrence],
        components: &BTreeMap<String, StreamComponents>,
        bodies: &[&str],
    ) -> super::LocalComponentProjection {
        let bindings = [
            binding(1000, 194, COMPONENT_BODY),
            binding(2000, 300, ROOT_BODY),
        ];
        let bodies = bodies
            .iter()
            .map(|body| BodyId::mint(*body).unwrap())
            .collect::<BTreeSet<_>>();
        super::project_local_components(&super::LocalComponentSources {
            scopes: &[],
            occurrences: carriers,
            components,
            body_bindings: &bindings,
            bodies: Some(&bodies),
            root_ordinal_start: 0,
        })
        .unwrap()
    }

    #[test]
    fn root_contained_carriers_are_placed_in_the_root_occurrence_at_their_transform() {
        const FIRST: &str = "9d257e32-3a1f-45da-bbec-cfc658b575dc";
        const SECOND: &str = "d8ea5386-bc3f-4eb1-93af-09024e6db29a";
        // A quarter turn about z followed by a centimetre translation.
        let transform = [
            [0.0, -1.0, 0.0, 0.42],
            [1.0, 0.0, 0.0, 0.98],
            [0.0, 0.0, 1.0, 2.3],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let carriers = [
            carrier(
                100,
                FIRST,
                DesignComponentOccurrencePlacement::Base,
                DOCUMENT_ROOT,
            ),
            carrier(500, SECOND, placed(2, transform), DOCUMENT_ROOT),
        ];
        let projection = project_carriers(
            &carriers,
            &two_component_stream(Some(276)),
            &[COMPONENT_BODY, ROOT_BODY],
        );
        assert!(projection.losses.is_empty(), "{:?}", projection.losses);

        let component = crate::ids::neutral_component_id(&guid(COMPONENT_CONTEXT));
        let root = crate::ids::neutral_component_id(&guid(ROOT_CONTEXT));
        let definition = |id: &cadmpeg_ir::ids::ProductDefinitionId| {
            projection
                .definitions
                .iter()
                .find(|definition| &definition.id == id)
                .expect("definition")
        };
        assert_eq!(projection.definitions.len(), 2);
        assert_eq!(
            definition(&component).bodies,
            [BodyId::mint(COMPONENT_BODY).unwrap()]
        );
        assert_eq!(definition(&root).bodies, [BodyId::mint(ROOT_BODY).unwrap()]);

        let occurrence = |id: cadmpeg_ir::ids::OccurrenceId| {
            projection
                .occurrences
                .iter()
                .find(|occurrence| occurrence.id == id)
                .expect("occurrence")
                .clone()
        };
        let root_occurrence =
            occurrence(crate::ids::neutral_root_occurrence_id(&guid(ROOT_CONTEXT)));
        assert_eq!(projection.occurrences.len(), 3);
        assert_eq!(root_occurrence.parent, OccurrenceParent::Root {});
        assert_eq!(
            root_occurrence.prototype,
            PrototypeReference::Local { definition: root }
        );
        assert_eq!(
            root_occurrence.transform,
            cadmpeg_ir::transform::Transform::identity()
        );
        let in_root = OccurrenceParent::Occurrence {
            occurrence: root_occurrence.id.clone(),
        };
        let first = occurrence(crate::ids::neutral_component_occurrence_id(&guid(FIRST)));
        let second = occurrence(crate::ids::neutral_component_occurrence_id(&guid(SECOND)));
        for (child, ordinal) in [(&first, 0), (&second, 1)] {
            assert_eq!(child.parent, in_root);
            assert_eq!(child.ordinal, ordinal);
            assert_eq!(
                child.prototype,
                PrototypeReference::Local {
                    definition: component.clone()
                }
            );
        }
        assert_eq!(
            first.transform,
            cadmpeg_ir::transform::Transform::identity()
        );
        let rows = second.transform.rows();
        let expected = [
            [0.0, -1.0, 0.0, 4.2],
            [1.0, 0.0, 0.0, 9.8],
            [0.0, 0.0, 1.0, 23.0],
        ];
        for (row, expected) in rows.iter().zip(expected) {
            for (value, expected) in row.iter().zip(expected) {
                assert!((value - expected).abs() < 1e-12, "{rows:?}");
            }
        }
    }

    #[test]
    fn carriers_the_source_does_not_place_stay_unplaced_with_a_loss() {
        const OCCURRENCE: &str = "443e5aa6-d561-4b5b-9634-661e7da12db3";
        let identity = DesignComponentOccurrencePlacement::Base;
        for (carriers, components, bodies, reason) in [
            (
                [carrier(100, OCCURRENCE, identity, COMPONENT_ROOT)],
                two_component_stream(Some(276)),
                vec![COMPONENT_BODY, ROOT_BODY],
                "nested placement",
            ),
            (
                [carrier(100, OCCURRENCE, identity, DOCUMENT_ROOT)],
                two_component_stream(None),
                vec![COMPONENT_BODY, ROOT_BODY],
                "no unique root instance",
            ),
            (
                [carrier(100, OCCURRENCE, identity, DOCUMENT_ROOT)],
                two_component_stream(Some(276)),
                vec![
                    COMPONENT_BODY,
                    ROOT_BODY,
                    "f3d:brep/other.smbh/brep:entity#1",
                ],
                "a model body has no component definition",
            ),
        ] {
            let projection = project_carriers(&carriers, &components, &bodies);
            assert!(projection.occurrences.is_empty(), "{reason}");
            assert!(projection.definitions.is_empty(), "{reason}");
            assert_eq!(projection.losses.len(), 1, "{reason}");
            assert_eq!(
                projection.losses[0].code,
                crate::loss::F3dLossCode::LocalOccurrenceUnplaced.kind()
            );
            assert!(projection.losses[0].message.contains(reason), "{reason}");
        }
    }

    #[test]
    fn local_component_placement_scales_only_translation_to_millimetres() {
        let transform = [
            [0.0, -1.0, 0.0, 1.25],
            [1.0, 0.0, 0.0, -2.5],
            [0.0, 0.0, 1.0, 3.75],
            [0.0, 0.0, 0.0, 1.0],
        ];
        assert_eq!(
            super::neutral_transform(transform).unwrap().rows(),
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
            DesignComponentOccurrence::try_new(
                crate::records::feature::assembly_features::DesignComponentOccurrenceDraft {
                    id: format!(
                        "f3d:Design/BulkStream.dat:design-component-occurrence#{record_index}"
                    ),
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "256".to_owned(),
                    )
                    .unwrap(),
                    record_index,
                    byte_offset: u64::from(record_index),
                    component_record_index,
                    component_guid: COMPONENT.to_owned().try_into().expect("GUID"),
                    occurrence_guid: occurrence_guid.to_owned().try_into().expect("GUID"),
                    placement: crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Base,
                    owner_entity: 0,
                },
            )
            .unwrap()
        };
        let native_occurrences = [occurrence(100, 700, SOURCE), occurrence(101, 701, COPY)];
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#10",
            crate::records::feature::scope::DesignFeatureKind::CopyPaste,
            10,
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::CopyPaste(slot) =
            scope.payload_mut()
        {
            *slot = Some(DesignCopyPasteComponentOperation {
                relation_record_index: 20,
                source_occurrence_record_index: 100,
                copied_occurrence_record_index: 101,
                component_guid: COMPONENT.to_owned().try_into().expect("GUID"),
                source_occurrence_guid: SOURCE.to_owned().try_into().expect("GUID"),
                copied_occurrence_guid: COPY.to_owned().try_into().expect("GUID"),
                source_transform: identity_matrix().try_into().unwrap(),
                source_transform_offset: 0,
                copied_transform: identity_matrix().try_into().unwrap(),
                copied_transform_offset: 0,
            });
        }

        let (definitions, occurrences) =
            project_without_naming_spaces(&[scope], &native_occurrences);

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
            crate::records::feature::scope::DesignFeatureKind::DerivedInstance,
            385,
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::DerivedInstance(slot) =
            scope.payload_mut()
        {
            *slot = Some(DesignDerivedInstanceConstruction {
                reference_record_index: 305,
                relation_record_index: 383,
                carrier_record_index: 382,
                component_guid: COMPONENT.to_owned().try_into().expect("GUID"),
                occurrence_guid: OCCURRENCE.to_owned().try_into().expect("GUID"),
                transform: identity_matrix().try_into().unwrap(),
                transform_offset: 473,
            });
        }
        let native_occurrence = DesignComponentOccurrence::try_new(
            crate::records::feature::assembly_features::DesignComponentOccurrenceDraft {
                id: "f3d:Design/BulkStream.dat:design-component-occurrence#382".into(),
                class_tag: crate::records::references::DesignClassTag::try_from("380".to_owned())
                    .unwrap(),
                record_index: 382,
                byte_offset: 0,
                component_record_index: 305,
                component_guid: COMPONENT.to_owned().try_into().expect("GUID"),
                occurrence_guid: OCCURRENCE.to_owned().try_into().expect("GUID"),
                placement: crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Explicit {
                    ordinal: std::num::NonZeroU32::MIN,
                    transform: identity_matrix().try_into().unwrap(),
                },
                owner_entity: 0,
            },
        )
        .unwrap();
        let (definitions, occurrences) =
            project_without_naming_spaces(&[scope.clone()], &[native_occurrence]);
        assert_eq!(definitions.len(), 1);
        assert_eq!(occurrences.len(), 1);
        assert_eq!(
            occurrences[0].id,
            crate::ids::neutral_component_occurrence_id(&OCCURRENCE.to_owned().try_into().unwrap())
        );
        assert!(matches!(
            &occurrences[0].prototype,
            PrototypeReference::Local { definition }
                if definition == &crate::ids::neutral_component_id(&COMPONENT.to_owned().try_into().unwrap())
        ));

        let mut feature = Feature {
            id: FeatureId::mint("f3d:model:feature#derived").expect("identity grammar"),
            ordinal: 1,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: "DerivedInstance".into(),
                    parameters: std::collections::BTreeMap::new(),
                }),
            ),
            native_ref: None,
        };
        feature.native_ref = Some(scope.id.clone());
        super::project_derived_instance_features(std::slice::from_mut(&mut feature), &[scope]);
        assert_eq!(
            *feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::InsertComponent {
                occurrence: crate::ids::neutral_component_occurrence_id(
                    &OCCURRENCE.to_owned().try_into().unwrap()
                ),
            })
        );
    }
}
