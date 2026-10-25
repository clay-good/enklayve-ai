# Ask Your Data (Retrieval)

## Purpose

The core promise: ask anything about your documents and get a grounded answer
with citations you can trust. This is retrieval-augmented generation done well —
good chunking, good retrieval, reranking, and honest grounding.

## Requirements

### Requirement: The app SHALL answer questions grounded in the user's indexed documents

Answers SHALL be generated from retrieved passages, not the model's parametric
memory alone, whenever relevant documents exist.

#### Scenario: Question with a clear answer in the docs
- **WHEN** the user asks a question whose answer is in an indexed document
- **THEN** the app retrieves the relevant passages, generates an answer from
  them, and the answer reflects the document content

### Requirement: Every grounded claim SHALL carry a citation to its source

Answers SHALL cite the source document and location (page or chunk), and the user
SHALL be able to open the cited passage.

#### Scenario: Citation opens source
- **WHEN** an answer cites "contract.pdf, p.4"
- **THEN** clicking the citation reveals the exact passage the answer drew from

### Requirement: Retrieval SHALL rank and rerank for relevance

The app SHALL use vector search over embeddings plus a reranking pass to select
the passages sent to the model, within the model's context budget.

#### Scenario: Large corpus, focused answer
- **WHEN** the corpus is large and only a few passages are relevant
- **THEN** reranking surfaces those passages and the answer stays focused rather
  than diluted by near-miss chunks

### Requirement: The app SHALL be honest when the answer is not in the data

When retrieval finds no sufficiently relevant passages, the app SHALL say so
rather than fabricate, and MAY offer to answer from the model's general knowledge
(clearly labeled as not from the user's documents) or to search the web if the
user has enabled web access (see `agent-tools`).

#### Scenario: Answer not present
- **WHEN** nothing relevant is retrieved
- **THEN** the app states that the documents don't cover it, and offers the
  labeled fallbacks rather than inventing a cited-looking answer

### Requirement: The app SHALL support chatting with no documents at all

The user SHALL be able to just chat with the local model without uploading
anything; retrieval simply contributes nothing in that case.

#### Scenario: Chat-only use
- **WHEN** the user has indexed no documents and asks a general question
- **THEN** the model answers directly, and (if enabled) may use web access
