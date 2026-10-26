use anyhow::{Context, Result};
use llama_cpp_2::{
    context::params::LlamaContextParams,
    context::LlamaContext,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{AddBos, LlamaModel, Special, params::LlamaModelParams},
    sampling::LlamaSampler,
};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::num::NonZeroU32;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Calculate text similarity ratio (simple word-based comparison)
fn similar_text_ratio(text1: &str, text2: &str) -> f64 {
    let words1: std::collections::HashSet<&str> = text1.split_whitespace().collect();
    let words2: std::collections::HashSet<&str> = text2.split_whitespace().collect();

    if words1.is_empty() && words2.is_empty() {
        return 1.0;
    }

    if words1.is_empty() || words2.is_empty() {
        return 0.0;
    }

    let intersection = words1.intersection(&words2).count();
    let union = words1.union(&words2).count();

    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

/// Detect filler phrase loops - when the model is stuck generating polite but useless responses
fn detect_filler_loop(text: &str) -> bool {
    let text_lower = text.to_lowercase();

    // Common filler phrases that indicate the model is stuck
    let filler_patterns = [
        "please let me know",
        "i'm happy to help",
        "i'm here to help",
        "feel free to ask",
        "if you have any",
        "thank you for your patience",
        "is there anything else",
        "i look forward to",
        "please feel free",
        "let me know if you",
        "i hope this helps",
    ];

    // Count how many times filler phrases appear
    let mut filler_count = 0;
    for pattern in filler_patterns.iter() {
        filler_count += text_lower.matches(pattern).count();
    }

    // If we see 4+ filler phrases in the text, the model is likely stuck
    filler_count >= 4
}

/// Detect repeated sentence patterns using n-gram analysis
fn detect_sentence_repetition(text: &str) -> bool {
    // Split into sentences (rough approximation)
    let sentences: Vec<&str> = text
        .split(|c| c == '.' || c == '!' || c == '?')
        .map(|s| s.trim())
        .filter(|s| s.len() > 50) // Only consider longer sentences (avoid short list items)
        .collect();

    if sentences.len() < 6 {
        return false;
    }

    // Check for repeated sentences
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for sentence in sentences.iter() {
        // Normalize: lowercase and remove extra whitespace
        let normalized = sentence.to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        // Only track longer sentences to avoid false positives on list items
        if normalized.len() > 40 {
            *seen.entry(normalized).or_insert(0) += 1;
        }
    }

    // Require 3+ repetitions to trigger (less aggressive)
    seen.values().any(|&count| count >= 3)
}

/// Detect if the model is repeating large blocks of content
fn detect_block_repetition(text: &str) -> bool {
    let len = text.len();
    if len < 200 {
        return false;
    }

    // Check if any 100-char block appears twice in the response
    for window_size in [100, 150, 200] {
        if len < window_size * 2 {
            continue;
        }

        // Split point, moved to a UTF-8 char boundary so byte-slicing a string
        // with multi-byte characters can never panic.
        let mut cut = len - window_size;
        while cut < len && !text.is_char_boundary(cut) {
            cut += 1;
        }

        // Get the last block
        let last_block = &text[cut..];

        // Search for it earlier in the text
        let search_area = &text[..cut];
        if search_area.contains(last_block) {
            return true;
        }
    }

    false
}

/// Detect if the model is echoing back ChatML tokens (indicates broken generation)
fn detect_prompt_echo(text: &str) -> bool {
    // Only check for ChatML tokens that should never appear in output
    // These are structural tokens, not content
    text.contains("<|im_start|>") || text.contains("<|im_end|>")
}

/// Helper macro to safely lock a mutex and recover from poison errors
macro_rules! safe_lock {
    ($mutex:expr) => {
        match $mutex.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                crate::logger::log_warn("Mutex poisoned, recovering from poison error");
                poisoned.into_inner()
            }
        }
    };
}

/// Prompt cache entry storing KV cache state
#[derive(Debug)]
struct PromptCacheEntry {
    hash: u64,
    n_tokens: usize,
    cache_hits: u64,
}

/// A cached model with prompt cache tracking
pub struct CachedModel {
    backend: LlamaBackend,
    model: LlamaModel,
    _path: String,
    prompt_cache: Arc<Mutex<Option<PromptCacheEntry>>>,
    cache_enabled: bool,
}

