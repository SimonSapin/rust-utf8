use std::borrow::Cow;
use utf8::{Decoder, Result, REPLACEMENT_CHARACTER};

/// A re-implementation of String::from_utf8_lossy
pub fn string_from_utf8_lossy(input: &[u8]) -> Cow<str> {
    let mut decoder = Decoder::new();
    let (ch, s, result) = decoder.decode(input);
    debug_assert!(ch.len() == 0);
    let mut remaining = match result {
        Result::Ok => return s.into(),
        Result::Error { remaining_input_after_error: r } => Some(r),
        Result::Incomplete => None,
    };
    let mut string = String::from(s);
    loop {
        string.push_str(REPLACEMENT_CHARACTER);
        if let Some(r) = remaining {
            let (ch, s, result) = decoder.decode(r);
            debug_assert!(ch.len() == 0);
            string.push_str(s);
            remaining = match result {
                Result::Ok => break,
                Result::Error { remaining_input_after_error: r } => Some(r),
                Result::Incomplete => None,
            };
        } else {
            break
        }
    }
    string.into()
}
