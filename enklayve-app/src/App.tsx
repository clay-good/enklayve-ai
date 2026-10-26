import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { WelcomeFlow } from "./components/Onboarding/WelcomeFlow";
import { ChatWindow } from "./components/Chat/ChatWindow";
import { ConversationList } from "./components/Sidebar/ConversationList";
import { DocumentPanel } from "./components/Documents/DocumentPanel";
import { UnlockScreen } from "./components/Security/UnlockScreen";
import { SecuritySettings } from "./components/Security/SecuritySettings";
import "./styles/design-tokens.css";
import "./App.css";

interface DocumentMetadata {
  id: number;
  file_name: string;
  file_path: string;
  file_type: string;
  upload_date: number;
  size_bytes: number;
  chunks_count: number;
}

interface OnboardingState {
  is_first_run: boolean;
  onboarding_completed: boolean;
  recommended_model_downloaded: boolean;
  first_launch_timestamp: number;
  completion_timestamp: number | null;
  security_enabled: boolean;
  biometric_enabled: boolean;
}

interface SecurityConfig {
  security_enabled: boolean;
  biometric_enabled: boolean;
  biometric_available: boolean;
}

interface ConversationSummary {
  id: number;
  title: string;
  created_at: number;
  updated_at: number;
  message_count: number;
}

interface Message {
  role: 'user' | 'assistant';
  content: string;
  isStreaming?: boolean;
}

interface OcrProgress {
  stage: string;
  message: string;
  progress: number;
}

interface DownloadedModelInfo {
  name: string;
  path: string;
  size_bytes: number;
  size_gb: number;
}

