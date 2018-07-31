use std::io::{self, BufRead};
use std::str;
use super::*;

/// Wraps a `std::io::BufRead` bufferred byte stream and decode it as UTF-8.
pub struct BufReadDecoder<B: BufRead> {
    buf_read: B,
    bytes_consumed: usize,
    incomplete: Incomplete,
}

/// Represents one UTF-8 error in the byte stream.
///
/// In lossy decoding, each error should be replaced with U+FFFD.
/// (See `BufReadDecoder::next_lossy`.)
pub struct BufReadDecoderError<'a> {
    pub invalid_sequence: &'a [u8],
}

impl<B: BufRead> BufReadDecoder<B> {
    pub fn new(buf_read: B) -> Self {
        Self {
            buf_read,
            bytes_consumed: 0,
            incomplete: Incomplete::empty(),
        }
    }

    /// Same as `BufReadDecoder::next`, but replace UTF-8 errors with U+FFFD replacement characters.
    pub fn next_lossy(&mut self) -> io::Result<Option<&str>> {
        let io_result = self.next();
        io_result.map(|option| {
            option.map(|decode_result| {
                decode_result.unwrap_or(REPLACEMENT_CHARACTER)
            })
        })
    }

    /// Decode and consume the next chunk of UTF-8 input.
    ///
    /// This method should be called repeatedly until it returns `Ok(None)`,
    /// which presents EOF from the underlying byte stream.
    /// This is similar to `Iterator::next`,
    /// except that decoded chunks borrow the decoder (~iterator)
    /// so they need to be handled or copied before the next chunk can start decoding.
    ///
    /// The outer `Result` carries I/O errors from the underlying byte stream.
    /// The inner `Result` carries UTF-8 decoding errors.
    pub fn next(&mut self) -> io::Result<Option<Result<&str, BufReadDecoderError>>> {
        enum BytesSource {
            BufRead(usize),
            Incomplete,
        }
        let (source, result) = loop {
            if self.bytes_consumed > 0 {
                self.buf_read.consume(self.bytes_consumed);
                self.bytes_consumed = 0;
            }
            let buf = self.buf_read.fill_buf()?;

            // Force loop iteration to go through an explicit `continue`
            enum Unreachable {}
            let _: Unreachable = if self.incomplete.is_empty() {
                if buf.is_empty() {
                    return Ok(None)  // EOF
                }
                match str::from_utf8(buf) {
                    Ok(_) => {
                        break (BytesSource::BufRead(buf.len()), Ok(()))
                    }
                    Err(error) => {
                        let valid_up_to = error.valid_up_to();
                        if valid_up_to > 0 {
                            break (BytesSource::BufRead(valid_up_to), Ok(()))
                        }
                        match error.error_len() {
                            Some(invalid_sequence_length) => {
                                break (BytesSource::BufRead(invalid_sequence_length), Err(()))
                            }
                            None => {
                                self.bytes_consumed = buf.len();
                                self.incomplete = Incomplete::new(buf);
                                // need more input bytes
                                continue
                            }
                        }
                    }
                }
            } else {
                if buf.is_empty() {
                    break (BytesSource::Incomplete, Err(()))  // EOF with incomplete code point
                }
                let (consumed, opt_result) = self.incomplete.try_complete_offsets(buf);
                self.bytes_consumed = consumed;
                match opt_result {
                    None => {
                        // need more input bytes
                        continue
                    }
                    Some(result) => {
                        break (BytesSource::Incomplete, result)
                    }
                }
            };
        };
        let bytes = match source {
            BytesSource::BufRead(byte_count) => {
                self.bytes_consumed = byte_count;
                &self.buf_read.fill_buf()?[..byte_count]
            }
            BytesSource::Incomplete => {
                self.incomplete.take_buffer()
            }
        };
        let result = match result {
            Ok(()) => Ok(unsafe { str::from_utf8_unchecked(bytes) }),
            Err(()) => Err(BufReadDecoderError { invalid_sequence: bytes }),
        };
        Ok(Some(result))
    }
}
