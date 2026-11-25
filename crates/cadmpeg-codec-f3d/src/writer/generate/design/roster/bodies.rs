// SPDX-License-Identifier: Apache-2.0
//! The body records of a Base Feature design: the `Body` owners, the bodies
//! root, the snapshot companions, the body maps, the body-reference records
//! with their Scene records, and the browser-node members (f3d.md §3.1,
//! "`Body` owners", "Snapshot body-map companions", "Body maps of a Base
//! Feature design", "Body-reference records", "Scene records of a
//! body-reference record", "Browser-node member").

use cadmpeg_core::CodecError;

use super::super::frames::{Bind, DesignRecords};
use super::component::{Assets, MATERIAL, MATERIAL_LIBRARY};
use super::fixed::DATA_MODEL_OBJECT;
use super::{
    count, BodyIndices, Context, RosterBody, AGGREGATE, ALTERNATE_OWNERS, ASSET_MANAGER,
    ASSET_SETTINGS, BODIES_ROOT, BODY_MAP, CONTEXT, DESIGN_COMPANION, DESIGN_OWNER,
    PARAMETER_COMPANION, PARAMETER_OWNER, SNAPSHOT_MAP, SNAPSHOT_MAP_COMPANION,
};
use crate::layout;

/// The body-presentation property of the design body-reference record.
const DESIGN_PROPERTY: &str = "BASEFEAT_DM_BODY_REF";
/// The body-presentation property of the parameter body-reference record.
const PARAMETER_PROPERTY: &str = "BASEFEAT_PM_BODY_REF";
/// The appearance library of the second presentation envelope.
const APPEARANCE_LIBRARY: &str = "BA5EE55E-9982-449B-9D66-9F036540E140";
/// The appearance of the second presentation envelope.
const APPEARANCE: &str = "PrismOpaque";
/// The colour of the appearance `PrismOpaque`: `(246, 246, 243) / 255`.
const APPEARANCE_COLOUR: [f32; 4] = [246.0 / 255.0, 246.0 / 255.0, 243.0 / 255.0, 1.0];
/// The word after the appearance-entry count.
const PRESENTATION_WORD: u32 = 0x0023_6810;
/// The box of a parameter body-reference record's Scene records.
const EMPTY_BOX: [f64; 6] = [f64::MIN, f64::MIN, f64::MIN, f64::MAX, f64::MAX, f64::MAX];
/// The box of a design body-reference record's Scene records where the body
/// has no measured box: the unit cube, a box of positive extent.
const UNIT_BOX: [f64; 6] = [1.0, 1.0, 1.0, 0.0, 0.0, 0.0];

/// The basenames of the history and snapshot streams.
pub(super) fn stream_basenames(ctx: &Context<'_>) -> [String; 2] {
    [
        format!("BREP.{}.smbh", ctx.values.lower_guid("history-stream")),
        format!("BREP.{}.smb", ctx.values.lower_guid("snapshot-stream")),
    ]
}

/// The mass-block GUID of the design body-reference record `record`: the
/// `Geometry` aspect GUID of its ACT aspect entry.
pub(super) fn geometry_guid(ctx: &Context<'_>, record: u32) -> String {
    ctx.values.lower_guid(&format!("act-geometry-{record}"))
}

