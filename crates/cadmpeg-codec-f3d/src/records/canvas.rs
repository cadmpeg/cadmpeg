// SPDX-License-Identifier: Apache-2.0

use super::DesignClassTag;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use serde::{Deserialize, Serialize};

const EPS_CANVAS_DECODE_GEOMETRY_PAYLOAD_E9: f64 = 1.0e-9;
const DESIGN_CANVAS_LENGTH_TO_MM: f64 = 10.0;

/// Canvas opacity and source-space frame; the fixed payload is emitted from these values.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignCanvasGeometryPayload {
    opacity: f32,
    origin_centimetres: [f64; 3],
    u_axis: Vector3,
    v_axis: Vector3,
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
        let origin_centimetres = vector()?;
        let u = vector()?;
        let v = vector()?;
        let u_axis = Vector3::new(u[0], u[1], u[2]);
        let v_axis = Vector3::new(v[0], v[1], v[2]);
        if !origin_centimetres.into_iter().all(f64::is_finite)
            || (u_axis.norm() - 1.0).abs() > EPS_CANVAS_DECODE_GEOMETRY_PAYLOAD_E9
            || (v_axis.norm() - 1.0).abs() > EPS_CANVAS_DECODE_GEOMETRY_PAYLOAD_E9
            || u_axis.dot(v_axis).abs() > EPS_CANVAS_DECODE_GEOMETRY_PAYLOAD_E9
        {
            return Err(
                "geometry_payload must contain a finite origin and an admitted orthonormal frame"
                    .into(),
            );
        }
        Ok(Self {
            opacity,
            origin_centimetres,
            u_axis,
            v_axis,
        })
    }
}
impl DesignCanvasGeometryPayload {
    pub fn decoded(&self) -> (f32, Point3, Vector3, Vector3) {
        (
            self.opacity,
            Point3::new(
                self.origin_centimetres[0] * DESIGN_CANVAS_LENGTH_TO_MM,
                self.origin_centimetres[1] * DESIGN_CANVAS_LENGTH_TO_MM,
                self.origin_centimetres[2] * DESIGN_CANVAS_LENGTH_TO_MM,
            ),
            self.u_axis,
            self.v_axis,
        )
    }
    pub fn bytes(&self) -> [u8; 77] {
        let mut bytes = [0; 77];
        bytes[..4].copy_from_slice(&self.opacity.to_le_bytes());
        let mut at = 5;
        for value in self
            .origin_centimetres
            .into_iter()
            .chain([self.u_axis.x, self.u_axis.y, self.u_axis.z])
            .chain([self.v_axis.x, self.v_axis.y, self.v_axis.z])
        {
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            at += 8;
        }
        bytes
    }
}

/// Authored Canvas boundary segments in an admitted horizontal or vertical form.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesignCanvasBounds {
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
        if segments
            .iter()
            .flatten()
            .any(|point| !point.u.is_finite() || !point.v.is_finite())
        {
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
    pub fn segments(self) -> [[Point2; 2]; 2] {
        match self.form {
            DesignCanvasBoundaryForm::Horizontal(segments)
            | DesignCanvasBoundaryForm::Vertical(segments) => segments,
        }
    }
    pub fn mirroring(self) -> (bool, bool) {
        match self.form {
            DesignCanvasBoundaryForm::Horizontal([[a, b], [c, _]]) => (a.u > b.u, a.v > c.v),
            DesignCanvasBoundaryForm::Vertical([[a, b], [c, _]]) => (a.u > c.u, a.v > b.v),
        }
    }
    pub fn extents(self) -> [Point2; 2] {
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
pub struct DesignCanvasPrologue {
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
    pub fn visible(self) -> bool {
        self.visible
    }
    pub fn bytes(self) -> [u8; 15] {
        let mut bytes = [0; 15];
        bytes[10] = u8::from(self.first_flag);
        bytes[14] = u8::from(self.visible);
        bytes
    }
}

const CANVAS_GEOMETRY_PREFIX_BYTES: u64 = 217;
const CANVAS_PAIRED_GEOMETRY_BYTES: u64 = 30;
const CANVAS_IMAGE_ASSET_PREFIX_BYTES: u64 = 25;

/// Canvas geometry and its same-index closing record.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignCanvasGeometry {
    class_tags: [String; 2],
    record_index: u32,
    byte_offset: u64,
    label: String,
    /// Flags from the fixed geometry prologue.
    pub prologue: DesignCanvasPrologue,
    /// Authored image boundaries and their orientation.
    pub boundary: DesignCanvasBounds,
    /// Opacity and source-space image frame.
    pub payload: DesignCanvasGeometryPayload,
}
impl DesignCanvasGeometry {
    pub fn new(
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
            byte_offset,
            label,
            prologue,
            boundary,
            payload,
        })
    }
    pub fn record_index(&self) -> u32 {
        self.record_index
    }
    fn frame_length(&self) -> u64 {
        CANVAS_GEOMETRY_PREFIX_BYTES + 2 * self.label.encode_utf16().count() as u64
    }
    fn paired_byte_offset(&self) -> u64 {
        self.byte_offset + self.frame_length()
    }
    fn scope_reference_offset(&self) -> u64 {
        self.byte_offset + 147
    }
    fn visibility_offset(&self) -> u64 {
        self.byte_offset + 25
    }
    fn paired_component_reference_offset(&self) -> u64 {
        self.paired_byte_offset() + 20
    }
    fn boundary_coordinate_offsets(&self) -> [u64; 8] {
        [26, 34, 42, 50, 181, 189, 197, 205].map(|relative| self.byte_offset + relative)
    }
    fn second_boundary_present_offset(&self) -> u64 {
        self.byte_offset + 180
    }
    fn plane_reference_offset(&self) -> u64 {
        self.byte_offset + 59
    }
    fn component_reference_offset(&self) -> u64 {
        self.byte_offset + 158
    }
    fn asset_reference_offset(&self) -> u64 {
        self.byte_offset + 170
    }
    fn label_offset(&self) -> u64 {
        self.byte_offset + CANVAS_GEOMETRY_PREFIX_BYTES
    }
}

