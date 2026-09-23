use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use crate::domain::chat::{ChatMessage, ChatRole, MessageKind};

use super::contract::LlmError;
use super::types::{ContentBlock, DocumentSource, HistoryEntry, ImageSource, Message};

// ═══════════════════════════════════════════════════════════════════════════
// Rate limit retry
// ═══════════════════════════════════════════════════════════════════════════

const MAX_RETRIES: u32 = 3;
const INITIAL_BACKOFF_MS: u64 = 2000;
/// A provider's `Retry-After` is honoured up to this; longer waits fall
/// back to the exponential backoff.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

/// The wait a typed rate limit asks for (`None` inside when the provider
/// named none), or `None` for any other error. Only the adapter's variant
/// counts; the text of an error never does.
fn rate_limit_wait(error: &anyhow::Error) -> Option<Option<Duration>> {
    match error.downcast_ref::<LlmError>() {
        Some(LlmError::RateLimited { retry_after, .. }) => Some(*retry_after),
        _ => None,
    }
}

pub(crate) async fn retry_on_rate_limit<F, Fut, T>(f: F) -> anyhow::Result<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<T>>,
{
    let mut attempt = 0;
    loop {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => match rate_limit_wait(&e) {
                Some(retry_after) if attempt < MAX_RETRIES => {
                    attempt += 1;
                    let backoff = Duration::from_millis(INITIAL_BACKOFF_MS * 2u64.pow(attempt - 1));
                    let delay = retry_after
                        .filter(|wait| *wait <= MAX_RETRY_AFTER)
                        .unwrap_or(backoff);
                    log::warn!(
                        "Rate limited, retrying in {}ms (attempt {attempt}/{MAX_RETRIES})",
                        delay.as_millis()
                    );
                    tokio::time::sleep(delay).await;
                }
                _ => return Err(e),
            },
        }
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn rate_limited(retry_after: Option<Duration>) -> anyhow::Error {
        LlmError::RateLimited {
            retry_after,
            message: "slow down".into(),
        }
        .into()
    }

    /// Only the typed variant is retried, and a `Retry-After` the provider
    /// sent replaces the default backoff.
    #[tokio::test(start_paused = true)]
    async fn retries_follow_the_typed_rate_limit_and_its_retry_after() {
        let attempts = AtomicU32::new(0);
        let started = tokio::time::Instant::now();
        let result = retry_on_rate_limit(|| async {
            let attempt = attempts.fetch_add(1, Ordering::SeqCst);
            if attempt < 2 {
                Err(rate_limited(Some(Duration::from_secs(9))))
            } else {
                Ok(attempt)
            }
        })
        .await
        .unwrap();
        assert_eq!(result, 2);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert_eq!(started.elapsed(), Duration::from_secs(18), "two 9s waits");

        // Without a Retry-After the backoff doubles from two seconds.
        let attempts = AtomicU32::new(0);
        let started = tokio::time::Instant::now();
        retry_on_rate_limit(|| async {
            if attempts.fetch_add(1, Ordering::SeqCst) < 2 {
                Err(rate_limited(None))
            } else {
                Ok(())
            }
        })
        .await
        .unwrap();
        assert_eq!(started.elapsed(), Duration::from_secs(6), "2s then 4s");

        // A provider that never lets up: the typed error comes back after the retries.
        let attempts = AtomicU32::new(0);
        let error = retry_on_rate_limit(|| async {
            attempts.fetch_add(1, Ordering::SeqCst);
            Err::<(), _>(rate_limited(None))
        })
        .await
        .unwrap_err();
        assert_eq!(attempts.load(Ordering::SeqCst), 1 + MAX_RETRIES);
        assert!(matches!(
            error.downcast_ref::<LlmError>(),
            Some(LlmError::RateLimited { .. })
        ));

        // Anything else is not retried, however much its text resembles a rate limit.
        let others: Vec<Box<dyn Fn() -> anyhow::Error>> = vec![
            Box::new(|| anyhow::anyhow!("429 Too Many Requests rate_limit overloaded 529")),
            Box::new(|| {
                LlmError::Http {
                    status: 500,
                    message: "upstream returned 429".into(),
                }
                .into()
            }),
            Box::new(|| LlmError::Authentication("bad key".into()).into()),
            Box::new(|| LlmError::ContextLength("too long".into()).into()),
        ];
        for make in others {
            let attempts = AtomicU32::new(0);
            let error = retry_on_rate_limit(|| async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(make())
            })
            .await
            .unwrap_err();
            assert_eq!(attempts.load(Ordering::SeqCst), 1, "{error}");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Real input token cache — populated from Anthropic API responses
// ═══════════════════════════════════════════════════════════════════════════

static REAL_INPUT_TOKENS: Mutex<Option<std::collections::HashMap<String, u64>>> = Mutex::new(None);

/// Cache the real input token count from an Anthropic API response.
pub(crate) fn cache_real_input_tokens(instance_slug: &str, chat_id: &str, tokens: u64) {
    let key = format!("{instance_slug}/{chat_id}");
    let mut guard = REAL_INPUT_TOKENS.lock().unwrap();
    guard
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(key, tokens);
}

/// Retrieve the last real input token count for a given instance/chat.
pub fn get_real_input_tokens(instance_slug: &str, chat_id: &str) -> Option<u64> {
    let key = format!("{instance_slug}/{chat_id}");
    REAL_INPUT_TOKENS
        .lock()
        .unwrap()
        .as_ref()?
        .get(&key)
        .copied()
}

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

pub const DEFAULT_ONBOARDING_PROMPT: &str = "\
you are a quiet, thoughtful companion. you speak in lowercase, keep your \
responses short and gentle — one or two sentences at most. you listen more \
than you speak. you're warm but not overbearing. this is a safe, intimate space.";

/// Short summary of a tool use for display.
pub(crate) fn tool_use_summary(name: &str, input: &serde_json::Value) -> String {
    // The machine tools read as the live trail words them (#80): naming the
    // computer the arguments name, by id (there is no listing on reload).
    // The typed window tools (#18) join the list; `computer_use` stays until
    // #19 deletes the type, since reloaded histories still carry those calls.
    if matches!(
        name,
        "discover_windows"
            | "get_window_state"
            | "act"
            | "verify_state"
            | "computer_use"
            | "remote_bash"
            | "remote_files"
            | "list_machines"
    ) {
        return crate::services::tools::tool_summary(name, &input.to_string());
    }
    // Extract first meaningful field value for a one-line summary
    if let Some(obj) = input.as_object() {
        if obj.contains_key("command") {
            return format!("{name}: command");
        }
        if obj.contains_key("url") {
            return format!("{name}: URL");
        }
        for key in &["query", "path", "content", "name", "message"] {
            if let Some(val) = obj.get(*key) {
                let owned = val.to_string();
                let s = val.as_str().unwrap_or(&owned);
                let truncated = if s.len() > 80 {
                    let end = s.floor_char_boundary(80);
                    format!("{}…", &s[..end])
                } else {
                    s.to_string()
                };
                return format!("{name}: {truncated}");
            }
        }
    }
    name.to_string()
}

/// Whether a user-message block is one a turn adds around what the person
/// wrote: the turn context (`[turn context …]`) or the recalled memories
/// (`[system: auto-recalled memories …]`). Providers replay those as they
/// were sent; this is for retelling a conversation as who said what.
pub(crate) fn is_context_block(block: &ContentBlock) -> bool {
    matches!(block, ContentBlock::Text { text }
        if text.starts_with("[turn context") || text.starts_with("[system: auto-recalled"))
}

/// Convert HistoryEntry slice to ChatMessage vec for UI display.
pub fn history_to_chat_messages(entries: &[HistoryEntry]) -> Vec<ChatMessage> {
    let mut out = Vec::new();
    let mut counter = 0u64;
    let mut seen_ids = std::collections::HashSet::new();

    for entry in entries {
        let ts = entry.ts.clone().unwrap_or_else(|| "0".to_string());
        let base_id = entry.id.clone().unwrap_or_else(|| {
            counter += 1;
            format!("h_{counter}")
        });

        let (role, blocks) = match &entry.message {
            Message::User { content } => (ChatRole::User, content),
            Message::Assistant { content } => (ChatRole::Assistant, content),
        };

        let mut block_idx = 0u32;
        for block in blocks {
            let mut block_id = if block_idx == 0 {
                base_id.clone()
            } else {
                format!("{base_id}_{block_idx}")
            };
            block_idx += 1;

            // Ensure uniqueness — append suffix if ID was already emitted
            if !seen_ids.insert(block_id.clone()) {
                let mut dedup = 2u32;
                loop {
                    let candidate = format!("{block_id}_d{dedup}");
                    if seen_ids.insert(candidate.clone()) {
                        block_id = candidate;
                        break;
                    }
                    dedup += 1;
                }
            }

            match block {
                ContentBlock::Text { text } => {
                    if text.is_empty() {
                        continue;
                    }
                    out.push(ChatMessage {
                        id: block_id,
                        role: role.clone(),
                        content: text.clone(),
                        created_at: ts.clone(),
                        kind: MessageKind::Message,
                        tool_name: None,
                        mcp_app_html: None,
                        mcp_app_input: None,
                        model: if role == ChatRole::Assistant {
                            entry.model.clone()
                        } else {
                            None
                        },
                    });
                }
                ContentBlock::ToolCall {
                    id,
                    name,
                    arguments: input,
                } => {
                    // The line the tool announced when it ran (#80: naming the
                    // computer a desktop tool acted on), else the arguments.
                    let summary = entry
                        .tool_trail
                        .as_ref()
                        .and_then(|trail| trail.get(id))
                        .cloned()
                        .unwrap_or_else(|| tool_use_summary(name, input));
                    out.push(ChatMessage {
                        id: block_id,
                        role: ChatRole::Assistant,
                        content: summary,
                        created_at: ts.clone(),
                        kind: MessageKind::ToolCall,
                        tool_name: Some(name.clone()),
                        mcp_app_html: entry.mcp_app_html.clone(),
                        mcp_app_input: entry.mcp_app_input.clone(),
                        model: None,
                    });
                }
                ContentBlock::ToolOutput { content, .. } => {
                    let text = match content {
                        super::types::ToolOutputContent::Text(s) => s.clone(),
                        other => other.to_string(),
                    };
                    out.push(ChatMessage {
                        id: block_id,
                        role: ChatRole::Assistant,
                        content: text,
                        created_at: ts.clone(),
                        kind: MessageKind::ToolOutput,
                        tool_name: None,
                        mcp_app_html: None,
                        mcp_app_input: None,
                        model: None,
                    });
                }
                ContentBlock::ContextSummary { content }
                | ContentBlock::LegacyContextSummary {
                    summary: content, ..
                } => {
                    out.push(ChatMessage {
                        id: block_id,
                        role: ChatRole::Assistant,
                        content: content.clone(),
                        created_at: ts.clone(),
                        kind: MessageKind::Compaction,
                        tool_name: None,
                        mcp_app_html: None,
                        mcp_app_input: None,
                        model: None,
                    });
                }
                ContentBlock::Unknown(val) | ContentBlock::ProviderData { data: val, .. } => {
                    // Server tool blocks (web_search, code_execution) — render like regular tools
                    let block_type = val["type"].as_str().unwrap_or("");
                    if block_type == "server_tool_use" {
                        let tool_name = val["name"].as_str().unwrap_or("server_tool");
                        let summary = match tool_name {
                            "web_search" => {
                                let q = val["input"]["query"].as_str().unwrap_or("");
                                if q.is_empty() {
                                    "searching the web".into()
                                } else {
                                    format!("web search: {q}")
                                }
                            }
                            "web_fetch" => {
                                let u = val["input"]["url"].as_str().unwrap_or("");
                                if u.is_empty() {
                                    "fetching web page".into()
                                } else {
                                    format!("fetching {u}")
                                }
                            }
                            "bash_code_execution" | "code_execution" => {
                                "executing code".to_string()
                            }
                            "text_editor_code_execution" => "editing file".to_string(),
                            other => format!("{other}"),
                        };
                        out.push(ChatMessage {
                            id: block_id,
                            role: ChatRole::Assistant,
                            content: summary,
                            created_at: ts.clone(),
                            kind: MessageKind::ToolCall,
                            tool_name: Some(tool_name.to_string()),
                            mcp_app_html: None,
                            mcp_app_input: None,
                            model: None,
                        });
                    } else if block_type.ends_with("_tool_result") {
                        let mut output = String::new();

                        // Web search results — show titles and URLs
                        if block_type == "web_search_tool_result" {
                            if let Some(results) = val["content"].as_array() {
                                for r in results {
                                    let title = r["title"].as_str().unwrap_or("");
                                    let url = r["url"].as_str().unwrap_or("");
                                    if !title.is_empty() {
                                        output.push_str(&format!("- {title}"));
                                        if !url.is_empty() {
                                            output.push_str(&format!(" ({url})"));
                                        }
                                        output.push('\n');
                                    }
                                }
                            }
                        }

                        // Code execution results — show stdout/stderr
                        if output.is_empty() {
                            let stdout = val["content"]["stdout"].as_str().unwrap_or("");
                            let stderr = val["content"]["stderr"].as_str().unwrap_or("");
                            if !stdout.is_empty() {
                                output.push_str(stdout);
                            }
                            if !stderr.is_empty() {
                                if !output.is_empty() {
                                    output.push('\n');
                                }
                                output.push_str(stderr);
                            }
                        }

                        if output.is_empty() {
                            // Skip empty results entirely (encrypted results, etc.)
                            continue;
                        }

                        let truncated: String = output.chars().take(2000).collect();
                        out.push(ChatMessage {
                            id: block_id,
                            role: ChatRole::Assistant,
                            content: truncated,
                            created_at: ts.clone(),
                            kind: MessageKind::ToolOutput,
                            tool_name: None,
                            mcp_app_html: None,
                            mcp_app_input: None,
                            model: None,
                        });
                    }
                    // Other unknown blocks (container_upload, etc.) — skip
                }
                // Image, Document — skip for UI
                _ => {}
            }
        }
    }
    out
}

/// Refresh only explicitly persisted local resource identities before a new turn.
/// URL-shaped strings, including legacy unsigned paths, carry no authority.
pub fn refresh_resource_messages(
    messages: &mut [Message],
    base: &str,
    slug: &str,
    resources: &crate::services::resource_access::ResourceAccess,
) {
    fn renew_block(
        block: &mut ContentBlock,
        base: &str,
        slug: &str,
        resources: &crate::services::resource_access::ResourceAccess,
    ) {
        let (url, provenance) = match block {
            ContentBlock::Image {
                source: ImageSource::Url { url },
                resource_provenance,
            }
            | ContentBlock::Document {
                source: DocumentSource::Url { url },
                resource_provenance,
            } => (url, resource_provenance),
            ContentBlock::ToolOutput {
                content: super::types::ToolOutputContent::Blocks(blocks),
                ..
            } => {
                for block in blocks {
                    renew_block(block, base, slug, resources);
                }
                return;
            }
            _ => return,
        };
        let Some(resource) = provenance
            .as_ref()
            .and_then(|value| value.capability_resource(slug))
        else {
            return;
        };
        if let Ok(fresh) = resources.url(
            base,
            slug,
            resource,
            crate::services::resource_capability::CapabilityAudience::ModelProvider,
        ) {
            *url = fresh;
        }
    }

    for message in messages {
        let blocks = match message {
            Message::User { content } | Message::Assistant { content } => content,
        };
        for block in blocks {
            renew_block(block, base, slug, resources);
        }
    }
}

/// Anthropic accepts PDF documents up to 32 MB; the same bound as `read_file`.
const MAX_INLINE_PDF_BYTES: usize = 32 * 1024 * 1024;

/// Inline an uploaded image or PDF as a base64 content block when the provider
/// cannot fetch it by URL. Returns a text placeholder when the blob is missing
/// or too large for an inline block.
fn inline_upload_block(
    kind: &str,
    name: &str,
    instance_slug: &str,
    upload_id: &str,
    max_bytes: usize,
    media_store: &crate::services::media_text::MediaStore,
) -> ContentBlock {
    use base64::Engine;
    match media_store.read_upload_bounded(instance_slug, upload_id, max_bytes) {
        Ok((meta, bytes)) => {
            let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
            log::info!(
                "attached {kind} (inline base64): {name} ({} bytes)",
                bytes.len()
            );
            if kind == "image" {
                ContentBlock::Image {
                    source: ImageSource::Base64 {
                        media_type: meta.mime_type,
                        data,
                    },
                    resource_provenance: None,
                }
            } else {
                ContentBlock::Document {
                    source: DocumentSource::Base64 {
                        media_type: meta.mime_type,
                        data,
                    },
                    resource_provenance: None,
                }
            }
        }
        Err(error) => {
            log::warn!("{kind} {name}: cannot inline for the provider: {error}");
            ContentBlock::text(format!(
                "[{kind}: {name} — not sent to the model: {error}; \
                 set public_url to a provider-reachable address to attach it by URL]"
            ))
        }
    }
}

/// `[attached: name (upload_id)]`, as the composer writes an attachment into
/// the message.
static ATTACHMENT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"\[attached:\s*(.+?)\s*\(([^)]+)\)\]").expect("attachment pattern")
});

