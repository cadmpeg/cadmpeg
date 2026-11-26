// SPDX-License-Identifier: Apache-2.0
//! The view records of a Base Feature design: `AssetSettings` (record 0),
//! the named-view list and its four views (records 163 to 171), and the
//! lighting environment (record 172) (f3d.md §3.1, "Named views",
//! "`AssetSettings`", "Lighting environment").

use cadmpeg_core::CodecError;
use cadmpeg_ir::up_axis::UpAxis;

use super::super::frames::{Bind, DesignRecords};
use super::values::BodyBox;
use super::{Context, ASSET_SETTINGS, LIGHTING, NAMED_VIEWS, ZERO_RECORD};
use crate::layout;

/// The extents of the `TOP`, `FRONT` and `RIGHT` cameras.
const FIXED_EXTENT: f64 = 0.4;
/// `HOME` extent over the projected box height.
const HEIGHT_MARGIN: f64 = 1.095;
/// `HOME` extent over the projected box width.
const WIDTH_MARGIN: f64 = 1.08;
/// The eye distance over the larger extent: the larger extent fills the
/// field of view `2 atan(0.2)`.
const EYE_DISTANCE: f64 = 2.5;
/// The scene radius over half the box diagonal.
const RADIUS_MARGIN: f64 = 1.0023;
/// The scene scale `AssetSettings` stores.
const SCENE_SCALE: [f64; 3] = [1.8; 3];
/// The front direction `AssetSettings` stores twice.
const FRONT: [f64; 3] = [0.0, 1.0, 0.0];
/// The up direction of the `FRONT`, `RIGHT` and `HOME` cameras.
const UP: [f64; 3] = [0.0, 0.0, 1.0];
/// The front direction `AssetSettings` stores twice in a Y-up document.
const FRONT_Y_UP: [f64; 3] = [0.0, 0.0, -1.0];
/// The up direction `AssetSettings` stores twice in a Y-up document.
const UP_Y_UP: [f64; 3] = [0.0, 1.0, 0.0];

/// `v`, a Z-up direction or offset, as the document of `axis` stores it: the
/// Y-up form is the Z-up form turned by -90 degrees about X,
/// `(x, y, z) -> (x, z, -y)`.
fn oriented(axis: Option<UpAxis>, v: [f64; 3]) -> [f64; 3] {
    match axis {
        Some(UpAxis::Y) => [v[0], v[2], 0.0 - v[1]],
        Some(UpAxis::Z) | None => v,
    }
}

/// The (front, up) pair `AssetSettings` stores twice for `axis`: the Y-up
/// form for `Y`, the Z-up form for `Z` or no stated axis.
pub(super) fn settings_pair(axis: Option<UpAxis>) -> ([f64; 3], [f64; 3]) {
    match axis {
        Some(UpAxis::Y) => (FRONT_Y_UP, UP_Y_UP),
        Some(UpAxis::Z) | None => (FRONT, UP),
    }
}
/// The box a design with no measured body box is fitted to.
const UNIT_BOX: BodyBox = BodyBox {
    min: [0.0; 3],
    max: [1.0; 3],
};

/// A camera's view block: extents, eye, target and up direction.
pub(super) struct View {
    pub(super) extents: [f64; 2],
    pub(super) eye: [f64; 3],
    pub(super) target: [f64; 3],
    pub(super) up: [f64; 3],
}

