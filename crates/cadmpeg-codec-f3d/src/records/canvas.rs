// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use super::references::DesignClassTag;
use cadmpeg_ir::features::{FeatureUnitPlaneFrame, FinitePoint3};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::units::UnitVector3;
use serde::{Deserialize, Serialize};

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

/// Authored Canvas boundary segments in an admitted horizontal or vertical form.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DesignCanvasBounds {
    form: DesignCanvasBoundaryForm,
}
#[derive(Debug, Clone, Copy, PartialEq)]
enum DesignCanvasBoundaryForm {
    Horizontal([[Point2; 2]; 2]),
    Vertical([[Point2; 2]; 2]),
}
impl TryFrom<[[Point2; 2]; 2]> for DesignCanvasBounds {
    type Error = String;
    fn try_from(segments: [[Point2; 2]; 2]) -> Result<Self, Self::Error> {
        if segments.iter().flatten().any(|point| !point.is_finite()) {
            return Err("boundary_segments must contain finite coordinates".into());
        }
        let [[a, b], [c, d]] = segments;
        let close = |left: f64, right: f64| {
            (left - right).abs() <= 64.0 * f64::EPSILON * left.abs().max(right.abs()).max(1.0)
        };
        let horizontal = close(a.v, b.v)
            && close(c.v, d.v)
            && close(a.u, c.u)
            && close(b.u, d.u)
            && !close(a.v, c.v);
        let vertical = close(a.u, b.u)
            && close(c.u, d.u)
            && close(a.v, c.v)
            && close(b.v, d.v)
            && !close(a.u, c.u);
        let form = if horizontal {
            DesignCanvasBoundaryForm::Horizontal(segments)
        } else if vertical {
            DesignCanvasBoundaryForm::Vertical(segments)
        } else {
            return Err(
                "boundary_segments must form an admitted horizontal or vertical pair".into(),
            );
        };
        Ok(Self { form })
    }
}
impl DesignCanvasBounds {
    fn segments(self) -> [[Point2; 2]; 2] {
        match self.form {
            DesignCanvasBoundaryForm::Horizontal(segments)
            | DesignCanvasBoundaryForm::Vertical(segments) => segments,
        }
    }
    pub(crate) fn mirroring(self) -> (bool, bool) {
        match self.form {
            DesignCanvasBoundaryForm::Horizontal([[a, b], [c, _]]) => (a.u > b.u, a.v > c.v),
            DesignCanvasBoundaryForm::Vertical([[a, b], [c, _]]) => (a.u > c.u, a.v > b.v),
        }
    }
    pub(crate) fn extents(self) -> [Point2; 2] {
        let [[a, b], [c, d]] = self.segments();
        let (minimum, maximum) = [b, c, d]
            .into_iter()
            .fold((a, a), |(minimum, maximum), point| {
                (
                    Point2::new(minimum.u.min(point.u), minimum.v.min(point.v)),
                    Point2::new(maximum.u.max(point.u), maximum.v.max(point.v)),
                )
            });
        [minimum, maximum]
    }
}

/// Canvas geometry flags; all other prologue bytes are fixed zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesignCanvasPrologue {
    first_flag: bool,
    visible: bool,
}
impl TryFrom<[u8; 15]> for DesignCanvasPrologue {
    type Error = String;
    fn try_from(bytes: [u8; 15]) -> Result<Self, Self::Error> {
        if bytes[..10] != [0; 10]
            || !matches!(bytes[10], 0 | 1)
            || bytes[11..14] != [0; 3]
            || !matches!(bytes[14], 0 | 1)
        {
            return Err(
                "geometry_prologue must contain only its two boolean flags and fixed zero bytes"
                    .into(),
            );
        }
        Ok(Self {
            first_flag: bytes[10] != 0,
            visible: bytes[14] != 0,
        })
    }
}
impl DesignCanvasPrologue {
    pub(crate) fn visible(self) -> bool {
        self.visible
    }
}

