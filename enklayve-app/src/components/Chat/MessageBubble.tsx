import { useMemo } from 'react';
import ReactMarkdown from 'react-markdown';
import './MessageBubble.css';

interface MessageBubbleProps {
  role: 'user' | 'assistant';
  content: string;
  isStreaming?: boolean;
  onOpenCitation?: (fileName: string) => void;
  onSave?: () => void;
}

// Matches a bracketed file name the model used as a citation, e.g. [report.pdf].
const CITATION_RE = /\[([^\]\n]+?\.(?:pdf|docx|txt|md|csv|xlsx|xls|html?|png|jpe?g))\]/gi;

export function MessageBubble({ role, content, isStreaming = false, onOpenCitation, onSave }: MessageBubbleProps) {
  // Turn [file.ext] citations into links the Markdown renderer can intercept.
  const prepared = useMemo(
    () => content.replace(CITATION_RE, (_m, name) => `[${name}](ek-cite:${encodeURIComponent(name)})`),
    [content]
  );

  return (
    <div className={`message-bubble ${role}`}>
      <div className="message-bubble-content">
        {role === 'assistant' ? (
          <ReactMarkdown
            components={{
              a({ href, children }) {
                if (href && href.startsWith('ek-cite:')) {
                  const name = decodeURIComponent(href.slice('ek-cite:'.length));
                  return (
                    <button
                      type="button"
                      className="citation-link"
                      title={`Show the passage from ${name}`}
                      onClick={() => onOpenCitation?.(name)}
                    >
                      {children}
                    </button>
                  );
                }
                return (
                  <a href={href} target="_blank" rel="noreferrer">
                    {children}
                  </a>
                );
              },
            }}
          >
            {prepared}
          </ReactMarkdown>
        ) : (
          <span>{content}</span>
        )}
        {isStreaming && <span className="cursor-blink">|</span>}
      </div>
      {role === 'assistant' && !isStreaming && onSave && content.trim() && (
        <button
          type="button"
          className="message-save-button"
          title="Save this answer to your working folder"
          onClick={onSave}
        >
          Save to folder
        </button>
      )}
    </div>
  );
}
