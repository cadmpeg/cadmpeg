// SPDX-License-Identifier: Apache-2.0
//! Morph-control payload decoding.

use std::fmt;
use std::ops::Range;

use serde::{Deserialize, Serialize};

use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface};
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal, NonZeroReal};
use cadmpeg_ir::units::FiniteVector;

use crate::cage::Cage;
use crate::chunks::{admitted_vec, checked_count_bytes, chunk_at, ArchiveVersion, BoundedReader};
use crate::curves::GeometryError;
use crate::mesh::MeshExpand;
use crate::settings::{interval, point, vector, xform, MillimeterScale};
use crate::wire::{scaled_coordinate, uuid, Uuid};

const ANONYMOUS: u32 = 0x4000_8000;
const MAX_LOCALIZERS: usize = 1 << 16;
const MAX_CAPTIVES: usize = 1 << 20;
pub(crate) const CLASS: Uuid = Uuid::from_canonical([
    0xd3, 0x79, 0xe6, 0xd8, 0x7c, 0x31, 0x44, 0x07, 0xa9, 0x13, 0xe3, 0xb7, 0x04, 0x0d, 0x03, 0x4a,
]);

#[derive(Debug, Clone)]
pub(crate) enum Control {
    Curve {
        start: NurbsCurve,
        end: NurbsCurve,
    },
    Surface {
        start: NurbsSurface,
        end: NurbsSurface,
    },
    Cage {
        start_transform: FiniteVector<16>,
        end: Cage,
    },
}

/// Localizer code as written by Rhino; the file may carry values outside the
/// documented table, which decode unchanged.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct LocalizerKind(pub(crate) i32);

impl LocalizerKind {
    const NONE: Self = Self(0);
    const SPHERE: Self = Self(1);
    const PLANE: Self = Self(2);
    const CYLINDER: Self = Self(3);
    const CURVE: Self = Self(4);
    const SURFACE: Self = Self(5);
    const DISTANCE: Self = Self(6);
}

impl fmt::Debug for LocalizerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match *self {
            Self::NONE => "None",
            Self::SPHERE => "Sphere",
            Self::PLANE => "Plane",
            Self::CYLINDER => "Cylinder",
            Self::CURVE => "Curve",
            Self::SURFACE => "Surface",
            Self::DISTANCE => "Distance",
            Self(code) => return write!(f, "LocalizerKind({code})"),
        };
        f.write_str(name)
    }
}

impl fmt::Display for LocalizerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Localizer {
    pub(crate) kind: LocalizerKind,
    pub(crate) point: FiniteVector<3>,
    pub(crate) vector: FiniteVector<3>,
    pub(crate) interval: FiniteVector<2>,
    pub(crate) curve: Option<NurbsCurve>,
    pub(crate) surface: Option<NurbsSurface>,
}

#[derive(Debug, Clone)]
pub(crate) struct Morph {
    source_range: Range<usize>,
    pub(crate) control: Control,
    pub(crate) captive_ids: Vec<Uuid>,
    pub(crate) localizers: Vec<Localizer>,
    pub(crate) tolerance: NonNegativeReal,
    pub(crate) quick_preview: bool,
    pub(crate) preserve_structure: bool,
}

fn anonymous<'a>(
    data: &'a [u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
    family: &str,
) -> Result<(BoundedReader<'a>, usize, i32, i32), GeometryError> {
    let chunk = chunk_at(data, offset, end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(GeometryError::malformed(
            offset,
            format!("{family} is not anonymous"),
        ));
    }
    let mut reader = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    let major = reader.i32()?;
    let minor = reader.i32()?;
    Ok((reader, chunk.next_offset(), major, minor))
}

fn count(
    reader: &mut BoundedReader<'_>,
    element_size: usize,
    cap: usize,
) -> Result<usize, GeometryError> {
    let offset = reader.position();
    let value = reader.i32()?;
    checked_count_bytes(value, element_size, reader.remaining(), cap, offset)?;
    usize::try_from(value)
        .map_err(|_| GeometryError::malformed(offset, "morph-control count overflows"))
}

fn captive_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Vec<Uuid>, GeometryError> {
    let (mut ids, next, major, minor) = anonymous(
        data,
        reader.position(),
        reader.end(),
        archive,
        "captive UUID list",
    )?;
    if major != 1 || minor < 0 {
        return Err(GeometryError::UnsupportedVersion {
            offset: ids.position() - 8,
            message: format!("unsupported captive UUID-list version {major}.{minor}"),
        });
    }
    let count = count(&mut ids, 16, MAX_CAPTIVES)?;
    let mut values = admitted_vec(ctx, count, "Rhino morph captive IDs")?;
    for _ in 0..count {
        values.push(uuid(&mut ids)?);
    }
    ids.skip_remaining()?;
    reader.skip(next - reader.position())?;
    Ok(values)
}

