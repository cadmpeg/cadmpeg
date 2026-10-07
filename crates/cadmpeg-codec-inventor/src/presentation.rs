// SPDX-License-Identifier: Apache-2.0
//! Typed `PmApp` document-default and rendering-style records.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::ids::{AppearanceBindingId, AppearanceId, BodyId, FaceId, IdentityKey};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::Color;

use crate::assembly::count_unresolved;
use crate::pmdc::{PmDcPairedReferenceList, PmDcReference};
use crate::record_identity::Located;
use crate::record_issue::{RecordIssue, RecordIssueFamily};
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};

const DEFAULT_STYLE_TYPE: [u8; 16] = [
    0xcd, 0xec, 0xfb, 0x11, 0xd1, 0x11, 0x6b, 0x25, 0x00, 0x08, 0xeb, 0xbb, 0x21, 0xed, 0xdc, 0x09,
];
const RENDERING_STYLE_TYPE: [u8; 16] = [
    0x6f, 0xd8, 0x59, 0x67, 0xd2, 0x11, 0x38, 0x78, 0x60, 0x00, 0x94, 0xb7, 0x0b, 0x02, 0xec, 0xb0,
];
const GRAPHICS_FACE_TYPE: [u8; 16] = [
    0xa3, 0xe9, 0x94, 0x51, 0xd2, 0x11, 0x9b, 0x28, 0x60, 0x00, 0x6a, 0xb7, 0x2c, 0x39, 0xcd, 0xb0,
];
const GRAPHICS_STYLE_COLLECTION_TYPE: [u8; 16] = [
    0x07, 0x86, 0xeb, 0x48, 0xd2, 0x11, 0x0c, 0x07, 0x60, 0x00, 0xf9, 0x9a, 0xc5, 0x36, 0x1a, 0xb0,
];
const GRAPHICS_PRIMARY_COLOR_STYLE_TYPE: [u8; 16] = [
    0x0f, 0x56, 0x48, 0xaf, 0xd4, 0x11, 0xc7, 0x8d, 0x10, 0x00, 0xd5, 0x8d, 0xc0, 0x4a, 0x0a, 0xb5,
];

#[derive(Debug)]
pub(crate) struct PresentationInventory<'a> {
    pub(crate) default_styles: Vec<Located<PmAppDefaultStyle<'a>>>,
    pub(crate) rendering_styles: Vec<Located<PmAppRenderingStyle<'a>>>,
    pub(crate) graphics_faces: Vec<Located<PmGraphicsFace>>,
    pub(crate) graphics_style_collections: Vec<Located<PmGraphicsStyleCollection>>,
    pub(crate) graphics_primary_color_styles: Vec<Located<PmGraphicsPrimaryColorStyle>>,
    pub(crate) issues: Vec<RecordIssue>,
}

#[derive(Debug)]
pub(crate) struct PmGraphicsPrimaryColorStyle {
    pub(crate) segment_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) controls: [u16; 7],
    pub(crate) color_header: [u8; 2],
    pub(crate) colors: [[cadmpeg_ir::scalar::FiniteBinary32; 4]; 4],
    pub(crate) color_tail: [u16; 2],
    pub(crate) state: u8,
    pub(crate) values: [u16; 2],
    pub(crate) terminal_state: u8,
}

#[derive(Debug)]
pub(crate) struct PmGraphicsStyleCollection {
    pub(crate) segment_version_major: u8,
    pub(crate) style_references: PmDcPairedReferenceList<[u32; 2]>,
}

#[derive(Debug)]
pub(crate) struct PmGraphicsFace {
    pub(crate) segment_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) flags: u32,
    pub(crate) styles: PmDcReference,
    pub(crate) surface: PmDcReference,
    pub(crate) parent: PmDcReference,
    pub(crate) state: u32,
    pub(crate) edge_references: PmDcPairedReferenceList<[u32; 2]>,
    pub(crate) visibility_state: u8,
    pub(crate) bounds: [FiniteReal; 6],
    pub(crate) key: u32,
    pub(crate) values: [u32; 2],
}

#[derive(Debug)]
pub(crate) struct PmAppDefaultStyle<'a> {
    pub(crate) segment_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) material_reference: u32,
    pub(crate) rendering_style_reference: u32,
    pub(crate) related_references: [u32; 7],
    pub(crate) state: u8,
    pub(crate) terminal_reference: u32,
    pub(crate) suffix: View<'a>,
}

#[derive(Debug)]
pub(crate) struct PmAppRenderingStyle<'a> {
    pub(crate) segment_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) state: u8,
    pub(crate) flags: u16,
    pub(crate) values: [u16; 2],
    pub(crate) default_state: u32,
    pub(crate) value: u32,
    pub(crate) name_reference: u32,
    pub(crate) name: String,
    pub(crate) comment: String,
    pub(crate) long_name: String,
    pub(crate) extension: Option<RenderingStyleExtension>,
    pub(crate) suffix: View<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderingStyleExtension {
    pub(crate) style_state: u16,
    pub(crate) style_label: String,
    pub(crate) asset_guid: String,
    pub(crate) material_id: String,
    pub(crate) asset_library_id: String,
    pub(crate) style_values: [u16; 2],
    pub(crate) guid: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum UnresolvedCause {
    GraphicsFace,
    FaceKey,
    StyleCollection,
    ColorStyle,
    Color,
}

impl cadmpeg_core::decode::cost::DecodeCost for UnresolvedCause {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

impl UnresolvedCause {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::GraphicsFace => "graphics face key is ambiguous",
            Self::FaceKey => "model face key is ambiguous",
            Self::StyleCollection => "style collection is missing or ambiguous",
            Self::ColorStyle => "primary color style is missing or ambiguous",
            Self::Color => "primary color is invalid",
        }
    }
}

/// The graphics faces that share one face key.
enum FaceKeyMatch<'a> {
    One(&'a Located<PmGraphicsFace>),
    /// Several faces; `styled` records whether any of them names a style.
    Many {
        styled: bool,
    },
}

pub(crate) struct PresentationProjection {
    pub(crate) appearances: Vec<Appearance>,
    pub(crate) bindings: Vec<AppearanceBinding>,
    pub(crate) unresolved_defaults: usize,
    pub(crate) unresolved_face_overrides: BTreeMap<UnresolvedCause, NonZeroUsize>,
}

pub(crate) fn project_bindings(
    ctx: &DecodeContext<'_>,
    inventory: &PresentationInventory<'_>,
    appearances: &[Appearance],
    bodies: &[BodyId],
    face_keys: &BTreeMap<FaceId, u64>,
) -> Result<PresentationProjection, CodecError> {
    let mut projection = project_default_bindings(ctx, inventory, appearances, bodies)?;
    project_face_bindings(ctx, inventory, face_keys, &mut projection)?;
    Ok(projection)
}

fn project_default_bindings(
    ctx: &DecodeContext<'_>,
    inventory: &PresentationInventory<'_>,
    appearances: &[Appearance],
    bodies: &[BodyId],
) -> Result<PresentationProjection, CodecError> {
    if inventory.default_styles.len() != 1 {
        return Ok(PresentationProjection {
            appearances: Vec::new(),
            bindings: Vec::new(),
            unresolved_defaults: usize::from(!inventory.default_styles.is_empty()),
            unresolved_face_overrides: BTreeMap::new(),
        });
    }
    let mut selected_storage = ctx.reserve_scoped(0, "select Inventor default rendering styles")?;
    let mut selected = Vec::new();
    for default in ctx.admit_iter(
        &inventory.default_styles,
        "visit Inventor presentation items",
    )? {
        let Some(ordinal) = default.rendering_style_reference.checked_sub(1) else {
            continue;
        };
        let mut matches_storage =
            ctx.reserve_scoped(0, "match Inventor default rendering styles")?;
        let matches = matches_storage.with_storage(|| {
            ctx.try_collect_vec(
                ctx.admit_iter(
                    &inventory.rendering_styles,
                    "match Inventor default rendering styles",
                )?
                .enumerate()
                .map(|(index, style)| -> Result<_, CodecError> {
                    Ok((ctx.equal(
                        &style.identity.segment_token,
                        &default.identity.segment_token,
                        "match Inventor default rendering token",
                    )? && style.identity.record_ordinal == ordinal)
                        .then_some(index))
                })
                .filter_map(Result::transpose),
                "match Inventor default rendering styles",
            )
        })?;
        if matches.len() == 1 {
            selected_storage.with_storage(|| {
                ctx.push_vec(
                    &mut selected,
                    matches[0],
                    "collect Inventor presentation items",
                )
            })?;
        }
    }
    ctx.stable_sort_by_key(
        &mut selected,
        |value| {
            let style = &inventory.rendering_styles[*value];
            (&style.identity.segment_token, style.identity.record_ordinal)
        },
        Ord::cmp,
        "Inventor default rendering styles sort",
    )?;
    ctx.dedup_by(
        &mut selected,
        |left, right| {
            let left = &inventory.rendering_styles[*left];
            let right = &inventory.rendering_styles[*right];
            Ok(ctx.equal(
                &left.identity.segment_token,
                &right.identity.segment_token,
                "deduplicate Inventor default rendering tokens",
            )? && left.identity.record_ordinal == right.identity.record_ordinal)
        },
        "deduplicate Inventor default rendering styles",
    )?;
    if selected.len() != 1 {
        return Ok(PresentationProjection {
            appearances: Vec::new(),
            bindings: Vec::new(),
            unresolved_defaults: usize::from(!inventory.default_styles.is_empty()),
            unresolved_face_overrides: BTreeMap::new(),
        });
    }
    let style = &inventory.rendering_styles[selected[0]];
    let Some(asset_guid) = style
        .extension
        .as_ref()
        .map(|extension| extension.asset_guid.as_str())
        .filter(|value| !value.is_empty())
    else {
        return Ok(PresentationProjection {
            appearances: Vec::new(),
            bindings: Vec::new(),
            unresolved_defaults: 1,
            unresolved_face_overrides: BTreeMap::new(),
        });
    };
    let library_id = style
        .extension
        .as_ref()
        .map(|extension| extension.asset_library_id.as_str());
    let mut matches_storage = ctx.reserve_scoped(0, "match Inventor default appearances")?;
    let matches = matches_storage.with_storage(|| {
        ctx.try_collect_vec(
            ctx.admit_iter(appearances, "match Inventor default appearances")?
                .map(|appearance| -> Result<_, CodecError> {
                    let guid_matches = match appearance.asset_guid.as_deref() {
                        Some(value) => ctx.eq_ignore_ascii_case(
                            value,
                            asset_guid,
                            "match Inventor default appearance GUID",
                        )?,
                        None => false,
                    };
                    if !guid_matches {
                        return Ok(None);
                    }
                    let library_matches = match library_id {
                        Some(value) if !value.is_empty() => {
                            match appearance.library_id.as_deref() {
                                Some(library) => ctx.eq_ignore_ascii_case(
                                    library,
                                    value,
                                    "match Inventor default appearance library",
                                )?,
                                None => false,
                            }
                        }
                        _ => true,
                    };
                    Ok(library_matches.then_some(appearance))
                })
                .filter_map(Result::transpose),
            "match Inventor default appearance",
        )
    })?;
    if matches.len() != 1 {
        return Ok(PresentationProjection {
            appearances: Vec::new(),
            bindings: Vec::new(),
            unresolved_defaults: 1,
            unresolved_face_overrides: BTreeMap::new(),
        });
    }
    let appearance = &matches[0].id;
    let mut bindings = Vec::new();
    for body in ctx.admit_iter(bodies, "visit Inventor presentation items")? {
        ctx.charge_entities(1, "project Inventor default appearance binding")?;
        let compose_work = "inventor:presentation:body-default#"
            .len()
            .checked_add(16)
            .ok_or_else(|| {
                ctx.refuse_codec_limit("compose Inventor default binding key", u64::MAX, u64::MAX)
            })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(compose_work),
            "compose Inventor default binding key",
        )?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index("inventor:presentation:body-default#".len() + 16),
            "retain Inventor default binding id",
        )?;
        let binding_id = {
            let mut digest_storage =
                ctx.reserve_scoped(0, "compose Inventor default binding key")?;
            let key = digest_storage.with_storage(|| {
                short_digest_key(
                    ctx,
                    body.as_str().as_bytes(),
                    "compose Inventor default binding key",
                )
            })?;
            let binding_id = AppearanceBindingId::compose(
                &cadmpeg_ir::identity_namespace!("inventor", "presentation", "body-default"),
                key,
            );
            drop(digest_storage);
            binding_id
        };
        ctx.push_vec(
            &mut bindings,
            AppearanceBinding {
                id: binding_id,
                target: AppearanceTarget::Body(
                    body.try_clone_for_decode(ctx, "retain Inventor bound body id")?,
                ),
                appearance: appearance
                    .try_clone_for_decode(ctx, "retain Inventor default appearance id")?,
                source_entity_id: Some(ctx.format_retained(
                    format_args!(
                        "inventor:presentation:rendering-style#{}-{}",
                        style.identity.segment_token, style.identity.record_ordinal
                    ),
                    "retain Inventor presentation binding source id",
                )?),
                object_type: Some(
                    ctx.copy_retained_text("Body", "retain Inventor body binding object type")?,
                ),
                visible: None,
                channels: BTreeMap::default(),
            },
            "project Inventor default appearance binding",
        )?;
    }
    Ok(PresentationProjection {
        appearances: Vec::new(),
        bindings,
        unresolved_defaults: 0,
        unresolved_face_overrides: BTreeMap::new(),
    })
}

