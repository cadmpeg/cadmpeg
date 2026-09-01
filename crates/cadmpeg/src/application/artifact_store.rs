// SPDX-License-Identifier: Apache-2.0
//! CADIR-plus-sidecar persistence for the CLI.
//!
//! CADIR and its decode-fidelity sidecar are written as two separate atomic
//! renames. Those renames are not transactional: a crash between them can leave
//! a stale sidecar beside a newer CADIR. Digest verification failing closed on
//! the next load is what saves the caller, not a transactional pair.

use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use cadmpeg_container::compound::read_detection_prefix;
use cadmpeg_ir::codec::ExportPlan;
use cadmpeg_ir::report::{DecodeReport, ExportReport};
use cadmpeg_ir::{decode_sidecar_path, DecodeSidecar, SourceFidelity};
use sha2::{Digest, Sha256};

/// Owner of bounded reads, sidecar paths, digest checks, and atomic writes.
///
/// Methods are associated functions: the type names the owner; there is no
/// instance state.
#[derive(Debug, Default, Clone, Copy)]
pub struct ArtifactStore;

impl ArtifactStore {
    /// Sidecar path for a CADIR path (`<stem>.fidelity.json`).
    pub fn sidecar_path(cadir_path: &Path) -> PathBuf {
        decode_sidecar_path(cadir_path)
    }

    /// Read the bytes used for native-format detection.
    ///
    /// Most codecs need only the leading prefix. A Compound File Binary
    /// directory may be physically remote from the header, so its codec
    /// evidence cannot be established from a short prefix. Extend such inputs
    /// until the bounded CFB probe reaches the directory or the configured
    /// input ceiling.
    pub fn read_detection_input(path: &Path, prefix_len: usize, max_bytes: u64) -> Result<Vec<u8>> {
        let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        read_detection_prefix(&mut file, prefix_len, max_bytes).map_err(|error| {
            if error.kind() == io::ErrorKind::FileTooLarge {
                anyhow!(
                    "{} exceeds the configured {}-byte input limit",
                    path.display(),
                    max_bytes
                )
            } else {
                error.into()
            }
        })
    }

    /// Read a UTF-8 text file, refusing payloads above `max_bytes`.
    pub fn read_bounded_text(path: &Path, max_bytes: u64) -> Result<String> {
        let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut limited = file.take(max_bytes.saturating_add(1));
        let mut text = String::new();
        limited
            .read_to_string(&mut text)
            .with_context(|| format!("reading UTF-8 text from {}", path.display()))?;
        if text.len() as u64 > max_bytes {
            return Err(anyhow!(
                "{} exceeds the configured {}-byte input limit",
                path.display(),
                max_bytes
            ));
        }
        Ok(text)
    }

    /// Load and parse a decode sidecar, verifying it against CADIR bytes.
    ///
    /// Mismatch is a hard error (fail-closed).
    pub fn load_matching_sidecar(
        cadir_path: &Path,
        cadir_bytes: &[u8],
        max_bytes: u64,
    ) -> Result<Option<DecodeSidecar>> {
        let path = Self::sidecar_path(cadir_path);
        if !path.exists() {
            return Ok(None);
        }
        let text = Self::read_bounded_text(&path, max_bytes)
            .with_context(|| format!("reading decode sidecar {}", path.display()))?;
        let sidecar = DecodeSidecar::from_json(&text)
            .with_context(|| format!("parsing decode sidecar {}", path.display()))?;
        if !sidecar.matches(cadir_bytes) {
            return Err(anyhow!(
                "decode sidecar {} does not match {}",
                path.display(),
                cadir_path.display()
            ));
        }
        Ok(Some(sidecar))
    }

    /// Refuse to overwrite the input path, or an existing output without force.
    pub fn check_output_path(input: &Path, output: &Path, force: bool) -> Result<()> {
        let input = std::fs::canonicalize(input)
            .with_context(|| format!("canonicalizing {}", input.display()))?;
        let output_absolute = Self::absolute_output_path(output)?;
        if input == output_absolute {
            bail!("refusing to overwrite input {}", input.display());
        }
        if output.exists() && !force {
            bail!("{} exists; pass --force to overwrite", output.display());
        }
        Ok(())
    }

