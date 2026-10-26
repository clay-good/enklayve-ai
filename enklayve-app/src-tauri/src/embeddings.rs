use anyhow::{Result, Context};
use std::path::Path;
use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};
use rayon::prelude::*;

/// Represents a text embedding (vector)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Embedding {
    pub vector: Vec<f32>,
    pub dimension: usize,
}

impl Embedding {
    pub fn new(vector: Vec<f32>) -> Self {
        let dimension = vector.len();
        Self { vector, dimension }
    }

    /// Compute cosine similarity between two embeddings
    pub fn cosine_similarity(&self, other: &Embedding) -> f32 {
        if self.dimension != other.dimension {
            return 0.0;
        }

        let dot_product: f32 = self.vector.iter()
            .zip(other.vector.iter())
            .map(|(a, b)| a * b)
            .sum();

        let magnitude_a: f32 = self.vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        let magnitude_b: f32 = other.vector.iter().map(|x| x * x).sum::<f32>().sqrt();

        if magnitude_a == 0.0 || magnitude_b == 0.0 {
            return 0.0;
        }

        dot_product / (magnitude_a * magnitude_b)
    }

    /// Convert to bytes for database storage
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(&self.vector).unwrap_or_default()
    }

    /// Load from bytes from database
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let vector: Vec<f32> = bincode::deserialize(bytes)
            .context("Failed to deserialize embedding")?;
        Ok(Self::new(vector))
    }
}

/// Embedding generator using FastEmbed (sentence-transformers)
pub struct EmbeddingGenerator {
    model: TextEmbedding,
}