fn project_face_bindings(
    ctx: &DecodeContext<'_>,
    inventory: &PresentationInventory<'_>,
    face_keys: &BTreeMap<FaceId, u64>,
    projection: &mut PresentationProjection,
) -> Result<(), CodecError> {
    let mut key_counts_storage = ctx.reserve_scoped(0, "count Inventor presentation face keys")?;
    let mut key_counts = std::collections::HashMap::new();
    for key in ctx
        .admit_iter(face_keys, "visit Inventor presentation values")?
        .map(|(_, value)| value)
    {
        key_counts_storage.with_storage(|| {
            ctx.admit_hash_map_entry(
                &mut key_counts,
                key,
                "count Inventor presentation face keys",
            )?;
            *key_counts.entry(*key).or_insert(0_usize) += 1;
            Ok::<_, CodecError>(())
        })?;
    }
    // Each record set is indexed once, so every model face resolves its
    // graphics face, style collection and colour style by lookup.
    let mut index_storage = ctx.reserve_scoped(0, "index Inventor presentation records")?;
    let mut faces_by_key = std::collections::HashMap::<u32, FaceKeyMatch<'_>>::new();
    for face in ctx.admit_iter(
        &inventory.graphics_faces,
        "index Inventor graphics faces by key",
    )? {
        let styled = face.styles.index() != 0;
        match ctx.get_mut_hash_map(
            &mut faces_by_key,
            &face.key,
            "index Inventor graphics faces by key",
        )? {
            Some(entry) => {
                let earlier_styled = match entry {
                    FaceKeyMatch::One(first) => first.styles.index() != 0,
                    FaceKeyMatch::Many { styled } => *styled,
                };
                *entry = FaceKeyMatch::Many {
                    styled: earlier_styled || styled,
                };
            }
            None => {
                index_storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut faces_by_key,
                        face.key,
                        FaceKeyMatch::One(face),
                        "index Inventor graphics faces by key",
                    )
                })?;
            }
        }
    }
    let (collections_by_identity, _collections_storage) = ctx.unique_index(
        inventory
            .graphics_style_collections
            .iter()
            .map(|collection| {
                (
                    (
                        collection.identity.segment_token.as_str(),
                        collection.identity.record_ordinal,
                    ),
                    collection,
                )
            }),
        "index Inventor graphics style collections",
    )?;
    let (color_styles_by_identity, _color_styles_storage) = ctx.unique_index(
        inventory.graphics_primary_color_styles.iter().map(|style| {
            (
                (
                    style.identity.segment_token.as_str(),
                    style.identity.record_ordinal,
                ),
                style,
            )
        }),
        "index Inventor primary color styles",
    )?;
    // Each colour style's appearance is projected once; later faces bound to
    // the same style copy its id from the projected appearance.
    let mut appearance_storage = ctx.reserve_scoped(0, "index Inventor face appearances")?;
    let mut appearance_indices = std::collections::HashMap::<(&str, u32), usize>::new();
    for (face_id, key) in ctx.admit_iter(face_keys, "visit Inventor ordered faces")? {
        let Ok(graphics_key) = u32::try_from(*key) else {
            continue;
        };
        let graphics_face =
            match ctx.get_hash_map(&faces_by_key, &graphics_key, "match Inventor graphics face")? {
                None => continue,
                Some(FaceKeyMatch::Many { styled }) => {
                    if *styled {
                        count_unresolved(
                            ctx,
                            &mut projection.unresolved_face_overrides,
                            UnresolvedCause::GraphicsFace,
                        )?;
                    }
                    continue;
                }
                Some(FaceKeyMatch::One(face)) => *face,
            };
        let Some(collection_ordinal) = graphics_face.styles.index().checked_sub(1) else {
            continue;
        };
        if !ctx.equal(
            &ctx.get_hash_map(&key_counts, key, "count Inventor presentation face keys")?,
            &Some(&1),
            "match Inventor unique face key",
        )? {
            count_unresolved(
                ctx,
                &mut projection.unresolved_face_overrides,
                UnresolvedCause::FaceKey,
            )?;
            continue;
        }
        let Some(&Some(collection)) = ctx.get_hash_map(
            &collections_by_identity,
            &(
                graphics_face.identity.segment_token.as_str(),
                collection_ordinal,
            ),
            "match Inventor graphics style collection",
        )?
        else {
            count_unresolved(
                ctx,
                &mut projection.unresolved_face_overrides,
                UnresolvedCause::StyleCollection,
            )?;
            continue;
        };
        // The collection must name exactly one colour style in all: `named`
        // is the one style named so far, or `Some(None)` once a second is.
        let mut named = None;
        for ordinal in ctx
            .admit_iter(
                collection.style_references.references(),
                "visit Inventor presentation items",
            )?
            .filter_map(|reference| reference.index().checked_sub(1))
        {
            match ctx.get_hash_map(
                &color_styles_by_identity,
                &(collection.identity.segment_token.as_str(), ordinal),
                "match Inventor primary color style",
            )? {
                None => {}
                Some(Some(style)) if named.is_none() => named = Some(Some(*style)),
                Some(_) => named = Some(None),
            }
        }
        let Some(Some(style)) = named else {
            count_unresolved(
                ctx,
                &mut projection.unresolved_face_overrides,
                UnresolvedCause::ColorStyle,
            )?;
            continue;
        };
        let [r, g, b, a] = style.colors[1].map(cadmpeg_ir::scalar::FiniteBinary32::get);
        let Some(color) = Color::new(r, g, b, a) else {
            count_unresolved(
                ctx,
                &mut projection.unresolved_face_overrides,
                UnresolvedCause::Color,
            )?;
            continue;
        };
        let appearance_key = (
            style.identity.segment_token.as_str(),
            style.identity.record_ordinal,
        );
        let appearance_id = if let Some(&index) = ctx.get_hash_map(
            &appearance_indices,
            &appearance_key,
            "access Inventor presentation records",
        )? {
            projection
                .appearances
                .get(index)
                .ok_or_else(|| {
                    CodecError::Malformed("Inventor face appearance index is stale".into())
                })?
                .id
                .try_clone_for_decode(ctx, "copy Inventor face appearance id")?
        } else {
            ctx.charge_entities(1, "project Inventor face appearance")?;
            let digits =
                usize::try_from(style.identity.record_ordinal.max(1).ilog10()).map_err(|_| {
                    CodecError::Malformed("Inventor numeric value exceeds target range".into())
                })?;
            let key_len = style
                .identity
                .segment_token
                .as_str()
                .len()
                .checked_add(digits)
                .and_then(|len| len.checked_add(2))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "compose Inventor face appearance key",
                        u64::MAX,
                        u64::MAX,
                    )
                })?;
            let id_len = "inventor:presentation:face-color#"
                .len()
                .checked_add(key_len)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "compose Inventor face appearance key",
                        u64::MAX,
                        u64::MAX,
                    )
                })?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(id_len),
                "compose Inventor face appearance key",
            )?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(id_len),
                "retain Inventor face appearance ids",
            )?;
            let id = {
                let mut copied_storage =
                    ctx.reserve_scoped(0, "compose Inventor face appearance key")?;
                copied_storage.with_storage(|| {
                    Ok::<_, CodecError>(AppearanceId::compose(
                        &cadmpeg_ir::identity_namespace!("inventor", "presentation", "face-color"),
                        style.identity.key(ctx)?,
                    ))
                })
            }?;
            ctx.push_vec(
                &mut projection.appearances,
                Appearance {
                    id: id.try_clone_for_decode(ctx, "retain Inventor face appearance ids")?,
                    name: None,
                    asset_guid: None,
                    library_id: None,
                    visual_guid: None,
                    physical_token: None,
                    schema: Some(ctx.copy_retained_text(
                        "InventorPrimaryColorStyle",
                        "retain Inventor face appearance schema",
                    )?),
                    category: None,
                    base_color: Some(color),
                    properties: BTreeMap::new(),
                    textures: Vec::new(),
                },
                "project Inventor face appearance",
            )?;
            let index = projection.appearances.len() - 1;
            appearance_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut appearance_indices,
                    appearance_key,
                    index,
                    "access Inventor presentation records",
                )
            })?;
            id
        };

        ctx.charge_entities(1, "project Inventor face appearance binding")?;
        let compose_work = "inventor:presentation:face-override#"
            .len()
            .checked_add(16)
            .ok_or_else(|| {
                ctx.refuse_codec_limit("compose Inventor face binding key", u64::MAX, u64::MAX)
            })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(compose_work),
            "compose Inventor face binding key",
        )?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index("inventor:presentation:face-override#".len() + 16),
            "retain Inventor face binding id",
        )?;
        let mut channels = BTreeMap::new();
        ctx.insert_btree_map(
            &mut channels,
            cadmpeg_core::nonblank_literal!("precedence"),
            ctx.copy_retained_text("face_over_body", "retain Inventor face binding precedence")?,
            "project Inventor face binding channel",
        )?;
        let binding_id = {
            let mut digest_storage = ctx.reserve_scoped(0, "compose Inventor face binding key")?;
            let key = digest_storage.with_storage(|| {
                short_digest_key(
                    ctx,
                    face_id.as_str().as_bytes(),
                    "compose Inventor face binding key",
                )
            })?;
            let binding_id = AppearanceBindingId::compose(
                &cadmpeg_ir::identity_namespace!("inventor", "presentation", "face-override"),
                key,
            );
            drop(digest_storage);
            binding_id
        };
        ctx.push_vec(
            &mut projection.bindings,
            AppearanceBinding {
                id: binding_id,
                target: AppearanceTarget::Face(
                    face_id.try_clone_for_decode(ctx, "retain Inventor bound face id")?,
                ),
                appearance: appearance_id,
                source_entity_id: Some(ctx.format_retained(
                    format_args!(
                        "inventor:presentation:graphics-face#{}-{}",
                        graphics_face.identity.segment_token, graphics_face.identity.record_ordinal
                    ),
                    "retain Inventor presentation binding source id",
                )?),
                object_type: Some(
                    ctx.copy_retained_text("Face", "retain Inventor face binding object type")?,
                ),
                visible: None,
                channels,
            },
            "project Inventor face appearance binding",
        )?;
    }
    Ok(())
}

