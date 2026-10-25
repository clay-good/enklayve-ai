# Chat Experience (delta)

## ADDED Requirements

### Requirement: Prompts SHALL be wrapped in the active model's chat template

The app SHALL format every turn using the active model's chat template, with a
system prompt, prior turns, and retrieved context, before tokenization.

#### Scenario: Template-wrapped generation
- **WHEN** the user sends a message to a Qwen3-class model
- **THEN** the prompt is assembled via the model's ChatML template and generation
  stops cleanly at the end-of-turn token with no special tokens in the output

### Requirement: A system prompt SHALL enforce consistent answer formatting

#### Scenario: Uniform answers
- **WHEN** the user asks two comparable questions
- **THEN** both answers follow the same markdown conventions

## MODIFIED Requirements

### Requirement: Streamed output SHALL render as stable, well-formed markdown

Tokens SHALL stream live and the rendered markdown SHALL remain stable as it
grows, with no flicker, no raw special tokens, and graceful handling of an
unclosed code fence mid-stream.

#### Scenario: Streaming a list
- **WHEN** the model streams a markdown list
- **THEN** it renders incrementally without reflowing earlier items