/// Canvas image-asset record with a nonempty UTF-16 name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignCanvasAsset {
    class_tag: DesignClassTag,
    record_index: u32,
    name: String,
}
impl DesignCanvasAsset {
    pub fn new(class_tag: DesignClassTag, record_index: u32, name: String) -> Result<Self, String> {
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
        CANVAS_IMAGE_ASSET_PREFIX_BYTES + 2 * self.name.encode_utf16().count() as u64
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignCanvasImageWire", into = "DesignCanvasImageWire")]
pub struct DesignCanvasImage {
    /// Globally unique native binding identity.
    pub id: String,
    /// Owning Canvas scope.
    pub scope_record_index: u32,
    /// Supporting construction-plane entity.
    pub plane_entity_suffix: u32,
    /// Component entity owning the Canvas.
    pub component_entity_suffix: u32,
    geometry: DesignCanvasGeometry,
    asset: DesignCanvasAsset,
    scope_form: DesignCanvasScopeForm,
}
impl DesignCanvasImage {
    pub fn new(
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
            .checked_add(CANVAS_PAIRED_GEOMETRY_BYTES)
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
            scope_form,
        })
    }
    pub fn geometry(&self) -> &DesignCanvasGeometry {
        &self.geometry
    }
    pub fn asset_name(&self) -> &str {
        &self.asset.name
    }
    pub fn scope_byte_offset(&self) -> u64 {
        self.asset_byte_offset() + self.asset.frame_length()
    }
    fn asset_byte_offset(&self) -> u64 {
        self.geometry.paired_byte_offset() + CANVAS_PAIRED_GEOMETRY_BYTES
    }
    fn asset_name_offset(&self) -> u64 {
        self.asset_byte_offset() + CANVAS_IMAGE_ASSET_PREFIX_BYTES
    }
    fn geometry_reference_offset(&self) -> u64 {
        self.scope_byte_offset() + self.scope_form.reference_offset()
    }
}

