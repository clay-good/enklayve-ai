use anyhow::Result;
use tauri::AppHandle;
use crate::embeddings::{Embedding, EmbeddingGenerator};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchResult {
    pub chunk_id: i64,
    pub document_id: i64,
    pub chunk_text: String,
    pub chunk_index: i64,
    pub similarity: f32,
    pub file_name: String,
}

/// Search for relevant chunks using vector similarity
pub async fn search_similar_chunks(
    query: &str,
    app_handle: &AppHandle,
    top_k: usize,
    conversation_id: i64,
) -> Result<Vec<SearchResult>> {
    // Generate embedding for the query
    let generator = EmbeddingGenerator::new()?;
    let query_embedding = generator.generate_embedding(query)?;

    // Get database connection
    let conn = crate::database::get_connection(app_handle)?;

    // Retrieve all chunks with embeddings
    let mut stmt = conn.prepare(
        "SELECT c.id, c.document_id, c.chunk_text, c.chunk_index, c.embedding, d.file_name
         FROM chunks c
         JOIN documents d ON c.document_id = d.id
         WHERE c.embedding IS NOT NULL AND d.conversation_id = ?1"
    )?;

    let chunks = stmt.query_map([conversation_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,        // chunk_id
            row.get::<_, i64>(1)?,        // document_id
            row.get::<_, String>(2)?,     // chunk_text
            row.get::<_, i64>(3)?,        // chunk_index
            row.get::<_, Vec<u8>>(4)?,    // embedding
            row.get::<_, String>(5)?,     // file_name
        ))
    })?;

    // Calculate similarities and collect results
    let mut results: Vec<SearchResult> = Vec::new();

    for chunk_result in chunks {
        let (chunk_id, document_id, chunk_text, chunk_index, embedding_bytes, file_name) =
            chunk_result?;

        // Deserialize embedding
        if let Ok(chunk_embedding) = Embedding::from_bytes(&embedding_bytes) {
            // Calculate cosine similarity
            let similarity = query_embedding.cosine_similarity(&chunk_embedding);

            results.push(SearchResult {
                chunk_id,
                document_id,
                chunk_text,
                chunk_index,
                similarity,
                file_name,
            });
        }
    }

    // Sort by similarity (descending) and take top_k
    // Use unwrap_or to handle NaN values (treat NaN as negative infinity for sorting)
    results.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(top_k);

    Ok(results)
}

/// Get chunks from a specific document
/// Best cosine similarity between the query and any indexed chunk (0.0 if none).
/// Used as a relevance gate to decide whether the documents actually cover a
/// question before feeding their chunks to the model.
pub async fn top_semantic_similarity(query: &str, conversation_id: i64, app_handle: &AppHandle) -> Result<f32> {
    let results = search_similar_chunks(query, app_handle, 1, conversation_id).await?;
    Ok(results.first().map(|r| r.similarity).unwrap_or(0.0))
}

pub async fn get_document_chunks(
    document_id: i64,
    app_handle: &AppHandle,
) -> Result<Vec<SearchResult>> {
    let conn = crate::database::get_connection(app_handle)?;

    let mut stmt = conn.prepare(
        "SELECT c.id, c.document_id, c.chunk_text, c.chunk_index, d.file_name
         FROM chunks c
         JOIN documents d ON c.document_id = d.id
         WHERE c.document_id = ?1
         ORDER BY c.chunk_index"
    )?;

    let chunks = stmt.query_map([document_id], |row| {
        Ok(SearchResult {
            chunk_id: row.get(0)?,
            document_id: row.get(1)?,
            chunk_text: row.get(2)?,
            chunk_index: row.get(3)?,
            similarity: 1.0, // Not a similarity search
            file_name: row.get(4)?,
        })
    })?;

    chunks.collect::<Result<Vec<_>, _>>().map_err(|e| e.into())
}

/// Search for chunks using FTS5 keyword search
pub async fn keyword_search(
    query: &str,
    app_handle: &AppHandle,
    top_k: usize,
    conversation_id: i64,
) -> Result<Vec<SearchResult>> {
    if query.trim().is_empty() {
        return Ok(vec![]);
    }

    let sanitized_query = sanitize_fts_query(query);

    let conn = crate::database::get_connection(app_handle)?;

    let mut stmt = conn.prepare(
        "SELECT c.id, c.document_id, c.chunk_text, c.chunk_index, d.file_name,
                bm25(chunks_fts) as rank
         FROM chunks_fts
         JOIN chunks c ON chunks_fts.rowid = c.id
         JOIN documents d ON c.document_id = d.id
         WHERE chunks_fts MATCH ?1 AND d.conversation_id = ?3
         ORDER BY rank
         LIMIT ?2"
    )?;

    let results = stmt.query_map(rusqlite::params![sanitized_query, top_k, conversation_id], |row| {
        Ok(SearchResult {
            chunk_id: row.get(0)?,
            document_id: row.get(1)?,
            chunk_text: row.get(2)?,
            chunk_index: row.get(3)?,
            file_name: row.get(4)?,
            similarity: -row.get::<_, f32>(5)? / 100.0,
        })
    })?;

    results.collect::<Result<Vec<_>, _>>().map_err(|e| e.into())
}

