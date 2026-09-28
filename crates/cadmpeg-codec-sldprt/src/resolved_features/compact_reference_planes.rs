//! Compact reference plane record index.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};

const EPS_COMPACT_REFERENCE_PLANES_COMPACT_COMPONENT_REFERENCE_PLANE_RECORD_E9: f64 = 1e-9;
const EPS_COMPACT_REFERENCE_PLANES_COMPACT_COMPONENT_PLANE_FRAME_E9: f64 = 1e-9;

const COMPACT_REFERENCE_PLANE_CLASS: &[u8] = b"moCompRefPlane_c";
const COMPACT_REFERENCE_PLANE_RECORD_LEN: usize = 67;
const COMPACT_COMPONENT_PLANE_RECORD_LEN: usize = 138;

pub(super) struct CompactReferencePlaneIndex {
    payload_len: usize,
    class_offsets: Vec<usize>,
    declared: Vec<(usize, u32)>,
    components: Vec<(usize, u32)>,
}

impl CompactReferencePlaneIndex {
    pub(super) fn new(ctx: &DecodeContext<'_>, payload: &[u8]) -> Result<Self, CodecError> {
        let scan_len = u64::try_from(payload.len()).map_err(|_| {
            ctx.refuse_codec_limit("index compact reference planes", u64::MAX - 1, u64::MAX)
        })?;
        let work = scan_len.checked_mul(3).ok_or_else(|| {
            ctx.refuse_codec_limit("index compact reference planes", u64::MAX - 1, u64::MAX)
        })?;
        ctx.charge_work(work, "index compact reference planes")?;
        let mut class_offsets = Vec::new();
        let mut declared = Vec::new();
        let mut components = Vec::new();
        for (offset, byte) in payload.iter().enumerate() {
            if *byte == COMPACT_REFERENCE_PLANE_CLASS[0]
                && payload.get(offset..offset + COMPACT_REFERENCE_PLANE_CLASS.len())
                    == Some(COMPACT_REFERENCE_PLANE_CLASS)
            {
                ctx.reserve_collection_vec(
                    &mut class_offsets,
                    1,
                    "collect compact reference plane classes",
                )?;
                class_offsets.push(offset);
            }
            if *byte == 0x3f {
                if let Some(start) = offset.checked_sub(46) {
                    if let Some(bytes) = payload.get(start..start + COMPACT_REFERENCE_PLANE_RECORD_LEN) {
                        if let Some(source) = compact_declared_reference_plane_record(bytes) {
                            ctx.reserve_collection_vec(
                                &mut declared,
                                1,
                                "collect declared compact reference planes",
                            )?;
                            declared.push((start, source));
                        }
                    }
                }
            }
            if *byte == 4
                && payload.get(offset..offset + 8)
                    == Some(&[4, 0, 0, 0, 0xff, 0xff, 0xff, 0xff])
            {
                if let Some(start) = offset.checked_sub(122) {
                    if let Some(bytes) = payload.get(start..start + COMPACT_COMPONENT_PLANE_RECORD_LEN) {
                        if let Some(source) = compact_component_reference_plane_record(bytes) {
                            ctx.reserve_collection_vec(
                                &mut components,
                                1,
                                "collect component compact reference planes",
                            )?;
                            components.push((start, source));
                        }
                    }
                }
            }
        }
        Ok(Self {
            payload_len: payload.len(),
            class_offsets,
            declared,
            components,
        })
    }

    fn declared_source(&self, start: usize, end: usize) -> Option<u32> {
        if start > end || end > self.payload_len {
            return None;
        }
        let class_count = self
            .class_offsets
            .iter()
            .filter(|offset| {
                **offset >= start
                    && offset.saturating_add(COMPACT_REFERENCE_PLANE_CLASS.len()) <= end
            })
            .count();
        if class_count != 1 {
            return None;
        }
        unique_reference_plane_source(
            self.declared
                .iter()
                .filter(|(offset, _)| {
                    *offset >= start
                        && offset.saturating_add(COMPACT_REFERENCE_PLANE_RECORD_LEN) <= end
                })
                .map(|(_, source)| *source),
        )
    }

    fn reference_source(&self, start: usize, end: usize) -> Option<u32> {
        if start > end || end > self.payload_len {
            return None;
        }
        unique_reference_plane_source(
            self.declared_source(start, end).into_iter().chain(
                self.components
                    .iter()
                    .filter(|(offset, _)| {
                        *offset >= start
                            && offset.saturating_add(COMPACT_COMPONENT_PLANE_RECORD_LEN) <= end
                    })
                    .map(|(_, source)| *source),
            ),
        )
    }

    fn lane_source(&self) -> Option<u32> {
        self.declared_source(0, self.payload_len)
            .or_else(|| self.reference_source(0, self.payload_len))
    }

    /// Return the reference plane source for a profile.
    pub(super) fn profile_source(
        &self,
        context_start: usize,
        profile_start: usize,
        profile_end: usize,
    ) -> Option<u32> {
        self.reference_source(profile_start, profile_end)
            .or_else(|| self.reference_source(context_start, profile_end))
            .or_else(|| self.lane_source())
    }
}

fn unique_reference_plane_source(sources: impl IntoIterator<Item = u32>) -> Option<u32> {
    let mut sources = sources.into_iter();
    let source = sources.next()?;
    sources.all(|candidate| candidate == source).then_some(source)
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
fn compact_reference_plane_source(payload: &[u8]) -> Option<u32> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(
        payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    ).ok()?;
    CompactReferencePlaneIndex::new(&ctx, payload)
        .ok()?
        .reference_source(0, payload.len())
}

fn compact_component_plane_frame(payload: &[u8]) -> Option<(Point3, Vector3, Vector3)> {
    const RECORD_LEN: usize = 138;
    const NATIVE_TO_IR: f64 = 1000.0;

    let mut frames = payload
        .windows(RECORD_LEN)
        .filter_map(|bytes| {
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
        });
    let frame = frames.next()?;
    frames.all(|candidate| candidate == frame).then_some(frame)
}

pub(super) fn compact_profile_component_plane_frame(
    payload: &[u8],
    context_start: usize,
    profile_start: usize,
    profile_end: usize,
) -> Option<(Point3, Vector3, Vector3)> {
    compact_component_plane_frame(payload.get(profile_start..profile_end)?)
        .or_else(|| compact_component_plane_frame(payload.get(context_start..profile_end)?))
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
