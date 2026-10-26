import { useEffect, useRef, useState } from 'react';
import { MessageBubble } from './MessageBubble';
import './ChatWindow.css';

const svgProps = {
  width: 18,
  height: 18,
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};
const IconPaperclip = () => (
  <svg {...svgProps}><path d="M21.4 11.1l-8.5 8.5a5 5 0 0 1-7-7l8.5-8.5a3 3 0 0 1 4.3 4.3l-8.5 8.5a1 1 0 0 1-1.4-1.4l7.8-7.8" /></svg>
);
const IconFolderPlus = () => (
  <svg {...svgProps}><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /><path d="M12 11v4M10 13h4" /></svg>
);
const IconFile = () => (
  <svg {...svgProps} width={14} height={14}><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6" /></svg>
);
const IconClose = () => (
  <svg {...svgProps} width={13} height={13}><path d="M18 6 6 18M6 6l12 12" /></svg>
);
const IconArrowDown = () => (
  <svg {...svgProps} width={16} height={16}><path d="M12 5v14M19 12l-7 7-7-7" /></svg>
);

const STARTER_PROMPTS = [
  'What can Enklayve do for me?',
  'Summarize the documents I add to this chat',
  'Help me draft a professional email',
  'Explain a complex topic in simple terms',
];

interface ChatDoc {
  id: number;
  file_name: string;
  chunks_count: number;
}

interface Message {
  role: 'user' | 'assistant';
  content: string;
  isStreaming?: boolean;
  timestamp?: number;
}

interface ChatWindowProps {
  messages: Message[];
  question: string;
  loading: boolean;
  documents?: ChatDoc[];
  folderProgress?: { processed: number; total: number; current_file: string } | null;
  ocrProgress?: { stage: string; message: string; progress: number } | null;
  onQuestionChange: (value: string) => void;
  onSubmit: () => void;
  onStarterPrompt?: (text: string) => void;
  onStop: (clearQueue?: boolean) => void;
  hasQueue?: boolean;
  onOpenCitation?: (fileName: string) => void;
  onAttachFile?: () => void;
  onAttachFolder?: () => void;
  onRemoveDocument?: (id: number) => void;
  webSearching?: boolean;
  queued?: boolean;
  busy?: boolean;
}

