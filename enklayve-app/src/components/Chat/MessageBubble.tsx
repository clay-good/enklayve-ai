import { useMemo, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import rehypeHighlight from 'rehype-highlight';
import './MessageBubble.css';

const copyIconProps = {
  width: 14,
  height: 14,
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};
const IconCopy = () => (
  <svg {...copyIconProps}><rect x="9" y="9" width="11" height="11" rx="2" /><path d="M5 15V5a2 2 0 0 1 2-2h10" /></svg>
);
const IconCheck = () => (
  <svg {...copyIconProps}><path d="M20 6 9 17l-5-5" /></svg>
);

interface MessageBubbleProps {
  role: 'user' | 'assistant';
  content: string;
  isStreaming?: boolean;
  timestamp?: number;
  onOpenCitation?: (fileName: string) => void;
}

// Matches a bracketed file name the model used as a citation, e.g. [report.pdf].
const CITATION_RE = /\[([^\]\n]+?\.(?:pdf|docx|txt|md|csv|xlsx|xls|html?|png|jpe?g))\]/gi;

// A fenced code block with its own copy button, used by the Markdown renderer.
function CodeBlock({ children }: { children?: React.ReactNode }) {
  const preRef = useRef<HTMLPreElement>(null);
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(preRef.current?.innerText ?? '');
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (error) {
      console.error('Copy failed:', error);
    }
  };
  return (
    <div className="code-block">
      <button className="code-copy-button" onClick={copy} aria-label="Copy code">
        {copied ? <IconCheck /> : <IconCopy />}
        <span>{copied ? 'Copied' : 'Copy'}</span>
      </button>
      <pre ref={preRef}>{children}</pre>
    </div>
  );
}

function formatTime(ts: number): string {
  return new Date(ts * 1000).toLocaleString([], {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  });
}

export function MessageBubble({ role, content, timestamp, onOpenCitation }: MessageBubbleProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(content);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (error) {
      console.error('Copy failed:', error);
    }
  };

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
            rehypePlugins={[[rehypeHighlight, { detect: true, ignoreMissing: true }]]}
            components={{
              pre({ children }) {
                return <CodeBlock>{children}</CodeBlock>;
              },
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
      </div>
      {content.trim() && (
        <div className="message-meta">
          <button
            className="message-copy-button"
            onClick={handleCopy}
            title={copied ? 'Copied' : 'Copy'}
            aria-label="Copy message"
          >
            {copied ? <IconCheck /> : <IconCopy />}
            <span>{copied ? 'Copied' : 'Copy'}</span>
          </button>
          {timestamp ? <span className="message-time">{formatTime(timestamp)}</span> : null}
        </div>
      )}
    </div>
  );
}
