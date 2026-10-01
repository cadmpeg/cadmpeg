use super::{
    BTreeMap, BodyId, BodySelection, BodyWriterHistory, CodecError, DecodeContext,
    FeatureDefinition, FeatureId, FeatureOperation,
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

pub(super) enum FeatureBodySelection<'ctx> {
    Native(String),
    Local {
        bodies: Vec<String>,
        native: String,
        identity_keys: Vec<FeatureBodyIdentity>,
        _reservation: cadmpeg_core::decode::ScopedReservation<'ctx>,
    },
    Resolved {
        bodies: Vec<BodyId>,
        native: String,
        identity_keys: Vec<FeatureBodyIdentity>,
        _reservation: cadmpeg_core::decode::ScopedReservation<'ctx>,
    },
}

pub(super) fn local_body_selection(
    ctx: &DecodeContext<'_>,
    bodies: Vec<String>,
    native: String,
) -> Result<BodySelection, CodecError> {
    let bodies = ctx.try_collect_retained_with(bodies.iter(), "NX local body selection members", |body| ctx.copy_retained_text(body, "NX local body selection identity"))?;
    let native_copy = ctx.copy_retained_text(&native, "NX feature projection text")?;
    Ok(BodySelection::local_for_decode(bodies, native_copy, ctx)?.unwrap_or(BodySelection::Native(native)))
}

impl FeatureBodySelection<'_> {
    pub(super) fn into_selection(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<BodySelection, CodecError> {
        match self {
            Self::Native(native) => Ok(BodySelection::Native(native)),
            Self::Local { bodies, native, .. } => local_body_selection(ctx, bodies, native),
            Self::Resolved { bodies, native, .. } => {
                if bodies.is_empty() { return Ok(BodySelection::Native(native)); }
                let bodies = ctx.try_collect_retained_with(bodies.iter(), "NX resolved body selection members", |body| body.try_clone_for_decode(ctx, "NX resolved body selection identity"))?;
                let bodies = match cadmpeg_ir::features::DistinctMembers::try_from_for_decode(bodies, ctx) {
                    Ok(bodies) => bodies,
                    Err(cadmpeg_ir::features::FeatureCollectionError::Resource(limit)) => return Err(limit.into()),
                    Err(cadmpeg_ir::features::FeatureCollectionError::Invalid(_)) => return Ok(BodySelection::Native(native)),
                };
                Ok(BodySelection::Resolved { bodies, native })
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
pub(super) fn feature_body_selection<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    object_indices: &[u32],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
    native: String,
) -> Result<FeatureBodySelection<'ctx>, CodecError> {
    feature_body_selection_with_offset_blocks(
        ctx,
        object_indices,
        body_alias_roots,
        &BTreeMap::new(),
        bodies_by_object_index,
        native,
    )
}

pub(super) fn feature_body_selection_with_offset_blocks<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    object_indices: &[u32],
    body_alias_roots: &BTreeMap<u32, u32>,
    offset_store_body_blocks: &BTreeMap<u32, String>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
    native: String,
) -> Result<FeatureBodySelection<'ctx>, CodecError> {
    let mut roots = Vec::new();
    let mut offset_blocks: Vec<&String> = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX feature body selection")?;
    for index in object_indices {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(
                roots
                    .len()
                    .checked_add(offset_blocks.len())
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit("NX feature body selection lookup", 0, 1)
                    })?,
            ),
            "NX feature body selection lookup",
        )?;
        match (
            body_alias_roots.get(index),
            offset_store_body_blocks.get(index),
        ) {
            (Some(_), Some(data_block)) => {
                if !offset_blocks.contains(&data_block) {
                    ctx.reserve_scoped_vec(
                        &mut reservation,
                        &mut offset_blocks,
                        1,
                        "NX feature body offset blocks",
                    )?;
                    offset_blocks.push(data_block);
                }
            }
            (Some(root), None) => {
                if !roots.contains(root) {
                    ctx.reserve_scoped_vec(
                        &mut reservation,
                        &mut roots,
                        1,
                        "NX feature body roots",
                    )?;
                    roots.push(*root);
                }
            }
            (None, Some(data_block)) => {
                if !offset_blocks.contains(&data_block) {
                    ctx.reserve_scoped_vec(
                        &mut reservation,
                        &mut offset_blocks,
                        1,
                        "NX feature body offset blocks",
                    )?;
                    offset_blocks.push(data_block);
                }
            }
            (None, None) => {
                return Ok(FeatureBodySelection::Native(native));
            }
        }
    }
    if !roots.is_empty() && !offset_blocks.is_empty() {
        return Ok(FeatureBodySelection::Native(native));
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
        return Ok(FeatureBodySelection::Native(native));
    }
    if !offset_blocks.is_empty() {
        let mut bodies = Vec::new();
        let mut identity_keys = Vec::new();
        for block in offset_blocks {
            ctx.charge_collection_items(2, "NX feature body offset selection")?;
            let bytes = std::mem::size_of::<String>()
                .checked_add(std::mem::size_of::<FeatureBodyIdentity>())
                .and_then(|bytes| bytes.checked_add(block.len().checked_mul(2)?))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "NX feature body offset selection",
                        0,
                        cadmpeg_core::decode::u64_from_index(block.len()),
                    )
                })?;
            reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
            cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                &mut bodies,
                1,
                "NX feature body offset selection",
            )?;
            cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                &mut identity_keys,
                1,
                "NX feature body offset identities",
            )?;
            bodies.push(block.clone());
            identity_keys.push(FeatureBodyIdentity::OffsetStore(block.clone()));
        }
        return Ok(FeatureBodySelection::Local {
            bodies,
            native,
            identity_keys,
            _reservation: reservation,
        });
    }
    let mut resolved = Vec::new();
    let mut all_resolved = true;
    for root in &roots {
        let Some([body]) = bodies_by_object_index.get(root).map(Vec::as_slice) else {
            all_resolved = false;
            break;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(resolved.len()),
            "NX feature body resolved uniqueness",
        )?;
        if resolved.contains(body) {
            all_resolved = false;
            break;
        }
        ctx.charge_collection_items(1, "NX feature body resolved candidates")?;
        let bytes = std::mem::size_of::<BodyId>()
            .checked_add(body.as_str().len())
            .ok_or_else(|| ctx.refuse_codec_limit("NX feature body resolved candidate", 0, 1))?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut resolved,
            1,
            "NX feature body resolved candidates",
        )?;
        resolved.push(body.clone());
    }
    if all_resolved {
        let mut identity_keys = Vec::new();
        for root in roots {
            ctx.reserve_scoped_vec(
                &mut reservation,
                &mut identity_keys,
                1,
                "NX feature body segment identities",
            )?;
            identity_keys.push(FeatureBodyIdentity::Segment(root));
        }
        return Ok(FeatureBodySelection::Resolved {
            bodies: resolved,
            native,
            identity_keys,
            _reservation: reservation,
        });
    }
    let mut bodies = Vec::new();
    let mut identity_keys = Vec::new();
    for root in roots {
        ctx.charge_collection_items(2, "NX feature body local selection")?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<String>() + std::mem::size_of::<FeatureBodyIdentity>(),
        ))?;
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut bodies,
            1,
            "NX feature body local selection",
        )?;
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut identity_keys,
            1,
            "NX feature body local identities",
        )?;
        bodies.push(ctx.format_scoped_text(
            &mut reservation,
            format_args!("nx:om-body-object#{root}"),
            "NX body selection text",
        )?);
        identity_keys.push(FeatureBodyIdentity::Segment(root));
    }
    Ok(FeatureBodySelection::Local {
        bodies,
        native,
        identity_keys,
        _reservation: reservation,
    })
}