    /// Refuse two independently written outputs that resolve to one path.
    pub fn check_distinct_output_paths(
        first: &Path,
        first_label: &str,
        second: &Path,
        second_label: &str,
    ) -> Result<()> {
        if Self::absolute_output_path(first)? == Self::absolute_output_path(second)? {
            bail!(
                "{first_label} and {second_label} resolve to the same path {}; choose distinct output paths",
                first.display()
            );
        }
        Ok(())
    }

    fn absolute_output_path(output: &Path) -> Result<PathBuf> {
        let parent = output
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if output.exists() {
            Ok(std::fs::canonicalize(output)?)
        } else {
            Ok(std::fs::canonicalize(parent)?.join(
                output
                    .file_name()
                    .ok_or_else(|| anyhow!("output path has no filename"))?,
            ))
        }
    }

    /// Stage bytes then atomically replace `output`.
    pub fn write_bytes_atomic(output: &Path, bytes: &[u8]) -> Result<()> {
        let parent = output
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .with_context(|| format!("creating temporary output in {}", parent.display()))?;
        temporary
            .write_all(bytes)
            .with_context(|| format!("writing temporary output for {}", output.display()))?;
        temporary
            .persist(output)
            .map_err(|error| error.error)
            .with_context(|| format!("persisting temporary output to {}", output.display()))?;
        Ok(())
    }

    /// Check the output path, then write bytes atomically.
    pub fn write_output(input: &Path, output: &Path, bytes: &[u8], force: bool) -> Result<()> {
        Self::check_output_path(input, output, force)?;
        Self::write_bytes_atomic(output, bytes)
    }

    /// Stage an export plan, optionally hashing CADIR bytes for the sidecar.
    pub fn write_plan_atomic(
        output: &Path,
        plan: ExportPlan,
        with_digest: bool,
    ) -> Result<(ExportReport, Option<String>)> {
        let parent = output
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .with_context(|| format!("creating temporary output in {}", parent.display()))?;
        let mut sink = TempFileWriter {
            file: &mut temporary,
            hasher: with_digest.then(Sha256::new),
        };
        let mut writer = BufWriter::new(&mut sink);
        let report = plan
            .write_to(&mut writer)
            .with_context(|| format!("writing temporary output for {}", output.display()))?;
        writer
            .flush()
            .with_context(|| format!("flushing temporary output for {}", output.display()))?;
        drop(writer);
        let digest = sink.finish();
        temporary
            .persist(output)
            .map_err(|error| error.error)
            .with_context(|| format!("persisting temporary output to {}", output.display()))?;
        Ok((report, digest))
    }

    /// Persist or remove the decode-fidelity sidecar beside a CADIR file.
    ///
    /// When both report and fidelity are present this is a second atomic rename
    /// after the CADIR write. The pair is not transactional; see the module
    /// docs. When origin metadata is absent, a stale sidecar is removed.
    pub fn persist_decode_sidecar(
        cadir_path: &Path,
        cadir_sha256: Option<&str>,
        report: Option<&DecodeReport>,
        fidelity: Option<&SourceFidelity>,
    ) -> Result<SidecarPersistOutcome> {
        let path = Self::sidecar_path(cadir_path);
        match (report, fidelity) {
            (Some(report), Some(fidelity)) => {
                let cadir_sha256 = cadir_sha256.ok_or_else(|| {
                    anyhow!("missing CADIR digest while writing decode-fidelity sidecar")
                })?;
                let sidecar =
                    DecodeSidecar::bind_sha256(cadir_sha256, report.clone(), fidelity.clone());
                let mut bytes = sidecar.to_canonical_json()?.into_bytes();
                bytes.push(b'\n');
                Self::write_bytes_atomic(&path, &bytes)?;
                Ok(SidecarPersistOutcome::Wrote(path))
            }
            _ if path.exists() => {
                std::fs::remove_file(&path)
                    .with_context(|| format!("removing stale decode sidecar {}", path.display()))?;
                Ok(SidecarPersistOutcome::RemovedStale(path))
            }
            _ => Ok(SidecarPersistOutcome::Absent),
        }
    }
}

