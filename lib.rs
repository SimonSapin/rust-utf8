#[macro_use] extern crate matches;

use std::ops::Deref;
use std::result;
use std::str;

/// The replacement character, U+FFFD. In lossy decoding, insert it for every decoding error.
pub const REPLACEMENT_CHARACTER: &'static str = "\u{FFFD}";

/// A low-level, zero-copy UTF-8 decoder with error handling.
///
/// This decoder can process input one chunk at a time,
/// returns `&str` Unicode slices into the given `&[u8]` bytes input,
/// and stops at each error to let the caller deal with it however they choose.
///
/// For example, `String::from_utf8_lossy` (but returning `String` instead of `Cow`)
/// can be rewritten as:
///
/// ```rust
/// fn string_from_utf8_lossy(mut input: &[u8]) -> String {
///     let mut decoder = utf8::Decoder::new();
///     let mut string = String::new();
///     loop {
///         let (reconstituted, decoded, result) = decoder.decode(input);
///         debug_assert!(reconstituted.is_empty());  // We only have one chunk of input.
///         string.push_str(decoded);
///         match result {
///             utf8::Result::Ok => return string,
///             utf8::Result::Incomplete => {
///                 string.push_str(utf8::REPLACEMENT_CHARACTER);
///                 return string
///             }
///             utf8::Result::Error { remaining_input_after_error } => {
///                 string.push_str(utf8::REPLACEMENT_CHARACTER);
///                 input = remaining_input_after_error;
///             }
///         }
///     }
/// }
/// ```
///
/// See also [`LossyDecoder`](struct.LossyDecoder.html).
pub struct Decoder {
    incomplete_sequence: IncompleteSequence,
}

/// `len == 0` means no sequence
struct IncompleteSequence {
    len: u8,
    first: u8,
    second: u8,
    third: u8,
}

impl Decoder {
    /// Create a new decoder.
    #[inline]
    pub fn new() -> Decoder {
        Decoder {
            incomplete_sequence: IncompleteSequence {
                len: 0,
                first: 0,
                second: 0,
                third: 0,
            }
        }
    }

    /// Return whether the input of the last call to `.decode()` returned `Result::Incomplete`.
    /// If this is true and there is no more input, this is a decoding error.
    #[inline]
    pub fn has_incomplete_sequence(&self) -> bool {
        self.incomplete_sequence.len > 0
    }