/// Resolve one complete body set when possible. Otherwise retain every exact
/// input-local object identity. A body set needs no cross-role disjointness
/// proof, so an identity outside the segment alias table remains its own root.
pub(super) fn feature_body_set_selection(
    ctx: &DecodeContext<'_>,
    object_indices: &[u32],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
    native: String,
) -> Result<BodySelection, CodecError> {
    let mut roots = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX feature body set")?;
    for index in object_indices {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(roots.len()),
            "NX feature body set roots",
        )?;
        let root = body_alias_roots.get(index).copied().unwrap_or(*index);
        if !roots.contains(&root) {
            ctx.reserve_scoped_vec(&mut reservation, &mut roots, 1, "NX feature body set roots")?;
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
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(resolved.len()),
            "NX feature body set uniqueness",
        )?;
        if resolved.contains(body) {
            all_resolved = false;
            break;
        }
        ctx.charge_collection_items(1, "NX feature body set resolved candidates")?;
        let bytes = std::mem::size_of::<BodyId>()
            .checked_add(body.as_str().len())
            .ok_or_else(|| ctx.refuse_codec_limit("NX feature body set candidate", 0, 1))?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut resolved,
            1,
            "NX feature body set resolved candidates",
        )?;
        resolved.push(body.clone());
    }
    if all_resolved && !resolved.is_empty() {
        return FeatureBodySelection::Resolved {
            bodies: resolved,
            native,
            identity_keys: Vec::new(),
            _reservation: reservation,
        }
        .into_selection(ctx);
    }
    let mut bodies = Vec::new();
    for root in roots {
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut bodies,
            1,
            "NX feature body set local selection",
        )?;
        bodies.push(ctx.format_scoped_text(
            &mut reservation,
            format_args!("nx:om-body-object#{root}"),
            "NX body selection text",
        )?);
    }
    FeatureBodySelection::Local {
        bodies,
        native,
        identity_keys: Vec::new(),
        _reservation: reservation,
    }
    .into_selection(ctx)
}

pub(super) fn atomic_disjoint_body_selections(
    ctx: &DecodeContext<'_>,
    left: FeatureBodySelection<'_>,
    right: FeatureBodySelection<'_>,
) -> Result<(BodySelection, BodySelection), CodecError> {
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
        Ok((left.into_selection(ctx)?, right.into_selection(ctx)?))
    } else {
        Ok((left.into_native(), right.into_native()))
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
