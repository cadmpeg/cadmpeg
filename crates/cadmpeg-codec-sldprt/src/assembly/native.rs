// SPDX-License-Identifier: Apache-2.0
//! Native assembly structure of the component instance tree.
//!
//! The tree walk records native definitions and placed occurrences in source
//! order. [`NativeAssembly::into_projection`] is the single mapping to
//! [`AssemblyProjection`].

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{ArenaClass, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{IdentityKey, OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    ExternalDocument, Occurrence, ProductDefinition, ProductDefinitionKind, Storage,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::unknown::part::{DecodeParts, GivenUpPart, Part};

use crate::loss::SldprtLossCode;

use super::AssemblyProjection;

/// The kind of a component tree section the decode gives up.
const TREE_PART: &str = "component-tree";

/// A component tree section an assembly reads: its stream and its bytes as
/// the file stores them.
#[derive(Debug, Clone)]
pub(crate) struct TreeSection<'a> {
    pub(crate) stream: cadmpeg_ir::StreamName,
    pub(crate) stored: &'a [u8],
}

/// Definitions and placements in native order, before neutral product
/// structure is projected.
#[derive(Debug, Default)]
pub(super) struct NativeAssembly {
    definitions: Vec<NativeDefinition>,
    placements: Vec<NativePlacement>,
    losses: Vec<LossNote>,
    given_up: Vec<GivenUpPart>,
}

impl NativeAssembly {
    /// An assembly that transferred no structure because the document holds
    /// no component tree section, with its blocking loss.
    pub(super) fn structure_lost(message: String) -> Self {
        let mut assembly = Self::default();
        assembly
            .losses
            .push(SldprtLossCode::AssemblyStructureNotTransferred.note(message));
        assembly
    }

    /// An assembly that transferred no structure from its component tree
    /// sections `trees`, with its blocking loss. Each stream of the sections
    /// is given up once. The section records retain the bytes.
    pub(super) fn structure_given_up(
        ctx: &DecodeContext<'_>,
        trees: &[TreeSection<'_>],
        message: String,
    ) -> Result<Self, CodecError> {
        let why = SldprtLossCode::AssemblyStructureNotTransferred.note(message);
        let mut assembly = Self::default();
        let mut streams = BTreeSet::new();
        for tree in trees {
            if !streams.insert(tree.stream.as_str()) {
                continue;
            }
            let part = Part::at("sldprt", TREE_PART, 0, tree.stored, [ArenaClass::Model])
                .named(tree.stream.as_str())
                .in_stream(tree.stream.clone())
                .digest_only();
            let error = CodecError::malformed(format_args!("{}: {}", why.code, why.message));
            assembly.given_up.push(ctx.give_up_part(part, error)?);
        }
        assembly.losses.push(why);
        Ok(assembly)
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

    /// Map the native structure to the neutral product projection. Each
    /// placement is an occurrence of the definition that owns it; a placement
    /// of a document this one does not hold places a definition of external
    /// storage, one per document and configuration.
    pub(super) fn into_projection(self) -> AssemblyProjection {
        let virtual_sites = self
            .definitions
            .iter()
            .filter_map(|definition| {
                let site = definition.bodies_site.clone()?;
                let name = definition
                    .source_name
                    .clone()
                    .unwrap_or_else(|| definition.native_ref.clone());
                Some((definition.id.clone(), site, name))
            })
            .collect();
        let mut definitions: Vec<ProductDefinition> = self
            .definitions
            .into_iter()
            .map(NativeDefinition::into_product_definition)
            .collect();
        let mut externals = BTreeMap::<(String, Option<String>), ProductDefinitionId>::new();
        let mut occurrences = Vec::new();
        for placement in self.placements {
            let prototype = match placement.prototype {
                NativePrototype::Local(definition) => definition,
                NativePrototype::External { document, object } => externals
                    .entry((document.clone(), object.clone()))
                    .or_insert_with(|| {
                        let mut key = IdentityKey::encode_segment(&document);
                        if let Some(object) = &object {
                            key = key.dash(IdentityKey::encode_segment(object));
                        }
                        let id = ProductDefinitionId::compose(&external_namespace(), key);
                        let mut external = ExternalDocument::path(document.clone());
                        if let Some(object) = &object {
                            external = external.with_object(object.clone());
                        }
                        let mut definition =
                            ProductDefinition::new(id.clone(), Storage::External(external));
                        definition.source_name = document_stem(&document);
                        definitions.push(definition);
                        id
                    })
                    .clone(),
                NativePrototype::Unresolved => {
                    let id = ProductDefinitionId::compose(
                        &unresolved_namespace(),
                        placement.key.clone(),
                    );
                    definitions.push(ProductDefinition::new(
                        id.clone(),
                        Storage::External(ExternalDocument::default()),
                    ));
                    id
                }
            };
            if let Some(owner) = definitions
                .iter_mut()
                .find(|definition| definition.id == placement.owner)
            {
                owner.occurrences.push(placement.id.clone());
            }
            let mut occurrence =
                Occurrence::new(placement.id, prototype, Some(placement.transform));
            occurrence.name = placement.name;
            occurrence.visible = placement.visible;
            occurrence.native_ref = Some(placement.native_ref);
            occurrences.push(occurrence);
        }
        AssemblyProjection {
            definitions,
            occurrences,
            losses: self.losses,
            given_up: self.given_up,
            carries_parasolid: false,
            virtual_sites,
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
    kind: ProductDefinitionKind,
    /// Whether this is the model of the document being decoded.
    root: bool,
    source_name: Option<String>,
    native_ref: String,
    /// The site key of the virtual component section whose bodies the
    /// definition owns.
    bodies_site: Option<String>,
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
            kind: ProductDefinitionKind::Assembly,
            root: false,
            source_name,
            native_ref: native_model_ref(model),
            bodies_site: None,
        })
    }

    /// The definition that owns the bodies decoded from the site `site`.
    pub(super) fn with_bodies_from(mut self, site: String) -> Self {
        self.bodies_site = Some(site);
        self.kind = ProductDefinitionKind::Part;
        self
    }

    /// The model of the document being decoded, the root definition.
    pub(super) fn into_root(mut self) -> Self {
        self.root = true;
        self
    }

    pub(super) fn id(&self) -> &ProductDefinitionId {
        &self.id
    }

    fn into_product_definition(self) -> ProductDefinition {
        let mut definition = if self.root {
            ProductDefinition::root(self.id, ExternalDocument::default())
        } else {
            ProductDefinition::new(self.id, Storage::Embedded)
        };
        definition.kind = Some(self.kind);
        definition.source_name = self.source_name;
        definition.native_ref = Some(self.native_ref);
        definition
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
    /// A virtual component whose document this assembly document does not
    /// hold.
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
}

/// One placed occurrence in native terms.
#[derive(Debug, Clone)]
pub(super) struct NativePlacement {
    id: OccurrenceId,
    key: IdentityKey,
    prototype: NativePrototype,
    owner: ProductDefinitionId,
    ordinal: u32,
    transform: Transform,
    name: Option<String>,
    visible: Option<bool>,
    native_ref: String,
}

impl NativePlacement {
    /// The key and identity of a component instance: its `swReference` id,
    /// the source's stable instance key.
    ///
    /// `None` when the reference identity is not a valid identity key.
    pub(super) fn tree_occurrence_id(reference: &str) -> Option<(IdentityKey, OccurrenceId)> {
        let key = IdentityKey::try_new(reference).ok()?;
        let occurrence = OccurrenceId::compose(&occurrence_namespace(), key.clone());
        Some((key, occurrence))
    }

    /// A component instance that the definition `owner` places.
    pub(super) fn tree_child(
        (key, occurrence): (IdentityKey, OccurrenceId),
        prototype: NativePrototype,
        owner: ProductDefinitionId,
        reference: &str,
    ) -> Self {
        Self {
            id: occurrence,
            key,
            prototype,
            owner,
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

    #[cfg(test)]
    pub(super) fn id(&self) -> &OccurrenceId {
        &self.id
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
    pub(super) fn owner(&self) -> &ProductDefinitionId {
        &self.owner
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

fn external_namespace() -> cadmpeg_ir::ids::IdentityNamespace {
    cadmpeg_ir::identity_namespace!("sldprt", "assembly", "external")
}

fn unresolved_namespace() -> cadmpeg_ir::ids::IdentityNamespace {
    cadmpeg_ir::identity_namespace!("sldprt", "assembly", "unresolved")
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
