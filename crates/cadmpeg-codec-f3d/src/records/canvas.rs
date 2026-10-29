// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use super::references::DesignClassTag;
use cadmpeg_ir::features::{FeatureImageBounds, FeatureUnitPlaneFrame, FinitePoint3};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::units::UnitVector3;
use serde::{Deserialize, Serialize};

pub(crate) mod geometry_layout;
use geometry_layout::DesignCanvasGeometryLayout;

const DESIGN_CANVAS_LENGTH_TO_MM: f64 = 10.0;

/// Canvas opacity and source-space frame; the fixed payload is emitted from these values.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignCanvasGeometryPayload {
    opacity: f32,
    frame: FeatureUnitPlaneFrame,
}
impl TryFrom<&[u8]> for DesignCanvasGeometryPayload {
    type Error = String;
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != 77 {
            return Err("geometry_payload must contain 77 bytes".into());
        }
        let mut view = cadmpeg_core::decode::View::over_retained(bytes);
        let opacity = view
            .req_f32_le()
            .map_err(|error| format!("geometry_payload: {error:?}"))?;
        let reserved = view
            .req_u8()
            .map_err(|error| format!("geometry_payload: {error:?}"))?;
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) || reserved != 0 {
            return Err(
                "geometry_payload must contain normalized finite opacity and a zero reserved byte"
                    .into(),
            );
        }
        let mut vector = || -> Result<[f64; 3], String> {
            Ok([
                view.req_f64_le()
                    .map_err(|error| format!("geometry_payload: {error:?}"))?,
                view.req_f64_le()
                    .map_err(|error| format!("geometry_payload: {error:?}"))?,
                view.req_f64_le()
                    .map_err(|error| format!("geometry_payload: {error:?}"))?,
            ])
        };
        let [origin_x, origin_y, origin_z] = vector()?;
        let u_components = vector()?;
        let v_components = vector()?;
        let invalid_frame = || {
            "geometry_payload must contain a finite origin and an admitted orthonormal frame"
                .to_owned()
        };
        let origin = FinitePoint3::new(Point3::new(
            origin_x * DESIGN_CANVAS_LENGTH_TO_MM,
            origin_y * DESIGN_CANVAS_LENGTH_TO_MM,
            origin_z * DESIGN_CANVAS_LENGTH_TO_MM,
        ))
        .ok_or_else(invalid_frame)?;
        let u_axis = UnitVector3::new(Vector3::from(u_components)).ok_or_else(invalid_frame)?;
        let v_axis = UnitVector3::new(Vector3::from(v_components)).ok_or_else(invalid_frame)?;
        let frame =
            FeatureUnitPlaneFrame::from_parts(origin, u_axis, v_axis).ok_or_else(invalid_frame)?;
        Ok(Self { opacity, frame })
    }
}
impl DesignCanvasGeometryPayload {
    pub(crate) fn decoded(&self) -> (f32, FeatureUnitPlaneFrame) {
        (self.opacity, self.frame)
    }
}

const EPS_CANVAS_BOUNDARY_CLOSURE: f64 = 64.0 * f64::EPSILON;
const EPS_CANVAS_BOUNDARY_ORTHOGONAL: f64 = 64.0 * f64::EPSILON;

