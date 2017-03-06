#[macro_use] extern crate matches;

include!("based_on_std.rs");

/// The replacement character, U+FFFD. In lossy decoding, insert it for every decoding error.
pub const REPLACEMENT_CHARACTER: &'static str = "\u{FFFD}";

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
    incomplete: Option<IncompleteChar>,
}

impl<F: FnMut(&str)> LossyDecoder<F> {
    /// Create a new decoder from a callback.
    #[inline]
    pub fn new(push_str: F) -> Self {
        LossyDecoder {
            push_str: push_str,
            incomplete: None,
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
        if let Some(mut incomplete) = self.incomplete.take() {
            match incomplete.try_complete(input) {
                Some((Ok(s), remaining)) => {
                    (self.push_str)(s);
                    input = remaining
                }
                Some((Err(_), remaining)) => {
                    (self.push_str)(REPLACEMENT_CHARACTER);
                    input = remaining
                }
                None => {
                    return
                }
            }
        }
        loop {
            match decode(input) {
                DecodeResult::Ok(s) => {
                    (self.push_str)(s);
                    return
                }
                DecodeResult::Incomplete(s, i) => {
                    (self.push_str)(s);
                    self.incomplete = Some(i);
                    return
                }
                DecodeResult::Error(s, _, remaining) => {
                    (self.push_str)(s);
                    (self.push_str)(REPLACEMENT_CHARACTER);
                    input = remaining
                }
            }
        }
    }
}

impl<F: FnMut(&str)> Drop for LossyDecoder<F> {
    #[inline]
    fn drop(&mut self) {
        if self.incomplete.is_some() {
            (self.push_str)(REPLACEMENT_CHARACTER)
        }
    }
}
