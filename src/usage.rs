use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Serialize)]
pub struct UsageTokens {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub source: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageProtocol {
    OpenAi,
    Anthropic,
    Codex,
    Gemini,
}

impl UsageProtocol {
    pub fn from_app_type(app_type: &str, upstream_path: &str) -> Self {
        if upstream_path.contains("/responses") {
            return Self::Codex;
        }
        match app_type {
            "anthropic" | "claude" => Self::Anthropic,
            "gemini" => Self::Gemini,
            "codex" => Self::Codex,
            _ => Self::OpenAi,
        }
    }

    fn source(self) -> &'static str {
        match self {
            Self::OpenAi => "market_openai_usage",
            Self::Anthropic => "market_anthropic_usage",
            Self::Codex => "market_codex_usage",
            Self::Gemini => "market_gemini_usage",
        }
    }

    fn stream_source(self) -> &'static str {
        match self {
            Self::OpenAi => "market_openai_stream_usage",
            Self::Anthropic => "market_anthropic_stream_usage",
            Self::Codex => "market_codex_stream_usage",
            Self::Gemini => "market_gemini_stream_usage",
        }
    }
}

#[derive(Default, Clone, Copy)]
struct UsageParts {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
}

impl UsageParts {
    fn has_usage(self) -> bool {
        self.input_tokens > 0
            || self.output_tokens > 0
            || self.cache_read_tokens > 0
            || self.cache_write_tokens > 0
    }

    fn into_tokens(self, source: &'static str) -> UsageTokens {
        UsageTokens {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_read_tokens: self.cache_read_tokens,
            cache_write_tokens: self.cache_write_tokens,
            source,
        }
    }
}

pub fn extract_response_usage(value: &Value, protocol: UsageProtocol) -> Option<UsageTokens> {
    let usage = match protocol {
        UsageProtocol::OpenAi => openai_usage(value),
        UsageProtocol::Anthropic => anthropic_usage(value),
        UsageProtocol::Codex => codex_usage_auto(value),
        UsageProtocol::Gemini => gemini_usage(value),
    }?;
    Some(usage.into_tokens(protocol.source()))
}

pub struct SseUsageParser {
    protocol: UsageProtocol,
    buffer: String,
    utf8_remainder: Vec<u8>,
    usage: Option<UsageTokens>,
    complete_usage_seen: bool,
    saw_done: bool,
    parse_errors: u64,
}