/// Sanitize FTS5 query to handle special characters and operators
fn sanitize_fts_query(query: &str) -> String {
    let mut sanitized = query.to_string();

    // Remove FTS5 boolean operators to prevent injection
    let fts_operators = ["OR", "AND", "NOT", "NEAR"];
    for op in fts_operators {
        // Case-insensitive replacement with spaces to avoid joining words
        sanitized = sanitized.replace(&format!(" {} ", op), " ");
        sanitized = sanitized.replace(&format!(" {}", op), " ");
        sanitized = sanitized.replace(&format!("{} ", op), " ");
    }

    // Remove all FTS5 special characters
    let fts_special_chars = ['*', '(', ')', '[', ']', '{', '}', '^', '~', ':', '"', '\''];
    for ch in fts_special_chars {
        sanitized = sanitized.replace(ch, "");
    }

    // Keep only alphanumeric and whitespace
    sanitized = sanitized.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();

    // Collapse multiple spaces
    let words: Vec<&str> = sanitized.split_whitespace().collect();
    sanitized = words.join(" ");

    // Limit query length to prevent DOS
    if sanitized.chars().count() > 500 {
        sanitized = sanitized.chars().take(500).collect();
    }

    // Wrap in quotes for exact phrase matching (prevents operator injection)
    if !sanitized.trim().is_empty() {
        format!("\"{}\"", sanitized.trim())
    } else {
        // If empty after sanitization, return safe fallback
        "\"\"".to_string()
    }
}

/// Expand query with synonyms and related terms
fn expand_query(query: &str) -> String {
    let mut expanded_terms = vec![query.to_string()];

    let expansions = [
        ("revenue", vec!["income", "earnings", "sales"]),
        ("cost", vec!["expense", "expenditure", "spending"]),
        ("profit", vec!["earnings", "gain", "margin"]),
        ("growth", vec!["increase", "expansion", "rise"]),
        ("decline", vec!["decrease", "reduction", "drop"]),
        ("customer", vec!["client", "consumer", "buyer"]),
        ("product", vec!["item", "goods", "merchandise"]),
        ("service", vec!["offering", "solution", "support"]),
        ("company", vec!["business", "organization", "corporation"]),
        ("employee", vec!["worker", "staff", "personnel"]),
        ("market", vec!["industry", "sector", "segment"]),
        ("strategy", vec!["plan", "approach", "tactic"]),
        ("risk", vec!["threat", "hazard", "danger"]),
        ("opportunity", vec!["chance", "prospect", "potential"]),
        ("analysis", vec!["examination", "review", "assessment"]),
    ];

    let query_lower = query.to_lowercase();
    for (term, synonyms) in &expansions {
        if query_lower.contains(term) {
            for synonym in synonyms {
                expanded_terms.push(synonym.to_string());
            }
        }
    }

    if expanded_terms.len() > 1 {
        expanded_terms.join(" OR ")
    } else {
        query.to_string()
    }
}

/// Hybrid search combining semantic and keyword search using Reciprocal Rank Fusion
pub async fn hybrid_search(
    query: &str,
    app_handle: &AppHandle,
    top_k: usize,
    conversation_id: i64,
) -> Result<Vec<SearchResult>> {
    let semantic_results = search_similar_chunks(query, app_handle, top_k * 2, conversation_id).await?;

    let expanded_query = expand_query(query);
    let keyword_results = keyword_search(&expanded_query, app_handle, top_k * 2, conversation_id).await?;

    if semantic_results.is_empty() && keyword_results.is_empty() {
        return Ok(vec![]);
    }

    if semantic_results.is_empty() {
        let mut results = keyword_results;
        results.truncate(top_k);
        return Ok(results);
    }

    if keyword_results.is_empty() {
        let mut results = semantic_results;
        results.truncate(top_k);
        return Ok(results);
    }

    let rrf_k = 60.0;
    let mut rrf_scores: std::collections::HashMap<i64, f32> = std::collections::HashMap::new();
    let mut chunk_map: std::collections::HashMap<i64, SearchResult> = std::collections::HashMap::new();

    for (rank, result) in semantic_results.iter().enumerate() {
        let score = 1.0 / (rrf_k + (rank + 1) as f32);
        *rrf_scores.entry(result.chunk_id).or_insert(0.0) += score * 1.2;
        chunk_map.entry(result.chunk_id).or_insert(result.clone());
    }

    for (rank, result) in keyword_results.iter().enumerate() {
        let score = 1.0 / (rrf_k + (rank + 1) as f32);
        *rrf_scores.entry(result.chunk_id).or_insert(0.0) += score;
        chunk_map.entry(result.chunk_id).or_insert(result.clone());
    }

    let mut combined_results: Vec<SearchResult> = chunk_map
        .into_iter()
        .map(|(chunk_id, mut result)| {
            result.similarity = *rrf_scores.get(&chunk_id).unwrap_or(&0.0);
            result
        })
        .collect();

    // Use unwrap_or to handle NaN values (treat NaN as equal for sorting)
    combined_results.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap_or(std::cmp::Ordering::Equal));
    combined_results.truncate(top_k);

    Ok(combined_results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_result_serialization() {
        let result = SearchResult {
            chunk_id: 1,
            document_id: 1,
            chunk_text: "Test chunk".to_string(),
            chunk_index: 0,
            similarity: 0.95,
            file_name: "test.pdf".to_string(),
        };

        // Should be serializable
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("Test chunk"));
        assert!(json.contains("0.95"));
    }
}