/// Exact image-plane binding owned by one Design `Canvas` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct DesignCanvasImageWire {
    /// Globally unique deterministic identifier for this native binding.
    id: String,
    /// Canvas scope record index.
    scope_record_index: u32,
    /// Byte offset of the marked scope reference in the geometry record.
    scope_reference_offset: u64,
    /// Dynamic class tag of the primary geometry record.
    geometry_class_tag: String,
    /// Geometry record index.
    geometry_record_index: u32,
    /// Byte offset of the scope's marked geometry-record reference.
    geometry_reference_offset: u64,
    /// Byte offset of the primary geometry record.
    geometry_byte_offset: u64,
    /// Fixed geometry prologue immediately following the primary record header.
    geometry_prologue: [u8; 15],
    /// Whether the Canvas raster is visible.
    visible: bool,
    /// Byte offset of the visibility byte in the geometry prologue.
    visibility_offset: u64,
    /// Byte length from the primary geometry header to its paired header.
    geometry_frame_length: u64,
    /// Dynamic class tag of the paired geometry record.
    paired_geometry_class_tag: String,
    /// Byte offset of the paired geometry record.
    paired_geometry_byte_offset: u64,
    /// Byte offset of the paired record's marked component reference.
    paired_component_reference_offset: u64,
    /// Two opposite boundary segments in plane-local coordinates.
    boundary_segments: [[Point2; 2]; 2],
    /// Byte offsets of the eight boundary-coordinate f64 values.
    boundary_coordinate_offsets: [u64; 8],
    /// Byte offset of the presence marker preceding the second boundary segment.
    second_boundary_present_offset: u64,
    /// Design entity suffix of the supporting construction plane.
    plane_entity_suffix: u32,
    /// Byte offset of the marked construction-plane reference.
    plane_reference_offset: u64,
    /// Design entity suffix of the component owning the Canvas.
    component_entity_suffix: u32,
    /// Byte offset of the marked component reference.
    component_reference_offset: u64,
    /// Dynamic class tag of the standalone image-asset record.
    asset_class_tag: String,
    /// Image-asset record index.
    asset_record_index: u32,
    /// Byte offset of the marked image-asset reference.
    asset_reference_offset: u64,
    /// Byte offset of the image-asset record.
    asset_byte_offset: u64,
    /// Archive entry basename stored by the image-asset record.
    asset_name: String,
    /// Byte offset of the asset name's UTF-16LE code units.
    asset_name_offset: u64,
    /// Persistent Canvas label stored after the boundary segments.
    label: String,
    /// Byte offset of the label's UTF-16LE code units.
    label_offset: u64,
    /// Normalized raster opacity.
    opacity: f32,
    /// Image-plane origin in model-space millimeters.
    origin: Point3,
    /// Unit direction of increasing image u coordinate.
    u_axis: Vector3,
    /// Unit direction of increasing image v coordinate.
    v_axis: Vector3,
    /// Uninterpreted fixed geometry payload between the plane reference and scope link.
    geometry_payload: Vec<u8>,
}

impl TryFrom<DesignCanvasImageWire> for DesignCanvasImage {
    type Error = String;
    fn try_from(wire: DesignCanvasImageWire) -> Result<Self, Self::Error> {
        let geometry_prologue = DesignCanvasPrologue::try_from(wire.geometry_prologue)?;
        if geometry_prologue.visible() != wire.visible {
            return Err("visible must match geometry_prologue".into());
        }
        let geometry_payload =
            DesignCanvasGeometryPayload::try_from(wire.geometry_payload.as_slice())?;
        let (opacity, origin, u_axis, v_axis) = geometry_payload.decoded();
        if wire.opacity.to_bits() != opacity.to_bits() {
            return Err("opacity must match geometry_payload".into());
        }
        for (name, declared, decoded) in [
            (
                "origin",
                [wire.origin.x, wire.origin.y, wire.origin.z],
                [origin.x, origin.y, origin.z],
            ),
            (
                "u_axis",
                [wire.u_axis.x, wire.u_axis.y, wire.u_axis.z],
                [u_axis.x, u_axis.y, u_axis.z],
            ),
            (
                "v_axis",
                [wire.v_axis.x, wire.v_axis.y, wire.v_axis.z],
                [v_axis.x, v_axis.y, v_axis.z],
            ),
        ] {
            if declared
                .into_iter()
                .zip(decoded)
                .any(|(left, right)| left.to_bits() != right.to_bits())
            {
                return Err(format!("{name} must match geometry_payload"));
            }
        }
        let geometry = DesignCanvasGeometry::new(
            [wire.geometry_class_tag, wire.paired_geometry_class_tag],
            wire.geometry_record_index,
            wire.geometry_byte_offset,
            wire.label,
            geometry_prologue,
            DesignCanvasBounds::try_from(wire.boundary_segments)?,
            geometry_payload,
        )?;
        let asset = DesignCanvasAsset::new(
            DesignClassTag::try_from(wire.asset_class_tag)
                .map_err(|error| format!("asset_class_tag: {error}"))?,
            wire.asset_record_index,
            wire.asset_name,
        )?;
        let image = Self::new(
            wire.id,
            wire.scope_record_index,
            wire.geometry_reference_offset,
            geometry,
            asset,
            wire.plane_entity_suffix,
            wire.component_entity_suffix,
        )?;
        for (name, declared, derived) in [
            (
                "scope_reference_offset",
                wire.scope_reference_offset,
                image.geometry.scope_reference_offset(),
            ),
            (
                "visibility_offset",
                wire.visibility_offset,
                image.geometry.visibility_offset(),
            ),
            (
                "geometry_frame_length",
                wire.geometry_frame_length,
                image.geometry.frame_length(),
            ),
            (
                "paired_geometry_byte_offset",
                wire.paired_geometry_byte_offset,
                image.geometry.paired_byte_offset(),
            ),
            (
                "paired_component_reference_offset",
                wire.paired_component_reference_offset,
                image.geometry.paired_component_reference_offset(),
            ),
            (
                "second_boundary_present_offset",
                wire.second_boundary_present_offset,
                image.geometry.second_boundary_present_offset(),
            ),
            (
                "plane_reference_offset",
                wire.plane_reference_offset,
                image.geometry.plane_reference_offset(),
            ),
            (
                "component_reference_offset",
                wire.component_reference_offset,
                image.geometry.component_reference_offset(),
            ),
            (
                "asset_reference_offset",
                wire.asset_reference_offset,
                image.geometry.asset_reference_offset(),
            ),
            (
                "asset_byte_offset",
                wire.asset_byte_offset,
                image.asset_byte_offset(),
            ),
            (
                "asset_name_offset",
                wire.asset_name_offset,
                image.asset_name_offset(),
            ),
            (
                "label_offset",
                wire.label_offset,
                image.geometry.label_offset(),
            ),
        ] {
            if declared != derived {
                return Err(format!("{name} must match the Canvas record layout"));
            }
        }
        if wire.boundary_coordinate_offsets != image.geometry.boundary_coordinate_offsets() {
            return Err("boundary_coordinate_offsets must match the Canvas record layout".into());
        }
        Ok(image)
    }
}