unsafe impl Send for CachedModel {}
unsafe impl Sync for CachedModel {}

impl CachedModel {
    /// Load a model from disk with GPU acceleration
    pub fn load(path: &str) -> Result<Self> {
        crate::logger::log_info(&format!("Loading model into cache from: {}", path));

        // Initialize backend
        let backend = LlamaBackend::init()?;

        // Log GPU support
        #[cfg(target_os = "macos")]
        crate::logger::log_info("🚀 Metal GPU support enabled for Apple Silicon");

        #[cfg(target_os = "windows")]
        crate::logger::log_info("🚀 GPU support enabled (CUDA if NVIDIA GPU detected)");

        #[cfg(target_os = "linux")]
        crate::logger::log_info("⚙️  GPU support enabled (CUDA if NVIDIA GPU detected)");

        // Detect hardware and calculate optimal GPU layers
        let hardware = crate::hardware::HardwareProfile::detect()
            .unwrap_or_else(|_| {
                crate::logger::log_warn("Failed to detect hardware, using CPU-only");
                // Create minimal default profile
                crate::hardware::HardwareProfile {
                    cpu_vendor: crate::hardware::CpuVendor::Unknown,
                    cpu_brand: "Unknown".to_string(),
                    cpu_cores: 1,
                    cpu_threads: 1,
                    ram_total_gb: 8.0,
                    ram_available_gb: 4.0,
                    has_gpu: false,
                    gpu_vendor: None,
                    gpu_name: None,
                    platform: crate::hardware::Platform::Unknown,
                    is_apple_silicon: false,
                    storage_available_gb: 100.0,
                    performance_tier: crate::hardware::PerformanceTier::Fair,
                }
            });

        let gpu_layers = hardware.get_optimal_gpu_layers(Some(path));
        crate::logger::log_info(&format!(
            "Hardware detected: {:.1} GB RAM, {} - Using {} GPU layers for this model",
            hardware.ram_total_gb,
            if hardware.is_apple_silicon { "Apple Silicon" } else { "x86" },
            gpu_layers
        ));

        let model_params = LlamaModelParams::default()
            .with_n_gpu_layers(gpu_layers)
            .with_use_mlock(false);  // Don't lock memory pages (allows system to manage RAM efficiently)

        let model = LlamaModel::load_from_file(&backend, path, &model_params)
            .context("Failed to load model")?;

        crate::logger::log_info(&format!(
            "Model loaded into cache successfully with {} GPU layers",
            gpu_layers
        ));

        Ok(CachedModel {
            backend,
            model,
            _path: path.to_string(),
            prompt_cache: Arc::new(Mutex::new(None)),
            cache_enabled: true,
        })
    }

    /// Hash a prompt to check if cache can be reused
    fn hash_prompt(prompt: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        prompt.hash(&mut hasher);
        hasher.finish()
    }

    /// Invalidate the prompt cache (call when documents change)
    pub fn invalidate_cache(&self) {
        let mut cache = safe_lock!(self.prompt_cache);
        if cache.is_some() {
            crate::logger::log_info("Invalidating prompt cache due to document changes");
            *cache = None;
        }
    }

    /// Get cache statistics
    pub fn get_cache_stats(&self) -> (bool, u64, f32) {
        let cache = safe_lock!(self.prompt_cache);
        match cache.as_ref() {
            Some(entry) => {
                let hit_rate = if entry.cache_hits > 0 {
                    entry.cache_hits as f32 / (entry.cache_hits + 1) as f32 * 100.0
                } else {
                    0.0
                };
                (true, entry.cache_hits, hit_rate)
            }
            None => (false, 0, 0.0),
        }
    }

    /// Generate a response using the cached model with prompt caching
    pub fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String> {
        // Validate max_tokens parameter
        if max_tokens == 0 {
            anyhow::bail!("max_tokens must be greater than 0");
        }
        if max_tokens > 8192 {
            crate::logger::log_warn(&format!("max_tokens {} exceeds recommended limit of 8192, capping to 8192", max_tokens));
        }

        crate::logger::log_info("Generating using cached model with prompt caching...");

        let prompt_hash = Self::hash_prompt(prompt);

        // Check cache status
        let mut cache_guard = safe_lock!(self.prompt_cache);

        let cache_hit = if self.cache_enabled {
            if let Some(ref cache_entry) = *cache_guard {
                if cache_entry.hash == prompt_hash {
                    crate::logger::log_info("Prompt cache HIT - same prompt detected");
                    true
                } else {
                    crate::logger::log_info("Prompt cache MISS - different prompt detected");
                    false
                }
            } else {
                crate::logger::log_info("Prompt cache MISS - no cache entry");
                false
            }
        } else {
            false
        };

        // Create context for this generation
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(NonZeroU32::new(8192).unwrap()))
            .with_n_batch(2048);

