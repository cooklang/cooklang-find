//! Reading recipe files whose bytes are not valid UTF-8.
//!
//! Cooklang files are meant to be UTF-8, but collections accumulate files that
//! are not: a recipe saved from a Latin-1 editor, a stray byte from a bad
//! copy-paste, a file converted to UTF-16 by a spreadsheet export. Those files
//! are still recipes, and the library's job is to find them.
//!
//! So text is decoded the way [`String::from_utf8_lossy`] does, substituting
//! U+FFFD for each bad byte, rather than refused. A mis-encoded accent costs
//! the user one wrong character in a title; refusing the file costs them the
//! recipe, and — where a caller propagates the failure — every other recipe in
//! the same walk (<https://github.com/cooklang/cookcli/issues/498>).
//!
//! Real I/O failures are a different thing and still surface as errors: an
//! unreadable file is not an empty one, and hiding a permissions problem or a
//! disappearing network mount would turn a fixable error into silently missing
//! recipes.

use std::io::{self, BufRead};

/// Read `reader` to a string, replacing invalid UTF-8 rather than failing.
pub(crate) fn read_to_string_lossy(reader: &mut impl BufRead) -> io::Result<String> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    Ok(decode(bytes))
}

/// The lines of `reader`, decoded lossily, without their terminators.
///
/// Lazy like [`BufRead::lines`], so a caller that only wants front matter stops
/// reading where it stops caring. Both `\n` and `\r\n` end a line; a final line
/// without a terminator is still yielded.
pub(crate) fn lines_lossy(mut reader: impl BufRead) -> impl Iterator<Item = io::Result<String>> {
    std::iter::from_fn(move || {
        let mut bytes = Vec::new();
        match reader.read_until(b'\n', &mut bytes) {
            Ok(0) => None,
            Ok(_) => {
                if bytes.last() == Some(&b'\n') {
                    bytes.pop();
                    if bytes.last() == Some(&b'\r') {
                        bytes.pop();
                    }
                }
                Some(Ok(decode(bytes)))
            }
            Err(e) => Some(Err(e)),
        }
    })
}

/// Decode bytes as UTF-8, substituting U+FFFD for anything invalid.
///
/// Takes ownership so that the overwhelmingly common case — bytes that *are*
/// valid UTF-8 — reuses the allocation instead of copying it.
fn decode(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `caf\xe9`: Latin-1, the shape of the files that provoked this module.
    const LATIN1: &[u8] = b"caf\xe9";

    #[test]
    fn valid_utf8_is_unchanged() {
        assert_eq!(decode("café ☕".as_bytes().to_vec()), "café ☕");
    }

    #[test]
    fn invalid_bytes_become_replacement_characters() {
        assert_eq!(decode(LATIN1.to_vec()), "caf\u{fffd}");
    }

    #[test]
    fn read_to_string_lossy_keeps_the_readable_text() {
        let mut bytes: &[u8] = b"---\ntitle: caf\xe9\n---\n\nBrew it.\n";
        assert_eq!(
            read_to_string_lossy(&mut bytes).unwrap(),
            "---\ntitle: caf\u{fffd}\n---\n\nBrew it.\n"
        );
    }

    #[test]
    fn lines_lossy_splits_on_both_line_endings_and_keeps_bad_bytes_out_of_the_way() {
        let bytes: &[u8] = b"one\r\ncaf\xe9\nlast";
        let lines: Vec<String> = lines_lossy(bytes).map(Result::unwrap).collect();
        assert_eq!(lines, ["one", "caf\u{fffd}", "last"]);
    }

    /// A bad line must not end the iteration: the lines after it are still
    /// recipe text, and front matter often sits behind one.
    #[test]
    fn lines_lossy_keeps_going_past_a_bad_line() {
        let bytes: &[u8] = b"\xff\xfe\ntitle: ok\n";
        let lines: Vec<String> = lines_lossy(bytes).map(Result::unwrap).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1], "title: ok");
    }

    #[test]
    fn an_empty_reader_yields_no_lines() {
        let bytes: &[u8] = b"";
        assert_eq!(lines_lossy(bytes).count(), 0);
    }

    /// Lazy, so that reading front matter does not read the whole file.
    #[test]
    fn lines_lossy_reads_no_further_than_it_is_asked_to() {
        let bytes: &[u8] = b"first\nsecond\nthird\n";
        let mut remaining = bytes;
        let first = lines_lossy(&mut remaining).next().unwrap().unwrap();
        assert_eq!(first, "first");
        assert_eq!(remaining, b"second\nthird\n");
    }

    /// A genuine I/O failure is not an encoding problem and must still be one.
    #[test]
    fn io_errors_are_not_swallowed() {
        struct Broken;
        impl io::Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
            }
        }
        impl BufRead for Broken {
            fn fill_buf(&mut self) -> io::Result<&[u8]> {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
            }
            fn consume(&mut self, _: usize) {}
        }

        let error = lines_lossy(Broken).next().unwrap().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(
            read_to_string_lossy(&mut Broken).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
