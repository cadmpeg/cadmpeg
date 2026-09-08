use super::super::graph::{B5LoopMember, B5LoopMetadata};
use crate::families::b5::graph::controls::B5FramingControl;

pub(crate) fn test_loop_metadata() -> B5LoopMetadata {
    B5LoopMetadata {
        framing_controls: [B5FramingControl::Control05; 2],
        extension: None,
    }
}

pub(crate) fn test_loop_members(pcurves: &[u32], edges: &[u32]) -> Vec<B5LoopMember> {
    pcurves
        .iter()
        .zip(edges)
        .map(|(&pcurve, &edge)| B5LoopMember {
            pcurve,
            edge,
            controls: [1, 1, 1],
        })
        .collect()
}

mod closure;
mod decode;
mod pcurves;
