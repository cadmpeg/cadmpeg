// SPDX-License-Identifier: Apache-2.0
//! The component records of a Base Feature design: the Protein asset
//! manager (record 1), the component aggregate (record 3), the origin point,
//! axes and planes (records 25 to 31), and the root instance (record 43)
//! (f3d.md §3.1, "Component aggregate", "Origin records", "Root instance",
//! "Protein asset manager").

use cadmpeg_core::CodecError;

use super::super::frames::{Bind, DesignRecords};
use super::segment::SegmentKind;
use super::{
    count, Context, ACT_COMPONENT, AGGREGATE, ASSET_MANAGER, ASSET_SETTINGS, CONTEXT, DESIGN_OWNER,
    ORIGIN, PARAMETER_COLLECTION, PARAMETER_OWNER, RECORD_23, ROOT_INSTANCE, SKETCHES,
};
use crate::layout;

/// The ancestor type of the aggregate's paired header.
const AGGREGATE_BASE: &str = "42054630-20A0-40E1-B969-CFE9E742F5C9";
/// The physical-material library of the aggregate and the body references.
pub(super) const MATERIAL_LIBRARY: &str = "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C";
/// The physical material of the aggregate and the body references.
pub(super) const MATERIAL: &str = "PrismMaterial-022";
/// The component colour: one value of the 0.01 grid Fusion's components use.
const COLOUR: [f32; 4] = [1.0, 0.69, 0.7, 1.0];
/// The next value of the persistent-identity counter: one above the
/// `Base Feature`'s identity `300`.
const NEXT_IDENTITY: u64 = 301;
/// The ACT instance root.
pub(super) const ACT_INSTANCE_ROOT: u32 = 6;

/// The instance GUIDs of the Protein package's physical material and
/// appearance, and the package's basename.
pub(super) struct Assets {
    pub(super) material: String,
    pub(super) appearance: String,
    pub(super) package: String,
}

impl Assets {
    pub(super) fn new(ctx: &Context<'_>) -> Self {
        Self {
            material: ctx.values.guid("physical-material"),
            appearance: ctx.values.guid("appearance"),
            package: format!(
                "ProteinAsset.{}.protein",
                ctx.values.lower_guid("protein-package")
            ),
        }
    }
}

pub(super) fn write(ctx: &Context<'_>, out: &mut DesignRecords) -> Result<(), CodecError> {
    let assets = Assets::new(ctx);
    let bodies = ctx.body_count()?;
    let references = |extra: u32| {
        bodies
            .checked_mul(2)
            .and_then(|twice| twice.checked_add(extra))
            .ok_or_else(|| CodecError::NotImplemented("Base Feature body count".into()))
    };
    ctx.templated(
        out,
        ASSET_MANAGER,
        &layout::base_feature_protein_asset_manager::TEMPLATE,
        &layout::base_feature_protein_asset_manager::UNSTATED,
        &[
            (
                layout::base_feature_protein_asset_manager::MATERIAL_GUID,
                Bind::Utf16(&assets.material),
            ),
            (
                layout::base_feature_protein_asset_manager::MATERIAL_REFERENCES,
                Bind::U32(references(1)?),
            ),
            (
                layout::base_feature_protein_asset_manager::APPEARANCE_GUID,
                Bind::Utf16(&assets.appearance),
            ),
            (
                layout::base_feature_protein_asset_manager::APPEARANCE_REFERENCES,
                Bind::U32(references(0)?),
            ),
            (
                layout::base_feature_protein_asset_manager::BASENAME,
                Bind::Units(&assets.package),
            ),
        ],
    )?;
    aggregate(ctx, out, &assets)?;
    origin(ctx, out)?;
    let instance = [
        ctx.values.lower_guid("root-instance-a"),
        ctx.values.lower_guid("root-instance-b"),
    ];
    ctx.templated(
        out,
        ROOT_INSTANCE,
        &layout::base_feature_root_instance::TEMPLATE,
        &layout::base_feature_root_instance::UNSTATED,
        &[
            (
                layout::base_feature_root_instance::AGGREGATE,
                Bind::Ref(AGGREGATE),
            ),
            (
                layout::base_feature_root_instance::GUID_A,
                Bind::Utf16(&instance[0]),
            ),
            (
                layout::base_feature_root_instance::GUID_B,
                Bind::Utf16(&instance[1]),
            ),
            (
                layout::base_feature_root_instance::ACT_INSTANCE_ROOT,
                Bind::Cross {
                    target: ACT_INSTANCE_ROOT,
                    segment: SegmentKind::Act.id(),
                },
            ),
            (
                layout::base_feature_root_instance::AGGREGATE_REPEAT,
                Bind::Ref(AGGREGATE),
            ),
            (
                layout::base_feature_root_instance::RECORD_0,
                Bind::Ref(ASSET_SETTINGS),
            ),
        ],
    )?;
    Ok(())
}