pub(super) fn write(ctx: &Context<'_>, out: &mut DesignRecords) -> Result<(), CodecError> {
    let indices = &ctx.roster.bodies;
    let design = indices.iter().map(|body| body.design).collect::<Vec<_>>();
    let parameter = indices
        .iter()
        .map(|body| body.parameter)
        .collect::<Vec<_>>();
    owner(
        ctx,
        out,
        DESIGN_OWNER,
        &design,
        DESIGN_COMPANION,
        BODY_MAP,
        Some(ALTERNATE_OWNERS),
    )?;
    owner(
        ctx,
        out,
        PARAMETER_OWNER,
        &parameter,
        PARAMETER_COMPANION,
        SNAPSHOT_MAP,
        None,
    )?;
    let rolled = ctx
        .roster
        .rolled
        .iter()
        .filter_map(|item| item.place.rolled)
        .collect::<Vec<_>>();
    out.record(&ctx.class_of(BODIES_ROOT)?, BODIES_ROOT)
        .zeros(4)
        .lp_ascii("BodiesRoot")?
        .zeros(2)
        .lp_ascii("BodiesRoot")?
        .u32(count(design.len() + parameter.len() + rolled.len())?);
    for record in design
        .iter()
        .chain(&parameter)
        .chain(rolled.iter().map(|body| &body.reference))
    {
        out.reference(*record);
    }
    out.byte(0);

    let entities = ctx
        .bodies
        .iter()
        .try_fold(0u64, |sum, body| {
            u64::try_from(body.entities)
                .ok()
                .and_then(|entities| sum.checked_add(entities))
        })
        .ok_or_else(|| CodecError::NotImplemented("Base Feature entity count".into()))?;
    let first_design = entities + 1;
    companion(
        ctx,
        out,
        PARAMETER_COMPANION,
        PARAMETER_OWNER,
        0,
        entities,
        first_design,
    )?;
    companion(
        ctx,
        out,
        DESIGN_COMPANION,
        DESIGN_OWNER,
        first_design,
        entities,
        first_design + entities,
    )?;

    let [history, snapshot] = stream_basenames(ctx);
    out.record(&ctx.class_of(SNAPSHOT_MAP)?, SNAPSHOT_MAP)
        .zeros(10)
        .reference(SNAPSHOT_MAP_COMPANION);
    body_pairs(out, &parameter)?;
    out.reference(PARAMETER_OWNER).byte(0).lp_utf16(&snapshot)?;
    out.record(&ctx.class_of(BODY_MAP)?, BODY_MAP).zeros(10);
    body_pairs(out, &design)?;
    out.reference(DESIGN_OWNER).byte(0).lp_utf16(&history)?;

    let assets = Assets::new(ctx);
    for (position, (body, records)) in ctx.bodies.iter().zip(indices).enumerate() {
        let ordinal = count(position + 1)?;
        let scene = ctx
            .values
            .boxes
            .get(position)
            .copied()
            .flatten()
            .map_or(UNIT_BOX, |measured| measured.scene());
        for side in [Side::Design, Side::Parameter] {
            reference(
                ctx,
                out,
                &Reference::of(ctx, body, records, ordinal, side),
                &assets,
            )?;
        }
        scene_records(ctx, out, records.design_scene, &scene)?;
        scene_records(ctx, out, records.parameter_scene, &EMPTY_BOX)?;
        node(
            ctx,
            out,
            records.node,
            &body.node_guid,
            false,
            records.design,
            ctx.item_scope(),
        )?;
    }
    for (position, item) in (ctx.bodies.len()..).zip(&ctx.roster.rolled) {
        let Some(body) = item.place.rolled else {
            continue;
        };
        let guid = super::swept::rolled_node_guid(ctx, body.node);
        let rolled = Reference {
            record: body.reference,
            property: None,
            owner: DESIGN_OWNER,
            scene: body.scene[0],
            guid: &guid,
            mass_guid: geometry_guid(ctx, body.reference),
            accuracy: 0.0,
            sheet: false,
            ordinal: count(position + 1)?,
            design: true,
        };
        reference(ctx, out, &rolled, &assets)?;
        scene_records(ctx, out, body.scene, &EMPTY_BOX)?;
        node(
            ctx,
            out,
            body.node,
            &guid,
            true,
            body.reference,
            item.scope(),
        )?;
    }
    Ok(())
}

