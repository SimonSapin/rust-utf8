#[macro_use] extern crate matches;

use std::borrow::Cow;
use std::ops::Deref;
use std::str;

pub const REPLACEMENT_CHARACTER: &'static str = "\u{FFFD}";

/// A re-implementation of std::str::from_utf8
pub fn str_from_utf8(input: &[u8]) -> Result<&str, usize> {
    let (s, status) = decode_step(input);
    match status {
        DecodeStepStatus::Ok => Ok(s),
        DecodeStepStatus::Incomplete(_) |
        DecodeStepStatus::Error { .. } => Err(s.len()),
    }
}

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

/// Low-level UTF-8 decoding.
///
/// Return the (possibly empty) str slice for the prefix of `input` that was well-formed UTF-8,
/// and details about the rest of the input.
pub fn decode_step(input: &[u8]) -> (&str, DecodeStepStatus) {
    let mut position = 0;
    loop {
        let first = match input.get(position) {
            Some(&b) => b,
            // we're at the end of the input and a codepoint
            // boundary at the same time, so this string is valid.
            None => return (
                unsafe {
                    str::from_utf8_unchecked(input)
                },
                DecodeStepStatus::Ok,
            )
        };
        // ASCII characters are always valid, so only large
        // bytes need more examination.
        if first < 128 {
            position += 1
        } else {
            macro_rules! valid_prefix {
                ($current_sequence_len: expr) => {
                    unsafe {
                        str::from_utf8_unchecked(&input[..position])
                    }
                }
            }

            macro_rules! next {
                ($current_sequence_len: expr, $first: expr, $second: expr, $third: expr) => {
                    match input.get(position + $current_sequence_len) {
                        Some(&b) => b,
                        None => {
                            return (
                                valid_prefix!($current_sequence_len),
                                DecodeStepStatus::Incomplete(
                                    IncompleteSequence {
                                        len: $current_sequence_len,
                                        first: $first,
                                        second: $second,
                                        third: $third,
                                    }
                                )
                            )
                        }
                    }
                }
            }

            macro_rules! check {
                ($valid: expr, $current_sequence_len: expr) => {
                    if !$valid {
                        return (
                            valid_prefix!($current_sequence_len),
                            DecodeStepStatus::Error {
                                remaining_input_after_error:
                                    &input[..position + $current_sequence_len]
                            }
                        )
                    }
                }
            }

            // 2-byte encoding is for codepoints  \u{0080} to  \u{07ff}
            //        first  C2 80        last DF BF
            // 3-byte encoding is for codepoints  \u{0800} to  \u{ffff}
            //        first  E0 A0 80     last EF BF BF
            //   excluding surrogates codepoints  \u{d800} to  \u{dfff}
            //               ED A0 80 to       ED BF BF
            // 4-byte encoding is for codepoints \u{1000}0 to \u{10ff}ff
            //        first  F0 90 80 80  last F4 8F BF BF
            //
            // Use the UTF-8 syntax from the RFC
            //
            // https://tools.ietf.org/html/rfc3629
            // UTF8-1      = %x00-7F
            // UTF8-2      = %xC2-DF UTF8-tail
            // UTF8-3      = %xE0 %xA0-BF UTF8-tail / %xE1-EC 2( UTF8-tail ) /
            //               %xED %x80-9F UTF8-tail / %xEE-EF 2( UTF8-tail )
            // UTF8-4      = %xF0 %x90-BF 2( UTF8-tail ) / %xF1-F3 3( UTF8-tail ) /
            //               %xF4 %x80-8F 2( UTF8-tail )
            let width = UTF8_CHAR_WIDTH[first as usize];
            let second = next!(1, first, 0, 0);
            let valid = match width {
                2 => is_continuation_byte(second),
                3 => valid_three_bytes_sequence_prefix(first, second),
                _ => {
                    debug_assert!(width == 4);
                    valid_four_bytes_sequence_prefix(first, second)
                }
            };
            check!(valid, 2);
            if width > 2 {
                let third = next!(2, first, second, 0);
                check!(is_continuation_byte(third), 3);
                if width > 3 {
                    let fourth = next!(3, first, second, third);
                    check!(is_continuation_byte(fourth), 4);
                }
            }
            position += width as usize;
        }
    }
}

#[must_use]
pub enum DecodeStepStatus<'a> {
    /// The input was entirely well-formed
    Ok,

    /// There was a decoding error.
    /// Each such error should be represented as one U+FFFD replacement character in lossy decoding.
    Error { remaining_input_after_error: &'a [u8] },

    /// The end of the input was reached in the middle of an UTF-8 sequence that is valid so far.
    /// More input (up to 3 more bytes) is required to determine if it is well-formed.
    /// If no more input is available, this is a decoding error.
    Incomplete(IncompleteSequence),
}

