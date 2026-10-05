//! Compact reference plane record index.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};

const EPS_COMPACT_REFERENCE_PLANES_COMPACT_COMPONENT_REFERENCE_PLANE_RECORD_E9: f64 = 1e-9;
const EPS_COMPACT_REFERENCE_PLANES_COMPACT_COMPONENT_PLANE_FRAME_E9: f64 = 1e-9;

const COMPACT_REFERENCE_PLANE_CLASS: &[u8] = b"moCompRefPlane_c";
const COMPACT_REFERENCE_PLANE_RECORD_LEN: usize = 67;
const COMPACT_COMPONENT_PLANE_RECORD_LEN: usize = 138;

/// The compact reference-plane records of one lane payload, in payload order.
/// Each record list keeps, for every position, the end of the run of equal
/// sources that starts there, so a range of records agrees on one source
/// exactly when its first run reaches its end.
pub(super) struct CompactReferencePlaneIndex<'ctx> {
    payload_len: usize,
    class_offsets: Vec<usize>,
    declared: SourceRecords,
    components: SourceRecords,
    lane_source: Option<u32>,
    _storage: ScopedReservation<'ctx>,
}

#[derive(Default)]
struct SourceRecords {
    records: Vec<(usize, u32)>,
    run_ends: Vec<usize>,
}

/// The sources of the records in one payload range.
enum RangeSource {
    Empty,
    One(u32),
    Mixed,
}

impl SourceRecords {
    fn push(
        &mut self,
        ctx: &DecodeContext<'_>,
        storage: &mut ScopedReservation<'_>,
        record: (usize, u32),
        operation: &'static str,
    ) -> Result<(), CodecError> {
        storage.with_storage(|| ctx.push_vec(&mut self.records, record, operation))
    }

    fn finish(
        &mut self,
        ctx: &DecodeContext<'_>,
        storage: &mut ScopedReservation<'_>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let count = self.records.len();
        self.run_ends = storage.with_storage(|| ctx.alloc_filled(count, count, operation))?;
        for index in ctx
            .admit_iter(&(0..count.checked_sub(1).unwrap_or_default()), operation)?
            .rev()
        {
            if self.records[index].1 == self.records[index + 1].1 {
                self.run_ends[index] = self.run_ends[index + 1];
            } else {
                self.run_ends[index] = index + 1;
            }
        }
        Ok(())
    }

    /// The sources of the records of `width` bytes that lie in `start..end`.
    fn source(
        &self,
        ctx: &DecodeContext<'_>,
        start: usize,
        end: usize,
        width: usize,
        operation: &'static str,
    ) -> Result<RangeSource, CodecError> {
        let (first, last) = record_range(
            ctx,
            &self.records,
            |record| record.0,
            start,
            end,
            width,
            operation,
        )?;
        Ok(if first >= last {
            RangeSource::Empty
        } else if self.run_ends[first] >= last {
            RangeSource::One(self.records[first].1)
        } else {
            RangeSource::Mixed
        })
    }
}

/// The positions of the sorted records of `width` bytes that lie wholly in
/// `start..end`.
fn record_range<T>(
    ctx: &DecodeContext<'_>,
    records: &[T],
    offset: impl Fn(&T) -> usize,
    start: usize,
    end: usize,
    width: usize,
    operation: &'static str,
) -> Result<(usize, usize), CodecError> {
    let first = ctx.partition_point(records, |record| Ok(offset(record) < start), operation)?;
    let last = ctx.partition_point(
        records,
        |record| {
            Ok(offset(record)
                .checked_add(width)
                .is_some_and(|stop| stop <= end))
        },
        operation,
    )?;
    Ok((first, last))
}