fn scale_point(
    value: crate::settings::Point3,
    scale: MillimeterScale,
    offset: usize,
) -> Result<FiniteVector<3>, GeometryError> {
    let [x, y, z] = value
        .0
        .get()
        .map(|coordinate| scaled_real(coordinate, scale));
    let (Some(x), Some(y), Some(z)) = (x, y, z) else {
        return Err(GeometryError::malformed(
            offset,
            "scaled morph-control coordinate is invalid",
        ));
    };
    Ok(FiniteVector::from([x, y, z]))
}

/// Multiply an admitted archive coordinate by the unit scale and admit the
/// product, as `scaled_coordinate` does.
fn scaled_real(coordinate: f64, scale: MillimeterScale) -> Option<FiniteReal> {
    FiniteReal::new(coordinate * scale.value())
}

fn scale_interval(
    value: [f64; 2],
    scale: MillimeterScale,
    offset: usize,
) -> Result<FiniteVector<2>, GeometryError> {
    let start = scaled_real(value[0], scale)
        .ok_or_else(|| GeometryError::malformed(offset, "scaled localizer interval is invalid"))?;
    let end = scaled_real(value[1], scale).ok_or_else(|| {
        GeometryError::malformed(offset + 8, "scaled localizer interval is invalid")
    })?;
    Ok(FiniteVector::from([start, end]))
}

