//! Release-mode scaling benchmark for the STEP pipeline.

use std::hint::black_box;
use std::io::Cursor;
use std::time::{Duration, Instant};

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::units::Units;
use cadmpeg_step::{parse, write_step, StepCodec, StepWriteOptions};

const ENTITY_COUNT: usize = 100_000;

fn exchange(entity: &str) -> Vec<u8> {
    let mut source = String::with_capacity(4_000_000);
    source.push_str(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('benchmark'),'2;1');\
         FILE_NAME('','','','','','','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\
         ENDSEC;DATA;",
    );
    for id in 1..=ENTITY_COUNT {
        match entity {
            "point" => source.push_str(&format!("#{id}=CARTESIAN_POINT('',(1.,2.,3.));")),
            "opaque" => source.push_str(&format!("#{id}=OPAQUE_VALUE('x');")),
            _ => unreachable!(),
        }
    }
    source.push_str("ENDSEC;END-ISO-10303-21;");
    source.into_bytes()
}

fn ir() -> CadIr {
    let mut ir = CadIr::empty(Units::default());
    ir.model.points.extend((0..ENTITY_COUNT).map(|index| Point {
        source_object: None,
        id: PointId(format!("point-{index}")),
        position: Point3::new(index as f64, 2.0, 3.0),
    }));
    ir
}

fn measure(name: &str, mut run: impl FnMut()) {
    run();
    let start = Instant::now();
    let mut iterations = 0_u64;
    while start.elapsed() < Duration::from_secs(1) {
        run();
        iterations += 1;
    }
    let elapsed = start.elapsed();
    println!(
        "{name}: {:.3} ms/iteration ({iterations} iterations)",
        elapsed.as_secs_f64() * 1000.0 / iterations as f64
    );
}

fn main() {
    let points = exchange("point");
    let opaque = exchange("opaque");
    let ir = ir();
    let codec = StepCodec::default();

    measure("parse typed", || {
        black_box(parse::parse(black_box(&points)).unwrap());
    });
    measure("decode typed", || {
        let mut input = Cursor::new(&points);
        black_box(codec.decode(&mut input, &DecodeOptions::default()).unwrap());
    });
    measure("decode opaque", || {
        let mut input = Cursor::new(&opaque);
        black_box(codec.decode(&mut input, &DecodeOptions::default()).unwrap());
    });
    measure("inspect opaque", || {
        let mut input = Cursor::new(&opaque);
        black_box(codec.inspect(&mut input).unwrap());
    });
    measure("encode points", || {
        let mut output = Vec::new();
        black_box(write_step(black_box(&ir), &mut output, &StepWriteOptions::default()).unwrap());
        black_box(output);
    });
}