pub(crate) fn inventory<'a>(
    ctx: &DecodeContext<'a>,
    document: &RseInventory<'a>,
) -> Result<PresentationInventory<'a>, CodecError> {
    let mut default_styles = Vec::new();
    let mut rendering_styles = Vec::new();
    let mut graphics_faces = Vec::new();
    let mut graphics_style_collections = Vec::new();
    let mut graphics_primary_color_styles = Vec::new();
    let mut issues = Vec::new();
    for segment in ctx.admit_iter(&document.segments, "visit Inventor presentation items")? {
        if !matches!(segment.kind, SegmentKind::PmApp | SegmentKind::PmGraphics) {
            continue;
        }
        let Some(version) = segment.registry.map(|join| join.version_major) else {
            continue;
        };
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        let is_graphics = matches!(segment.kind, SegmentKind::PmGraphics);
        for record in ctx.admit_iter(&table.records, "visit Inventor presentation items")? {
            let token = segment.pair.token.key();
            let ordinal = record.ordinal;
            let parsed = match record.type_id {
                DEFAULT_STYLE_TYPE => {
                    parse_default_style(ctx, record.payload, version).and_then(|value| {
                        push_presentation_record(
                            ctx,
                            &mut default_styles,
                            value,
                            record.type_id,
                            token,
                            ordinal,
                            "admit Inventor default style record",
                        )
                    })
                }
                RENDERING_STYLE_TYPE => parse_rendering_style(ctx, record.payload, version)
                    .and_then(|value| {
                        push_presentation_record(
                            ctx,
                            &mut rendering_styles,
                            value,
                            record.type_id,
                            token,
                            ordinal,
                            "admit Inventor rendering style record",
                        )
                    }),
                GRAPHICS_FACE_TYPE if is_graphics => {
                    parse_graphics_face(ctx, record.payload, version).and_then(|value| {
                        push_presentation_record(
                            ctx,
                            &mut graphics_faces,
                            value,
                            record.type_id,
                            token,
                            ordinal,
                            "admit Inventor graphics face record",
                        )
                    })
                }
                GRAPHICS_STYLE_COLLECTION_TYPE if is_graphics => {
                    parse_graphics_style_collection(ctx, record.payload, version).and_then(
                        |value| {
                            push_presentation_record(
                                ctx,
                                &mut graphics_style_collections,
                                value,
                                record.type_id,
                                token,
                                ordinal,
                                "admit Inventor graphics style collection record",
                            )
                        },
                    )
                }
                GRAPHICS_PRIMARY_COLOR_STYLE_TYPE if is_graphics => {
                    parse_graphics_primary_color_style(record.payload, version).and_then(|value| {
                        push_presentation_record(
                            ctx,
                            &mut graphics_primary_color_styles,
                            value,
                            record.type_id,
                            token,
                            ordinal,
                            "admit Inventor graphics primary color style record",
                        )
                    })
                }
                _ => continue,
            };
            if let Err(error) = parsed {
                if matches!(error, CodecError::ResourceLimit(_)) {
                    return Err(error);
                }

                ctx.charge_entities(1, "admit Inventor presentation issue")?;
                let detail =
                    crate::issue_detail(ctx, error, "retain Inventor presentation issue detail")?;
                ctx.push_vec(
                    &mut issues,
                    RecordIssue {
                        family: RecordIssueFamily::Presentation,
                        segment_token: segment.pair.token.key().try_clone_for_decode(
                            ctx,
                            "retain Inventor presentation issue token",
                        )?,
                        record_ordinal: record.ordinal,
                        detail,
                    },
                    "admit Inventor presentation issue",
                )?;
            }
        }
    }
    Ok(PresentationInventory {
        default_styles,
        rendering_styles,
        graphics_faces,
        graphics_style_collections,
        graphics_primary_color_styles,
        issues,
    })
}

fn push_presentation_record<T>(
    ctx: &DecodeContext<'_>,
    records: &mut Vec<Located<T>>,
    value: T,
    type_id: [u8; 16],
    token: &IdentityKey,
    ordinal: u32,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.push_vec(
        records,
        Located::new(
            value,
            crate::record_identity::RecordTypeId::from_bytes(
                ctx,
                type_id,
                "retain Inventor presentation record type id",
            )?,
            token.try_clone_for_decode(ctx, "retain Inventor presentation record segment token")?,
            ordinal,
        ),
        operation,
    )?;
    Ok(())
}

fn parse_graphics_primary_color_style(
    source: View<'_>,
    version: u8,
) -> Result<PmGraphicsPrimaryColorStyle, CodecError> {
    let mut cursor = Cursor::new(source);
    cursor.skip(
        legacy_block_len(version),
        "graphics primary-color legacy prefix",
    )?;
    let header_value = cursor.u32("graphics primary-color header value")?;
    cursor.skip(
        legacy_block_len(version),
        "graphics primary-color legacy header padding",
    )?;
    let mut controls = [0; 7];
    for value in &mut controls {
        *value = cursor.u16("graphics primary-color control")?;
    }
    cursor.skip(
        legacy_block_len(version),
        "graphics primary-color legacy color prefix",
    )?;
    let color_header = [
        cursor.u8("graphics primary-color header 0")?,
        cursor.u8("graphics primary-color header 1")?,
    ];
    let mut colors = [[cadmpeg_ir::scalar::FiniteBinary32::ZERO; 4]; 4];
    for color in &mut colors {
        for component in color {
            *component = cursor.finite_f32("graphics primary-color component")?;
        }
        cursor.skip(
            legacy_block_len(version),
            "graphics primary-color legacy component padding",
        )?;
    }
    let color_tail = [
        cursor.u16("graphics primary-color tail 0")?,
        cursor.u16("graphics primary-color tail 1")?,
    ];
    cursor.skip(
        legacy_block_len(version),
        "graphics primary-color legacy tail padding",
    )?;
    let state = cursor.u8("graphics primary-color state")?;
    let values = [
        cursor.u16("graphics primary-color value 0")?,
        cursor.u16("graphics primary-color value 1")?,
    ];
    cursor.skip(
        legacy_block_len(version),
        "graphics primary-color legacy value padding",
    )?;
    let terminal_state = cursor.u8("graphics primary-color terminal state")?;
    let suffix = cursor.remainder()?;
    if !suffix.window().is_empty() {
        return Err(CodecError::malformed(format_args!(
            "PmGraphics primary-color record has {} trailing bytes",
            suffix.window().len()
        )));
    }
    Ok(PmGraphicsPrimaryColorStyle {
        segment_version_major: version,
        header_value,
        controls,
        color_header,
        colors,
        color_tail,
        state,
        values,
        terminal_state,
    })
}

