//! Chunk-safe helpers for streamed process output.

/// Decodes a byte stream as UTF-8 without ever splitting a multi-byte
/// character across chunks. Invalid bytes become U+FFFD.
#[derive(Debug, Default)]
pub struct Utf8Chunker {
    pending: Vec<u8>,
}

impl Utf8Chunker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::new();
        let mut rest: &[u8] = &self.pending;
        loop {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    out.push_str(s);
                    rest = &[];
                    break;
                }
                Err(e) => {
                    let (valid, after) = rest.split_at(e.valid_up_to());
                    // SAFETY-free: `valid` was just validated by from_utf8.
                    out.push_str(std::str::from_utf8(valid).unwrap_or_default());
                    match e.error_len() {
                        Some(len) => {
                            out.push('\u{FFFD}');
                            rest = &after[len..];
                        }
                        None => {
                            // Incomplete sequence at the end: keep it for the next chunk.
                            rest = after;
                            break;
                        }
                    }
                }
            }
        }
        self.pending = rest.to_vec();
        out
    }

    /// Flush whatever is left when the stream ends.
    pub fn finish(&mut self) -> String {
        let rest = std::mem::take(&mut self.pending);
        String::from_utf8_lossy(&rest).into_owned()
    }
}

pub const TOKEN_OPEN: &str = "[[jarvis:";
pub const TOKEN_CLOSE: &str = "]]";
const TOKEN_MAX: usize = 64;

/// Removes `[[jarvis:…]]` control tokens (sudo prompt marker, remote PID)
/// from a text stream and reports their payloads. Tokens may be split across
/// chunks; a possible partial token is held back until it resolves.
#[derive(Debug, Default)]
pub struct TokenFilter {
    held: String,
    /// A token ended exactly at a chunk boundary; its newline may still come.
    eat_newline: bool,
}

impl TokenFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &str) -> (String, Vec<String>) {
        let chunk = match chunk.strip_prefix('\n') {
            Some(rest) if self.eat_newline => rest,
            _ => chunk,
        };
        if !chunk.is_empty() {
            self.eat_newline = false;
        }
        self.held.push_str(chunk);
        let mut out = String::new();
        let mut tokens = Vec::new();
        loop {
            match self.held.find(TOKEN_OPEN) {
                Some(start) => {
                    let body_start = start + TOKEN_OPEN.len();
                    match self.held[body_start..].find(TOKEN_CLOSE) {
                        Some(len) if len <= TOKEN_MAX => {
                            out.push_str(&self.held[..start]);
                            tokens.push(self.held[body_start..body_start + len].to_string());
                            let mut end = body_start + len + TOKEN_CLOSE.len();
                            // A token printed with `echo` drags a newline along.
                            if self.held[end..].starts_with('\n') {
                                end += 1;
                            } else if end == self.held.len() {
                                self.eat_newline = true;
                            }
                            self.held.drain(..end);
                        }
                        Some(_) => {
                            // Not one of ours: emit the opener literally and move on.
                            out.push_str(&self.held[..body_start]);
                            self.held.drain(..body_start);
                        }
                        None if self.held.len() - body_start > TOKEN_MAX => {
                            out.push_str(&self.held[..body_start]);
                            self.held.drain(..body_start);
                        }
                        None => {
                            out.push_str(&self.held[..start]);
                            self.held.drain(..start);
                            break;
                        }
                    }
                }
                None => {
                    let keep = partial_suffix(&self.held, TOKEN_OPEN);
                    let cut = self.held.len() - keep;
                    out.push_str(&self.held[..cut]);
                    self.held.drain(..cut);
                    break;
                }
            }
        }
        (out, tokens)
    }

    pub fn finish(&mut self) -> String {
        std::mem::take(&mut self.held)
    }
}

/// Length of the longest suffix of `text` that is a proper prefix of `pattern`.
fn partial_suffix(text: &str, pattern: &str) -> usize {
    (1..pattern.len().min(text.len() + 1))
        .rev()
        .find(|&n| text.is_char_boundary(text.len() - n) && text.ends_with(&pattern[..n]))
        .unwrap_or(0)
}

/// Strip all control tokens from a complete buffer.
pub fn strip_tokens(text: &str) -> (String, Vec<String>) {
    let mut filter = TokenFilter::new();
    let (mut out, tokens) = filter.push(text);
    out.push_str(&filter.finish());
    (out, tokens)
}

/// Heuristic used before opening a file in the text editor.
pub fn looks_binary(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(8192)];
    if sample.contains(&0) {
        return true;
    }
    let control = sample.iter().filter(|b| **b < 0x20 && !matches!(**b, b'\n' | b'\r' | b'\t' | 0x0c | 0x1b)).count();
    !sample.is_empty() && control * 10 > sample.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_split_across_chunks() {
        let text = "zażółć 🐧 gęślą";
        let bytes = text.as_bytes();
        for split in 0..=bytes.len() {
            let mut c = Utf8Chunker::new();
            let mut out = c.push(&bytes[..split]);
            out.push_str(&c.push(&bytes[split..]));
            out.push_str(&c.finish());
            assert_eq!(out, text, "split at {split}");
        }
    }

    #[test]
    fn utf8_byte_by_byte() {
        let text = "日本語 ok";
        let mut c = Utf8Chunker::new();
        let out: String = text.as_bytes().iter().map(|b| c.push(&[*b])).collect();
        assert_eq!(out, text);
    }

    #[test]
    fn utf8_invalid_bytes_are_replaced() {
        let mut c = Utf8Chunker::new();
        assert_eq!(c.push(b"a\xffb"), "a\u{FFFD}b");
        assert_eq!(c.push(b"\xe2\x82"), "");
        assert_eq!(c.finish(), "\u{FFFD}");
    }

    #[test]
    fn tokens_are_removed_and_reported() {
        let (out, tokens) = strip_tokens("[[jarvis:sudo]][[jarvis:pid:4242]]\nhello\n");
        assert_eq!(out, "hello\n");
        assert_eq!(tokens, vec!["sudo", "pid:4242"]);
    }

    #[test]
    fn tokens_split_at_every_position() {
        let text = "before [[jarvis:pid:17]]\nafter [[not ours]] end";
        for split in 0..=text.len() {
            let mut f = TokenFilter::new();
            let (mut out, mut tokens) = f.push(&text[..split]);
            let (o2, t2) = f.push(&text[split..]);
            out.push_str(&o2);
            tokens.extend(t2);
            out.push_str(&f.finish());
            assert_eq!(out, "before after [[not ours]] end", "split at {split}");
            assert_eq!(tokens, vec!["pid:17"]);
        }
    }

    #[test]
    fn unterminated_opener_is_eventually_released() {
        let mut f = TokenFilter::new();
        let (out, _) = f.push("x [[jarvis:");
        assert_eq!(out, "x ");
        let long = "y".repeat(100);
        let (out, tokens) = f.push(&long);
        assert!(tokens.is_empty());
        assert_eq!(format!("{out}{}", f.finish()), format!("[[jarvis:{long}"));
    }

    #[test]
    fn binary_detection() {
        assert!(!looks_binary(b"plain text\nwith lines\n"));
        assert!(!looks_binary("zażółć".as_bytes()));
        assert!(looks_binary(b"\x7fELF\x00\x01"));
        assert!(!looks_binary(b""));
    }
}
