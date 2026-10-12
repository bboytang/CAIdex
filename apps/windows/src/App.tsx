import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type RefObject,
} from "react";
import Markdown from "react-markdown";
import {
  ArrowDown,
  ArrowUp,
  Check,
  ChevronDown,
  Copy,
  Download,
  FileText,
  Globe,
  KeyRound,
  MessageSquare,
  Monitor,
  Moon,
  PanelLeft,
  Plus,
  Search,
  Settings as SettingsIcon,
  SlidersHorizontal,
  Square,
  Sun,
  X,
} from "lucide-react";
import {
  api,
  providers,
  type Bootstrap,
  type Conversation,
  type Message,
  type Preferences,
  type Profile,
  type ProfileStatus,
} from "./api";
import { errorText, translations, type Text } from "./i18n";

export function App() {
  const [boot, setBoot] = useState<Bootstrap | null>(null);
  const [fatal, setFatal] = useState(false);
  const [preferences, setPreferences] = useState<Preferences>({
    language: "zh",
    theme: "system",
  });
  const t = translations[preferences.language];
  const [sidebar, setSidebar] = useState(true);
  const [searching, setSearching] = useState(false);
  const [query, setQuery] = useState("");
  const [current, setCurrent] = useState<string | null>(null);
  const currentRef = useRef<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [head, setHead] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [draftReady, setDraftReady] = useState(false);
  const inputRef = useRef("");
  const [attachments, setAttachments] = useState<
    { id: string; name: string }[]
  >([]);
  const [operation, setOperation] = useState<string | null>(null);
  const [activeConversation, setActiveConversation] = useState<string | null>(
    null,
  );
  const stopRequested = useRef<string | null>(null);
  const [stream, setStream] = useState("");
  const [error, setError] = useState("");
  const [settings, setSettings] = useState(false);
  const [settingsTab, setSettingsTab] = useState("general");
  const [mode, setMode] = useState<"chat" | "codex">("chat");
  const [modelMenu, setModelMenu] = useState(false);
  const [atBottom, setAtBottom] = useState(true);
  const scroll = useRef<HTMLDivElement>(null);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const composing = useRef(false);
  const changeInput = (value: string) => {
    inputRef.current = value;
    setInput(value);
  };
  const fail = (reason: unknown) =>
    setError(errorText(reason, preferences.language));
  const refresh = async () => {
    const state = await api.bootstrap();
    let page = state.conversations;
    while (page.length === 128) {
      page = await api.conversations(page.at(-1)!.id);
      state.conversations.push(...page);
    }
    setBoot(state);
    setPreferences(state.preferences);
    return state;
  };
  const start = async () => {
    setFatal(false);
    try {
      await refresh();
    } catch {
      setFatal(true);
    }
  };
  useEffect(() => {
    void start();
  }, []);
  useEffect(() => {
    document.documentElement.lang =
      preferences.language === "zh" ? "zh-CN" : "en";
    document.documentElement.dataset.theme = preferences.theme;
  }, [preferences]);
  useEffect(() => {
    if (!current || !draftReady) return;
    const value = input;
    const id = current;
    const timeout = setTimeout(() => {
      void api.draft(id, value).catch(() => {
        if (currentRef.current === id) setError(t.draftError);
      });
    }, 400);
    return () => clearTimeout(timeout);
  }, [input, current, draftReady]);
  useEffect(() => {
    const el = textarea.current;
    if (el) {
      el.style.height = "auto";
      el.style.height = `${Math.min(el.scrollHeight, 220)}px`;
    }
  }, [input]);
  useEffect(() => {
    if (atBottom)
      scroll.current?.scrollTo({ top: scroll.current.scrollHeight });
  }, [messages, stream]);
  const loadMessages = async (id: string) => {
    let after = 0;
    const all: Message[] = [];
    while (true) {
      const page = await api.snapshot(id, after);
      if (currentRef.current !== id) return;
      all.push(...page.messages);
      if (page.messages.length < 128) break;
      after = page.messages.at(-1)!.sequence;
    }
    if (currentRef.current === id) {
      setMessages(all);
      setHead(all.at(-1)?.id ?? null);
    }
  };
  const select = async (id: string) => {
    if (id === currentRef.current) return;
    try {
      if (currentRef.current)
        await api.draft(currentRef.current, inputRef.current);
      setDraftReady(false);
      currentRef.current = id;
      setCurrent(id);
      setMessages([]);
      setHead(null);
      setStream("");
      setAttachments([]);
      changeInput("");
      setError("");
      setAtBottom(true);
      const draft = await api.draft(id);
      if (currentRef.current !== id) return;
      changeInput(draft);
      setDraftReady(true);
      await loadMessages(id);
      textarea.current?.focus();
    } catch (reason) {
      fail(reason);
    }
  };
  const create = async (title = t.newChat) => {
    const id = await api.create(title);
    await refresh();
    await select(id);
    return id;
  };
  useEffect(() => {
    if (!operation || !activeConversation) return;
    let live = true;
    let running = false;
    const poll = async () => {
      if (running) return;
      running = true;
      try {
        const snapshot = await api.snapshot(activeConversation, 0);
        if (live) {
          if (
            snapshot.operation === stopRequested.current &&
            snapshot.operation
          )
            await api.cancel(snapshot.operation);
          if (currentRef.current === activeConversation) {
            setStream(snapshot.draft ?? "");
            await loadMessages(activeConversation);
          }
        }
      } catch (reason) {
        if (live) fail(reason);
      } finally {
        running = false;
      }
    };
    const timer = setInterval(() => {
      void poll();
    }, 150);
    void poll();
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [operation, activeConversation]);
  const send = async (event?: FormEvent) => {
    event?.preventDefault();
    if (operation || (!input.trim() && !attachments.length) || mode !== "chat")
      return;
    if (!boot?.selected) {
      setSettingsTab("models");
      setSettings(true);
      return;
    }
    const text = inputRef.current;
    const files = [...attachments];
    const parent = head;
    const after = currentRef.current ? (messages.at(-1)?.sequence ?? 0) : 0;
    const op = crypto.randomUUID();
    setOperation(op);
    setError("");
    let id = currentRef.current;
    try {
      if (!id) id = await create(text.trim().slice(0, 48) || t.newChat);
      await api.draft(id, text);
      if (stopRequested.current === op) return;
      setActiveConversation(id);
      const submitted = api.send(
        op,
        id,
        parent,
        text,
        files.map((file) => file.id),
      );
      changeInput("");
      setAttachments([]);
      setStream("");
      let accepted = false;
      try {
        await submitted;
        accepted = true;
      } catch (reason) {
        // A lost reply can follow a committed turn: inspect history, never retry the POST.
        const snapshot = await api.snapshot(id, after);
        accepted = snapshot.messages.some(
          (message) =>
            message.role === "user" &&
            message.content.text === text &&
            message.parent === parent,
        );
        fail(reason);
      }
      if (accepted) await api.draft(id, inputRef.current);
      else if (currentRef.current === id && !inputRef.current) {
        changeInput(text);
        setAttachments(files);
      }
      await loadMessages(id);
      await refresh();
    } catch (reason) {
      if (id === currentRef.current && !inputRef.current) {
        changeInput(text);
        setAttachments(files);
      }
      fail(reason);
    } finally {
      setOperation(null);
      setActiveConversation(null);
      stopRequested.current = null;
      setStream("");
      textarea.current?.focus();
    }
  };
  const addFiles = async (files: FileList | null) => {
    if (!files) return;
    if (attachments.length + files.length > 4) {
      setError(t.attachmentHint);
      return;
    }
    try {
      const id = currentRef.current ?? (await create());
      const additions: { id: string; name: string }[] = [];
      for (const file of Array.from(files)) {
        if (!file.size || file.size > 8 * 1024 * 1024)
          throw "invalid_or_sensitive_input";
        const mediaType =
          file.type || (file.name.endsWith(".txt") ? "text/plain" : "");
        const attachment = await api.attach(
          id,
          file.name,
          mediaType,
          Array.from(new Uint8Array(await file.arrayBuffer())),
        );
        additions.push({ id: attachment, name: file.name });
      }
      if (currentRef.current === id)
        setAttachments((old) => [...old, ...additions]);
    } catch (reason) {
      fail(reason);
    }
  };
  const exportChat = async () => {
    if (!current) return;
    try {
      const output = await api.exportChat(current);
      const url = URL.createObjectURL(
        new Blob([output], { type: "application/json" }),
      );
      const link = document.createElement("a");
      link.href = url;
      link.download = `CAIdex-${current}.json`;
      link.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (reason) {
      fail(reason);
    }
  };
  const chooseModel = async (id: string) => {
    try {
      await api.selectProfile(id);
      await refresh();
      setModelMenu(false);
    } catch (reason) {
      fail(reason);
    }
  };
  const changePreferences = async (next: Preferences) => {
    try {
      await api.preferences(next);
      setPreferences(next);
      setBoot((old) => old && { ...old, preferences: next });
    } catch (reason) {
      fail(reason);
    }
  };
  const selected = boot?.profiles.find(
    (row) => row.profile.id === boot.selected,
  );
  const byId = new Map(messages.map((message) => [message.id, message]));
  const lineage: Message[] = [];
  let node = head && byId.get(head);
  while (node && lineage.length <= messages.length) {
    lineage.unshift(node);
    node = node.parent ? byId.get(node.parent) : undefined;
  }
  const conversations =
    boot?.conversations.filter((chat) =>
      chat.title.toLowerCase().includes(query.toLowerCase()),
    ) ?? [];
  if (fatal)
    return (
      <main className="fatal">
        <img src="/caidex-symbol.png" alt="CAIdex" />
        <h1>{t.fatal}</h1>
        <p>{t.fatalHelp}</p>
        <button onClick={() => void start()}>{t.retry}</button>
      </main>
    );
  if (!boot)
    return (
      <main className="fatal">
        <img src="/caidex-symbol.png" alt="CAIdex" />
        <p role="status">{t.loading}</p>
      </main>
    );
  return (
    <div className={`app ${sidebar ? "" : "sidebar-closed"}`}>
      {sidebar && (
        <aside className="sidebar" aria-label={t.chats}>
          <div className="sidebar-top">
            <img className="brand" src="/caidex-symbol.png" alt="CAIdex" />
            <button
              className="icon"
              aria-label={t.collapse}
              onClick={() => setSidebar(false)}
            >
              <PanelLeft size={20} />
            </button>
          </div>
          <button
            className="nav-item"
            onClick={() => void create().catch(fail)}
          >
            <Plus size={19} />
            {t.newChat}
            <kbd>Ctrl N</kbd>
          </button>
          <button
            className="nav-item"
            onClick={() => setSearching((old) => !old)}
          >
            <Search size={19} />
            {t.search}
          </button>
          {searching && (
            <input
              className="search"
              autoFocus
              aria-label={t.search}
              placeholder={t.search}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
          )}
          <div className="mode-nav">
            <button
              className={mode === "chat" ? "active" : ""}
              onClick={() => setMode("chat")}
            >
              <MessageSquare size={17} />
              Chat
            </button>
            <button
              className={mode === "codex" ? "active" : ""}
              aria-label="Codex"
              onClick={() => setMode("codex")}
            >
              <span className="code-icon">&lt;/&gt;</span>Codex
            </button>
          </div>
          <div className="history">
            <h2>{t.chats}</h2>
            {conversations.length ? (
              conversations.map((chat) => (
                <button
                  key={chat.id}
                  className={`history-item ${current === chat.id ? "selected" : ""}`}
                  title={chat.title}
                  onClick={() => {
                    setMode("chat");
                    void select(chat.id);
                  }}
                >
                  {chat.title}
                </button>
              ))
            ) : (
              <p className="muted empty-history">
                {query ? t.noResults : t.noHistory}
              </p>
            )}
          </div>
          <button className="account-menu" onClick={() => setSettings(true)}>
            <span className="avatar">
              <Monitor size={18} />
            </span>
            <span>
              <strong>CAIdex</strong>
              <small>{t.local}</small>
            </span>
            <SettingsIcon size={18} />
          </button>
        </aside>
      )}
      <main className="main">
        <header className="topbar">
          <div className="header-left">
            {!sidebar && (
              <button
                className="icon"
                aria-label={t.expand}
                onClick={() => setSidebar(true)}
              >
                <PanelLeft size={20} />
              </button>
            )}
            <div className="model-menu">
              <button
                className="model-button"
                aria-label={t.model}
                onClick={() => setModelMenu((old) => !old)}
              >
                CAIdex <span>{selected?.profile.name ?? "Chat"}</span>
                <ChevronDown size={17} />
              </button>
              {modelMenu && (
                <>
                  <button
                    className="menu-dismiss"
                    aria-label={t.close}
                    onClick={() => setModelMenu(false)}
                  />
                  <div className="dropdown" role="menu">
                    {boot.profiles.map((row) => (
                      <button
                        key={row.profile.id}
                        role="menuitem"
                        disabled={!!operation}
                        onClick={() => void chooseModel(row.profile.id)}
                      >
                        <span>
                          {row.profile.name}
                          <small>{row.profile.model}</small>
                        </span>
                        {boot.selected === row.profile.id && (
                          <Check size={16} />
                        )}
                      </button>
                    ))}
                    <button
                      role="menuitem"
                      onClick={() => {
                        setModelMenu(false);
                        setSettingsTab("models");
                        setSettings(true);
                      }}
                    >
                      <Plus size={16} />
                      {t.configure}
                    </button>
                  </div>
                </>
              )}
            </div>
          </div>
          <div className="header-right">
            <span className="local-badge">
              <span />
              {t.local}
            </span>
            {current && (
              <button
                className="icon"
                title={t.export}
                aria-label={t.export}
                onClick={() => void exportChat()}
              >
                <Download size={18} />
              </button>
            )}
            <button
              className="icon"
              aria-label={t.settings}
              onClick={() => setSettings(true)}
            >
              <SlidersHorizontal size={19} />
            </button>
          </div>
        </header>
        {mode === "codex" ? (
          <div className="welcome codex">
            <span className="welcome-mark">&lt;/&gt;</span>
            <h1>Codex</h1>
            <p>{t.codex}</p>
            <button onClick={() => setMode("chat")}>Chat</button>
          </div>
        ) : (
          <>
            <div
              className="messages-scroll"
              ref={scroll}
              onScroll={(event) => {
                const el = event.currentTarget;
                setAtBottom(
                  el.scrollHeight - el.scrollTop - el.clientHeight < 80,
                );
              }}
            >
              {!messages.length && !operation ? (
                <div className="welcome">
                  <h1>{t.greeting}</h1>
                  {!selected && (
                    <div className="setup-card">
                      <KeyRound size={20} />
                      <div>
                        <strong>{t.noModel}</strong>
                        <p>{t.modelHint}</p>
                      </div>
                      <button
                        onClick={() => {
                          setSettingsTab("models");
                          setSettings(true);
                        }}
                      >
                        <Plus size={17} />
                        {t.configure}
                      </button>
                    </div>
                  )}
                </div>
              ) : (
                <div className="message-list">
                  {lineage.map((message) => (
                    <MessageView
                      key={message.id}
                      message={message}
                      t={t}
                      siblings={messages.filter(
                        (other) =>
                          other.role === message.role &&
                          other.parent === message.parent,
                      )}
                      choose={(id, continueHere) => {
                        let target = id;
                        if (!continueHere) {
                          for (
                            let depth = 0;
                            depth < messages.length;
                            depth++
                          ) {
                            const next = messages
                              .filter((row) => row.parent === target)
                              .at(-1);
                            if (!next) break;
                            target = next.id;
                          }
                        }
                        setHead(target);
                      }}
                      fail={fail}
                    />
                  ))}
                  {operation && activeConversation === current && (
                    <article className="message assistant streaming">
                      <div className="assistant-brand">
                        <img src="/caidex-symbol.png" alt="" />
                        CAIdex
                      </div>
                      <div className="message-content">
                        {stream || (
                          <span className="pulse" aria-label={t.generated} />
                        )}
                      </div>
                      <small role="status">{t.generated}</small>
                    </article>
                  )}
                </div>
              )}
            </div>
            <div
              className={`composer-area ${!messages.length && !operation ? "empty-composer" : ""}`}
            >
              {!atBottom && (
                <button
                  className="scroll-bottom icon"
                  aria-label={t.generated}
                  onClick={() => {
                    setAtBottom(true);
                    scroll.current?.scrollTo({
                      top: scroll.current.scrollHeight,
                      behavior: "smooth",
                    });
                  }}
                >
                  <ArrowDown size={18} />
                </button>
              )}
              {error && (
                <div className="error" role="alert">
                  <span>{error}</span>
                  <button
                    className="icon"
                    aria-label={t.close}
                    onClick={() => setError("")}
                  >
                    <X size={16} />
                  </button>
                </div>
              )}
              <form className="composer" onSubmit={(event) => void send(event)}>
                {!!attachments.length && (
                  <div className="attachments">
                    {attachments.map((file) => (
                      <span className="file-chip" key={file.id}>
                        <FileText size={17} />
                        <span>{file.name}</span>
                        <button
                          type="button"
                          className="icon"
                          aria-label={`${t.removeAttachment} ${file.name}`}
                          onClick={() =>
                            setAttachments((old) =>
                              old.filter((other) => other.id !== file.id),
                            )
                          }
                        >
                          <X size={14} />
                        </button>
                      </span>
                    ))}
                  </div>
                )}
                <textarea
                  ref={textarea}
                  aria-label={t.placeholder}
                  placeholder={t.placeholder}
                  value={input}
                  rows={1}
                  maxLength={1024 * 1024}
                  onChange={(event) => changeInput(event.target.value)}
                  onCompositionStart={() => {
                    composing.current = true;
                  }}
                  onCompositionEnd={() => {
                    composing.current = false;
                  }}
                  onKeyDown={(event) => {
                    if (
                      event.key === "Enter" &&
                      !event.shiftKey &&
                      !event.nativeEvent.isComposing &&
                      !composing.current &&
                      event.keyCode !== 229
                    ) {
                      event.preventDefault();
                      void send();
                    }
                  }}
                />
                <div className="composer-tools">
                  <label
                    className="icon attach-button"
                    title={t.attachmentHint}
                  >
                    <Plus size={21} />
                    <input
                      type="file"
                      multiple
                      accept="image/png,image/jpeg,application/pdf,text/plain"
                      aria-label={t.attach}
                      onChange={(event) => {
                        void addFiles(event.target.files);
                        event.target.value = "";
                      }}
                    />
                  </label>
                  <button
                    className="composer-model"
                    type="button"
                    onClick={() => {
                      setSettingsTab("models");
                      setSettings(true);
                    }}
                  >
                    {selected?.profile.name ?? t.model}
                    <ChevronDown size={13} />
                  </button>
                  <span className="spacer" />
                  {operation ? (
                    <button
                      type="button"
                      className="send-button"
                      aria-label={t.stop}
                      onClick={() => {
                        stopRequested.current = operation;
                      }}
                    >
                      <Square size={15} fill="currentColor" />
                    </button>
                  ) : (
                    <button
                      className="send-button"
                      type="submit"
                      aria-label={t.send}
                      disabled={!input.trim() && !attachments.length}
                    >
                      <ArrowUp size={20} />
                    </button>
                  )}
                </div>
              </form>
              <p className="disclaimer">{t.disclaimer}</p>
            </div>
          </>
        )}
      </main>
      {settings && (
        <SettingsDialog
          t={t}
          preferences={preferences}
          profiles={boot.profiles}
          tab={settingsTab}
          setTab={setSettingsTab}
          changePreferences={changePreferences}
          refresh={refresh}
          close={() => setSettings(false)}
          operation={operation}
          fail={fail}
        />
      )}
      <Shortcuts
        create={() => void create().catch(fail)}
        close={() => {
          setSettings(false);
          setModelMenu(false);
        }}
        textarea={textarea}
      />
    </div>
  );
}

function MessageView({
  message,
  t,
  siblings,
  choose,
  fail,
}: {
  message: Message;
  t: Text;
  siblings: Message[];
  choose: (id: string, continueHere?: boolean) => void;
  fail: (reason: unknown) => void;
}) {
  const [copied, setCopied] = useState(false);
  return (
    <article className={`message ${message.role}`}>
      {message.role === "assistant" && (
        <div className="assistant-brand">
          <img src="/caidex-symbol.png" alt="" />
          CAIdex
        </div>
      )}
      <div className="message-content">
        {message.role === "assistant" ? (
          <Markdown
            skipHtml
            components={{
              a: ({ children, href }) => (
                <span className="reference-link" title={href}>
                  {children}
                </span>
              ),
              img: ({ alt }) => <span>{alt}</span>,
            }}
          >
            {message.content.text}
          </Markdown>
        ) : (
          message.content.text
        )}
      </div>
      {!!message.content.attachments.length && (
        <small className="attachment-count">
          <FileText size={13} /> {message.content.attachments.length}
        </small>
      )}
      <div className="message-actions">
        {message.role === "assistant" && (
          <button
            className="branch-button"
            title={t.branchHint}
            onClick={() => choose(message.id, true)}
          >
            {t.branch}
          </button>
        )}
        {message.role === "assistant" && (
          <button
            className="icon"
            aria-label={copied ? t.copied : t.copy}
            onClick={() => {
              void navigator.clipboard
                .writeText(message.content.text)
                .then(() => {
                  setCopied(true);
                  setTimeout(() => setCopied(false), 2000);
                })
                .catch(fail);
            }}
          >
            {copied ? <Check size={16} /> : <Copy size={16} />}
          </button>
        )}
        {siblings.length > 1 && (
          <button
            className="branch-button"
            title={t.branchHint}
            onClick={() =>
              choose(
                siblings[
                  (siblings.findIndex((row) => row.id === message.id) + 1) %
                    siblings.length
                ].id,
              )
            }
          >
            {t.branch} {siblings.findIndex((row) => row.id === message.id) + 1}/
            {siblings.length}
          </button>
        )}
        {message.end !== "complete" && (
          <small className="status-label">
            {message.end === "cancelled"
              ? t.cancelled
              : message.end === "failed" || message.end === "invalid"
                ? t.failed
                : t.interrupted}
          </small>
        )}
      </div>
    </article>
  );
}

function Shortcuts({
  create,
  close,
  textarea,
}: {
  create: () => void;
  close: () => void;
  textarea: RefObject<HTMLTextAreaElement | null>;
}) {
  const callbacks = useRef({ create, close });
  callbacks.current = { create, close };
  useEffect(() => {
    const handle = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "n") {
        event.preventDefault();
        if (!document.querySelector('[role="dialog"]'))
          callbacks.current.create();
      }
      if (event.key === "Escape") {
        callbacks.current.close();
        textarea.current?.focus();
      }
    };
    window.addEventListener("keydown", handle);
    return () => window.removeEventListener("keydown", handle);
  }, []);
  return null;
}