/// Ordered opposite image edges in the supporting plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DesignCanvasBounds {
    segments: [[Point2; 2]; 2],
}
impl TryFrom<[[Point2; 2]; 2]> for DesignCanvasBounds {
    type Error = String;
    fn try_from(segments: [[Point2; 2]; 2]) -> Result<Self, Self::Error> {
        if segments.iter().flatten().any(|point| !point.is_finite()) {
            return Err("boundary_segments must contain finite coordinates".into());
        }
        let [[origin_corner, width_corner], [height_corner, opposite_corner]] = segments;
        let width_delta = Point2::new(
            width_corner.u - origin_corner.u,
            width_corner.v - origin_corner.v,
        );
        let height_delta = Point2::new(
            height_corner.u - origin_corner.u,
            height_corner.v - origin_corner.v,
        );
        let width = width_delta.u.hypot(width_delta.v);
        let height = height_delta.u.hypot(height_delta.v);
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err("boundary_segments must have finite nonzero image axes".into());
        }
        let dot = (width_delta.u / width) * (height_delta.u / height)
            + (width_delta.v / width) * (height_delta.v / height);
        let close = |actual: f64, expected: f64| {
            expected.is_finite()
                && (actual - expected).abs()
                    <= EPS_CANVAS_BOUNDARY_CLOSURE * actual.abs().max(expected.abs()).max(1.0)
        };
        if dot.abs() > EPS_CANVAS_BOUNDARY_ORTHOGONAL
            || !close(opposite_corner.u, width_corner.u + height_delta.u)
            || !close(opposite_corner.v, width_corner.v + height_delta.v)
        {
            return Err("boundary_segments must form an ordered rectangle".into());
        }
        Ok(Self { segments })
    }
}
impl DesignCanvasBounds {
    fn segments(self) -> [[Point2; 2]; 2] {
        self.segments
    }

    pub(crate) fn image_placement(
        self,
        plane: FeatureUnitPlaneFrame,
    ) -> Result<(FeatureUnitPlaneFrame, FeatureImageBounds), String> {
        let [[origin_corner, width_corner], [height_corner, _]] = self.segments;
        let width_delta = Point2::new(
            width_corner.u - origin_corner.u,
            width_corner.v - origin_corner.v,
        );
        let height_delta = Point2::new(
            height_corner.u - origin_corner.u,
            height_corner.v - origin_corner.v,
        );
        let width = width_delta.u.hypot(width_delta.v);
        let height = height_delta.u.hypot(height_delta.v);
        let plane_u = *plane.u_axis().as_raw();
        let plane_v = *plane.v_axis().as_raw();
        let origin = plane
            .origin()
            .get()
            .translated(plane_u, origin_corner.u * DESIGN_CANVAS_LENGTH_TO_MM)
            .translated(plane_v, origin_corner.v * DESIGN_CANVAS_LENGTH_TO_MM);
        let frame = FeatureUnitPlaneFrame::new(
            origin,
            plane_u.scale(width_delta.u / width) + plane_v.scale(width_delta.v / width),
            plane_u.scale(height_delta.u / height) + plane_v.scale(height_delta.v / height),
        )
        .ok_or("Canvas image frame must be finite and orthonormal")?;
        let bounds = FeatureImageBounds::new([
            Point2::new(0.0, 0.0),
            Point2::new(
                width * DESIGN_CANVAS_LENGTH_TO_MM,
                height * DESIGN_CANVAS_LENGTH_TO_MM,
            ),
        ])
        .ok_or("Canvas image dimensions must be finite and positive")?;
        Ok((frame, bounds))
    }
}

/// Canvas geometry flags and their physical layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesignCanvasPrologue {
    layout: DesignCanvasGeometryLayout,
    first_flag: bool,
    visible: bool,
}
impl TryFrom<[u8; 15]> for DesignCanvasPrologue {
    type Error = String;
    fn try_from(bytes: [u8; 15]) -> Result<Self, Self::Error> {
        Self::from_layout(DesignCanvasGeometryLayout::Modern, &bytes)
    }
}
impl DesignCanvasPrologue {
    pub(crate) fn from_layout(
        layout: DesignCanvasGeometryLayout,
        bytes: &[u8],
    ) -> Result<Self, String> {
        let length = layout.offsets().first_boundary - 11;
        if bytes.len() != length || !matches!(bytes.last(), Some(0 | 1)) {
            return Err("geometry_prologue must match its layout and visibility Boolean".into());
        }
        let valid = match layout {
            DesignCanvasGeometryLayout::Legacy | DesignCanvasGeometryLayout::TypedLegacy => {
                bytes[..12] == [0; 12]
            }
            DesignCanvasGeometryLayout::Transitional
            | DesignCanvasGeometryLayout::TypedTransitional => {
                bytes[..10] == [0; 10] && bytes[10..13] == [1, 0, 0]
            }
            _ => bytes[..10] == [0; 10] && matches!(bytes[10], 0 | 1) && bytes[11..14] == [0; 3],
        };
        if !valid {
            return Err("geometry_prologue must contain its fixed bytes and Boolean flags".into());
        }
        Ok(Self {
            layout,
            first_flag: bytes[10] != 0,
            visible: bytes[length - 1] != 0,
        })
    }
    pub(crate) fn visible(self) -> bool {
        self.visible
    }
}

