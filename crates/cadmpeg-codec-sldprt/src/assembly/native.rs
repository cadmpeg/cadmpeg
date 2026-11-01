// SPDX-License-Identifier: Apache-2.0
//! Native assembly structure of the component instance tree.
//!
//! The tree walk records native definitions and placed occurrences in source
//! order. [`NativeAssembly::into_projection`] is the single mapping to
//! [`AssemblyProjection`].

use std::collections::BTreeMap;

use cadmpeg_ir::ids::{IdentityKey, OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    ExternalDocument, Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind,
    PrototypeReference,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::transform::Transform;

use crate::loss::SldprtLossCode;

use super::AssemblyProjection;

/// Key of the root occurrence, the assembly document itself.
const ROOT_KEY: &str = "root";

/// Definitions and placements in native order, before neutral product
/// structure is projected.
#[derive(Debug, Default)]
pub(super) struct NativeAssembly {
    definitions: Vec<NativeDefinition>,
    placements: Vec<NativePlacement>,
    losses: Vec<LossNote>,
}

impl NativeAssembly {
    /// An assembly that transferred no structure, with its blocking loss.
    pub(super) fn structure_lost(message: String) -> Self {
        let mut assembly = Self::default();
        assembly
            .losses
            .push(SldprtLossCode::AssemblyStructureNotTransferred.note(message));
        assembly
    }

    pub(super) fn push_definition(&mut self, definition: NativeDefinition) {
        self.definitions.push(definition);
    }

    pub(super) fn push_placement(&mut self, placement: NativePlacement) {
        self.placements.push(placement);
    }

    pub(super) fn push_loss(&mut self, loss: LossNote) {
        self.losses.push(loss);
    }

    /// Map the native structure to the neutral product projection.
    pub(super) fn into_projection(self) -> AssemblyProjection {
        AssemblyProjection {
            definitions: self
                .definitions
                .into_iter()
                .map(NativeDefinition::into_product_definition)
                .collect(),
            occurrences: self
                .placements
                .into_iter()
                .map(NativePlacement::into_occurrence)
                .collect(),
            losses: self.losses,
            carries_parasolid: false,
        }
    }

    #[cfg(test)]
    pub(super) fn definitions(&self) -> &[NativeDefinition] {
        &self.definitions
    }

    #[cfg(test)]
    pub(super) fn placements(&self) -> &[NativePlacement] {
        &self.placements
    }

    #[cfg(test)]
    pub(super) fn losses(&self) -> &[LossNote] {
        &self.losses
    }
}

/// One product definition in native terms.
#[derive(Debug)]
pub(super) struct NativeDefinition {
    id: ProductDefinitionId,
    source_name: Option<String>,
    native_ref: String,
}

impl NativeDefinition {
    /// A `swModel` definition of the component instance tree, or `None` when
    /// the model identity is not a valid identity key.
    pub(super) fn tree_model(model: &str, source_name: Option<String>) -> Option<Self> {
        let id = ProductDefinitionId::compose(
            &definition_namespace(),
            IdentityKey::try_new(model).ok()?,
        );
        Some(Self {
            id,
            source_name,
            native_ref: native_model_ref(model),
        })
    }

    pub(super) fn id(&self) -> &ProductDefinitionId {
        &self.id
    }

    fn into_product_definition(self) -> ProductDefinition {
        ProductDefinition {
            id: self.id,
            kind: ProductDefinitionKind::Part,
            source_name: self.source_name,
            label: None,
            description: None,
            part_number: None,
            bom_properties: BTreeMap::default(),
            bodies: Vec::new(),
            sketches: Vec::new(),
            native_ref: Some(self.native_ref),
        }
    }

    #[cfg(test)]
    pub(super) fn source_name(&self) -> Option<&str> {
        self.source_name.as_deref()
    }

    #[cfg(test)]
    pub(super) fn native_ref(&self) -> &str {
        &self.native_ref
    }
}

/// The prototype of one native placement.
#[derive(Debug, Clone)]
pub(super) enum NativePrototype {
    /// A definition in this document.
    Local(ProductDefinitionId),
    /// A document the decoder does not open.
    External {
        document: String,
        object: Option<String>,
    },
    /// A virtual component, stored in the assembly document.
    Unresolved,
}

impl NativePrototype {
    /// An external reference to `document`, naming `configuration` as the
    /// object when the source supplies a nonblank one.
    pub(super) fn external(document: impl Into<String>, configuration: Option<&str>) -> Self {
        Self::External {
            document: document.into(),
            object: configuration
                .filter(|configuration| !configuration.trim().is_empty())
                .map(str::to_string),
        }
    }

    fn into_prototype_reference(self) -> PrototypeReference {
        match self {
            Self::Local(definition) => PrototypeReference::Local { definition },
            Self::External { document, object } => PrototypeReference::External {
                document: ExternalDocument::path(document),
                object,
            },
            Self::Unresolved => PrototypeReference::Unresolved {},
        }
    }
}

/// One placed occurrence in native terms.
#[derive(Debug, Clone)]
pub(super) struct NativePlacement {
    id: OccurrenceId,
    prototype: NativePrototype,
    parent: OccurrenceParent,
    ordinal: u32,
    transform: Transform,
    name: Option<String>,
    visible: Option<bool>,
    native_ref: String,
}

impl NativePlacement {
    /// The key and identity of a component-tree child occurrence.
    ///
    /// A `swReference` id is joined to the parent key with a dash, so one
    /// model placed at two paths yields two distinct occurrences.
    ///
    /// `None` when the reference identity is not a valid identity key.
    pub(super) fn tree_occurrence_id(
        parent_key: Option<&IdentityKey>,
        reference: &str,
    ) -> Option<(IdentityKey, OccurrenceId)> {
        let id_key = IdentityKey::try_new(reference).ok()?;
        let key = match parent_key {
            Some(parent_key) => parent_key.clone().dash(id_key),
            None => id_key,
        };
        let occurrence = OccurrenceId::compose(&occurrence_namespace(), key.clone());
        Some((key, occurrence))
    }

    /// The root occurrence of a component tree.
    pub(super) fn tree_root(
        model: &str,
        definition: ProductDefinitionId,
        name: Option<String>,
    ) -> Option<Self> {
        let key = IdentityKey::try_new(ROOT_KEY).ok()?;
        Some(Self {
            id: OccurrenceId::compose(&occurrence_namespace(), key),
            prototype: NativePrototype::Local(definition),
            parent: OccurrenceParent::Root {},
            ordinal: 0,
            transform: Transform::identity(),
            name,
            visible: None,
            native_ref: native_model_ref(model),
        })
    }

    /// A child occurrence of a component tree.
    pub(super) fn tree_child(
        occurrence: OccurrenceId,
        prototype: NativePrototype,
        parent: OccurrenceId,
        reference: &str,
    ) -> Self {
        Self {
            id: occurrence,
            prototype,
            parent: OccurrenceParent::Occurrence { occurrence: parent },
            ordinal: 0,
            transform: Transform::identity(),
            name: None,
            visible: None,
            native_ref: format!(
                "sldprt:tree:swReference#{}",
                IdentityKey::encode_segment(reference).as_str()
            ),
        }
    }

    /// Set the placement fields of a native occurrence.
    pub(super) fn placed(
        mut self,
        ordinal: u32,
        transform: Transform,
        name: String,
        visible: Option<bool>,
    ) -> Self {
        self.ordinal = ordinal;
        self.transform = transform;
        self.name = Some(name);
        self.visible = visible;
        self
    }

    pub(super) fn id(&self) -> &OccurrenceId {
        &self.id
    }

    fn into_occurrence(self) -> Occurrence {
        Occurrence {
            id: self.id,
            prototype: self.prototype.into_prototype_reference(),
            parent: self.parent,
            ordinal: self.ordinal,
            transform: self.transform,
            linked_prototype: None,
            scale: [FiniteReal::ONE; 3],
            name: self.name,
            visible: self.visible,
            link: None,
            native_ref: Some(self.native_ref),
        }
    }

    #[cfg(test)]
    pub(super) fn native_ref(&self) -> &str {
        &self.native_ref
    }

    #[cfg(test)]
    pub(super) fn prototype(&self) -> &NativePrototype {
        &self.prototype
    }

    #[cfg(test)]
    pub(super) fn parent(&self) -> &OccurrenceParent {
        &self.parent
    }

    #[cfg(test)]
    pub(super) fn ordinal(&self) -> u32 {
        self.ordinal
    }

    #[cfg(test)]
    pub(super) fn transform(&self) -> Transform {
        self.transform
    }

    #[cfg(test)]
    pub(super) fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[cfg(test)]
    pub(super) fn visible(&self) -> Option<bool> {
        self.visible
    }
}

fn definition_namespace() -> cadmpeg_ir::ids::IdentityNamespace {
    cadmpeg_ir::identity_namespace!("sldprt", "assembly", "model")
}

fn occurrence_namespace() -> cadmpeg_ir::ids::IdentityNamespace {
    cadmpeg_ir::identity_namespace!("sldprt", "assembly", "occurrence")
}

/// The native identity of a `swModel` element.
fn native_model_ref(model: &str) -> String {
    format!(
        "sldprt:tree:swModel#{}",
        IdentityKey::encode_segment(model).as_str()
    )
}

/// The file stem of a document path.
pub(super) fn document_stem(name: &str) -> Option<String> {
    let file = name.rsplit(['\\', '/']).next().unwrap_or(name);
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    (!stem.trim().is_empty()).then(|| stem.to_string())
}