/// The component aggregate with no property group: the prefix through the
/// code-unit count of the component name, the name, a zero byte and the
/// root instance.
fn aggregate(
    ctx: &Context<'_>,
    out: &mut DesignRecords,
    assets: &Assets,
) -> Result<(), CodecError> {
    use layout::base_feature_component_aggregate_prefix as p;
    let paired = ctx.class(AGGREGATE_BASE)?;
    let guids = [
        ctx.values.lower_guid("component-a"),
        ctx.values.lower_guid("component-b"),
    ];
    let name = &ctx.values.name;
    let reference = |offset: usize, record: u32| (offset, Bind::Ref(record));
    ctx.templated(
        out,
        AGGREGATE,
        &p::TEMPLATE,
        &p::UNSTATED,
        &[
            reference(p::RECORD_8EF00D65, 34),
            reference(p::RECORD_878C70A7, 36),
            reference(p::RECORD_1E650EA7, 40),
            reference(p::RECORD_F670456D, RECORD_23),
            reference(p::RECORD_B4CDE096, 162),
            reference(p::ORIGIN_CONTAINER, ORIGIN),
            reference(p::RECORD_64435F6E, 32),
            reference(p::RECORD_BF7A6B8B, 33),
            (p::NEXT_PERSISTENT_IDENTITY, Bind::U64(NEXT_IDENTITY)),
            reference(p::RECORD_AD3F9695, 37),
            reference(p::PARAMETER_BODY_OWNER, PARAMETER_OWNER),
            reference(p::RECORD_91461D14, 39),
            (
                p::ARRANGEMENTS_CONTAINER_CODE_UNITS,
                Bind::Units("arrangementsContainer"),
            ),
            reference(p::RECORD_60F93DE2, 178),
            reference(p::TIMELINE_CONTEXT, CONTEXT),
            reference(p::RECORD_7E84430B, 41),
            reference(p::RECORD_FA21074E, 194),
            reference(p::RECORD_3978701C, 38),
            (p::PAIRED_HEADER, Bind::Paired(&paired)),
            reference(p::RECORD_0C5DAB90, 22),
            reference(p::RECORD_47C245A2, 21),
            (p::MINUS_ONE, Bind::F64s(&[-1.0; 4])),
            reference(p::RECORD_AC202226, 14),
            reference(p::RECORD_596C8B98, 13),
            reference(p::DESIGN_BODY_OWNER, DESIGN_OWNER),
            reference(p::RECORD_0DBF95ED, 15),
            reference(p::RECORD_FCB17D75, 12),
            reference(p::RECORD_77EF6384, 16),
            reference(p::PARAMETER_COLLECTION, PARAMETER_COLLECTION),
            reference(p::RECORD_30DCFC57, 20),
            reference(p::SKETCH_COLLECTION, SKETCHES),
            reference(p::RECORD_372234A0, 19),
            reference(p::RECORD_A8635B5F, 11),
            (p::COLOUR, Bind::F32s(&COLOUR)),
            reference(p::MATERIAL_ASSET_MANAGER, ASSET_MANAGER),
            (p::MATERIAL_GUID_CODE_UNITS, Bind::Units(&assets.material)),
            (
                p::MATERIAL_LIBRARY_CODE_UNITS,
                Bind::Units(MATERIAL_LIBRARY),
            ),
            (p::MATERIAL_NAME_CODE_UNITS, Bind::Units(MATERIAL)),
            reference(p::RECORD_F9CB8717, 5),
            (p::FIRST_LOWERCASE_GUID_CODE_UNITS, Bind::Units(&guids[0])),
            (
                p::ACT_COMPONENT_RECORD,
                Bind::Cross {
                    target: ACT_COMPONENT,
                    segment: SegmentKind::Act.id(),
                },
            ),
            reference(p::COMPONENT_NAMING_SPACE, super::NAMING_SPACE),
            (p::SECOND_LOWERCASE_GUID_CODE_UNITS, Bind::Units(&guids[1])),
            (
                p::COMPONENT_NAME_LENGTH,
                Bind::U32(count(name.encode_utf16().count())?),
            ),
        ],
    )?;
    for unit in name.encode_utf16() {
        out.raw(&unit.to_le_bytes());
    }
    out.byte(0).reference(ROOT_INSTANCE);
    Ok(())
}