        let mut context = self.model.new_context(&self.backend, ctx_params)
            .context("Failed to create context")?;

        // Tokenize the prompt
        let tokens = self.model.str_to_token(prompt, AddBos::Always)
            .context("Failed to tokenize prompt")?;

        crate::logger::log_info(&format!("Tokenized into {} tokens", tokens.len()));

        // Check if prompt is too large for context window
        if tokens.len() > 7000 {
            crate::logger::log_error(&format!("Prompt too large: {} tokens exceeds safe limit of 7000 tokens", tokens.len()));
            anyhow::bail!("Prompt is too large ({} tokens). Try asking a shorter question or removing some documents.", tokens.len());
        }

        let mut batch = LlamaBatch::new(2048, 1);

        // Process prompt (llama.cpp internally uses KV cache for repeated sequences)
        crate::logger::log_info("Processing prompt tokens");
        let prompt_batch_size = 2048;

        // Process tokens in batches
        for chunk_start in (0..tokens.len()).step_by(prompt_batch_size) {
            batch.clear();
            let chunk_end = std::cmp::min(chunk_start + prompt_batch_size, tokens.len());
            let is_final_batch = chunk_end == tokens.len();

            for (i, token) in tokens[chunk_start..chunk_end].iter().enumerate() {
                let global_pos = chunk_start + i;
                let is_last = is_final_batch && (i == chunk_end - chunk_start - 1);
                batch.add(*token, global_pos as i32, &[0], is_last)
                    .context("Failed to add token to batch")?;
            }

            context.decode(&mut batch)
                .context("Failed to decode prompt batch")?;
        }

        // Update cache tracking
        if cache_hit {
            // Cache hit - increment counter
            if let Some(ref mut entry) = *cache_guard {
                entry.cache_hits += 1;
                crate::logger::log_info(&format!(
                    "Cache hit #{} - prompt hash matched (llama.cpp KV cache active)",
                    entry.cache_hits
                ));
            }
        } else {
            // Store cache entry
            *cache_guard = Some(PromptCacheEntry {
                hash: prompt_hash,
                n_tokens: tokens.len(),
                cache_hits: 0,
            });

            crate::logger::log_info("Prompt hash cached for future tracking");
        }

        batch.clear();

        // Release locks before generation
        drop(cache_guard);

        // Set up sampler chain (heavily optimized for speed while maintaining quality)
        let mut sampler = LlamaSampler::chain_simple(vec![
            LlamaSampler::temp(0.2),   // Very low temp for faster, deterministic responses
            LlamaSampler::top_k(10),    // Reduced from 20 for faster sampling
            LlamaSampler::top_p(0.9, 1), // Slightly increased for quality
            LlamaSampler::dist(42),
        ]);

        // Generate tokens
        let mut response = String::new();
        let mut n_cur = tokens.len();

        for i in 0..max_tokens {
            let new_token = sampler.sample(&context, -1);

            if self.model.is_eog_token(new_token) {
                crate::logger::log_info(&format!("Hit end-of-generation token at iteration {}", i));
                break;
            }

            sampler.accept(new_token);

            let piece = self.model.token_to_str(new_token, Special::Tokenize)
                .unwrap_or_else(|_| String::new());
            response.push_str(&piece);

            if i % 50 == 0 && i > 0 {
                crate::logger::log_info(&format!("Generated {} tokens so far...", i));
            }

            // Add token to batch for next iteration
            batch.add(new_token, n_cur as i32, &[0], true)
                .context("Failed to add generated token to batch")?;

            // Decode the batch
            context.decode(&mut batch)
                .context("Failed to decode generated token")?;

            batch.clear();
            n_cur += 1;
        }

