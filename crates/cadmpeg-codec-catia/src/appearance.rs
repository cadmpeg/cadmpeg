//! Transfer of byte-proven CATIA display colors to neutral appearance bindings.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::ids::{AppearanceBindingId, AppearanceId};
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::CadIr;

use crate::families::standard::fbb::standard_face_colors;
use crate::native::CatiaNative;
use crate::value_block::ValueField;
use cadmpeg_ir::hash::LowerHex;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct TransferResult {
    transferred_packets: usize,
    unresolved_packets: usize,
    pub(crate) emitted_assets: usize,
    pub(crate) emitted_bindings: usize,
}

impl TransferResult {
    pub(crate) fn decoded_packets(&self) -> usize {
        self.transferred_packets + self.unresolved_packets
    }

    pub(crate) fn transferred_packets(&self) -> usize {
        self.transferred_packets
    }

    pub(crate) fn unresolved_packets(&self) -> usize {
        self.unresolved_packets
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Packet {
    AllFaces([u8; 3]),
    Body([u8; 4]),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourcedPacket<'a> {
    packet: Packet,
    source_block: &'a str,
    source_offset: usize,
    source_ordinal: usize,
}

impl SourcedPacket<'_> {
    fn rgba(&self) -> [u8; 4] {
        match self.packet {
            Packet::AllFaces([r, g, b]) => [r, g, b, 0xff],
            Packet::Body(rgba) => rgba,
        }
    }

    fn is_all_faces(&self) -> bool {
        matches!(self.packet, Packet::AllFaces(_))
    }

    fn is_body(&self) -> bool {
        matches!(self.packet, Packet::Body(_))
    }
}

/// Transfers presentation packets belonging to the selected modeling graph.
///
/// `01 R G B` assigns an opaque color to the complete face population.
/// `03 R G B A` assigns a body color when singular. A population of `03`
/// packets is positional; its authoritative face colors are the ABGR payloads
/// in the standard FBB face rows.
///
/// Every decoded packet reaches exactly one of the transferred and unresolved
/// populations. Their sum is the decoded packet count.
pub(crate) fn transfer(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    native: &CatiaNative,
    graph_scope: &crate::decode::ModelingGraphScope,
    standard_fbb: Option<&[u8]>,
) -> Result<TransferResult, cadmpeg_core::CodecError> {
    let mut asset_storage = ctx.reserve_scoped(0, "catia_appearance_identity_index")?;
    let mut assets = HashMap::<String, usize>::new();
    for (index, appearance) in ctx
        .admit_iter(
            &ir.model.appearances,
            "catia_appearance_existing_identity_visits",
        )?
        .enumerate()
    {
        asset_storage.with_storage(|| {
            let key =
                ctx.copy_retained_text(appearance.id.as_str(), "catia_appearance_identity_key")?;
            ctx.entry_hash_map(&mut assets, key, "catia_appearance_identity_index")?
                .or_insert(index);
            Ok::<_, cadmpeg_core::CodecError>(())
        })?;
    }
    let initial_assets = ir.model.appearances.len();
    let initial_bindings = ir.model.appearance_bindings.len();
    let mut packet_storage = ctx.reserve_scoped(0, "catia_appearance_packets")?;
    let mut packets = Vec::new();
    for block in ctx.admit_iter(&native.value_blocks, "catia_appearance_block_visits")? {
        let in_scope = graph_scope.is_unscoped()
            || match block.object_graph.as_deref() {
                Some(graph) => graph_scope.contains(ctx, graph)?,
                None => false,
            };
        if !in_scope {
            continue;
        }
        let (fields, _field_storage) = ctx
            .with_scoped_storage("catia_appearance_fields", || {
                crate::value_block::tokenize_charged(ctx, &block.payload)
            })?;
        for (ordinal, field) in ctx
            .admit_iter(fields, "catia_appearance_field_visits")?
            .enumerate()
        {
            let Some(packet) = packet(&field) else {
                continue;
            };
            let ValueField::Inline { offset, .. } = field else {
                continue;
            };
            packet_storage.with_storage(|| {
                ctx.push_vec(
                    &mut packets,
                    SourcedPacket {
                        packet,
                        source_block: &block.id,
                        source_offset: offset,
                        source_ordinal: ordinal,
                    },
                    "catia_appearance_packets",
                )
            })?;
        }
    }
    let mut result = TransferResult::default();

    let mut all_faces_storage = ctx.reserve_scoped(0, "catia_appearance_all_faces")?;
    let mut body_storage = ctx.reserve_scoped(0, "catia_appearance_body")?;
    let mut all_faces = Vec::new();
    let mut body = Vec::new();
    // Each decoded color remains an asset even when its target is unresolved.
    for packet in ctx.admit_iter(&packets, "catia_appearance_packet_visits")? {
        match packet.packet {
            Packet::AllFaces(_) => all_faces_storage.with_storage(|| {
                ctx.push_vec(&mut all_faces, packet.rgba(), "catia_appearance_all_faces")
            })?,
            Packet::Body(rgba) => body_storage
                .with_storage(|| ctx.push_vec(&mut body, rgba, "catia_appearance_body"))?,
        }
        insert_appearance(
            ctx,
            &mut assets,
            &mut asset_storage,
            &mut ir.model.appearances,
            packet.rgba(),
        )?;
    }
    let (positional_colors, positional_storage) = ctx.with_scoped_storage(
        "catia_appearance_positional_colors",
        || match standard_fbb {
            Some(bytes) => standard_face_colors(ctx, bytes),
            None => Ok(None),
        },
    )?;
    let positional_colors = if let Some(colors) = positional_colors {
        let compatible = if colors.len() == ir.model.faces.len() {
            match all_faces.as_slice() {
                [] => {
                    body.len() > 1
                        && ctx.equal(&colors, &body, "catia_appearance_positional_color_match")?
                }
                [base] => {
                    let (overrides, _override_storage) =
                        ctx.with_scoped_storage("catia_appearance_overrides", || {
                            let mut overrides = Vec::new();
                            for &rgba in
                                ctx.admit_iter(&colors, "catia_appearance_override_visits")?
                            {
                                if &rgba != base {
                                    ctx.push_vec(
                                        &mut overrides,
                                        rgba,
                                        "catia_appearance_overrides",
                                    )?;
                                }
                            }
                            Ok::<_, cadmpeg_core::CodecError>(overrides)
                        })?;
                    same_color_multiset(ctx, &overrides, &body)?
                }
                _ => false,
            }
        } else {
            false
        };
        compatible.then_some(colors)
    } else {
        None
    };
    if let Some(colors) = positional_colors {
        for (index, (face, rgba)) in ctx
            .admit_iter(&ir.model.faces, "catia_appearance_face_visits")?
            .zip(colors)
            .enumerate()
        {
            let asset = insert_appearance(
                ctx,
                &mut assets,
                &mut asset_storage,
                &mut ir.model.appearances,
                rgba,
            )?;
            let face = face
                .id
                .try_clone_for_decode(ctx, "catia_appearance_face_id")?;
            insert_binding(
                ctx,
                &mut ir.model.appearance_bindings,
                &ir.model.appearances[asset].id,
                AppearanceTarget::Face(face),
                index,
            )?;
        }
        drop(positional_storage);
        result.transferred_packets += body.len() + all_faces.len();
    } else {
        drop(positional_storage);
        if all_faces.len() == 1 && !ir.model.faces.is_empty() {
            let asset = insert_appearance(
                ctx,
                &mut assets,
                &mut asset_storage,
                &mut ir.model.appearances,
                all_faces[0],
            )?;
            for (index, face) in ctx
                .admit_iter(&ir.model.faces, "catia_appearance_face_visits")?
                .enumerate()
            {
                let face = face
                    .id
                    .try_clone_for_decode(ctx, "catia_appearance_face_id")?;
                insert_binding(
                    ctx,
                    &mut ir.model.appearance_bindings,
                    &ir.model.appearances[asset].id,
                    AppearanceTarget::Face(face),
                    index,
                )?;
            }
            result.transferred_packets += 1;
        } else {
            for packet in ctx.admit_iter(&packets, "catia_appearance_unresolved_packet_visits")? {
                if !packet.is_all_faces() {
                    continue;
                }
                let source_id = ctx.format_retained(
                    format_args!(
                        "{}:field#{:010}:{:06}",
                        packet.source_block, packet.source_offset, packet.source_ordinal
                    ),
                    "catia_appearance_source_id",
                )?;
                insert_source_binding(
                    ctx,
                    &mut assets,
                    &mut asset_storage,
                    ir,
                    packet.packet,
                    source_id,
                )?;
            }
            result.unresolved_packets += all_faces.len();
        }

        match body.as_slice() {
            [rgba] if all_faces.is_empty() && ir.model.bodies.len() == 1 => {
                let asset = insert_appearance(
                    ctx,
                    &mut assets,
                    &mut asset_storage,
                    &mut ir.model.appearances,
                    *rgba,
                )?;
                let target = AppearanceTarget::Body(
                    ir.model.bodies[0]
                        .id
                        .try_clone_for_decode(ctx, "catia_appearance_body_target")?,
                );
                insert_binding(
                    ctx,
                    &mut ir.model.appearance_bindings,
                    &ir.model.appearances[asset].id,
                    target,
                    0,
                )?;
                result.transferred_packets += 1;
            }
            values => {
                for packet in
                    ctx.admit_iter(&packets, "catia_appearance_unresolved_packet_visits")?
                {
                    if !packet.is_body() {
                        continue;
                    }
                    let source_id = ctx.format_retained(
                        format_args!(
                            "{}:field#{:010}:{:06}",
                            packet.source_block, packet.source_offset, packet.source_ordinal
                        ),
                        "catia_appearance_source_id",
                    )?;
                    insert_source_binding(
                        ctx,
                        &mut assets,
                        &mut asset_storage,
                        ir,
                        packet.packet,
                        source_id,
                    )?;
                }
                result.unresolved_packets += values.len();
            }
        }
    }
    result.emitted_assets = ir.model.appearances.len() - initial_assets;
    result.emitted_bindings = ir.model.appearance_bindings.len() - initial_bindings;
    Ok(result)
}

fn same_color_multiset(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    left: &[[u8; 4]],
    right: &[[u8; 4]],
) -> Result<bool, cadmpeg_core::CodecError> {
    if left.len() != right.len() {
        return Ok(false);
    }
    let ((left, right), _storage) =
        ctx.with_scoped_storage("catia_appearance_color_counts", || {
            let mut left_counts = BTreeMap::<[u8; 4], usize>::new();
            let mut right_counts = BTreeMap::<[u8; 4], usize>::new();
            for (counts, values) in [(&mut left_counts, left), (&mut right_counts, right)] {
                for value in ctx.admit_iter(values, "catia_appearance_color_visits")? {
                    *ctx.entry_btree_map(counts, *value, "catia_appearance_color_counts")?
                        .or_default() += 1usize;
                }
            }
            Ok::<_, cadmpeg_core::CodecError>((left_counts, right_counts))
        })?;
    if left.len() != right.len() {
        return Ok(false);
    }
    ctx.all_by(
        left.iter().zip(&right),
        |(left, right)| Ok(left == right),
        "catia_appearance_color_multiset_match",
    )
}

fn packet(field: &ValueField) -> Option<Packet> {
    let ValueField::Inline { bytes, .. } = field else {
        return None;
    };
    match bytes.as_slice() {
        [0x01, r, g, b] => Some(Packet::AllFaces([*r, *g, *b])),
        [0x03, r, g, b, a] => Some(Packet::Body([*r, *g, *b, *a])),
        _ => None,
    }
}

fn insert_appearance(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    assets: &mut HashMap<String, usize>,
    asset_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    appearances: &mut Vec<Appearance>,
    rgba: [u8; 4],
) -> Result<usize, cadmpeg_core::CodecError> {
    let (id, id_storage) = ctx.with_scoped_storage("catia_appearance_id", || {
        let id = ctx.format_retained(
            format_args!("catia:appearance:rgba#{}", LowerHex(&rgba)),
            "catia_appearance_id",
        )?;
        let id = AppearanceId::from(crate::resource::admit_identity(
            ctx,
            id,
            "catia_appearance_asset_identity",
            "catia_appearance_asset_identity_error",
        )?);
        Ok::<_, cadmpeg_core::CodecError>(id)
    })?;
    if let Some(index) =
        ctx.get_hash_map(assets, id.as_str(), "catia_appearance_asset_identity_match")?
    {
        return Ok(*index);
    }
    let index = appearances.len();
    asset_storage.with_storage(|| {
        let key = ctx.copy_retained_text(id.as_str(), "catia_appearance_identity_key")?;
        ctx.insert_hash_map(assets, key, index, "catia_appearance_identity_index")
    })?;
    ctx.charge_entities(1, "admit CATIA appearance")?;
    id_storage.commit()?;
    let schema = {
        let mut text = ctx.retained_string(22, "catia_appearance_schema")?;
        text.push_str("CATIA V5 display color");
        text
    };
    ctx.push_vec(
        appearances,
        Appearance {
            id,
            name: None,
            library_id: None,
            asset_guid: None,
            visual_guid: None,
            physical_token: None,
            schema: Some(schema),
            category: None,
            base_color: Some(Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3])),
            properties: BTreeMap::new(),
            textures: Vec::new(),
        },
        "catia_appearance_assets",
    )?;
    Ok(index)
}

