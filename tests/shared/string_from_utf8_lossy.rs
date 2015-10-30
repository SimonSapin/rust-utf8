use std::borrow::Cow;
use utf8::{decode_step, Result, REPLACEMENT_CHARACTER};

/// A re-implementation of String::from_utf8_lossy
pub fn string_from_utf8_lossy(input: &[u8]) -> Cow<str> {
    let (s, status) = decode_step(input);
    let mut remaining = match status {
        Result::Ok => return s.into(),
        Result::Error { remaining_input_after_error: r } => Some(r),
        Result::Incomplete(_) => None,
    };
    let mut string = String::from(s);
    loop {
        string.push_str(REPLACEMENT_CHARACTER);
        if let Some(r) = remaining {
            let (s, status) = decode_step(r);
            string.push_str(s);
            remaining = match status {
                Result::Ok => break,
                Result::Error { remaining_input_after_error: r } => Some(r),
                Result::Incomplete(_) => None,
            };
        } else {
            break
        }
    }
    string.into()
}
