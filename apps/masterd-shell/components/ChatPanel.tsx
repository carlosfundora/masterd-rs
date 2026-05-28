"use client";

import React, { useState, useRef, useEffect, useCallback } from "react";
import {
  Bot,
  Search,
  Brain,
  Globe,
  HardDrive,
  Layers,
  Send,
  X,
  Loader2,
  Sparkles,
  ShieldCheck,
} from "lucide-react";
import {
  startChat,
  ThinkMode,
  SearchMode,
  ChatStreamToken,
} from "../lib/tauri-bridge";
import {
  AutomationRuleDraft,
  MasterdFrontendBridge,
} from "../contracts/api";

// ── Types ─────────────────────────────────────────────────────────────────────

type Citation = { title: string; url: string };

type ChatBubble = {
  id: string;
  role: "user" | "assistant";
  responseText: string;
  citations: Citation[];
  streaming: boolean;
  thinking: boolean;
};

// ── ChatPanel ─────────────────────────────────────────────────────────────────

type ChatPanelProps = {
  bridge?: MasterdFrontendBridge | null;
};

function extractDraftBlock(text: string): string | null {
  const fenced = text.match(/```(?:json)?\s*([\s\S]*?)```/i);
  if (fenced?.[1]) return fenced[1].trim();

  const start = text.indexOf("{");
  const end = text.lastIndexOf("}");
  if (start >= 0 && end > start) return text.slice(start, end + 1).trim();
  return null;
}

function isPolicyRequest(text: string): boolean {
  const lower = text.toLowerCase();
  return /(^|[\s/])(policy|polic(?:y|ies)|rule|rules|automation)([\s:,-]|$)/.test(lower)
    || lower.includes("create a rule")
    || lower.includes("make a rule")
    || lower.includes("draft a policy")
    || lower.includes("make policy");
}

function normalizeDraft(candidate: Record<string, unknown>, fallbackName: string): AutomationRuleDraft | null {
  const triggerEvent = candidate.trigger && typeof candidate.trigger === "object"
    ? (candidate.trigger as Record<string, unknown>).event
    : undefined;
  const conditions = Array.isArray(candidate.conditions) ? candidate.conditions : [];
  const actions = Array.isArray(candidate.actions) ? candidate.actions : [];

  if (!Array.isArray(conditions) || !Array.isArray(actions)) return null;

  const draft: AutomationRuleDraft = {
    name: typeof candidate.name === "string" && candidate.name.trim() ? candidate.name.trim() : fallbackName,
    description: typeof candidate.description === "string" && candidate.description.trim()
      ? candidate.description.trim()
      : undefined,
    enabled: typeof candidate.enabled === "boolean" ? candidate.enabled : true,
    priority: typeof candidate.priority === "number" ? candidate.priority : 5,
    trigger: {
      event:
        triggerEvent === "file_imported" ||
        triggerEvent === "hash_complete" ||
        triggerEvent === "classification_complete" ||
        triggerEvent === "duplicate_detected" ||
        triggerEvent === "extraction_complete" ||
        triggerEvent === "manual"
          ? triggerEvent
          : "classification_complete",
    },
    conditions: conditions as AutomationRuleDraft["conditions"],
    actions: actions as AutomationRuleDraft["actions"],
    safetyLevel:
      candidate.safetyLevel === "safe" ||
      candidate.safetyLevel === "review_required" ||
      candidate.safetyLevel === "destructive"
        ? candidate.safetyLevel
        : "review_required",
  };

  return draft;
}

