use std::str::Utf8Error;

/// Remove this when https://github.com/rust-lang/rust/pull/40212 is stable
fn utf8error_resume_from(error: &Utf8Error, input: &[u8]) -> Option<usize> {
    let valid_up_to = error.valid_up_to();
    let after_valid = &input[valid_up_to..];

    // `after_valid` is not empty, `str::from_utf8` would have returned `Ok(_)`.
    let first = after_valid[0];
    let char_width = UTF8_CHAR_WIDTH[first as usize];

    macro_rules! get_byte {
        ($i: expr) => {
            if let Some(&byte) = after_valid.get($i) {
                byte
            } else {
                return None
            }
        }
    }

    let invalid_sequence_length;
    match char_width {
        0 => invalid_sequence_length = 1,
        1 => panic!("found ASCII byte after Utf8Error.valid_up_to()"),
        2 => {
            let second = get_byte!(1);
            debug_assert!(!is_continuation_byte(second));
            invalid_sequence_length = 1;
        }
        3 => {
            let second = get_byte!(1);
            if valid_three_bytes_sequence_prefix(first, second) {
                let third = get_byte!(2);
                debug_assert!(!is_continuation_byte(third));
                invalid_sequence_length = 2;
            } else {
                invalid_sequence_length = 1;
            }
        }
        4 => {
            let second = get_byte!(1);
            if valid_four_bytes_sequence_prefix(first, second) {
                let third = get_byte!(2);
                if is_continuation_byte(third) {
                    let fourth = get_byte!(3);
                    debug_assert!(!is_continuation_byte(fourth));
                    invalid_sequence_length = 3;
                } else {
                    invalid_sequence_length = 2;
                }
            } else {
                invalid_sequence_length = 1;
            }
        }
        _ => unreachable!()
    }

    Some(valid_up_to + invalid_sequence_length)
}