const CANVAS_IMAGE_ASSET_PREFIX_BYTES: u64 = 25;

/// Canvas geometry and its same-index closing record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignCanvasGeometry {
    class_tags: [String; 2],
    record_index: u32,
    byte_offset: Option<u64>,
    label: String,
    /// Flags from the fixed geometry prologue.
    pub(crate) prologue: DesignCanvasPrologue,
    /// Authored image boundaries and their orientation.
    pub(crate) boundary: DesignCanvasBounds,
    /// Opacity and source-space image frame.
    pub(crate) payload: DesignCanvasGeometryPayload,
}
impl DesignCanvasGeometry {
    pub(crate) fn new(
        class_tags: [String; 2],
        record_index: u32,
        byte_offset: u64,
        label: String,
        prologue: DesignCanvasPrologue,
        boundary: DesignCanvasBounds,
        payload: DesignCanvasGeometryPayload,
    ) -> Result<Self, String> {
        for (name, tag) in ["geometry_class_tag", "paired_geometry_class_tag"]
            .into_iter()
            .zip(&class_tags)
        {
            if tag.is_empty() || !tag.bytes().all(|byte| byte.is_ascii_graphic()) {
                return Err(format!("{name} must contain printable ASCII characters"));
            }
        }
        if label.is_empty() {
            return Err("label must be nonempty".into());
        }
        let units = u32::try_from(label.encode_utf16().count())
            .map_err(|_| "label UTF-16 count must fit u32")?;
        let frame_length = u64_from_index(prologue.layout.offsets().prefix) + 2 * u64::from(units);
        byte_offset
            .checked_add(frame_length)
            .and_then(|end| end.checked_add(prologue.layout.paired_len()))
            .ok_or("geometry_byte_offset and label must leave a complete paired geometry record")?;
        Ok(Self {
            class_tags,
            record_index,
            byte_offset: Some(byte_offset),
            label,
            prologue,
            boundary,
            payload,
        })
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index
    }
    fn frame_length(&self) -> u64 {
        u64_from_index(self.prologue.layout.offsets().prefix)
            + 2 * u64_from_index(self.label.encode_utf16().count())
    }
    fn paired_byte_offset(&self) -> Option<u64> {
        self.byte_offset?.checked_add(self.frame_length())
    }
}

