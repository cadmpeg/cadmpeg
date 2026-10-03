// SPDX-License-Identifier: Apache-2.0
//! Charged borrowed text queries and ASCII case conversion.
use super::{u64_from_index, DecodeContext, ScopedReservation};

/// One live admission for a standard Unicode case conversion of these bytes.
struct UnicodeCaseAdmission<'ctx, 'text> {
    text: &'text str,
    _workspace: ScopedReservation<'ctx>,
}
use crate::CodecError;

impl DecodeContext<'_> {
    /// Admits contextual scans, geometric text growth and temporary relocation storage.
    fn unicode_case_admission<'ctx, 'text>(
        &'ctx self,
        text: &'text str,
        operation: &'static str,
    ) -> Result<UnicodeCaseAdmission<'ctx, 'text>, CodecError> {
        let n = u64_from_index(text.len());
        // Each scalar maps to at most three four-byte scalars. Geometric growth
        // has capacity below twice that output, with an eight-byte minimum.
        let retained = self.cost_sum(self.cost_product(n, 24, operation)?, 8, operation)?;
        // During growth, old and new allocations can coexist.
        let temporary = self.cost_sum(self.cost_product(n, 36, operation)?, 16, operation)?;
        // Each contextual sigma can scan both surrounding input halves.
        let scans = self.cost_product(n, self.cost_sum(n, 1, operation)?, operation)?;
        let conversion = self.cost_sum(self.cost_product(n, 48, operation)?, 16, operation)?;
        self.charge_work(self.cost_sum(scans, conversion, operation)?, operation)?;
        self.charge_retained(retained, operation)?;
        let workspace = self.reserve_scoped(temporary, operation)?;
        Ok(UnicodeCaseAdmission {
            text,
            _workspace: workspace,
        })
    }

    /// Converts Unicode case after admitting input scans and bounded result storage.
    /// Standard lowercase preserves context-dependent Greek final sigma.
    pub fn to_lowercase(&self, text: &str, operation: &'static str) -> Result<String, CodecError> {
        let admission = self.unicode_case_admission(text, operation)?;
        Ok(admission.text.to_lowercase())
    }

    /// Converts Unicode case after admitting input scans and bounded result storage.
    pub fn to_uppercase(&self, text: &str, operation: &'static str) -> Result<String, CodecError> {
        let admission = self.unicode_case_admission(text, operation)?;
        Ok(admission.text.to_uppercase())
    }

    /// Replaces non-overlapping substrings through admitted searches and appends.
    pub fn replace_text(
        &self,
        text: &str,
        pattern: &str,
        replacement: &str,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        let mut output = String::new();
        if pattern.is_empty() {
            self.append_retained(&mut output, replacement, operation)?;
            for character in self.admit_iter(text, operation)? {
                let mut buffer = [0; 4];
                self.append_retained(&mut output, character.encode_utf8(&mut buffer), operation)?;
                self.append_retained(&mut output, replacement, operation)?;
            }
            return Ok(output);
        }
        let mut rest = text;
        loop {
            self.charge_work(1, operation)?;
            let Some(index) = self.find_text(rest, pattern, operation)? else {
                break;
            };
            self.append_retained(&mut output, &rest[..index], operation)?;
            self.append_retained(&mut output, replacement, operation)?;
            rest = &rest[index + pattern.len()..];
        }
        self.append_retained(&mut output, rest, operation)?;
        Ok(output)
    }

    /// Builds an admitted replacement before changing the original UTF-8 text.
    /// Invalid byte ranges return a domain error and leave the input unchanged.
    pub fn replace_text_range(
        &self,
        text: &mut String,
        range: std::ops::Range<usize>,
        replacement: &str,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if text.get(range.clone()).is_none() {
            return Err(CodecError::malformed(
                "text replacement range is not on UTF-8 boundaries",
            ));
        }
        let mut output = String::new();
        self.append_retained(&mut output, &text[..range.start], operation)?;
        self.append_retained(&mut output, replacement, operation)?;
        self.append_retained(&mut output, &text[range.end..], operation)?;
        *text = output;
        Ok(())
    }

    /// Finds a UTF-8 substring after admitting every candidate comparison.
    pub fn find_text(
        &self,
        text: &str,
        pattern: &str,
        operation: &'static str,
    ) -> Result<Option<usize>, CodecError> {
        let positions = self.cost_sum(u64_from_index(text.len()), 1, operation)?;
        let comparisons = self.cost_sum(u64_from_index(pattern.len()), 1, operation)?;
        self.charge_work(
            self.cost_product(positions, comparisons, operation)?,
            operation,
        )?;
        Ok(text.find(pattern))
    }
    /// Finds the last UTF-8 substring after admitting every candidate comparison.
    pub fn rfind_text(
        &self,
        text: &str,
        pattern: &str,
        operation: &'static str,
    ) -> Result<Option<usize>, CodecError> {
        let positions = self.cost_sum(u64_from_index(text.len()), 1, operation)?;
        let comparisons = self.cost_sum(u64_from_index(pattern.len()), 1, operation)?;
        self.charge_work(
            self.cost_product(positions, comparisons, operation)?,
            operation,
        )?;
        Ok(text.rfind(pattern))
    }
    /// Tests substring membership through the admitted forward search.
    pub fn contains_text(
        &self,
        text: &str,
        pattern: &str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        Ok(self.find_text(text, pattern, operation)?.is_some())
    }
    /// Splits at the first UTF-8 substring without allocating either half.
    pub fn split_once<'text>(
        &self,
        text: &'text str,
        pattern: &str,
        operation: &'static str,
    ) -> Result<Option<(&'text str, &'text str)>, CodecError> {
        Ok(self
            .find_text(text, pattern, operation)?
            .map(|index| (&text[..index], &text[index + pattern.len()..])))
    }
    /// Splits at the last UTF-8 substring without allocating either half.
    pub fn rsplit_once<'text>(
        &self,
        text: &'text str,
        pattern: &str,
        operation: &'static str,
    ) -> Result<Option<(&'text str, &'text str)>, CodecError> {
        Ok(self
            .rfind_text(text, pattern, operation)?
            .map(|index| (&text[..index], &text[index + pattern.len()..])))
    }
    /// Compares a prefix through the one byte-slice equality operation.
    pub fn starts_with(
        &self,
        text: &str,
        prefix: &str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        match text.as_bytes().get(..prefix.len()) {
            Some(bytes) => self.equal_bytes(bytes, prefix.as_bytes(), operation),
            None => Ok(false),
        }
    }
    /// Compares a suffix through the one byte-slice equality operation.
    pub fn ends_with(
        &self,
        text: &str,
        suffix: &str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        match text.len().checked_sub(suffix.len()) {
            Some(index) => {
                self.equal_bytes(&text.as_bytes()[index..], suffix.as_bytes(), operation)
            }
            None => Ok(false),
        }
    }
    /// Removes a matched prefix without allocating the remaining text.
    pub fn strip_prefix<'text>(
        &self,
        text: &'text str,
        prefix: &str,
        operation: &'static str,
    ) -> Result<Option<&'text str>, CodecError> {
        Ok(if self.starts_with(text, prefix, operation)? {
            Some(&text[prefix.len()..])
        } else {
            None
        })
    }
    /// Removes a matched suffix without allocating the remaining text.
    pub fn strip_suffix<'text>(
        &self,
        text: &'text str,
        suffix: &str,
        operation: &'static str,
    ) -> Result<Option<&'text str>, CodecError> {
        Ok(if self.ends_with(text, suffix, operation)? {
            Some(&text[..text.len() - suffix.len()])
        } else {
            None
        })
    }
    /// Removes leading and trailing Unicode whitespace after admitting input bytes.
    pub fn trim_text<'text>(
        &self,
        text: &'text str,
        operation: &'static str,
    ) -> Result<&'text str, CodecError> {
        self.charge_work(u64_from_index(text.len()), operation)?;
        Ok(text.trim())
    }
    /// Removes leading Unicode whitespace after admitting input bytes.
    pub fn trim_start_text<'text>(
        &self,
        text: &'text str,
        operation: &'static str,
    ) -> Result<&'text str, CodecError> {
        self.charge_work(u64_from_index(text.len()), operation)?;
        Ok(text.trim_start())
    }
    /// Removes trailing Unicode whitespace after admitting input bytes.
    pub fn trim_end_text<'text>(
        &self,
        text: &'text str,
        operation: &'static str,
    ) -> Result<&'text str, CodecError> {
        self.charge_work(u64_from_index(text.len()), operation)?;
        Ok(text.trim_end())
    }

    /// Removes leading matching characters after admitting input bytes.
    /// The predicate admits its own child work.
    pub fn trim_start_matches<'text>(
        &self,
        text: &'text str,
        mut matches: impl FnMut(char) -> Result<bool, CodecError>,
        operation: &'static str,
    ) -> Result<&'text str, CodecError> {
        let mut start = 0;
        for character in self.admit_iter(text, operation)? {
            if !matches(character)? {
                break;
            }
            start += character.len_utf8();
        }
        Ok(&text[start..])
    }

    /// Removes trailing matching characters after admitting input bytes.
    /// The predicate admits its own child work.
    pub fn trim_end_matches<'text>(
        &self,
        text: &'text str,
        mut matches: impl FnMut(char) -> Result<bool, CodecError>,
        operation: &'static str,
    ) -> Result<&'text str, CodecError> {
        let mut end = text.len();
        for character in self.admit_iter(text, operation)?.rev() {
            if !matches(character)? {
                break;
            }
            end -= character.len_utf8();
        }
        Ok(&text[..end])
    }

    /// Removes matching characters from both ends through the two admitted scans.
    pub fn trim_matches<'text>(
        &self,
        text: &'text str,
        mut matches: impl FnMut(char) -> Result<bool, CodecError>,
        operation: &'static str,
    ) -> Result<&'text str, CodecError> {
        let text = self.trim_start_matches(text, &mut matches, operation)?;
        self.trim_end_matches(text, matches, operation)
    }

    /// Removes trailing ASCII whitespace after admitting input bytes.
    pub fn trim_ascii_end<'text>(
        &self,
        text: &'text str,
        operation: &'static str,
    ) -> Result<&'text str, CodecError> {
        self.charge_work(u64_from_index(text.len()), operation)?;
        Ok(text.trim_ascii_end())
    }

    /// Tests whether every byte is ASCII after admitting the complete scan.
    pub fn is_ascii(&self, bytes: &[u8], operation: &'static str) -> Result<bool, CodecError> {
        self.charge_work(u64_from_index(bytes.len()), operation)?;
        Ok(bytes.is_ascii())
    }
    /// Converts ASCII letters in place after admitting the complete byte scan.
    pub fn make_ascii_lowercase(
        &self,
        text: &mut str,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_work(u64_from_index(text.len()), operation)?;
        text.make_ascii_lowercase();
        Ok(())
    }
    /// Converts ASCII letters in place after admitting the complete byte scan.
    pub fn make_ascii_uppercase(
        &self,
        text: &mut str,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_work(u64_from_index(text.len()), operation)?;
        text.make_ascii_uppercase();
        Ok(())
    }
    /// Copies retained UTF-8 text and converts its ASCII letters to lowercase.
    pub fn to_ascii_lowercase(
        &self,
        text: &str,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        let mut output = self.copy_retained_text(text, operation)?;
        self.make_ascii_lowercase(&mut output, operation)?;
        Ok(output)
    }
    /// Copies retained UTF-8 text and converts its ASCII letters to uppercase.
    pub fn to_ascii_uppercase(
        &self,
        text: &str,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        let mut output = self.copy_retained_text(text, operation)?;
        self.make_ascii_uppercase(&mut output, operation)?;
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use crate::CodecError;
    #[test]
    fn unicode_case_preserves_context_and_multiscalar_expansions() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test operation succeeds");
        for text in [
            "",
            "ABC",
            "İßﬃ",
            "ΟΣ",
            "ΟΣΑ",
            "Ο\u{301}Σ\u{301}",
            "Σ",
            "农历新年",
        ] {
            assert_eq!(
                ctx.to_lowercase(text, "lowercase")
                    .expect("test operation succeeds"),
                text.to_lowercase()
            );
            assert_eq!(
                ctx.to_uppercase(text, "uppercase")
                    .expect("test operation succeeds"),
                text.to_uppercase()
            );
        }
    }

    #[test]
    fn unicode_case_refusal_precedes_conversion_and_preserves_original_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // One byte: n*(n+1) scan bound plus 48*n+16 conversion and relocation bytes.
        policy.limits.max_work_units = 66;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
        assert_eq!(
            ctx.to_lowercase("A", "case")
                .expect("test operation succeeds"),
            "a"
        );
        let CodecError::ResourceLimit(first) =
            ctx.charge_work(1, "probe").expect_err("operation refuses")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first.used, 66);
        let CodecError::ResourceLimit(repeated) = ctx
            .to_uppercase("a", "later")
            .expect_err("operation refuses")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first, repeated);
        policy.limits.max_work_units = 65;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
        assert!(matches!(
            ctx.to_lowercase("A", "case"),
            Err(CodecError::ResourceLimit(_))
        ));
        policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 31;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
        assert!(matches!(
            ctx.to_uppercase("a", "storage"),
            Err(CodecError::ResourceLimit(_))
        ));
    }

    #[test]
    fn charged_text_replacement_preserves_empty_patterns_and_utf8_ranges() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test operation succeeds");
        for (text, pattern, replacement) in [
            ("éλé", "é", "Σ"),
            ("éλ", "", "-"),
            ("", "", "x"),
            ("aaaa", "aa", "b"),
            ("abc", "x", ""),
        ] {
            assert_eq!(
                ctx.replace_text(text, pattern, replacement, "replace")
                    .expect("test operation succeeds"),
                text.replace(pattern, replacement)
            );
        }
        let mut text = String::from("éλé");
        ctx.replace_text_range(&mut text, 2..4, "abc", "range")
            .expect("test operation succeeds");
        assert_eq!(text, "éabcé");
        assert!(ctx
            .replace_text_range(&mut text, 1..2, "x", "invalid")
            .is_err());
        assert_eq!(text, "éabcé");
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
        assert!(matches!(
            ctx.replace_text_range(&mut text, 2..5, "x", "refusal"),
            Err(CodecError::ResourceLimit(_))
        ));
        assert_eq!(text, "éabcé");
    }

    #[test]
    fn borrowed_text_queries_preserve_unicode_boundaries_and_empty_patterns() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        for pattern in ["", "é", "éλ", "x", "λ"] {
            let text = "éλé";
            assert_eq!(
                ctx.find_text(text, pattern, "find").expect("admission"),
                text.find(pattern)
            );
            assert_eq!(
                ctx.rfind_text(text, pattern, "find").expect("admission"),
                text.rfind(pattern)
            );
            assert_eq!(
                ctx.split_once(text, pattern, "split").expect("admission"),
                text.split_once(pattern)
            );
            assert_eq!(
                ctx.rsplit_once(text, pattern, "split").expect("admission"),
                text.rsplit_once(pattern)
            );
            assert_eq!(
                ctx.strip_prefix(text, pattern, "prefix")
                    .expect("admission"),
                text.strip_prefix(pattern)
            );
            assert_eq!(
                ctx.strip_suffix(text, pattern, "suffix")
                    .expect("admission"),
                text.strip_suffix(pattern)
            );
        }
        assert_eq!(
            ctx.trim_text("\u{2003}é\n", "trim").expect("admission"),
            "é"
        );
        assert_eq!(
            ctx.to_ascii_lowercase("ÉAZλ", "case").expect("admission"),
            "Éazλ"
        );
        assert_eq!(
            ctx.to_ascii_uppercase("éazλ", "case").expect("admission"),
            "éAZλ"
        );
    }
    #[test]
    fn text_search_admits_candidate_and_pattern_work_before_search() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Four candidate positions times three pattern steps.
        policy.limits.max_work_units = 12;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert_eq!(ctx.find_text("abc", "bc", "find").expect("search"), Some(1));
        policy.limits.max_work_units = 11;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let CodecError::ResourceLimit(first) =
            ctx.rfind_text("abc", "bc", "rfind").expect_err("refusal")
        else {
            panic!("refusal")
        };
        let CodecError::ResourceLimit(second) = ctx.find_text("", "", "find").expect_err("fused")
        else {
            panic!("refusal")
        };
        assert_eq!(first, second);
    }
    #[test]
    fn ascii_case_refuses_before_mutation_and_keeps_original_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut text = String::from("ABCé");
        let CodecError::ResourceLimit(first) = ctx
            .make_ascii_lowercase(&mut text, "case")
            .expect_err("refusal")
        else {
            panic!("refusal")
        };
        assert_eq!(text, "ABCé");
        let CodecError::ResourceLimit(second) =
            ctx.find_text("a", "a", "search").expect_err("fused")
        else {
            panic!("refusal")
        };
        assert_eq!(first, second);
    }

    #[test]
    fn charged_trim_patterns_preserve_unicode_boundaries_and_scan_totals() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The leading scan admits seven bytes; its five-byte suffix is admitted for the trailing scan.
        policy.limits.max_work_units = 12;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert_eq!(
            ctx.trim_matches("éabcé", |character| Ok(character == 'é'), "trim")
                .expect("admission"),
            "abc"
        );
        let CodecError::ResourceLimit(limit) = ctx.charge_work(1, "probe").expect_err("exact work")
        else {
            panic!("refusal")
        };
        assert_eq!(limit.used, 12);
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        for text in ["", "é", "éé", "éabcé", " abc ", "λ"] {
            assert_eq!(
                ctx.trim_start_matches(text, |character| Ok(character == 'é'), "start")
                    .expect("admission"),
                text.trim_start_matches('é')
            );
            assert_eq!(
                ctx.trim_end_matches(text, |character| Ok(character == 'é'), "end")
                    .expect("admission"),
                text.trim_end_matches('é')
            );
            assert_eq!(
                ctx.trim_matches(text, |character| Ok(character == 'é'), "both")
                    .expect("admission"),
                text.trim_matches('é')
            );
            assert_eq!(
                ctx.trim_ascii_end(text, "ASCII trim").expect("admission"),
                text.trim_ascii_end()
            );
            assert_eq!(
                ctx.is_ascii(text.as_bytes(), "ASCII").expect("admission"),
                text.is_ascii()
            );
        }
    }

    #[test]
    fn charged_trim_patterns_refuse_before_predicate_and_keep_child_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let called = std::cell::Cell::new(false);
        let CodecError::ResourceLimit(first) = ctx
            .trim_start_matches(
                "a",
                |_| {
                    called.set(true);
                    Ok(true)
                },
                "trim",
            )
            .expect_err("refusal")
        else {
            panic!("refusal")
        };
        assert!(!called.get());
        let CodecError::ResourceLimit(second) = ctx.is_ascii(b"a", "ASCII").expect_err("fused")
        else {
            panic!("refusal")
        };
        assert_eq!(first, second);
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let mut calls = 0;
        let CodecError::ResourceLimit(child) = ctx
            .trim_matches(
                "abc",
                |_| {
                    calls += 1;
                    Err(ctx.refuse_codec_limit("child", 0, 1))
                },
                "trim",
            )
            .expect_err("child refusal")
        else {
            panic!("refusal")
        };
        assert_eq!(calls, 1);
        assert_eq!(child.operation, "child");
    }
}
