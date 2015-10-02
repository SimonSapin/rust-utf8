extern crate utf8;

use utf8::Decoder;

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
        let mut decoder = Decoder::new();
        for &chunk in &*chunks {
            for piece in decoder.feed(chunk) {
                string.push_str(&piece)
            }
        }
        if let Some(piece) = decoder.end() {
            string.push_str(&piece)
        }
        assert_eq!(string, expected);
    }
    for i in (1..input.len()).rev() {
        chunks.push(&input[..i]);
        all_partitions(chunks, &input[i..], expected);
        chunks.pop();
    }
}
