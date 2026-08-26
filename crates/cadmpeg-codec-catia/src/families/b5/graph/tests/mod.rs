use super::*;

fn test_pcurve(object_id: u32, surface: u32) -> B5Pcurve {
    B5Pcurve {
        object_id,
        surface,
        degree: 1,
        distinct_knots: vec![0.0, 1.0],
        multiplicities: vec![2, 2],
        control_points: vec![[0.0, 0.0], [1.0, 0.0]],
        weights: None,
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    }
}

fn object_stream_pcurve(
    surface: u32,
    distinct_knots: Vec<f64>,
    suffix: Option<f64>,
) -> B5ObjectStreamPcurve {
    B5ObjectStreamPcurve {
        class: 0x21,
        surface,
        parameter_range: [
            *distinct_knots.first().expect("test knot"),
            *distinct_knots.last().expect("test knot"),
        ],
        class_21_suffix_scalar: suffix,
        distinct_knots,
    }
}

fn test_loop_metadata(edge_count: usize) -> B5LoopMetadata {
    B5LoopMetadata {
        framing_controls: [0x05, 0x05],
        edge_controls: vec![[1, 1, 1]; edge_count],
        extension: None,
    }
}

fn extended_loop_metadata(metadata_control: u8) -> Vec<u8> {
    let mut bytes = vec![0x03, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00];
    bytes.push(0x0d);
    for value in [1.0_f64, -2.0, 3.5, 4.25] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0x05, 0x05, metadata_control, 0x05, 0x01]);
    for value in [1.0_f32, -2.0, 3.5, 4.25, 5.5, -6.75] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

mod analytic;
mod loops;
mod typed;
mod walk;