pub struct IncompleteSequence {
    len: u8,
    first: u8,
    second: u8,
    third: u8,
}

impl IncompleteSequence {
    /// Consume more input to attempt to make this incomplete sequence complete.
    pub fn complete(mut self, input: &[u8]) -> CompleteResult {
        let width = width(self.first);
        debug_assert!(0 < self.len && self.len < width && width <= 4);

        let mut i = 0;
        macro_rules! next {
            () => {
                match input.get(i) {
                    Some(&b) => {
                        i += 1;
                        b
                    }
                    None => {
                        let new_len = self.len + i as u8;
                        debug_assert!(new_len < 4);
                        self.len = new_len;
                        return CompleteResult::StillIncomplete(self)
                    }
                }
            }
        }

        macro_rules! check {
            ($valid: expr) => {
                if !$valid {
                    return CompleteResult::Error { remaining_input_after_error: &input[i..] }
                }
            }
        }

        if self.len < 2 {
            self.second = next!();
            let valid = match width {
                2 => is_continuation_byte(self.second),
                3 => valid_three_bytes_sequence_prefix(self.first, self.second),
                _ => {
                    debug_assert!(width == 4);
                    valid_four_bytes_sequence_prefix(self.first, self.second)
                }
            };
            check!(valid);
        }

        let mut fourth = 0;
        if width > 2 {
            if self.len < 3 {
                self.third = next!();
                check!(is_continuation_byte(self.third));
            }
            if width > 3 {
                fourth = next!();
                check!(is_continuation_byte(fourth));
            }
        }

        let ch = StrChar { bytes: [self.first, self.second, self.third, fourth] };
        CompleteResult::Ok { code_point: ch, remaining_input: &input[i..] }
    }
}

pub enum CompleteResult<'a> {
    /// A well-formed code point that was split across input chunks.
    Ok { code_point: StrChar, remaining_input: &'a [u8] },

    /// There was a decoding error.
    /// Each such error should be represented as one U+FFFD replacement character in lossy decoding.
    Error { remaining_input_after_error: &'a [u8] },

    /// There is still not enough input to determine if this is a well-formed code point
    /// or a decoding error.
    /// This can only happen if the `input` argument to `IncompleteSequence::complete`
    /// is less than three bytes.
    StillIncomplete(IncompleteSequence),
}

/// Like `char`, but represented in memory as UTF-8
pub struct StrChar {
    bytes: [u8; 4],
}

impl Deref for StrChar {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        let width = width(self.bytes[0]) as usize;
        let bytes = &self.bytes[..width];
        unsafe {
            str::from_utf8_unchecked(bytes)
        }
    }
}

#[inline]
fn width(first_byte: u8) -> u8 {
    UTF8_CHAR_WIDTH[first_byte as usize]
}

// https://tools.ietf.org/html/rfc3629
static UTF8_CHAR_WIDTH: [u8; 256] = [
    1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
    1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1, // 0x1F
    1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
    1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1, // 0x3F
    1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
    1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1, // 0x5F
    1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
    1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1, // 0x7F
    0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
    0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0, // 0x9F
    0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
    0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0, // 0xBF
    0,0,2,2,2,2,2,2,2,2,2,2,2,2,2,2,
    2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2, // 0xDF
    3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3, // 0xEF
    4,4,4,4,4,0,0,0,0,0,0,0,0,0,0,0, // 0xFF
];

#[inline]
fn is_continuation_byte(b: u8) -> bool {
     b & !CONTINUATION_MASK == CONTINUATION_TAG
}

#[inline]
fn valid_three_bytes_sequence_prefix(first: u8, second: u8) -> bool {
    matches!((first, second),
        (0xE0         , 0xA0 ... 0xBF) |
        (0xE1 ... 0xEC, 0x80 ... 0xBF) |
        (0xED         , 0x80 ... 0x9F) |
        // Exclude surrogates: (0xED, 0xA0 ... 0xBF)
        (0xEE ... 0xEF, 0x80 ... 0xBF)
    )
}

#[inline]
fn valid_four_bytes_sequence_prefix(first: u8, second: u8) -> bool {
    matches!((first, second),
        (0xF0         , 0x90 ... 0xBF) |
        (0xF1 ... 0xF3, 0x80 ... 0xBF) |
        (0xF4         , 0x80 ... 0x8F)
    )
}

/// Mask of the value bits of a continuation byte
const CONTINUATION_MASK: u8 = 0b0011_1111;

/// Value of the tag bits (tag mask is !CONTINUATION_MASK) of a continuation byte
const CONTINUATION_TAG: u8 = 0b1000_0000;