/// The origin point, the axes `X`, `Y`, `Z` and the planes `XY`, `XZ`,
/// `YZ`, at the origin.
fn origin(ctx: &Context<'_>, out: &mut DesignRecords) -> Result<(), CodecError> {
    let zero = [0.0; 3];
    ctx.templated(
        out,
        25,
        &layout::base_feature_origin_point::TEMPLATE,
        &layout::base_feature_origin_point::UNSTATED,
        &[
            (layout::base_feature_origin_point::POINT, Bind::F64s(&zero)),
            (
                layout::base_feature_origin_point::NAME_CODE_UNITS,
                Bind::Units("O"),
            ),
            (
                layout::base_feature_origin_point::ASSET_SETTINGS,
                Bind::Ref(ASSET_SETTINGS),
            ),
            (
                layout::base_feature_origin_point::ORIGIN_CONTAINER,
                Bind::Ref(ORIGIN),
            ),
        ],
    )?;
    let axes = [
        ("X", [1.0, 0.0, 0.0]),
        ("Y", [0.0, 1.0, 0.0]),
        ("Z", [0.0, 0.0, 1.0]),
    ];
    for (index, (name, direction)) in (26..).zip(&axes) {
        ctx.templated(
            out,
            index,
            &layout::base_feature_origin_axis::TEMPLATE,
            &layout::base_feature_origin_axis::UNSTATED,
            &[
                (layout::base_feature_origin_axis::ORIGIN, Bind::F64s(&zero)),
                (
                    layout::base_feature_origin_axis::DIRECTION,
                    Bind::F64s(direction),
                ),
                (
                    layout::base_feature_origin_axis::NAME_CODE_UNITS,
                    Bind::Units(name),
                ),
                (
                    layout::base_feature_origin_axis::ASSET_SETTINGS,
                    Bind::Ref(ASSET_SETTINGS),
                ),
                (
                    layout::base_feature_origin_axis::ORIGIN_CONTAINER,
                    Bind::Ref(ORIGIN),
                ),
            ],
        )?;
    }
    let planes = [
        ("XY", [1.0, 0.0, -0.0], [-0.0, 1.0, 0.0]),
        ("XZ", [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ("YZ", [-0.0, 1.0, 0.0], [0.0, -0.0, 1.0]),
    ];
    for (index, (name, u, v)) in (29..).zip(&planes) {
        ctx.templated(
            out,
            index,
            &layout::base_feature_origin_plane::TEMPLATE,
            &layout::base_feature_origin_plane::UNSTATED,
            &[
                (layout::base_feature_origin_plane::ORIGIN, Bind::F64s(&zero)),
                (
                    layout::base_feature_origin_plane::U_DIRECTION,
                    Bind::F64s(u),
                ),
                (
                    layout::base_feature_origin_plane::V_DIRECTION,
                    Bind::F64s(v),
                ),
                (
                    layout::base_feature_origin_plane::NAME_CODE_UNITS,
                    Bind::Units(name),
                ),
                (
                    layout::base_feature_origin_plane::ASSET_SETTINGS,
                    Bind::Ref(ASSET_SETTINGS),
                ),
                (
                    layout::base_feature_origin_plane::ORIGIN_CONTAINER,
                    Bind::Ref(ORIGIN),
                ),
            ],
        )?;
    }
    Ok(())
}
