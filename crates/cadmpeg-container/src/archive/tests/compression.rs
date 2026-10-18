use super::*;

/// The raw fixture builder is not silently laying out a different archive from `ZipWriter`.
///
/// `archive_bytes` cannot use `zip::ZipWriter` because `zip` without its `zstd` feature
/// cannot write method 93, which replaces a well-tested writer with sixty lines in this repository.  On
/// the two methods both can write, the records `ArchiveSnapshot` derives must agree, so a
/// layout mistake in the builder shows up here rather than as a decoder failure.
#[test]
fn the_raw_builder_and_zip_writer_produce_the_same_records() {
    let mut written = zip::ZipWriter::new(Cursor::new(Vec::new()));
    written
        .start_file(
            "stored.bin",
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .expect("stored entry starts");
    written.write_all(b"stored").expect("stored entry writes");
    written
        .start_file(
            "deflated.bin",
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .expect("deflated entry starts");
    written
        .write_all(b"deflated payload")
        .expect("deflated entry writes");
    let written = written.finish().expect("archive finishes").into_inner();

    let built = zip_fixture::archive(&[
        RawEntry::stored("stored.bin", b"stored"),
        RawEntry::compressed(
            "deflated.bin",
            zip_fixture::DEFLATE,
            &deflate(b"deflated payload"),
            b"deflated payload",
        ),
    ]);

    // Every LOCAL header must agree with its central record, and the end-of-central-directory
    // record must agree with what was actually written. `zip::ZipArchive` validates neither:
    // measured, it refuses a central-directory OFFSET that is off by one and accepts both a
    // wrong `central_size` and a wrong total entry count, and it never reads a local header
    // at all through this path. So the builder could emit an archive whose local headers say
    // "stored, crc 0, sizes transposed" while the central directory says "zstd, crc X" -- an
    // archive no streaming reader would agree with -- and the comparison below, which is
    // built entirely from central-directory fields, would stay green.
    //
    // A byte-for-byte comparison against `ZipWriter` is NOT the fix and was measured: both
    // producers emit 242 bytes for this pair but share only a 4-byte prefix, because
    // `ZipWriter` writes a different version-needed and a real modification time. Agreement
    // between the two headers is the property; identity with another writer is not.
    assert_local_headers_agree(&built);
    assert_local_headers_agree(&written);

    let arena = DecodeArena::new();
    let mut readings = Vec::new();
    for bytes in [&written, &built] {
        let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
        readings.push(
            snapshot
                .entries()
                .iter()
                .map(|entry| {
                    let view = snapshot.open(&ctx, &entry.name).expect("entry opens");
                    (
                        entry.name.clone(),
                        entry.compression,
                        entry.crc32,
                        entry.uncompressed_size,
                        view.window().to_vec(),
                    )
                })
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(readings[0], readings[1]);
}

/// The expansion loop reassembles a payload larger than one 16 KiB chunk, for BOTH methods.
///
/// Every other fixture in this file is smaller than one read.  Measured, that leaves the
/// classic `Read` misuse -- treating a short read as end of stream -- a no-op on the whole
/// suite: the golden Zstandard entry reads `16384, 16384, 3584, 0`, so its only short read
/// IS the last, and the deflate fixtures are 16 and 32 bytes.  A large enough deflate
/// member makes `flate2` return short reads mid-stream, and under that misuse such a member
/// decodes truncated while every other test stays green.  Deflate is by far the commonest
/// method in real containers, so this is the arm that most needs the fixture.
///
/// The payload is generated rather than committed, and is deliberately not uniform: a run of
/// one byte compresses to almost nothing and would not produce multiple reads on the way
/// out.
#[test]
fn a_payload_larger_than_one_read_is_reassembled_for_every_method() {
    let plain = zip_fixture::zstd_golden_large_plaintext();
    assert!(
        plain.len() > 16 * 1024 * 4,
        "the payload must span many reads"
    );
    let entries = [
        RawEntry::stored("stored.bin", &plain),
        RawEntry::compressed(
            "deflated.bin",
            zip_fixture::DEFLATE,
            &deflate(&plain),
            &plain,
        ),
        RawEntry::compressed(
            "zstd.bin",
            zip_fixture::ZSTD,
            zip_fixture::ZSTD_GOLDEN_LARGE_FRAME,
            &plain,
        ),
    ];
    let bytes = zip_fixture::archive(&entries);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("archive fits root policy");
    let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
    for entry in &entries {
        let record = snapshot.entry(&entry.name).expect("record");
        let view = snapshot.open(&ctx, &record.name).expect("entry opens");
        // The whole payload, not merely a successful call: a truncating decoder returns Ok
        // for as long as the guards downstream let it.
        assert_eq!(
            view.window(),
            plain.as_slice(),
            "{} did not round-trip in full",
            entry.name
        );
    }
}

/// `open_expanded` must not treat a SHORT read as end of stream, nor an error as one.
///
/// This drives the expansion loop directly instead of through a decoder, and that is the
/// point. The obvious fixture -- a
/// payload larger than one 16 KiB chunk -- does NOT hold this property: measured, both
/// `flate2` and `ruzstd` return exact 16 384-byte reads until the tail at 20 KB and at
/// 200 KB (`16384 x 12, 3392, 0`), so a plant that returns after the first short read is a
/// no-op on every fixture in this file. It is not a no-op on real input -- a large enough
/// deflate member makes `flate2` return MID-STREAM short reads, and the member then decodes
/// truncated -- but sizing a fixture until a dependency
/// happens to fragment is a test of that dependency's buffering, not of this function,
/// and it would go quiet the next time `flate2` changed.
///
/// A chunk reader is permitted to return fewer bytes than the buffer holds at any time
/// and `0` only at end of stream, so the reader below is a conforming one that exercises
/// exactly the contract.
#[test]
fn open_expanded_is_not_defeated_by_a_short_read_or_an_error() {
    let plain = zip_fixture::zstd_golden_plaintext();
    // Only the record matters here: it declares `plain`'s size and CRC-32.
    let bytes = zip_fixture::archive(&[RawEntry::compressed(
        "probe",
        zip_fixture::DEFLATE,
        &deflate(&plain),
        &plain,
    )]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).expect("root");
    let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
    let entry = snapshot.entry("probe").expect("record");

    // Short reads must be reassembled in full.
    for step in [1_usize, 7, 4096] {
        let mut rest = plain.as_slice();
        let view = ArchiveSnapshot::open_expanded(&ctx, entry, |chunk| {
            let n = step.min(chunk.len()).min(rest.len());
            chunk[..n].copy_from_slice(&rest[..n]);
            rest = &rest[n..];
            Ok(n)
        })
        .expect("expansion succeeds");
        assert_eq!(
            view.window(),
            plain.as_slice(),
            "a {step}-byte read cadence lost bytes"
        );
    }

    // A mid-stream error must propagate, not read as end of stream.
    let mut served = 0_usize;
    let result = ArchiveSnapshot::open_expanded(&ctx, entry, |chunk| {
        if served >= 4096 {
            return Err(CodecError::Malformed(
                "planted mid-stream read error".into(),
            ));
        }
        let n = 4096.min(chunk.len());
        chunk[..n].copy_from_slice(&plain[served..served + n]);
        served += n;
        Ok(n)
    });
    assert!(
        matches!(&result, Err(CodecError::Malformed(message)) if message == "planted mid-stream read error"),
        "a mid-stream read error was swallowed: {result:?}"
    );
}

/// A wrong CRC-32 in the central directory is refused, on every method.
///
/// The phase's argument for why a decoder disagreement cannot become wrong output rests on
/// two guards: `ExpandSpec::Exact` and this CRC.  `Exact` is held by
/// `a_zstandard_entry_whose_declared_size_disagrees_is_refused`; this one was held by
/// nothing, and disabling it left all 34 tests green.  It is genuinely redundant against
/// everything the suite can currently produce -- with both guards disabled the full
/// 103 275-mutation sweep still reports zero wrong outputs -- which is a reason to keep the
/// guard and write the case, not to delete either.
#[test]
fn a_wrong_crc32_is_refused_on_every_method() {
    let plain = zip_fixture::zstd_golden_plaintext();
    let plain = plain.as_slice();
    for (label, mut entry) in [
        ("stored", RawEntry::stored("payload.bin", plain)),
        (
            "deflate",
            RawEntry::compressed("payload.bin", zip_fixture::DEFLATE, &deflate(plain), plain),
        ),
        (
            "zstd",
            RawEntry::compressed(
                "payload.bin",
                zip_fixture::ZSTD,
                zip_fixture::ZSTD_GOLDEN_FRAME,
                plain,
            ),
        ),
    ] {
        // Control first: the untouched entry opens and returns the payload.
        let bytes = zip_fixture::archive(std::slice::from_ref(&entry));
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
        let record = snapshot.entry("payload.bin").expect("record");
        assert_eq!(
            snapshot
                .open(&ctx, &record.name)
                .expect("control opens")
                .window(),
            plain,
            "{label}: the control did not open"
        );

        entry.crc32 ^= 0x0000_0001;
        let bytes = zip_fixture::archive(std::slice::from_ref(&entry));
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
        let record = snapshot.entry("payload.bin").expect("record");
        assert!(
            snapshot.open(&ctx, &record.name).is_err(),
            "{label}: a wrong CRC-32 was accepted"
        );
    }
}

/// The Zstandard window cap is the decode policy's per-expansion allowance, checked on
/// BOTH sides, and its refusal leaves the session open.
///
/// `cadmpeg_core`'s `open_zstd` admits a frame whose buffered output -- its window, or its
/// declared content size when that is smaller -- fits the per-expansion allowance. For an
/// archive this small the allowance is its 16 MiB base, so windowLog 24 decodes and one
/// mantissa step past it is refused as `NotImplemented`, with no session refusal.
///
/// The frames are built here rather than committed, and are deliberately minimal: a header
/// declaring a window and one raw block. That makes this a boundary test of the ceiling and
/// NOT an interop test -- the frames come from this file, not from a producer.
#[test]
fn the_zstandard_window_cap_is_the_policys_and_bites_on_both_sides() {
    /// A frame declaring `window_log` and `mantissa`, carrying one raw block of `payload`.
    fn frame_declaring(window_log: u8, mantissa: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0x28, 0xb5, 0x2f, 0xfd];
        out.push(0x00); // no Frame_Content_Size, not single-segment, no checksum
        out.push((window_log - 10) << 3 | mantissa);
        let size = u32::try_from(payload.len()).expect("fixture block fits");
        let header = size << 3 | 1; // last block, Raw_Block
        out.extend_from_slice(&header.to_le_bytes()[..3]);
        out.extend_from_slice(payload);
        out
    }

    let payload = b"abcd";
    let desktop = DecodePolicy::default();
    let service = DecodePolicy::service();
    for (policy, window_log, mantissa, accepted) in [
        (&desktop, 23_u8, 0_u8, true),
        (&desktop, 24, 0, true),
        (&desktop, 24, 1, false),
        (&desktop, 27, 0, false),
        (&service, 24, 0, true),
        (&service, 24, 1, false),
        (&service, 30, 0, false),
    ] {
        let frame = frame_declaring(window_log, mantissa, payload);
        let bytes = zip_fixture::archive(&[RawEntry::compressed(
            "payload.bin",
            zip_fixture::ZSTD,
            &frame,
            payload,
        )]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, policy)
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
        let got = snapshot
            .open(&ctx, "payload.bin")
            .map(|view| view.window().to_vec());
        if accepted {
            assert_eq!(
                got.as_deref().ok(),
                Some(payload.as_slice()),
                "windowLog {window_log} mantissa {mantissa}: got {got:?}"
            );
        } else {
            assert!(
                matches!(&got, Err(CodecError::NotImplemented(message))
                    if message.ends_with("per-expansion allowance")),
                "windowLog {window_log} mantissa {mantissa}: got {got:?}"
            );
            assert_eq!(ctx.resource_refusal(), None);
        }
    }
}

/// A frame carrying NO content checksum decodes, and its guards are the remaining ones.
///
/// The decoder's checksum comparison is skipped whenever the frame declares no checksum --
/// legal, and the case the Zstandard CLI accepts too.  It is also the case this container
/// will actually meet: libzstd defaults `ZSTD_c_checksumFlag` to 0 and `zip`'s own zstd
/// writer never sets it, so every method-93 entry that writer produced is checksum-free.
/// The committed golden is the unusual one. Without this fixture, making an absent checksum
/// REFUSE left all tests green.
#[test]
fn a_frame_without_a_content_checksum_decodes_and_is_still_guarded() {
    let plain = zip_fixture::zstd_golden_plaintext();
    assert_eq!(
        open_one_zstd_entry(zip_fixture::ZSTD_GOLDEN_NOCHECK_FRAME, &plain)
            .expect("a checksum-free frame must decode"),
        plain
    );

    // With the checksum gone, the CRC-32 and the decoder's framing of the whole entry are
    // what is left. Both are asserted here rather than assumed, because the argument that a
    // decoder disagreement cannot become wrong output rests on exactly them.
    let mut frame = zip_fixture::ZSTD_GOLDEN_NOCHECK_FRAME.to_vec();
    frame.extend_from_slice(b"tail");
    assert!(
        open_one_zstd_entry(&frame, &plain).is_err(),
        "trailing bytes were accepted on a checksum-free frame"
    );
    let bytes = zip_fixture::archive(&[{
        let mut e = RawEntry::compressed(
            "payload.bin",
            zip_fixture::ZSTD,
            zip_fixture::ZSTD_GOLDEN_NOCHECK_FRAME,
            &plain,
        );
        e.crc32 ^= 1;
        e
    }]);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("archive fits root policy");
    let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
    let record = snapshot.entry("payload.bin").expect("record");
    assert!(
        snapshot.open(&ctx, &record.name).is_err(),
        "a wrong CRC-32 was accepted on a checksum-free frame"
    );
}

/// A declared-encrypted entry is refused, whether it says so with the flag or with AE-x.
///
/// `zip` rewrites an AE-x entry's method 99 to the method its 0x9901 extra field names, so
/// a reader that asked `zip` for the method would see an AE-x entry with the encrypted bit
/// CLEAR as ordinary Stored and decode it. The snapshot reads the central record itself
/// (`entry::read`) and never asks `zip` for a method, so the rewrite cannot reach it:
/// method 99 stays 99, which no arm of `open` decodes. Both shapes are refused: the archive
/// is admitted and the member is refused at `open` as `NotImplemented`.
#[test]
fn a_declared_encrypted_entry_is_refused() {
    let plain = b"ciphertext";
    for (label, entry, encrypted, compression) in [
        (
            "general-purpose bit 0",
            RawEntry::stored("payload.bin", plain).with_flags(1),
            true,
            ZipCompression::Stored,
        ),
        (
            "AE-x with the encrypted bit clear",
            RawEntry::stored("payload.bin", plain)
                .aes_without_the_encrypted_bit(zip_fixture::STORED),
            false,
            ZipCompression::Unsupported(99),
        ),
        (
            "AE-x with the encrypted bit set",
            RawEntry::stored("payload.bin", plain)
                .aes_without_the_encrypted_bit(zip_fixture::STORED)
                .with_flags(1),
            true,
            ZipCompression::Unsupported(99),
        ),
    ] {
        // Beside an ordinary member, which must stay readable.
        let bytes = zip_fixture::archive(&[entry, RawEntry::stored("plain.bin", plain)]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("the archive is admitted");
        assert!(
            snapshot.recovery_reason().is_none(),
            "{label}: the directory was not read"
        );
        let record = snapshot.entry("payload.bin").expect("record");
        assert_eq!(record.encrypted, encrypted, "{label}: encrypted flag");
        assert_eq!(record.compression, compression, "{label}: method");
        let opened = snapshot.open(&ctx, "payload.bin");
        assert!(
            matches!(opened, Err(CodecError::NotImplemented(_))),
            "{label}: an encrypted entry was opened: {opened:?}"
        );
        assert_eq!(
            snapshot
                .open(&ctx, "plain.bin")
                .expect("the ordinary member opens")
                .window(),
            plain
        );
    }

    // The control: the same entry without either marking is opened, so the refusals above
    // are about the marking and not about the fixture.
    let bytes = zip_fixture::archive(&[RawEntry::stored("payload.bin", plain)]);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("archive fits root policy");
    let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
    assert_eq!(
        snapshot
            .open(&ctx, "payload.bin")
            .expect("the control opens")
            .window(),
        plain
    );
}

/// A Zstandard frame written by the reference implementation decodes through `ruzstd`.
///
/// Zstandard members are rare in real containers, so ordinary inputs seldom reach this
/// arm; this fixture is what holds it in the unit suite.
/// The frame comes from the Zstandard CLI and the expected bytes are generated, so the two
/// sides of the comparison do not share a producer -- unlike a round trip through this
/// crate's own decoder, which cannot see the two implementations disagreeing.
#[test]
fn a_reference_written_zstandard_entry_decodes() {
    let plain = zip_fixture::zstd_golden_plaintext();
    let bytes = zip_fixture::archive(&[RawEntry::compressed(
        "payload.bin",
        zip_fixture::ZSTD,
        zip_fixture::ZSTD_GOLDEN_FRAME,
        &plain,
    )]);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("archive fits root policy");
    let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
    let entry = snapshot.entry("payload.bin").expect("Zstandard record");
    assert_eq!(entry.compression, ZipCompression::Zstd);
    assert_eq!(entry.compression.label(), "zstd");
    assert_eq!(entry.compressed_size, 405);
    assert_eq!(entry.uncompressed_size, 19_968);
    let view = snapshot
        .open(&ctx, &entry.name)
        .expect("Zstandard entry opens");
    assert_eq!(view.window(), plain.as_slice());
}

/// Every byte of the fixture's every accepted position, measured by the exhaustive sweep
/// (all 255 deltas at all 405 bytes) that `ZSTD_FULL_SWEEP=1` re-runs.  Read as a
/// CEILING, not an equality: see the assertion below.
///
/// Measured 2026-10-12 (103 275 mutations, 100 s, no wrong output).  Bytes 5 and 6 are not
/// in the set: they are the frame's `Frame_Content_Size`, which the decoder compares with
/// what the frame regenerated.
const ZSTD_GOLDEN_ACCEPTING_BYTES: &[usize] = &[4, 18, 19, 285, 290, 294, 301, 302, 334];

/// No corruption of the golden frame yields WRONG output, and every corruption that is
/// accepted at all yields the correct bytes.
///
/// A decoder with no negative case is a gate whose passing form is no output: the positive
/// test above passes just as well against a decoder that returns the expected bytes for
/// anything.  Three things about the shape of this test, each of which was got wrong first:
///
/// **What is asserted is the property, not the reading.**  An earlier version pinned
/// `accepted == [5, 6, 285, 334]`, which goes RED when `ruzstd` gets *stricter* -- i.e. on
/// an improvement -- and a gate that is red for a fix is a gate that gets deleted.  The
/// assertions are therefore: nothing decodes to the wrong bytes; the accepting positions are
/// a SUBSET of the measured set above; and the checksum trailer is refused outright, which
/// is the row this crate's own checksum comparison holds and the one a plant confirms.
/// There is no `assert_eq!(refused, 401)`: every index lands in exactly one bucket, so the
/// count is implied by the one above it.
///
/// **The sampling is stated.**  `frame[i] ^= 1` is one bit of eight and 405 of the 103 275
/// possible single-byte mutations -- 0.39 %, and widening it changes the answer.  The
/// default here sweeps all 255 deltas over the frame header and the trailer, where the
/// structural behaviour lives, and all eight single-bit flips over every byte, so no byte
/// is unswept: 9 168 mutations, ~10 s (eight accepting positions).  The exhaustive 103 275
/// takes 100 s in a debug build (nine), which is a test someone eventually marks
/// `#[ignore]`, so it is opt-in -- and the numbers recorded above come from it.
///
/// **The interesting bytes are 4, 5, 6 and 401-404**, all inside the exhaustive region:
///
/// | byte(s) | what it is | this decoder | Zstandard CLI v1.5.7 |
/// |---|---|---|---|
/// | 4 | frame descriptor | some deltas accepted, output correct | varies |
/// | 5, 6 | `Frame_Content_Size` | refused | refused |
/// | 401-404 | content checksum | refused | refused |
///
/// `ruzstd` compares neither `Frame_Content_Size` nor the content checksum with what it
/// produced.  Both comparisons are made by `cadmpeg_core`'s `open_zstd` decoder, which reads
/// the frame header itself and so can tell an absent size field from one that says 0 --
/// which `ruzstd`'s accessor cannot.  `ExpandSpec::Exact`, fed the central directory's
/// size, refuses independently of both.
/// Bytes 401-404 are refused only because that decoder compares the checksum itself.  Delete
/// the comparison and the assertion that fires is the SUBSET one -- `these bytes now accept
/// a corruption and did not before: [401, 402, 403, 404]` -- not `checksum_bytes_accepted`,
/// which cannot speak while the ceiling excludes those bytes.
#[test]
fn no_corruption_of_the_zstandard_frame_yields_wrong_output() {
    let plain = zip_fixture::zstd_golden_plaintext();
    let len = zip_fixture::ZSTD_GOLDEN_FRAME.len();
    // `is_some()` on the OsString would make `ZSTD_FULL_SWEEP=0` and `=` both run the 76 s
    // sweep, which is how a run gets labelled "default" and silently isn't.
    let full = matches!(
        std::env::var("ZSTD_FULL_SWEEP").as_deref(),
        Ok("1" | "true" | "yes")
    );
    let exhaustive = |i: usize| full || i < 12 || i + 12 >= len;
    let deltas = |i: usize| -> Vec<u8> {
        if exhaustive(i) {
            (1..=255_u8).collect()
        } else {
            (0..8).map(|b| 1_u8 << b).collect()
        }
    };

    let mut accepting = std::collections::BTreeSet::new();
    let mut checksum_bytes_accepted = Vec::new();
    let mut swept_bytes = std::collections::BTreeSet::new();
    let mut swept = 0_usize;
    for index in 0..len {
        for delta in deltas(index) {
            swept += 1;
            swept_bytes.insert(index);
            let mut frame = zip_fixture::ZSTD_GOLDEN_FRAME.to_vec();
            frame[index] ^= delta;
            match open_one_zstd_entry(&frame, &plain) {
                Ok(bytes) if bytes == plain => {
                    accepting.insert(index);
                    if (401..=404).contains(&index) {
                        checksum_bytes_accepted.push((index, delta));
                    }
                }
                Ok(bytes) => panic!(
                    "byte {index} ^ {delta:#04x} produced {} bytes of WRONG output",
                    bytes.len()
                ),
                // `Truncated` is the bounded reader's own refusal: a corrupted block size
                // can end the frame early and leave fewer than four bytes where the next
                // frame's magic would be.
                Err(CodecError::Malformed(_) | CodecError::Truncated { .. }) => {}
                // Flipping the frame descriptor can turn the next byte into a window
                // descriptor, and a window above the per-expansion allowance is refused as
                // unsupported rather than as malformed input.
                Err(CodecError::NotImplemented(message))
                    if message.starts_with("Zstandard window") => {}
                Err(other) => panic!("byte {index} ^ {delta:#04x} gave {other:?}"),
            }
        }
    }
    // The floor is the byte SET, not the total.  A total-only floor is satisfied by a sweep
    // that skips a hundred contiguous bytes -- measured: giving bytes 100..200 no mutations
    // at all drops the count 9 168 -> 8 368, over any threshold loose enough not to be
    // tuned, and the docstring's "no byte is unswept" was held by nothing.
    assert_eq!(
        swept_bytes.len(),
        len,
        "the sweep reached {} of {len} bytes",
        swept_bytes.len()
    );
    assert!(swept >= 9_000, "the sweep collapsed to {swept} mutations");

    // The measured set is a CEILING: a stricter decoder accepts fewer and stays green.
    let ceiling: std::collections::BTreeSet<usize> =
        ZSTD_GOLDEN_ACCEPTING_BYTES.iter().copied().collect();
    let surprises: Vec<_> = accepting.difference(&ceiling).copied().collect();
    assert!(
        surprises.is_empty(),
        "these bytes now accept a corruption and did not before: {surprises:?}"
    );

    // The row this crate's own checksum comparison holds.  Its region is always exhaustive.
    assert!(
        checksum_bytes_accepted.is_empty(),
        "the content-checksum trailer accepted {checksum_bytes_accepted:?}"
    );

    // Truncation, which no single-byte mutation reaches.
    for keep in [0_usize, 3, 4, 8, 200, 404] {
        assert!(
            open_one_zstd_entry(&zip_fixture::ZSTD_GOLDEN_FRAME[..keep], &plain).is_err(),
            "a frame truncated to {keep} bytes was accepted"
        );
    }

    // Trailing bytes after a complete frame.  `ruzstd` stops at the end of the first frame
    // and ignores what follows, so this is held by the decoder's own framing of the whole
    // entry: bytes that are not a frame are refused before anything is decoded.  A second
    // complete frame IS a frame, is decoded (every frame of an entry is decoded, as libzstd does), and
    // is refused here by `ExpandSpec::Exact`, because the entry declares one plaintext.
    for tail in [
        b"".as_slice(),
        b"\x00",
        b"junk",
        zip_fixture::ZSTD_GOLDEN_FRAME,
    ] {
        let mut frame = zip_fixture::ZSTD_GOLDEN_FRAME.to_vec();
        frame.extend_from_slice(tail);
        let result = open_one_zstd_entry(&frame, &plain);
        if tail.is_empty() {
            assert_eq!(
                result.expect("the untailed frame opens"),
                plain,
                "control: the frame with an EMPTY tail must still decode"
            );
        } else {
            assert!(
                result.is_err(),
                "a frame with {} trailing bytes was accepted",
                tail.len()
            );
        }
    }

    // The control, last: the unmutated frame through the same code is still accepted.
    // Without it a decoder that refused everything would pass every assertion above.
    assert_eq!(
        open_one_zstd_entry(zip_fixture::ZSTD_GOLDEN_FRAME, &plain)
            .expect("the unmutated frame opens"),
        plain
    );
}

/// Re-parses a ZIP the way a streaming reader would and requires it to agree with itself.
///
/// Independent of `zip` on purpose: the point is to hold the fields `zip::ZipArchive` does
/// not read on the snapshot's path (every local header) or does not validate (the EOCD's
/// central-directory size and total entry count).
fn assert_local_headers_agree(bytes: &[u8]) {
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at =
        |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let len16 = |i: usize| usize::from(u16_at(i));
    let len32 = |i: usize| usize::try_from(u32_at(i)).expect("offset fits memory");
    let eocd = (0..bytes.len().saturating_sub(21))
        .rev()
        .find(|&i| &bytes[i..i + 4] == b"PK\x05\x06")
        .expect("end-of-central-directory record");
    let count = len16(eocd + 10);
    assert_eq!(len16(eocd + 8), count, "disk vs total entry count");
    let central_size = len32(eocd + 12);
    let central_start = len32(eocd + 16);
    assert_eq!(
        central_start + central_size,
        eocd,
        "the central directory does not end where the EOCD begins"
    );

    let mut at = central_start;
    for index in 0..count {
        assert_eq!(&bytes[at..at + 4], b"PK\x01\x02", "central header {index}");
        let (flags, method) = (u16_at(at + 8), u16_at(at + 10));
        let (crc, csize, usize_) = (u32_at(at + 16), u32_at(at + 20), u32_at(at + 24));
        let (nlen, elen, clen) = (len16(at + 28), len16(at + 30), len16(at + 32));
        let name = &bytes[at + 46..at + 46 + nlen];
        let local = len32(at + 42);

        assert_eq!(
            &bytes[local..local + 4],
            b"PK\x03\x04",
            "local header {index}"
        );
        assert_eq!(u16_at(local + 6), flags, "{index}: flags disagree");
        assert_eq!(u16_at(local + 8), method, "{index}: method disagrees");
        assert_eq!(u32_at(local + 14), crc, "{index}: CRC-32 disagrees");
        assert_eq!(
            u32_at(local + 18),
            csize,
            "{index}: compressed size disagrees"
        );
        assert_eq!(
            u32_at(local + 22),
            usize_,
            "{index}: uncompressed size disagrees"
        );
        let lnlen = len16(local + 26);
        assert_eq!(
            &bytes[local + 30..local + 30 + lnlen],
            name,
            "{index}: name disagrees"
        );
        at += 46 + nlen + elen + clen;
    }
    assert_eq!(
        at, eocd,
        "the central directory has {count} entries but does not end at EOCD"
    );
}

/// Wraps `frame` as the sole Zstandard entry of a ZIP declaring `plain`, and opens it.
fn open_one_zstd_entry(frame: &[u8], plain: &[u8]) -> Result<Vec<u8>, CodecError> {
    open_one_zstd_entry_with(frame, plain, &DecodePolicy::default())
}

fn open_one_zstd_entry_with(
    frame: &[u8],
    plain: &[u8],
    policy: &DecodePolicy,
) -> Result<Vec<u8>, CodecError> {
    let bytes = zip_fixture::archive(&[RawEntry::compressed(
        "payload.bin",
        zip_fixture::ZSTD,
        frame,
        plain,
    )]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, policy).expect("archive fits root policy");
    let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
    let entry = snapshot.entry("payload.bin").expect("Zstandard record");
    snapshot
        .open(&ctx, &entry.name)
        .map(|view| view.window().to_vec())
}

/// An entry of several Zstandard frames decodes to their concatenation, and skippable
/// frames contribute nothing.
///
/// This is libzstd's behaviour (its streaming decoder continues into the next
/// frame), kept with `ruzstd`, whose `FrameDecoder` stops at the end of the first frame.
/// The Zstandard CLI v1.5.5 decodes the same layouts.
#[test]
fn a_multi_frame_zstandard_entry_decodes() {
    let plain = zip_fixture::zstd_golden_plaintext();
    let skippable = [&[0x50, 0x2a, 0x4d, 0x18, 5, 0, 0, 0][..], b"notes"].concat();
    let frames = [
        skippable.as_slice(),
        zip_fixture::ZSTD_GOLDEN_FRAME,
        skippable.as_slice(),
        zip_fixture::ZSTD_GOLDEN_NOCHECK_FRAME,
        &zip_fixture::zstd_stored_frame(b"tail"),
        skippable.as_slice(),
    ]
    .concat();
    let expected = [plain.as_slice(), plain.as_slice(), b"tail"].concat();
    assert_eq!(
        open_one_zstd_entry(&frames, &expected).expect("every frame decodes"),
        expected
    );

    // The second frame's own checksum still counts: one frame's verdict does not cover
    // another's.
    let mut corrupt = [
        zip_fixture::ZSTD_GOLDEN_NOCHECK_FRAME,
        zip_fixture::ZSTD_GOLDEN_FRAME,
    ]
    .concat();
    let last = corrupt.len() - 1;
    corrupt[last] ^= 1;
    let doubled = [plain.as_slice(), plain.as_slice()].concat();
    assert!(matches!(
        open_one_zstd_entry(&corrupt, &doubled),
        Err(CodecError::Malformed(message)) if message.contains("checksum")
    ));
    corrupt[last] ^= 1;
    assert_eq!(
        open_one_zstd_entry(&corrupt, &doubled).expect("control"),
        doubled
    );
}

/// A payload whose declared uncompressed size disagrees with the frame is refused.
///
/// `ExpandSpec::Exact` is the guard that does not depend on the decoder at all: whatever a
/// Zstandard decoder makes of the entry, the bytes it yields must be exactly as many as the
/// central directory declares.  The two directions are separate failures and both are
/// asserted.
#[test]
fn a_zstandard_entry_whose_declared_size_disagrees_is_refused() {
    let plain = zip_fixture::zstd_golden_plaintext();
    for declared in [plain.len() - 1, plain.len() + 1] {
        let mut entry = RawEntry::compressed(
            "payload.bin",
            zip_fixture::ZSTD,
            zip_fixture::ZSTD_GOLDEN_FRAME,
            &plain,
        );
        entry.uncompressed_size = u32::try_from(declared).expect("fits u32");
        let bytes = zip_fixture::archive(&[entry]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
        let entry = snapshot.entry("payload.bin").expect("Zstandard record");
        assert!(
            snapshot.open(&ctx, &entry.name).is_err(),
            "a declared size of {declared} was accepted against a 19 968-byte frame"
        );
    }
}

/// A compression method this crate does not implement is still named, not mistaken for one.
///
/// The snapshot reads the method number from the central record itself and carries it as
/// `ZipCompression::Unsupported(n)`: the archive is admitted, the member is summarized
/// with its number, and `open` refuses it as `NotImplemented`, while the other members
/// stay readable.
///
/// Method 20 is NOT in this list any more. It is the Zstandard method ID that APPNOTE
/// 6.3.7 deprecated in favour of 93, and this crate decodes both; the second half holds
/// that with the reference-written frame.
#[test]
fn an_unimplemented_compression_method_is_refused_by_number() {
    for method in [1_u16, 12, 14, 94, 95, 98] {
        // Distinct compressed and uncompressed sizes: with `b"whatever"` for both, this
        // fixture was the one archive test blind to a builder that transposes them.
        let bytes = zip_fixture::archive(&[
            RawEntry::compressed(
                "payload.bin",
                method,
                b"compressed",
                b"the uncompressed form, which is longer",
            ),
            RawEntry::stored("plain.bin", b"plain"),
        ]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("the archive is admitted");
        assert!(snapshot.recovery_reason().is_none());
        assert_eq!(
            snapshot.entry("payload.bin").expect("record").compression,
            ZipCompression::Unsupported(method)
        );
        let opened = snapshot.open(&ctx, "payload.bin");
        assert!(
            matches!(&opened, Err(CodecError::NotImplemented(message))
                if message.contains(&format!("ZIP compression {method} "))),
            "ZIP compression method {method} was not refused as unimplemented: {opened:?}"
        );
        assert_eq!(
            snapshot.open(&ctx, "plain.bin").expect("sibling").window(),
            b"plain"
        );
    }

    let plain = zip_fixture::zstd_golden_plaintext();
    let bytes = zip_fixture::archive(&[RawEntry::compressed(
        "payload.bin",
        20,
        zip_fixture::ZSTD_GOLDEN_FRAME,
        &plain,
    )]);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("archive fits root policy");
    let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
    assert_eq!(
        snapshot.entry("payload.bin").expect("record").compression,
        ZipCompression::Zstd
    );
    assert_eq!(
        snapshot.open(&ctx, "payload.bin").expect("opens").window(),
        plain.as_slice()
    );
}