impl SseUsageParser {
    pub fn new(protocol: UsageProtocol) -> Self {
        Self {
            protocol,
            buffer: String::new(),
            utf8_remainder: Vec::new(),
            usage: None,
            complete_usage_seen: false,
            saw_done: false,
            parse_errors: 0,
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) {
        append_utf8_safe(&mut self.buffer, &mut self.utf8_remainder, chunk);
        while let Some(block) = take_sse_block(&mut self.buffer) {
            self.feed_block(&block);
        }
    }

    pub fn finish(&mut self) {
        if !self.utf8_remainder.is_empty() {
            self.buffer
                .push_str(&String::from_utf8_lossy(&self.utf8_remainder));
            self.utf8_remainder.clear();
        }
        if !self.buffer.trim().is_empty() {
            let block = std::mem::take(&mut self.buffer);
            self.feed_block(&block);
        }
    }

    pub fn usage(&self) -> Option<UsageTokens> {
        if self.protocol == UsageProtocol::Anthropic && !self.complete_usage_seen {
            return None;
        }
        self.usage
    }

    pub fn saw_done(&self) -> bool {
        self.saw_done
    }

    pub fn audit_flags(&self) -> Value {
        let mut flags = vec!["stream_started".to_string()];
        flags.push(format!("usage_protocol_{:?}", self.protocol).to_ascii_lowercase());
        if self.saw_done {
            flags.push("stream_done_seen".to_string());
        }
        if self.parse_errors > 0 {
            flags.push("stream_usage_parse_error".to_string());
        }
        if self.usage().is_none() {
            flags.push("stream_usage_missing".to_string());
        }
        serde_json::json!(flags)
    }

    fn feed_block(&mut self, block: &str) {
        let mut data_lines = Vec::new();
        for line in block.lines() {
            let line = line.trim_end_matches('\r');
            if let Some(data) = strip_sse_field(line, "data") {
                data_lines.push(data.to_string());
            }
        }
        if data_lines.is_empty() {
            return;
        }
        let data = data_lines.join("\n");
        let data = data.trim();
        if data == "[DONE]" {
            self.saw_done = true;
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            self.parse_errors += 1;
            return;
        };
        let usage = match self.protocol {
            UsageProtocol::OpenAi => openai_usage(&value),
            UsageProtocol::Anthropic => anthropic_stream_usage_event(&value),
            UsageProtocol::Codex => codex_stream_usage_event(&value),
            UsageProtocol::Gemini => gemini_usage(&value),
        };
        if let Some(usage) = usage {
            if self.protocol != UsageProtocol::Anthropic
                || anthropic_stream_event_has_output(&value)
            {
                self.complete_usage_seen = true;
            }
            let incoming = usage.into_tokens(self.protocol.stream_source());
            self.usage = Some(match (self.protocol, self.usage) {
                (UsageProtocol::Anthropic, Some(existing)) => UsageTokens {
                    input_tokens: incoming.input_tokens.max(existing.input_tokens),
                    output_tokens: incoming.output_tokens.max(existing.output_tokens),
                    cache_read_tokens: incoming.cache_read_tokens.max(existing.cache_read_tokens),
                    cache_write_tokens: incoming
                        .cache_write_tokens
                        .max(existing.cache_write_tokens),
                    source: incoming.source,
                },
                _ => incoming,
            });
        }
    }
}

fn openai_usage(value: &Value) -> Option<UsageParts> {
    let usage = value.get("usage")?;
    if usage.is_null() {
        return None;
    }
    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(Value::as_u64)?;
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(Value::as_u64)?;
    let cache_read_tokens = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .or_else(|| usage.get("cache_read_input_tokens"))
        .or_else(|| usage.get("prompt_cache_hit_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_write_tokens = usage
        .get("cache_creation_input_tokens")
        .or_else(|| usage.get("prompt_cache_miss_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(UsageParts {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
    })
}

fn anthropic_usage(value: &Value) -> Option<UsageParts> {
    let usage = value.get("usage")?;
    anthropic_usage_object(usage)
}

fn anthropic_stream_usage_event(value: &Value) -> Option<UsageParts> {
    match value.get("type").and_then(Value::as_str) {
        Some("message_start") => value
            .pointer("/message/usage")
            .and_then(anthropic_usage_object),
        Some("message_delta") => value.get("usage").and_then(anthropic_usage_object),
        _ => None,
    }
}

fn anthropic_stream_event_has_output(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("message_delta")
        && value.pointer("/usage/output_tokens").is_some()
}

fn anthropic_usage_object(usage: &Value) -> Option<UsageParts> {
    if usage.is_null() {
        return None;
    }
    let parts = UsageParts {
        input_tokens: usage
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: usage
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_read_tokens: usage
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_write_tokens: usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    };
    parts.has_usage().then_some(parts)
}

fn codex_usage_auto(value: &Value) -> Option<UsageParts> {
    let usage = value.get("usage")?;
    if usage.get("prompt_tokens").is_some() {
        return openai_usage(value);
    }
    if usage.get("input_tokens").is_some() {
        return codex_usage(value);
    }
    None
}

fn codex_stream_usage_event(value: &Value) -> Option<UsageParts> {
    if value.get("type").and_then(Value::as_str) == Some("response.completed") {
        if let Some(response) = value.get("response") {
            return codex_usage_auto(response);
        }
    }
    if value.get("usage").is_some() {
        return codex_usage_auto(value);
    }
    openai_usage(value)
}

fn codex_usage(value: &Value) -> Option<UsageParts> {
    let usage = value.get("usage")?;
    let input_tokens = usage.get("input_tokens").and_then(Value::as_u64)?;
    let output_tokens = usage.get("output_tokens").and_then(Value::as_u64)?;
    let cache_read_tokens = usage
        .get("cache_read_input_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_write_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(UsageParts {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
    })
}

fn gemini_usage(value: &Value) -> Option<UsageParts> {
    let usage = value.get("usageMetadata")?;
    let input_tokens = usage.get("promptTokenCount").and_then(Value::as_u64)?;
    let total_tokens = usage.get("totalTokenCount").and_then(Value::as_u64)?;
    Some(UsageParts {
        input_tokens,
        output_tokens: total_tokens.saturating_sub(input_tokens),
        cache_read_tokens: usage
            .get("cachedContentTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_write_tokens: 0,
    })
}

fn strip_sse_field<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    line.strip_prefix(&format!("{field}: "))
        .or_else(|| line.strip_prefix(&format!("{field}:")))
}

fn take_sse_block(buffer: &mut String) -> Option<String> {
    let mut best: Option<(usize, usize)> = None;
    for (delimiter, len) in [("\r\n\r\n", 4usize), ("\n\n", 2usize)] {
        if let Some(pos) = buffer.find(delimiter) {
            if best.is_none_or(|(best_pos, _)| pos < best_pos) {
                best = Some((pos, len));
            }
        }
    }
    let (pos, len) = best?;
    let block = buffer[..pos].to_string();
    buffer.drain(..pos + len);
    Some(block)
}

fn append_utf8_safe(buffer: &mut String, remainder: &mut Vec<u8>, new_bytes: &[u8]) {
    let combined;
    let input = if remainder.is_empty() {
        new_bytes
    } else {
        combined = [remainder.as_slice(), new_bytes].concat();
        remainder.clear();
        combined.as_slice()
    };

    let mut pos = 0;
    loop {
        match std::str::from_utf8(&input[pos..]) {
            Ok(s) => {
                buffer.push_str(s);
                return;
            }
            Err(err) => {
                let valid_up_to = pos + err.valid_up_to();
                if valid_up_to > pos {
                    buffer.push_str(std::str::from_utf8(&input[pos..valid_up_to]).unwrap_or(""));
                }
                if let Some(invalid_len) = err.error_len() {
                    buffer.push('\u{FFFD}');
                    pos = valid_up_to + invalid_len;
                } else {
                    *remainder = input[valid_up_to..].to_vec();
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_openai_stream_usage_split_utf8_safely() {
        let mut parser = SseUsageParser::new(UsageProtocol::OpenAi);
        parser.feed("data: {\"choices\":[{\"delta\":{\"content\":\"你".as_bytes());
        parser.feed(
            "好\"}}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":20,\"prompt_tokens_details\":{\"cached_tokens\":3}}}\n\ndata: [DONE]\n\n"
                .as_bytes(),
        );
        let usage = parser.usage().expect("usage");
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.cache_read_tokens, 3);
        assert!(parser.saw_done());
    }

    #[test]
    fn parses_anthropic_stream_usage_from_delta() {
        let mut parser = SseUsageParser::new(UsageProtocol::Anthropic);
        parser.feed(
            br#"event: message_start
data: {"type":"message_start","message":{"usage":{"input_tokens":100,"cache_read_input_tokens":20}}}

event: message_delta
data: {"type":"message_delta","usage":{"output_tokens":50}}

"#,
        );
        let usage = parser.usage().expect("usage");
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.source, "market_anthropic_stream_usage");
    }

    #[test]
    fn anthropic_stream_start_usage_alone_is_not_complete() {
        let mut parser = SseUsageParser::new(UsageProtocol::Anthropic);
        parser.feed(
            br#"event: message_start
data: {"type":"message_start","message":{"usage":{"input_tokens":100}}}

"#,
        );
        assert!(parser.usage().is_none());
        assert!(
            parser
                .audit_flags()
                .as_array()
                .unwrap()
                .iter()
                .any(|flag| flag.as_str() == Some("stream_usage_missing"))
        );
    }

    #[test]
    fn parses_response_usage_by_protocol() {
        let usage = extract_response_usage(
            &json!({"usage":{"input_tokens":7,"output_tokens":9,"cache_creation_input_tokens":2}}),
            UsageProtocol::Anthropic,
        )
        .expect("usage");
        assert_eq!(usage.input_tokens, 7);
        assert_eq!(usage.output_tokens, 9);
        assert_eq!(usage.cache_write_tokens, 2);
    }
}
