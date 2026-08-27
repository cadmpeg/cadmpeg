// SPDX-License-Identifier: Apache-2.0
//! "What is this file?", answered at inspection depth.

use std::collections::BTreeMap;
use std::io::{Read, SeekFrom};

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_core::dialect::{primary_layer, DialectId};
use cadmpeg_core::ReadSeek;
use cadmpeg_ir::codec::Confidence;

use crate::support::{support, Disposition};
use crate::InputCatalog;

/// Leading byte window offered to prefix detection.
///
/// The same window the CLI reads before it loads a file: detection that saw
/// less than the loader does could name a different codec than the loader
/// then picks.
pub const DETECTION_PREFIX_LEN: usize = 128 * 1024;

/// The confidence at which [`identify`] opens a candidate's container.
///
/// [`Confidence::Low`] is documented as a generic container signature, and a
/// ZIP with no format marker earns exactly that from every ZIP-based codec at
/// once. Reconstructing a container per Low candidate would spend the whole
/// budget proving what the prefix already said: this file is ambiguous. A
/// candidate below the floor is reported with no dialect.
pub const INSPECTION_FLOOR: Confidence = Confidence::Medium;

/// What cadmpeg makes of one file, before any semantic decode.
///
/// # No admission field
///
/// [`cadmpeg_core::dialect::Admission`] is the record of what a run did: it
/// states whether the strategy that parsed the bytes was the one the
/// identified dialect declares. Identification precedes that run. A preflight
/// that reported an admission would be predicting one, and the prediction
/// would be wrong exactly where it matters — on the legacy file whose declared
/// grammar is not the grammar its bytes obey.
///
/// [`Self::disposition`] is the honest static half: what cadmpeg *says* it
/// does with this dialect, which is what an open dialog needs before it opens
/// anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identification {
    /// The stable format id of the candidate codec, for example `"f3d"`.
    pub format: &'static str,
    /// How strongly the byte prefix named this format.
    ///
    /// Detection confidence, not classification confidence. A `High` here with
    /// a `None` dialect means the format is certain and the dialect is not.
    pub confidence: Confidence,
    /// The registry dialect id of the primary format layer.
    ///
    /// `None` when the candidate stayed below [`INSPECTION_FLOOR`], when the
    /// inspection exhausted its budget or failed, or when the discriminants
    /// matched no declared dialect.
    pub dialect: Option<DialectId>,
    /// Version fields the source declared, verbatim, under keys pinned per
    /// codec in the registry.
    ///
    /// Evidence, never a control input: the dialect is what the bytes obey,
    /// not what they declare. Empty whenever [`Self::dialect`] is `None`,
    /// because nothing opened the container to read them.
    pub declared: BTreeMap<String, String>,
    /// What the capability registry declares cadmpeg does with
    /// [`Self::dialect`].
    ///
    /// `None` when no dialect was settled, and also when a settled id carries
    /// no capability row — a registry break the checkers forbid.
    pub disposition: Option<Disposition>,
}

/// Names every format `source` might be, and the dialect of each that this
/// build can settle within `options`.
///
/// Inspection depth, not prefix depth. Dialect classification for the
/// container formats needs the container back: F3D walks and inflates ZIP
/// entries, `FreeCAD` parses the decompressed `Document.xml`, CATIA needs the
/// rebuilt compound container plus a record census. So a candidate at or above
/// [`INSPECTION_FLOOR`] is run through `Codec::inspect`, under the same
/// resource limits an inspection gets and with no *semantic* decode: no
/// geometry is read and no [`cadmpeg_ir::CadIr`] is built.
///
/// Strongest candidate first. Empty when no codec recognized the prefix; more
/// than one entry when the prefix is genuinely ambiguous, which a ZIP carrying
/// no format marker is by design. An entry whose inspection ran out of budget
/// or failed keeps its format and confidence and reports no dialect: the
/// prefix evidence survives a failed reconstruction.
///
/// The `Err` is I/O on `source` itself — reading the prefix, or seeking back
/// to the start before each inspection. A codec's own failure is not an error
/// here; it is a candidate with no dialect.
pub fn identify(
    source: &mut dyn ReadSeek,
    options: &InspectOptions,
) -> std::io::Result<Vec<Identification>> {
    let catalog = InputCatalog::with_builtins();
    identify_with(&catalog, source, options)
}

