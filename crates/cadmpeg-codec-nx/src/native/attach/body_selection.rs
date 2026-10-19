use super::{
    BTreeMap, BodyId, BodySelection, BodyWriterHistory, FeatureDefinition, FeatureId,
    FeatureOperation,
};

/// Identity namespace used to prove that two Boolean selections are disjoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FeatureBodyIdentity {
    Segment(u32),
    OffsetStore(String),
}

pub(super) fn offset_store_identity(data_block: &str) -> Option<&str> {
    data_block
        .strip_prefix("nx:om-data-blocks-")
        .and_then(|data_block| data_block.split_once(":block#"))
        .map(|(store, _)| store)
}

pub(super) enum FeatureBodySelection {
    Native(String),
    Local {
        bodies: Vec<String>,
        native: String,
        identity_keys: Vec<FeatureBodyIdentity>,
    },
    Resolved {
        bodies: Vec<BodyId>,
        native: String,
        identity_keys: Vec<FeatureBodyIdentity>,
    },
}

pub(super) fn local_body_selection(bodies: &[String], native: String) -> BodySelection {
    BodySelection::local(bodies.to_vec(), native.clone()).unwrap_or(BodySelection::Native(native))
}

impl FeatureBodySelection {
    pub(super) fn into_selection(self) -> BodySelection {
        match self {
            Self::Native(native) => BodySelection::Native(native),
            Self::Local { bodies, native, .. } => local_body_selection(&bodies, native),
            Self::Resolved { bodies, native, .. } => {
                if bodies.is_empty() {
                    return BodySelection::Native(native);
                }
                match cadmpeg_ir::features::DistinctMembers::try_from(bodies) {
                    Ok(bodies) => BodySelection::Resolved { bodies, native },
                    Err(cadmpeg_ir::features::FeatureCollectionError::Invalid(_)) => {
                        BodySelection::Native(native)
                    }
                }
            }
        }
    }

    fn into_native(self) -> BodySelection {
        let (Self::Native(native) | Self::Local { native, .. } | Self::Resolved { native, .. }) =
            self;
        BodySelection::Native(native)
    }
}

/// Resolve a complete object-index selection only when every alias root owns one
/// decoded body image. Retain the complete feature-input-local identities when
/// current topology cannot represent a consumed historical body. An offset-store
/// selection uses the exact data-block identities from its feature-history
/// section. A complete operation-local offset-store map takes precedence over
/// a segment alias with the same integer; mixed namespace coverage remains
/// native.
pub(super) fn feature_body_selection(
    object_indices: &[u32],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
    native: String,
) -> FeatureBodySelection {
    feature_body_selection_with_offset_blocks(
        object_indices,
        body_alias_roots,
        &BTreeMap::new(),
        bodies_by_object_index,
        native,
    )
}

pub(super) fn feature_body_selection_with_offset_blocks(
    object_indices: &[u32],
    body_alias_roots: &BTreeMap<u32, u32>,
    offset_store_body_blocks: &BTreeMap<u32, String>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
    native: String,
) -> FeatureBodySelection {
    let mut roots = Vec::new();
    let mut offset_blocks: Vec<&String> = Vec::new();
    for index in object_indices {
        match (
            body_alias_roots.get(index),
            offset_store_body_blocks.get(index),
        ) {
            (Some(_), Some(data_block)) => {
                if !offset_blocks.contains(&data_block) {
                    offset_blocks.push(data_block);
                }
            }
            (Some(root), None) => {
                if !roots.contains(root) {
                    roots.push(*root);
                }
            }
            (None, Some(data_block)) => {
                if !offset_blocks.contains(&data_block) {
                    offset_blocks.push(data_block);
                }
            }
            (None, None) => {
                return FeatureBodySelection::Native(native);
            }
        }
    }
    if !roots.is_empty() && !offset_blocks.is_empty() {
        return FeatureBodySelection::Native(native);
    }
    let offset_store = offset_blocks
        .first()
        .and_then(|block| offset_store_identity(block));
    if !offset_blocks.is_empty()
        && (offset_store.is_none()
            || offset_blocks
                .iter()
                .any(|block| offset_store_identity(block) != offset_store))
    {
        return FeatureBodySelection::Native(native);
    }
    if !offset_blocks.is_empty() {
        let mut bodies = Vec::new();
        let mut identity_keys = Vec::new();
        for block in offset_blocks {
            bodies.push(block.clone());
            identity_keys.push(FeatureBodyIdentity::OffsetStore(block.clone()));
        }
        return FeatureBodySelection::Local {
            bodies,
            native,
            identity_keys,
        };
    }
    let mut resolved = Vec::new();
    let mut all_resolved = true;
    for root in &roots {
        let Some([body]) = bodies_by_object_index.get(root).map(Vec::as_slice) else {
            all_resolved = false;
            break;
        };

        if resolved.contains(body) {
            all_resolved = false;
            break;
        }

        resolved.push(body.clone());
    }
    if all_resolved {
        let mut identity_keys = Vec::new();
        for root in roots {
            identity_keys.push(FeatureBodyIdentity::Segment(root));
        }
        return FeatureBodySelection::Resolved {
            bodies: resolved,
            native,
            identity_keys,
        };
    }
    let mut bodies = Vec::new();
    let mut identity_keys = Vec::new();
    for root in roots {
        bodies.push(format!("nx:om-body-object#{root}"));
        identity_keys.push(FeatureBodyIdentity::Segment(root));
    }
    FeatureBodySelection::Local {
        bodies,
        native,
        identity_keys,
    }
}