impl<'ctx> CompactReferencePlaneIndex<'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>, payload: &[u8]) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "index compact reference planes")?;
        let mut class_offsets = Vec::new();
        let mut declared = SourceRecords::default();
        let mut components = SourceRecords::default();
        for (offset, byte) in ctx
            .admit_iter(payload, "index compact reference planes")?
            .enumerate()
        {
            if *byte == COMPACT_REFERENCE_PLANE_CLASS[0]
                && payload.get(offset..offset + COMPACT_REFERENCE_PLANE_CLASS.len())
                    == Some(COMPACT_REFERENCE_PLANE_CLASS)
            {
                storage.with_storage(|| {
                    ctx.push_vec(
                        &mut class_offsets,
                        offset,
                        "collect compact reference plane classes",
                    )
                })?;
            }
            if *byte == 0x3f {
                if let Some(start) = offset.checked_sub(46) {
                    if let Some(source) = payload
                        .get(start..start + COMPACT_REFERENCE_PLANE_RECORD_LEN)
                        .and_then(compact_declared_reference_plane_record)
                    {
                        declared.push(
                            ctx,
                            &mut storage,
                            (start, source),
                            "collect declared compact reference planes",
                        )?;
                    }
                }
            }
            if *byte == 4
                && payload.get(offset..offset + 8) == Some(&[4, 0, 0, 0, 0xff, 0xff, 0xff, 0xff])
            {
                if let Some(start) = offset.checked_sub(122) {
                    if let Some(source) = payload
                        .get(start..start + COMPACT_COMPONENT_PLANE_RECORD_LEN)
                        .and_then(compact_component_reference_plane_record)
                    {
                        components.push(
                            ctx,
                            &mut storage,
                            (start, source),
                            "collect component compact reference planes",
                        )?;
                    }
                }
            }
        }
        declared.finish(ctx, &mut storage, "index declared compact reference planes")?;
        components.finish(
            ctx,
            &mut storage,
            "index component compact reference planes",
        )?;
        let mut index = Self {
            payload_len: payload.len(),
            class_offsets,
            declared,
            components,
            lane_source: None,
            _storage: storage,
        };
        index.lane_source = match index.declared_source(ctx, 0, index.payload_len)? {
            Some(source) => Some(source),
            None => index.reference_source(ctx, 0, index.payload_len)?,
        };
        Ok(index)
    }

    fn declared_source(
        &self,
        ctx: &DecodeContext<'_>,
        start: usize,
        end: usize,
    ) -> Result<Option<u32>, CodecError> {
        if start > end || end > self.payload_len {
            return Ok(None);
        }
        let (first, last) = record_range(
            ctx,
            &self.class_offsets,
            |offset| *offset,
            start,
            end,
            COMPACT_REFERENCE_PLANE_CLASS.len(),
            "count compact reference plane classes",
        )?;
        if last.checked_sub(first) != Some(1) {
            return Ok(None);
        }
        Ok(
            match self.declared.source(
                ctx,
                start,
                end,
                COMPACT_REFERENCE_PLANE_RECORD_LEN,
                "scan declared compact reference plane records",
            )? {
                RangeSource::One(source) => Some(source),
                RangeSource::Empty | RangeSource::Mixed => None,
            },
        )
    }

    fn reference_source(
        &self,
        ctx: &DecodeContext<'_>,
        start: usize,
        end: usize,
    ) -> Result<Option<u32>, CodecError> {
        if start > end || end > self.payload_len {
            return Ok(None);
        }
        let declared_source = self.declared_source(ctx, start, end)?;
        let components = self.components.source(
            ctx,
            start,
            end,
            COMPACT_COMPONENT_PLANE_RECORD_LEN,
            "scan compact component plane sources",
        )?;
        Ok(match (declared_source, components) {
            (Some(source), RangeSource::Empty) => Some(source),
            (Some(source), RangeSource::One(component)) => (component == source).then_some(source),
            (None, RangeSource::One(component)) => Some(component),
            (_, RangeSource::Mixed) | (None, RangeSource::Empty) => None,
        })
    }

    /// Return the reference plane source for a profile.
    pub(super) fn profile_source(
        &self,
        ctx: &DecodeContext<'_>,
        context_start: usize,
        profile_start: usize,
        profile_end: usize,
    ) -> Result<Option<u32>, CodecError> {
        if let Some(source) = self.reference_source(ctx, profile_start, profile_end)? {
            return Ok(Some(source));
        }
        if let Some(source) = self.reference_source(ctx, context_start, profile_end)? {
            return Ok(Some(source));
        }
        Ok(self.lane_source)
    }
}

