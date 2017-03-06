use std::cmp;
use std::str;

include!("polyfill.rs");

#[derive(Debug, Copy, Clone)]
pub enum DecodeError<'a> {
    Invalid {
        valid_prefix: &'a str,

        /// In lossy decoding, replace this with "\u{FFFD}"
        invalid_sequence: &'a [u8],

        /// To keep decoding, call `decode()` again with this.
        remaining_input: &'a [u8],
    },
    Incomplete {
        valid_prefix: &'a str,

        /// Call the `try_complete` method with more input is available.
        /// If no more input is available, this is a decoding error.
        incomplete_suffix: IncompleteChar,
    },
}

#[derive(Debug, Copy, Clone)]
pub struct IncompleteChar {
    buffer: [u8; 4],
    buffer_len: u8,
}

pub fn decode(input: &[u8]) -> Result<&str, DecodeError> {
    let error = match str::from_utf8(input) {
        Ok(valid) => return Ok(valid),
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
            Err(DecodeError::Invalid {
                valid_prefix: valid,
                invalid_sequence: invalid,
                remaining_input: rest
            })
        }
        None => {
            let mut buffer = [0, 0, 0, 0];
            let after_valid = &input[error.valid_up_to()..];
            buffer[..after_valid.len()].copy_from_slice(after_valid);
            Err(DecodeError::Incomplete {
                valid_prefix: valid,
                incomplete_suffix: IncompleteChar {
                    buffer: buffer,
                    buffer_len: after_valid.len() as u8,
                }
            })
        }
    }
}

impl IncompleteChar {
    /// * `None`: still incomplete, call `try_complete` again with more input.
    ///   If no more input is available, this is a decoding error.
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
                Some((Ok(valid), &input[bytes_from_input..]))
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                if valid_up_to > 0 {
                    let valid = &self.buffer[..valid_up_to];
                    let valid = unsafe {
                        str::from_utf8_unchecked(valid)
                    };
                    assert!(valid_up_to > buffer_len);
                    let bytes_from_input = valid_up_to - buffer_len;
                    Some((Ok(valid), &input[bytes_from_input..]))
                } else {
                    match utf8error_resume_from(&error, spliced) {
                        Some(resume_from) => {
                            let invalid = &spliced[..resume_from];
                            assert!(resume_from > buffer_len);
                            let bytes_from_input = resume_from - buffer_len;
                            let rest = &input[bytes_from_input..];
                            Some((Err(invalid), rest))
                        }
                        None => None
                    }
                }
            }
        }
    }
}