impl From<DesignCanvasImage> for DesignCanvasImageWire {
    fn from(value: DesignCanvasImage) -> Self {
        let (opacity, origin, u_axis, v_axis) = value.geometry.payload.decoded();
        let scope_reference_offset = value.geometry.scope_reference_offset();
        let geometry_record_index = value.geometry.record_index;
        let geometry_reference_offset = value.geometry_reference_offset();
        let geometry_byte_offset = value.geometry.byte_offset;
        let geometry_prologue = value.geometry.prologue.bytes();
        let visible = value.geometry.prologue.visible();
        let visibility_offset = value.geometry.visibility_offset();
        let geometry_frame_length = value.geometry.frame_length();
        let paired_geometry_byte_offset = value.geometry.paired_byte_offset();
        let paired_component_reference_offset = value.geometry.paired_component_reference_offset();
        let boundary_segments = value.geometry.boundary.segments();
        let boundary_coordinate_offsets = value.geometry.boundary_coordinate_offsets();
        let second_boundary_present_offset = value.geometry.second_boundary_present_offset();
        let plane_reference_offset = value.geometry.plane_reference_offset();
        let component_reference_offset = value.geometry.component_reference_offset();
        let asset_record_index = value.asset.record_index;
        let asset_reference_offset = value.geometry.asset_reference_offset();
        let asset_byte_offset = value.asset_byte_offset();
        let asset_name_offset = value.asset_name_offset();
        let label_offset = value.geometry.label_offset();
        let geometry_payload = value.geometry.payload.bytes().to_vec();
        let [geometry_class_tag, paired_geometry_class_tag] = value.geometry.class_tags;
        Self {
            id: value.id,
            scope_record_index: value.scope_record_index,
            scope_reference_offset,
            geometry_class_tag,
            geometry_record_index,
            geometry_reference_offset,
            geometry_byte_offset,
            geometry_prologue,
            visible,
            visibility_offset,
            geometry_frame_length,
            paired_geometry_class_tag,
            paired_geometry_byte_offset,
            paired_component_reference_offset,
            boundary_segments,
            boundary_coordinate_offsets,
            second_boundary_present_offset,
            plane_entity_suffix: value.plane_entity_suffix,
            plane_reference_offset,
            component_entity_suffix: value.component_entity_suffix,
            component_reference_offset,
            asset_class_tag: value.asset.class_tag.into(),
            asset_record_index,
            asset_reference_offset,
            asset_byte_offset,
            asset_name: value.asset.name,
            asset_name_offset,
            label: value.geometry.label,
            label_offset,
            opacity,
            origin,
            u_axis,
            v_axis,
            geometry_payload,
        }
    }
}