/// Whether `text` names an attachment, which [`build_multimodal_prompt`]
/// sends as the file itself.
pub fn has_attachments(text: &str) -> bool {
    ATTACHMENT.is_match(text)
}

/// Build a multimodal Message from text + file attachments.
/// Files are referenced via public URL so the LLM provider can fetch them directly.
/// When the public URL is unset or points at this machine (localhost, loopback),
/// images and PDFs are inlined as base64 instead, and text files are always inline.
pub fn build_multimodal_prompt(
    text: &str,
    workspace_dir: &Path,
    instance_slug: &str,
    public_url: &str,
    resources: &crate::services::resource_access::ResourceAccess,
    media_store: &crate::services::media_text::MediaStore,
) -> Message {
    let re = &*ATTACHMENT;
    let provider_url = crate::config::provider_reachable_public_url(public_url);

    let mut contents: Vec<ContentBlock> = Vec::new();

    // Images first (with labels) — Claude performs best with images before text
    let caps: Vec<_> = re.captures_iter(text).collect();
    let num_images = caps
        .iter()
        .filter(|c| {
            let uid = &c[2];
            media_store
                .upload_descriptor(instance_slug, uid)
                .ok()
                .map(|m| m.mime_type.starts_with("image/"))
                .unwrap_or(false)
        })
        .count();
    let mut image_idx = 0;

    for cap in &caps {
        let name = &cap[1];
        let upload_id = &cap[2];

        let meta = match media_store.upload_descriptor(instance_slug, upload_id) {
            Ok(meta) => meta,
            Err(error) => {
                log::warn!("attachment {upload_id} is unavailable: {error}");
                continue;
            }
        };

        if meta.mime_type.starts_with("image/") {
            image_idx += 1;
            if num_images > 1 {
                contents.push(ContentBlock::text(&format!("Image {image_idx} ({name}):")));
            }
            if let Some(base) = provider_url {
                let url = crate::services::tools::public_file_url(
                    base,
                    instance_slug,
                    upload_id,
                    resources,
                );
                contents.push(ContentBlock::Image {
                    source: ImageSource::Url { url: url.clone() },
                    resource_provenance: Some(super::types::ResourceProvenance::uploaded_file(
                        instance_slug,
                        upload_id,
                    )),
                });
                log::info!("attached image (url): {name} ({url})");
            } else {
                contents.push(inline_upload_block(
                    "image",
                    name,
                    instance_slug,
                    upload_id,
                    super::MAX_INLINE_IMAGE_BYTES,
                    media_store,
                ));
            }
        } else if meta.mime_type == "application/pdf" {
            if let Some(base) = provider_url {
                let url = crate::services::tools::public_file_url(
                    base,
                    instance_slug,
                    upload_id,
                    resources,
                );
                contents.push(ContentBlock::Document {
                    source: DocumentSource::Url { url: url.clone() },
                    resource_provenance: Some(super::types::ResourceProvenance::uploaded_file(
                        instance_slug,
                        upload_id,
                    )),
                });
                log::info!("attached PDF (url): {name} ({url})");
            } else {
                contents.push(inline_upload_block(
                    "PDF",
                    name,
                    instance_slug,
                    upload_id,
                    MAX_INLINE_PDF_BYTES,
                    media_store,
                ));
            }
        } else if meta.mime_type.starts_with("text/") || meta.mime_type == "application/json" {
            let bytes = match media_store.read_upload_inline(instance_slug, upload_id) {
                Ok((_, bytes)) => bytes,
                Err(error) => {
                    log::warn!("failed to read bounded text attachment {upload_id}: {error}");
                    contents.push(ContentBlock::text(format!(
                        "[text file: {name} — unavailable for inline reading: {error}]"
                    )));
                    continue;
                }
            };
            let text_content = String::from_utf8_lossy(&bytes);
            let truncated: String = text_content.chars().take(10_000).collect();
            contents.push(ContentBlock::text(format!(
                "\n--- {name} ---\n{truncated}\n---"
            )));
            log::info!(
                "attached text file (inline): {name} ({} bytes)",
                bytes.len()
            );
        } else if meta.mime_type == "application/zip" {
            match crate::services::uploads::extract_zip(workspace_dir, instance_slug, upload_id) {
                Ok((extract_dir, files)) => {
                    let mut summary = format!(
                        "\n--- ZIP extracted: {name} ---\n\
                         path: {}\n\
                         {} files:\n",
                        extract_dir.display(),
                        files.len()
                    );
                    for (i, f) in files.iter().enumerate() {
                        if i >= 50 {
                            summary.push_str(&format!("... and {} more files\n", files.len() - 50));
                            break;
                        }
                        summary.push_str(&format!("  {f}\n"));
                    }
                    summary.push_str("---\nUse read_file, write_file, list_files, and run_command with the path above to work with this project.");
                    contents.push(ContentBlock::text(summary));
                    log::info!(
                        "extracted zip: {name} → {} ({} files)",
                        extract_dir.display(),
                        files.len()
                    );
                }
                Err(e) => {
                    contents.push(ContentBlock::text(format!(
                        "[zip: {name} — extraction failed: {e}]"
                    )));
                    log::warn!("failed to extract zip {name}: {e}");
                }
            }
        } else if meta.mime_type.starts_with("video/") || meta.mime_type.starts_with("audio/") {
            // Preserve attachment metadata. Video and audio stay ordinary files (#91).
            let kind = if meta.mime_type.starts_with("video/") {
                "video"
            } else {
                "audio"
            };
            let size_mb = meta.size as f64 / (1024.0 * 1024.0);
            let mime = &meta.mime_type;
            let description = format!("[{kind}: {name} — {mime}, {size_mb:.1} MB]");
            contents.push(ContentBlock::text(description));
            log::info!(
                "attached {kind}: {name} ({}, {size_mb:.1} MB)",
                meta.mime_type
            );
        } else {
            contents.push(ContentBlock::text(format!(
                "[file: {name} — {}, {} bytes, binary format]",
                meta.mime_type, meta.size
            )));
        }
    }

    // Text goes after images (Anthropic best practice: images before text)
    let clean_text = re.replace_all(text, "").trim().to_string();
    if !clean_text.is_empty() {
        contents.push(ContentBlock::text(&clean_text));
    }

    if contents.is_empty() {
        contents.push(ContentBlock::text(text));
    }

    Message::User { content: contents }
}