fn optional_localizer<T>(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    kind: &str,
    parse: impl FnOnce(&mut BoundedReader<'_>) -> Result<T, GeometryError>,
) -> Result<Option<T>, GeometryError> {
    let (mut child, next, major, minor) = anonymous(
        data,
        reader.position(),
        reader.end(),
        archive,
        &format!("localizer {kind}"),
    )?;
    if major != 1 || minor < 0 {
        return Err(GeometryError::UnsupportedVersion {
            offset: child.position() - 8,
            message: format!("unsupported localizer-{kind} version {major}.{minor}"),
        });
    }
    let value = child.bool()?.then(|| parse(&mut child)).transpose()?;
    child.skip_remaining()?;
    reader.skip(next - reader.position())?;
    Ok(value)
}

fn localizer(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
) -> Result<Localizer, GeometryError> {
    let (mut value, next, major, minor) =
        anonymous(data, reader.position(), reader.end(), archive, "localizer")?;
    if major != 1 || minor < 0 {
        return Err(GeometryError::UnsupportedVersion {
            offset: value.position() - 8,
            message: format!("unsupported localizer version {major}.{minor}"),
        });
    }
    let kind = LocalizerKind(value.i32()?);
    let offset = value.position();
    let point = scale_point(point(&mut value)?, scale, offset)?;
    let vector = vector(&mut value)?.0;
    let offset = value.position();
    let interval = scale_interval(interval(&mut value)?.0.get(), scale, offset)?;
    let curve = optional_localizer(data, &mut value, archive, "curve", |child| {
        crate::surfaces::read_nurbs_curve(ctx, child, scale)
    })?;
    let surface = optional_localizer(data, &mut value, archive, "surface", |child| {
        crate::surfaces::read_nurbs_surface(ctx, child, scale)
    })?;
    value.skip_remaining()?;
    reader.skip(next - reader.position())?;
    Ok(Localizer {
        kind,
        point,
        vector,
        interval,
        curve,
        surface,
    })
}

fn control_child<T>(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    label: &str,
    read: impl FnOnce(&mut BoundedReader<'_>) -> Result<T, GeometryError>,
) -> Result<T, GeometryError> {
    let (mut child, next, major, minor) =
        anonymous(data, reader.position(), reader.end(), archive, label)?;
    if major != 1 || minor < 0 {
        return Err(GeometryError::UnsupportedVersion {
            offset: child.position() - 8,
            message: format!("unsupported {label} version {major}.{minor}"),
        });
    }
    let value = read(&mut child)?;
    child.skip_remaining()?;
    reader.skip(next - reader.position())?;
    Ok(value)
}

fn scaled_transform(
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
) -> Result<FiniteVector<16>, GeometryError> {
    let mut transform = xform(reader)?.0;
    for index in [3, 7, 11] {
        let scaled = scaled_coordinate(transform[index], scale).ok_or_else(|| {
            GeometryError::malformed(reader.position() - 128, "scaled cage transform is invalid")
        })?;
        transform = transform.with_component(index, scaled).ok_or_else(|| {
            GeometryError::malformed(reader.position() - 128, "scaled cage transform is invalid")
        })?;
    }
    Ok(transform)
}

fn cage_at(
    expand: MeshExpand<'_>,
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
) -> Result<Cage, GeometryError> {
    let (cage, next) =
        crate::cage::decode_at(expand, reader.position(), reader.end(), scale, archive)?;
    reader.skip(next - reader.position())?;
    Ok(cage)
}

pub(crate) fn decode(
    expand: MeshExpand<'_>,
    range: Range<usize>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
) -> Result<Morph, GeometryError> {
    let data = expand.data();
    let (mut outer, _next, major, minor) =
        anonymous(data, range.start, range.end, archive, "morph control")?;
    if !matches!(major, 1 | 2) || minor < 0 {
        return Err(GeometryError::UnsupportedVersion {
            offset: range.start,
            message: format!("unsupported morph-control version {major}.{minor}"),
        });
    }
    if major == 1 {
        let end = cage_at(expand, &mut outer, scale, archive)?;
        let captive_ids = captive_ids(expand.ctx(), data, &mut outer, archive)?;
        let start_transform = scaled_transform(&mut outer, scale)?;
        outer.skip_remaining()?;
        return Ok(Morph {
            source_range: range,
            control: Control::Cage {
                start_transform,
                end,
            },
            captive_ids,
            localizers: Vec::new(),
            tolerance: NonNegativeReal::ZERO,
            quick_preview: false,
            preserve_structure: false,
        });
    }

    let control = match outer.i32()? {
        1 => Control::Curve {
            start: control_child(data, &mut outer, archive, "morph start control", |reader| {
                crate::surfaces::read_nurbs_curve(expand.ctx(), reader, scale)
            })?,
            end: control_child(data, &mut outer, archive, "morph end control", |reader| {
                crate::surfaces::read_nurbs_curve(expand.ctx(), reader, scale)
            })?,
        },
        2 => Control::Surface {
            start: control_child(data, &mut outer, archive, "morph start control", |reader| {
                crate::surfaces::read_nurbs_surface(expand.ctx(), reader, scale)
            })?,
            end: control_child(data, &mut outer, archive, "morph end control", |reader| {
                crate::surfaces::read_nurbs_surface(expand.ctx(), reader, scale)
            })?,
        },
        3 => Control::Cage {
            start_transform: control_child(
                data,
                &mut outer,
                archive,
                "morph start control",
                |reader| scaled_transform(reader, scale),
            )?,
            end: control_child(data, &mut outer, archive, "morph end control", |reader| {
                cage_at(expand, reader, scale, archive)
            })?,
        },
        _ => {
            return Err(GeometryError::malformed(
                outer.position() - 4,
                "invalid morph-control variant",
            ))
        }
    };
    let captive_ids = captive_ids(expand.ctx(), data, &mut outer, archive)?;

    let localizers = localizers(expand.ctx(), data, &mut outer, scale, archive)?;
    let (tolerance, quick_preview, preserve_structure) = if minor >= 1 {
        let tolerance = scaled_coordinate(outer.f64()?, scale)
            .and_then(NonNegativeReal::from_finite)
            .ok_or_else(|| {
                GeometryError::malformed(outer.position() - 8, "invalid morph tolerance")
            })?;
        (tolerance, outer.bool()?, outer.bool()?)
    } else {
        (NonNegativeReal::ZERO, false, false)
    };
    outer.skip_remaining()?;
    Ok(Morph {
        source_range: range,
        control,
        captive_ids,
        localizers,
        tolerance,
        quick_preview,
        preserve_structure,
    })
}

fn localizers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    outer: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
) -> Result<Vec<Localizer>, GeometryError> {
    let (mut list, list_next, list_major, list_minor) = anonymous(
        data,
        outer.position(),
        outer.end(),
        archive,
        "morph localizers",
    )?;
    if list_major != 1 || list_minor < 0 {
        return Err(GeometryError::UnsupportedVersion {
            offset: list.position() - 8,
            message: format!("unsupported morph-localizer-list version {list_major}.{list_minor}"),
        });
    }
    let localizer_count = count(&mut list, 12, MAX_LOCALIZERS)?;
    let mut localizers = admitted_vec(ctx, localizer_count, "Rhino morph localizers")?;
    for _ in 0..localizer_count {
        localizers.push(localizer(ctx, data, &mut list, scale, archive)?);
    }
    list.skip_remaining()?;
    outer.skip(list_next - outer.position())?;
    Ok(localizers)
}

struct CommaList<I>(I);

impl<I> fmt::Display for CommaList<I>
where
    I: Clone + Iterator,
    I::Item: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, value) in self.0.clone().enumerate() {
            if index > 0 {
                f.write_str(",")?;
            }
            write!(f, "{value}")?;
        }
        Ok(())
    }
}

