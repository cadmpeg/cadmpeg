// SPDX-License-Identifier: Apache-2.0
//! Orientation readings of a face.
//!
//! Loops keep the face on their left viewed from the oriented normal. With
//! `N = S_u × S_v` of the placed carrier, the parameter frame viewed from
//! `+N` is right-handed for every chart, so the oriented normal is `+N` when
//! the face lies on the left of its loops in the parameter plane. A face's
//! sense states one reading; the winding of its loops against their computed
//! roles gives another.

use cadmpeg_ir::topology::{Face, Sense};

use crate::region::Region;
use crate::report::{Classification, FaceOrientation, NormalReading, RegionRule};

/// The oriented normal a face's sense states.
pub(crate) fn declared_normal(face: &Face) -> NormalReading {
    match face.sense {
        Sense::Forward => NormalReading::Surface,
        Sense::Reversed => NormalReading::Reversed,
    }
}

/// The oriented normal the loops give against their computed roles. A loop
/// whose direction is not known (`directed[index]` false) does not vote.
pub(crate) fn loop_normal(region: &Region, directed: &[bool]) -> NormalReading {
    match region.rule {
        RegionRule::Nesting | RegionRule::DomainExterior => {}
        RegionRule::WholeSurface | RegionRule::DeclaredOrientation => {
            return NormalReading::NotDeterminable
        }
    }
    let mut agree = false;
    let mut oppose = false;
    for (index, member) in region.loops.iter().enumerate() {
        if !directed.get(index).copied().unwrap_or(true) {
            continue;
        }
        if member.factor > 0.0 {
            agree = true;
        } else if member.factor < 0.0 {
            oppose = true;
        }
    }
    match (agree, oppose) {
        (true, false) => NormalReading::Surface,
        (false, true) => NormalReading::Reversed,
        (true, true) => NormalReading::Mixed,
        (false, false) => NormalReading::NotDeterminable,
    }
}

/// The orientation facts of a face whose region is `region`.
pub(crate) fn face_orientation(
    face: &Face,
    region: Option<&Region>,
    directed: &[bool],
) -> FaceOrientation {
    let declared = declared_normal(face);
    let Some(region) = region else {
        return FaceOrientation {
            declared_normal: declared,
            loop_normal: NormalReading::NotDeterminable,
            witness: None,
            consistent: None,
            classification: Classification::Unspecified,
        };
    };
    let loops = loop_normal(region, directed);
    let consistent = match loops {
        NormalReading::Mixed => Some(false),
        NormalReading::NotDeterminable => None,
        NormalReading::Surface | NormalReading::Reversed => Some(loops == declared),
    };
    FaceOrientation {
        declared_normal: declared,
        loop_normal: loops,
        witness: None,
        consistent,
        classification: region.classification,
    }
}