/// [`identify`], against a caller-held catalog.
///
/// Building the catalog constructs every codec this build carries. A caller
/// identifying many files holds one catalog and reuses it.
pub fn identify_with(
    catalog: &InputCatalog,
    source: &mut dyn ReadSeek,
    options: &InspectOptions,
) -> std::io::Result<Vec<Identification>> {
    let prefix = read_prefix(source, options)?;
    let candidates = catalog.candidates(&prefix);

    let mut identifications = Vec::with_capacity(candidates.len());
    for (descriptor, confidence) in candidates {
        let mut identification = Identification {
            format: descriptor.format_id(),
            confidence,
            dialect: None,
            declared: BTreeMap::new(),
            disposition: None,
        };
        if confidence >= INSPECTION_FLOOR {
            // Every candidate carries a codec: detection asked one for the
            // confidence that put the descriptor in this list.
            let codec = descriptor
                .codec
                .as_deref()
                .expect("a detected descriptor has a codec");
            source.seek(SeekFrom::Start(0))?;
            if let Ok(summary) = codec.inspect(source, options) {
                if let Some(entry) = primary_layer(&summary.dialects, &summary.format) {
                    identification.dialect.clone_from(&entry.dialect);
                    identification.declared.clone_from(&entry.declared);
                }
            }
        }
        identification.disposition = identification.dialect.as_ref().and_then(support);
        identifications.push(identification);
    }
    Ok(identifications)
}