fn write_points(
    f: &mut fmt::Formatter<'_>,
    values: impl Iterator<Item = cadmpeg_ir::math::Point3>,
) -> fmt::Result {
    for (index, point) in values.enumerate() {
        if index > 0 {
            f.write_str(";")?;
        }
        write!(f, "{},{},{}", point.x, point.y, point.z)?;
    }
    Ok(())
}

struct CurvePoints<'a>(&'a NurbsPoles3<cadmpeg_ir::features::FinitePoint3>);

impl fmt::Display for CurvePoints<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            NurbsPoles3::Polynomial { points } => {
                write_points(f, points.iter().map(|point| point.get()))
            }
            NurbsPoles3::Rational { points } => {
                write_points(f, points.iter().map(|pole| pole.point.get()))
            }
        }
    }
}

struct SurfacePoints<'a>(&'a NurbsPoleGrid<cadmpeg_ir::features::FinitePoint3>);

impl fmt::Display for SurfacePoints<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            NurbsPoleGrid::Polynomial { rows } => {
                write_points(f, rows.iter().flatten().map(|point| point.get()))
            }
            NurbsPoleGrid::Rational { rows } => {
                write_points(f, rows.iter().flatten().map(|pole| pole.point.get()))
            }
        }
    }
}

struct CagePoints<'a>(&'a [Vec<FiniteReal>]);

impl fmt::Display for CagePoints<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, point) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(";")?;
            }
            write!(
                f,
                "{}",
                CommaList(point.iter().copied().map(FiniteReal::get))
            )?;
        }
        Ok(())
    }
}

fn insert_property(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    properties: &mut std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    key: fmt::Arguments<'_>,
    value: fmt::Arguments<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let key = crate::wire::admitted_format(ctx, key, "Rhino morph property key")?;
    let key = cadmpeg_core::text::NonBlankString::new(key)
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("blank generated Rhino morph key"))?;
    let value = crate::wire::admitted_format(ctx, value, "Rhino morph property value")?;
    ctx.charge_collection_items(1, "Rhino morph property entries")?;
    properties.insert(key, value);
    Ok(())
}

fn curve_properties(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    prefix: &str,
    curve: &NurbsCurve,
    properties: &mut std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_degree"),
        format_args!("{}", curve.degree()),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_knots"),
        format_args!("{}", CommaList(curve.knots().iter().copied())),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_control_points"),
        format_args!("{}", CurvePoints(curve.pole_rows())),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_periodic"),
        format_args!("{}", curve.periodic()),
    )?;
    if let NurbsPoles3::Rational { points } = curve.pole_rows() {
        insert_property(
            ctx,
            properties,
            format_args!("{prefix}_weights"),
            format_args!("{}", CommaList(points.iter().map(|pole| pole.weight.get()))),
        )?;
    }
    Ok(())
}

fn surface_properties(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    prefix: &str,
    surface: &NurbsSurface,
    properties: &mut std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_u_degree"),
        format_args!("{}", surface.u_degree()),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_v_degree"),
        format_args!("{}", surface.v_degree()),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_u_knots"),
        format_args!("{}", CommaList(surface.u_knots().iter().copied())),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_v_knots"),
        format_args!("{}", CommaList(surface.v_knots().iter().copied())),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_u_count"),
        format_args!("{}", surface.u_count()),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_v_count"),
        format_args!("{}", surface.v_count()),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_control_points"),
        format_args!("{}", SurfacePoints(surface.pole_grid())),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_u_periodic"),
        format_args!("{}", surface.u_periodic()),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_v_periodic"),
        format_args!("{}", surface.v_periodic()),
    )?;
    if let NurbsPoleGrid::Rational { rows } = surface.pole_grid() {
        insert_property(
            ctx,
            properties,
            format_args!("{prefix}_weights"),
            format_args!(
                "{}",
                CommaList(rows.iter().flatten().map(|pole| pole.weight.get()))
            ),
        )?;
    }
    Ok(())
}