    /// Start decoding one chunk of input bytes. The return value is a tuple of:
    ///
    /// * An inline buffer of up to 4 bytes that dereferences to `&str`.
    ///   When the length is non-zero
    ///   (which can only happen when calling `Decoder::decode` with more input
    ///   after the previous call returned `Result::Incomplete`),
    ///   it represents a single code point that was re-assembled from multiple input chunks.
    /// * The Unicode slice of at the start of the input bytes chunk that is well-formed UTF-8.
    ///   May be empty, for example when a decoding error occurs immediately after another.
    /// * Details about the rest of the input chuck.
    ///   See the documentation of [`Result`](enum.Result.html).
    pub fn decode<'a>(&mut self, input_chunk: &'a [u8]) -> (InlineString, &'a str, Result<'a>) {
        let (ch, input) = match self.incomplete_sequence.complete(input_chunk) {
            Ok(tuple) => tuple,
            Err(result) => return (InlineString::empty(), "", result)
        };

        let mut position = 0;
        loop {
            let first = match input.get(position) {
                Some(&b) => b,
                // we're at the end of the input and a codepoint
                // boundary at the same time, so this string is valid.
                None => return (
                    ch,
                    unsafe {
                        str::from_utf8_unchecked(input)
                    },
                    Result::Ok,
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
                                self.incomplete_sequence = IncompleteSequence {
                                    len: $current_sequence_len,
                                    first: $first,
                                    second: $second,
                                    third: $third,
                                };
                                return (ch, valid_prefix!(), Result::Incomplete)
                            }
                        }
                    }
                }

                macro_rules! check {
                    ($valid: expr, $current_sequence_len: expr) => {
                        if !$valid {
                            return (
                                ch,
                                valid_prefix!(),
                                Result::Error {
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
}

#[derive(Debug, Copy, Clone)]
pub enum Result<'a> {
    /// The input chunk is entirely well-formed.
    /// The returned `&str` goes to its end.
    Ok,

    /// The end of the input chunk was reached in the middle of an UTF-8 sequence
    /// that is valid so far.
    /// More input (up to 3 more bytes) is required to decode that sequence.
    /// At the end of the input, the sequence is ill-formed and this is a decoding error.
    Incomplete,

    /// An ill-formed byte sequence was found. This is a decoding error.
    /// If errors are not fatal, decoding should continue after handling the error
    /// (typically by appending a U+FFFD replacement character to the output)
    /// by calling `Decoder::decode` again with `remaining_input_after_error` as its argument.
    Error { remaining_input_after_error: &'a [u8] },
}

impl IncompleteSequence {
    fn complete<'a>(&mut self, input: &'a [u8])
                    -> result::Result<(InlineString, &'a [u8]), Result<'a>> {
        if self.len == 0 {
            return Ok((InlineString::empty(), input))
        }
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
                        return Err(Result::Incomplete)
                    }
                }
            }
        }

        macro_rules! check {
            ($valid: expr) => {
                if !$valid {
                    self.len = 0;
                    return Err(Result::Error { remaining_input_after_error: &input[position..] })
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

        let ch = InlineString {
            buffer: [self.first, self.second, self.third, fourth],
            len: width,
        };
        self.len = 0;
        Ok((ch, &input[position..]))
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

/// A push-based, lossy decoder for UTF-8.
/// Errors are replaced with the U+FFFD replacement character.
///
/// Users “push” bytes into the decoder, which in turn “pushes” `&str` slices into a callback.
///
/// For example, `String::from_utf8_lossy` (but returning `String` instead of `Cow`)
/// can be rewritten as:
///
/// ```rust
/// fn string_from_utf8_lossy(input: &[u8]) -> String {
///     let mut string = String::new();
///     utf8::LossyDecoder::new(|s| string.push_str(s)).feed(input);
///     string
/// }
/// ```
///
/// **Note:** Dropping the decoder signals the end of the input:
/// If the last input chunk ended with an incomplete byte sequence for a code point,
/// this is an error and a replacement character is emitted.
/// Use `std::mem::forget` to inhibit this behavior.
pub struct LossyDecoder<F: FnMut(&str)> {
    push_str: F,
    decoder: Decoder,
}

impl<F: FnMut(&str)> LossyDecoder<F> {
    /// Create a new decoder from a callback.
    #[inline]
    pub fn new(push_str: F) -> Self {
        LossyDecoder {
            push_str: push_str,
            decoder: Decoder::new(),
        }
    }

    /// Feed one chunk of input into the decoder.
    ///
    /// The input is decoded lossily
    /// and the callback called once or more with `&str` string slices.
    ///
    /// If the UTF-8 byte sequence for one code point was split into this bytes chunk
    /// and previous bytes chunks, it will be correctly pieced back together.
    pub fn feed(&mut self, mut input: &[u8]) {
        loop {
            let (ch, s, result) = self.decoder.decode(input);
            if !ch.is_empty() {
                (self.push_str)(&ch);
            }
            if !s.is_empty() {
                (self.push_str)(s);
            }
            match result {
                Result::Ok | Result::Incomplete => break,
                Result::Error { remaining_input_after_error: remaining } => {
                    (self.push_str)(REPLACEMENT_CHARACTER);
                    input = remaining;
                }
            }
        }
    }
}

impl<F: FnMut(&str)> Drop for LossyDecoder<F> {
    #[inline]
    fn drop(&mut self) {
        if self.decoder.has_incomplete_sequence() {
            (self.push_str)(REPLACEMENT_CHARACTER)
        }
    }
}

/// Like `String`, but does not allocate memory and has a fixed capacity of 4 bytes.
/// This is used by `Decoder` to represent either the empty string or a single code point.
#[derive(Copy, Clone)]
pub struct InlineString {
    buffer: [u8; 4],
    len: u8,
}

impl Deref for InlineString {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        unsafe {
            str::from_utf8_unchecked(&self.buffer[..self.len as usize])
        }
    }
}

impl InlineString {
    fn empty() -> InlineString {
        InlineString {
            buffer: [0, 0, 0, 0],
            len: 0,
        }
    }

    // Bypass bounds check in deref()

    /// Returns the length of `self`.
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// Returns true if this string has a length of zero bytes.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}