        crate::logger::log_info(&format!("Generated {} tokens total", response.split_whitespace().count()));

        // Log cache stats
        let (has_cache, hits, hit_rate) = self.get_cache_stats();
        if has_cache {
            crate::logger::log_info(&format!(
                "Prompt cache stats: {} hits, {:.1}% hit rate",
                hits, hit_rate
            ));
        }

        Ok(response)
    }

    /// Generate a response with streaming output (buffered token emission)
    pub fn generate_streaming<F>(
        &self,
        prompt: &str,
        max_tokens: u32,
        mut on_token_batch: F,
        stop_flag: Option<Arc<AtomicBool>>,
    ) -> Result<String>
    where
        F: FnMut(&str) -> Result<()>,
    {
        // Validate max_tokens parameter
        if max_tokens == 0 {
            anyhow::bail!("max_tokens must be greater than 0");
        }
        if max_tokens > 8192 {
            crate::logger::log_warn(&format!("max_tokens {} exceeds recommended limit of 8192, capping to 8192", max_tokens));
        }

        crate::logger::log_info("Generating using cached model with streaming (buffered)...");

        let start_time = std::time::Instant::now();
        let prompt_hash = Self::hash_prompt(prompt);

        let mut cache_guard = self.prompt_cache.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() });

        let cache_hit = if self.cache_enabled {
            if let Some(ref cache_entry) = *cache_guard {
                if cache_entry.hash == prompt_hash {
                    crate::logger::log_info("Prompt cache HIT - same prompt detected");
                    true
                } else {
                    crate::logger::log_info("Prompt cache MISS - different prompt detected");
                    false
                }
            } else {
                crate::logger::log_info("Prompt cache MISS - no cache entry");
                false
            }
        } else {
            false
        };

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(NonZeroU32::new(8192).unwrap()))
            .with_n_batch(2048);

        let mut context = self.model.new_context(&self.backend, ctx_params)
            .context("Failed to create context")?;

        let tokens = self.model.str_to_token(prompt, AddBos::Always)
            .context("Failed to tokenize prompt")?;

        crate::logger::log_info(&format!("Tokenized into {} tokens", tokens.len()));

        if tokens.len() > 7000 {
            crate::logger::log_error(&format!("Prompt too large: {} tokens exceeds safe limit of 7000 tokens", tokens.len()));
            anyhow::bail!("Prompt is too large ({} tokens). Try asking a shorter question or removing some documents.", tokens.len());
        }

        let mut batch = LlamaBatch::new(2048, 1);

        crate::logger::log_info("Processing prompt tokens");
        let prompt_batch_size = 2048;

        for chunk_start in (0..tokens.len()).step_by(prompt_batch_size) {
            batch.clear();
            let chunk_end = std::cmp::min(chunk_start + prompt_batch_size, tokens.len());
            let is_final_batch = chunk_end == tokens.len();

            for (i, token) in tokens[chunk_start..chunk_end].iter().enumerate() {
                let global_pos = chunk_start + i;
                let is_last = is_final_batch && (i == chunk_end - chunk_start - 1);
                batch.add(*token, global_pos as i32, &[0], is_last)
                    .context("Failed to add token to batch")?;
            }

            context.decode(&mut batch)
                .context("Failed to decode prompt batch")?;
        }

        if cache_hit {
            if let Some(ref mut entry) = *cache_guard {
                entry.cache_hits += 1;
                crate::logger::log_info(&format!(
                    "Cache hit #{} - prompt hash matched (llama.cpp KV cache active)",
                    entry.cache_hits
                ));
            }
        } else {
            *cache_guard = Some(PromptCacheEntry {
                hash: prompt_hash,
                n_tokens: tokens.len(),
                cache_hits: 0,
            });

            crate::logger::log_info("Prompt hash cached for future tracking");
        }

        batch.clear();
        drop(cache_guard);

        let mut sampler = LlamaSampler::chain_simple(vec![
            LlamaSampler::temp(0.5), // Balanced temp for natural, intelligent responses
            LlamaSampler::top_k(40), // Focused diversity for quality
            LlamaSampler::top_p(0.9, 1), // Slightly tighter for coherence
            LlamaSampler::penalties(self.model.n_vocab(), 256, 1.1, 0.0, 0.95), // Moderate penalties for natural flow
            LlamaSampler::dist(42),
        ]);

        let mut response = String::new();
        let mut n_cur = tokens.len();
        let mut token_buffer = String::new();
        const BUFFER_SIZE: usize = 2; // Smaller buffer for IMMEDIATE stop response
        let mut tokens_generated = 0;

        // Repetition detection to prevent infinite loops
        let mut recent_text = String::new();
        const REPETITION_WINDOW: usize = 100; // Check last 100 chars (faster detection)
        const REPETITION_THRESHOLD: usize = 40; // If 40+ chars repeat, stop (more aggressive)
        let mut repetition_count = 0;

        let generation_start = std::time::Instant::now();

        for i in 0..max_tokens {
            // CRITICAL: Check stop flag FIRST before any processing
            if let Some(ref flag) = stop_flag {
                if flag.load(Ordering::Relaxed) {
                    if !token_buffer.is_empty() {
                        on_token_batch(&token_buffer)?;
                    }
                    crate::logger::log_info("Generation stopped by user request (pre-sample check)");
                    break;
                }
            }

            let new_token = sampler.sample(&context, -1);

            // Check IMMEDIATELY after token generation
            if let Some(ref flag) = stop_flag {
                if flag.load(Ordering::Relaxed) {
                    if !token_buffer.is_empty() {
                        on_token_batch(&token_buffer)?;
                    }
                    crate::logger::log_info("Generation stopped by user request (post-sample check)");
                    break;
                }
            }

            if self.model.is_eog_token(new_token) {
                if !token_buffer.is_empty() {
                    on_token_batch(&token_buffer)?;
                }
                crate::logger::log_info(&format!("Hit end-of-generation token at iteration {}", i));
                break;
            }

            // Additional EOG check for common stop patterns
            let token_str = self.model.token_to_str(new_token, Special::Tokenize)
                .unwrap_or_else(|_| String::new());

            // Check for explicit stop markers (including ChatML format)
            if token_str.contains("</s>") || token_str.contains("<|endoftext|>") ||
               token_str.contains("<|end|>") || token_str.contains("<|im_end|>") ||
               token_str.contains("<|im_start|>") {
                if !token_buffer.is_empty() {
                    on_token_batch(&token_buffer)?;
                }
                crate::logger::log_info(&format!("Hit stop marker token at iteration {}", i));
                break;
            }

            // Accept the token before using it
            sampler.accept(new_token);

            let piece = token_str;

            response.push_str(&piece);
            token_buffer.push_str(&piece);
            tokens_generated += 1;

            // Update repetition detection window
            recent_text.push_str(&piece);
            if recent_text.len() > REPETITION_WINDOW {
                // Keep the last REPETITION_WINDOW characters (char-based, so
                // multi-byte UTF-8 is never split and the count can't underflow).
                let overflow = recent_text.chars().count().saturating_sub(REPETITION_WINDOW);
                recent_text = recent_text.chars().skip(overflow).collect();
            }

            // Check for repetition every 10 tokens
            if i % 10 == 0 && response.len() > 200 {
                // Check for prompt echo (model repeating instructions)
                if detect_prompt_echo(&response) {
                    if !token_buffer.is_empty() {
                        on_token_batch(&token_buffer)?;
                    }
                    crate::logger::log_warn("Stopping generation due to prompt echo detected");
                    break;
                }

                // Check for block repetition (large repeated sections)
                if response.len() > 300 && detect_block_repetition(&response) {
                    if !token_buffer.is_empty() {
                        on_token_batch(&token_buffer)?;
                    }
                    crate::logger::log_warn("Stopping generation due to block repetition detected");
                    break;
                }

                // Check for filler phrase loops (polite but useless responses)
                if response.len() > 500 && detect_filler_loop(&response) {
                    if !token_buffer.is_empty() {
                        on_token_batch(&token_buffer)?;
                    }
                    crate::logger::log_warn("Stopping generation due to filler phrase loop detected");
                    break;
                }

                // Check for repeated sentence patterns
                if response.len() > 400 && detect_sentence_repetition(&response) {
                    if !token_buffer.is_empty() {
                        on_token_batch(&token_buffer)?;
                    }
                    crate::logger::log_warn("Stopping generation due to repeated sentence patterns detected");
                    break;
                }
            }

            // Original similarity-based repetition detection (less frequent check)
            if i % 10 == 0 && response.len() > REPETITION_WINDOW * 2 {
                // Get the text before the recent window
                let response_len = response.len();
                if response_len > REPETITION_WINDOW * 2 {
                    let earlier_start = response_len.saturating_sub(REPETITION_WINDOW * 2);
                    let earlier_end = response_len.saturating_sub(REPETITION_WINDOW);
                    // Move each index forward to the next UTF-8 char boundary so
                    // byte-slicing never splits a multi-byte character (crash fix).
                    let mut es = earlier_start;
                    while es < response_len && !response.is_char_boundary(es) {
                        es += 1;
                    }
                    let mut ee = earlier_end;
                    while ee < response_len && !response.is_char_boundary(ee) {
                        ee += 1;
                    }
                    let earlier_text = if es <= ee { &response[es..ee] } else { "" };

                    // Check if recent text is very similar to earlier text
                    if recent_text.len() >= REPETITION_THRESHOLD && earlier_text.len() >= REPETITION_THRESHOLD {
                        let similarity = similar_text_ratio(earlier_text, &recent_text);
                        if similarity > 0.7 {
                            repetition_count += 1;
                            if repetition_count >= 3 {
                                if !token_buffer.is_empty() {
                                    on_token_batch(&token_buffer)?;
                                }
                                crate::logger::log_warn(&format!(
                                    "Stopping generation due to repetition detected (similarity: {:.2})",
                                    similarity
                                ));
                                break;
                            }
                        } else {
                            repetition_count = 0;
                        }
                    }
                }
            }

            // Check before buffer emission for maximum responsiveness
            if let Some(ref flag) = stop_flag {
                if flag.load(Ordering::Relaxed) {
                    if !token_buffer.is_empty() {
                        on_token_batch(&token_buffer)?;
                    }
                    crate::logger::log_info("Generation stopped by user request (pre-buffer check)");
                    break;
                }
            }

            if token_buffer.chars().count() >= BUFFER_SIZE || i == max_tokens - 1 {
                on_token_batch(&token_buffer)?;
                token_buffer.clear();
            }

            // Check again after buffer emission
            if let Some(ref flag) = stop_flag {
                if flag.load(Ordering::Relaxed) {
                    crate::logger::log_info("Generation stopped by user request (post-buffer check)");
                    break;
                }
            }

            batch.add(new_token, n_cur as i32, &[0], true)
                .context("Failed to add generated token to batch")?;

            context.decode(&mut batch)
                .context("Failed to decode generated token")?;

            batch.clear();
            n_cur += 1;
        }

        let generation_elapsed = generation_start.elapsed();
        let tokens_per_second = if generation_elapsed.as_secs_f64() > 0.0 {
            tokens_generated as f64 / generation_elapsed.as_secs_f64()
        } else {
            0.0
        };

        let total_elapsed = start_time.elapsed();

        crate::logger::log_info(&format!(
            "Streaming generation complete: {} tokens in {:.2}s ({:.1} tokens/sec), total time {:.2}s",
            tokens_generated,
            generation_elapsed.as_secs_f64(),
            tokens_per_second,
            total_elapsed.as_secs_f64()
        ));

        let (has_cache, hits, hit_rate) = self.get_cache_stats();
        if has_cache {
            crate::logger::log_info(&format!(
                "Prompt cache stats: {} hits, {:.1}% hit rate",
                hits, hit_rate
            ));
        }

        Ok(response)
    }
}

