#[macro_use] extern crate matches;
extern crate string_wrapper;

use std::str;
use string_wrapper::StringWrapper;

/// The replacement character. In lossy decoding, insert it for every decoding error.
pub const REPLACEMENT_CHARACTER: &'static str = "\u{FFFD}";

pub struct PushLossyDecoder<F: FnMut(&str)> {
    push_str: F,
    incomplete_sequence: Option<IncompleteSequence>,
}

impl<F: FnMut(&str)> PushLossyDecoder<F> {
    #[inline]
    pub fn new(push_str: F) -> Self {
        PushLossyDecoder {
            push_str: push_str,
            incomplete_sequence: None,
        }
    }

    pub fn feed(&mut self, input: &[u8]) {
        let mut result = if let Some(seq) = self.incomplete_sequence.take() {
            let (ch, s, status) = seq.complete(input);
            (self.push_str)(&ch);
            (s, status)
        } else {
            decode_step(input)
        };
        loop {
            let (s, status) = result;
            (self.push_str)(s);
            match status {
                DecodeStepStatus::Ok => break,
                DecodeStepStatus::Incomplete(incomplete_sequence) => {
                    self.incomplete_sequence = Some(incomplete_sequence);
                    break
                }
                DecodeStepStatus::Error { remaining_input_after_error } => {
                    (self.push_str)(REPLACEMENT_CHARACTER);
                    result = decode_step(remaining_input_after_error);
                }
            }
        }
    }

    #[inline]
    pub fn end(self) {
        // drop
    }
}

impl<F: FnMut(&str)> Drop for PushLossyDecoder<F> {
    #[inline]
    fn drop(&mut self) {
        if self.incomplete_sequence.is_some() {
            (self.push_str)(REPLACEMENT_CHARACTER)
        }
    }
}

/// Low-level UTF-8 decoding.
///
/// Return the (possibly empty) str slice for the prefix of `input` that is well-formed UTF-8,
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
                () => {
                    unsafe {
                        str::from_utf8_unchecked(&input[..position])
                    }
                }
            }

            macro_rules! next {
                ($current_sequence_len: expr, $first: expr, $second: expr, $third: expr) => {
                    match input.get(position + $current_sequence_len) {
                        Some(&b) => b,
                        None => return (
                            valid_prefix!(),
                            DecodeStepStatus::Incomplete(IncompleteSequence {
                                len: $current_sequence_len,
                                first: $first,
                                second: $second,
                                third: $third,
                            }),
                        )
                    }
                }
            }

            macro_rules! check {
                ($valid: expr, $current_sequence_len: expr) => {
                    if !$valid {
                        return (
                            valid_prefix!(),
                            DecodeStepStatus::Error {
                                remaining_input_after_error:
                                    &input[position + $current_sequence_len..]
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
            check!(width != 0, 1);
            let second = next!(1, first, 0, 0);
            let valid = match width {
                2 => is_continuation_byte(second),
                3 => valid_three_bytes_sequence_prefix(first, second),
                _ => {
                    debug_assert!(width == 4);
                    valid_four_bytes_sequence_prefix(first, second)
                }
            };
            check!(valid, 1);
            if width > 2 {
                let third = next!(2, first, second, 0);
                check!(is_continuation_byte(third), 2);
                if width > 3 {
                    let fourth = next!(3, first, second, third);
                    check!(is_continuation_byte(fourth), 3);
                }
            }
            position += width as usize;
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum DecodeStepStatus<'a> {
    /// The input is entirely well-formed
    Ok,

    /// There is a decoding error.
    /// Each such error should be represented as one U+FFFD replacement character in lossy decoding.
    Error { remaining_input_after_error: &'a [u8] },

    /// The end of the input was reached in the middle of an UTF-8 sequence that is valid so far.
    /// More input (up to 3 more bytes) is required to determine if it is well-formed.
    /// If at the end of the input, this is a decoding error.
    Incomplete(IncompleteSequence),
}

#[derive(Debug, Copy, Clone)]
pub struct IncompleteSequence {
    len: u8,
    first: u8,
    second: u8,
    third: u8,
}

impl IncompleteSequence {
    /// Try to complete an incomplete sequence.
    pub fn complete(mut self, input: &[u8]) -> (StringWrapper<[u8; 4]>, &str, DecodeStepStatus) {
        let width = width(self.first);
        debug_assert!(0 < self.len && self.len < width && width <= 4);

        let mut position = 0;
        macro_rules! next {
            () => {
                match input.get(position) {
                    Some(&b) => b,
                    None => {
                        let new_len = self.len + position as u8;
                        debug_assert!(new_len < 4);
                        self.len = new_len;
                        return (
                            StringWrapper::new([0, 0, 0, 0]),
                            "",
                            DecodeStepStatus::Incomplete(self),
                        )
                    }
                }
            }
        }

        macro_rules! check {
            ($valid: expr) => {
                if !$valid {
                    return (
                        StringWrapper::new([0, 0, 0, 0]),
                        "",
                        DecodeStepStatus::Error { remaining_input_after_error: &input[position..] },
                    )
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
            position += 1;
        }

        let mut fourth = 0;
        if width > 2 {
            if self.len < 3 {
                self.third = next!();
                check!(is_continuation_byte(self.third));
                position += 1;
            }
            if width > 3 {
                fourth = next!();
                check!(is_continuation_byte(fourth));
                position += 1;
            }
        }

        let ch = unsafe {
            StringWrapper::from_raw_parts(
                [self.first, self.second, self.third, fourth],
                width as usize,
            )
        };
        let (decoded, status) = decode_step(&input[position..]);
        (ch, decoded, status)
    }
}

#[inline]
fn width(first_byte: u8) -> u8 {
    UTF8_CHAR_WIDTH[first_byte as usize]
}

// https://tools.ietf.org/html/rfc3629
const UTF8_CHAR_WIDTH: &'static [u8; 256] = &[
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
    const CONTINUATION_MASK: u8 = 0b1100_0000;
    const CONTINUATION_TAG: u8 = 0b1000_0000;
    b & CONTINUATION_MASK == CONTINUATION_TAG
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
