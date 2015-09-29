extern crate utf8;

use std::borrow::Cow;
use utf8::{decode_step, DecodeStepStatus, REPLACEMENT_CHARACTER};

/// A re-implementation of String::from_utf8_lossy
pub fn string_from_utf8_lossy(input: &[u8]) -> Cow<str> {
    // The first step is special: we want to return Cow::Borrowed if possible.
    let (s, status) = decode_step(input);
    let mut remaining = match status {
        DecodeStepStatus::Ok => return s.into(),
        DecodeStepStatus::Incomplete(_) => None,
        DecodeStepStatus::Error { remaining_input_after_error } => {
            Some(remaining_input_after_error)
        }
    };
    let mut string = s.to_owned();
    loop {
        string.push_str(REPLACEMENT_CHARACTER);
        if let Some(input) = remaining {
            let (s, status) = decode_step(input);
            string.push_str(s);
            remaining = match status {
                DecodeStepStatus::Ok => break,
                DecodeStepStatus::Incomplete(_) => None,
                DecodeStepStatus::Error { remaining_input_after_error } => {
                    Some(remaining_input_after_error)
                }
            };
        } else {
            break
        }
    }
    string.into()
}

#[test]
fn test_string_from_utf8_lossy() {
    assert_eq!(string_from_utf8_lossy(b"hello"), "hello");

    assert_eq!(string_from_utf8_lossy("ศไทย中华Việt Nam".as_bytes()), "ศไทย中华Việt Nam");

    assert_eq!(string_from_utf8_lossy(b"Hello\xC2 There\xFF Goodbye"),
               "Hello\u{FFFD} There\u{FFFD} Goodbye");

    assert_eq!(string_from_utf8_lossy(b"Hello\xC0\x80 There\xE6\x83 Goodbye"),
               "Hello\u{FFFD}\u{FFFD} There\u{FFFD} Goodbye");

    assert_eq!(string_from_utf8_lossy(b"\xF5foo\xF5\x80bar"),
               "\u{FFFD}foo\u{FFFD}\u{FFFD}bar");

    assert_eq!(string_from_utf8_lossy(b"\xF1foo\xF1\x80bar\xF1\x80\x80baz"),
               "\u{FFFD}foo\u{FFFD}bar\u{FFFD}baz");

    assert_eq!(string_from_utf8_lossy(b"\xF4foo\xF4\x80bar\xF4\xBFbaz"),
               "\u{FFFD}foo\u{FFFD}bar\u{FFFD}\u{FFFD}baz");

    assert_eq!(string_from_utf8_lossy(b"\xF0\x80\x80\x80foo\xF0\x90\x80\x80bar"),
               "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}foo\u{10000}bar");

    // surrogates
    assert_eq!(string_from_utf8_lossy(b"\xED\xA0\x80foo\xED\xBF\xBFbar"),
               "\u{FFFD}\u{FFFD}\u{FFFD}foo\u{FFFD}\u{FFFD}\u{FFFD}bar");
}
