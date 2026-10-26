import { useEffect, useRef } from 'react';
import { MessageBubble } from './MessageBubble';
import './ChatWindow.css';

interface Message {
  role: 'user' | 'assistant';
  content: string;
  isStreaming?: boolean;
}

interface ChatWindowProps {
  messages: Message[];
  question: string;
  loading: boolean;
  onQuestionChange: (value: string) => void;
  onSubmit: () => void;
  onStop: () => void;
  onOpenCitation?: (fileName: string) => void;
  onSaveMessage?: (content: string) => void;
  webSearching?: boolean;
}

export function ChatWindow({
  messages,
  question,
  loading,
  onQuestionChange,
  onSubmit,
  onStop,
  onOpenCitation,
  onSaveMessage,
  webSearching = false,
}: ChatWindowProps) {
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const chatMessagesRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Grow the input with the prompt, up to a cap, then scroll inside it.
  useEffect(() => {
    const el = textareaRef.current;
    if (el) {
      el.style.height = 'auto';
      el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
    }
  }, [question]);

  useEffect(() => {
    // Only auto-scroll if user is at or near the bottom
    if (chatMessagesRef.current) {
      const { scrollTop, scrollHeight, clientHeight } = chatMessagesRef.current;
      const isNearBottom = scrollHeight - scrollTop - clientHeight < 100;

      if (isNearBottom) {
        messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
      }
    }
  }, [messages]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // Submit on Enter (but allow Shift+Enter for new lines)
    if (e.key === 'Enter' && !e.shiftKey && !loading && question.trim()) {
      e.preventDefault(); // Prevent double submission
      onSubmit();
    }
  };

  return (
    <div className="chat-window">
      <div className="chat-messages" ref={chatMessagesRef}>
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
          </div>
        ) : (
          <>
            {messages.map((msg, idx) => (
              <MessageBubble
                key={idx}
                role={msg.role}
                content={msg.content}
                isStreaming={msg.isStreaming || false}
                onOpenCitation={onOpenCitation}
                onSave={
                  msg.role === 'assistant' && onSaveMessage
                    ? () => onSaveMessage(msg.content)
                    : undefined
                }
              />
            ))}
            {loading && (
              <div className="loading-message">
                <div className="loading-dots">
                  <span></span>
                  <span></span>
                  <span></span>
                </div>
                <span className="loading-text">{webSearching ? 'Searching the web…' : 'Processing locally...'}</span>
              </div>
            )}
            <div ref={messagesEndRef} />
          </>
        )}
      </div>

      <div className="chat-input-area">
        <textarea
          ref={textareaRef}
          className="chat-input"
          placeholder="Ask a question... (Shift+Enter for new line)"
          value={question}
          onChange={(e) => onQuestionChange(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={loading}
          rows={1}
          style={{ resize: 'none', maxHeight: '200px', overflowY: 'auto' }}
        />
        {loading ? (
          <button
            className="chat-stop-button"
            onClick={onStop}
          >
            Stop
          </button>
        ) : (
          <button
            className="chat-submit-button"
            onClick={onSubmit}
            disabled={!question.trim()}
          >
            Send
          </button>
        )}
      </div>
    </div>
  );
}