export function ChatWindow({
  messages,
  question,
  loading,
  documents = [],
  folderProgress,
  ocrProgress,
  onQuestionChange,
  onSubmit,
  onStarterPrompt,
  onStop,
  hasQueue = false,
  onOpenCitation,
  onAttachFile,
  onAttachFolder,
  onRemoveDocument,
  webSearching = false,
  queued = false,
  busy = false,
}: ChatWindowProps) {
  const chatMessagesRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const [attachOpen, setAttachOpen] = useState(false);
  const [showJump, setShowJump] = useState(false);

  // Grow the input with the prompt, up to a cap, then scroll inside it.
  useEffect(() => {
    const el = textareaRef.current;
    if (el) {
      el.style.height = 'auto';
      el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
    }
  }, [question]);

  // Auto-track the latest message/answer: keep the view pinned to the bottom
  // while the user is at the bottom, and stop the moment they scroll up.
  const stickRef = useRef(true);

  const handleMessagesScroll = () => {
    const el = chatMessagesRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
    stickRef.current = atBottom;
    setShowJump((prev) => (prev === !atBottom ? prev : !atBottom));
  };

  useEffect(() => {
    const el = chatMessagesRef.current;
    if (el && stickRef.current) {
      el.scrollTop = el.scrollHeight;
      setShowJump(false);
    }
  }, [messages, loading]);

  const scrollToLatest = () => {
    const el = chatMessagesRef.current;
    if (el) el.scrollTop = el.scrollHeight;
    stickRef.current = true;
    setShowJump(false);
  };

  const submit = () => {
    stickRef.current = true; // sending always jumps to the latest
    onSubmit();
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // Submit on Enter (but allow Shift+Enter for new lines)
    if (e.key === 'Enter' && !e.shiftKey && question.trim()) {
      e.preventDefault(); // Prevent double submission
      submit();
    }
  };

  return (
    <div className="chat-window">
      <div className="chat-messages" ref={chatMessagesRef} onScroll={handleMessagesScroll}>
        {messages.length === 0 ? (
          <div className="empty-state">
            <h2>Welcome to Enklayve</h2>
            <p>Your local, private, and secure AI assistant.</p>
            <ul className="feature-list">
              <li>Chat freely - no documents required</li>
              <li>Upload documents (PDF, DOCX, TXT, MD) for intelligent Q&A</li>
              <li>Local processing - your documents NEVER leave your device</li>
              <li>Works offline; optional web search sends only your question, never your documents</li>
              <li>No telemetry, no tracking, no API keys - private and free forever</li>
            </ul>
            <div className="starter-prompts">
              {STARTER_PROMPTS.map((p) => (
                <button
                  key={p}
                  type="button"
                  className="starter-prompt"
                  onClick={() => onStarterPrompt?.(p)}
                >
                  {p}
                </button>
              ))}
            </div>
          </div>
        ) : (
          <>
            {messages.map((msg, idx) => (
              <MessageBubble
                key={idx}
                role={msg.role}
                content={msg.content}
                timestamp={msg.timestamp}
                onOpenCitation={onOpenCitation}
              />
            ))}
            {loading && (
              <div className="generating-indicator" aria-live="polite">
                <span className="generating-dots">
                  <span></span>
                  <span></span>
                  <span></span>
                </span>
                <span className="generating-text">
                  {webSearching ? 'Searching the web…' : 'Generating answer…'}
                </span>
              </div>
            )}
          </>
        )}
      </div>

      {showJump && messages.length > 0 && (
        <button className="jump-to-latest" onClick={scrollToLatest} aria-label="Jump to latest">
          <IconArrowDown />
          <span>Latest</span>
        </button>
      )}

      <div className="chat-input-area">
        {documents.length > 0 && (
          <div className="composer-docs">
            {documents.map((doc) => (
              <span className="doc-chip" key={doc.id} title={`${doc.chunks_count} chunks`}>
                <IconFile />
                <span className="doc-chip-name">{doc.file_name}</span>
                {onRemoveDocument && (
                  <button
                    className="doc-chip-remove"
                    onClick={() => onRemoveDocument(doc.id)}
                    aria-label={`Remove ${doc.file_name}`}
                  >
                    <IconClose />
                  </button>
                )}
              </span>
            ))}
          </div>
        )}

        {folderProgress && folderProgress.total > 0 && folderProgress.processed < folderProgress.total && (
          <div className="composer-progress">
            Indexing {folderProgress.processed + 1} of {folderProgress.total}
            {folderProgress.current_file ? `: ${folderProgress.current_file}` : ''}
          </div>
        )}

        {ocrProgress && (
          <div className="composer-progress">
            {ocrProgress.message} ({ocrProgress.progress}%)
          </div>
        )}

        {queued && (
          <div className="composer-progress">Queued — will answer after the current one finishes…</div>
        )}

        <div className="composer-row">
          <div className="attach-wrap">
            <button
              className="attach-button"
              onClick={() => setAttachOpen((v) => !v)}
              disabled={loading || busy}
              title="Attach a file or folder to this chat"
              aria-label="Attach"
            >
              <IconPaperclip />
            </button>
            {attachOpen && (
              <>
                <div className="attach-backdrop" onClick={() => setAttachOpen(false)} />
                <div className="attach-menu">
                  <button
                    className="attach-menu-item"
                    onClick={() => { setAttachOpen(false); onAttachFile?.(); }}
                  >
                    <IconFile /> Add file
                  </button>
                  <button
                    className="attach-menu-item"
                    onClick={() => { setAttachOpen(false); onAttachFolder?.(); }}
                  >
                    <IconFolderPlus /> Add folder
                  </button>
                </div>
              </>
            )}
          </div>
          <textarea
            ref={textareaRef}
            className="chat-input"
            placeholder="Ask anything, or attach files to ask about them…"
            value={question}
            onChange={(e) => onQuestionChange(e.target.value)}
            onKeyDown={handleKeyDown}
            rows={1}
            style={{ resize: 'none', maxHeight: '200px', overflowY: 'auto' }}
          />
          {loading ? (
            <div className="stop-buttons">
              <button className="chat-stop-button" onClick={() => onStop(false)}>
                Stop
              </button>
              {hasQueue && (
                <button className="chat-stop-all-button" onClick={() => onStop(true)} title="Stop and clear the queue">
                  Stop all
                </button>
              )}
            </div>
          ) : (
            <button
              className="chat-submit-button"
              onClick={submit}
              disabled={!question.trim()}
            >
              Send
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