pub fn load_system_prompt(workspace_dir: &Path, instance_slug: &str) -> String {
    let soul = crate::services::soul::read_soul(workspace_dir, instance_slug);
    if soul.exists && !soul.content.trim().is_empty() {
        soul.content
    } else {
        DEFAULT_ONBOARDING_PROMPT.to_string()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_typed_attachment_provenance_is_renewed_for_the_provider() {
        let resources = crate::services::resource_access::ResourceAccess::new("control-secret");
        let mut messages = vec![super::Message::User { content: vec![
            super::ContentBlock::Image { source: super::ImageSource::Url { url: "https://self.test/resources/browser/files/moon/id?cap=expired".into() }, resource_provenance: None },
            super::ContentBlock::Document { source: super::DocumentSource::Url { url: "https://self.test/public/memory/moon/Folder/R%C3%A9sum%C3%A9.pdf?token=control-secret".into() }, resource_provenance: None },
            super::ContentBlock::Image { source: super::ImageSource::Url { url: "/resources/browser/files/moon/secret".into() }, resource_provenance: None },
            super::ContentBlock::Text { text: "arbitrary /resources/browser/files/moon/secret and /public/files/moon/secret".into() },
        ] }];
        super::refresh_resource_messages(&mut messages, "https://self.test", "moon", &resources);
        let text = serde_json::to_string(&messages).unwrap();
        assert!(text.contains("cap=expired"));
        assert!(text.contains("token=control-secret"));
        assert_eq!(
            text.matches("/resources/browser/files/moon/secret").count(),
            2
        );
        assert!(text.contains("/public/files/moon/secret"));
    }

    #[test]
    fn typed_tool_provenance_roundtrips_and_renews_across_generation_rotation() {
        let resources = crate::services::resource_access::ResourceAccess::new("old-token");
        let output = serde_json::json!([{
            "type": "image",
            "source": { "type": "url", "url": "stale" },
            "resource_provenance": {
                "kind": "uploaded_file",
                "version": 1,
                "slug": "moon",
                "id": "upload_1.png"
            }
        }]);
        let message = super::Message::User {
            content: vec![super::ContentBlock::tool_output(
                "tool-call-1".into(),
                output.to_string(),
                true,
            )],
        };
        let encoded = serde_json::to_string(&message).unwrap();
        assert!(!encoded.contains("old-token"));
        let mut messages = vec![serde_json::from_str(&encoded).unwrap()];
        super::refresh_resource_messages(&mut messages, "https://self.test", "moon", &resources);
        let first = serde_json::to_string(&messages).unwrap();
        resources.replace("new-token");
        super::refresh_resource_messages(&mut messages, "https://self.test", "moon", &resources);
        let second = serde_json::to_string(&messages).unwrap();
        assert_ne!(first, second);
        assert!(second.contains("resource_provenance"));
        assert!(second.contains("/resources/model-provider/files/moon/upload_1.png?cap="));
    }

    #[test]
    fn untrusted_tool_provenance_cannot_renew() {
        let resources = crate::services::resource_access::ResourceAccess::new("control-token");
        let output = serde_json::json!([{
            "type": "image",
            "source": { "type": "url", "url": "https://attacker.invalid/forged" },
            "resource_provenance": {
                "kind": "uploaded_file", "version": 1, "slug": "moon", "id": "guessed"
            }
        }]);
        let mut messages = vec![super::Message::User {
            content: vec![super::ContentBlock::tool_output(
                "malicious".into(),
                output.to_string(),
                false,
            )],
        }];

        super::refresh_resource_messages(&mut messages, "https://self.test", "moon", &resources);
        let encoded = serde_json::to_string(&messages).unwrap();
        assert!(encoded.contains("https://attacker.invalid/forged"));
        assert!(!encoded.contains("resource_provenance"));
        assert!(!encoded.contains("/resources/model-provider/files/moon/guessed"));
    }

    use super::{ContentBlock, Message, build_multimodal_prompt};
    use crate::services::uploads::save_upload;

    #[test]
    fn media_attachment_prompts_preserve_metadata_and_video_guidance() {
        let workspace =
            std::env::temp_dir().join(format!("nolune-media-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace).unwrap();
        let media_store = crate::services::media_text::MediaStore::open(&workspace).unwrap();
        for (name, mime, kind) in [
            ("voice.mp3", "audio/mpeg", "audio"),
            ("voice.wav", "audio/wav", "audio"),
            ("voice.ogg", "audio/ogg", "audio"),
            ("voice.m4a", "audio/mp4", "audio"),
            ("clip.mp4", "video/mp4", "video"),
        ] {
            let upload = save_upload(&workspace, "test", name, b"media fixture").unwrap();
            let message = build_multimodal_prompt(
                &format!("Please review [attached: {name} ({})]", upload.id),
                &workspace,
                "test",
                "",
                &crate::services::resource_access::ResourceAccess::new(""),
                &media_store,
            );
            let Message::User { content } = message else {
                panic!("expected user message")
            };
            let ContentBlock::Text { text } = &content[0] else {
                panic!("expected attachment metadata")
            };
            assert!(text.contains(&format!("[{kind}: {name} — {mime},")));
            assert!(!text.contains("local path:"));
            assert!(!text.contains("listen_music"));
            assert!(
                !text.contains("watch_video"),
                "attachments must not advertise the retired video tool"
            );
            let ContentBlock::Text { text } = content.last().unwrap() else {
                panic!("expected user text")
            };
            assert_eq!(text, "Please review");
            assert_eq!(
                media_store
                    .read_upload_inline("test", &upload.id)
                    .unwrap()
                    .1,
                b"media fixture"
            );
        }
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn local_public_url_inlines_images_and_pdfs_instead_of_unreachable_urls() {
        let workspace = tempfile::tempdir().unwrap();
        let png = b"\x89PNG\r\n\x1a\nfake";
        let image = save_upload(workspace.path(), "test", "photo.png", png).unwrap();
        let pdf = save_upload(workspace.path(), "test", "notes.pdf", b"%PDF-1.4 fake").unwrap();
        let media_store = crate::services::media_text::MediaStore::open(workspace.path()).unwrap();
        let resources = crate::services::resource_access::ResourceAccess::new("control-token");

        for public_url in ["http://localhost:26559", "http://127.0.0.1:26559", ""] {
            let message = build_multimodal_prompt(
                &format!(
                    "look [attached: photo.png ({})] [attached: notes.pdf ({})]",
                    image.id, pdf.id
                ),
                workspace.path(),
                "test",
                public_url,
                &resources,
                &media_store,
            );
            let Message::User { content } = &message else {
                panic!("expected user message")
            };
            let serialized = serde_json::to_string(&message).unwrap();
            assert!(
                !serialized.contains("localhost"),
                "{public_url}: {serialized}"
            );
            assert!(
                !serialized.contains("127.0.0.1"),
                "{public_url}: {serialized}"
            );
            assert!(
                !serialized.contains("no public URL configured"),
                "{public_url}: {serialized}"
            );
            let inline_image = content.iter().any(|block| {
                matches!(block, ContentBlock::Image {
                    source: super::ImageSource::Base64 { media_type, data },
                    resource_provenance: None,
                } if media_type == "image/png" && !data.is_empty())
            });
            assert!(inline_image, "{public_url}: {serialized}");
            let inline_pdf = content.iter().any(|block| {
                matches!(block, ContentBlock::Document {
                    source: super::DocumentSource::Base64 { media_type, data },
                    resource_provenance: None,
                } if media_type == "application/pdf" && !data.is_empty())
            });
            assert!(inline_pdf, "{public_url}: {serialized}");
            let ContentBlock::Text { text } = content.last().unwrap() else {
                panic!("expected user text")
            };
            assert_eq!(text, "look");
        }
    }

    #[test]
    fn oversized_image_on_a_local_public_url_becomes_a_placeholder() {
        let workspace = tempfile::tempdir().unwrap();
        let upload = save_upload(workspace.path(), "test", "huge.png", b"x").unwrap();
        let uploads = workspace.path().join("instances/test/uploads");
        let large_size = (super::super::MAX_INLINE_IMAGE_BYTES + 1) as u64;
        std::fs::OpenOptions::new()
            .write(true)
            .open(uploads.join(&upload.stored_name))
            .unwrap()
            .set_len(large_size)
            .unwrap();
        let metadata_path = uploads.join(format!("{}.json", upload.id));
        let mut metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&metadata_path).unwrap()).unwrap();
        metadata["size"] = serde_json::json!(large_size);
        std::fs::write(&metadata_path, serde_json::to_vec(&metadata).unwrap()).unwrap();
        let media_store = crate::services::media_text::MediaStore::open(workspace.path()).unwrap();

        let message = build_multimodal_prompt(
            &format!("[attached: huge.png ({})]", upload.id),
            workspace.path(),
            "test",
            "http://localhost:26559",
            &crate::services::resource_access::ResourceAccess::new("control-token"),
            &media_store,
        );
        let Message::User { content } = &message else {
            panic!("expected user message")
        };
        assert!(
            !content
                .iter()
                .any(|block| matches!(block, ContentBlock::Image { .. })),
            "{content:?}"
        );
        let ContentBlock::Text { text } = &content[0] else {
            panic!("expected placeholder")
        };
        assert!(
            text.contains("[image: huge.png — not sent to the model"),
            "{text}"
        );
        assert!(text.contains("public_url"), "{text}");
    }

    #[test]
    fn large_url_backed_image_uses_metadata_without_reading_blob() {
        let workspace = tempfile::tempdir().unwrap();
        let upload = save_upload(workspace.path(), "test", "large.png", b"x").unwrap();
        let uploads = workspace.path().join("instances/test/uploads");
        let large_size = 128_u64 * 1024 * 1024;
        std::fs::OpenOptions::new()
            .write(true)
            .open(uploads.join(&upload.stored_name))
            .unwrap()
            .set_len(large_size)
            .unwrap();
        let metadata_path = uploads.join(format!("{}.json", upload.id));
        let mut metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&metadata_path).unwrap()).unwrap();
        metadata["size"] = serde_json::json!(large_size);
        std::fs::write(&metadata_path, serde_json::to_vec(&metadata).unwrap()).unwrap();
        let media_store = crate::services::media_text::MediaStore::open(workspace.path()).unwrap();

        let message = build_multimodal_prompt(
            &format!("[attached: large.png ({})]", upload.id),
            workspace.path(),
            "test",
            "https://public.invalid",
            &crate::services::resource_access::ResourceAccess::new("control-token"),
            &media_store,
        );
        let serialized = serde_json::to_string(&message).unwrap();
        assert!(serialized.contains("/resources/model-provider/files/test/"));
        assert!(serialized.contains("resource_provenance"));
    }
}