/// Reads the detection window and leaves `source` at the start.
///
/// Bounded by the inspection's own input limit as well as the window: a
/// caller that capped the input has capped what detection may look at too.
fn read_prefix(source: &mut dyn ReadSeek, options: &InspectOptions) -> std::io::Result<Vec<u8>> {
    let window = u64::try_from(DETECTION_PREFIX_LEN)
        .unwrap_or(u64::MAX)
        .min(options.limits.max_input_bytes);
    source.seek(SeekFrom::Start(0))?;
    let mut prefix = Vec::new();
    (&mut *source).take(window).read_to_end(&mut prefix)?;
    source.seek(SeekFrom::Start(0))?;
    Ok(prefix)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cadmpeg_core::decode::ResourceLimits;

    use super::*;

    fn inspection() -> InspectOptions {
        InspectOptions {
            limits: ResourceLimits::desktop(),
        }
    }

    fn run(bytes: &[u8], options: &InspectOptions) -> Vec<Identification> {
        let mut reader = Cursor::new(bytes.to_vec());
        identify(&mut reader, options).expect("a Cursor neither fails to read nor to seek")
    }

    /// The per-format fixture table and the case it drives.
    ///
    /// Gated once, on the features that contribute a fixture, so a build
    /// carrying none of them compiles without an empty table to walk.
    #[cfg(any(
        feature = "rhino",
        feature = "iges",
        feature = "step",
        feature = "f3d",
        feature = "fcstd",
        feature = "sldprt",
        feature = "catia",
        feature = "creo",
    ))]
    mod declared {
        use super::*;
        use crate::support::{ReadDisposition, WriteDisposition};

        /// One fixture, and what the two registries say it is.
        ///
        /// The dialect ids and the two dispositions are the rows
        /// `docs/dialect-support.toml` carries for these exact files, which is what
        /// makes each case a join over three sources: the codec's classifier, the
        /// identity registry, and the capability registry.
        struct Case {
            bytes: &'static [u8],
            format: &'static str,
            dialect: &'static str,
            read: ReadDisposition,
            write: WriteDisposition,
        }

        fn cases() -> Vec<Case> {
            vec![
                #[cfg(feature = "rhino")]
                Case {
                    bytes: include_bytes!(
                        "../../cadmpeg-codec-rhino/tests/golden/fixtures/arc.3dm"
                    ),
                    format: "rhino",
                    dialect: "rhino:archive-50",
                    read: ReadDisposition::Level(1),
                    write: WriteDisposition::Emitted,
                },
                #[cfg(feature = "iges")]
                Case {
                    bytes: include_bytes!(
                        "../../cadmpeg-codec-iges/tests/golden/fixtures/analytic_cone_surface.igs"
                    ),
                    format: "iges",
                    dialect: "iges:5.3-fixed-ascii",
                    read: ReadDisposition::Level(9),
                    write: WriteDisposition::Verified,
                },
                #[cfg(feature = "step")]
                Case {
                    bytes: include_bytes!(
                        "../../cadmpeg-codec-step/tests/fixtures/ap203_sheet.p21"
                    ),
                    format: "step",
                    dialect: "step:ap203-e1",
                    read: ReadDisposition::Level(9),
                    write: WriteDisposition::Emitted,
                },
                #[cfg(feature = "f3d")]
                Case {
                    bytes: include_bytes!(
                        "../../cadmpeg-codec-f3d/tests/golden/fixtures/attributes.f3d"
                    ),
                    format: "f3d",
                    dialect: "f3d:manifest-3-2-0-0",
                    read: ReadDisposition::Level(4),
                    write: WriteDisposition::Verified,
                },
                #[cfg(feature = "fcstd")]
                Case {
                    bytes: include_bytes!(
                        "../../../corpus/freecad_fcstd/fixtures/application_payloads.FCStd"
                    ),
                    format: "fcstd",
                    dialect: "fcstd:schema-4",
                    read: ReadDisposition::Level(5),
                    write: WriteDisposition::Verified,
                },
                #[cfg(feature = "sldprt")]
                Case {
                    bytes: include_bytes!(
                        "../../cadmpeg-codec-sldprt/tests/golden/fixtures/analytic_cylinder.sldprt"
                    ),
                    format: "sldprt",
                    dialect: "sldprt:unknown",
                    read: ReadDisposition::UnclassifiedRecovered,
                    write: WriteDisposition::Emitted,
                },
                #[cfg(feature = "catia")]
                Case {
                    bytes: include_bytes!(
                    "../../cadmpeg-codec-catia/tests/golden/fixtures/fbb_only_fallthrough.catpart"
                ),
                    format: "catia",
                    dialect: "catia:fbb-only",
                    read: ReadDisposition::Level(1),
                    write: WriteDisposition::None,
                },
                #[cfg(feature = "creo")]
                Case {
                    bytes: include_bytes!(
                        "../../cadmpeg-codec-creo/tests/golden/fixtures/depdb_recipe_history.prt"
                    ),
                    format: "creo",
                    dialect: "creo:depdb",
                    read: ReadDisposition::Level(1),
                    write: WriteDisposition::None,
                },
            ]
        }

        /// One fixture per format: identification names the format, settles the
        /// dialect, and carries the declared disposition for it.
        #[test]
        fn a_fixture_of_each_format_identifies_to_its_declared_disposition() {
            let cases = cases();
            assert!(
                !cases.is_empty(),
                "the feature gate on this test must match the table it walks"
            );
            for case in cases {
                let found = run(case.bytes, &inspection());
                let winner = found
                    .first()
                    .unwrap_or_else(|| panic!("{}: no candidate at all", case.format));
                assert_eq!(winner.format, case.format, "{found:?}");
                assert!(
                    winner.confidence >= INSPECTION_FLOOR,
                    "{}: a fixture of its own format must reach the inspection floor, got {}",
                    case.format,
                    winner.confidence
                );
                assert_eq!(
                    winner.dialect.as_ref().map(DialectId::as_str),
                    Some(case.dialect),
                    "{}: dialect",
                    case.format
                );
                assert_eq!(
                    winner.disposition,
                    Some(Disposition {
                        read: case.read,
                        write: case.write,
                    }),
                    "{}: disposition",
                    case.format
                );
            }
        }
    }

    /// A ZIP with no format marker is several formats at once, and the answer
    /// says so instead of picking one.
    ///
    /// Every ZIP-based codec reports [`Confidence::Low`] for it, which is below
    /// the inspection floor, so no container is opened and no dialect is
    /// claimed. This is the design's `Ambiguous { Low, [fcstd, f3d, step] }`.
    #[cfg(all(feature = "fcstd", feature = "f3d"))]
    #[test]
    fn a_markerless_zip_identifies_as_several_formats_with_no_dialect() {
        let found = run(b"PK\x03\x04 markerless", &inspection());
        assert!(found.len() > 1, "{found:?}");
        for identification in &found {
            assert_eq!(identification.confidence, Confidence::Low);
            assert_eq!(identification.dialect, None);
            assert_eq!(identification.disposition, None);
            assert!(identification.declared.is_empty());
        }
        let formats = found
            .iter()
            .map(|identification| identification.format)
            .collect::<Vec<_>>();
        assert!(
            formats.contains(&"fcstd") && formats.contains(&"f3d"),
            "{formats:?}"
        );
    }

    /// A budget too small to rebuild the container keeps the prefix evidence
    /// and reports no dialect.
    ///
    /// The prefix still names the format at high confidence; only the
    /// reconstruction that would settle the dialect is refused. Reporting a
    /// dialect here would be reporting one nothing read.
    #[cfg(feature = "rhino")]
    #[test]
    fn an_exhausted_budget_keeps_the_format_and_drops_the_dialect() {
        let bytes = include_bytes!("../../cadmpeg-codec-rhino/tests/golden/fixtures/arc.3dm");
        let starved = InspectOptions {
            limits: ResourceLimits {
                // Enough for the 32-byte archive header the prefix detector
                // reads, far short of the whole file the inspection acquires.
                max_input_bytes: 64,
                ..ResourceLimits::desktop()
            },
        };
        assert!(bytes.len() > 64);

        let found = run(bytes, &starved);
        let winner = found.first().expect("the prefix still names rhino");
        assert_eq!(winner.format, "rhino");
        assert_eq!(winner.confidence, Confidence::High);
        assert_eq!(winner.dialect, None);
        assert_eq!(winner.disposition, None);

        // The same bytes under the default budget do settle the dialect, so
        // the difference above is the budget and not the fixture.
        assert!(run(bytes, &inspection())[0].dialect.is_some());
    }

    /// Bytes no codec recognizes produce no candidates at all.
    #[test]
    fn an_unrecognized_prefix_names_nothing() {
        assert!(run(b"not a CAD file at all\n", &inspection()).is_empty());
    }
}