fn compact_component_reference_plane_record(bytes: &[u8]) -> Option<u32> {
    let source = View::u32_le_at(bytes, 0)?;
    if source == 0
        || bytes.get(8..14)?.iter().any(|byte| *byte != 0)
        || bytes.get(14) != Some(&1)
        || bytes.get(122..126) != Some(&4u32.to_le_bytes())
        || bytes.get(126..130) != Some(&[0xff; 4])
    {
        return None;
    }
    let scalar = |offset| {
        let value = View::f64_le_at(bytes, offset)?;
        value.is_finite().then_some(value)
    };
    let basis = [
        Vector3::new(scalar(15)?, scalar(23)?, scalar(31)?),
        Vector3::new(scalar(39)?, scalar(47)?, scalar(55)?),
        Vector3::new(scalar(63)?, scalar(71)?, scalar(79)?),
    ];
    (basis.iter().all(|vector| {
        (vector.norm() - 1.0).abs()
            <= EPS_COMPACT_REFERENCE_PLANES_COMPACT_COMPONENT_REFERENCE_PLANE_RECORD_E9
    }) && basis[0].dot(basis[1]).abs()
        <= EPS_COMPACT_REFERENCE_PLANES_COMPACT_COMPONENT_REFERENCE_PLANE_RECORD_E9
        && basis[0].dot(basis[2]).abs()
            <= EPS_COMPACT_REFERENCE_PLANES_COMPACT_COMPONENT_REFERENCE_PLANE_RECORD_E9
        && basis[1].dot(basis[2]).abs()
            <= EPS_COMPACT_REFERENCE_PLANES_COMPACT_COMPONENT_REFERENCE_PLANE_RECORD_E9)
        .then_some(source)
}

fn compact_declared_reference_plane_record(bytes: &[u8]) -> Option<u32> {
    let identity = View::u32_le_at(bytes, 0)?;
    let legacy_source = View::u16_le_at(bytes, 10)?;
    let trailer = bytes.get(47..63)?;
    let common = bytes.get(12..39)?.iter().all(|byte| *byte == 0)
        && bytes.get(39..47) == Some(&1.0f64.to_le_bytes())
        && trailer[..3] == [0; 3]
        && matches!(trailer[3], 2..=4)
        && trailer[4..7] == [0; 3]
        && matches!(trailer[7], 0xf9 | 0xfb | 0xff)
        && trailer[8..11] == [0xff; 3]
        && trailer[11..15] == [0; 4]
        && trailer[15] >= 0x65;
    if !common {
        return None;
    }
    if identity != 0
        && !(bytes.get(4..10)?.iter().all(|byte| *byte == 0) && legacy_source != 0)
        && bytes.get(8..12) == Some(&[0, 0, 3, 0])
    {
        Some(identity)
    } else if identity != 0
        && identity != u32::MAX
        && legacy_source != 0
        && bytes.get(4..10)?.iter().all(|byte| *byte == 0)
    {
        Some(u32::from(legacy_source))
    } else {
        None
    }
}

#[cfg(test)]
fn compact_reference_plane_source(payload: &[u8]) -> Result<Option<u32>, CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(
        payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;
    let index = CompactReferencePlaneIndex::new(&ctx, payload)?;
    index.reference_source(&ctx, 0, payload.len())
}

