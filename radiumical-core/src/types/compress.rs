/// Threshold for lz4 transparent compression (1 KB).
pub(crate) const COMPRESS_THRESHOLD: usize = 1024;

/// Magic prefix indicating lz4-compressed text content.
pub(crate) const LZ4_PREFIX: &str = "\x00lz4:";

/// Compress text with lz4, returning prefixed string. Returns None on failure.
pub fn compress_text(text: &str) -> Option<String> {
    let compressed = lz4_flex::compress_prepend_size(text.as_bytes());
    let encoded = base64_encode(&compressed);
    Some(format!("{LZ4_PREFIX}{encoded}"))
}

/// Decompress lz4-prefixed text. Returns None on failure.
pub fn decompress_text(s: &str) -> Option<String> {
    let encoded = s.strip_prefix(LZ4_PREFIX)?;
    let compressed = base64_decode(encoded)?;
    let bytes = lz4_flex::decompress_size_prepended(&compressed).ok()?;
    String::from_utf8(bytes).ok()
}

/// Minimal base64 encode (no external dep needed for this use case).
pub(crate) fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() * 4 / 3) + 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

pub(crate) fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 {
            return None;
        }
        let a = val(chunk[0])? as u32;
        let b = val(chunk[1])? as u32;
        let c = if chunk.len() > 2 && chunk[2] != b'=' {
            val(chunk[2])? as u32
        } else {
            0
        };
        let d = if chunk.len() > 3 && chunk[3] != b'=' {
            val(chunk[3])? as u32
        } else {
            0
        };
        let triple = (a << 18) | (b << 12) | (c << 6) | d;
        out.push((triple >> 16) as u8);
        if chunk.len() > 2 && chunk[2] != b'=' {
            out.push((triple >> 8) as u8);
        }
        if chunk.len() > 3 && chunk[3] != b'=' {
            out.push(triple as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_decompress_small() {
        let input = "Hello, World!";
        let compressed = compress_text(input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn compress_decompress_exactly_1kb() {
        let input = "x".repeat(1024);
        let compressed = compress_text(&input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn compress_decompress_large() {
        let input = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(200);
        assert!(input.len() > 10_000);
        let compressed = compress_text(&input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn compress_decompress_unicode() {
        let input = "😀🎉💻🚀✨αβγδελΩΣΠ".repeat(50);
        let compressed = compress_text(&input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn compress_decompress_cjk() {
        let input = "日本語テスト文字列です。这是中文测试文本。한국어 테스트 문자열입니다.".repeat(80);
        assert!(input.len() > 1024);
        let compressed = compress_text(&input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn compress_decompress_newlines_and_special() {
        let input = "line1\nline2\r\nline3\tindented\n\n".repeat(100);
        let compressed = compress_text(&input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn compress_decompress_repeated_patterns() {
        let input = "ABCD".repeat(5000);
        let compressed = compress_text(&input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn compress_decompress_null_bytes() {
        let input = "\x00hello\x00world\x00".repeat(200);
        let compressed = compress_text(&input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn decompress_invalid_returns_none() {
        assert!(decompress_text("not compressed").is_none());
        assert!(decompress_text(&format!("{LZ4_PREFIX}!!!bad_base64!!!")).is_none());
        assert!(decompress_text(&format!("{LZ4_PREFIX}{}", base64_encode(b"garbage"))).is_none());
    }

    #[test]
    fn test_compress_decompress_empty() {
        let input = "";
        let compressed = compress_text(input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn test_compress_decompress_unicode() {
        let input = "日本語テスト中文测试한국어테스트";
        let compressed = compress_text(input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn test_base64_roundtrip_random() {
        let data: Vec<u8> = (0..1000).map(|i| ((i * 7 + 13) ^ (i >> 3)) as u8).collect();
        let encoded = base64_encode(&data);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }
}
