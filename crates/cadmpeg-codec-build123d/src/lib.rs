// SPDX-License-Identifier: Apache-2.0
//! Writes a cadmpeg document as a [build123d](https://build123d.readthedocs.io)
//! Python program.
//!
//! build123d is a code-first CAD library, so this target differs from the other
//! encoders: the output is source, not a container. That makes the exported
//! model something a person can read, diff, and edit, which is the point.
//!
//! This encoder writes the solved B-rep. Every face is rebuilt from its surface
//! carrier and its solved boundary edges, and the faces are sewn into solids.
//! It therefore works for any document that carries topology, with or without a
//! feature history.
//!
//! ```no_run
//! # use cadmpeg_ir::codec::{EncodeInput, Encoder};
//! # use cadmpeg_codec_build123d::Build123dEncoder;
//! # fn run(ir: &cadmpeg_ir::document::CadIr) -> Result<(), Box<dyn std::error::Error>> {
//! let plan = Build123dEncoder.plan(EncodeInput { ir, fidelity: None })?;
//! let mut source = Vec::new();
//! plan.write_to(&mut source)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Nothing is guessed
//!
//! A carrier the IR cannot supply, and a boundary that does not lie on the
//! carrier it would trim, are both refused rather than approximated. This is
//! not only good manners: `OpenCascade` aborts the host process instead of
//! returning an error for either, so the emitted program would take the whole
//! interpreter down with it. Both cases are decided analytically here, and
//! reported as loss.
//!
//! # Blend concavity
//!
//! A toroidal blend face is bounded by two circles that are equally consistent
//! with the quarter tube of a fillet and with the three-quarter tube around it.
//! cadmpeg keeps the distinction in the sign of `minor_radius`, which STEP's
//! `TOROIDAL_SURFACE` has no room for. This encoder emits the band explicitly
//! rather than leaving an importer to guess.

mod brep;
mod geom;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{EncodeInput, Encoder, ExportPlan};
use cadmpeg_ir::report::{CensusBasis, EntityCensus, ExportReport, FidelityResolution, WritePath};

/// Stable identifier for this export target.
pub const FORMAT_ID: &str = "build123d";

/// Encoder writing a build123d program.
#[derive(Debug, Clone, Copy, Default)]
pub struct Build123dEncoder;

impl Encoder for Build123dEncoder {
    fn id(&self) -> &'static str {
        FORMAT_ID
    }

    fn plan<'a>(&self, input: EncodeInput<'a>) -> Result<ExportPlan<'a>, CodecError> {
        let (source, losses, counts) = brep::Writer::new(input.ir).write();
        let report = ExportReport {
            format: FORMAT_ID.to_owned(),
            census: EntityCensus {
                basis: CensusBasis::TargetRecords,
                counts,
            },
            fidelity: if input.fidelity.is_some() {
                FidelityResolution::NotConsumed
            } else {
                FidelityResolution::NotProvided
            },
            write_path: WritePath::Synthesized,
            losses,
            notes: vec!["The emitted program requires build123d 0.10 or newer.".to_owned()],
        };
        Ok(ExportPlan::buffered(report, source.into_bytes()))
    }
}
