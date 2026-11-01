// SPDX-License-Identifier: Apache-2.0
//! The files beside a decoded input that it names by path.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use cadmpeg_core::decode::{Companion, Companions};

/// Opens the files that an input names, relative to the file that names
/// them. A file's identity is its canonical path.
#[derive(Debug)]
pub(crate) struct Directory {
    input: PathBuf,
}

impl Directory {
    pub(crate) fn beside(input: &Path) -> Self {
        Self {
            input: input.canonicalize().unwrap_or_else(|_| input.to_owned()),
        }
    }
}

impl Directory {
    /// The canonical path of the file that `path` names from `anchor`.
    fn resolve(&self, anchor: Option<&str>, path: &str) -> Result<PathBuf, String> {
        let anchor = anchor.map_or(self.input.as_path(), Path::new);
        let named = Path::new(path);
        let joined = if named.is_absolute() {
            named.to_owned()
        } else {
            anchor.parent().unwrap_or(Path::new("")).join(named)
        };
        joined
            .canonicalize()
            .or_else(|error| {
                // A path that names no file as written can be percent-encoded,
                // as Omniverse and HTTP asset paths are.
                percent_decoded(path)
                    .filter(|decoded| decoded != path)
                    .ok_or(error)
                    .and_then(|decoded| {
                        let named = Path::new(&decoded);
                        if named.is_absolute() {
                            named.canonicalize()
                        } else {
                            anchor
                                .parent()
                                .unwrap_or(Path::new(""))
                                .join(named)
                                .canonicalize()
                        }
                    })
            })
            .map_err(|error| format!("{}: {error}", joined.display()))
    }
}

/// `path` with each `%XX` escape replaced by its byte, or `None` when an
/// escape is malformed or the bytes are not UTF-8.
fn percent_decoded(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(byte) = bytes.get(at) {
        if *byte == b'%' {
            let hex = std::str::from_utf8(bytes.get(at + 1..at + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(*byte);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

impl Companions for Directory {
    fn locate(&self, anchor: Option<&str>, path: &str) -> Result<String, String> {
        Ok(self.resolve(anchor, path)?.to_string_lossy().into_owned())
    }

    fn open(&self, anchor: Option<&str>, path: &str, max_bytes: u64) -> Result<Companion, String> {
        let resolved = self.resolve(anchor, path)?;
        let file =
            File::open(&resolved).map_err(|error| format!("{}: {error}", resolved.display()))?;
        let mut bytes = Vec::new();
        file.take(max_bytes.checked_add(1).unwrap_or(max_bytes))
            .read_to_end(&mut bytes)
            .map_err(|error| format!("{}: {error}", resolved.display()))?;
        if cadmpeg_core::decode::u64_from_index(bytes.len()) > max_bytes {
            return Err(format!(
                "{} exceeds the remaining input budget of {max_bytes} bytes",
                resolved.display()
            ));
        }
        Ok(Companion {
            id: resolved.to_string_lossy().into_owned(),
            bytes,
        })
    }
}

#[cfg(test)]
mod tests;