fn compact_component_plane_frame(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<(Point3, Vector3, Vector3)>, CodecError> {
    const RECORD_LEN: usize = 138;
    const NATIVE_TO_IR: f64 = 1000.0;

    let frame_at = |bytes: &[u8]| {
        // Cheap byte-pattern guards run before any float is read; every
        // guard is side-effect free, so rejecting early keeps the accept
        // set identical while skipping the frame math at almost every
        // window offset.
        let source = View::u32_le_at(bytes, 0)?;
        if source == 0
            || bytes.get(8..14) != Some(&[0; 6])
            || bytes.get(14) != Some(&1)
            || bytes.get(119..122) != Some(&[0; 3])
            || bytes.get(122..126) != Some(&4u32.to_le_bytes())
            || bytes.get(126..130) != Some(&[0xff; 4])
        {
            return None;
        }
        let scalar = |index: usize| {
            let offset = 15 + index * 8;
            let value = View::f64_le_at(bytes, offset)?;
            value.is_finite().then_some(value)
        };
        let u_axis = Vector3::new(scalar(0)?, scalar(1)?, scalar(2)?);
        let v_axis = Vector3::new(scalar(3)?, scalar(4)?, scalar(5)?);
        let normal = Vector3::new(scalar(6)?, scalar(7)?, scalar(8)?);
        let expected_normal = u_axis.cross(v_axis);
        if (u_axis.dot(u_axis) - 1.0).abs()
            > EPS_COMPACT_REFERENCE_PLANES_COMPACT_COMPONENT_PLANE_FRAME_E9
            || (v_axis.dot(v_axis) - 1.0).abs()
                > EPS_COMPACT_REFERENCE_PLANES_COMPACT_COMPONENT_PLANE_FRAME_E9
            || (normal.dot(normal) - 1.0).abs()
                > EPS_COMPACT_REFERENCE_PLANES_COMPACT_COMPONENT_PLANE_FRAME_E9
            || (expected_normal.x - normal.x).abs()
                > EPS_COMPACT_REFERENCE_PLANES_COMPACT_COMPONENT_PLANE_FRAME_E9
            || (expected_normal.y - normal.y).abs()
                > EPS_COMPACT_REFERENCE_PLANES_COMPACT_COMPONENT_PLANE_FRAME_E9
            || (expected_normal.z - normal.z).abs()
                > EPS_COMPACT_REFERENCE_PLANES_COMPACT_COMPONENT_PLANE_FRAME_E9
            || scalar(12)? != 1.0
        {
            return None;
        }
        Some((
            Point3::new(
                scalar(9)? * NATIVE_TO_IR,
                scalar(10)? * NATIVE_TO_IR,
                scalar(11)? * NATIVE_TO_IR,
            ),
            normal,
            u_axis,
        ))
    };
    // Each window step reads a fixed record; the search charges the windows it
    // visits and stops at the first disagreeing frame.
    let mut windows = payload.windows(RECORD_LEN);
    let Some(frame) = ctx.find_map(
        &mut windows,
        |bytes| Ok(frame_at(bytes)),
        "scan compact component plane frames",
    )?
    else {
        return Ok(None);
    };
    let disagrees = ctx.any_by(
        &mut windows,
        |bytes| Ok(frame_at(bytes).is_some_and(|candidate| candidate != frame)),
        "scan compact component plane frames",
    )?;
    Ok((!disagrees).then_some(frame))
}

pub(super) fn compact_profile_component_plane_frame(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    context_start: usize,
    profile_start: usize,
    profile_end: usize,
) -> Result<Option<(Point3, Vector3, Vector3)>, CodecError> {
    let Some(profile) = payload.get(profile_start..profile_end) else {
        return Ok(None);
    };
    if let Some(frame) = compact_component_plane_frame(ctx, profile)? {
        return Ok(Some(frame));
    }
    let Some(context) = payload.get(context_start..profile_end) else {
        return Ok(None);
    };
    compact_component_plane_frame(ctx, context)
}

pub(crate) fn principal_sketch_frame(
    plane: cadmpeg_ir::features::PrincipalPlane,
) -> (Point3, Vector3, Vector3) {
    use cadmpeg_ir::features::PrincipalPlane;
    match plane {
        PrincipalPlane::Front => (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
        ),
        PrincipalPlane::Top => (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ),
        PrincipalPlane::Right => (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
        ),
    }
}

#[cfg(test)]
mod compact_reference_planes_tests;