/// Canvas image-asset record with a nonempty UTF-16 name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignCanvasAsset {
    class_tag: DesignClassTag,
    record_index: u32,
    name: String,
}
impl DesignCanvasAsset {
    pub(crate) fn new(
        class_tag: DesignClassTag,
        record_index: u32,
        name: String,
    ) -> Result<Self, String> {
        if name.is_empty() {
            return Err("asset_name must be nonempty".into());
        }
        u32::try_from(name.encode_utf16().count())
            .map_err(|_| "asset_name UTF-16 count must fit u32")?;
        Ok(Self {
            class_tag,
            record_index,
            name,
        })
    }
    fn frame_length(&self) -> u64 {
        CANVAS_IMAGE_ASSET_PREFIX_BYTES + 2 * u64_from_index(self.name.encode_utf16().count())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesignCanvasScopeForm {
    Compact,
    Expanded,
}
impl DesignCanvasScopeForm {}

/// Exact image-plane binding owned by one Design `Canvas` scope.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignCanvasImageWire")]
pub(crate) struct DesignCanvasImage {
    /// Globally unique native binding identity.
    pub(crate) id: String,
    /// Owning Canvas scope.
    pub(crate) scope_record_index: u32,
    /// Supporting construction-plane entity.
    pub(crate) plane_entity_suffix: u32,
    /// Component entity owning the Canvas.
    pub(crate) component_entity_suffix: u32,
    geometry: DesignCanvasGeometry,
    asset: DesignCanvasAsset,
    scope_form: Option<DesignCanvasScopeForm>,
}

#[cfg(test)]
thread_local! {
    pub(super) static CANVAS_IMAGE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignCanvasImage {
    fn clone(&self) -> Self {
        CANVAS_IMAGE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            scope_record_index: self.scope_record_index,
            plane_entity_suffix: self.plane_entity_suffix,
            component_entity_suffix: self.component_entity_suffix,
            geometry: self.geometry.clone(),
            asset: self.asset.clone(),
            scope_form: self.scope_form,
        }
    }
}
impl DesignCanvasImage {
    pub(crate) fn new(
        id: String,
        scope_record_index: u32,
        geometry_reference_offset: u64,
        geometry: DesignCanvasGeometry,
        asset: DesignCanvasAsset,
        plane_entity_suffix: u32,
        component_entity_suffix: u32,
    ) -> Result<Self, String> {
        if geometry.record_index == asset.record_index {
            return Err("asset_record_index must differ from geometry_record_index".into());
        }
        let scope_offset = geometry
            .paired_byte_offset()
            .and_then(|offset| offset.checked_add(geometry.prologue.layout.paired_len()))
            .and_then(|offset| offset.checked_add(asset.frame_length()))
            .ok_or("asset_name must end at a representable scope byte offset")?;
        let scope_form = match geometry_reference_offset.checked_sub(scope_offset) {
            Some(22) => DesignCanvasScopeForm::Compact,
            Some(26) => DesignCanvasScopeForm::Expanded,
            _ => return Err("geometry_reference_offset must locate a compact or expanded Canvas scope reference".into()),
        };
        geometry_reference_offset
            .checked_add(4)
            .ok_or("geometry_reference_offset must leave a complete u32 reference")?;
        Ok(Self {
            id,
            scope_record_index,
            plane_entity_suffix,
            component_entity_suffix,
            geometry,
            asset,
            scope_form: Some(scope_form),
        })
    }
    pub(crate) fn geometry(&self) -> &DesignCanvasGeometry {
        &self.geometry
    }
    pub(crate) fn asset_name(&self) -> &str {
        &self.asset.name
    }
}

/// Canvas content without encoded payloads or source-layout columns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignCanvasImageWire {
    id: String,
    scope_record_index: u32,
    geometry_class_tag: String,
    geometry_record_index: u32,
    geometry_first_flag: bool,
    visible: bool,
    paired_geometry_class_tag: String,
    boundary_segments: [[Point2; 2]; 2],
    plane_entity_suffix: u32,
    component_entity_suffix: u32,
    asset_class_tag: String,
    asset_record_index: u32,
    asset_name: String,
    label: String,
    opacity: f32,
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
}
impl Serialize for DesignCanvasImage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            scope_record_index: u32,
            geometry_class_tag: &'a str,
            geometry_record_index: u32,
            geometry_first_flag: bool,
            visible: bool,
            paired_geometry_class_tag: &'a str,
            boundary_segments: [[Point2; 2]; 2],
            plane_entity_suffix: u32,
            component_entity_suffix: u32,
            asset_class_tag: &'a str,
            asset_record_index: u32,
            asset_name: &'a str,
            label: &'a str,
            opacity: f32,
            origin: Point3,
            u_axis: Vector3,
            v_axis: Vector3,
        }
        let (opacity, frame) = self.geometry.payload.decoded();
        WireRef {
            id: &self.id,
            scope_record_index: self.scope_record_index,
            geometry_class_tag: &self.geometry.class_tags[0],
            geometry_record_index: self.geometry.record_index,
            geometry_first_flag: self.geometry.prologue.first_flag,
            visible: self.geometry.prologue.visible,
            paired_geometry_class_tag: &self.geometry.class_tags[1],
            boundary_segments: self.geometry.boundary.segments(),
            plane_entity_suffix: self.plane_entity_suffix,
            component_entity_suffix: self.component_entity_suffix,
            asset_class_tag: self.asset.class_tag.as_str(),
            asset_record_index: self.asset.record_index,
            asset_name: &self.asset.name,
            label: &self.geometry.label,
            opacity,
            origin: frame.origin().get(),
            u_axis: *frame.u_axis().as_raw(),
            v_axis: *frame.v_axis().as_raw(),
        }
        .serialize(serializer)
    }
}
impl TryFrom<DesignCanvasImageWire> for DesignCanvasImage {
    type Error = String;
    fn try_from(wire: DesignCanvasImageWire) -> Result<Self, String> {
        if !wire.opacity.is_finite() || !(0.0..=1.0).contains(&wire.opacity) {
            return Err("opacity must be finite and normalized".into());
        }
        let origin = FinitePoint3::new(wire.origin).ok_or("origin must be finite")?;
        let u_axis = UnitVector3::new(wire.u_axis).ok_or("u_axis must be finite and nonzero")?;
        let v_axis = UnitVector3::new(wire.v_axis).ok_or("v_axis must be finite and nonzero")?;
        let frame = FeatureUnitPlaneFrame::from_parts(origin, u_axis, v_axis)
            .ok_or("canvas axes must form an orthonormal frame")?;
        for (name, tag) in [
            ("geometry_class_tag", &wire.geometry_class_tag),
            ("paired_geometry_class_tag", &wire.paired_geometry_class_tag),
        ] {
            if tag.is_empty() || !tag.bytes().all(|byte| byte.is_ascii_graphic()) {
                return Err(format!("{name} must contain printable ASCII characters"));
            }
        }
        if wire.label.is_empty() {
            return Err("label must be nonempty".into());
        }
        u32::try_from(wire.label.encode_utf16().count())
            .map_err(|_| "label UTF-16 count must fit u32")?;
        let asset = DesignCanvasAsset::new(
            DesignClassTag::try_from(wire.asset_class_tag)?,
            wire.asset_record_index,
            wire.asset_name,
        )?;
        if asset.record_index == wire.geometry_record_index {
            return Err("asset_record_index must differ from geometry_record_index".into());
        }
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            plane_entity_suffix: wire.plane_entity_suffix,
            component_entity_suffix: wire.component_entity_suffix,
            geometry: DesignCanvasGeometry {
                class_tags: [wire.geometry_class_tag, wire.paired_geometry_class_tag],
                record_index: wire.geometry_record_index,
                byte_offset: None,
                label: wire.label,
                prologue: DesignCanvasPrologue {
                    layout: DesignCanvasGeometryLayout::Modern,
                    first_flag: wire.geometry_first_flag,
                    visible: wire.visible,
                },
                boundary: DesignCanvasBounds::try_from(wire.boundary_segments)?,
                payload: DesignCanvasGeometryPayload {
                    opacity: wire.opacity,
                    frame,
                },
            },
            asset,
            scope_form: None,
        })
    }
}
#[cfg(test)]
impl From<DesignCanvasImage> for DesignCanvasImageWire {
    fn from(image: DesignCanvasImage) -> Self {
        let (opacity, frame) = image.geometry.payload.decoded();
        Self {
            id: image.id,
            scope_record_index: image.scope_record_index,
            geometry_class_tag: image.geometry.class_tags[0].clone(),
            geometry_record_index: image.geometry.record_index,
            geometry_first_flag: image.geometry.prologue.first_flag,
            visible: image.geometry.prologue.visible,
            paired_geometry_class_tag: image.geometry.class_tags[1].clone(),
            boundary_segments: image.geometry.boundary.segments(),
            plane_entity_suffix: image.plane_entity_suffix,
            component_entity_suffix: image.component_entity_suffix,
            asset_class_tag: image.asset.class_tag.into(),
            asset_record_index: image.asset.record_index,
            asset_name: image.asset.name,
            label: image.geometry.label,
            opacity,
            origin: frame.origin().get(),
            u_axis: *frame.u_axis().as_raw(),
            v_axis: *frame.v_axis().as_raw(),
        }
    }
}
impl DesignCanvasImage {}

#[cfg(test)]
mod tests;