fn parse_graphics_style_collection(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmGraphicsStyleCollection, CodecError> {
    let mut cursor = Cursor::new(source);
    cursor.skip(
        legacy_block_len(version),
        "graphics-style collection legacy prefix",
    )?;
    let style_references = cursor.reference_list(ctx, "graphics-style collection")?;
    let suffix = cursor.remainder()?;
    if !suffix.window().is_empty() {
        return Err(CodecError::malformed(format_args!(
            "PmGraphics style-collection record has {} trailing bytes",
            suffix.window().len()
        )));
    }
    Ok(PmGraphicsStyleCollection {
        segment_version_major: version,
        style_references,
    })
}

fn parse_graphics_face(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmGraphicsFace, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("graphics-face header value")?;
    let header_id = cursor.u16("graphics-face header id")?;
    cursor.skip(
        legacy_block_len(version),
        "graphics-face legacy header padding",
    )?;
    let flags = cursor.u32("graphics-face flags")?;
    let styles = cursor.node_reference("graphics-face styles reference")?;
    let surface = cursor.node_reference("graphics-face surface reference")?;
    let parent = cursor.node_reference("graphics-face parent reference")?;
    let state = cursor.u32("graphics-face state")?;
    cursor.skip(
        legacy_block_len(version),
        "graphics-face legacy object padding",
    )?;
    let edge_references = cursor.reference_list(ctx, "graphics-face edge list")?;
    let visibility_state = cursor.u8("graphics-face visibility state")?;
    cursor.skip(
        legacy_block_len(version) * 2,
        "graphics-face legacy visibility padding",
    )?;
    let mut bounds = [FiniteReal::ZERO; 6];
    for value in &mut bounds {
        *value = cursor.finite_f64("graphics-face bound")?;
    }
    cursor.skip(
        legacy_block_len(version),
        "graphics-face legacy bounds padding",
    )?;
    let key = cursor.u32("graphics-face key")?;
    let values = [
        cursor.u32("graphics-face value 0")?,
        cursor.u32("graphics-face value 1")?,
    ];
    let suffix = cursor.remainder()?;
    if !suffix.window().is_empty() {
        return Err(CodecError::malformed(format_args!(
            "PmGraphics face record has {} trailing bytes",
            suffix.window().len()
        )));
    }
    Ok(PmGraphicsFace {
        segment_version_major: version,
        header_value,
        header_id,
        flags,
        styles,
        surface,
        parent,
        state,
        edge_references,
        visibility_state,
        bounds,
        key,
        values,
    })
}

/// Field names of the seven related references a default style states, in
/// wire order.
const RELATED_REFERENCE_FIELDS: [&str; 7] = [
    "default-style related reference 0",
    "default-style related reference 1",
    "default-style related reference 2",
    "default-style related reference 3",
    "default-style related reference 4",
    "default-style related reference 5",
    "default-style related reference 6",
];

fn parse_default_style<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    version: u8,
) -> Result<PmAppDefaultStyle<'a>, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("default-style header value")?;
    let header_id = cursor.u16("default-style header id")?;
    cursor.skip(
        legacy_block_len(version),
        "default-style legacy header padding",
    )?;
    let material_reference = cursor.reference("default-style material reference")?;
    let rendering_style_reference = cursor.reference("default-style rendering reference")?;
    let mut related_references = [0; RELATED_REFERENCE_FIELDS.len()];
    for (field, reference) in RELATED_REFERENCE_FIELDS
        .into_iter()
        .zip(related_references.iter_mut())
    {
        *reference = cursor.reference(field)?;
    }
    let state = cursor.u8("default-style state")?;
    cursor.skip(
        legacy_block_len(version),
        "default-style legacy state padding",
    )?;
    if version == 15 {
        cursor.skip(4, "default-style version-15 padding")?;
    }
    let terminal_reference = cursor.reference("default-style terminal reference")?;
    if version > 19 {
        cursor.zeroes(ctx, 8, "default-style suffix padding")?;
    }
    let suffix = cursor.remainder()?;
    if !suffix.window().is_empty() {
        return Err(CodecError::malformed(format_args!(
            "PmApp default-style record has {} trailing bytes",
            suffix.window().len()
        )));
    }
    Ok(PmAppDefaultStyle {
        segment_version_major: version,
        header_value,
        header_id,
        material_reference,
        rendering_style_reference,
        related_references,
        state,
        terminal_reference,
        suffix,
    })
}

fn parse_rendering_style<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    version: u8,
) -> Result<PmAppRenderingStyle<'a>, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("rendering-style header value")?;
    let header_id = cursor.u16("rendering-style header id")?;
    cursor.skip(
        legacy_block_len(version),
        "rendering-style legacy header padding",
    )?;
    let state = cursor.u8("rendering-style state")?;
    let flags = cursor.u16("rendering-style flags")?;
    if version > 23 {
        cursor.zeroes(ctx, 2, "rendering-style alignment padding")?;
    }
    let values = [
        cursor.u16("rendering-style value 0")?,
        cursor.u16("rendering-style value 1")?,
    ];
    let default_state = cursor.u32("rendering-style default state")?;
    let value = cursor.u32("rendering-style value")?;
    let name_reference = cursor.reference("rendering-style name reference")?;
    let name = cursor.utf16(ctx, "rendering-style name")?;
    let comment = if version < 17 {
        let value = cursor.utf16(ctx, "rendering-style comment")?;
        let _comment_state = cursor.u16("rendering-style comment state")?;
        value
    } else {
        String::new()
    };
    let long_name = cursor.utf16(ctx, "rendering-style long name")?;
    cursor.skip(
        legacy_block_len(version),
        "rendering-style legacy name padding",
    )?;
    let extension = if version > 16 {
        let style_state = cursor.u16("rendering-style style state")?;
        let style_label = cursor.utf16(ctx, "rendering-style text 0")?;
        let asset_guid = cursor.utf16(ctx, "rendering-style text 1")?;
        let material_id = cursor.utf16(ctx, "rendering-style text 2")?;
        let asset_library_id = cursor.utf16(ctx, "rendering-style text 3")?;
        let style_values = [
            cursor.u16("rendering-style style value 0")?,
            cursor.u16("rendering-style style value 1")?,
        ];
        let guid = cursor.guid(ctx, "rendering-style guid")?;
        Some(RenderingStyleExtension {
            style_state,
            style_label,
            asset_guid,
            material_id,
            asset_library_id,
            style_values,
            guid,
        })
    } else {
        None
    };
    let suffix = cursor.remainder()?;
    Ok(PmAppRenderingStyle {
        segment_version_major: version,
        header_value,
        header_id,
        state,
        flags,
        values,
        default_state,
        value,
        name_reference,
        name,
        comment,
        long_name,
        extension,
        suffix,
    })
}

const fn legacy_block_len(version: u8) -> usize {
    if version <= 14 {
        4
    } else {
        0
    }
}

struct Cursor<'a> {
    source: View<'a>,
}

impl<'a> Cursor<'a> {
    const fn new(source: View<'a>) -> Self {
        Self { source }
    }

    fn take(&mut self, len: usize, field: &'static str) -> Result<&'a [u8], CodecError> {
        crate::reader::take(&mut self.source, len, field)
    }

    fn skip(&mut self, len: usize, field: &'static str) -> Result<(), CodecError> {
        self.take(len, field).map(|_| ())
    }

    fn zeroes(
        &mut self,
        ctx: &DecodeContext<'_>,
        len: usize,
        field: &'static str,
    ) -> Result<(), CodecError> {
        if ctx.any_by(self.take(len, field)?, |byte| Ok(*byte != 0), field)? {
            return Err(CodecError::malformed(format_args!(
                "Inventor presentation {field} is not zero-filled"
            )));
        }
        Ok(())
    }

    fn u8(&mut self, field: &'static str) -> Result<u8, CodecError> {
        crate::reader::u8(&mut self.source, field)
    }

    fn u16(&mut self, field: &'static str) -> Result<u16, CodecError> {
        crate::reader::u16(&mut self.source, field)
    }

    fn u32(&mut self, field: &'static str) -> Result<u32, CodecError> {
        crate::reader::u32(&mut self.source, field)
    }

    fn finite_f64(&mut self, field: &'static str) -> Result<FiniteReal, CodecError> {
        let value = self
            .source
            .req_f64_le()
            .map_err(|error| error.during(field))?;
        FiniteReal::new(value).ok_or_else(|| {
            CodecError::malformed(format_args!("Inventor presentation {field} is not finite"))
        })
    }

    fn finite_f32(
        &mut self,
        field: &'static str,
    ) -> Result<cadmpeg_ir::scalar::FiniteBinary32, CodecError> {
        let value = self
            .source
            .req_f32_le()
            .map_err(|error| error.during(field))?;
        cadmpeg_ir::scalar::FiniteBinary32::new(value).ok_or_else(|| {
            CodecError::malformed(format_args!("Inventor presentation {field} is not finite"))
        })
    }

    fn reference(&mut self, field: &'static str) -> Result<u32, CodecError> {
        let value = self.u32(field)?;
        if value != 0 && value & 0x8000_0000 == 0 {
            return Err(CodecError::malformed(format_args!(
                "Inventor presentation {field} lacks its reference qualifier"
            )));
        }
        Ok(value & 0x7fff_ffff)
    }

    fn node_reference(&mut self, field: &'static str) -> Result<PmDcReference, CodecError> {
        crate::reader::pmdc_reference(&mut self.source, field)
    }

    fn reference_list(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &'static str,
    ) -> Result<PmDcPairedReferenceList<[u32; 2]>, CodecError> {
        let mut cursor = crate::pmdc::Cursor::new(self.source);
        let list = crate::pmdc::reference_list(ctx, &mut cursor, 2, field)?;
        self.source = cursor.into_view();
        let (_, metadata, references) = list.into_parts();
        let metadata = match metadata {
            None => None,
            Some(crate::pmdc::PmDcListMetadata::U32(values)) => Some(values),
            Some(crate::pmdc::PmDcListMetadata::U16(_)) => {
                return Err(CodecError::malformed(
                    "graphics reference metadata must be u32",
                ));
            }
        };
        PmDcPairedReferenceList::new(metadata, references).ok_or_else(|| {
            CodecError::malformed("graphics reference metadata disagrees with length")
        })
    }

    fn utf16(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &'static str,
    ) -> Result<String, CodecError> {
        let units = usize::try_from(self.u32(field)?).map_err(|_| {
            CodecError::Malformed("Inventor numeric value exceeds target range".into())
        })?;
        if units > 1_048_576 {
            if self
                .source
                .counted(cadmpeg_core::decode::u64_from_index(units), 2)
                .is_none()
            {
                return Err(CodecError::malformed(format_args!(
                    "Inventor presentation {field} UTF-16 payload is truncated"
                )));
            }
            return Err(ctx.refuse_codec_limit(
                "Inventor presentation UTF-16 code units",
                1_048_576,
                cadmpeg_core::decode::u64_from_index(units),
            ));
        }
        let byte_len = units.checked_mul(2).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "Inventor presentation {field} byte length overflows"
            ))
        })?;
        let bytes = crate::reader::take(&mut self.source, byte_len, field)?;
        let mut length = bytes.len();
        loop {
            ctx.charge_work(1, "trim Inventor PmApp UTF-16 string")?;
            if length < 2 || bytes.get(length - 2..length) != Some(&[0, 0]) {
                break;
            }
            length -= 2;
        }
        ctx.utf16le_text(
            &bytes[..length],
            length / 2,
            false,
            "retain Inventor PmApp UTF-16 string",
        )
    }

    fn guid(&mut self, ctx: &DecodeContext<'_>, field: &'static str) -> Result<String, CodecError> {
        let first = self.u32(field)?;
        let second = self.u16(field)?;
        let third = self.u16(field)?;
        let tail: [u8; 8] = self.source.array().ok_or_else(|| {
            CodecError::malformed(format_args!("truncated Inventor presentation {field}"))
        })?;
        ctx.format_retained(format_args!(
            "{first:08x}-{second:04x}-{third:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            tail[0], tail[1], tail[2], tail[3], tail[4], tail[5], tail[6], tail[7]
        ), "retain Inventor rendering-style GUID")
    }

    fn remainder(&mut self) -> Result<View<'a>, CodecError> {
        let start = self.source.position();
        let end = self.source.end();
        self.source.seek(end).ok_or_else(|| {
            CodecError::Malformed("Inventor presentation suffix range is invalid".into())
        })?;
        self.source.child(start, end).ok_or_else(|| {
            CodecError::Malformed("Inventor presentation suffix range is invalid".into())
        })
    }
}

