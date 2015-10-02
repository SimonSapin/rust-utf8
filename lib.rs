#[macro_use] extern crate matches;

use std::ops::Deref;
use std::str;

/// The replacement character. In lossy decoding, insert it for every decoding error.
pub const REPLACEMENT_CHARACTER: &'static str = "\u{FFFD}";

pub struct Decoder {
    incomplete_sequence: IncompleteSequence,
    has_undecoded_input: bool,
}

#[derive(Debug)]
struct IncompleteSequence {
    len: u8,
    first: u8,
    second: u8,
    third: u8,
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
            has_error_next: false,
        }
    }

    pub fn end(self) -> Option<DecodedPiece<'static>> {
        assert!(!self.has_undecoded_input, "The previous `utf8::ChunkDecoder` must be consumed \
                before `utf8::Decoder::end` can be called.");
        if self.incomplete_sequence.len > 0 {
            Some(DecodedPiece::Error)
        } else {
            None
        }
    }
}

pub struct ChunkDecoder<'d, 'i> {
    decoder: &'d mut Decoder,
    input_chunk: &'i [u8],
    has_error_next: bool,
}

impl<'d, 'i> ChunkDecoder<'d, 'i> {
    /// Return whether `next()` would return `None`.
    pub fn eof(&self) -> bool {
        self.input_chunk.is_empty() &&
        !self.has_error_next &&
        self.decoder.incomplete_sequence.len == 0
    }

    #[inline]
    fn consume(&mut self, n_bytes: usize) {
        self.input_chunk = &self.input_chunk[n_bytes..];
        self.decoder.has_undecoded_input = !self.input_chunk.is_empty();
    }

    #[inline]
    fn consume_all(&mut self) {
        self.input_chunk = &[];
        self.decoder.has_undecoded_input = false;
    }

    /// Try to complete an incomplete sequence.
    /// `None`: needs more input for this sequence.
    /// `Some(DecodedPiece::AcrossChunks(_))`: decoded a code point, this sequence is done.
    /// `Some(DecodedPiece::Error)`: decoding error, this sequence is done.
    /// (`Some(DecodedPiece::InputSlice(_))` is never returned.)
    #[inline]
    fn complete(&mut self) -> Option<DecodedPiece<'i>> {
        macro_rules! sequence {
            () => {
                self.decoder.incomplete_sequence
            }
        }
        let width = width(sequence!().first);
        debug_assert!(0 < sequence!().len && sequence!().len < width && width <= 4);

        let mut position = 0;
        macro_rules! next {
            () => {
                match self.input_chunk.get(position) {
                    Some(&b) => b,
                    None => {
                        let new_len = sequence!().len + position as u8;
                        debug_assert!(new_len < 4);
                        sequence!().len = new_len;
                        self.consume_all();
                        return None
                    }
                }
            }
        }

        macro_rules! check {
            ($valid: expr) => {
                if !$valid {
                    sequence!().len = 0;
                    self.consume(position);
                    return Some(DecodedPiece::Error)
                }
            }
        }

        if sequence!().len < 2 {
            sequence!().second = next!();
            let valid = match width {
                2 => is_continuation_byte(sequence!().second),
                3 => valid_three_bytes_sequence_prefix(sequence!().first, sequence!().second),
                _ => {
                    debug_assert!(width == 4);
                    valid_four_bytes_sequence_prefix(sequence!().first, sequence!().second)
                }
            };
            check!(valid);
            position += 1;
        }

        let mut fourth = 0;
        if width > 2 {
            if sequence!().len < 3 {
                sequence!().third = next!();
                check!(is_continuation_byte(sequence!().third));
                position += 1;
            }
            if width > 3 {
                fourth = next!();
                check!(is_continuation_byte(fourth));
                position += 1;
            }
        }

        sequence!().len = 0;
        self.consume(position);
        Some(DecodedPiece::AcrossChunks(StrChar {
            bytes: [sequence!().first, sequence!().second, sequence!().third, fourth],
            width: width as usize,
        }))
    }

    #[inline]
    fn decode_step(&mut self) -> Option<DecodedPiece<'i>> {
        let mut position = 0;
        loop {
            let first = match self.input_chunk.get(position) {
                Some(&b) => b,
                // we're at the end of the input and a codepoint
                // boundary at the same time, so this string is valid.
                None => return if position > 0 {
                    let slice = unsafe {
                        str::from_utf8_unchecked(self.input_chunk)
                    };
                    self.consume_all();
                    Some(DecodedPiece::InputSlice(slice))
                } else {
                    None
                }
            };
            // ASCII characters are always valid, so only large
            // bytes need more examination.
            if first < 128 {
                position += 1
            } else {
                macro_rules! valid_prefix {
                    ($current_sequence_len: expr) => {
                        {
                            let slice = unsafe {
                                str::from_utf8_unchecked(&self.input_chunk[..position])
                            };
                            self.consume(position + $current_sequence_len);
                            return Some(DecodedPiece::InputSlice(slice))
                        }
                    }
                }

                macro_rules! next {
                    ($current_sequence_len: expr, $first: expr, $second: expr, $third: expr) => {
                        match self.input_chunk.get(position + $current_sequence_len) {
                            Some(&b) => b,
                            None => {
                                self.decoder.incomplete_sequence = IncompleteSequence {
                                    len: $current_sequence_len,
                                    first: $first,
                                    second: $second,
                                    third: $third,
                                };
                                valid_prefix!($current_sequence_len);
                            }
                        }
                    }
                }

                macro_rules! check {
                    ($valid: expr, $current_sequence_len: expr) => {
                        if !$valid {
                            if position > 0 {
                                self.has_error_next = true;
                                valid_prefix!($current_sequence_len)
                            } else {
                                self.consume($current_sequence_len);
                                return Some(DecodedPiece::Error)
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
}

impl<'d, 'i> Iterator for ChunkDecoder<'d, 'i> {
    type Item = DecodedPiece<'i>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.has_error_next {
            self.has_error_next = false;
            Some(DecodedPiece::Error)
        } else if self.decoder.incomplete_sequence.len > 0 {
            self.complete()
        } else {
            self.decode_step()
        }
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

/// Like `char`, but represented in memory as UTF-8
#[derive(Copy, Clone)]
pub struct StrChar {
    bytes: [u8; 4],
    width: usize,
}

impl Deref for StrChar {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        let bytes = &self.bytes[..self.width];
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