fn insert_binding(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bindings: &mut Vec<AppearanceBinding>,
    appearance: &AppearanceId,
    target: AppearanceTarget,
    index: usize,
) -> Result<(), cadmpeg_core::CodecError> {
    let key = ctx
        .split_once(appearance.as_str(), "#", "catia_appearance_binding_key")?
        .map_or("", |(_, key)| key);
    let id = ctx.format_retained(
        format_args!("catia:appearance:binding#{index}:{key}"),
        "catia_appearance_binding_id",
    )?;
    let id = AppearanceBindingId::from(crate::resource::admit_identity(
        ctx,
        id,
        "catia_appearance_binding_identity",
        "catia_appearance_binding_identity_error",
    )?);
    insert_binding_record(ctx, bindings, appearance, target, id)
}

fn insert_source_binding(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    assets: &mut HashMap<String, usize>,
    asset_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ir: &mut CadIr,
    packet: Packet,
    source_id: String,
) -> Result<(), cadmpeg_core::CodecError> {
    let rgba = match packet {
        Packet::AllFaces([r, g, b]) => [r, g, b, 0xff],
        Packet::Body(rgba) => rgba,
    };
    let asset = insert_appearance(ctx, assets, asset_storage, &mut ir.model.appearances, rgba)?;
    let appearance = &ir.model.appearances[asset].id;
    // Hex encoding preserves the source token while excluding key delimiters.
    let key = ctx
        .split_once(
            appearance.as_str(),
            "#",
            "catia_appearance_source_binding_key",
        )?
        .map_or("", |(_, key)| key);
    let id = ctx.format_retained(
        format_args!(
            "catia:appearance:source-binding#source-{}:{key}",
            LowerHex(source_id.as_bytes())
        ),
        "catia_appearance_source_binding_id",
    )?;
    let id = AppearanceBindingId::from(crate::resource::admit_identity(
        ctx,
        id,
        "catia_appearance_source_binding_identity",
        "catia_appearance_source_binding_identity_error",
    )?);
    insert_binding_record(
        ctx,
        &mut ir.model.appearance_bindings,
        appearance,
        AppearanceTarget::Source { source_id },
        id,
    )
}

