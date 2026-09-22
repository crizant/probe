//! Base64 and hex views of a response body.

use std::fmt::Write;

use super::{PreparedDocument, ResponseImagePreview};

const BASE64_LINE_LENGTH: usize = 76;

pub(super) fn page_bytes(document: &PreparedDocument) -> &[u8] {
    if let Some(ResponseImagePreview::Ready(image)) = &document.image_preview {
        image.bytes()
    } else if document.binary {
        &document.page_body
    } else {
        document.raw_text.as_bytes()
    }
}

pub(crate) fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let output_len = input.len().div_ceil(3).saturating_mul(4);
    let mut output = vec![0; output_len];
    let mut offset = 0;
    let mut index = 0;
    while index + 3 <= input.len() {
        let n = (u32::from(input[index]) << 16)
            | (u32::from(input[index + 1]) << 8)
            | u32::from(input[index + 2]);
        output[offset] = TABLE[((n >> 18) & 0x3F) as usize];
        output[offset + 1] = TABLE[((n >> 12) & 0x3F) as usize];
        output[offset + 2] = TABLE[((n >> 6) & 0x3F) as usize];
        output[offset + 3] = TABLE[(n & 0x3F) as usize];
        index += 3;
        offset += 4;
    }
    match input.len() - index {
        1 => {
            let n = u32::from(input[index]) << 16;
            output[offset] = TABLE[((n >> 18) & 0x3F) as usize];
            output[offset + 1] = TABLE[((n >> 12) & 0x3F) as usize];
            output[offset + 2] = b'=';
            output[offset + 3] = b'=';
        }
        2 => {
            let n = (u32::from(input[index]) << 16) | (u32::from(input[index + 1]) << 8);
            output[offset] = TABLE[((n >> 18) & 0x3F) as usize];
            output[offset + 1] = TABLE[((n >> 12) & 0x3F) as usize];
            output[offset + 2] = TABLE[((n >> 6) & 0x3F) as usize];
            output[offset + 3] = b'=';
        }
        _ => {}
    }
    wrap_base64(output)
}

fn wrap_base64(encoded: Vec<u8>) -> String {
    if encoded.len() <= BASE64_LINE_LENGTH {
        return String::from_utf8(encoded).expect("base64 alphabet is ASCII");
    }
    let extra_newlines = encoded.len().saturating_sub(1) / BASE64_LINE_LENGTH;
    let mut wrapped = String::with_capacity(encoded.len() + extra_newlines);
    for (index, chunk) in encoded.chunks(BASE64_LINE_LENGTH).enumerate() {
        if index > 0 {
            wrapped.push('\n');
        }
        wrapped.push_str(std::str::from_utf8(chunk).expect("base64 alphabet is ASCII"));
    }
    wrapped
}

pub(crate) fn encode_hex(input: &[u8], base_offset: usize) -> String {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    const BYTES_PER_LINE: usize = 16;

    if input.is_empty() {
        return String::new();
    }

    let line_count = input.len().div_ceil(BYTES_PER_LINE);
    let mut output = String::with_capacity(line_count * 80);

    for (line_index, chunk) in input.chunks(BYTES_PER_LINE).enumerate() {
        if line_index > 0 {
            output.push('\n');
        }

        let offset = base_offset + (line_index * BYTES_PER_LINE);
        let _ = write!(output, "{:08x}  ", offset);

        for (byte_index, byte) in chunk.iter().enumerate() {
            if byte_index == 8 {
                output.push(' ');
            }
            output.push(HEX_CHARS[(byte >> 4) as usize] as char);
            output.push(HEX_CHARS[(byte & 0x0F) as usize] as char);
            output.push(' ');
        }

        let missing = BYTES_PER_LINE - chunk.len();
        for index in 0..missing {
            if chunk.len() + index == 8 {
                output.push(' ');
            }
            output.push_str("   ");
        }

        output.push_str(" |");
        for byte in chunk {
            if byte.is_ascii_graphic() || *byte == b' ' {
                output.push(*byte as char);
            } else {
                output.push('.');
            }
        }
        output.push('|');
    }

    output
}