/// Resolve one complete body set when possible. Otherwise retain every exact
/// input-local object identity. A body set needs no cross-role disjointness
/// proof, so an identity outside the segment alias table remains its own root.
pub(super) fn feature_body_set_selection(
    object_indices: &[u32],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
    native: String,
) -> BodySelection {
    let mut roots = Vec::new();
    for index in object_indices {
        let root = body_alias_roots.get(index).copied().unwrap_or(*index);
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    let mut resolved = Vec::new();
    let mut all_resolved = true;
    for root in &roots {
        let Some([body]) = bodies_by_object_index.get(root).map(Vec::as_slice) else {
            all_resolved = false;
            break;
        };

        if resolved.contains(body) {
            all_resolved = false;
            break;
        }

        resolved.push(body.clone());
    }
    if all_resolved && !resolved.is_empty() {
        return FeatureBodySelection::Resolved {
            bodies: resolved,
            native,
            identity_keys: Vec::new(),
        }
        .into_selection();
    }
    let mut bodies = Vec::new();
    for root in roots {
        bodies.push(format!("nx:om-body-object#{root}"));
    }
    FeatureBodySelection::Local {
        bodies,
        native,
        identity_keys: Vec::new(),
    }
    .into_selection()
}

pub(super) fn atomic_disjoint_body_selections(
    left: FeatureBodySelection,
    right: FeatureBodySelection,
) -> (BodySelection, BodySelection) {
    let complete = match (&left, &right) {
        (
            FeatureBodySelection::Local {
                identity_keys: left,
                ..
            }
            | FeatureBodySelection::Resolved {
                identity_keys: left,
                ..
            },
            FeatureBodySelection::Local {
                identity_keys: right,
                ..
            }
            | FeatureBodySelection::Resolved {
                identity_keys: right,
                ..
            },
        ) => {
            let same_namespace =
                left.first()
                    .zip(right.first())
                    .is_none_or(|(left, right)| match (left, right) {
                        (FeatureBodyIdentity::Segment(_), FeatureBodyIdentity::Segment(_)) => true,
                        (
                            FeatureBodyIdentity::OffsetStore(left),
                            FeatureBodyIdentity::OffsetStore(right),
                        ) => offset_store_identity(left) == offset_store_identity(right),
                        _ => false,
                    });
            same_namespace && !left.iter().any(|key| right.contains(key))
        }
        _ => false,
    };
    if complete {
        (left.into_selection(), right.into_selection())
    } else {
        (left.into_native(), right.into_native())
    }
}

/// Resolve one Boolean participant through the namespace selected by the
/// complete Boolean definition. Native integer identity is used only when the
/// definition did not establish one exact offset-store selection.
pub(super) fn boolean_participant_writer<'a>(
    selection: &BodySelection,
    object_index: u32,
    offset_store_body_blocks: Option<&BTreeMap<u32, String>>,
    body_alias_roots: &BTreeMap<u32, u32>,
    history: &'a BodyWriterHistory,
) -> Option<&'a FeatureId> {
    let offset_store_selection = matches!(
        selection,
        BodySelection::Local { bodies, .. }
            if !bodies.is_empty()
                && bodies
                    .iter()
                    .all(|body| offset_store_identity(body).is_some())
    );
    if offset_store_selection {
        return offset_store_body_blocks
            .and_then(|blocks| blocks.get(&object_index))
            .and_then(|data_block| history.offset_store_writer(data_block));
    }
    history.native_writer(
        body_alias_roots
            .get(&object_index)
            .copied()
            .unwrap_or(object_index),
    )
}

/// Register a Boolean's target in the namespace established by its complete
/// target selection. An offset-store target must not create a native writer
/// for the same integer object index.
pub(super) fn boolean_target_writer(
    definition: &FeatureDefinition,
    native_body: u32,
) -> (Option<u32>, Option<&str>) {
    if let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) = definition {
        if let BodySelection::Local { bodies, .. } = operands.target() {
            if let [body] = bodies.as_slice() {
                if offset_store_identity(body).is_some() {
                    return (None, Some(body.as_str()));
                }
            }
        }
    }
    (Some(native_body), None)
}

pub(super) fn boolean_target_output(definition: Option<&FeatureDefinition>) -> Option<&BodyId> {
    let Some(FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. })) = definition
    else {
        return None;
    };
    let BodySelection::Resolved { bodies, .. } = operands.target() else {
        return None;
    };
    bodies.first()
}
