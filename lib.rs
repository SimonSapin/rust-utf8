#[macro_use] extern crate matches;

use std::borrow::Cow;
use std::ops::Deref;
use std::str;

pub const REPLACEMENT_CHARACTER: &'static str = "\u{FFFD}";

pub fn lossy_to_string(input: &[u8]) -> Cow<str> {
    // The first step is special: we want to return Cow::Borrowed if possible.
    let (mut string, mut remaining) = match step(input) {
        StepResult::Valid(s) => return s.into(),
        StepResult::Incomplete(s, _) => {
            let mut string = s.to_owned();
            string.push_str(REPLACEMENT_CHARACTER);
            return string.into()
        }
        StepResult::Error(s, remaining) => (s.to_owned(), remaining),
    };
    loop {
        string.push_str(REPLACEMENT_CHARACTER);
        match step(remaining) {
            StepResult::Valid(s) => {
                string.push_str(s);
                break
            }
            StepResult::Incomplete(s, _) => {
                string.push_str(s);
                string.push_str(REPLACEMENT_CHARACTER);
                break
            }
            StepResult::Error(s, r) => {
                string.push_str(s);
                string.push_str(REPLACEMENT_CHARACTER);
                remaining = r
            }
        }
    }
    string.into()
}

pub fn step(input: &[u8]) -> StepResult {
    let mut iter = input.iter();
    loop {
        let first = match iter.next() {
            Some(&b) => b,
            // we're at the end of the iterator and a codepoint
            // boundary at the same time, so this string is valid.
            None => return StepResult::Valid(unsafe {
                str::from_utf8_unchecked(input)
            })
        };
        // ASCII characters are always valid, so only large
        // bytes need more examination.
        if first >= 128 {
            macro_rules! valid_prefix {
                ($incomplete_sequence_len: expr) => {
                    {
                        let consumed = iter.as_slice().as_ptr() as usize - input.as_ptr() as usize;
                        let valid = consumed - $incomplete_sequence_len as usize;
                        unsafe {
                            str::from_utf8_unchecked(&input[..valid])
                        }
                    }
                }
            }

            macro_rules! next {
                ($first: expr, $second: expr, $third: expr, $incomplete_sequence_len: expr) => {
                    match iter.next() {
                        Some(&b) => b,
                        None => {
                            return StepResult::Incomplete(
                                valid_prefix!($incomplete_sequence_len),
                                IncompleteSequence {
                                    bytes: [$first, $second, $third, $incomplete_sequence_len]
                                }
                            )
                        }
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
            let second = next!(first, 0, 0, 1);
            let valid = match width {
                2 => is_continuation_byte(second),
                3 => valid_three_bytes_sequence_prefix(first, second),
                _ => {
                    debug_assert!(width == 4);
                    valid_four_bytes_sequence_prefix(first, second)
                }
            };
            if !valid {
                return StepResult::Error(valid_prefix!(2), iter.as_slice())
            }
            if width == 2 {
                continue
            }
            let third = next!(first, second, 0, 2);
            if !is_continuation_byte(third) {
                return StepResult::Error(valid_prefix!(3), iter.as_slice())
            }
            if width == 3 {
                continue
            }
            let fourth = next!(first, second, third, 3);
            if !is_continuation_byte(fourth) {
                return StepResult::Error(valid_prefix!(4), iter.as_slice())
            }
        }
    }
}

pub enum StepResult<'a> {
    Valid(&'a str),
    Error(&'a str, &'a [u8]),
    Incomplete(&'a str, IncompleteSequence),
}

pub struct IncompleteSequence {
    /// Use the 4th byte as a length field,
    /// but [u8; 4] makes code easier than [u8; 3] and u8.
    bytes: [u8; 4],
}

impl IncompleteSequence {
    pub fn complete(mut self, input: &[u8]) -> CompleteResult {
        let width = width(self.bytes[0]);
        let len = self.bytes[3] as usize;
        debug_assert!(0 < len && len < width && width <= 4);
        let missing = width - len;
        for i in 0..missing {
            match input.get(i) {
                Some(&byte) => {
                    let valid = if len + i == 1 {
                        match width {
                            2 => is_continuation_byte(byte),
                            3 => valid_three_bytes_sequence_prefix(self.bytes[0], byte),
                            _ => {
                                debug_assert!(width == 4);
                                valid_four_bytes_sequence_prefix(self.bytes[0], byte)
                            }
                        }
                    } else {
                        is_continuation_byte(byte)
                    };
                    if valid {
                        // If len + i == 3 this overrites our “len field”, but that’s OK
                        // as we’re necessarily about to return CompleteResult::Ok.
                        self.bytes[len + i] = byte
                    } else {
                        return CompleteResult::Error(&input[i + 1..])
                    }
                }
                None => {
                    let new_len = len + i;
                    debug_assert!(new_len < 4);
                    self.bytes[3] = new_len as u8;
                    return CompleteResult::StillIncomplete(self)
                }
            }
        }
        CompleteResult::Ok(StrChar { bytes: self.bytes }, &input[missing..])
    }
}

pub enum CompleteResult<'a> {
    Ok(StrChar, &'a [u8]),
    Error(&'a [u8]),
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
        let width = width(self.bytes[0]);
        let bytes = &self.bytes[..width];
        unsafe {
            str::from_utf8_unchecked(bytes)
        }
    }
}

#[inline]
fn width(first_byte: u8) -> usize {
    UTF8_CHAR_WIDTH[first_byte as usize] as usize
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