/// Result of attempting to keep a CADIR sidecar in sync with its document.
#[derive(Debug)]
pub enum SidecarPersistOutcome {
    /// Sidecar written beside the CADIR file.
    Wrote(PathBuf),
    /// Stale sidecar removed because the CADIR has no decode origin.
    RemovedStale(PathBuf),
    /// No sidecar present and none required.
    Absent,
}

struct TempFileWriter<'a> {
    file: &'a mut tempfile::NamedTempFile,
    hasher: Option<Sha256>,
}

impl TempFileWriter<'_> {
    fn finish(self) -> Option<String> {
        self.hasher.map(|hasher| {
            let digest = hasher.finalize();
            let mut encoded = String::with_capacity(digest.len() * 2);
            for byte in digest {
                write!(encoded, "{byte:02x}").expect("writing a digest to a String");
            }
            encoded
        })
    }
}

impl Write for TempFileWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.file.write(bytes)?;
        if let Some(hasher) = &mut self.hasher {
            hasher.update(&bytes[..written]);
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::default_trait_access)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use cadmpeg_core::decode::InspectOptions;
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::{CadIr, DecodeReport, SourceFidelity};
    use cadmpeg_registry::{identify, InputCatalog, DETECTION_PREFIX_LEN};

    #[test]
    fn matching_sidecar_loads_and_mismatch_fails_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("part.cadir.json");
        let text = CadIr::empty(Units::default()).to_canonical_json().unwrap();
        std::fs::write(&path, &text).unwrap();
        let report = DecodeReport::unclassified(
            "test",
            cadmpeg_ir::DecodeTransfer::full(false),
            Default::default(),
            Vec::new(),
            Vec::new(),
            cadmpeg_ir::report::TransferLedger::default(),
        );
        let sidecar = DecodeSidecar::bind(text.as_bytes(), report, SourceFidelity::default());
        std::fs::write(
            ArtifactStore::sidecar_path(&path),
            sidecar.to_canonical_json().unwrap(),
        )
        .unwrap();

        let loaded =
            ArtifactStore::load_matching_sidecar(&path, text.as_bytes(), 1024 * 1024).unwrap();
        assert!(loaded.is_some());

        std::fs::write(&path, format!("{text}\n")).unwrap();
        let error = ArtifactStore::load_matching_sidecar(
            &path,
            format!("{text}\n").as_bytes(),
            1024 * 1024,
        )
        .unwrap_err();
        assert!(error.to_string().contains("does not match"));
    }

    #[test]
    fn text_reader_refuses_input_above_the_configured_limit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large.json");
        std::fs::write(&path, "12345").unwrap();
        let error = ArtifactStore::read_bounded_text(&path, 4).unwrap_err();
        assert!(error.to_string().contains("4-byte input limit"));
    }

    #[test]
    fn compound_detection_probes_beyond_a_short_prefix() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("part.prt");
        let mut bytes = vec![0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
        bytes.extend(std::iter::repeat_n(0x5a, 128));
        std::fs::write(&path, &bytes).unwrap();

        let detected = ArtifactStore::read_detection_input(&path, 8, 1024).unwrap();
        assert_eq!(detected, bytes);
    }

    #[test]
    fn non_compound_detection_under_a_small_limit_returns_a_prefix() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("part.igs");
        std::fs::write(&path, vec![b'x'; 1024]).unwrap();

        let detected = ArtifactStore::read_detection_input(&path, DETECTION_PREFIX_LEN, 16)
            .expect("a non-CFB detection prefix respects the limit without refusing the file");

        assert_eq!(detected, vec![b'x'; 16]);
    }

    #[cfg(feature = "nx")]
    #[test]
    fn identify_and_cli_detection_reach_remote_compound_directory_evidence() {
        let bytes = cfb_with_remote_ug_part_directory();
        assert!(bytes.len() > DETECTION_PREFIX_LEN);

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("remote-directory.prt");
        std::fs::write(&path, &bytes).unwrap();
        let cli_prefix =
            ArtifactStore::read_detection_input(&path, DETECTION_PREFIX_LEN, bytes.len() as u64)
                .unwrap();
        assert_eq!(cli_prefix, bytes);

        let cli_candidates = InputCatalog::with_builtins()
            .candidates(&cli_prefix)
            .into_iter()
            .map(|(descriptor, confidence)| (descriptor.format_id(), confidence))
            .collect::<Vec<_>>();
        let mut source = Cursor::new(bytes);
        let library_candidates = identify(&mut source, &InspectOptions::default())
            .unwrap()
            .into_iter()
            .map(|identified| (identified.format, identified.confidence))
            .collect::<Vec<_>>();

        assert!(cli_candidates.iter().any(|(format, _)| *format == "nx"));
        assert_eq!(library_candidates, cli_candidates);
    }

    #[cfg(feature = "nx")]
    fn cfb_with_remote_ug_part_directory() -> Vec<u8> {
        const SECTOR: usize = 512;
        const DIRECTORY_SECTOR: usize = 256;
        const END: u32 = 0xffff_fffe;
        const FREE: u32 = 0xffff_ffff;
        const FAT: u32 = 0xffff_fffd;
        let mut file = vec![0; SECTOR * (DIRECTORY_SECTOR + 2)];
        file[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
        put_u16(&mut file, 24, 0x003e);
        put_u16(&mut file, 26, 3);
        put_u16(&mut file, 28, 0xfffe);
        put_u16(&mut file, 30, 9);
        put_u16(&mut file, 32, 6);
        put_u32(&mut file, 44, 3);
        put_u32(&mut file, 48, DIRECTORY_SECTOR as u32);
        put_u32(&mut file, 56, 4096);
        put_u32(&mut file, 60, END);
        put_u32(&mut file, 68, END);
        for index in 0..109 {
            put_u32(&mut file, 76 + index * 4, FREE);
        }
        for index in 0..3 {
            put_u32(&mut file, 76 + index * 4, index as u32);
            file[SECTOR * (index + 1)..SECTOR * (index + 2)].fill(0xff);
            put_u32(&mut file, SECTOR + index * 4, FAT);
        }
        put_u32(&mut file, SECTOR * 3 + (DIRECTORY_SECTOR - 256) * 4, END);

        let directory = SECTOR * (DIRECTORY_SECTOR + 1);
        directory_entry(
            &mut file[directory..directory + SECTOR],
            0,
            "Root Entry",
            5,
            FREE,
            1,
        );
        directory_entry(
            &mut file[directory..directory + SECTOR],
            1,
            "UG_PART",
            1,
            FREE,
            2,
        );
        directory_entry(
            &mut file[directory..directory + SECTOR],
            2,
            "UG_PART",
            2,
            END,
            FREE,
        );
        file
    }

    #[cfg(feature = "nx")]
    fn directory_entry(
        directory: &mut [u8],
        index: usize,
        name: &str,
        kind: u8,
        start: u32,
        child: u32,
    ) {
        const FREE: u32 = 0xffff_ffff;
        let entry = &mut directory[index * 128..(index + 1) * 128];
        let mut encoded = name.encode_utf16().collect::<Vec<_>>();
        encoded.push(0);
        for (offset, word) in encoded.iter().enumerate() {
            put_u16(entry, offset * 2, *word);
        }
        put_u16(entry, 64, (encoded.len() * 2) as u16);
        entry[66] = kind;
        entry[67] = 1;
        put_u32(entry, 68, FREE);
        put_u32(entry, 72, FREE);
        put_u32(entry, 76, child);
        put_u32(entry, 116, start);
    }

    #[cfg(feature = "nx")]
    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    #[cfg(feature = "nx")]
    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}
