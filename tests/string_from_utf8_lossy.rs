extern crate utf8;

use std::borrow::Cow;
use utf8::{decode_step, DecodeStepStatus, REPLACEMENT_CHARACTER};

#[path = "shared/data.rs"]
mod data;

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
    for &(input, expected) in data::DECODED_LOSSY {
        assert_eq!(string_from_utf8_lossy(input), expected);
    }
}
