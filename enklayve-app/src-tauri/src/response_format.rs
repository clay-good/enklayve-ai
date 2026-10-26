//! Response formatting: strip model control tokens and Qwen3 `<think>` reasoning
//! blocks from generated text, while preserving the model's Markdown intact.
//!
//! Two entry points that must agree with each other:
//! - [`sanitize_final`] cleans a complete response in one pass.
//! - [`StreamSanitizer`] cleans a token stream incrementally, and is guaranteed
//!   (by test) to produce the same visible text as [`sanitize_final`] would on
//!   the concatenated raw stream.
//!
//! We intentionally do NOT touch Markdown (bold, lists, headings, code fences):
//! the chat UI renders Markdown, so stripping it only produced the flat,
//! inconsistent output users saw before.

/// Markers that are never shown to the user. `<think>` additionally switches the
/// sanitizer into "drop until `</think>`" mode.
const DROP_MARKERS: &[&str] = &[
    "<|im_start|>",
    "<|im_end|>",
    "<|endoftext|>",
    "</think>",
];
const THINK_OPEN: &str = "<think>";

/// All markers whose partial prefix we must hold back at a chunk boundary.
fn all_markers() -> [&'static str; 5] {
    ["<think>", "</think>", "<|im_start|>", "<|im_end|>", "<|endoftext|>"]
}

/// Clean a complete response: drop `<think>...</think>` blocks (including an
/// unclosed trailing `<think>`), drop control tokens, collapse blank runs.
pub fn sanitize_final(input: &str) -> String {
    let mut s = input.to_string();

    // Remove complete think blocks. Done manually (no regex engine quirks with
    // nesting or newlines) by repeatedly cutting the first <think>..</think>.
    loop {
        let Some(open) = s.find(THINK_OPEN) else { break };
        if let Some(rel_close) = s[open..].find("</think>") {
            let close = open + rel_close + "</think>".len();
            s.replace_range(open..close, "");
        } else {
            // Unclosed think: drop everything from <think> onward.
            s.truncate(open);
            break;
        }
    }

    for m in DROP_MARKERS {
        s = s.replace(m, "");
    }

    collapse_blank_lines(s.trim())
}

/// Collapse 3+ consecutive newlines to exactly 2, preserving paragraph breaks.
fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut newline_run = 0;
    for ch in s.chars() {
        if ch == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                out.push(ch);
            }
        } else {
            newline_run = 0;
            out.push(ch);
        }
    }
    out
}

/// Incremental sanitizer for streamed tokens. Feed raw pieces with [`push`];
/// it returns only text that is safe to display now, holding back anything that
/// could be the start of a control token or `<think>` tag until it can decide.
///
/// [`push`]: StreamSanitizer::push
#[derive(Default)]
pub struct StreamSanitizer {
    in_think: bool,
    carry: String,
}

impl StreamSanitizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a raw piece; return text safe to emit to the UI now.
    pub fn push(&mut self, piece: &str) -> String {
        self.carry.push_str(piece);
        let mut out = String::new();

        loop {
            if self.in_think {
                if let Some(idx) = self.carry.find("</think>") {
                    self.carry.replace_range(..idx + "</think>".len(), "");
                    self.in_think = false;
                    continue;
                }
                // Still thinking: drop all but a tail that might start "</think>".
                let keep = max_marker_prefix_suffix(&self.carry).max(
                    // also keep a possible partial "</think>"
                    partial_suffix_len(&self.carry, "</think>"),
                );
                let cut = self.carry.len() - keep;
                self.carry.drain(..cut);
                break;
            }

            // Not thinking: find earliest full marker.
            match earliest_marker(&self.carry) {
                Some((idx, marker)) => {
                    out.push_str(&self.carry[..idx]);
                    self.carry.drain(..idx + marker.len());
                    if marker == THINK_OPEN {
                        self.in_think = true;
                    }
                    continue;
                }
                None => {
                    // Emit everything except a suffix that could be a partial marker.
                    let hold = max_marker_prefix_suffix(&self.carry);
                    let cut = self.carry.len() - hold;
                    out.push_str(&self.carry[..cut]);
                    self.carry.drain(..cut);
                    break;
                }
            }
        }

        out
    }

    /// Flush remaining buffered text at end of stream.
    pub fn flush(&mut self) -> String {
        if self.in_think {
            // Dangling think block: drop it entirely.
            self.carry.clear();
            return String::new();
        }
        let rest = std::mem::take(&mut self.carry);
        rest
    }
}