/// The browser-node member `record` of the body-reference record `body`,
/// which the item `scope` makes: its GUID, its hidden flag, the body and the
/// scope.
fn node(
    ctx: &Context<'_>,
    out: &mut DesignRecords,
    record: u32,
    guid: &str,
    hidden: bool,
    body: u32,
    scope: u32,
) -> Result<(), CodecError> {
    let object = ctx.class(DATA_MODEL_OBJECT)?;
    out.record(&ctx.class_of(record)?, record)
        .zeros(10)
        .lp_utf16(guid)?
        .byte(u8::from(hidden))
        .byte(1)
        .reference(body)
        .reference(scope)
        .header(&object, record)
        .zeros(8)
        .reference(CONTEXT);
    Ok(())
}

/// A `Body` owner of the `records` body-reference records.
fn owner(
    ctx: &Context<'_>,
    out: &mut DesignRecords,
    index: u32,
    records: &[u32],
    companion: u32,
    map: u32,
    alternates: Option<u32>,
) -> Result<(), CodecError> {
    let bodies = count(records.len())?;
    out.record(&ctx.class_of(index)?, index)
        .zeros(10)
        .byte(0)
        .u32(bodies)
        .u32(bodies);
    for record in records {
        out.reference(*record);
    }
    out.byte(1).reference(companion).reference(map).byte(1);
    match alternates {
        Some(list) => out.reference(list),
        None => out.byte(0),
    };
    out.byte(0).u32(bodies);
    for record in records {
        out.reference(*record);
    }
    out.reference(AGGREGATE);
    Ok(())
}

/// A snapshot body-map companion listing `count` consecutive values from
/// `first`.
fn companion(
    ctx: &Context<'_>,
    out: &mut DesignRecords,
    index: u32,
    owner: u32,
    first: u64,
    values: u64,
    next: u64,
) -> Result<(), CodecError> {
    let listed = u32::try_from(values)
        .map_err(|_| CodecError::NotImplemented("Base Feature entity count exceeds u32".into()))?;
    out.record(&ctx.class_of(index)?, index)
        .zeros(14)
        .u64(next)
        .reference(owner)
        .byte(0)
        .u32(listed);
    for value in first..first + values {
        out.u64(value);
    }
    Ok(())
}

