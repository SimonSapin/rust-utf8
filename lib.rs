#[macro_use] extern crate matches;

use std::ops::Deref;
use std::str;

/// The replacement character. In lossy decoding, insert it for every decoding error.
pub const REPLACEMENT_CHARACTER: &'static str = "\u{FFFD}";

pub struct PushLossyDecoder<F: FnMut(&str)> {
    push_str: F,
    decoder: Decoder,
}

impl<F: FnMut(&str)> PushLossyDecoder<F> {
    #[inline]
    pub fn new(push_str: F) -> Self {
        PushLossyDecoder {
            push_str: push_str,
            decoder: Decoder::new(),
        }
    }

    pub fn feed(&mut self, input: &[u8]) {
        for piece in self.decoder.feed(input) {
            (self.push_str)(&piece);
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
        if let Some(piece) = self.decoder.end() {
            (self.push_str)(&piece)
        }
    }
}


pub struct Decoder {
    incomplete_sequence: IncompleteSequence,
    has_undecoded_input: bool,
}

impl Decoder {
    pub fn new() -> Decoder {
        Decoder {
            has_undecoded_input: false,
            incomplete_sequence: IncompleteSequence {
                len: 0,
                first: 0,
                second: 0,
                third: 0,
            }
        }
    }

    pub fn feed<'d, 'i>(&'d mut self, input_chunk: &'i [u8]) -> ChunkDecoder<'d, 'i> {
        assert!(!self.has_undecoded_input, "The previous `utf8::ChunkDecoder` must be consumed \
                before `utf8::Decoder::feed` can be called again.");
        self.has_undecoded_input = !input_chunk.is_empty();
        ChunkDecoder {
            decoder: self,
            input_chunk: input_chunk,
        }
    }

    pub fn end(&mut self) -> Option<DecodedPiece<'static>> {
        assert!(!self.has_undecoded_input, "The previous `utf8::ChunkDecoder` must be consumed \
                before `utf8::Decoder::end` can be called.");
        if self.incomplete_sequence.len > 0 {
            self.incomplete_sequence.len = 0;
            Some(DecodedPiece::Error)
        } else {
            None
        }
    }
}


pub struct ChunkDecoder<'d, 'i> {
    decoder: &'d mut Decoder,
    input_chunk: &'i [u8],
}

impl<'d, 'i> ChunkDecoder<'d, 'i> {
    pub fn eof(&self) -> bool {
        self.input_chunk.is_empty() && self.decoder.incomplete_sequence.len == 0
    }
}

impl<'d, 'i> Iterator for ChunkDecoder<'d, 'i> {
    type Item = DecodedPiece<'i>;

    fn next(&mut self) -> Option<Self::Item> {
        let result;
        if self.input_chunk.is_empty() {
            result = None
        } else if self.decoder.incomplete_sequence.len > 0 {
            match self.decoder.incomplete_sequence.complete(self.input_chunk) {
                CompleteResult::Ok { code_point, remaining_input } => {
                    result = Some(DecodedPiece::AcrossChunks(code_point));
                    self.input_chunk = remaining_input
                }
                CompleteResult::Error { remaining_input_after_error } => {
                    result = Some(DecodedPiece::Error);
                    self.input_chunk = remaining_input_after_error
                }
                CompleteResult::StillIncomplete => {
                    result = None;
                    self.input_chunk = &[];
                }
            }
        } else {
            let (s, status) = decode_step(self.input_chunk);
            if !s.is_empty() {
                self.input_chunk = &self.input_chunk[s.len()..];
                result = Some(DecodedPiece::InputSlice(s))
            } else {
                match status {
                    DecodeStepStatus::Ok => {
                        self.input_chunk = &[];
                        result = None
                    }
                    DecodeStepStatus::Incomplete(incomplete_sequence) => {
                        self.decoder.incomplete_sequence = incomplete_sequence;
                        self.input_chunk = &[];
                        result = None
                    }
                    DecodeStepStatus::Error { remaining_input_after_error } => {
                        self.input_chunk = remaining_input_after_error;
                        result = Some(DecodedPiece::Error)
                    }
                }
            }
        }
        self.decoder.has_undecoded_input = !self.input_chunk.is_empty();
        result
    }
}

pub enum DecodedPiece<'a> {
    InputSlice(&'a str),
    AcrossChunks(StrChar),
    Error,
}

impl<'a> Deref for DecodedPiece<'a> {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        match *self {
            DecodedPiece::InputSlice(slice) => slice,
            DecodedPiece::AcrossChunks(ref ch) => ch,
            DecodedPiece::Error => REPLACEMENT_CHARACTER,
        }
    }
}

/// Low-level UTF-8 decoding.
///
/// Return the (possibly empty) str slice for the prefix of `input` that was well-formed UTF-8,
/// and details about the rest of the input.
fn decode_step(input: &[u8]) -> (&str, DecodeStepStatus) {
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
                        None => {
                            return (
                                valid_prefix!(),
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

#[must_use]
#[derive(Debug)]
enum DecodeStepStatus<'a> {
    /// The input was entirely well-formed
    Ok,

    /// There was a decoding error.
    /// Each such error should be represented as one U+FFFD replacement character in lossy decoding.
    Error { remaining_input_after_error: &'a [u8] },

    /// The end of the input was reached in the middle of an UTF-8 sequence that is valid so far.
    /// More input (up to 3 more bytes) is required to determine if it is well-formed.
    /// If at the end of the input, this is a decoding error.
    Incomplete(IncompleteSequence),
}

#[derive(Debug)]
struct IncompleteSequence {
    len: u8,
    first: u8,
    second: u8,
    third: u8,
}

impl IncompleteSequence {
    /// Consume more input to attempt to make this incomplete sequence complete.
    pub fn complete<'a>(&mut self, input: &'a [u8]) -> CompleteResult<'a> {
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
                        return CompleteResult::StillIncomplete
                    }
                }
            }
        }

        macro_rules! check {
            ($valid: expr) => {
                if !$valid {
                    self.len = 0;
                    return CompleteResult::Error { remaining_input_after_error: &input[position..] }
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

        self.len = 0;
        let ch = StrChar { bytes: [self.first, self.second, self.third, fourth] };
        CompleteResult::Ok { code_point: ch, remaining_input: &input[position..] }
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
    /// If at the end of the input, this is a decoding error.
    StillIncomplete,
}

/// Like `char`, but represented in memory as UTF-8
#[derive(Copy, Clone)]
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

impl StrChar {
    #[inline]
    pub fn to_char(&self) -> char {
        self.chars().next().unwrap()
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
