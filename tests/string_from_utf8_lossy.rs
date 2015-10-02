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
    match iter.next() {
        Some(DecodedPiece::InputSlice(ref s)) if iter.eof() => (*s).into(),
        None => "".into(),
        Some(first) => {
            let mut string = first.to_owned();
            for piece in iter {
                string.push_str(&piece)
            }
            string.into()
        }
    }
}

#[test]
fn test_string_from_utf8_lossy() {
    for &(input, expected) in data::DECODED_LOSSY {
        assert_eq!(string_from_utf8_lossy(input), expected);
    }
}