function SettingsDialog({
  t,
  preferences,
  profiles,
  tab,
  setTab,
  changePreferences,
  refresh,
  close,
  operation,
  fail,
}: {
  t: Text;
  preferences: Preferences;
  profiles: ProfileStatus[];
  tab: string;
  setTab: (value: string) => void;
  changePreferences: (value: Preferences) => Promise<void>;
  refresh: () => Promise<Bootstrap>;
  close: () => void;
  operation: string | null;
  fail: (reason: unknown) => void;
}) {
  const [editing, setEditing] = useState<Profile | null>(null);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [dialogError, setDialogError] = useState("");
  const dialog = useRef<HTMLDivElement>(null);
  const report = (reason: unknown) =>
    setDialogError(errorText(reason, preferences.language));
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialog.current?.focus();
    const trap = (event: KeyboardEvent) => {
      if (event.key !== "Tab") return;
      const elements = Array.from(
        dialog.current?.querySelectorAll<HTMLElement>(
          "button:not(:disabled),input:not(:disabled),select:not(:disabled),a[href]",
        ) ?? [],
      );
      const first = elements[0],
        last = elements.at(-1);
      if (
        event.shiftKey &&
        (document.activeElement === first ||
          document.activeElement === dialog.current)
      ) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    };
    document.addEventListener("keydown", trap);
    return () => {
      document.removeEventListener("keydown", trap);
      previous?.focus();
    };
  }, []);
  const edit = (profile: Profile) => {
    setEditing({ ...profile });
    setKey("");
    setNotice("");
    setDialogError("");
  };
  const save = async (event: FormEvent) => {
    event.preventDefault();
    if (!editing || busy) return;
    const original = profiles.find((row) => row.profile.id === editing.id);
    const reuse =
      !!original?.configured &&
      !key &&
      original.profile.endpoint !== editing.endpoint;
    if (reuse && !window.confirm(t.reuseKeyConfirm)) return;
    setBusy(true);
    setDialogError("");
    try {
      await api.saveProfile(editing, key || null, reuse);
      setKey("");
      await refresh();
      setEditing(null);
      setNotice(t.saved);
    } catch (reason) {
      report(reason);
    } finally {
      setBusy(false);
    }
  };
  const remove = async (id: string) => {
    if (!window.confirm(t.removeKeyConfirm)) return;
    try {
      await api.removeKey(id);
      await refresh();
    } catch (reason) {
      report(reason);
    }
  };
  const tabs = [
    { id: "general", label: t.general, icon: <SettingsIcon size={17} /> },
    { id: "appearance", label: t.appearance, icon: <Sun size={17} /> },
    { id: "language", label: t.language, icon: <Globe size={17} /> },
    { id: "models", label: t.models, icon: <SlidersHorizontal size={17} /> },
    { id: "about", label: t.about, icon: <Monitor size={17} /> },
  ];
  return (
    <div
      className="modal-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) close();
      }}
    >
      <div
        className="settings-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={t.settings}
        tabIndex={-1}
        ref={dialog}
      >
        <div className="settings-nav">
          <h2>{t.settings}</h2>
          {tabs.map((item) => (
            <button
              key={item.id}
              className={tab === item.id ? "selected" : ""}
              onClick={() => {
                setTab(item.id);
                setEditing(null);
                setKey("");
                setDialogError("");
              }}
            >
              {item.icon}
              {item.label}
            </button>
          ))}
        </div>
        <div className="settings-content">
          <header>
            <h2>{tabs.find((item) => item.id === tab)?.label}</h2>
            <button
              className="icon"
              aria-label={t.close}
              disabled={busy}
              onClick={close}
            >
              <X size={21} />
            </button>
          </header>
          {dialogError && (
            <p className="error" role="alert">
              {dialogError}
            </p>
          )}
          {notice && (
            <p className="notice" role="status">
              <Check size={16} />
              {notice}
            </p>
          )}
          {(tab === "general" || tab === "appearance") && (
            <div className="setting-row">
              <span>{t.theme}</span>
              <select
                aria-label={t.theme}
                value={preferences.theme}
                onChange={(event) =>
                  void changePreferences({
                    ...preferences,
                    theme: event.target.value as Preferences["theme"],
                  })
                }
              >
                <option value="system">{t.system}</option>
                <option value="light">{t.light}</option>
                <option value="dark">{t.dark}</option>
              </select>
            </div>
          )}
          {(tab === "general" || tab === "language") && (
            <div className="setting-row">
              <span>{t.language}</span>
              <select
                aria-label={t.language}
                value={preferences.language}
                onChange={(event) =>
                  void changePreferences({
                    ...preferences,
                    language: event.target.value as Preferences["language"],
                  })
                }
              >
                <option value="zh">{t.zh}</option>
                <option value="en">{t.en}</option>
              </select>
            </div>
          )}
          {tab === "models" && (
            <>
              <div className="section-heading">
                <span className="muted">{t.keyDestination}</span>
                <button
                  disabled={!!operation}
                  onClick={() =>
                    edit({
                      id: crypto.randomUUID(),
                      name: "OpenAI",
                      kind: "openai",
                      endpoint: providers[0].endpoint,
                      model: "",
                    })
                  }
                >
                  <Plus size={16} />
                  {t.addModel}
                </button>
              </div>
              {editing ? (
                <form
                  className="profile-form"
                  onSubmit={(event) => void save(event)}
                >
                  <label>
                    {t.provider}
                    <select
                      aria-label={t.provider}
                      disabled={profiles.some(
                        (row) => row.profile.id === editing.id,
                      )}
                      value={editing.kind}
                      onChange={(event) => {
                        const preset = providers.find(
                          (item) => item.kind === event.target.value,
                        )!;
                        setEditing({
                          ...editing,
                          kind: preset.kind,
                          name: preset.name,
                          endpoint: preset.endpoint,
                          model: "",
                        });
                        setKey("");
                      }}
                    >
                      {providers.map((item) => (
                        <option value={item.kind} key={item.kind}>
                          {item.name}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label>
                    {t.name}
                    <input
                      required
                      maxLength={128}
                      value={editing.name}
                      onChange={(event) =>
                        setEditing({ ...editing, name: event.target.value })
                      }
                    />
                  </label>
                  <label>
                    {t.nativeModel}
                    <input
                      required
                      maxLength={256}
                      placeholder={
                        providers.find((item) => item.kind === editing.kind)
                          ?.example
                      }
                      value={editing.model}
                      onChange={(event) =>
                        setEditing({ ...editing, model: event.target.value })
                      }
                    />
                  </label>
                  <label>
                    {t.endpoint}
                    <input
                      required
                      maxLength={2048}
                      placeholder="https://…"
                      value={editing.endpoint}
                      onChange={(event) =>
                        setEditing({ ...editing, endpoint: event.target.value })
                      }
                    />
                  </label>
                  <label>
                    {t.key}
                    <input
                      type="password"
                      autoComplete="off"
                      spellCheck={false}
                      maxLength={16384}
                      placeholder={t.keyPlaceholder}
                      value={key}
                      onChange={(event) => setKey(event.target.value)}
                    />
                  </label>
                  <p className="field-help">
                    <KeyRound size={14} />
                    {t.keyHelp}
                  </p>
                  <p className="field-help">{t.basicOnly}</p>
                  <div className="form-actions">
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => {
                        setEditing(null);
                        setKey("");
                      }}
                    >
                      {t.cancel}
                    </button>
                    <button
                      type="submit"
                      className="primary"
                      disabled={busy || !!operation}
                    >
                      {busy ? t.loading : t.save}
                    </button>
                  </div>
                </form>
              ) : (
                <div className="profiles">
                  {!profiles.length && <p className="muted">{t.modelHint}</p>}
                  {profiles.map((row) => (
                    <div className="profile-row" key={row.profile.id}>
                      <span className="provider-avatar">
                        {row.profile.name[0]}
                      </span>
                      <div>
                        <strong>{row.profile.name}</strong>
                        <small>{row.profile.model}</small>
                        <small
                          className={
                            row.configured ? "key-configured" : "muted"
                          }
                        >
                          {row.configured
                            ? t.configured
                            : row.profile.kind === "ollama"
                              ? t.noKey
                              : t.unconfigured}
                        </small>
                      </div>
                      <button
                        disabled={!!operation}
                        onClick={() => edit(row.profile)}
                      >
                        {t.edit}
                      </button>
                      {row.configured && (
                        <button
                          className="icon"
                          disabled={!!operation}
                          aria-label={`${t.removeKey} ${row.profile.name}`}
                          onClick={() => void remove(row.profile.id)}
                        >
                          <KeyRound size={17} />
                        </button>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </>
          )}
          {tab === "about" && (
            <div className="about">
              <img src="/caidex-symbol.png" alt="CAIdex" />
              <h3>CAIdex</h3>
              <p>{t.version} 0.1.0 · J-1</p>
              <p>{t.localNotice}</p>
              <p className="muted">{t.privacy}</p>
              <details>
                <summary>{t.licenses}</summary>
                <p>CAIdex dependency notices</p>
                <p>React · MIT / Tauri · MIT or Apache-2.0 / Lucide · ISC</p>
                <p>
                  Rust dependencies: see the source distribution and upstream
                  license notices.
                </p>
              </details>
            </div>
          )}
          {tab === "appearance" && (
            <div className="theme-previews">
              {(["light", "dark", "system"] as const).map((theme) => (
                <button
                  key={theme}
                  className={preferences.theme === theme ? "selected" : ""}
                  onClick={() =>
                    void changePreferences({ ...preferences, theme })
                  }
                >
                  {theme === "dark" ? (
                    <Moon size={25} />
                  ) : theme === "light" ? (
                    <Sun size={25} />
                  ) : (
                    <Monitor size={25} />
                  )}
                  <span>{t[theme]}</span>
                </button>
              ))}
            </div>
          )}
          {tab === "general" && <p className="field-help">{t.localNotice}</p>}
        </div>
      </div>
    </div>
  );
}