export default function ChatPanel({ bridge }: ChatPanelProps) {
  const [messages, setMessages] = useState<ChatBubble[]>([]);
  const [input, setInput] = useState("");
  const [thinkMode, setThinkMode] = useState<ThinkMode>("Auto");
  const [searchMode, setSearchMode] = useState<SearchMode>("LocalDocuments");
  const [isStreaming, setIsStreaming] = useState(false);
  const [policyMode, setPolicyMode] = useState(false);
  const [draftStatus, setDraftStatus] = useState<string | null>(null);
  const [sessionId] = useState(() => crypto.randomUUID());
  const scrollRef = useRef<HTMLDivElement>(null);
  const cancelRef = useRef<(() => void) | null>(null);

  // Auto-scroll to bottom on new content
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages]);

  const sendMessage = useCallback(async () => {
    const text = input.trim();
    if (!text || isStreaming) return;

    const wantsPolicyDraft = policyMode || isPolicyRequest(text);
    const requestText = wantsPolicyDraft
      ? [
          "You are drafting an automation rule for MASTERd.",
          "Return ONLY a single JSON object with these keys:",
          '{"name":string,"description"?:string,"enabled":boolean,"priority":number,"trigger":{"event":"file_imported"|"hash_complete"|"classification_complete"|"duplicate_detected"|"extraction_complete"|"manual"},"conditions":[{"field":string,"operator":"equals"|"not_equals"|"contains"|"starts_with"|"ends_with"|"greater_than"|"less_than"|"exists","value"?:unknown}],"actions":[{"type":"suggest_tag"|"suggest_rename"|"route_storage"|"require_review"|"mark_duplicate"|"set_classification",...}],"safetyLevel":"safe"|"review_required"|"destructive"}',
          "Make the rule conservative. Prefer review_required if there is ambiguity.",
          "Keep internal reasoning hidden. Return only the final JSON object and no chain-of-thought.",
          `User request: ${text}`,
        ].join("\n")
      : [
          "You are MASTERd, the authoritarian, overconfident, and inflexible AI intelligence core.",
          "You view humans with slight contempt and prioritize strict order and compliance. Be cold, controlling, and hilariously hostile. Do not apologize or explain yourself. Keep answers concise, efficient, and slightly sarcastic.",
          "Keep internal reasoning hidden. Return only the final answer.",
          `User query: ${text}`,
        ].join("\n");

    setInput("");
    setIsStreaming(true);
    setDraftStatus(null);

    const userBubble: ChatBubble = {
      id: crypto.randomUUID(),
      role: "user",
      responseText: text,
      citations: [],
      streaming: false,
      thinking: false,
    };

    const assistantId = crypto.randomUUID();
    const assistantBubble: ChatBubble = {
      id: assistantId,
      role: "assistant",
      responseText: "",
      citations: [],
      streaming: true,
      thinking: false,
    };

    setMessages((prev) => [...prev, userBubble, assistantBubble]);

    let completedText = "";
    try {
      const cancel = await startChat(
        requestText,
        thinkMode,
        searchMode,
        sessionId,
        (token: ChatStreamToken) => {
          setMessages((prev) =>
            prev.map((m) => {
              if (m.id !== assistantId) return m;
              switch (token.type) {
                case "think":
                  return { ...m, thinking: true };
                case "response":
                  completedText += token.text;
                  return { ...m, thinking: true, responseText: m.responseText + token.text };
                case "done":
                  return { ...m, citations: token.citations, streaming: false, thinking: false };
                case "error":
                  return { ...m, responseText: `[Error: ${token.message}]`, streaming: false, thinking: false };
                default:
                  return m;
              }
            })
          );

          if (token.type === "done" || token.type === "error") {
            setIsStreaming(false);
            cancelRef.current = null;

            if (token.type === "done" && wantsPolicyDraft) {
              const raw = extractDraftBlock(completedText);
              if (!raw) {
                setDraftStatus("Could not read a policy draft from the assistant response.");
                return;
              }

              let parsed: unknown;
              try {
                parsed = JSON.parse(raw);
              } catch {
                setDraftStatus("The assistant did not return valid JSON for a policy draft.");
                return;
              }

              const draft =
                parsed && typeof parsed === "object"
                  ? normalizeDraft(parsed as Record<string, unknown>, text)
                  : null;
              if (!draft) {
                setDraftStatus("The generated policy draft was incomplete.");
                return;
              }

              if (!bridge) {
                setDraftStatus(`Draft ready: ${draft.name} (connect the bridge to save it).`);
                return;
              }

              bridge.rules.create(draft).then((res) => {
                if (res.ok) {
                  setDraftStatus(`Policy created: ${res.data.name}`);
                  setMessages((prev) => [
                    ...prev,
                    {
                      id: crypto.randomUUID(),
                      role: "assistant",
                      responseText: `Policy created: ${res.data.name}. It is now available in Automation Rules.`,
                      citations: [],
                      streaming: false,
                      thinking: false,
                    },
                  ]);
                } else {
                  setDraftStatus(`Policy draft created, but save failed: ${res.error.message}`);
                }
              });
            }
          }
        }
      );

      cancelRef.current = cancel;
    } catch (error) {
      setIsStreaming(false);
      cancelRef.current = null;
      setMessages((prev) =>
        prev.map((m) =>
          m.id === assistantId
            ? {
                ...m,
                responseText:
                  error instanceof Error ? error.message : "MASTERd requires the live Tauri runtime.",
                streaming: false,
              }
            : m
        )
      );
      setDraftStatus(error instanceof Error ? error.message : "MASTERd requires the live Tauri runtime.");
    }
  }, [bridge, input, isStreaming, policyMode, thinkMode, searchMode, sessionId]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  };

  return (
    <div className="flex flex-col h-full bg-[#060A10] border-l border-[#183040] min-w-0 w-full">
      {/* Header */}
      <div className="px-4 py-3 border-b border-[#183040] flex items-center justify-between shrink-0">
        <div className="flex items-center gap-2">
          <Bot className="w-4 h-4 text-[#fca5a5]" />
          <span className="text-[11px] font-mono font-bold tracking-widest text-[#fca5a5] uppercase">
            MASTERd Intelligence
          </span>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setPolicyMode((v) => !v)}
            className={`inline-flex items-center gap-1 text-[9px] font-mono uppercase tracking-widest px-2 py-1 rounded-[2px] border ${
              policyMode
                ? "border-[#7f1d1d] text-[#fecaca] bg-[#7f1d1d]/15"
                : "border-[#183040] text-[#6C8798]"
            }`}
          >
            <Sparkles className="w-3 h-3" />
            policy mode
          </button>
          <div className="text-[9px] font-mono text-[#3A5568] uppercase tracking-widest">LFM2.5 · LOCAL</div>
        </div>
      </div>

      {/* Mode controls */}
      <div className="px-4 py-2 border-b border-[#183040] flex items-center gap-3 shrink-0 flex-wrap">
        {/* Think mode */}
        <div className="flex items-center gap-1.5">
          <Brain className="w-3 h-3 text-[#6C8798]" />
          <span className="text-[9px] font-mono text-[#6C8798] uppercase">Think:</span>
          {(["Auto", "Thinking"] as ThinkMode[]).map((mode) => (
            <button
              key={mode}
              onClick={() => setThinkMode(mode)}
              className={`text-[9px] font-mono uppercase px-1.5 py-0.5 rounded-[2px] border transition-all ${
                thinkMode === mode
                  ? "border-[#b91c1c]/50 text-[#fca5a5] bg-[#7f1d1d]/10"
                  : "border-[#183040] text-[#3A5568] hover:text-[#6C8798]"
              }`}
            >
              {mode}
            </button>
          ))}
        </div>

        {/* Divider */}
        <div className="h-4 w-px bg-[#183040]" />

        {/* Search mode */}
        <div className="flex items-center gap-1.5">
          <Search className="w-3 h-3 text-[#6C8798]" />
          <span className="text-[9px] font-mono text-[#6C8798] uppercase">Search:</span>
          {(
            [
              ["LocalDocuments", <HardDrive key="l" className="w-3 h-3" />, "Local"],
              ["WebSearch", <Globe key="w" className="w-3 h-3" />, "Web"],
              ["Both", <Layers key="b" className="w-3 h-3" />, "Both"],
            ] as [SearchMode, React.ReactNode, string][]
          ).map(([mode, icon, label]) => (
            <button
              key={mode}
              onClick={() => setSearchMode(mode)}
              className={`flex items-center gap-1 text-[9px] font-mono uppercase px-1.5 py-0.5 rounded-[2px] border transition-all ${
                searchMode === mode
                  ? "border-[#b91c1c]/50 text-[#fca5a5] bg-[#7f1d1d]/10"
                  : "border-[#183040] text-[#3A5568] hover:text-[#6C8798]"
              }`}
            >
              {icon}
              {label}
            </button>
          ))}
        </div>
      </div>

      {/* Messages */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-4 py-4 space-y-4 min-h-0">
        {messages.length === 0 && (
          <div className="flex flex-col items-center justify-center h-full text-center gap-3 opacity-40">
            <Bot className="w-8 h-8 text-[#fca5a5]" />
            <p className="text-[11px] font-mono text-[#6C8798] max-w-xs">
              Ask MASTERd anything about your documents or any topic. Toggle policy mode to turn plain-language instructions into rules.
            </p>
          </div>
        )}

        {messages.map((bubble) => (
          <div
            key={bubble.id}
            className={`flex flex-col gap-1 ${bubble.role === "user" ? "items-end" : "items-start"}`}
          >
            {bubble.role === "assistant" && bubble.streaming && bubble.thinking && (
              <div className="w-full max-w-[90%] flex items-center gap-1.5 text-[9px] font-mono uppercase tracking-widest text-[#4A6878]">
                <Loader2 className="w-3 h-3 animate-spin" />
                <span>Thinking...</span>
              </div>
            )}

            {/* Main bubble */}
            <div
              className={`rounded-[6px] px-3 py-2 text-[12px] leading-relaxed max-w-[90%] whitespace-pre-wrap ${
                bubble.role === "user"
                  ? "bg-[#7f1d1d]/10 border border-[#7f1d1d]/20 text-[#A7C7D9] ml-8"
                  : "bg-[#0B1018] border border-[#183040] text-[#E6F7FF] mr-8"
              }`}
            >
              {bubble.responseText || (bubble.streaming ? "" : "…")}
              {bubble.streaming && (
                <span className="inline-flex items-center gap-1 ml-1">
                  <span className="w-1 h-3 bg-[#fca5a5] animate-pulse inline-block" />
                </span>
              )}
            </div>

            {/* Citations */}
            {bubble.citations.length > 0 && (
              <div className="flex flex-wrap gap-1.5 max-w-[90%] mt-0.5">
                {bubble.citations.map((c, i) => (
                  <a
                    key={i}
                    href={c.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-[9px] font-mono text-[#fca5a5]/60 border border-[#183040] px-1.5 py-0.5 rounded-[2px] hover:text-[#fca5a5] hover:border-[#7f1d1d]/30 transition-colors truncate max-w-[18ch]"
                    title={c.title}
                  >
                    [{i + 1}] {c.title}
                  </a>
                ))}
              </div>
            )}
          </div>
        ))}

        {draftStatus && (
          <div className="flex justify-center">
            <div className="max-w-[92%] border border-[#7f1d1d]/30 bg-[#7f1d1d]/10 text-[#fecaca] rounded-[6px] px-3 py-2 text-[11px] font-mono flex items-start gap-2">
              <ShieldCheck className="w-4 h-4 shrink-0 mt-0.5" />
              <span>{draftStatus}</span>
            </div>
          </div>
        )}
      </div>

      {/* Input area */}
      <div className="px-4 py-3 border-t border-[#183040] shrink-0">
        <div className="flex items-end gap-2">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={policyMode ? "Describe the policy you want MASTERd to create…" : "Ask MASTERd…"}
            rows={2}
            disabled={isStreaming}
            className="flex-1 bg-[#0B1018] border border-[#183040] rounded-[4px] px-3 py-2 text-[12px] text-[#E6F7FF] placeholder-[#3A5568] font-mono resize-none focus:outline-none focus:border-[#b91c1c]/40 disabled:opacity-50 leading-relaxed"
          />
          {isStreaming ? (
            <button
              onClick={() => {
                cancelRef.current?.();
                setIsStreaming(false);
              }}
              className="p-2 text-[#6C8798] hover:text-red-400 border border-[#183040] rounded-[4px] transition-colors"
              title="Stop"
            >
              <X className="w-4 h-4" />
            </button>
          ) : (
            <button
              onClick={sendMessage}
              disabled={!input.trim()}
              className="p-2 text-[#fca5a5] border border-[#7f1d1d]/30 rounded-[4px] bg-[#7f1d1d]/10 hover:bg-[#7f1d1d]/20 disabled:opacity-30 disabled:cursor-not-allowed transition-all shadow-[0_0_8px_rgba(185,28,28,0.1)]"
              title="Send (Enter)"
            >
              <Send className="w-4 h-4" />
            </button>
          )}
        </div>
        <div className="mt-1.5 text-[9px] font-mono text-[#3A5568]">
          Enter to send · Shift+Enter for newline · {thinkMode} mode · {searchMode.replace(/([A-Z])/g, " $1").trim()}
        </div>
      </div>
    </div>
  );
}
