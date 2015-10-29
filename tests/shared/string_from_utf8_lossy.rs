use std::borrow::Cow;
use utf8::{decode_step, DecodeStepStatus, REPLACEMENT_CHARACTER};

/// A re-implementation of String::from_utf8_lossy
pub fn string_from_utf8_lossy(input: &[u8]) -> Cow<str> {
    let (s, status) = decode_step(input);
    let mut remaining = match status {
        DecodeStepStatus::Ok => return s.into(),
        DecodeStepStatus::Error { remaining_input_after_error: r } => Some(r),
        DecodeStepStatus::Incomplete(_) => None,
    };
    let mut string = String::from(s);
    loop {
        string.push_str(REPLACEMENT_CHARACTER);
        if let Some(r) = remaining {
            let (s, status) = decode_step(r);
            string.push_str(s);
            remaining = match status {
                DecodeStepStatus::Ok => break,
                DecodeStepStatus::Error { remaining_input_after_error: r } => Some(r),
                DecodeStepStatus::Incomplete(_) => None,
            };
        } else {
            break
        }
    }
    string.into()
}