const CANVAS_GEOMETRY_PREFIX_BYTES: u64 = 217;
const CANVAS_PAIRED_GEOMETRY_BYTES: u64 = 30;
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
        let frame_length = CANVAS_GEOMETRY_PREFIX_BYTES + 2 * u64::from(units);
        byte_offset
            .checked_add(frame_length)
            .and_then(|end| end.checked_add(CANVAS_PAIRED_GEOMETRY_BYTES))
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
        CANVAS_GEOMETRY_PREFIX_BYTES + 2 * u64_from_index(self.label.encode_utf16().count())
    }
    fn paired_byte_offset(&self) -> Option<u64> {
        self.byte_offset?.checked_add(self.frame_length())
    }
    fn scope_reference_offset(&self) -> Option<u64> {
        self.byte_offset?.checked_add(147)
    }
    fn visibility_offset(&self) -> Option<u64> {
        self.byte_offset?.checked_add(25)
    }
    fn paired_component_reference_offset(&self) -> Option<u64> {
        self.paired_byte_offset()?.checked_add(20)
    }
    fn boundary_coordinate_offsets(&self) -> Option<[u64; 8]> {
        let start = self.byte_offset?;
        Some([26, 34, 42, 50, 181, 189, 197, 205].map(|relative| start + relative))
    }
    fn second_boundary_present_offset(&self) -> Option<u64> {
        self.byte_offset?.checked_add(180)
    }
    fn plane_reference_offset(&self) -> Option<u64> {
        self.byte_offset?.checked_add(59)
    }
    fn component_reference_offset(&self) -> Option<u64> {
        self.byte_offset?.checked_add(158)
    }
    fn asset_reference_offset(&self) -> Option<u64> {
        self.byte_offset?.checked_add(170)
    }
    fn label_offset(&self) -> Option<u64> {
        self.byte_offset?.checked_add(CANVAS_GEOMETRY_PREFIX_BYTES)
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
impl DesignCanvasScopeForm {
    fn reference_offset(self) -> u64 {
        match self {
            Self::Compact => 22,
            Self::Expanded => 26,
        }
    }
}

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
            .and_then(|offset| offset.checked_add(CANVAS_PAIRED_GEOMETRY_BYTES))
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
    pub(crate) fn scope_byte_offset(&self) -> Option<u64> {
        self.asset_byte_offset()?
            .checked_add(self.asset.frame_length())
    }
    fn asset_byte_offset(&self) -> Option<u64> {
        self.geometry
            .paired_byte_offset()?
            .checked_add(CANVAS_PAIRED_GEOMETRY_BYTES)
    }
    fn asset_name_offset(&self) -> Option<u64> {
        self.asset_byte_offset()?
            .checked_add(CANVAS_IMAGE_ASSET_PREFIX_BYTES)
    }
    fn geometry_reference_offset(&self) -> Option<u64> {
        self.scope_byte_offset()?
            .checked_add(self.scope_form?.reference_offset())
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
impl DesignCanvasImage {
    /// The source offsets of a decoded image, as the `record_layout`
    /// pointers of its capture.
    pub(crate) fn source_offsets(&self) -> Vec<(&'static str, Option<u64>)> {
        const BOUNDARY: [&str; 8] = [
            "boundary_coordinate_offsets.0",
            "boundary_coordinate_offsets.1",
            "boundary_coordinate_offsets.2",
            "boundary_coordinate_offsets.3",
            "boundary_coordinate_offsets.4",
            "boundary_coordinate_offsets.5",
            "boundary_coordinate_offsets.6",
            "boundary_coordinate_offsets.7",
        ];
        let mut offsets = vec![
            (
                "scope_reference_offset",
                self.geometry.scope_reference_offset(),
            ),
            (
                "geometry_reference_offset",
                self.geometry_reference_offset(),
            ),
            ("geometry_byte_offset", self.geometry.byte_offset),
            ("visibility_offset", self.geometry.visibility_offset()),
            (
                "paired_geometry_byte_offset",
                self.geometry.paired_byte_offset(),
            ),
            (
                "paired_component_reference_offset",
                self.geometry.paired_component_reference_offset(),
            ),
            (
                "second_boundary_present_offset",
                self.geometry.second_boundary_present_offset(),
            ),
            (
                "plane_reference_offset",
                self.geometry.plane_reference_offset(),
            ),
            (
                "component_reference_offset",
                self.geometry.component_reference_offset(),
            ),
            (
                "asset_reference_offset",
                self.geometry.asset_reference_offset(),
            ),
            ("asset_byte_offset", self.asset_byte_offset()),
            ("asset_name_offset", self.asset_name_offset()),
            ("label_offset", self.geometry.label_offset()),
        ];
        if let Some(boundary) = self.geometry.boundary_coordinate_offsets() {
            offsets.extend(BOUNDARY.into_iter().zip(boundary.into_iter().map(Some)));
        }
        offsets
    }
}

#[cfg(test)]
mod tests;
