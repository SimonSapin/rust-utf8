extern crate utf8;

use std::borrow::Cow;
use utf8::{Decoder, DecodedPiece};

#[path = "shared/data.rs"]
mod data;

/// A re-implementation of String::from_utf8_lossy
pub fn string_from_utf8_lossy(input: &[u8]) -> Cow<str> {
    let mut decoder = Decoder::new();
    let mut iter = decoder.feed(input);
    // The first piece is special: we want to return Cow::Borrowed if possible.
    let first = iter.next();
    let second = iter.next();
    if let (&Some(DecodedPiece::InputSlice(s)), &None) = (&first, &second) {
        return (*s).into()
    }
    let mut string = String::new();
    if let Some(ref first) = first {
        string.push_str(first)
    }
    if let Some(ref second) = second {
        string.push_str(second)
    }
    for piece in iter {
        string.push_str(&piece)
    }
    string.into()
}

#[test]
fn test_string_from_utf8_lossy() {
    for &(input, expected) in data::DECODED_LOSSY {
        assert_eq!(string_from_utf8_lossy(input), expected);
    }
}
