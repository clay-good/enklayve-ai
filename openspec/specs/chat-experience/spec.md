# Chat Experience

## Purpose

Chat must feel calm, consistent, and fast. Today's output is "weirdly formatted
and not uniform" — the root cause is that prompts are sent to the model without
its chat template, so the model never reliably produces clean markdown. This
capability fixes generation correctness and rendering consistency together.

## Requirements

### Requirement: Prompts SHALL be wrapped in the active model's chat template

The app SHALL format every turn using the model's own chat template (e.g. Qwen's
`<|im_start|>role ... <|im_end|>`), including a system prompt, prior turns, and
retrieved context, before tokenization — never a raw concatenated string.

#### Scenario: Qwen-class model
- **WHEN** a Qwen3-class model is active and the user sends a message
- **THEN** the prompt is assembled with that model's chat template and special
  tokens, and generation stops cleanly at the model's end-of-turn token
- **AND** the model's output is coherent, complete markdown rather than leaking
  template tokens or trailing fragments

### Requirement: A system prompt SHALL enforce consistent answer formatting

The system prompt SHALL instruct the model to produce well-structured markdown
(short paragraphs, lists where appropriate, fenced code only for code) so answers
look uniform across questions.

#### Scenario: Two similar questions
- **WHEN** the user asks two comparable questions in a session
- **THEN** both answers follow the same formatting conventions

### Requirement: Streamed output SHALL render as stable, well-formed markdown

Tokens SHALL stream live, and the rendered markdown SHALL remain stable as it
grows — no flicker, no broken half-parsed syntax, no raw special tokens shown.

#### Scenario: Streaming a list
- **WHEN** the model streams a markdown list
- **THEN** the UI renders it incrementally without reflowing earlier items or
  flashing raw markdown syntax

#### Scenario: Incomplete code fence mid-stream
- **WHEN** a code block is still streaming and its closing fence hasn't arrived
- **THEN** the renderer degrades gracefully (treats it as code-in-progress)
  rather than mis-rendering the rest of the message

### Requirement: Conversations SHALL persist and be resumable

Sessions SHALL be saved locally (encrypted), listed in a sidebar, and resumable
with full history and citations intact.

#### Scenario: Reopen a past conversation
- **WHEN** the user reopens a saved conversation
- **THEN** the full message history and its citations are restored
