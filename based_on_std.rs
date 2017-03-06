use std::cmp;
use std::str;

include!("polyfill.rs");

#[derive(Debug, Copy, Clone)]
pub enum DecodeError<'a> {
    /// In lossy decoding insert `valid_prefix`, then `"\u{FFFD}"`,
    /// then call `decode()` again with `remaining_input`.
    Invalid {
        valid_prefix: &'a str,
        invalid_sequence: &'a [u8],
        remaining_input: &'a [u8],
    },

    /// Call the `incomplete_suffix.try_complete` method with more input when available.
    /// If no more input is available, this is an invalid byte sequence.
    Incomplete {
        valid_prefix: &'a str,
        incomplete_suffix: IncompleteChar,
    },
}

#[derive(Debug, Copy, Clone)]
pub struct IncompleteChar {
    pub buffer: [u8; 4],
    pub buffer_len: u8,
}

pub fn decode(input: &[u8]) -> Result<&str, DecodeError> {
    let error = match str::from_utf8(input) {
        Ok(valid) => return Ok(valid),
        Err(error) => error,
    };

    // FIXME: separate function from here to guide inlining?
    let (valid, after_valid) = input.split_at(error.valid_up_to());
    let valid = unsafe {
        str::from_utf8_unchecked(valid)
    };

    match utf8error_error_len(&error, input) {
        Some(invalid_sequence_length) => {
            let (invalid, rest) = after_valid.split_at(invalid_sequence_length);
            Err(DecodeError::Invalid {
                valid_prefix: valid,
                invalid_sequence: invalid,
                remaining_input: rest
            })
        }
        None => {
            Err(DecodeError::Incomplete {
                valid_prefix: valid,
                incomplete_suffix: IncompleteChar::new(after_valid),
            })
        }
    }
}

impl IncompleteChar {
    pub fn new(bytes: &[u8]) -> Self {
        let mut buffer = [0, 0, 0, 0];
        let len = bytes.len();
        buffer[..len].copy_from_slice(bytes);
        IncompleteChar {
            buffer: buffer,
            buffer_len: len as u8,
        }
    }

    /// * `None`: still incomplete, call `try_complete` again with more input.
    ///   If no more input is available, this is invalid byte sequence.
    /// * `Some((result, rest))`: We’re done with this `IncompleteChar`.
    ///   To keep decoding, pass `rest` to `decode()`.
    pub fn try_complete<'char, 'input>(&'char mut self, input: &'input [u8])
                                       -> Option<(Result<&'char str, &'char [u8]>, &'input [u8])> {
        let buffer_len = self.buffer_len as usize;
        let bytes_from_input;
        {
            let unwritten = &mut self.buffer[buffer_len..];
            bytes_from_input = cmp::min(unwritten.len(), input.len());
            unwritten[..bytes_from_input].copy_from_slice(&input[..bytes_from_input]);
        }
        let spliced = &self.buffer[..buffer_len + bytes_from_input];
        match str::from_utf8(spliced) {
            Ok(valid) => {
                self.buffer_len = 0;
                Some((Ok(valid), &input[bytes_from_input..]))
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                if valid_up_to > 0 {
                    let valid = &self.buffer[..valid_up_to];
                    let valid = unsafe {
                        str::from_utf8_unchecked(valid)
                    };
                    let bytes_from_input = valid_up_to.checked_sub(buffer_len).unwrap();
                    self.buffer_len = 0;
                    Some((Ok(valid), &input[bytes_from_input..]))
                } else {
                    match utf8error_error_len(&error, spliced) {
                        Some(invalid_sequence_length) => {
                            let invalid = &spliced[..invalid_sequence_length];
                            let bytes_from_input = invalid_sequence_length.checked_sub(buffer_len).unwrap();
                            let rest = &input[bytes_from_input..];
                            self.buffer_len = 0;
                            Some((Err(invalid), rest))
                        }
                        None => {
                            self.buffer_len = spliced.len() as u8;
                            None
                        }
                    }
                }
            }
        }
    }
}