impl EmbeddingGenerator {
    /// A process-wide shared embedder. The BGE ONNX model is loaded from disk
    /// only once and reused, instead of being reloaded on every search/upload
    /// (which previously happened twice per query).
    pub fn shared() -> Result<&'static EmbeddingGenerator> {
        use std::sync::OnceLock;
        static SHARED: OnceLock<EmbeddingGenerator> = OnceLock::new();
        if let Some(g) = SHARED.get() {
            return Ok(g);
        }
        let g = Self::new()?;
        let _ = SHARED.set(g);
        Ok(SHARED.get().expect("embedder just initialized"))
    }

    /// Create a new embedding generator with the default model
    /// Uses BGE-small-en-v1.5 (33MB, 384 dimensions) - excellent quality and fast
    pub fn new() -> Result<Self> {
        // Set cache directory to user's home
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| "/tmp".to_string());
        let cache_dir = std::path::PathBuf::from(format!("{}/.cache/fastembed", home));
        std::fs::create_dir_all(&cache_dir)?;

        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::BGESmallENV15)
                .with_cache_dir(cache_dir)
                .with_show_download_progress(true)
        ).context("Failed to initialize FastEmbed model. This will download a 33MB model file on first use.")?;

        Ok(Self { model })
    }

    /// Load a specific embedding model
    pub fn load_model(&mut self, _model_path: &Path) -> Result<()> {
        // For now, we use FastEmbed's built-in models
        // This method is kept for API compatibility
        // Future: Could support custom ONNX models
        Ok(())
    }

    /// Generate embedding for a single text
    /// Returns a 384-dimensional vector for BGE-small-en-v1.5
    pub fn generate_embedding(&self, text: &str) -> Result<Embedding> {
        // FastEmbed returns Vec<Vec<f32>> for batch processing
        // We pass a single text and take the first result
        let embeddings = self.model.embed(vec![text.to_string()], None)?;

        let vector = embeddings
            .first()
            .ok_or_else(|| anyhow::anyhow!("No embedding generated"))?
            .clone();

        Ok(Embedding::new(vector))
    }

    /// Generate embeddings for multiple texts in batch
    /// More efficient than calling generate_embedding repeatedly
    pub fn generate_embeddings_batch(&self, texts: &[String]) -> Result<Vec<Embedding>> {
        let embeddings = self.model.embed(texts.to_vec(), None)?;

        Ok(embeddings
            .into_iter()
            .map(Embedding::new)
            .collect())
    }

    /// Generate embeddings for multiple texts with parallel batch processing
    /// Optimized for large document sets with 100+ chunks
    /// Uses all available CPU cores and shows progress for large batches
    pub fn generate_embeddings_parallel<F>(
        &self,
        texts: &[String],
        progress_callback: F,
    ) -> Result<Vec<Embedding>>
    where
        F: Fn(usize, usize) + Send + Sync,
    {
        let total_chunks = texts.len();
        let start_time = std::time::Instant::now();

        crate::logger::log_info(&format!(
            "Starting parallel embedding generation for {} chunks",
            total_chunks
        ));

        // Determine optimal batch size based on total chunks
        // FastEmbed is optimized for batch processing, so larger batches are better
        let batch_size = if total_chunks > 1000 {
            128
        } else if total_chunks > 100 {
            64
        } else {
            32
        };

        crate::logger::log_info(&format!(
            "Using batch size {} for {} chunks",
            batch_size, total_chunks
        ));

        // Split texts into batches for parallel processing
        let batches: Vec<&[String]> = texts.chunks(batch_size).collect();
        let num_batches = batches.len();

        crate::logger::log_info(&format!(
            "Processing {} batches in parallel using {} CPU cores",
            num_batches,
            num_cpus::get()
        ));

        // Process batches in parallel using rayon
        // Each batch is processed by FastEmbed which is already optimized
        let processed = std::sync::atomic::AtomicUsize::new(0);

        let results: Result<Vec<Vec<Embedding>>> = batches
            .par_iter()
            .map(|batch| {
                // Generate embeddings for this batch
                let batch_embeddings = self.model.embed(batch.to_vec(), None)
                    .context("Failed to generate batch embeddings")?;

                let embeddings: Vec<Embedding> = batch_embeddings
                    .into_iter()
                    .map(Embedding::new)
                    .collect();

                // Update progress
                let chunks_processed = processed.fetch_add(batch.len(), std::sync::atomic::Ordering::Relaxed) + batch.len();

                // Call progress callback (thread-safe)
                progress_callback(chunks_processed, total_chunks);

                Ok(embeddings)
            })
            .collect();

        let all_embeddings: Vec<Embedding> = results?
            .into_iter()
            .flatten()
            .collect();

        let elapsed = start_time.elapsed();
        let chunks_per_second = if elapsed.as_secs_f64() > 0.0 {
            total_chunks as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };

        crate::logger::log_info(&format!(
            "Parallel embedding generation complete: {} chunks in {:.2}s ({:.1} chunks/sec)",
            total_chunks,
            elapsed.as_secs_f64(),
            chunks_per_second
        ));

        Ok(all_embeddings)
    }

    /// Generate embeddings for multiple texts with simple parallel processing (no progress callback)
    pub fn generate_embeddings_parallel_simple(&self, texts: &[String]) -> Result<Vec<Embedding>> {
        self.generate_embeddings_parallel(texts, |_, _| {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity() {
        let emb1 = Embedding::new(vec![1.0, 0.0, 0.0]);
        let emb2 = Embedding::new(vec![1.0, 0.0, 0.0]);
        let emb3 = Embedding::new(vec![0.0, 1.0, 0.0]);

        // Identical vectors should have similarity 1.0
        assert!((emb1.cosine_similarity(&emb2) - 1.0).abs() < 0.0001);

        // Orthogonal vectors should have similarity 0.0
        assert!((emb1.cosine_similarity(&emb3) - 0.0).abs() < 0.0001);
    }

    #[test]
    fn test_embedding_serialization() {
        let emb = Embedding::new(vec![0.1, 0.2, 0.3, 0.4, 0.5]);
        let bytes = emb.to_bytes();
        let restored = Embedding::from_bytes(&bytes).unwrap();

        assert_eq!(emb.dimension, restored.dimension);
        for (a, b) in emb.vector.iter().zip(restored.vector.iter()) {
            assert!((a - b).abs() < 0.0001);
        }
    }

    #[test]
    fn test_generate_embedding() {
        let generator = EmbeddingGenerator::new().unwrap();
        let text = "This is a test document about machine learning.";
        let emb = generator.generate_embedding(text).unwrap();

        assert_eq!(emb.dimension, 384);

        // Vector should be normalized
        let magnitude: f32 = emb.vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((magnitude - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_similar_texts_have_high_similarity() {
        let generator = EmbeddingGenerator::new().unwrap();

        let emb1 = generator.generate_embedding("machine learning artificial intelligence").unwrap();
        let emb2 = generator.generate_embedding("machine learning artificial intelligence").unwrap();
        let emb3 = generator.generate_embedding("cooking recipes food preparation").unwrap();

        let sim_same = emb1.cosine_similarity(&emb2);
        let sim_different = emb1.cosine_similarity(&emb3);

        // Identical texts should have similarity 1.0
        assert!((sim_same - 1.0).abs() < 0.0001);

        // Different topics should have lower similarity
        assert!(sim_different < sim_same);
    }
}
