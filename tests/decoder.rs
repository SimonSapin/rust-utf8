extern crate utf8;

use utf8::PushLossyDecoder;

#[path = "shared/data.rs"]
mod data;


/// This takes a while in debug mode. Use --release
#[test]
fn test_incremental_decoder() {
    let mut chunks = Vec::new();
    for &(input, expected) in data::DECODED_LOSSY {
        all_partitions(&mut chunks, input, expected);
        assert_eq!(chunks.len(), 0);
    }
}

fn all_partitions<'a>(chunks: &mut Vec<&'a [u8]>, input: &'a [u8], expected: &str) {
    if input.is_empty() {
        let mut string = String::new();
        {
            let mut decoder = PushLossyDecoder::new(|s| string.push_str(s));
            for &chunk in &*chunks {
                decoder.feed(chunk);
            }
            decoder.end();
        }
        assert_eq!(string, expected);
    }
    for i in (1..input.len()).rev() {
        chunks.push(&input[..i]);
        all_partitions(chunks, &input[i..], expected);
        chunks.pop();
    }
}