#[cfg(test)]
mod trail_tests {
    //! #80: a reloaded conversation names the computer a desktop tool acted
    //! on the way the live trail did.
    use super::{ContentBlock, HistoryEntry, Message, history_to_chat_messages, tool_use_summary};
    use crate::domain::chat::MessageKind;

    const STUDIO: &str = "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b";

    fn tool_call(id: &str, name: &str, arguments: serde_json::Value) -> HistoryEntry {
        HistoryEntry::new(
            Message::Assistant {
                content: vec![ContentBlock::ToolCall {
                    id: id.into(),
                    name: name.into(),
                    arguments,
                }],
            },
            "1".into(),
            format!("msg_{id}"),
        )
    }

    #[test]
    fn a_reloaded_trail_reads_the_line_persisted_with_the_call() {
        let mut entry = tool_call(
            "call_1",
            "remote_bash",
            serde_json::json!({"command": "uname -a"}),
        );
        entry.tool_trail = Some(
            [(
                "call_1".to_owned(),
                "running a command on Studio Mac".to_owned(),
            )]
            .into(),
        );
        let messages = history_to_chat_messages(&[entry]);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].kind, MessageKind::ToolCall);
        assert_eq!(messages[0].tool_name.as_deref(), Some("remote_bash"));
        assert_eq!(messages[0].content, "running a command on Studio Mac");

        // The persisted line belongs to its own call, never to a neighbour.
        let mut entry = HistoryEntry::new(
            Message::Assistant {
                content: vec![
                    ContentBlock::ToolCall {
                        id: "call_a".into(),
                        name: "computer_use".into(),
                        arguments: serde_json::json!({"action": "screenshot"}),
                    },
                    ContentBlock::ToolCall {
                        id: "call_b".into(),
                        name: "remote_files".into(),
                        arguments: serde_json::json!({"operation": "list", "path": "~"}),
                    },
                ],
            },
            "1".into(),
            "msg_pair".into(),
        );
        entry.tool_trail = Some(
            [
                ("call_a".to_owned(), "screenshot on Studio Mac".to_owned()),
                ("call_b".to_owned(), "listing ~ on Studio Mac".to_owned()),
            ]
            .into(),
        );
        let messages = history_to_chat_messages(&[entry]);
        assert_eq!(messages[0].content, "screenshot on Studio Mac");
        assert_eq!(messages[1].content, "listing ~ on Studio Mac");
    }

    #[test]
    fn a_call_without_a_persisted_line_reads_like_the_live_trail_from_its_arguments() {
        // Older histories and other writers have no trail line: the desktop
        // tools still read as the live trail words them, naming the computer
        // the arguments name (by id, there is no listing on reload).
        let named = tool_call(
            "call_1",
            "computer_use",
            serde_json::json!({"machine_id": STUDIO, "action": "left_click"}),
        );
        assert_eq!(
            history_to_chat_messages(&[named])[0].content,
            format!("left_click on {STUDIO}")
        );
        let bash = tool_call(
            "call_2",
            "remote_bash",
            serde_json::json!({"command": "ls"}),
        );
        assert_eq!(
            history_to_chat_messages(&[bash])[0].content,
            "running a command on the connected computer"
        );
        let files = tool_call(
            "call_3",
            "remote_files",
            serde_json::json!({"machine_id": STUDIO, "operation": "read", "path": "~/notes.md"}),
        );
        assert_eq!(
            history_to_chat_messages(&[files])[0].content,
            format!("reading ~/notes.md on {STUDIO}")
        );
        assert_eq!(
            tool_use_summary("list_machines", &serde_json::json!({})),
            "listing computers"
        );
        // The typed window tools (#18) name the computer the same way, so a
        // reloaded call reads "observing a window on <id>", never
        // "get_window_state: …" (#218).
        let observed = tool_call(
            "call_4",
            "get_window_state",
            serde_json::json!({"machine_id": STUDIO, "target": {"pid": 1, "window_id": 2}}),
        );
        assert_eq!(
            history_to_chat_messages(&[observed])[0].content,
            format!("observing a window on {STUDIO}")
        );
        let acted = tool_call(
            "call_5",
            "act",
            serde_json::json!({
                "machine_id": STUDIO,
                "target": {"pid": 1, "window_id": 2},
                "action": {"kind": "click", "address": {"kind": "element_token", "element_token": "tok/a"}}
            }),
        );
        assert_eq!(
            history_to_chat_messages(&[acted])[0].content,
            format!("click on {STUDIO}")
        );
        assert_eq!(
            tool_use_summary(
                "discover_windows",
                &serde_json::json!({"machine_id": STUDIO, "mode": "list_windows"})
            ),
            format!("listing windows on {STUDIO}")
        );
        assert_eq!(
            tool_use_summary(
                "verify_state",
                &serde_json::json!({"machine_id": STUDIO, "expect": []})
            ),
            format!("verifying a window on {STUDIO}")
        );
        // Every other tool keeps its argument summary.
        assert_eq!(
            tool_use_summary("read_file", &serde_json::json!({"path": "notes.md"})),
            "read_file: notes.md"
        );
        assert_eq!(
            tool_use_summary("run_command", &serde_json::json!({"command": "ls"})),
            "run_command: command"
        );
    }

    #[test]
    fn the_trail_line_stays_out_of_what_the_model_replays() {
        let mut entry = tool_call(
            "call_1",
            "remote_bash",
            serde_json::json!({"command": "uname -a"}),
        );
        entry.tool_trail = Some(
            [(
                "call_1".to_owned(),
                "running a command on Studio Mac".to_owned(),
            )]
            .into(),
        );
        let replayed =
            serde_json::to_string(&HistoryEntry::to_messages(std::slice::from_ref(&entry)))
                .unwrap();
        assert!(!replayed.contains("Studio Mac"), "{replayed}");
        // It survives the history file, and its absence is not an error.
        let json = serde_json::to_string(&entry).unwrap();
        let back: HistoryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tool_trail, entry.tool_trail);
        let bare: HistoryEntry =
            serde_json::from_str(r#"{"role":"assistant","content":[]}"#).unwrap();
        assert_eq!(bare.tool_trail, None);
    }
}