/// Global model cache manager
pub struct ModelCache {
    current_model: Arc<Mutex<Option<CachedModel>>>,
    current_path: Arc<Mutex<Option<String>>>,
    preload_status: Arc<Mutex<PreloadStatus>>,
    stop_generation: Arc<AtomicBool>,
}

#[derive(Clone, Debug)]
pub enum PreloadStatus {
    NotStarted,
    Loading,
    Loaded,
    Failed(String),
    Cancelled,
}

impl ModelCache {
    pub fn new() -> Self {
        ModelCache {
            current_model: Arc::new(Mutex::new(None)),
            current_path: Arc::new(Mutex::new(None)),
            preload_status: Arc::new(Mutex::new(PreloadStatus::NotStarted)),
            stop_generation: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Request to stop ongoing generation
    pub fn stop_generation(&self) {
        crate::logger::log_info("Stop generation requested");
        self.stop_generation.store(true, Ordering::Relaxed);
    }

    /// Reset the stop flag
    fn reset_stop_flag(&self) {
        self.stop_generation.store(false, Ordering::Relaxed);
    }

    /// Preload a model in the background
    pub fn preload_model(&self, path: String) {
        let current_model = Arc::clone(&self.current_model);
        let current_path = Arc::clone(&self.current_path);
        let preload_status = Arc::clone(&self.preload_status);

        *preload_status.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() }) = PreloadStatus::Loading;
        crate::logger::log_info("Starting background model preload...");

        std::thread::spawn(move || {
            let start_time = std::time::Instant::now();

            match CachedModel::load(&path) {
                Ok(model) => {
                    let elapsed = start_time.elapsed();
                    crate::logger::log_info(&format!(
                        "Model preloaded successfully in {:.2} seconds",
                        elapsed.as_secs_f64()
                    ));

                    *current_model.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() }) = Some(model);
                    *current_path.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() }) = Some(path);
                    *preload_status.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() }) = PreloadStatus::Loaded;
                }
                Err(e) => {
                    crate::logger::log_error(&format!("Model preload failed: {}", e));
                    *preload_status.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() }) = PreloadStatus::Failed(e.to_string());
                }
            }
        });
    }

    /// Get preload status
    pub fn get_preload_status(&self) -> PreloadStatus {
        self.preload_status.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() }).clone()
    }

    /// Cancel preload (note: can only prevent model from being cached, not stop loading)
    pub fn cancel_preload(&self) {
        crate::logger::log_info("Preload cancelled");
        *self.preload_status.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() }) = PreloadStatus::Cancelled;
    }

    /// Get or load a model, caching it for future use
    pub fn get_or_load(&self, path: &str) -> Result<String> {
        let current_path = self.current_path.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() });

        // Check if we already have this model loaded
        if let Some(cached_path) = current_path.as_ref() {
            if cached_path == path {
                crate::logger::log_info("Using already-loaded model from cache");
                drop(current_path);
                return Ok("cached".to_string());
            }
        }
        drop(current_path);

        // Need to load new model
        crate::logger::log_info(&format!("Loading new model: {}", path));
        let model = CachedModel::load(path)?;

        // Store in cache
        *self.current_model.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() }) = Some(model);
        *self.current_path.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() }) = Some(path.to_string());

        Ok("loaded".to_string())
    }

    /// Generate using the cached model
    pub fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String> {
        let model_guard = self.current_model.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() });

        match model_guard.as_ref() {
            Some(model) => model.generate(prompt, max_tokens),
            None => Err(anyhow::anyhow!("No model loaded in cache")),
        }
    }

    /// Generate with streaming output using the cached model
    pub fn generate_streaming<F>(
        &self,
        prompt: &str,
        max_tokens: u32,
        on_token_batch: F,
    ) -> Result<String>
    where
        F: FnMut(&str) -> Result<()>,
    {
        // Reset stop flag before starting generation
        self.reset_stop_flag();

        let model_guard = self.current_model.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() });
        let stop_flag = Arc::clone(&self.stop_generation);

        match model_guard.as_ref() {
            Some(model) => model.generate_streaming(prompt, max_tokens, on_token_batch, Some(stop_flag)),
            None => Err(anyhow::anyhow!("No model loaded in cache")),
        }
    }

    /// Clear the cache
    pub fn clear(&self) {
        crate::logger::log_info("Clearing model cache");
        *self.current_model.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() }) = None;
        *self.current_path.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() }) = None;
    }

    /// Invalidate prompt cache (call when documents change)
    pub fn invalidate_prompt_cache(&self) {
        let model_guard = self.current_model.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() });
        if let Some(model) = model_guard.as_ref() {
            model.invalidate_cache();
        }
    }

    /// Get cache statistics
    pub fn get_prompt_cache_stats(&self) -> (bool, u64, f32) {
        let model_guard = self.current_model.lock().unwrap_or_else(|poisoned| { crate::logger::log_warn("Mutex poisoned, recovering"); poisoned.into_inner() });
        match model_guard.as_ref() {
            Some(model) => model.get_cache_stats(),
            None => (false, 0, 0.0),
        }
    }
}