/// The first eight digest bytes of `bytes`, as sixteen lowercase hex digits.
///
/// A hexadecimal digit is identity-key text, so the key is built from the
/// digest bytes rather than sliced back out of a rendered string.
fn short_digest_key(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<IdentityKey, CodecError> {
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(bytes.len()), operation)?;
    let digest = cadmpeg_ir::hash::sha256(bytes);
    let mut text = ctx.retained_string(16, operation)?;
    crate::pmdc::push_hex(ctx, &mut text, &digest[..8], operation)?;
    crate::record_identity::try_identity_key(ctx, text, operation, None)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::appearance::AppearanceTarget;
    use cadmpeg_ir::ids::AppearanceId;
    use cadmpeg_ir::scalar::FiniteReal;

    use super::{
        inventory, parse_default_style, parse_graphics_face, parse_graphics_primary_color_style,
        parse_graphics_style_collection, parse_rendering_style, project_bindings,
        project_default_bindings, short_digest_key, Cursor, PmGraphicsFace,
        PmGraphicsPrimaryColorStyle, PmGraphicsStyleCollection, PresentationInventory,
        DEFAULT_STYLE_TYPE, GRAPHICS_FACE_TYPE, GRAPHICS_PRIMARY_COLOR_STYLE_TYPE,
        GRAPHICS_STYLE_COLLECTION_TYPE, RENDERING_STYLE_TYPE,
    };
    use crate::container::InventorContainer;
    use crate::pmdc::{PmDcPairedReferenceList, PmDcReference};
    use crate::record_identity::Located;
    use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
    use crate::test_support::test_fixtures::primary_envelope_fixture;
    use crate::test_support::truncation::displayed_truncation;
    use cadmpeg_core::decode::{DecodeContext, View};
    use cadmpeg_ir::appearance::Appearance;
    use cadmpeg_ir::ids::{BodyId, FaceId};
    use cadmpeg_ir::topology::Color;

    #[test]
    fn short_digest_key_preserves_the_first_eight_sha256_bytes() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"abc", &arena, &DecodePolicy::service())
            .expect("digest source view");
        let mut storage = ctx
            .reserve_scoped(0, "short digest key test storage")
            .expect("digest storage");
        let key = storage
            .with_storage(|| short_digest_key(&ctx, b"abc", "short digest key test storage"))
            .expect("admitted digest key");
        assert_eq!(key.as_str(), "ba7816bf8f01cfea");
    }

    #[test]
    fn short_digest_key_refuses_scoped_limit_before_allocation() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 15;
        let (ctx, _) =
            DecodeContext::from_root_bytes(b"abc", &arena, &policy).expect("digest source view");
        let mut storage = ctx
            .reserve_scoped(0, "compose Inventor default binding key")
            .expect("empty digest storage");
        assert!(matches!(
            storage.with_storage(|| {
                short_digest_key(
                    &ctx,
                    b"abc",
                    "compose Inventor default binding key",
                )
            }),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::MaterializedBytes
                    && limit.operation == "compose Inventor default binding key"
                    && limit.used == 0
                    && limit.additional == 16
        ));
    }

    #[test]
    fn presentation_utf16_local_ceiling_refuses_resources_for_complete_payload() {
        let units = 1_048_577_u32;
        let mut bytes = units.to_le_bytes().to_vec();
        bytes.extend(std::iter::repeat_n(
            0_u8,
            usize::try_from(units).expect("length") * 2,
        ));
        crate::test_support::test_fixtures::parse(&bytes, |ctx, root| {
            let error = Cursor::new(root)
                .utf16(ctx, "name")
                .expect_err("local string ceiling");
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.operation == "Inventor presentation UTF-16 code units"
                    && limit.limit == 1_048_576 && Some(limit) == ctx.resource_refusal()));
        });
        crate::test_support::test_fixtures::parse(&units.to_le_bytes(), |ctx, root| {
            assert!(matches!(
                Cursor::new(root).utf16(ctx, "name"),
                Err(CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn presentation_utf16_text_charges_trimmed_utf8_size_and_keeps_interior_nul() {
        let mut bytes = Vec::new();
        utf16(&mut bytes, "ࠀ\0A\0\0");
        for retained in [4, 5] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = retained;
            let (ctx, root) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                    .expect("test root fits input admission");
            let result = Cursor::new(root).utf16(&ctx, "label");
            if retained == 4 {
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes && limit.additional == 5));
            } else {
                assert_eq!(
                    result.expect("trimmed text fits five retained bytes"),
                    "ࠀ\0A"
                );
                assert!(ctx.charge_retained(1, "after presentation text").is_err());
            }
        }
    }

    #[test]
    fn graphics_reference_lists_use_bounded_retained_grammar() {
        let mut bytes = vec![2_u8, 0, 0, 0x30];
        bytes.extend_from_slice(&1_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        crate::test_support::test_fixtures::parse(&bytes, |ctx, root| {
            assert!(matches!(
                super::Cursor::new(root).reference_list(ctx, "test"),
                Err(CodecError::Malformed(_))
            ));
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("test context");
        assert!(matches!(
            super::Cursor::new(root).reference_list(&ctx, "test"),
            Err(CodecError::Malformed(_))
        ));
        bytes[4..8].copy_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        policy.limits.max_collection_items = DecodePolicy::service().limits.max_collection_items;
        policy.limits.max_retained_bytes = 0;
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("test context");
        assert!(
            matches!(super::Cursor::new(root).reference_list(&ctx, "test"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn presentation_projection_refuses_collection_limit_before_face_key_index() {
        let inventory = super::PresentationInventory {
            default_styles: Vec::new(),
            rendering_styles: Vec::new(),
            graphics_faces: Vec::new(),
            graphics_style_collections: Vec::new(),
            graphics_primary_color_styles: Vec::new(),
            issues: Vec::new(),
        };
        let face = FaceId::mint("inventor:test:face#1").expect("face id");
        let face_keys = BTreeMap::from([(face, 42)]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            super::project_bindings(&ctx, &inventory, &[], &[], &face_keys),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "count Inventor presentation face keys"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert!(
            super::project_bindings(&ctx, &inventory, &[], &[], &face_keys)
                .expect("presentation projection")
                .bindings
                .is_empty()
        );
    }

    #[test]
    fn default_binding_projection_refuses_collection_limit_before_output() {
        let default_bytes = default_style_fixture();
        let style_bytes = rendering_style_fixture();
        let arena = DecodeArena::new();
        let (parse_ctx, default_root) =
            DecodeContext::from_root_bytes(&default_bytes, &arena, &DecodePolicy::service())
                .expect("default style view");
        let (_, style_root) =
            DecodeContext::from_root_bytes(&style_bytes, &arena, &DecodePolicy::service())
                .expect("rendering style view");
        let (inventory, appearance, body) =
            default_binding_projection_fixture(&parse_ctx, default_root, style_root);
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("limited projection context");
        assert!(matches!(
            project_default_bindings(&ctx, &inventory, std::slice::from_ref(&appearance), std::slice::from_ref(&body)),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "project Inventor default appearance binding"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert_eq!(
            project_default_bindings(&ctx, &inventory, &[appearance], &[body])
                .expect("default binding projection")
                .bindings
                .len(),
            1
        );
    }

    #[test]
    fn default_binding_projection_refuses_entity_limit_before_creation() {
        let default_bytes = default_style_fixture();
        let style_bytes = rendering_style_fixture();
        let arena = DecodeArena::new();
        let (parse_ctx, default_root) =
            DecodeContext::from_root_bytes(&default_bytes, &arena, &DecodePolicy::service())
                .expect("default style view");
        let (_, style_root) =
            DecodeContext::from_root_bytes(&style_bytes, &arena, &DecodePolicy::service())
                .expect("rendering style view");
        let (inventory, appearance, body) =
            default_binding_projection_fixture(&parse_ctx, default_root, style_root);
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("limited projection context");
        assert!(matches!(
            project_default_bindings(&ctx, &inventory, std::slice::from_ref(&appearance), std::slice::from_ref(&body)),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "project Inventor default appearance binding"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert_eq!(
            project_default_bindings(&ctx, &inventory, &[appearance], &[body])
                .expect("default binding projection")
                .bindings
                .len(),
            1
        );
    }

    fn default_binding_projection_fixture<'a>(
        parse_ctx: &DecodeContext<'a>,
        default_root: View<'a>,
        style_root: View<'a>,
    ) -> (PresentationInventory<'a>, Appearance, BodyId) {
        let default = parse_default_style(parse_ctx, default_root, 26).expect("default style");
        let style = parse_rendering_style(parse_ctx, style_root, 26).expect("rendering style");
        let inventory = PresentationInventory {
            default_styles: vec![Located::new(
                default,
                crate::record_identity::RecordTypeId::from_bytes(
                    &cadmpeg_test_support::service_decode_context(),
                    DEFAULT_STYLE_TYPE,
                    "retain Inventor fixture type id",
                )
                .expect("fixture type id"),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                0,
            )],
            rendering_styles: vec![Located::new(
                style,
                crate::record_identity::RecordTypeId::from_bytes(
                    &cadmpeg_test_support::service_decode_context(),
                    RENDERING_STYLE_TYPE,
                    "retain Inventor fixture type id",
                )
                .expect("fixture type id"),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                8,
            )],
            graphics_faces: Vec::new(),
            graphics_style_collections: Vec::new(),
            graphics_primary_color_styles: Vec::new(),
            issues: Vec::new(),
        };
        let appearance = Appearance {
            id: AppearanceId::mint("inventor:test:appearance#1").expect("appearance id"),
            name: None,
            asset_guid: Some("d3c6130d-6c0f-4525-b268-53517ab46a78".into()),
            library_id: Some("afefc330-5e61-4e24-814f-ae810148b79d".into()),
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: None,
            properties: BTreeMap::new(),
            textures: Vec::new(),
        };
        let body = BodyId::mint("inventor:test:body#1").expect("body id");
        (inventory, appearance, body)
    }

    fn inventory_with_record(
        kind: SegmentKind,
        type_id: [u8; 16],
        payload: &[u8],
        policy: DecodePolicy,
    ) -> Result<(usize, Vec<crate::record_issue::RecordIssue>), CodecError> {
        let bytes = primary_envelope_fixture();
        let payload = payload.to_vec();
        let arena = DecodeArena::new();
        let (setup_ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("envelope view");
        let mut container = InventorContainer::open(&setup_ctx, source).expect("framed envelope");
        let segment = &mut container.rse.segments[0];
        segment.kind = kind;
        let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
            panic!("framed bulk fixture");
        };
        let RecordFrameState::Framed(table) = &mut bulk.records else {
            panic!("framed record fixture");
        };
        table.records[0].type_id = type_id;
        table.records[0].payload = View::over_retained(&payload);
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input view");
        let result = inventory(&ctx, &container.rse)?;
        Ok((
            result.default_styles.len()
                + result.rendering_styles.len()
                + result.graphics_faces.len()
                + result.graphics_style_collections.len()
                + result.graphics_primary_color_styles.len(),
            result.issues,
        ))
    }

    #[test]
    fn presentation_record_forms_refuse_collection_limit_before_push() {
        let mut default = default_style_fixture();
        default.truncate(default.len() - 8);
        let mut rendering = Vec::new();
        rendering.extend_from_slice(&0u32.to_le_bytes());
        rendering.extend_from_slice(&0u16.to_le_bytes());
        rendering.push(0);
        rendering.extend_from_slice(&[0; 2 + 4 + 4 + 4 + 4]);
        utf16(&mut rendering, "a");
        utf16(&mut rendering, "b");
        rendering.extend_from_slice(&0u16.to_le_bytes());
        for _ in 0..4 {
            utf16(&mut rendering, "");
        }
        rendering.extend_from_slice(&[0; 4 + 16]);
        let mut collection = Vec::new();
        collection.extend_from_slice(&2u16.to_le_bytes());
        collection.extend_from_slice(&0x3000u16.to_le_bytes());
        collection.extend_from_slice(&0u32.to_le_bytes());
        for (kind, type_id, payload, parser_items, operation) in [
            (
                SegmentKind::PmApp,
                DEFAULT_STYLE_TYPE,
                default,
                0,
                "admit Inventor default style record",
            ),
            (
                SegmentKind::PmApp,
                RENDERING_STYLE_TYPE,
                rendering,
                0,
                "admit Inventor rendering style record",
            ),
            (
                SegmentKind::PmGraphics,
                GRAPHICS_FACE_TYPE,
                graphics_face_fixture([1.0; 6]),
                2,
                "admit Inventor graphics face record",
            ),
            (
                SegmentKind::PmGraphics,
                GRAPHICS_STYLE_COLLECTION_TYPE,
                collection,
                0,
                "admit Inventor graphics style collection record",
            ),
            (
                SegmentKind::PmGraphics,
                GRAPHICS_PRIMARY_COLOR_STYLE_TYPE,
                primary_color_fixture([0.4; 4]),
                0,
                "admit Inventor graphics primary color style record",
            ),
        ] {
            let admitted =
                inventory_with_record(kind.clone(), type_id, &payload, DecodePolicy::service())
                    .expect("presentation record is admitted");
            assert_eq!(admitted.0, 1, "{operation}");
            assert!(admitted.1.is_empty());
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = parser_items;
            assert!(matches!(
                inventory_with_record(kind, type_id, &payload, policy),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == operation
                        && limit.used == parser_items
            ));
        }
    }

    #[test]
    fn presentation_parse_issue_refuses_collection_limit_before_push() {
        assert_eq!(
            inventory_with_record(
                SegmentKind::PmApp,
                DEFAULT_STYLE_TYPE,
                &[],
                DecodePolicy::service()
            )
            .expect("truncated style becomes an issue")
            .1
            .len(),
            1
        );
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        assert!(matches!(
            inventory_with_record(SegmentKind::PmApp, DEFAULT_STYLE_TYPE, &[], policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit Inventor presentation issue"
                    && limit.used == 0
        ));
    }

    #[test]
    fn presentation_parse_issue_refuses_entity_limit_before_push() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        assert!(matches!(
            inventory_with_record(SegmentKind::PmApp, DEFAULT_STYLE_TYPE, &[], policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "admit Inventor presentation issue"
        ));
        assert_eq!(
            inventory_with_record(
                SegmentKind::PmApp,
                DEFAULT_STYLE_TYPE,
                &[],
                DecodePolicy::service()
            )
            .expect("service issue")
            .1
            .len(),
            1
        );
    }

    #[test]
    fn presentation_record_identity_refuses_retained_limits_before_copy() {
        let mut payload = default_style_fixture();
        payload.truncate(payload.len() - 8);
        for (limit_bytes, operation, used) in [
            (31, "retain Inventor presentation record type id", 0),
            (32, "retain Inventor presentation record segment token", 32),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit_bytes;
            assert!(matches!(
                inventory_with_record(SegmentKind::PmApp, DEFAULT_STYLE_TYPE, &payload, policy),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.operation == operation
                        && limit.used == used
            ));
        }
    }

    #[test]
    fn presentation_issue_copies_refuse_retained_limits_before_creation() {
        let admitted = inventory_with_record(
            SegmentKind::PmApp,
            DEFAULT_STYLE_TYPE,
            &[],
            DecodePolicy::service(),
        )
        .expect("truncated style becomes an issue");
        let detail_len = admitted.1[0].detail.len();
        let token_len = admitted.1[0].segment_token.as_str().len();
        for (limit_bytes, operation, used) in [
            (
                detail_len - 1,
                "retain Inventor presentation issue detail",
                0,
            ),
            (
                detail_len + token_len - 1,
                "retain Inventor presentation issue token",
                detail_len,
            ),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(limit_bytes);
            assert!(matches!(
                inventory_with_record(SegmentKind::PmApp, DEFAULT_STYLE_TYPE, &[], policy),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.operation == operation
                        && limit.used == cadmpeg_core::decode::u64_from_index(used)
            ));
        }
    }

    #[test]
    fn parses_current_default_style_and_one_based_rendering_reference() {
        let bytes = default_style_fixture();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic default style fits policy");

        let style = parse_default_style(&ctx, root, 26).expect("default style parses");

        assert_eq!(style.material_reference, 8);
        assert_eq!(style.rendering_style_reference, 9);
        assert_eq!(style.related_references, [10, 11, 12, 13, 14, 15, 16]);
        assert_eq!(style.terminal_reference, 17);
        assert!(style.suffix.window().is_empty());
    }

    #[test]
    fn rejects_nonzero_unqualified_record_reference() {
        let mut bytes = default_style_fixture();
        bytes[10..14].copy_from_slice(&9_u32.to_le_bytes());
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic default style fits policy");

        let error = parse_default_style(&ctx, root, 26).expect_err("reference must be qualified");

        assert!(error.to_string().contains("lacks its reference qualifier"));
    }

    #[test]
    fn parses_current_rendering_style_asset_identity_and_retains_suffix() {
        let bytes = rendering_style_fixture();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic rendering style fits policy");

        let style = parse_rendering_style(&ctx, root, 26).expect("rendering style parses");

        assert_eq!(style.name, "Steel");
        let extension = style
            .extension
            .as_ref()
            .expect("version 26 stores the rendering-style extension");
        assert_eq!(extension.style_label, "1:Steel");
        assert_eq!(extension.asset_guid, "d3c6130d-6c0f-4525-b268-53517ab46a78");
        assert_eq!(extension.material_id, "InvGen-066");
        assert_eq!(
            extension.asset_library_id,
            "afefc330-5e61-4e24-814f-ae810148b79d"
        );
        assert_eq!(style.suffix.window(), &[0xaa, 0x55]);
    }

    #[test]
    fn projects_only_the_exact_default_style_asset_join() {
        let default_bytes = default_style_fixture();
        let style_bytes = rendering_style_fixture();
        let arena = DecodeArena::new();
        let (ctx, default_root) =
            DecodeContext::from_root_bytes(&default_bytes, &arena, &DecodePolicy::default())
                .expect("synthetic default style fits policy");
        let (_, style_root) =
            DecodeContext::from_root_bytes(&style_bytes, &arena, &DecodePolicy::default())
                .expect("synthetic rendering style fits policy");
        let default = parse_default_style(&ctx, default_root, 26).expect("default parses");
        let style = parse_rendering_style(&ctx, style_root, 26).expect("style parses");
        let inventory = PresentationInventory {
            default_styles: vec![Located::new(
                default,
                crate::record_identity::RecordTypeId::from_bytes(
                    &cadmpeg_test_support::service_decode_context(),
                    DEFAULT_STYLE_TYPE,
                    "retain Inventor fixture type id",
                )
                .expect("fixture type id"),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                0,
            )],
            rendering_styles: vec![Located::new(
                style,
                crate::record_identity::RecordTypeId::from_bytes(
                    &cadmpeg_test_support::service_decode_context(),
                    RENDERING_STYLE_TYPE,
                    "retain Inventor fixture type id",
                )
                .expect("fixture type id"),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                8,
            )],
            graphics_faces: Vec::new(),
            graphics_style_collections: Vec::new(),
            graphics_primary_color_styles: Vec::new(),
            issues: Vec::new(),
        };
        let appearance = Appearance {
            id: AppearanceId::mint("inventor:test:appearance#1").expect("identity grammar"),
            name: None,
            asset_guid: Some("d3c6130d-6c0f-4525-b268-53517ab46a78".into()),
            library_id: Some("afefc330-5e61-4e24-814f-ae810148b79d".into()),
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: None,
            properties: BTreeMap::new(),
            textures: Vec::new(),
        };

        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        let projection = project_default_bindings(
            &ctx,
            &inventory,
            &[appearance],
            &[BodyId::mint("inventor:test:body#1").expect("identity grammar")],
        )
        .expect("default binding projection");

        assert_eq!(projection.unresolved_defaults, 0);
        let [binding] = projection.bindings.as_slice() else {
            panic!("one body binding must be projected");
        };
        assert_eq!(
            binding.appearance,
            AppearanceId::mint("inventor:test:appearance#1").expect("identity grammar")
        );
        assert_eq!(
            binding.target,
            AppearanceTarget::Body(BodyId::mint("inventor:test:body#1").expect("identity grammar"))
        );
        assert_eq!(
            binding.id.as_str(),
            "inventor:presentation:body-default#c33e8e01e06e1f56"
        );
    }

    fn default_style_fixture() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(0_u32.to_le_bytes());
        bytes.extend(1_u16.to_le_bytes());
        for reference in 8_u32..=16 {
            bytes.extend((reference | 0x8000_0000).to_le_bytes());
        }
        bytes.push(0);
        bytes.extend(0x8000_0011_u32.to_le_bytes());
        bytes.extend([0; 8]);
        bytes
    }

    /// A current graphics-face record with qualified references and `bounds`.
    fn graphics_face_fixture(bounds: [f64; 6]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(4_u32.to_le_bytes());
        bytes.extend(5_u16.to_le_bytes());
        bytes.extend(6_u32.to_le_bytes());
        for reference in 7_u32..=9 {
            bytes.extend((reference | 0x8000_0000).to_le_bytes());
        }
        bytes.extend(10_u32.to_le_bytes());
        bytes.extend(2_u16.to_le_bytes());
        bytes.extend(0x3000_u16.to_le_bytes());
        bytes.extend(2_u32.to_le_bytes());
        bytes.extend(11_u32.to_le_bytes());
        bytes.extend(12_u32.to_le_bytes());
        bytes.extend(0x8000_000d_u32.to_le_bytes());
        bytes.extend(0x8000_000e_u32.to_le_bytes());
        bytes.push(1);
        for value in bounds {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(15_u32.to_le_bytes());
        bytes.extend(16_u32.to_le_bytes());
        bytes.extend(17_u32.to_le_bytes());
        bytes
    }

    #[test]
    fn parses_current_graphics_face_with_qualified_references() {
        let bytes = graphics_face_fixture([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic graphics face fits policy");

        let face = parse_graphics_face(&ctx, root, 26).expect("graphics face parses");

        assert_eq!(face.styles.index(), 7);
        assert!(face.styles.qualified());
        assert_eq!(face.surface.index(), 8);
        assert!(face.surface.qualified());
        assert_eq!(face.parent.index(), 9);
        assert!(face.parent.qualified());
        assert_eq!(
            face.edge_references.references(),
            [
                PmDcReference::new(13, true).expect("test reference index fits 31 bits"),
                PmDcReference::new(14, true).expect("test reference index fits 31 bits")
            ]
        );
        assert_eq!(face.edge_references.metadata().copied(), Some([11, 12]));
        assert_eq!(
            FiniteReal::raw_array(face.bounds),
            [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
        );
        assert_eq!(face.key, 15);
        assert_eq!(face.values, [16, 17]);
    }

    #[test]
    fn a_non_finite_graphics_face_bound_is_refused() {
        let bytes = graphics_face_fixture([1.0, 2.0, f64::NAN, 4.0, 5.0, 6.0]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic graphics face fits policy");

        let error = parse_graphics_face(&ctx, root, 26).expect_err("a NaN bound is refused");
        assert_eq!(
            error.to_string(),
            "malformed container: Inventor presentation graphics-face bound is not finite"
        );
    }

    #[test]
    fn parses_current_graphics_style_collection() {
        let mut bytes = Vec::new();
        bytes.extend(2_u16.to_le_bytes());
        bytes.extend(0x3000_u16.to_le_bytes());
        bytes.extend(2_u32.to_le_bytes());
        bytes.extend(21_u32.to_le_bytes());
        bytes.extend(22_u32.to_le_bytes());
        bytes.extend(0x8000_0017_u32.to_le_bytes());
        bytes.extend(24_u32.to_le_bytes());
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic graphics style collection fits policy");

        let styles = parse_graphics_style_collection(&ctx, root, 26)
            .expect("graphics style collection parses");

        assert_eq!(
            styles.style_references.references(),
            [
                PmDcReference::new(23, true).expect("test reference index fits 31 bits"),
                PmDcReference::new(24, false).expect("test reference index fits 31 bits")
            ]
        );
        assert_eq!(styles.style_references.metadata().copied(), Some([21, 22]));
    }

    #[test]
    fn parses_current_graphics_primary_color_style() {
        let bytes = primary_color_fixture([0.2, 0.4, 0.6, 0.8]);
        let arena = DecodeArena::new();
        let (_, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic primary-color style fits policy");

        let style = parse_graphics_primary_color_style(root, 26)
            .expect("graphics primary-color style parses");

        assert_eq!(style.header_value, 31);
        assert_eq!(style.controls, [32, 33, 34, 35, 36, 37, 38]);
        assert_eq!(style.color_header, [39, 40]);
        assert_eq!(
            style.colors[1].map(cadmpeg_ir::scalar::FiniteBinary32::get),
            [0.2, 0.4, 0.6, 0.8]
        );
        assert_eq!(style.color_tail, [41, 42]);
        assert_eq!(style.state, 43);
        assert_eq!(style.values, [44, 45]);
        assert_eq!(style.terminal_state, 46);
    }

    #[test]
    fn primary_color_reader_refuses_nonfinite_component() {
        let bytes = primary_color_fixture([0.2, f32::NAN, 0.6, 0.8]);
        let arena = DecodeArena::new();
        let (_, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic primary-color style fits policy");
        let error = parse_graphics_primary_color_style(root, 26)
            .expect_err("a nonfinite primary-color component is refused by the reader");
        assert!(error
            .to_string()
            .contains("graphics primary-color component is not finite"));
    }

    fn graphics_face(key: u32, ordinal: u32) -> Located<PmGraphicsFace> {
        Located::new(
            PmGraphicsFace {
                segment_version_major: 26,
                header_value: 0,
                header_id: 0,
                flags: 0,
                styles: PmDcReference::new(5, true).expect("test reference index fits 31 bits"),
                surface: PmDcReference::new(0, false).expect("test reference index fits 31 bits"),
                parent: PmDcReference::new(0, false).expect("test reference index fits 31 bits"),
                state: 0,
                edge_references: PmDcPairedReferenceList::default(),
                visibility_state: 0,
                bounds: [FiniteReal::ZERO; 6],
                key,
                values: [0; 2],
            },
            crate::record_identity::RecordTypeId::from_bytes(
                &cadmpeg_test_support::service_decode_context(),
                GRAPHICS_FACE_TYPE,
                "retain Inventor fixture type id",
            )
            .expect("fixture type id"),
            cadmpeg_ir::identity_key!("graphics")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            ordinal,
        )
    }

    fn face_override_inventory() -> PresentationInventory<'static> {
        let face = graphics_face(42, 2);
        let collection = Located::new(
            PmGraphicsStyleCollection {
                segment_version_major: 26,
                style_references: PmDcPairedReferenceList::new(
                    Some([1, 2]),
                    vec![PmDcReference::new(7, true).expect("test reference index fits 31 bits")],
                )
                .expect("valid reference list"),
            },
            crate::record_identity::RecordTypeId::from_bytes(
                &cadmpeg_test_support::service_decode_context(),
                GRAPHICS_STYLE_COLLECTION_TYPE,
                "retain Inventor fixture type id",
            )
            .expect("fixture type id"),
            cadmpeg_ir::identity_key!("graphics")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            4,
        );
        let style = Located::new(
            PmGraphicsPrimaryColorStyle {
                segment_version_major: 26,
                header_value: 0,
                controls: [0; 7],
                color_header: [0; 2],
                colors: [[0.0; 4], [0.2, 0.4, 0.6, 0.8], [0.0; 4], [0.0; 4]].map(|color| {
                    color.map(|value| {
                        cadmpeg_ir::scalar::FiniteBinary32::new(value).expect("finite")
                    })
                }),
                color_tail: [0; 2],
                state: 0,
                values: [0; 2],
                terminal_state: 0,
            },
            crate::record_identity::RecordTypeId::from_bytes(
                &cadmpeg_test_support::service_decode_context(),
                GRAPHICS_PRIMARY_COLOR_STYLE_TYPE,
                "retain Inventor fixture type id",
            )
            .expect("fixture type id"),
            cadmpeg_ir::identity_key!("graphics")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            6,
        );
        PresentationInventory {
            default_styles: Vec::new(),
            rendering_styles: Vec::new(),
            graphics_faces: vec![face],
            graphics_style_collections: vec![collection],
            graphics_primary_color_styles: vec![style],
            issues: Vec::new(),
        }
    }

    #[test]
    fn projects_face_override_through_native_key_and_style_graph() {
        let inventory = face_override_inventory();
        let face_id = FaceId::mint("inventor:test:face#1").expect("identity grammar");
        let face_keys = BTreeMap::from([(face_id.clone(), 42)]);

        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        let projection = project_bindings(&ctx, &inventory, &[], &[], &face_keys)
            .expect("face binding projection");

        assert!(projection.unresolved_face_overrides.is_empty());
        let [appearance] = projection.appearances.as_slice() else {
            panic!("one primary-color appearance must be projected");
        };
        assert_eq!(
            appearance.base_color,
            Some(Color::new(0.2, 0.4, 0.6, 0.8).expect("valid color"))
        );
        let [binding] = projection.bindings.as_slice() else {
            panic!("one face binding must be projected");
        };
        assert_eq!(binding.target, AppearanceTarget::Face(face_id));
        assert_eq!(binding.appearance, appearance.id);
        assert_eq!(
            binding.id.as_str(),
            "inventor:presentation:face-override#13eff7fe6cd1d8c5"
        );
        assert_eq!(
            binding.channels.get("precedence").map(String::as_str),
            Some("face_over_body")
        );
    }

    fn project_faces(
        inventory: &PresentationInventory<'_>,
        keys: &[(&str, u64)],
    ) -> super::PresentationProjection {
        let face_keys = keys
            .iter()
            .map(|(id, key)| (FaceId::mint(*id).expect("identity grammar"), *key))
            .collect::<BTreeMap<_, _>>();
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        project_bindings(&ctx, inventory, &[], &[], &face_keys).expect("face binding projection")
    }

    #[test]
    fn face_overrides_report_each_ambiguous_join_through_the_record_indexes() {
        let face = [("inventor:test:face#1", 42)];
        let mut inventory = face_override_inventory();
        inventory.graphics_faces.push(graphics_face(42, 3));
        let projection = project_faces(&inventory, &face);
        assert!(projection.bindings.is_empty());
        assert_eq!(
            projection
                .unresolved_face_overrides
                .get(&super::UnresolvedCause::GraphicsFace)
                .map(|count| count.get()),
            Some(1)
        );

        let mut inventory = face_override_inventory();
        inventory
            .graphics_style_collections
            .extend(face_override_inventory().graphics_style_collections);
        let projection = project_faces(&inventory, &face);
        assert!(projection.bindings.is_empty());
        assert!(projection
            .unresolved_face_overrides
            .contains_key(&super::UnresolvedCause::StyleCollection));

        let mut inventory = face_override_inventory();
        inventory
            .graphics_primary_color_styles
            .extend(face_override_inventory().graphics_primary_color_styles);
        let projection = project_faces(&inventory, &face);
        assert!(projection.bindings.is_empty());
        assert!(projection
            .unresolved_face_overrides
            .contains_key(&super::UnresolvedCause::ColorStyle));
    }

    #[test]
    fn faces_sharing_a_color_style_share_its_one_appearance() {
        let mut inventory = face_override_inventory();
        inventory.graphics_faces.push(graphics_face(43, 3));
        let projection = project_faces(
            &inventory,
            &[("inventor:test:face#1", 42), ("inventor:test:face#2", 43)],
        );
        assert!(projection.unresolved_face_overrides.is_empty());
        let [appearance] = projection.appearances.as_slice() else {
            panic!("one primary-color appearance must be projected");
        };
        let [first, second] = projection.bindings.as_slice() else {
            panic!("two face bindings must be projected");
        };
        assert_eq!(first.appearance, appearance.id);
        assert_eq!(second.appearance, appearance.id);
    }

    #[test]
    fn face_binding_projection_refuses_entity_limits_before_creations() {
        let inventory = face_override_inventory();
        let face_id = FaceId::mint("inventor:test:face#1").expect("identity grammar");
        let face_keys = BTreeMap::from([(face_id, 42)]);
        let arena = DecodeArena::new();
        for (max_entities, operation) in [
            (0, "project Inventor face appearance"),
            (1, "project Inventor face appearance binding"),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_entities = max_entities;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
            assert!(matches!(
                project_bindings(&ctx, &inventory, &[], &[], &face_keys),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::Entities
                        && limit.operation == operation
            ));
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert_eq!(
            project_bindings(&ctx, &inventory, &[], &[], &face_keys)
                .expect("face binding projection")
                .bindings
                .len(),
            1
        );
    }

    #[test]
    fn face_binding_projection_refuses_work_limit_before_graphics_face_index() {
        let inventory = face_override_inventory();
        let face_id = FaceId::mint("inventor:test:face#1").expect("identity grammar");
        let face_keys = BTreeMap::from([(face_id, 42)]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // One face-key traversal, an eight-byte hash key and the key-count table's growth
        // bound (four buckets, their control bytes, alignment and trailing controls) precede
        // the graphics-face index.
        let key_count_table = 4 * std::mem::size_of::<(&u64, usize)>() + 15 + 4 + 16;
        policy.limits.max_work_units =
            cadmpeg_core::decode::u64_from_index(face_keys.len() + 8 + key_count_table);
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            project_bindings(&ctx, &inventory, &[], &[], &face_keys),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "index Inventor graphics faces by key"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert_eq!(
            project_bindings(&ctx, &inventory, &[], &[], &face_keys)
                .expect("face binding projection")
                .bindings
                .len(),
            1
        );
    }

    fn rendering_style_fixture() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(0_u32.to_le_bytes());
        bytes.extend(7_u16.to_le_bytes());
        bytes.push(0);
        bytes.extend(3_u16.to_le_bytes());
        bytes.extend([0; 2]);
        bytes.extend(0_u16.to_le_bytes());
        bytes.extend(0_u16.to_le_bytes());
        bytes.extend(1_u32.to_le_bytes());
        bytes.extend(0_u32.to_le_bytes());
        bytes.extend(0x8000_0016_u32.to_le_bytes());
        utf16(&mut bytes, "Steel");
        utf16(&mut bytes, "Generic material");
        bytes.extend(0_u16.to_le_bytes());
        utf16(&mut bytes, "1:Steel");
        utf16(&mut bytes, "d3c6130d-6c0f-4525-b268-53517ab46a78");
        utf16(&mut bytes, "InvGen-066");
        utf16(&mut bytes, "afefc330-5e61-4e24-814f-ae810148b79d");
        bytes.extend(0_u16.to_le_bytes());
        bytes.extend(2_u16.to_le_bytes());
        bytes.extend([
            0xf0, 0x23, 0x1c, 0x26, 0xcd, 0x46, 0x2d, 0x79, 0x3e, 0x2d, 0x3d, 0x21, 0x13, 0xbd,
            0x6b, 0xac,
        ]);
        bytes.extend([0xaa, 0x55]);
        bytes
    }

    fn primary_color_fixture(diffuse: [f32; 4]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(31_u32.to_le_bytes());
        for value in 32_u16..=38 {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend([39, 40]);
        for color in [[0.1; 4], diffuse, [0.7; 4], [0.9; 4]] {
            for component in color {
                bytes.extend(component.to_le_bytes());
            }
        }
        bytes.extend(41_u16.to_le_bytes());
        bytes.extend(42_u16.to_le_bytes());
        bytes.push(43);
        bytes.extend(44_u16.to_le_bytes());
        bytes.extend(45_u16.to_le_bytes());
        bytes.push(46);
        bytes
    }

    fn utf16(bytes: &mut Vec<u8>, value: &str) {
        let units = value.encode_utf16().collect::<Vec<_>>();
        bytes.extend((u32::try_from(units.len()).expect("fixture value fits u32")).to_le_bytes());
        for unit in units {
            bytes.extend(unit.to_le_bytes());
        }
    }

    #[test]
    fn truncated_presentation_reads_name_the_field() {
        let empty = &[];
        for (field, text) in [
            (
                "graphics primary-color state",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).u8("graphics primary-color state"),
                ),
            ),
            (
                "graphics-face header id",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).u16("graphics-face header id"),
                ),
            ),
            (
                "graphics-face key",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).u32("graphics-face key"),
                ),
            ),
            (
                "graphics-face bound",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).finite_f64("graphics-face bound"),
                ),
            ),
            (
                "graphics primary-color component",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty))
                        .finite_f32("graphics primary-color component"),
                ),
            ),
            (
                "graphics-face legacy object padding",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty))
                        .take(4, "graphics-face legacy object padding"),
                ),
            ),
            (
                "default-style suffix padding",
                displayed_truncation(Cursor::new(View::over_retained(empty)).zeroes(
                    &cadmpeg_test_support::service_decode_context(),
                    8,
                    "default-style suffix padding",
                )),
            ),
            (
                "default-style material reference",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty))
                        .reference("default-style material reference"),
                ),
            ),
            (
                "graphics-face styles reference",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty))
                        .node_reference("graphics-face styles reference"),
                ),
            ),
            (
                "rendering-style guid",
                displayed_truncation(Cursor::new(View::over_retained(empty)).guid(
                    &cadmpeg_test_support::service_decode_context(),
                    "rendering-style guid",
                )),
            ),
        ] {
            assert_eq!(
                text,
                format!("truncated input during {field} at space 0 offset 0")
            );
        }
    }

    #[test]
    fn a_truncated_graphics_record_names_the_field_it_stopped_in() {
        let mut bytes = Vec::new();
        bytes.extend(31_u32.to_le_bytes());
        bytes.extend(32_u16.to_le_bytes());
        assert_eq!(
            displayed_truncation(parse_graphics_primary_color_style(
                View::over_retained(&bytes),
                26
            )),
            "truncated input during graphics primary-color control at space 0 offset 6"
        );
    }
}