impl View {
    /// The `HOME` view of `fitted`: its centre seen along `(1, -1, 1)` with
    /// up `(0, 0, 1)`, the extents fitting its projection.
    pub(super) fn home(fitted: &BodyBox, axis: Option<UpAxis>) -> Self {
        let sqrt3 = 3f64.sqrt();
        let normal = oriented(axis, [1.0 / sqrt3, -1.0 / sqrt3, 1.0 / sqrt3]);
        let sqrt6 = 6f64.sqrt();
        let up = oriented(axis, [-1.0 / sqrt6, 1.0 / sqrt6, 2.0 / sqrt6]);
        let right = oriented(axis, [1.0 / 2f64.sqrt(), 1.0 / 2f64.sqrt(), 0.0]);
        let corners = (0..8).map(|corner: u8| {
            [0, 1, 2].map(|axis| {
                if (corner >> axis) & 1 == 0 {
                    fitted.min[axis]
                } else {
                    fitted.max[axis]
                }
            })
        });
        let span = |direction: [f64; 3]| {
            let (low, high) = corners
                .clone()
                .map(|corner| dot(corner, direction))
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| {
                    (low.min(value), high.max(value))
                });
            high - low
        };
        let extents = [HEIGHT_MARGIN * span(up), WIDTH_MARGIN * span(right)];
        let distance = EYE_DISTANCE * extents[0].max(extents[1]);
        let target = fitted.centre();
        Self {
            extents,
            eye: [0, 1, 2].map(|axis| target[axis] + distance * normal[axis]),
            target,
            up: oriented(axis, UP),
        }
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The box `HOME` fits: the union of the measured body boxes.
pub(super) fn fitted_box(ctx: &Context<'_>) -> Option<BodyBox> {
    ctx.values
        .boxes
        .iter()
        .flatten()
        .copied()
        .reduce(|union, next| union.union(&next))
}

pub(super) fn write(ctx: &Context<'_>, out: &mut DesignRecords) -> Result<(), CodecError> {
    let fitted = fitted_box(ctx).unwrap_or(UNIT_BOX);
    let axis = ctx.values.up_axis;
    let home = View::home(&fitted, axis);
    let fixed = |target: [f64; 3], up: [f64; 3]| View {
        extents: [FIXED_EXTENT; 2],
        eye: [0.0; 3],
        target: oriented(axis, target),
        up: oriented(axis, up),
    };
    let views = [
        ("TOP", fixed([-0.0, -0.0, -1.0], FRONT)),
        ("FRONT", fixed(FRONT, UP)),
        ("RIGHT", fixed([-1.0, 0.0, 0.0], UP)),
        ("HOME", home),
    ];
    let names = [165, 167, 169, 171];
    ctx.templated(
        out,
        NAMED_VIEWS,
        &layout::base_feature_named_view_list::TEMPLATE,
        &layout::base_feature_named_view_list::UNSTATED,
        &[
            (
                layout::base_feature_named_view_list::TOP,
                Bind::Ref(names[0]),
            ),
            (
                layout::base_feature_named_view_list::FRONT,
                Bind::Ref(names[1]),
            ),
            (
                layout::base_feature_named_view_list::RIGHT,
                Bind::Ref(names[2]),
            ),
            (
                layout::base_feature_named_view_list::HOME,
                Bind::Ref(names[3]),
            ),
        ],
    )?;
    for ((name, view), view_name) in views.iter().zip(names) {
        let camera = view_name - 1;
        ctx.templated(
            out,
            camera,
            &layout::base_feature_camera::TEMPLATE,
            &layout::base_feature_camera::UNSTATED,
            &[
                (
                    layout::base_feature_camera::EXTENT_0,
                    Bind::F64(view.extents[0]),
                ),
                (
                    layout::base_feature_camera::EXTENT_1,
                    Bind::F64(view.extents[1]),
                ),
                (layout::base_feature_camera::EYE, Bind::F64s(&view.eye)),
                (
                    layout::base_feature_camera::TARGET,
                    Bind::F64s(&view.target),
                ),
                (layout::base_feature_camera::UP, Bind::F64s(&view.up)),
            ],
        )?;
        ctx.templated(
            out,
            view_name,
            &layout::base_feature_view_name_prefix::TEMPLATE,
            &layout::base_feature_view_name_prefix::UNSTATED,
            &[],
        )?;
        out.lp_utf16(name)?.reference(camera).reference(NAMED_VIEWS);
    }
    let [_, _, _, (_, home)] = &views;
    let diagonal = (0..3)
        .map(|axis| (fitted.max[axis] - fitted.min[axis]).powi(2))
        .sum::<f64>()
        .sqrt();
    let radius = RADIUS_MARGIN * diagonal / 2.0;
    let settings = |block: usize| {
        [
            (block, Bind::F64(home.extents[0])),
            (block + 8, Bind::F64(home.extents[1])),
            (block + 16, Bind::F64s(&home.eye)),
            (block + 44, Bind::F64s(&home.target)),
            (block + 68, Bind::F64s(&home.up)),
        ]
    };
    let pair = settings_pair(ctx.values.up_axis);
    let mut binds = vec![(
        layout::base_feature_asset_settings::UNIT_SYSTEM,
        Bind::Ref(ctx.values.unit_system),
    )];
    binds.extend(settings(
        layout::base_feature_asset_settings::VIEW_A_EXTENT_0,
    ));
    binds.extend(settings(
        layout::base_feature_asset_settings::VIEW_B_EXTENT_0,
    ));
    binds.extend([
        (
            layout::base_feature_asset_settings::FRONT_A,
            Bind::F64s(&pair.0),
        ),
        (
            layout::base_feature_asset_settings::UP_A,
            Bind::F64s(&pair.1),
        ),
        (
            layout::base_feature_asset_settings::NAMED_VIEWS,
            Bind::Ref(NAMED_VIEWS),
        ),
        (
            layout::base_feature_asset_settings::LIGHTING,
            Bind::Ref(LIGHTING),
        ),
        (
            layout::base_feature_asset_settings::SCENE_CENTRE,
            Bind::F64s(&home.target),
        ),
        (
            layout::base_feature_asset_settings::SCENE_SCALE,
            Bind::F64s(&SCENE_SCALE),
        ),
        (
            layout::base_feature_asset_settings::SCENE_RADIUS,
            Bind::F64(radius),
        ),
        (
            layout::base_feature_asset_settings::FRONT_B,
            Bind::F64s(&pair.0),
        ),
        (
            layout::base_feature_asset_settings::UP_B,
            Bind::F64s(&pair.1),
        ),
        (
            layout::base_feature_asset_settings::RECORD_2,
            Bind::Ref(ZERO_RECORD),
        ),
    ]);
    ctx.templated(
        out,
        ASSET_SETTINGS,
        &layout::base_feature_asset_settings::TEMPLATE,
        &layout::base_feature_asset_settings::UNSTATED,
        &binds,
    )?;
    let grey = [178.0f32 / 255.0; 3];
    ctx.templated(
        out,
        LIGHTING,
        &layout::base_feature_lighting_environment::TEMPLATE,
        &layout::base_feature_lighting_environment::UNSTATED,
        &[
            (
                layout::base_feature_lighting_environment::NAME,
                Bind::Utf16("Sharp Highlights"),
            ),
            (
                layout::base_feature_lighting_environment::COLOUR,
                Bind::F32s(&grey),
            ),
            (
                layout::base_feature_lighting_environment::RECORD_0,
                Bind::Ref(ASSET_SETTINGS),
            ),
            (
                layout::base_feature_lighting_environment::ENVIRONMENT,
                Bind::Utf16("Studio1"),
            ),
            (
                layout::base_feature_lighting_environment::ENTRY_175,
                Bind::Ref(175),
            ),
            (
                layout::base_feature_lighting_environment::ENTRY_176,
                Bind::Ref(176),
            ),
            (
                layout::base_feature_lighting_environment::ENTRY_173,
                Bind::Ref(173),
            ),
            (
                layout::base_feature_lighting_environment::ENTRY_174,
                Bind::Ref(174),
            ),
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::up_axis::UpAxis;

    use super::settings_pair;

    #[test]
    fn the_stored_pair_follows_the_document_up_axis() {
        let z_up = ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        let y_up = ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
        assert_eq!(settings_pair(Some(UpAxis::Y)), y_up);
        assert_eq!(settings_pair(Some(UpAxis::Z)), z_up);
        assert_eq!(settings_pair(None), z_up);
    }
}