/// The pair count and one pair `(i, record)` for each body.
fn body_pairs(out: &mut DesignRecords, records: &[u32]) -> Result<(), CodecError> {
    out.u32(count(records.len())?);
    for (position, record) in (0u64..).zip(records) {
        out.u64(position).u64(u64::from(*record));
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Design,
    Parameter,
}

/// The values of one body-reference record.
struct Reference<'g> {
    record: u32,
    /// The body-presentation property and the other body-reference record
    /// of the body; `None` in the presentation-owner form.
    property: Option<(&'static str, u32)>,
    owner: u32,
    /// The record's Scene node.
    scene: u32,
    guid: &'g str,
    mass_guid: String,
    accuracy: f64,
    sheet: bool,
    ordinal: u32,
    /// Whether it is a design body-reference record.
    design: bool,
}

impl<'g> Reference<'g> {
    /// The `side` body-reference record of the result body `body`.
    fn of(
        ctx: &Context<'_>,
        body: &'g RosterBody<'_>,
        records: &BodyIndices,
        ordinal: u32,
        side: Side,
    ) -> Self {
        match side {
            Side::Design => Self {
                record: records.design,
                // A geometry item's result body has the presentation-owner
                // form: no property block (f3d.md §3.1, "Result body").
                property: ctx
                    .roster
                    .item
                    .is_none()
                    .then_some((DESIGN_PROPERTY, records.parameter)),
                owner: DESIGN_OWNER,
                scene: records.design_scene[0],
                guid: &body.node_guid,
                mass_guid: geometry_guid(ctx, records.design),
                accuracy: 0.0,
                sheet: body.sheet,
                ordinal,
                design: true,
            },
            Side::Parameter => Self {
                record: records.parameter,
                property: Some((PARAMETER_PROPERTY, records.design)),
                owner: PARAMETER_OWNER,
                scene: records.parameter_scene[0],
                guid: &body.parameter_guid,
                mass_guid: ctx
                    .values
                    .lower_guid(&format!("mass-block-{}", records.parameter)),
                accuracy: -1.0,
                sheet: body.sheet,
                ordinal,
                design: false,
            },
        }
    }
}

/// A body-reference record in the form with no appearance entry, mass
/// properties of zero, and the appearance `PrismOpaque`.
fn reference(
    ctx: &Context<'_>,
    out: &mut DesignRecords,
    form: &Reference<'_>,
    assets: &Assets,
) -> Result<(), CodecError> {
    let Reference {
        record,
        property,
        owner,
        scene: node,
        guid,
        accuracy,
        ordinal,
        ..
    } = *form;
    let mass_guid = &form.mass_guid;
    out.record(&ctx.class_of(record)?, record);
    match property {
        None => {
            out.zeros(10);
        }
        Some((property, other)) => {
            out.zeros(9)
                .byte(1)
                .u32(1)
                .lp_ascii(property)?
                .lp_ascii("PassiveRefMetaType")?
                .reference(other);
        }
    }
    out.lp_utf16(&format!("0_{record}"))?
        .u32(if form.sheet { 4 } else { 2 })
        .f64(0.0)
        .f64(accuracy)
        .f64(0.0)
        .f64(0.0)
        .f64(0.0)
        .f64(accuracy);
    for _ in 0..6 {
        out.f64(0.0);
    }
    out.lp_utf16(mass_guid)?
        .f64(0.0)
        .f64(accuracy)
        .u32(ordinal)
        .byte(1)
        .u32(0)
        .u32(PRESENTATION_WORD)
        .byte(1)
        .byte(u8::from(form.design));
    for _ in 0..4 {
        out.f64(-1.0);
    }
    out.f32(1.0)
        .zeros(10)
        .lp_utf16(guid)?
        .reference(ASSET_MANAGER)
        .lp_utf16(&assets.material)?
        .lp_utf16(MATERIAL_LIBRARY)?
        .u32(0)
        .lp_utf16(MATERIAL)?
        .reference(owner)
        .byte(0)
        .reference(node)
        .u32(0);
    for channel in APPEARANCE_COLOUR {
        out.f32(channel);
    }
    out.reference(ASSET_MANAGER)
        .byte(1)
        .lp_utf16(&assets.appearance)?
        .lp_utf16(APPEARANCE_LIBRARY)?
        .u32(0)
        .lp_utf16(APPEARANCE)?
        .u32(0)
        .f32(1.0)
        .byte(1)
        .u32(0)
        .reference(owner);
    Ok(())
}

/// The Scene node, box and auxiliary record `[node, box, auxiliary]` of a
/// body-reference record, each storing `scene`.
fn scene_records(
    ctx: &Context<'_>,
    out: &mut DesignRecords,
    [node, scene_box, auxiliary]: [u32; 3],
    scene: &[f64; 6],
) -> Result<(), CodecError> {
    ctx.templated(
        out,
        node,
        &layout::base_feature_scene_node::TEMPLATE,
        &layout::base_feature_scene_node::UNSTATED,
        &[
            (
                layout::base_feature_scene_node::BOX_RECORD,
                Bind::Ref(scene_box),
            ),
            (
                layout::base_feature_scene_node::AUXILIARY,
                Bind::Ref(auxiliary),
            ),
            (layout::base_feature_scene_node::BOX, Bind::F64s(scene)),
        ],
    )?;
    ctx.templated(
        out,
        scene_box,
        &layout::base_feature_scene_box::TEMPLATE,
        &layout::base_feature_scene_box::UNSTATED,
        &[(layout::base_feature_scene_box::BOX, Bind::F64s(scene))],
    )?;
    ctx.templated(
        out,
        auxiliary,
        &layout::base_feature_scene_auxiliary::TEMPLATE,
        &layout::base_feature_scene_auxiliary::UNSTATED,
        &[
            (
                layout::base_feature_scene_auxiliary::RECORD_0,
                Bind::Ref(ASSET_SETTINGS),
            ),
            (layout::base_feature_scene_auxiliary::BOX, Bind::F64s(scene)),
        ],
    )?;
    Ok(())
}
