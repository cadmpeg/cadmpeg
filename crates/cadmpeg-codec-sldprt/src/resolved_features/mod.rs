// SPDX-License-Identifier: Apache-2.0
//! Typed views over `SolidWorks` `ResolvedFeatures` sketch records.

const SKETCH_MARKER: &[u8] = &[0xff, 0xff, 0x1f, 0x00, 0x03];

const LEGACY_SKETCH_MARKER: &[u8] = &[0xff, 0xff, 0x07, 0x00, 0x01];

const LEGACY_EXTENDED_SKETCH_MARKER: &[u8] = &[0xff, 0xff, 0x1f, 0x00, 0x01];

const CLASS_MARKER: &[u8] = &[0xff, 0xff, 0x01, 0x00];

const NAME_MARKER: &[u8] = &[0x04, 0x80, 0xff, 0xfe, 0xff];

const SCALAR_HEADER: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
    0xff, 0xfe, 0xff, 0x00, 0x00, 0x00,
];

const COMPACT_SCALAR_HEADER: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00,
];

const VALUE_ONLY_SCALAR_HEADER: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00,
];

const SKETCH_POINT_TOLERANCE: f64 = 1.0e-9;

const SKETCH_ANGLE_TOLERANCE: f64 = 1.0e-9;

const SPATIAL_VERTEX_PREFIX: &[u8] = &[
    0xff, 0xfe, 0xff, 0x06, b'V', 0x00, b'e', 0x00, b'r', 0x00, b't', 0x00, b'e', 0x00, b'x', 0x00,
];

fn is_class_token(token: u16) -> bool {
    token & 0x8000 != 0 && token != u16::MAX
}

/// The end offset one object declares, proved to lie inside the bytes that
/// carry it.
///
/// An object states where it ends. A declared end past the available bytes is
/// a contradiction inside bytes that are present: the object does not frame
/// there, and the shorter region is a different record. [`DeclaredEnd::of`] is
/// the only constructor and refuses that state, so a reader cannot decode a
/// truncated region as if it were the declared one - the overrun is not
/// representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeclaredEnd(usize);

impl DeclaredEnd {
    /// The declared end, or `None` when `declared_end` runs past `available`.
    fn of(declared_end: usize, available: usize) -> Option<Self> {
        (declared_end <= available).then_some(Self(declared_end))
    }

    /// The declared end offset.
    fn get(self) -> usize {
        self.0
    }
}

/// One lane's classes of one name, in offset order.
fn sorted_classes<'l>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    lane: &'l crate::records::FeatureInputLane,
    name: &str,
    operation: &'static str,
) -> Result<Vec<&'l crate::records::FeatureInputClass>, cadmpeg_core::CodecError> {
    let mut classes = Vec::new();
    for class in ctx.admit_iter(&lane.classes, operation)? {
        if ctx.equal(class.name.as_str(), name, operation)? {
            storage.with_storage(|| ctx.push_vec(&mut classes, class, operation))?;
        }
    }
    ctx.stable_sort_by(&mut classes, |class| &class.offset, Ord::cmp, operation)?;
    Ok(classes)
}

/// The classes of an offset-sorted list declared in `start..end`.
fn classes_within<'c, 'l>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    classes: &'c [&'l crate::records::FeatureInputClass],
    start: u64,
    end: u64,
    operation: &'static str,
) -> Result<&'c [&'l crate::records::FeatureInputClass], cadmpeg_core::CodecError> {
    let first = ctx.partition_point(classes, |class| Ok(class.offset < start), operation)?;
    let last = ctx.partition_point(classes, |class| Ok(class.offset < end), operation)?;
    Ok(classes.get(first..last).unwrap_or_default())
}

/// Whether each history feature, in history order, carries a name that no
/// other feature repeats. An empty name counts only with `count_empty`.
fn unique_feature_names(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    temporary: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    histories: &[crate::records::FeatureHistory],
    count_empty: bool,
) -> Result<Vec<bool>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "count SLDPRT history feature names";
    let mut once = std::collections::HashMap::<&str, bool>::new();
    for history in ctx.admit_iter(histories, OPERATION)? {
        for feature in ctx.admit_iter(&history.features, OPERATION)? {
            if feature.name.is_empty() && !count_empty {
                continue;
            }
            if let Some(single) =
                ctx.get_mut_hash_map(&mut once, feature.name.as_str(), OPERATION)?
            {
                *single = false;
                continue;
            }
            temporary.with_storage(|| {
                ctx.insert_hash_map(&mut once, feature.name.as_str(), true, OPERATION)
            })?;
        }
    }
    let mut unique = Vec::new();
    for history in ctx.admit_iter(histories, "mark SLDPRT unique history feature names")? {
        for feature in ctx.admit_iter(
            &history.features,
            "mark SLDPRT unique history feature names",
        )? {
            let single = ctx.get_hash_map(
                &once,
                feature.name.as_str(),
                "find SLDPRT history feature-name count",
            )? == Some(&true);
            temporary.with_storage(|| {
                ctx.push_vec(
                    &mut unique,
                    single,
                    "mark SLDPRT unique history feature names",
                )
            })?;
        }
    }
    Ok(unique)
}

pub(crate) mod assembly;

pub(crate) mod axes;

pub(crate) mod bindings;

pub(crate) mod classes;

pub(crate) mod compact_reference_planes;

pub(crate) mod component_paths;

mod curves;

pub(crate) mod dimensions;

pub(crate) mod direct_edits;

mod drafts;

mod endpoints;

pub(crate) mod hashes;

mod helix;

pub(crate) mod holes;

pub(crate) mod markers;

pub(crate) mod names;

pub(crate) mod operands;

pub(crate) mod operations;

pub(crate) mod parameters;

pub(crate) mod profiles;

pub(crate) mod projections;

pub(crate) mod reference_geometry;

pub(crate) mod relation_geometry;

mod relation_loci;

mod relation_records;

pub(crate) mod scalars;

pub(crate) mod selections;

mod sketch_edges;

pub(crate) mod sketch_projection;

mod sketch_write;

pub(crate) mod terminations;

mod grid;
mod transforms;

pub(crate) mod typed_relations;

pub(crate) mod validate;

mod write_generate;

pub(crate) mod write_prepare;
