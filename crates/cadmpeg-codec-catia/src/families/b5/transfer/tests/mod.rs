use super::super::graph::B5LoopMetadata;

pub(crate) fn test_loop_metadata(edge_count: usize) -> B5LoopMetadata {
    B5LoopMetadata {
        framing_controls: [0x05, 0x05],
        edge_controls: vec![[1, 1, 1]; edge_count],
        extension: None,
    }
}

mod closure;
mod pcurves;
