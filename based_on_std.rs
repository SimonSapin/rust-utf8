use std::str;

include!("polyfill.rs");

#[derive(Debug, Copy, Clone)]
pub enum DecodeResult<'a> {
    Ok(&'a str),
    Error(&'a str, InvalidSequence<'a>, &'a [u8]),
    Incomplete(&'a str, IncompleteChar),
}

#[derive(Debug, Copy, Clone)]
pub struct InvalidSequence<'a>(pub &'a [u8]);

#[derive(Debug, Copy, Clone)]
pub struct IncompleteChar {
    buffer: [u8; 4],
    buffer_len: u8,
    char_width: u8,
}

pub fn decode(input: &[u8]) -> DecodeResult {
    let error = match str::from_utf8(input) {
        Ok(valid) => return DecodeResult::Ok(valid),
        Err(error) => error,
    };

    // FIXME: separate function from here to guide inlining?
    let valid_up_to = error.valid_up_to();
    let (valid, after_valid) = input.split_at(valid_up_to);
    let valid = unsafe {
        str::from_utf8_unchecked(valid)
    };

    match utf8error_resume_from(&error, input) {
        Some(resume_from) => {
            let invalid_sequence_length = resume_from - valid_up_to;
            let (invalid, rest) = after_valid.split_at(invalid_sequence_length);
            DecodeResult::Error(valid, InvalidSequence(invalid), rest)
        }
        None => {
            let mut buffer = [0, 0, 0, 0];
            let after_valid = &input[error.valid_up_to()..];
            buffer[..after_valid.len()].copy_from_slice(after_valid);
            DecodeResult::Incomplete(valid, IncompleteChar {
                buffer: buffer,
                buffer_len: after_valid.len() as u8,
                char_width: UTF8_CHAR_WIDTH[buffer[0] as usize],
            })
        }
    }
}

pub enum TryCompleteResult<'char, 'input> {
    Ok(&'char str, &'input [u8]),
    Error(InvalidSequence<'char>, &'input [u8]),
    StillIncomplete,
}

impl IncompleteChar {
    pub fn try_complete<'char, 'input>(&'char mut self, mut input: &'input [u8])
                                       -> TryCompleteResult<'char, 'input> {
        macro_rules! require {
            ($condition: expr) => {
                if !$condition {
                    self.char_width = 0xFF;  // Make try_complete panic if called again
                    let invalid = &self.buffer[..self.buffer_len as usize];
                    return TryCompleteResult::Error(InvalidSequence(invalid), input)
                }
            }
        }

        macro_rules! take_one_byte {
            () => {
                if let Some((&next_byte, rest)) = input.split_first() {
                    self.buffer[self.buffer_len as usize] = next_byte;
                    self.buffer_len += 1;
                    input = rest;
                    next_byte
                } else {
                    return TryCompleteResult::StillIncomplete
                }
            }
        }

        match (self.buffer_len, self.char_width) {
            (1, 2) | (2, 3) | (3, 4) => {
                require!(is_continuation_byte(take_one_byte!()));
            }
            (1, 3) => {
                require!(valid_three_bytes_sequence_prefix(self.buffer[0], take_one_byte!()));
                require!(is_continuation_byte(take_one_byte!()));
            }
            (1, 4) => {
                require!(valid_four_bytes_sequence_prefix(self.buffer[0], take_one_byte!()));
                require!(is_continuation_byte(take_one_byte!()));
                require!(is_continuation_byte(take_one_byte!()));
            }
            (2, 4) => {
                require!(is_continuation_byte(take_one_byte!()));
                require!(is_continuation_byte(take_one_byte!()));
            }
            _ => panic!("IncompleteChar::try_complete called again after returning \
                         TryCompleteResult::Ok or TryCompleteResult::Error")
        }

        // try_complete will panic if called again:
        debug_assert!(self.buffer_len == self.char_width);

        let one_code_point = &self.buffer[..self.buffer_len as usize];
        debug_assert!(str::from_utf8(one_code_point).is_ok());
        let one_code_point = unsafe {
            str::from_utf8_unchecked(one_code_point)
        };
        TryCompleteResult::Ok(one_code_point, input)
    }
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
