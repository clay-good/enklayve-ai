import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { open, ask } from "@tauri-apps/plugin-dialog";
import { WelcomeFlow } from "./components/Onboarding/WelcomeFlow";
import { ChatWindow } from "./components/Chat/ChatWindow";
import { ConversationList } from "./components/Sidebar/ConversationList";
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
  timestamp?: number;
}

const nowSec = () => Math.floor(Date.now() / 1000);

interface OcrProgress {
  stage: string;
  message: string;
  progress: number;
}

interface ActiveModel {
  path: string;
  name: string;
  size_gb: number;
  performance_tier: string;
  context_length: number;
  auto_selected: boolean;
  note: string;
}

interface ModelOption {
  name: string;
  file_name: string;
  repo_url: string;
  size_gb: number;
  performance_tier: string;
  speed_estimate: string;
  downloaded: boolean;
  active: boolean;
  recommended: boolean;
}

function App() {
  const [question, setQuestion] = useState<string>("");
  const [messages, setMessages] = useState<Message[]>([]);
  const [documents, setDocuments] = useState<DocumentMetadata[]>([]);
  const [selectedDocument, setSelectedDocument] = useState<DocumentMetadata | null>(null);
  const [conversations, setConversations] = useState<ConversationSummary[]>([]);
  const [filteredConversations, setFilteredConversations] = useState<ConversationSummary[]>([]);
  const [archivedConversations, setArchivedConversations] = useState<ConversationSummary[]>([]);
  const [archivedSearchResults, setArchivedSearchResults] = useState<ConversationSummary[]>([]);
  const [currentConversationId, setCurrentConversationId] = useState<number | null>(null);
  const [isDarkMode, setIsDarkMode] = useState(true);
  const [searchQuery, setSearchQuery] = useState<string>("");
  const [showOnboarding, setShowOnboarding] = useState(false);
  const [checkingOnboarding, setCheckingOnboarding] = useState(true);
  const [ocrProgress, setOcrProgress] = useState<OcrProgress | null>(null);
  const [modelPath, setModelPath] = useState<string | null>(null);
  const [activeModel, setActiveModel] = useState<ActiveModel | null>(null);
  const [modelOptions, setModelOptions] = useState<ModelOption[]>([]);
  const [downloadingModel, setDownloadingModel] = useState<{ fileName: string; percent: number } | null>(null);
  const [showAbout, setShowAbout] = useState(false);
  const [appVersion, setAppVersion] = useState<string>("");

  // Security state
  const [isLocked, setIsLocked] = useState(false);
  const [securityEnabled, setSecurityEnabled] = useState(false);
  const [showSecuritySettings, setShowSecuritySettings] = useState(false);
  const [citationDoc, setCitationDoc] = useState<string | null>(null);
  const [citationPassages, setCitationPassages] = useState<string[]>([]);
  const [folderProgress, setFolderProgress] = useState<{ processed: number; total: number; current_file: string } | null>(null);
  // Which chat an in-progress import belongs to, so its "indexing" banner only
  // shows in that chat and never bleeds into a new or different one.
  const [importConvId, setImportConvId] = useState<number | null>(null);
  // Absolute start times (epoch ms) so the elapsed clocks keep counting from the
  // real start even after switching chats and coming back.
  const [genStartedAt, setGenStartedAt] = useState<number | null>(null);
  const [importStartedAt, setImportStartedAt] = useState<number | null>(null);
  const [webAccess, setWebAccess] = useState<string>("unasked");
  const [webSearching, setWebSearching] = useState(false);
  // Which conversation (if any) is currently generating, and which one is on
  // screen — so "working" only shows in its own chat, not everywhere.
  const [generatingConvId, setGeneratingConvId] = useState<number | null>(null);
  const [queuedConvIds, setQueuedConvIds] = useState<number[]>([]);
  const viewConvRef = useRef<number | null>(null);
  const genConvRef = useRef<number | null>(null);
  const queueRef = useRef<Array<{ convId: number; question: string }>>([]);
  const processingRef = useRef(false);
  // Holds the answer as it accumulates, so Stop can keep whatever was produced.
  const accumulatedRef = useRef("");
  const stopRequestedRef = useRef(false);
  useEffect(() => {
    viewConvRef.current = currentConversationId;
  }, [currentConversationId]);

  // Show a finished answer at once, but only in its own chat if that chat is
  // the one on screen.
  function appendAssistantToView(convId: number, content: string) {
    if (viewConvRef.current !== convId) return;
    setMessages((prev) => [...prev, { role: "assistant", content, timestamp: nowSec() }]);
  }

  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => {});
    invoke<string>("get_web_access").then(setWebAccess).catch(() => {});
    // Auto-repair on launch: WAL recovers interrupted commits on open, then we
    // quietly drop orphan chunks and re-embed any corrupt embeddings. It only
    // speaks up if it actually had to fix something.
    invoke<{
      integrity_ok: boolean;
      orphan_chunks_removed: number;
      embeddings_repaired: number;
    }>("run_startup_recovery")
      .then((r) => {
        if (!r) return;
        const fixed = r.orphan_chunks_removed + r.embeddings_repaired;
        if (fixed > 0) {
          loadDocuments();
          setMessages((prev) => [
            ...prev,
            {
              role: "assistant",
              content: `Tidied up the search index on startup (repaired ${fixed} item${fixed === 1 ? "" : "s"}).`,
            },
          ]);
        } else if (!r.integrity_ok) {
          setMessages((prev) => [
            ...prev,
            {
              role: "assistant",
              content:
                "The local database reported an integrity problem on startup. Your documents are safe; re-importing any affected files will rebuild their index.",
            },
          ]);
        }
      })
      .catch(() => {});
    let unlistenWeb: (() => void) | null = null;
    listen("web-search", () => setWebSearching(true)).then((fn) => {
      unlistenWeb = fn;
    });
    return () => {
      if (unlistenWeb) unlistenWeb();
    };
  }, []);

  const handleSetWeb = async (value: "enabled" | "disabled") => {
    try {
      await invoke("set_web_access", { value });
    } catch (error) {
      console.error("Failed to set web access:", error);
    }
    setWebAccess(value);
  };

  const handleToggleWeb = () => {
    handleSetWeb(webAccess === "enabled" ? "disabled" : "enabled");
  };

  // Keyboard shortcuts: ⌘/Ctrl+N new chat, ⌘/Ctrl+K focus search, Esc stops.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.key.toLowerCase() === "n") {
        e.preventDefault();
        createNewConversation();
      } else if (mod && e.key.toLowerCase() === "k") {
        e.preventDefault();
        document.getElementById("conversation-search")?.focus();
      } else if (e.key === "Escape" && generatingConvId !== null) {
        e.preventDefault();
        handleStopGeneration();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [generatingConvId]);

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

  // Documents are scoped to a conversation; with no conversation there are none.
  async function loadDocuments(convId: number | null = null) {
    if (!convId) {
      setDocuments([]);
      return;
    }
    try {
      const docs = await invoke<DocumentMetadata[]>("list_documents", { conversationId: convId });
      setDocuments(docs);
    } catch (error) {
      console.error("Failed to load documents:", error);
      setDocuments([]);
    }
  }

  // Return the current conversation, creating one if the user hasn't started yet.
  async function ensureConversation(): Promise<number> {
    if (currentConversationId) return currentConversationId;
    const newId = await invoke<number>("create_conversation", { title: "New Conversation" });
    setCurrentConversationId(newId);
    await loadConversations();
    return newId;
  }

  async function loadConversations() {
    try {
      const convs = await invoke<ConversationSummary[]>("list_conversations", { limit: 50 });
      setConversations(convs);
      setFilteredConversations(convs);
      await loadArchivedConversations();
    } catch (error) {
      console.error("Failed to load conversations:", error);
    }
  }

  async function loadArchivedConversations() {
    try {
      const archived = await invoke<ConversationSummary[]>("list_archived_conversations");
      setArchivedConversations(archived);
    } catch (error) {
      console.error("Failed to load archived conversations:", error);
    }
  }

  // The app auto-selects a sensible model for this machine, but the user can
  // switch anytime from the model picker. We load the active model and preload it.
  async function loadModelPath() {
    try {
      const active = await invoke<ActiveModel | null>("get_active_model");
      if (active) {
        setActiveModel(active);
        setModelPath(active.path);
        try {
          await invoke("preload_model", { modelPath: active.path });
        } catch (preloadError) {
          console.error("Model preload failed:", preloadError);
        }
      }
    } catch (error) {
      console.error("Failed to load model path:", error);
    }
    loadModelOptions();
  }

  async function loadModelOptions() {
    try {
      const options = await invoke<ModelOption[]>("list_model_options");
      setModelOptions(options);
    } catch (error) {
      console.error("Failed to load model options:", error);
    }
  }

  // Switch the active model. If it isn't downloaded yet, download it first
  // (showing progress), then make it active, load and preload it.
  async function switchModel(option: ModelOption) {
    if (downloadingModel) return;
    try {
      if (!option.downloaded) {
        setDownloadingModel({ fileName: option.file_name, percent: 0 });
        const unlisten = await listen<{ percentage: number }>("download-progress", (e) => {
          setDownloadingModel({ fileName: option.file_name, percent: e.payload.percentage });
        });
        try {
          await invoke("download_model", {
            url: option.repo_url,
            modelName: option.file_name,
            checksum: null,
          });
        } finally {
          unlisten();
          setDownloadingModel(null);
        }
      }
      await invoke("set_active_model", { fileName: option.file_name });
      const active = await invoke<ActiveModel | null>("get_active_model");
      if (active) {
        setActiveModel(active);
        setModelPath(active.path);
        invoke("preload_model", { modelPath: active.path }).catch((e) =>
          console.error("Model preload failed:", e)
        );
      }
      await loadModelOptions();
    } catch (error) {
      console.error("Failed to switch model:", error);
      setDownloadingModel(null);
    }
  }

  async function searchConversations(query: string) {
    if (!query.trim()) {
      setFilteredConversations(conversations);
      setArchivedSearchResults([]);
      return;
    }

    try {
      const [active, archived] = await Promise.all([
        invoke<ConversationSummary[]>("search_conversations", { query, limit: 50 }),
        invoke<ConversationSummary[]>("search_archived_conversations", { query, limit: 50 }),
      ]);
      setFilteredConversations(active);
      setArchivedSearchResults(archived);
    } catch (error) {
      console.error("Failed to search conversations:", error);
      setFilteredConversations(conversations);
      setArchivedSearchResults([]);
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
      const newConvId = await invoke<number>("create_conversation", {
        title: "New Conversation",
      });
      setCurrentConversationId(newConvId);
      setMessages([]);
      setDocuments([]);
      await loadConversations();
    } catch (error) {
      console.error("Failed to create conversation:", error);
    }
  }

  async function switchConversation(convId: number) {
    try {
      // Note: an in-progress answer keeps generating in its own chat.
      setCurrentConversationId(convId);
      const msgs = await invoke<Message[]>("get_conversation_messages", {
        conversationId: convId,
      });
      setMessages(msgs);
      await loadDocuments(convId);
    } catch (error) {
      console.error("Failed to switch conversation:", error);
    }
  }

  // Send a question. One local model answers one thing at a time, but you can
  // line up questions across chats; they run in order and each shows progress in
  // its own chat.
  async function handleAskQuestion(override?: string) {
    const userQuestion = (override ?? question).trim();
    if (!userQuestion) return;
    if (override === undefined) setQuestion("");

    const convId = await ensureConversation();

    await invoke("add_message", { conversationId: convId, role: "user", content: userQuestion });
    if (viewConvRef.current === convId) {
      setMessages((prev) => [...prev, { role: "user", content: userQuestion, timestamp: nowSec() }]);
    }

    queueRef.current.push({ convId, question: userQuestion });
    setQueuedConvIds(queueRef.current.map((q) => q.convId));
    processQueue();
  }

  async function processQueue() {
    if (processingRef.current) return;
    const next = queueRef.current.shift();
    setQueuedConvIds(queueRef.current.map((q) => q.convId));
    if (!next) return;

    processingRef.current = true;
    await runGeneration(next.convId, next.question);
    processingRef.current = false;
    processQueue();
  }

  async function runGeneration(convId: number, userQuestion: string) {
    setGeneratingConvId(convId);
    setGenStartedAt(Date.now());
    genConvRef.current = convId;
    accumulatedRef.current = "";

    // We buffer the tokens instead of painting them live: the chat shows a
    // "generating" indicator, then the finished answer appears all at once.
    let accumulatedResponse = "";
    let unlisten: (() => void) | null = null;
    let unlistenComplete: (() => void) | null = null;

    try {
      unlisten = await listen<string>("llm-token", (event) => {
        accumulatedResponse += event.payload;
        accumulatedRef.current = accumulatedResponse;
      });
      unlistenComplete = await listen<string>("llm-complete", (event) => {
        accumulatedResponse = event.payload;
        accumulatedRef.current = accumulatedResponse;
      });

      await invoke<string>("query_documents_streaming", {
        question: userQuestion,
        modelPath: modelPath,
        conversationId: convId,
      });

      if (accumulatedResponse && accumulatedResponse.trim()) {
        await invoke("add_message", { conversationId: convId, role: "assistant", content: accumulatedResponse });
        appendAssistantToView(convId, accumulatedResponse);
        await maybeGenerateTitle(convId, userQuestion);
      } else {
        appendAssistantToView(
          convId,
          "I wasn't able to generate a response. Please try rephrasing your question or check if documents are uploaded."
        );
      }

      await loadConversations();
    } catch (error) {
      console.error("Failed to process question:", error);
      const errorMessage = typeof error === 'string' ? error :
        (error && typeof error === 'object' && 'message' in error) ? (error as Error).message : '';
      let errorCategory = "Error";
      let errorText = "I encountered an error processing your question.";
      if (errorMessage.includes('model') || errorMessage.includes('load')) {
        errorCategory = "Model Error";
        errorText = "Failed to load the AI model. Please ensure a model is downloaded and try again.";
      } else if (errorMessage.includes('memory') || errorMessage.includes('allocation')) {
        errorCategory = "Memory Error";
        errorText = "Not enough memory to process this request. Try closing other applications or asking a simpler question.";
      } else if (errorMessage.includes('timeout') || errorMessage.includes('timed out')) {
        errorCategory = "Timeout Error";
        errorText = "The request took too long to process. Try asking a simpler question.";
      } else if (errorMessage) {
        errorText += `\n\nDetails: ${errorMessage}`;
      }
      // A manual stop isn't an error — keep whatever was produced instead.
      if (stopRequestedRef.current) {
        if (accumulatedRef.current.trim()) {
          await invoke("add_message", { conversationId: convId, role: "assistant", content: accumulatedRef.current }).catch(() => {});
          appendAssistantToView(convId, accumulatedRef.current);
        }
      } else {
        appendAssistantToView(convId, `**${errorCategory}**\n\n${errorText}`);
      }
    } finally {
      stopRequestedRef.current = false;
      setWebSearching(false);
      if (unlisten) unlisten();
      if (unlistenComplete) unlistenComplete();
      setGeneratingConvId(null);
      setGenStartedAt(null);
      genConvRef.current = null;
    }
  }

  // Give a brand-new chat a short title once it has its first answer.
  async function maybeGenerateTitle(convId: number, question: string) {
    const conv = conversations.find((c) => c.id === convId);
    if (conv && conv.title !== "New Conversation") return; // already has a real title
    try {
      const raw = await invoke<string>("generate_chat_title", { question, modelPath });
      const title = (raw || "").trim().replace(/^["']+|["']+$/g, "").slice(0, 60);
      if (title) {
        await invoke("update_conversation_title", { conversationId: convId, title });
      }
    } catch (error) {
      console.error("Failed to generate title:", error);
    }
  }

  // Stop the current answer. With clearQueue, also drop anything lined up behind
  // it so nothing else starts.
  async function handleStopGeneration(clearQueue = false) {
    if (clearQueue) {
      queueRef.current = [];
      setQueuedConvIds([]);
    }
    stopRequestedRef.current = true;
    setWebSearching(false);
    try {
      await invoke("stop_generation");
    } catch (error) {
      console.error("Failed to stop generation:", error);
    }
    // runGeneration's own cleanup keeps whatever was produced and resets state.
  }

  async function handleFolderUpload() {
    const selected = await open({ directory: true, multiple: false });
    if (!selected || typeof selected !== "string") return;

    let unlisten: (() => void) | null = null;
    try {
      unlisten = await listen<{ processed: number; total: number; current_file: string }>(
        "folder-progress",
        (event) => setFolderProgress(event.payload)
      );

      const convId = await ensureConversation();
      setImportConvId(convId);
      setImportStartedAt(Date.now());
      const summary = await invoke<{
        ingested: string[];
        failed: { file_name: string; error: string }[];
        total: number;
      }>("upload_folder", { folderPath: selected, conversationId: convId });

      await loadDocuments(convId);

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
      setOcrProgress(null);
      setImportConvId(null);
      setImportStartedAt(null);
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
      try {
        const convId = await ensureConversation();
        setImportConvId(convId);
        setImportStartedAt(Date.now());
        await invoke<DocumentMetadata>("upload_document", {
          filePath: selected,
          conversationId: convId,
        });
        await loadDocuments(convId);
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
        setOcrProgress(null);
        setImportConvId(null);
        setImportStartedAt(null);
      }
    }
  }

  async function handleDocumentDelete(documentId: number) {
    try {
      await invoke("delete_document", { documentId });
      if (selectedDocument?.id === documentId) {
        setSelectedDocument(null);
      }
      await loadDocuments(currentConversationId);
    } catch (error) {
      console.error("Failed to delete document:", error);
    }
  }

  async function handleConversationDelete(conversationId: number) {
    const confirmed = await ask("Delete this chat? This can't be undone.", {
      title: "Delete chat",
      kind: "warning",
    });
    if (!confirmed) return;
    try {
      await invoke("delete_conversation", { conversationId });
      if (currentConversationId === conversationId) {
        setCurrentConversationId(null);
        setMessages([]);
        setDocuments([]);
      }
      await loadConversations();
    } catch (error) {
      console.error("Failed to delete conversation:", error);
    }
  }

  async function handleConversationArchive(conversationId: number) {
    try {
      await invoke("set_conversation_archived", { conversationId, archived: true });
      if (currentConversationId === conversationId) {
        setCurrentConversationId(null);
        setMessages([]);
        setDocuments([]);
      }
      await loadConversations();
    } catch (error) {
      console.error("Failed to archive conversation:", error);
    }
  }

  async function handleConversationUnarchive(conversationId: number) {
    try {
      await invoke("set_conversation_archived", { conversationId, archived: false });
      await loadConversations();
    } catch (error) {
      console.error("Failed to unarchive conversation:", error);
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

      {webAccess === "unasked" && !showOnboarding && !checkingOnboarding && !isLocked && (
        <div className="citation-modal-overlay">
          <div className="citation-modal">
            <div className="citation-modal-header">
              <h3>Enable web search?</h3>
            </div>
            <div className="citation-modal-body">
              <p>
                When your documents don't have the answer, Enklayve can search the web for you.
                Only your question is sent out — never your documents. You can change this anytime
                from the Documents panel.
              </p>
              <div className="modal-actions">
                <button className="modal-primary-btn" onClick={() => handleSetWeb("enabled")}>
                  Enable web search
                </button>
                <button className="modal-secondary-btn" onClick={() => handleSetWeb("disabled")}>
                  Keep it offline
                </button>
              </div>
            </div>
          </div>
        </div>
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

      {showAbout && (
        <div className="citation-modal-overlay" onClick={() => setShowAbout(false)}>
          <div className="citation-modal about-modal" onClick={(e) => e.stopPropagation()}>
            <div className="citation-modal-header">
              <h3>Enklayve{appVersion ? ` ${appVersion}` : ""}</h3>
              <button
                className="citation-modal-close"
                aria-label="Close"
                onClick={() => setShowAbout(false)}
              >
                ×
              </button>
            </div>
            <div className="citation-modal-body">
              <div className="about-row">
                <span className="about-label">AI model</span>
                <span className="about-value">
                  {activeModel ? activeModel.name : "No model loaded yet"}
                </span>
              </div>
              {activeModel && (
                <>
                  <div className="about-row">
                    <span className="about-label">Profile</span>
                    <span className="about-value">
                      {activeModel.performance_tier} · {activeModel.size_gb.toFixed(1)} GB
                    </span>
                  </div>
                  <p className="about-note">{activeModel.note}</p>
                </>
              )}
              <p className="about-privacy">
                Everything runs on this device and is encrypted at rest. No accounts,
                no telemetry, no tracking. Web search, when on, sends only your question —
                never your documents.
              </p>
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
            onConversationArchive={handleConversationArchive}
            onConversationUnarchive={handleConversationUnarchive}
            archivedConversations={archivedConversations}
            archivedSearchResults={archivedSearchResults}
            generatingConvId={generatingConvId}
            importingConvId={importConvId}
            queuedConvIds={queuedConvIds}
            onNewConversation={createNewConversation}
            isDarkMode={isDarkMode}
            onThemeToggle={toggleTheme}
            onBackupComplete={handleBackupComplete}
            onSecuritySettings={() => setShowSecuritySettings(true)}
            securityEnabled={securityEnabled}
            webAccess={webAccess}
            onToggleWeb={handleToggleWeb}
            onAbout={() => setShowAbout(true)}
            activeModelName={activeModel?.name}
            modelOptions={modelOptions}
            onSelectModel={switchModel}
            downloadingModel={downloadingModel}
          />
        </div>
      </aside>

      <main className="main-content">
        <ChatWindow
          messages={messages}
          question={question}
          loading={generatingConvId === currentConversationId}
          importing={importConvId !== null}
          genStartedAt={genStartedAt}
          importStartedAt={importStartedAt}
          queued={currentConversationId != null && queuedConvIds.includes(currentConversationId)}
          documents={documents}
          folderProgress={importConvId === currentConversationId ? folderProgress : null}
          ocrProgress={importConvId === currentConversationId ? ocrProgress : null}
          onQuestionChange={setQuestion}
          onSubmit={handleAskQuestion}
          onStarterPrompt={(text) => handleAskQuestion(text)}
          onStop={handleStopGeneration}
          hasQueue={queuedConvIds.length > 0}
          onOpenCitation={handleOpenCitation}
          onAttachFile={handleDocumentUpload}
          onAttachFolder={handleFolderUpload}
          onRemoveDocument={handleDocumentDelete}
          webSearching={webSearching && generatingConvId === currentConversationId}
        />
        {/* save-to-folder removed: chats are already saved & encrypted in-app */}
      </main>
    </div>
  );
}

export default App;