fn insert_binding_record(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bindings: &mut Vec<AppearanceBinding>,
    appearance: &AppearanceId,
    target: AppearanceTarget,
    id: AppearanceBindingId,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.charge_entities(1, "admit CATIA appearance binding")?;
    let retained_appearance =
        appearance.try_clone_for_decode(ctx, "catia_appearance_binding_asset_id")?;
    let object_type = {
        let mut text = ctx.retained_string(25, "catia_appearance_object_type")?;
        text.push_str("CATIA V5 display property");
        text
    };
    ctx.push_vec(
        bindings,
        AppearanceBinding {
            id,
            target,
            appearance: retained_appearance,
            source_entity_id: None,
            object_type: Some(object_type),
            visible: None,
            channels: BTreeMap::new(),
        },
        "catia_appearance_bindings",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{packet, transfer, Packet, TransferResult};
    use crate::native::CatiaNative;
    use crate::native::CatiaValueBlock;
    use crate::value_block::ValueField;
    use cadmpeg_ir::appearance::AppearanceTarget;
    use cadmpeg_ir::ids::{BodyId, FaceId, ShellId, SurfaceId};
    use cadmpeg_ir::topology::{Body, BodyKind, Face, Sense};
    use cadmpeg_ir::CadIr;

    fn assert_identity_grammar_refusal(
        operation: &'static str,
        mut construct: impl FnMut(
            &cadmpeg_core::decode::DecodeContext<'_>,
        ) -> Result<(), cadmpeg_core::CodecError>,
    ) {
        let refused = crate::test_support::with_work_refusal(operation, |ctx| {
            let result = construct(ctx);
            if let Err(cadmpeg_core::CodecError::ResourceLimit(ref limit)) = result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            } else if result.is_ok() {
                assert_eq!(ctx.resource_refusal(), None);
            }
            result
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == operation)
        );
    }

    fn insert_source_binding(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        ir: &mut CadIr,
        packet: Packet,
        source_id: String,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let mut storage = ctx.reserve_scoped(0, "catia_appearance_test_index")?;
        let mut assets = std::collections::HashMap::new();
        for (index, appearance) in ir.model.appearances.iter().enumerate() {
            storage.with_storage(|| {
                let key =
                    ctx.copy_retained_text(appearance.id.as_str(), "catia_appearance_test_key")?;
                ctx.insert_hash_map(&mut assets, key, index, "catia_appearance_test_index")
            })?;
        }
        super::insert_source_binding(ctx, &mut assets, &mut storage, ir, packet, source_id)
    }

    #[test]
    fn appearance_identity_index_admits_distinct_assets_and_reuses_existing_ids() {
        let mut appearances = Vec::new();
        crate::test_support::with_work_limit(1_000_000, |ctx| {
            let mut storage = ctx.reserve_scoped(0, "catia_appearance_test_index")?;
            let mut assets = std::collections::HashMap::new();
            for index in 0u16..512 {
                let [low, high] = index.to_le_bytes();
                let rgba = [low, high, 0, 255];
                assert_eq!(
                    super::insert_appearance(
                        ctx,
                        &mut assets,
                        &mut storage,
                        &mut appearances,
                        rgba
                    )?,
                    usize::from(index)
                );
                assert_eq!(
                    super::insert_appearance(
                        ctx,
                        &mut assets,
                        &mut storage,
                        &mut appearances,
                        rgba
                    )?,
                    usize::from(index)
                );
            }
            Ok::<_, cadmpeg_core::CodecError>(())
        })
        .expect("indexed asset transfer fits a linear allowance");
        assert_eq!(appearances.len(), 512);
    }

    #[test]
    fn appearance_asset_identity_refuses_grammar_work() {
        assert_identity_grammar_refusal("catia_appearance_asset_identity", |ctx| {
            let mut ir = model(0);
            let result = {
                let mut storage = ctx.reserve_scoped(0, "catia_appearance_test_index")?;
                super::insert_appearance(
                    ctx,
                    &mut std::collections::HashMap::new(),
                    &mut storage,
                    &mut ir.model.appearances,
                    [1, 2, 3, 4],
                )
            };
            if result.is_err() {
                assert!(ir.model.appearances.is_empty());
            }
            result.map(|_| ())
        });
    }

    #[test]
    fn appearance_binding_identity_refuses_grammar_work() {
        assert_identity_grammar_refusal("catia_appearance_binding_identity", |ctx| {
            let mut ir = model(0);
            let appearance = cadmpeg_ir::ids::AppearanceId::mint("catia:appearance:rgba#01020304")
                .expect("fixture identity");
            let result = super::insert_binding(
                ctx,
                &mut ir.model.appearance_bindings,
                &appearance,
                AppearanceTarget::Source {
                    source_id: "catia:test:source#0".into(),
                },
                0,
            );
            if result.is_err() {
                assert!(ir.model.appearance_bindings.is_empty());
            }
            result
        });
    }

    #[test]
    fn appearance_source_binding_identity_refuses_grammar_work() {
        assert_identity_grammar_refusal("catia_appearance_source_binding_identity", |ctx| {
            let mut ir = model(0);
            let packet = Packet::Body([1, 2, 3, 4]);
            let source_id = "catia:test:source#nested:field#0".into();
            let result = insert_source_binding(ctx, &mut ir, packet, source_id);
            if result.is_err() {
                assert!(ir.model.appearance_bindings.is_empty());
            }
            result
        });
    }

    #[test]
    fn appearance_entity_limit_refuses_before_asset() {
        let mut ir = model(0);
        let native = native(vec![inline(&[0x01, 1, 2, 3])]);
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits service input limit");
        let error = transfer(
            &ctx,
            &mut ir,
            &native,
            &crate::decode::ModelingGraphScope::Unscoped,
            None,
        )
        .expect_err("one appearance exceeds zero entities");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
                && limit.operation == "admit CATIA appearance")
        );
        assert!(ir.model.appearances.is_empty());
    }

    #[test]
    fn appearance_binding_entity_limit_refuses_before_binding() {
        let mut ir = model(0);
        let native = native(vec![inline(&[0x01, 1, 2, 3])]);
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_entities = 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits service input limit");
        let error = transfer(
            &ctx,
            &mut ir,
            &native,
            &crate::decode::ModelingGraphScope::Unscoped,
            None,
        )
        .expect_err("asset and binding need two entities");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
                && limit.operation == "admit CATIA appearance binding")
        );
        assert_eq!(ir.model.appearances.len(), 1);
        assert!(ir.model.appearance_bindings.is_empty());
    }

    #[test]
    fn appearance_packet_views_refuse_collection_and_materialized_limits() {
        let input = native(vec![inline(&[0x01, 1, 2, 3])]);
        let limited_collection = crate::test_support::with_collection_limit(0, |ctx| {
            transfer(
                ctx,
                &mut model(0),
                &input,
                &crate::decode::ModelingGraphScope::Unscoped,
                None,
            )
        });
        assert!(matches!(
            limited_collection,
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_value_field_bytes"
                    && limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
        ));
        let limited_retained = crate::test_support::with_materialized_limit(0, |ctx| {
            transfer(
                ctx,
                &mut model(0),
                &input,
                &crate::decode::ModelingGraphScope::Unscoped,
                None,
            )
        });
        assert!(matches!(
            limited_retained,
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_value_field_bytes"
                    && limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
        ));
        let admitted = crate::test_support::with_service_context(|ctx| {
            transfer(
                ctx,
                &mut model(0),
                &input,
                &crate::decode::ModelingGraphScope::Unscoped,
                None,
            )
        })
        .expect("service profile admits one appearance packet");
        assert_eq!(admitted.decoded_packets(), 1);
    }

    #[test]
    fn appearance_emission_refuses_each_collection_boundary() {
        let input = native(vec![inline(&[0x01, 1, 2, 3])]);
        for operation in [
            "catia_value_fields",
            "catia_appearance_packets",
            "catia_appearance_all_faces",
            "catia_appearance_identity_index",
            "catia_appearance_assets",
            "catia_appearance_bindings",
        ] {
            let refusal = cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems,
                operation,
                |limit| {
                    crate::test_support::with_collection_limit(limit, |ctx| {
                        transfer(
                            ctx,
                            &mut model(0),
                            &input,
                            &crate::decode::ModelingGraphScope::Unscoped,
                            None,
                        )
                    })
                },
            );
            assert!(
                matches!(
                    refusal,
                    cadmpeg_core::CodecError::ResourceLimit(resource)
                        if resource.operation == operation
                            && resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                ),
                "operation {operation}"
            );
        }
        let retained_refusal =
            crate::test_support::with_retained_refusal(&[], "catia_appearance_source_id", |ctx| {
                transfer(
                    ctx,
                    &mut model(0),
                    &input,
                    &crate::decode::ModelingGraphScope::Unscoped,
                    None,
                )
            });
        assert!(matches!(
            retained_refusal,
            Err(cadmpeg_core::CodecError::ResourceLimit(resource))
                if resource.operation == "catia_appearance_source_id"
                    && resource.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
        ));
    }

    fn model(face_count: usize) -> CadIr {
        let mut ir = CadIr::empty();
        ir.model.bodies.push(Body {
            id: BodyId::mint("catia:test:body#body").expect("identity grammar"),
            kind: BodyKind::Solid,
            regions: vec![],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        for index in 0..face_count {
            ir.model.faces.push(Face {
                id: FaceId::mint(format!("catia:test:face#face-{index}"))
                    .expect("identity grammar"),
                shell: ShellId::mint("catia:test:shell#shell").expect("identity grammar"),
                surface: SurfaceId::mint(format!("catia:test:surface#surface-{index}"))
                    .expect("identity grammar"),
                sense: Sense::Forward,
                loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
                name: None,
                color: None,
                tolerance: None,
            });
        }
        ir
    }

    // This conversion consumes the input carrier at the typed construction boundary.
    #[allow(clippy::needless_pass_by_value)]
    fn native(fields: Vec<ValueField>) -> CatiaNative {
        let mut native = CatiaNative::default();
        let payload = fields
            .iter()
            .flat_map(|field| {
                let ValueField::Inline { bytes, .. } = field else {
                    panic!("appearance fixture requires inline fields");
                };
                [0x8e, bytes.code().expect("validated inline bytes"), 0x84]
                    .into_iter()
                    .chain(bytes.as_slice().iter().copied())
            })
            .collect::<Vec<_>>();
        native.value_blocks.push(CatiaValueBlock {
            id: "values".into(),
            byte_offset: 0,
            object_graph: None,
            catalog: "catalog".into(),
            payload,
            schema_selections: vec![],
        });
        native
    }

    fn inline(bytes: &[u8]) -> ValueField {
        ValueField::Inline {
            bytes: bytes.to_vec().try_into().expect("inline byte count"),
            offset: 0,
        }
    }

    fn six_face_brep(rgba: [u8; 4]) -> Vec<u8> {
        (0..6)
            .flat_map(|_| [0xb0, 4, 4, 0xff, rgba[3], rgba[2], rgba[1], rgba[0]])
            .collect()
    }

    #[test]
    fn accepts_only_exact_display_packets() {
        let inline = |bytes: Vec<u8>| ValueField::Inline {
            bytes: bytes.try_into().expect("inline byte count"),
            offset: 0,
        };
        assert_eq!(
            packet(&inline(vec![1, 0xd1, 0x1a, 0x1f])),
            Some(Packet::AllFaces([0xd1, 0x1a, 0x1f]))
        );
        assert_eq!(
            packet(&inline(vec![3, 0xd1, 0x1a, 0x1f, 0x99])),
            Some(Packet::Body([0xd1, 0x1a, 0x1f, 0x99]))
        );
        assert_eq!(packet(&inline(vec![2, 0xd1, 0x1a, 0x1f])), None);
        assert_eq!(packet(&inline(vec![1, 0xd1, 0x1a, 0x1f, 0x99])), None);
        assert_eq!(packet(&inline(vec![3, 0xd1, 0x1a, 0x1f])), None);
        assert_eq!(
            packet(&ValueField::Marker {
                code: 0xe7,
                offset: 0
            }),
            None
        );
    }

    #[test]
    fn every_packet_is_accounted_for_across_target_populations() {
        for all_face_count in 0..=3 {
            for body_packet_count in 0..=3 {
                let fields = std::iter::repeat_with(|| inline(&[1, 0x10, 0x20, 0x30]))
                    .take(all_face_count)
                    .chain(
                        std::iter::repeat_with(|| inline(&[3, 0x40, 0x50, 0x60, 0xff]))
                            .take(body_packet_count),
                    )
                    .collect::<Vec<_>>();
                let native = native(fields);
                for face_count in [0, 2] {
                    for body_count in 0..=2 {
                        let mut ir = model(face_count);
                        let body = ir.model.bodies[0].clone();
                        ir.model.bodies = (0..body_count)
                            .map(|index| Body {
                                id: BodyId::compose(
                                    &cadmpeg_ir::identity_namespace!("catia", "test", "body"),
                                    index,
                                ),
                                ..body.clone()
                            })
                            .collect();
                        let result = crate::test_support::with_service_context(|ctx| {
                            transfer(
                                ctx,
                                &mut ir,
                                &native,
                                &crate::decode::ModelingGraphScope::Unscoped,
                                None,
                            )
                        })
                        .expect("service profile admits appearance transfer");
                        assert_eq!(
                            result.decoded_packets(),
                            all_face_count + body_packet_count,
                            "{all_face_count} all-face packets, {body_packet_count} body packets, \
                             {face_count} faces, {body_count} bodies",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn transfers_unstyled_body_and_all_faces_without_inventing_targets() {
        let mut ir = model(6);
        assert_eq!(
            crate::test_support::with_service_context(|ctx| transfer(
                ctx,
                &mut ir,
                &CatiaNative::default(),
                &crate::decode::ModelingGraphScope::Unscoped,
                None
            ))
            .expect("service profile admits appearance transfer"),
            TransferResult::default()
        );
        assert!(ir.model.appearances.is_empty());

        let mut ir = model(6);
        let result = crate::test_support::with_service_context(|ctx| {
            transfer(
                ctx,
                &mut ir,
                &native(vec![inline(&[3, 0xd1, 0x1a, 0x1f, 0xff])]),
                &crate::decode::ModelingGraphScope::Unscoped,
                None,
            )
        })
        .expect("service profile admits appearance transfer");
        assert_eq!(
            (
                result.decoded_packets(),
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_assets,
                result.emitted_bindings
            ),
            (1, 1, 0, 1, 1)
        );
        assert!(matches!(
            ir.model.appearance_bindings[0].target,
            AppearanceTarget::Body(_)
        ));
        assert_eq!(
            ir.model.appearance_bindings[0].id.as_str(),
            "catia:appearance:binding#0:d11a1fff"
        );

        let mut ir = model(6);
        let result = crate::test_support::with_service_context(|ctx| {
            transfer(
                ctx,
                &mut ir,
                &native(vec![inline(&[1, 0xd1, 0x1a, 0x1f])]),
                &crate::decode::ModelingGraphScope::Unscoped,
                None,
            )
        })
        .expect("service profile admits appearance transfer");
        assert_eq!(
            (
                result.decoded_packets(),
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_assets,
                result.emitted_bindings
            ),
            (1, 1, 0, 1, 6)
        );
        assert!(ir
            .model
            .appearance_bindings
            .iter()
            .all(|binding| matches!(binding.target, AppearanceTarget::Face(_))));
    }

    #[test]
    fn retains_unbound_override_asset_and_reports_its_packet() {
        let mut ir = model(6);
        let result = crate::test_support::with_service_context(|ctx| {
            transfer(
                ctx,
                &mut ir,
                &native(vec![
                    inline(&[1, 0xd1, 0x1a, 0x1f]),
                    inline(&[3, 0x14, 0x3d, 0xe0, 0xff]),
                ]),
                &crate::decode::ModelingGraphScope::Unscoped,
                None,
            )
        })
        .expect("service profile admits appearance transfer");
        assert_eq!(
            (
                result.decoded_packets(),
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_assets,
                result.emitted_bindings
            ),
            (2, 1, 1, 2, 7)
        );
        assert!(ir
            .model
            .appearances
            .iter()
            .any(|asset| asset.id.as_str().contains("143de0ff")));
        assert_eq!(
            ir.model
                .appearance_bindings
                .iter()
                .filter(|binding| matches!(binding.target, AppearanceTarget::Source { .. }))
                .count(),
            1
        );
        assert!(ir.model.appearance_bindings.iter().any(|binding| {
            binding.appearance.as_str().contains("143de0ff")
                && matches!(
                    &binding.target,
                    AppearanceTarget::Source { source_id }
                        if source_id == "values:field#0000000007:000001"
                )
        }));
        assert!(ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
            .all(|binding| binding.appearance.as_str().contains("d11a1fff")));
    }

    #[test]
    fn source_binding_identity_encodes_nested_source_delimiters() {
        let packet = Packet::Body([0x14, 0x3d, 0xe0, 0xff]);
        let source_id = "catia:outer:value-block#0001:field#0002".into();
        let mut ir = model(0);
        crate::test_support::with_service_context(|ctx| {
            insert_source_binding(ctx, &mut ir, packet, source_id)
        })
        .expect("service profile admits appearance transfer");
        let binding = ir
            .model
            .appearance_bindings
            .first()
            .expect("source binding");
        assert_eq!(binding.id.as_str().matches('#').count(), 1);
        assert!(binding
            .id
            .as_str()
            .starts_with("catia:appearance:source-binding#source-"));
        assert!(matches!(
            &binding.target,
            AppearanceTarget::Source { source_id } if source_id == "catia:outer:value-block#0001:field#0002"
        ));
    }

    #[test]
    fn positional_transparency_requires_matching_fbb_values() {
        let rgba = [0xd1, 0x1a, 0x1f, 0x99];
        let fields = (0..6)
            .map(|_| inline(&[3, rgba[0], rgba[1], rgba[2], rgba[3]]))
            .collect::<Vec<_>>();
        let mut ir = model(6);
        let result = crate::test_support::with_service_context(|ctx| {
            transfer(
                ctx,
                &mut ir,
                &native(fields.clone()),
                &crate::decode::ModelingGraphScope::Unscoped,
                Some(&six_face_brep(rgba)),
            )
        })
        .expect("service profile admits appearance transfer");
        assert_eq!(
            (
                result.decoded_packets(),
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_assets,
                result.emitted_bindings
            ),
            (6, 6, 0, 1, 6)
        );
        let ids = ir
            .model
            .appearance_bindings
            .iter()
            .map(|binding| &binding.id)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), 6);
        assert!(ir.model.appearances[0]
            .base_color
            .is_some_and(|color| color.a() == 0.6));

        let mut ir = model(6);
        let result = crate::test_support::with_service_context(|ctx| {
            transfer(
                ctx,
                &mut ir,
                &native(fields),
                &crate::decode::ModelingGraphScope::Unscoped,
                Some(&six_face_brep([0x14, 0x3d, 0xe0, 0xff])),
            )
        })
        .expect("service profile admits appearance transfer");
        assert_eq!(
            (
                result.decoded_packets(),
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_bindings
            ),
            (6, 0, 6, 6)
        );
        assert!(ir
            .model
            .appearance_bindings
            .iter()
            .all(|binding| matches!(binding.target, AppearanceTarget::Source { .. })));
    }

    #[test]
    fn positional_colors_require_standard_face_population_provenance() {
        let rgba = [0xd1, 0x1a, 0x1f, 0x99];
        let fields = (0..6)
            .map(|_| inline(&[3, rgba[0], rgba[1], rgba[2], rgba[3]]))
            .collect::<Vec<_>>();
        let mut ir = model(6);
        let result = crate::test_support::with_service_context(|ctx| {
            transfer(
                ctx,
                &mut ir,
                &native(fields),
                &crate::decode::ModelingGraphScope::Unscoped,
                None,
            )
        })
        .expect("service profile admits appearance transfer");
        assert_eq!(
            (
                result.decoded_packets(),
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_bindings,
            ),
            (6, 0, 6, 6)
        );
        assert!(ir
            .model
            .appearance_bindings
            .iter()
            .all(|binding| matches!(binding.target, AppearanceTarget::Source { .. })));
    }

    #[test]
    fn positional_population_supersedes_but_still_accounts_for_all_faces_packet() {
        let rgba = [0xd1, 0x1a, 0x1f, 0x99];
        let mut fields = vec![inline(&[1, 0xd1, 0x1a, 0x1f])];
        fields.extend((0..6).map(|_| inline(&[3, rgba[0], rgba[1], rgba[2], rgba[3]])));
        let mut ir = model(6);
        let result = crate::test_support::with_service_context(|ctx| {
            transfer(
                ctx,
                &mut ir,
                &native(fields.clone()),
                &crate::decode::ModelingGraphScope::Unscoped,
                Some(&six_face_brep(rgba)),
            )
        })
        .expect("service profile admits appearance transfer");
        assert_eq!(
            (
                result.decoded_packets(),
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_assets,
                result.emitted_bindings
            ),
            (7, 7, 0, 2, 6)
        );
        assert!(ir
            .model
            .appearance_bindings
            .iter()
            .all(|binding| binding.appearance.as_str().contains("d11a1f99")));

        let mut ir = model(6);
        let result = crate::test_support::with_service_context(|ctx| {
            transfer(
                ctx,
                &mut ir,
                &native(fields),
                &crate::decode::ModelingGraphScope::Unscoped,
                Some(&six_face_brep([0x14, 0x3d, 0xe0, 0xff])),
            )
        })
        .expect("service profile admits appearance transfer");
        assert_eq!(
            (
                result.decoded_packets(),
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_bindings
            ),
            (7, 1, 6, 12)
        );
        assert_eq!(
            ir.model
                .appearance_bindings
                .iter()
                .filter(|binding| matches!(binding.target, AppearanceTarget::Source { .. }))
                .count(),
            6
        );
        assert!(ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
            .all(|binding| binding.appearance.as_str().contains("d11a1fff")));
    }

    #[test]
    fn base_plus_override_population_uses_effective_fbb_face_colors() {
        let gray = [0x8c, 0x8c, 0x8c, 0xff];
        let blue = [0x0d, 0x26, 0xe6, 0xff];
        for target in 0..6 {
            let mut colors = [gray; 6];
            colors[target] = blue;
            let brep = colors
                .into_iter()
                .flat_map(|rgba| [0xb0, 4, 4, 0xff, rgba[3], rgba[2], rgba[1], rgba[0]])
                .collect::<Vec<_>>();
            let mut ir = model(6);
            let result = crate::test_support::with_service_context(|ctx| {
                transfer(
                    ctx,
                    &mut ir,
                    &native(vec![
                        inline(&[1, gray[0], gray[1], gray[2]]),
                        inline(&[3, blue[0], blue[1], blue[2], blue[3]]),
                    ]),
                    &crate::decode::ModelingGraphScope::Unscoped,
                    Some(&brep),
                )
            })
            .expect("service profile admits appearance transfer");
            assert_eq!(
                (
                    result.decoded_packets(),
                    result.transferred_packets,
                    result.unresolved_packets,
                    result.emitted_assets,
                    result.emitted_bindings
                ),
                (2, 2, 0, 2, 6)
            );
            for (index, binding) in ir.model.appearance_bindings.iter().enumerate() {
                let key = if index == target {
                    "0d26e6ff"
                } else {
                    "8c8c8cff"
                };
                assert!(binding.appearance.as_str().contains(key));
            }
        }
    }

    #[test]
    fn base_plus_distinct_overrides_requires_exact_fbb_multiset() {
        let colors = [
            [0xe6, 0x0d, 0x0d, 0xff],
            [0x0d, 0xcc, 0x1a, 0xff],
            [0x0d, 0x26, 0xe6, 0xff],
            [0xf2, 0xcc, 0x0d, 0xff],
            [0xd9, 0x0d, 0xbf, 0xff],
            [0x0d, 0xcc, 0xd9, 0xff],
        ];
        let fields = std::iter::once(inline(&[1, colors[0][0], colors[0][1], colors[0][2]]))
            .chain(
                colors[1..]
                    .iter()
                    .map(|rgba| inline(&[3, rgba[0], rgba[1], rgba[2], rgba[3]])),
            )
            .collect::<Vec<_>>();
        let brep = colors
            .into_iter()
            .flat_map(|rgba| [0xb0, 4, 4, 0xff, rgba[3], rgba[2], rgba[1], rgba[0]])
            .collect::<Vec<_>>();
        let mut ir = model(6);
        let result = crate::test_support::with_service_context(|ctx| {
            transfer(
                ctx,
                &mut ir,
                &native(fields.clone()),
                &crate::decode::ModelingGraphScope::Unscoped,
                Some(&brep),
            )
        })
        .expect("service profile admits appearance transfer");
        assert_eq!(
            (
                result.decoded_packets(),
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_bindings
            ),
            (6, 6, 0, 6)
        );
        for (binding, rgba) in ir.model.appearance_bindings.iter().zip(colors) {
            assert!(binding.appearance.as_str().contains(&format!(
                "{:02x}{:02x}{:02x}{:02x}",
                rgba[0], rgba[1], rgba[2], rgba[3]
            )));
        }

        let mut mismatched = brep;
        mismatched[7] = 0xff;
        let mut ir = model(6);
        let result = crate::test_support::with_service_context(|ctx| {
            transfer(
                ctx,
                &mut ir,
                &native(fields),
                &crate::decode::ModelingGraphScope::Unscoped,
                Some(&mismatched),
            )
        })
        .expect("service profile admits appearance transfer");
        assert_eq!(
            (
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_bindings
            ),
            (1, 5, 11)
        );
        assert!(ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
            .all(|binding| binding.appearance.as_str().contains("e60d0dff")));
        assert_eq!(
            ir.model
                .appearance_bindings
                .iter()
                .filter(|binding| matches!(binding.target, AppearanceTarget::Source { .. }))
                .count(),
            5
        );
    }
}