function App() {
  const [question, setQuestion] = useState<string>("");
  const [messages, setMessages] = useState<Message[]>([]);
  const [loading, setLoading] = useState(false);
  const [documents, setDocuments] = useState<DocumentMetadata[]>([]);
  const [selectedDocument, setSelectedDocument] = useState<DocumentMetadata | null>(null);
  const [conversations, setConversations] = useState<ConversationSummary[]>([]);
  const [filteredConversations, setFilteredConversations] = useState<ConversationSummary[]>([]);
  const [currentConversationId, setCurrentConversationId] = useState<number | null>(null);
  const [isDarkMode, setIsDarkMode] = useState(true);
  const [searchQuery, setSearchQuery] = useState<string>("");
  const [showOnboarding, setShowOnboarding] = useState(false);
  const [checkingOnboarding, setCheckingOnboarding] = useState(true);
  const [ocrProgress, setOcrProgress] = useState<OcrProgress | null>(null);
  const [modelPath, setModelPath] = useState<string | null>(null);

  // Security state
  const [isLocked, setIsLocked] = useState(false);
  const [securityEnabled, setSecurityEnabled] = useState(false);
  const [showSecuritySettings, setShowSecuritySettings] = useState(false);
  const [citationDoc, setCitationDoc] = useState<string | null>(null);
  const [citationPassages, setCitationPassages] = useState<string[]>([]);
  const [folderProgress, setFolderProgress] = useState<{ processed: number; total: number; current_file: string } | null>(null);

  useEffect(() => {
    checkOnboarding();
    loadDocuments();
    loadConversations();
    loadModelPath();

    // Properly handle async cleanup for event listener
    let unlistenFn: (() => void) | null = null;

    listen<OcrProgress>('ocr-progress', (event) => {
      setOcrProgress(event.payload);
      if (event.payload.stage === 'complete') {
        setTimeout(() => setOcrProgress(null), 3000);
      }
    }).then(fn => {
      unlistenFn = fn;
    });

    return () => {
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, []);

  async function checkOnboarding() {
    try {
      const state = await invoke<OnboardingState>("check_first_run");
      if (state.is_first_run && !state.onboarding_completed) {
        setShowOnboarding(true);
      } else {
        // Check if security is enabled and we need to show unlock screen
        const secConfig = await invoke<SecurityConfig>("get_security_config");
        setSecurityEnabled(secConfig.security_enabled);
        if (secConfig.security_enabled) {
          setIsLocked(true);
        }
      }
      setCheckingOnboarding(false);
    } catch (error) {
      console.error("Failed to check onboarding:", error);
      setCheckingOnboarding(false);
    }
  }

  const handleUnlock = () => {
    setIsLocked(false);
    // Load data after unlock
    loadDocuments();
    loadConversations();
    loadModelPath();
  };

  const handleSecuritySettingsClose = async () => {
    setShowSecuritySettings(false);
    // Refresh security status
    try {
      const secConfig = await invoke<SecurityConfig>("get_security_config");
      setSecurityEnabled(secConfig.security_enabled);
    } catch (error) {
      console.error("Failed to refresh security config:", error);
    }
  };

  const handleOnboardingComplete = async () => {
    setShowOnboarding(false);
    // Check if security was enabled during onboarding
    try {
      const secConfig = await invoke<SecurityConfig>("get_security_config");
      setSecurityEnabled(secConfig.security_enabled);
      // Don't lock after onboarding - user just set up their password
    } catch (error) {
      console.error("Failed to check security config after onboarding:", error);
    }
    loadDocuments();
    loadConversations();
    await loadModelPath();
  };

  async function loadDocuments() {
    try {
      const docs = await invoke<DocumentMetadata[]>("list_documents");
      setDocuments(docs);
    } catch (error) {
      console.error("Failed to load documents:", error);
    }
  }

  async function loadConversations() {
    try {
      const convs = await invoke<ConversationSummary[]>("list_conversations", { limit: 50 });
      setConversations(convs);
      setFilteredConversations(convs);
    } catch (error) {
      console.error("Failed to load conversations:", error);
    }
  }

  async function loadModelPath() {
    try {
      const models = await invoke<DownloadedModelInfo[]>("list_downloaded_models");
      if (models.length > 0) {
        setModelPath(models[0].path);

        try {
          await invoke("preload_model", { modelPath: models[0].path });
        } catch (preloadError) {
          console.error("Model preload failed:", preloadError);
        }
      } else {
      }
    } catch (error) {
      console.error("Failed to load model path:", error);
    }
  }

  async function searchConversations(query: string) {
    if (!query.trim()) {
      setFilteredConversations(conversations);
      return;
    }

    try {
      const results = await invoke<ConversationSummary[]>("search_conversations", {
        query: query,
        limit: 50
      });
      setFilteredConversations(results);
    } catch (error) {
      console.error("Failed to search conversations:", error);
      setFilteredConversations(conversations);
    }
  }

  async function updateConversationTitle(conversationId: number, newTitle: string) {
    try {
      await invoke("update_conversation_title", {
        conversationId: conversationId,
        title: newTitle
      });
      await loadConversations();
    } catch (error) {
      console.error("Failed to update conversation title:", error);
    }
  }

  async function createNewConversation() {
    try {
      // Stop any ongoing generation before creating new conversation
      if (loading) {
        await handleStopGeneration();
      }

      const newConvId = await invoke<number>("create_conversation", {
        title: "New Conversation",
      });
      setCurrentConversationId(newConvId);
      setMessages([]);
      await loadConversations();
    } catch (error) {
      console.error("Failed to create conversation:", error);
    }
  }

  async function switchConversation(convId: number) {
    try {
      // Stop any ongoing generation before switching
      if (loading) {
        await handleStopGeneration();
      }

      setCurrentConversationId(convId);
      const msgs = await invoke<Message[]>("get_conversation_messages", {
        conversationId: convId,
      });
      setMessages(msgs);
    } catch (error) {
      console.error("Failed to switch conversation:", error);
    }
  }

  async function handleAskQuestion() {
    if (!question.trim() || loading) return;

    const userQuestion = question;
    setQuestion("");
    setLoading(true);

    const userMessage: Message = { role: "user", content: userQuestion };
    setMessages((prev) => [...prev, userMessage]);

    const streamingMessage: Message = { role: "assistant", content: "", isStreaming: true };
    setMessages((prev) => [...prev, streamingMessage]);

    let accumulatedResponse = "";
    let unlisten: (() => void) | null = null;
    let unlistenComplete: (() => void) | null = null;

    try {
      let conversationId = currentConversationId;

      if (!conversationId) {
        conversationId = await invoke<number>("create_conversation", {
          title: userQuestion.slice(0, 50),
        });
        setCurrentConversationId(conversationId);
        await loadConversations();
      }

      await invoke("add_message", {
        conversationId: conversationId,
        role: "user",
        content: userQuestion,
      });

      unlisten = await listen<string>("llm-token", (event) => {
        accumulatedResponse += event.payload;
        setMessages((prev) => {
          const newMessages = [...prev];
          newMessages[newMessages.length - 1] = {
            role: "assistant",
            content: accumulatedResponse,
            isStreaming: true,
          };
          return newMessages;
        });
      });

      unlistenComplete = await listen<string>("llm-complete", (event) => {
        accumulatedResponse = event.payload;
        setMessages((prev) => {
          const newMessages = [...prev];
          newMessages[newMessages.length - 1] = {
            role: "assistant",
            content: accumulatedResponse,
            isStreaming: false,
          };
          return newMessages;
        });
      });

      await invoke<string>("query_documents_streaming", {
        question: userQuestion,
        modelPath: modelPath,
        conversationId: conversationId,
      });

      if (accumulatedResponse && accumulatedResponse.trim()) {
        await invoke("add_message", {
          conversationId: conversationId,
          role: "assistant",
          content: accumulatedResponse,
        });
      } else if (!accumulatedResponse || !accumulatedResponse.trim()) {
        // Handle empty response
        const emptyMessage = "I wasn't able to generate a response. Please try rephrasing your question or check if documents are uploaded.";
        setMessages((prev) => {
          const newMessages = [...prev];
          newMessages[newMessages.length - 1] = {
            role: "assistant",
            content: emptyMessage,
            isStreaming: false,
          };
          return newMessages;
        });
      }

      await loadConversations();
    } catch (error) {
      console.error("Failed to process question:", error);

      let errorText = "I encountered an error processing your question.";
      let errorCategory = "Unknown error";

      const errorMessage = typeof error === 'string' ? error :
                          (error && typeof error === 'object' && 'message' in error) ?
                          (error as Error).message : '';

      // Categorize errors for better user guidance
      if (errorMessage.includes('model') || errorMessage.includes('load')) {
        errorCategory = "Model Error";
        errorText = "Failed to load the AI model. Please ensure a model is downloaded and try again.";
      } else if (errorMessage.includes('memory') || errorMessage.includes('allocation')) {
        errorCategory = "Memory Error";
        errorText = "Not enough memory to process this request. Try closing other applications or asking a simpler question.";
      } else if (errorMessage.includes('document') || errorMessage.includes('retrieval')) {
        errorCategory = "Document Error";
        errorText = "Failed to retrieve relevant documents. Please check your uploaded documents and try again.";
      } else if (errorMessage.includes('timeout') || errorMessage.includes('timed out')) {
        errorCategory = "Timeout Error";
        errorText = "The request took too long to process. Try asking a simpler question or check if the model is loaded.";
      } else if (errorMessage) {
        errorText += `\n\nDetails: ${errorMessage}`;
      }

      setMessages((prev) => {
        const newMessages = [...prev];
        newMessages[newMessages.length - 1] = {
          role: "assistant",
          content: `**${errorCategory}**\n\n${errorText}`,
          isStreaming: false,
        };
        return newMessages;
      });

    } finally {
      if (unlisten) {
        unlisten();
      }
      if (unlistenComplete) {
        unlistenComplete();
      }
      setLoading(false);
    }
  }

  async function handleStopGeneration() {
    try {
      await invoke("stop_generation");
      // Wait a brief moment for generation to actually stop
      await new Promise(resolve => setTimeout(resolve, 100));
    } catch (error) {
      console.error("Failed to stop generation:", error);
    }
  }

  async function handleFolderUpload() {
    const selected = await open({ directory: true, multiple: false });
    if (!selected || typeof selected !== "string") return;

    setLoading(true);
    let unlisten: (() => void) | null = null;
    try {
      unlisten = await listen<{ processed: number; total: number; current_file: string }>(
        "folder-progress",
        (event) => setFolderProgress(event.payload)
      );

      const summary = await invoke<{
        ingested: string[];
        failed: { file_name: string; error: string }[];
        total: number;
      }>("upload_folder", { folderPath: selected });

      await loadDocuments();

      let msg = `Imported ${summary.ingested.length} of ${summary.total} supported file${summary.total === 1 ? "" : "s"} from the folder.`;
      if (summary.failed.length > 0) {
        const sample = summary.failed
          .slice(0, 5)
          .map((f) => `- ${f.file_name}: ${f.error}`)
          .join("\n");
        const more = summary.failed.length > 5 ? `\n...and ${summary.failed.length - 5} more.` : "";
        msg += `\n\n${summary.failed.length} file(s) could not be imported:\n${sample}${more}`;
      }
      setMessages((prev) => [...prev, { role: "assistant", content: msg }]);
    } catch (error) {
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: `Folder import failed.\n\n${String(error)}` },
      ]);
    } finally {
      if (unlisten) unlisten();
      setFolderProgress(null);
      setLoading(false);
    }
  }

  async function handleRepairIndex() {
    setLoading(true);
    try {
      const health = await invoke<{
        documents: number;
        chunks: number;
        invalid_embeddings: number;
        orphan_chunks: number;
        documents_without_chunks: number;
        healthy: boolean;
      }>("check_index_integrity");

      if (health.healthy) {
        setMessages((prev) => [
          ...prev,
          {
            role: "assistant",
            content: `Your search index is healthy: ${health.documents} document(s), ${health.chunks} chunk(s).`,
          },
        ]);
      } else {
        const result = await invoke<{ reembedded: number; orphans_removed: number }>("rebuild_index");
        await loadDocuments();
        setMessages((prev) => [
          ...prev,
          {
            role: "assistant",
            content: `Repaired the search index: re-embedded ${result.reembedded} chunk(s) and removed ${result.orphans_removed} orphaned chunk(s).`,
          },
        ]);
      }
    } catch (error) {
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: `Index check failed.\n\n${String(error)}` },
      ]);
    } finally {
      setLoading(false);
    }
  }

  async function handleDocumentUpload() {
    const selected = await open({
      multiple: false,
      filters: [
        {
          name: "Documents & Images",
          extensions: ["pdf", "txt", "docx", "md", "jpg", "jpeg", "png"],
        },
      ],
    });

    if (selected && typeof selected === "string") {
      setLoading(true);
      try {
        await invoke<DocumentMetadata>("upload_document", {
          filePath: selected,
        });
        await loadDocuments();
      } catch (error) {
        console.error("Failed to upload document:", error);

        // Show error message in chat
        let errorText = "Failed to upload document.";
        if (typeof error === 'string') {
          errorText += `\n\n${error}`;
        } else if (error && typeof error === 'object' && 'message' in error) {
          errorText += `\n\n${(error as Error).message}`;
        }

        const errorMessage: Message = {
          role: "assistant",
          content: errorText,
        };
        setMessages((prev) => [...prev, errorMessage]);
      } finally {
        setLoading(false);
      }
    }
  }

  async function handleDocumentDelete(documentId: number) {
    try {
      await invoke("delete_document", { documentId });
      if (selectedDocument?.id === documentId) {
        setSelectedDocument(null);
      }
      await loadDocuments();
    } catch (error) {
      console.error("Failed to delete document:", error);
    }
  }

  async function handleConversationDelete(conversationId: number) {
    try {
      await invoke("delete_conversation", { conversationId });
      if (currentConversationId === conversationId) {
        setCurrentConversationId(null);
        setMessages([]);
      }
      await loadConversations();
    } catch (error) {
      console.error("Failed to delete conversation:", error);
    }
  }

  function toggleTheme() {
    setIsDarkMode(!isDarkMode);
    document.documentElement.classList.toggle('light-mode');
  }


  if (checkingOnboarding) {
    return (
      <div className="app loading-screen">
        <div className="loading-content">
          <div className="spinner"></div>
          <p>Loading Enklayve...</p>
        </div>
      </div>
    );
  }

  if (showOnboarding) {
    return <WelcomeFlow onComplete={handleOnboardingComplete} />;
  }

  if (isLocked) {
    return <UnlockScreen onUnlock={handleUnlock} />;
  }

  const handleOpenCitation = async (fileName: string) => {
    try {
      const passages = await invoke<string[]>("get_document_passages", { fileName });
      setCitationDoc(fileName);
      setCitationPassages(passages);
    } catch (error) {
      console.error("Failed to load citation passages:", error);
      setCitationDoc(fileName);
      setCitationPassages([]);
    }
  };

  const handleBackupComplete = (success: boolean, message: string) => {
    if (success && message.includes('restored')) {
      loadDocuments();
      loadConversations();
    }
  };

  return (
    <div className="app">
      {showSecuritySettings && (
        <SecuritySettings onClose={handleSecuritySettingsClose} />
      )}

      {citationDoc && (
        <div className="citation-modal-overlay" onClick={() => setCitationDoc(null)}>
          <div className="citation-modal" onClick={(e) => e.stopPropagation()}>
            <div className="citation-modal-header">
              <h3>{citationDoc}</h3>
              <button
                className="citation-modal-close"
                aria-label="Close"
                onClick={() => setCitationDoc(null)}
              >
                ×
              </button>
            </div>
            <div className="citation-modal-body">
              {citationPassages.length === 0 ? (
                <p className="citation-empty">
                  No passages found for this document. It may have been removed.
                </p>
              ) : (
                citationPassages.map((passage, i) => (
                  <p key={i} className="citation-passage">
                    {passage}
                  </p>
                ))
              )}
            </div>
          </div>
        </div>
      )}

      <aside className="sidebar">
        <div className="sidebar-top">
          <ConversationList
            conversations={filteredConversations}
            currentConversationId={currentConversationId}
            searchQuery={searchQuery}
            onSearchChange={(query) => {
              setSearchQuery(query);
              searchConversations(query);
            }}
            onConversationSelect={switchConversation}
            onConversationDelete={handleConversationDelete}
            onConversationTitleUpdate={updateConversationTitle}
            onNewConversation={createNewConversation}
            isDarkMode={isDarkMode}
            onThemeToggle={toggleTheme}
            onBackupComplete={handleBackupComplete}
            onSecuritySettings={() => setShowSecuritySettings(true)}
            securityEnabled={securityEnabled}
          />
        </div>

        <div className="sidebar-bottom">
          <DocumentPanel
            documents={documents}
            selectedDocument={selectedDocument}
            highlightedDocument={null}
            ocrProgress={ocrProgress}
            onDocumentSelect={setSelectedDocument}
            onDocumentDelete={handleDocumentDelete}
            onUpload={handleDocumentUpload}
            onUploadFolder={handleFolderUpload}
            onRepairIndex={handleRepairIndex}
            folderProgress={folderProgress}
          />
        </div>
      </aside>

      <main className="main-content">
        <ChatWindow
          messages={messages}
          question={question}
          loading={loading}
          onQuestionChange={setQuestion}
          onSubmit={handleAskQuestion}
          onStop={handleStopGeneration}
          onOpenCitation={handleOpenCitation}
        />
      </main>
    </div>
  );
}

export default App;