/// Index and marker of the earliest full marker occurrence in `s`.
fn earliest_marker(s: &str) -> Option<(usize, &'static str)> {
    let mut best: Option<(usize, &'static str)> = None;
    for m in all_markers() {
        if let Some(idx) = s.find(m) {
            match best {
                Some((b, _)) if b <= idx => {}
                _ => best = Some((idx, m)),
            }
        }
    }
    best
}

/// Longest `k` such that the last `k` bytes of `s` equal the first `k` bytes of
/// some marker (a partial marker we must hold back). `k < marker.len()`.
fn max_marker_prefix_suffix(s: &str) -> usize {
    let mut best = 0;
    for m in all_markers() {
        best = best.max(partial_suffix_len(s, m));
    }
    best
}

/// Longest proper prefix of `marker` that is a suffix of `s`.
fn partial_suffix_len(s: &str, marker: &str) -> usize {
    let max_k = marker.len().saturating_sub(1).min(s.len());
    for k in (1..=max_k).rev() {
        if s.is_char_boundary(s.len() - k) && s[s.len() - k..] == marker[..k] {
            return k;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream_all(chunks: &[&str]) -> String {
        let mut s = StreamSanitizer::new();
        let mut out = String::new();
        for c in chunks {
            out.push_str(&s.push(c));
        }
        out.push_str(&s.flush());
        out
    }

    #[test]
    fn final_strips_think_block() {
        let raw = "<think>Let me reason about this.</think>The answer is **42**.";
        assert_eq!(sanitize_final(raw), "The answer is **42**.");
    }

    #[test]
    fn final_preserves_markdown() {
        let raw = "Here:\n\n- one\n- two\n\n**bold** and `code`";
        assert_eq!(sanitize_final(raw), "Here:\n\n- one\n- two\n\n**bold** and `code`");
    }

    #[test]
    fn final_strips_control_tokens() {
        let raw = "<|im_start|>assistant\nHi<|im_end|>";
        assert_eq!(sanitize_final(raw), "assistant\nHi");
    }

    #[test]
    fn final_drops_unclosed_think() {
        let raw = "Answer first.<think>and then I ramble forever";
        assert_eq!(sanitize_final(raw), "Answer first.");
    }

    #[test]
    fn final_collapses_blank_lines() {
        assert_eq!(sanitize_final("a\n\n\n\nb"), "a\n\nb");
    }

    #[test]
    fn stream_matches_final_simple() {
        let raw = "<think>reasoning here</think>The **answer**.";
        // Any chunking must equal sanitize_final of the whole.
        assert_eq!(stream_all(&["<think>reasoning here</think>The **answer**."]), sanitize_final(raw));
    }

    #[test]
    fn stream_matches_final_across_tag_boundaries() {
        // Split the think tags across many tiny chunks, the worst case.
        let chunks = ["Be", "fore", "<th", "ink>", "hid", "den", "</thi", "nk>", "Af", "ter"];
        let raw: String = chunks.concat();
        assert_eq!(stream_all(&chunks), sanitize_final(&raw));
        assert_eq!(stream_all(&chunks), "BeforeAfter");
    }

    #[test]
    fn stream_holds_back_partial_control_token() {
        // "<|im_" arrives, then completes to "<|im_end|>" — must never leak.
        let chunks = ["Hello<|im_", "end|> world"];
        let raw: String = chunks.concat();
        assert_eq!(stream_all(&chunks), sanitize_final(&raw));
        assert_eq!(stream_all(&chunks), "Hello world");
    }

    #[test]
    fn stream_does_not_hold_normal_angle_bracket() {
        let chunks = ["a < b and c > d"];
        assert_eq!(stream_all(&chunks), "a < b and c > d");
    }

    #[test]
    fn stream_token_by_token_think() {
        let raw = "<think>x</think>Y";
        let chunks: Vec<String> = raw.chars().map(|c| c.to_string()).collect();
        let refs: Vec<&str> = chunks.iter().map(|s| s.as_str()).collect();
        assert_eq!(stream_all(&refs), "Y");
    }
}