fn cage_properties(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    prefix: &str,
    cage: &Cage,
    properties: &mut std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_dimension"),
        format_args!("{}", cage.dimension),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_rational"),
        format_args!("{}", cage.rational()),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_orders"),
        format_args!("{},{},{}", cage.orders[0], cage.orders[1], cage.orders[2]),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_counts"),
        format_args!("{},{},{}", cage.counts[0], cage.counts[1], cage.counts[2]),
    )?;
    for (axis, knots) in ["u", "v", "w"].into_iter().zip(&cage.knots) {
        insert_property(
            ctx,
            properties,
            format_args!("{prefix}_{axis}_knots"),
            format_args!("{}", CommaList(knots.iter().copied().map(FiniteReal::get))),
        )?;
    }
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_control_points"),
        format_args!("{}", CagePoints(&cage.control_points)),
    )?;
    if let Some(weights) = &cage.weights {
        insert_property(
            ctx,
            properties,
            format_args!("{prefix}_weights"),
            format_args!(
                "{}",
                CommaList(weights.iter().copied().map(NonZeroReal::get))
            ),
        )?;
    }
    Ok(())
}

/// Projects one decoded morph control into a native feature.
///
/// `resolve_captive` maps each captive UUID to its native record identity.
/// It charges unresolved references as needed; raw UUIDs stay in `captive_ids`.
pub(crate) fn project(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    morph: &Morph,
    key: &str,
    name: Option<String>,
    native_ref: String,
    mut resolve_captive: impl FnMut(Uuid) -> Result<Option<String>, cadmpeg_core::CodecError>,
) -> Result<cadmpeg_ir::features::Feature, cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    use std::collections::BTreeMap;

    let (variant, mut properties) = match &morph.control {
        Control::Curve { start, end } => {
            let mut properties = BTreeMap::new();
            curve_properties(ctx, "start", start, &mut properties)?;
            curve_properties(ctx, "end", end, &mut properties)?;
            ("curve", properties)
        }
        Control::Surface { start, end } => {
            let mut properties = BTreeMap::new();
            surface_properties(ctx, "start", start, &mut properties)?;
            surface_properties(ctx, "end", end, &mut properties)?;
            ("surface", properties)
        }
        Control::Cage {
            start_transform,
            end,
        } => {
            let mut properties = BTreeMap::new();
            insert_property(
                ctx,
                &mut properties,
                format_args!("start_transform"),
                format_args!("{}", CommaList(start_transform.iter().copied())),
            )?;
            cage_properties(ctx, "end", end, &mut properties)?;
            ("cage", properties)
        }
    };
    for (index, localizer) in morph.localizers.iter().enumerate() {
        let prefix = crate::wire::admitted_format(
            ctx,
            format_args!("localizer_{index}"),
            "Rhino morph localizer prefix",
        )?;
        insert_property(
            ctx,
            &mut properties,
            format_args!("{prefix}_type"),
            format_args!("{}", localizer.kind),
        )?;
        insert_property(
            ctx,
            &mut properties,
            format_args!("{prefix}_point"),
            format_args!("{}", CommaList(localizer.point.into_iter())),
        )?;
        insert_property(
            ctx,
            &mut properties,
            format_args!("{prefix}_vector"),
            format_args!("{}", CommaList(localizer.vector.into_iter())),
        )?;
        insert_property(
            ctx,
            &mut properties,
            format_args!("{prefix}_interval"),
            format_args!("{}", CommaList(localizer.interval.into_iter())),
        )?;
        if let Some(curve) = &localizer.curve {
            let curve_prefix = crate::wire::admitted_format(
                ctx,
                format_args!("{prefix}_curve"),
                "Rhino morph localizer prefix",
            )?;
            curve_properties(ctx, &curve_prefix, curve, &mut properties)?;
        }
        if let Some(surface) = &localizer.surface {
            let surface_prefix = crate::wire::admitted_format(
                ctx,
                format_args!("{prefix}_surface"),
                "Rhino morph localizer prefix",
            )?;
            surface_properties(ctx, &surface_prefix, surface, &mut properties)?;
        }
    }
    let key = crate::wire::copy_retained_string(ctx, key, "Rhino morph feature key")?;
    let key = cadmpeg_ir::ids::IdentityKey::try_new(key)
        .map_err(|error| cadmpeg_core::CodecError::malformed(error.to_string()))?;
    let feature_id = FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "morph", "feature"),
        key,
    );
    let ordinal = morph.source_range.start as u64;
    let mut parameters = BTreeMap::new();
    insert_property(
        ctx,
        &mut parameters,
        format_args!("variant"),
        format_args!("{variant}"),
    )?;
    insert_property(
        ctx,
        &mut parameters,
        format_args!("captive_ids"),
        format_args!("{}", CommaList(morph.captive_ids.iter().copied())),
    )?;
    insert_property(
        ctx,
        &mut parameters,
        format_args!("tolerance"),
        format_args!("{}", morph.tolerance.get()),
    )?;
    insert_property(
        ctx,
        &mut parameters,
        format_args!("quick_preview"),
        format_args!("{}", morph.quick_preview),
    )?;
    insert_property(
        ctx,
        &mut parameters,
        format_args!("preserve_structure"),
        format_args!("{}", morph.preserve_structure),
    )?;
    for (index, id) in morph.captive_ids.iter().enumerate() {
        if let Some(record) = resolve_captive(*id)? {
            let key = crate::wire::admitted_format(
                ctx,
                format_args!("captive_{index}_object"),
                "Rhino morph property key",
            )?;
            let key = cadmpeg_core::text::NonBlankString::new(key).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("blank generated Rhino morph key")
            })?;
            ctx.charge_collection_items(1, "Rhino morph property entries")?;
            parameters.insert(key, record);
        }
    }
    Ok(Feature {
        id: feature_id.clone(),
        ordinal,
        name,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: properties,
        source_tag: Some("RhinoMorphControl".to_string()),
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "morph_control".into(),
                parameters,
            }),
        ),
        native_ref: Some(native_ref),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        captive_ids, decode, localizer, localizers, project, Control, LocalizerKind, ANONYMOUS,
    };
    use crate::chunks::{ArchiveVersion, BoundedReader};
    use crate::curves::GeometryError;
    use crate::settings::MillimeterScale;
    use crate::test_support::test_dump::crc_chunk;

    fn anonymous(major: i32, minor: i32, suffix: &[u8]) -> Vec<u8> {
        let mut body = major.to_le_bytes().to_vec();
        body.extend(minor.to_le_bytes());
        body.extend(suffix);
        crc_chunk(ArchiveVersion::V5, ANONYMOUS, &body)
    }

    fn cage() -> Vec<u8> {
        let mut body = 3_i32.to_le_bytes().to_vec();
        body.extend(0_i32.to_le_bytes());
        for _ in 0..6 {
            body.extend(2_i32.to_le_bytes());
        }
        for _ in 0..3 {
            body.extend(0.0_f64.to_le_bytes());
            body.extend(1.0_f64.to_le_bytes());
        }
        for index in 0..8 {
            for coordinate in [index as f64, 0.0, 0.0] {
                body.extend(coordinate.to_le_bytes());
            }
        }
        anonymous(1, 0, &body)
    }

    fn curve(end: f64) -> Vec<u8> {
        let mut bytes = vec![0x11];
        for value in [3_i32, 0, 2, 2, 0, 0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend([0; 48]);
        bytes.extend(2_i32.to_le_bytes());
        bytes.extend(0.0_f64.to_le_bytes());
        bytes.extend(1.0_f64.to_le_bytes());
        bytes.extend(2_i32.to_le_bytes());
        for value in [0.0_f64, 0.0, 0.0, end, 0.0, 0.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.push(0);
        bytes
    }

    #[test]
    fn captive_ids_refuse_collection_limit() {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend([0; 16]);
        let bytes = anonymous(1, 0, &body);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded fixture");
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("root bytes admitted");
        let error = captive_ids(&ctx, &bytes, &mut reader, ArchiveVersion::V5)
            .expect_err("one captive exceeds zero collection items");
        assert!(matches!(
            error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.operation == "Rhino morph captive IDs"
        ));
    }

    #[test]
    fn localizers_refuse_collection_limit() {
        let mut localizer_body = 6_i32.to_le_bytes().to_vec();
        for value in [1.0_f64, 2.0, 3.0, 0.0, 0.0, 1.0, 4.0, 5.0] {
            localizer_body.extend(value.to_le_bytes());
        }
        localizer_body.extend(anonymous(1, 0, &[0]));
        localizer_body.extend(anonymous(1, 0, &[0]));
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(anonymous(1, 0, &localizer_body));
        let bytes = anonymous(1, 0, &body);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded fixture");
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("root bytes admitted");
        let error = localizers(
            &ctx,
            &bytes,
            &mut reader,
            MillimeterScale::IDENTITY,
            ArchiveVersion::V5,
        )
        .expect_err("one localizer exceeds zero collection items");
        assert!(matches!(
            error,
            GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.operation == "Rhino morph localizers"
        ));
    }

    #[test]
    fn decodes_cage_morph_future_minor_and_unit_scaling() {
        let mut transform = Vec::new();
        for value in [
            1.0_f64, 0.0, 0.0, 2.0, 0.0, 1.0, 0.0, 3.0, 0.0, 0.0, 1.0, 4.0, 0.0, 0.0, 0.0, 1.0,
        ] {
            transform.extend(value.to_le_bytes());
        }
        let start = anonymous(1, 0, &transform);
        let end = anonymous(1, 0, &cage());
        let mut captives = 1_i32.to_le_bytes().to_vec();
        captives.extend([0; 16]);
        let captives = anonymous(1, 0, &captives);
        let localizers = anonymous(1, 0, &0_i32.to_le_bytes());
        let mut content = 3_i32.to_le_bytes().to_vec();
        content.extend(start);
        content.extend(end);
        content.extend(captives);
        content.extend(localizers);
        content.extend(0.01_f64.to_le_bytes());
        content.extend([1, 0]);
        content.extend(0x1234_5678_i32.to_le_bytes());
        let bytes = anonymous(2, 2, &content);

        let morph = crate::decode::with_expand_bytes(&bytes, |expand| {
            decode(
                expand,
                0..bytes.len(),
                crate::test_support::millimeter_scale(10.0),
                ArchiveVersion::V8,
            )
        })
        .expect("required invariant");
        assert_eq!(morph.captive_ids.len(), 1);
        assert_eq!(morph.tolerance.get(), 0.1);
        assert!(morph.quick_preview);
        assert!(!morph.preserve_structure);
        let Control::Cage {
            start_transform,
            end,
        } = &morph.control
        else {
            panic!("expected cage morph");
        };
        assert_eq!(start_transform[3], 20.0);
        assert_eq!(start_transform[7], 30.0);
        assert_eq!(start_transform[11], 40.0);
        assert_eq!(end.control_points[7][0].get(), 70.0);
        // One nil captive: no resolved identity and no charge.
        assert_eq!(morph.captive_ids.len(), 1);
        let feature = project(
            &cadmpeg_test_support::service_decode_context(),
            &morph,
            "test",
            None,
            "native".to_string(),
            |_| Ok(None),
        )
        .expect("the fixture states named properties");
        assert_eq!(feature.source_tag.as_deref(), Some("RhinoMorphControl"));
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native { parameters, .. },
        ) = feature.evaluation.definition()
        else {
            panic!("expected a native morph definition");
        };
        assert!(!parameters.contains_key("captive_0_object"));
        let resolved = project(
            &cadmpeg_test_support::service_decode_context(),
            &morph,
            "test",
            None,
            "native".to_string(),
            |_| Ok(Some("rhino:object:record#000007".to_string())),
        )
        .expect("the fixture states named properties");
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native { parameters, .. },
        ) = resolved.evaluation.definition()
        else {
            panic!("expected a native morph definition");
        };
        assert_eq!(parameters["captive_0_object"], "rhino:object:record#000007");
    }

    #[test]
    fn morph_projection_refuses_property_and_captive_limits() {
        let curve = super::NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            ],
            None,
            false,
        )
        .expect("valid test curve");
        let morph = super::Morph {
            source_range: 0..1,
            control: super::Control::Curve {
                start: curve.clone(),
                end: curve,
            },
            captive_ids: vec![super::Uuid::nil()],
            localizers: Vec::new(),
            tolerance: super::NonNegativeReal::new(0.0).expect("valid tolerance"),
            quick_preview: false,
            preserve_structure: false,
        };
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admitted");
        let refusal = super::project(&ctx, &morph, "fixture", None, "native".into(), |_| Ok(None))
            .expect_err("first property exceeds zero collection items");
        assert!(matches!(
            refusal,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino morph property entries"
        ));
        let mut captive_policy = cadmpeg_core::decode::DecodePolicy::service();
        captive_policy.limits.max_collection_items = 13;
        let (captive_ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &captive_policy)
                .expect("empty root admitted");
        let refusal = super::project(
            &captive_ctx,
            &morph,
            "fixture",
            None,
            "native".into(),
            |_| {
                captive_ctx.charge_collection_items(1, "Rhino morph captive resolution")?;
                Ok(None)
            },
        )
        .expect_err("captive resolution exceeds the collection limit");
        assert!(matches!(
            refusal,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino morph captive resolution"
        ));
        let feature = super::project(
            &cadmpeg_test_support::service_decode_context(),
            &morph,
            "fixture",
            None,
            "native".into(),
            |_| Ok(None),
        )
        .expect("service profile admits morph properties");
        assert_eq!(feature.source_properties["start_degree"], "1");
    }

    #[test]
    fn rejects_unknown_morph_control_major() {
        let bytes = anonymous(3, 0, &[]);
        let result = crate::decode::with_expand_bytes(&bytes, |expand| {
            decode(
                expand,
                0..bytes.len(),
                MillimeterScale::IDENTITY,
                ArchiveVersion::V8,
            )
        });
        assert!(matches!(
            result,
            Err(GeometryError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn decodes_curve_morph_and_distance_localizer() {
        let start = anonymous(1, 0, &curve(1.0));
        let end = anonymous(1, 0, &curve(2.0));
        let captives = anonymous(1, 0, &0_i32.to_le_bytes());
        let mut localizer = 6_i32.to_le_bytes().to_vec();
        for value in [1.0_f64, 2.0, 3.0, 0.0, 0.0, 1.0, 4.0, 5.0] {
            localizer.extend(value.to_le_bytes());
        }
        localizer.extend(anonymous(1, 0, &[0]));
        localizer.extend(anonymous(1, 0, &[0]));
        let localizer = anonymous(1, 0, &localizer);
        let mut localizers = 1_i32.to_le_bytes().to_vec();
        localizers.extend(localizer);
        let localizers = anonymous(1, 0, &localizers);
        let mut content = 1_i32.to_le_bytes().to_vec();
        content.extend(start);
        content.extend(end);
        content.extend(captives);
        content.extend(localizers);
        content.extend(0.0_f64.to_le_bytes());
        content.extend([0, 1]);
        let bytes = anonymous(2, 1, &content);

        let morph = crate::decode::with_expand_bytes(&bytes, |expand| {
            decode(
                expand,
                0..bytes.len(),
                crate::test_support::millimeter_scale(10.0),
                ArchiveVersion::V8,
            )
        })
        .expect("required invariant");
        let Control::Curve { start, end } = &morph.control else {
            panic!("expected curve morph");
        };
        assert_eq!(start.control_points()[1].x, 10.0);
        assert_eq!(end.control_points()[1].x, 20.0);
        assert_eq!(morph.localizers[0].point.get(), [10.0, 20.0, 30.0]);
        assert_eq!(morph.localizers[0].vector.get(), [0.0, 0.0, 1.0]);
        assert_eq!(morph.localizers[0].interval.get(), [40.0, 50.0]);
        assert!(morph.preserve_structure);
    }

    #[test]
    fn localizer_geometry_presence_is_independent_of_its_kind() {
        let mut surface = vec![0x10];
        for value in [3_i32, 0, 2, 2, 2, 2, 0, 0] {
            surface.extend(value.to_le_bytes());
        }
        surface.extend([0; 48]);
        for _ in 0..2 {
            surface.extend(2_i32.to_le_bytes());
            surface.extend(0.0_f64.to_le_bytes());
            surface.extend(1.0_f64.to_le_bytes());
        }
        surface.extend(4_i32.to_le_bytes());
        for point in [
            [0.0_f64, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
        ] {
            for value in point {
                surface.extend(value.to_le_bytes());
            }
        }
        let mut curve_payload = vec![1];
        curve_payload.extend(curve(2.0));
        let mut surface_payload = vec![1];
        surface_payload.extend(surface);
        for kind in [
            LocalizerKind::NONE,
            LocalizerKind::SPHERE,
            LocalizerKind::CURVE,
            LocalizerKind::SURFACE,
            LocalizerKind(99),
        ] {
            let mut payload = kind.0.to_le_bytes().to_vec();
            for value in [0.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0] {
                payload.extend(value.to_le_bytes());
            }
            payload.extend(anonymous(1, 0, &curve_payload));
            payload.extend(anonymous(1, 0, &surface_payload));
            let bytes = anonymous(1, 0, &payload);
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("localizer bounds");
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let policy = cadmpeg_core::decode::DecodePolicy::service();
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                    .expect("localizer fixture fits service profile");
            let value = localizer(
                &ctx,
                &bytes,
                &mut reader,
                MillimeterScale::IDENTITY,
                ArchiveVersion::V8,
            )
            .expect("localizer fields");
            assert_eq!(value.kind, kind);
            assert!(value.curve.is_some());
            assert!(value.surface.is_some());
            assert_eq!(reader.remaining(), 0);
        }
    }
}
