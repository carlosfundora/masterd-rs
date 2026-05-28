"use client";

import React, { useState, useEffect, useCallback } from "react";
import { 
  Activity, Database, LayoutDashboard, Inbox, Layers, Settings, ShieldAlert, 
  History, Sliders, Play, Terminal, HelpCircle, HardDrive, Cpu, AlertTriangle, 
  CheckCircle2, Flame, Bot, BookOpen, ChevronRight, Lock, Minimize2, MessageSquare
} from "lucide-react";

// Types
import { 
  SystemStatus, SystemHealth, IntakeItem, WatchFolder, DocumentRecord, 
  PipelineJob, ReviewItem, AutomationRule, AuditEntry, MasterdFrontendBridge
} from "../contracts/api";

// Bridge (live Tauri runtime only)
import { getBridge } from "../lib/tauri-bridge";

// Subcomponents
import Dashboard from "../components/Dashboard";
import Intake from "../components/Intake";
import Documents from "../components/Documents";
import Pipeline from "../components/Pipeline";
import ReviewQueue from "../components/ReviewQueue";
import Rules from "../components/Rules";
import AuditLog from "../components/AuditLog";
import SettingsScreen from "../components/Settings";
import ChatPanel from "../components/ChatPanel";
import WelcomeTour from "../components/WelcomeTour";

const ONBOARDING_STORAGE_KEY = "masterd:onboarding:v1-complete";

export default function Home() {
  const [bridge, setBridge] = useState<MasterdFrontendBridge | null>(null);
  const [runtimeError, setRuntimeError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<string>("dashboard");
  const [initLoaded, setInitLoaded] = useState(false);
  const [chatOpen, setChatOpen] = useState(false);
  const [tutorialOpen, setTutorialOpen] = useState(false);
  const [tutorialStep, setTutorialStep] = useState(0);

  // Synced system variables
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [health, setHealth] = useState<SystemHealth | null>(null);
  
  // Watcher folders state
  const [watchFolders, setWatchFolders] = useState<WatchFolder[]>([]);

  // Sub-panel registers
  const [documents, setDocuments] = useState<DocumentRecord[]>([]);
  const [intakeQueue, setIntakeQueue] = useState<IntakeItem[]>([]);
  const [jobs, setJobs] = useState<PipelineJob[]>([]);
  const [reviewQueue, setReviewQueue] = useState<ReviewItem[]>([]);
  const [rules, setRules] = useState<AutomationRule[]>([]);
  const [auditLog, setAuditLog] = useState<AuditEntry[]>([]);

  // Currently selected doc for multi-surface focus state
  const [selectedDocument, setSelectedDocument] = useState<DocumentRecord | null>(null);

  // Query state in real time
  const refreshState = useCallback(async (activeBridge?: MasterdFrontendBridge) => {
    const b = activeBridge || bridge;
    if (!b) return;
    try {
      const statusRes = await b.system.getStatus();
      if (statusRes.ok) setStatus(statusRes.data);

      const healthRes = await b.system.getHealth();
      if (healthRes.ok) setHealth(healthRes.data);

      const docsRes = await b.documents.search({});
      if (docsRes.ok) setDocuments(docsRes.data.items);

      const queueRes = await b.intake.listQueue({});
      if (queueRes.ok) setIntakeQueue(queueRes.data.items);

      const foldersRes = await b.intake.listWatchFolders();
      if (foldersRes.ok) setWatchFolders(foldersRes.data);

      const jobsRes = await b.pipeline.listJobs({});
      if (jobsRes.ok) setJobs(jobsRes.data.items);

      const reviewRes = await b.review.list({});
      if (reviewRes.ok) setReviewQueue(reviewRes.data.items);

      const rulesRes = await b.rules.list();
      if (rulesRes.ok) setRules(rulesRes.data);

      const auditRes = await b.audit.list({});
      if (auditRes.ok) setAuditLog(auditRes.data.items);

    } catch (err) {
      console.error("Critical error syncing frontend with bridge", err);
    }
  }, [bridge]);

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let telemetryInterval: ReturnType<typeof setInterval> | undefined;
    let cancelled = false;

    const initBoot = async () => {
      try {
        const b = await getBridge();
        if (cancelled) return;
        setBridge(b);
        await refreshState(b);
        if (cancelled) return;
        setInitLoaded(true);

        // Subscribe to backend events
        unsubscribe = b.events.subscribe(() => refreshState(b));

        // Hardware telemetry every 3 seconds
        telemetryInterval = setInterval(async () => {
          const healthRes = await b.system.getHealth();
          if (healthRes.ok) setHealth(healthRes.data);
          const statusRes = await b.system.getStatus();
          if (statusRes.ok) setStatus(statusRes.data);
        }, 3000);
      } catch (err) {
        if (!cancelled) {
          setRuntimeError(err instanceof Error ? err.message : "MASTERd requires the live Tauri desktop runtime.");
          setInitLoaded(true);
        }
      }
    };

    initBoot();
    return () => {
      cancelled = true;
      unsubscribe?.();
      if (telemetryInterval) clearInterval(telemetryInterval);
    };
  }, [refreshState]);

  useEffect(() => {
    if (!initLoaded || runtimeError) return;
    if (typeof window === "undefined") return;

    const completed = window.localStorage.getItem(ONBOARDING_STORAGE_KEY) === "true";
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setTutorialOpen(!completed);
  }, [initLoaded, runtimeError]);

  const closeTutorial = useCallback(() => {
    if (typeof window !== "undefined") {
      window.localStorage.setItem(ONBOARDING_STORAGE_KEY, "true");
    }
    setTutorialOpen(false);
  }, []);

  const replayTutorial = useCallback(() => {
    setTutorialStep(0);
    setTutorialOpen(true);
  }, []);

  const activeWatchFolders: WatchFolder[] = watchFolders.length > 0 ? watchFolders : [
    { id: "wf-1", path: "/Users/username/Desktop/Tax2025", enabled: true, profileId: "Receipts & Financial Docs", fileCount: 28, createdAt: "" },
    { id: "wf-2", path: "/Users/username/Downloads/Invoices", enabled: true, profileId: "Fast Scan", fileCount: 114, createdAt: "" },
    { id: "wf-3", path: "/Users/username/Documents/ScannedCorrespondence", enabled: false, profileId: "Full Analysis", fileCount: 8, createdAt: "" }
  ];

  const errorCount = intakeQueue.filter(item => item.status === "error").length;
  const pendingReviewCount = reviewQueue.filter(r => !r.resolved).length;

  return (
    runtimeError ? (
      <main className="min-h-screen bg-[var(--masterd-bg)] text-[var(--masterd-text)] flex items-center justify-center p-6">
        <div className="max-w-lg w-full bg-[var(--masterd-surface)] border border-[var(--masterd-border)] rounded-[8px] p-6 space-y-3">
          <div className="text-xs uppercase tracking-[0.2em] text-[#fca5a5] font-mono">Live runtime required</div>
          <h1 className="text-2xl font-semibold text-[#f4f4f5]">MASTERd needs the desktop backend</h1>
          <p className="text-sm text-[#a1a1aa]">
            {runtimeError}
          </p>
          <p className="text-xs text-[#71717a] font-mono">
            Launch with <span className="text-[#fca5a5]">pnpm dev</span> or <span className="text-[#fca5a5]">cargo tauri dev</span>.
          </p>
        </div>
      </main>
    ) : (
    <main id="app-root" className="min-h-screen bg-[var(--masterd-bg)] text-[var(--masterd-text)] flex overflow-hidden font-sans select-none selection:bg-[#7f1d1d] selection:text-white">
      
      {/* Navigation Sidebar Panel */}
      <aside id="app-sidebar" className="w-60 bg-[var(--masterd-surface)] border-r border-[var(--masterd-border)] hidden md:flex flex-col justify-between shrink-0 z-20">
        <div>
          <div className="p-5 flex items-center gap-3 border-b border-[var(--masterd-border)]">
            <div className="w-6 h-6 border border-[#7f1d1d] shadow-[0_0_8px_rgba(127,29,29,0.35)] flex items-center justify-center text-[10px] font-mono font-bold text-[#fca5a5] bg-[#7f1d1d]/15 rounded-[4px]">M</div>
            <span className="font-semibold tracking-[0.2em] text-xs text-[#fca5a5] font-mono">MASTERD</span>
          </div>

          <div className="p-4 space-y-6">
            <div>
              <span className="text-[10px] text-[var(--masterd-smoke)] uppercase tracking-[0.16em] font-mono px-3">Primary Areas</span>
              <div className="space-y-1 mt-2.5">
                {[
                  { id: "dashboard", label: "Dashboard", icon: <LayoutDashboard className="w-4 h-4" /> },
                  { id: "intake", label: "Document Intake", icon: <Inbox className="w-4 h-4" />, count: intakeQueue.filter(i => i.status !== "complete" && i.status !== "error").length },
                  { id: "documents", label: "Documents Database", icon: <Database className="w-4 h-4" /> },
                  { id: "pipeline", label: "Execution Pipeline", icon: <Layers className="w-4 h-4" /> },
                  { id: "rules", label: "Automation Rules", icon: <Sliders className="w-4 h-4" /> }
                ].map(item => (
                  <button
                    key={item.id}
                    onClick={() => {
                      setActiveTab(item.id);
                      if (item.id !== "documents") setSelectedDocument(null);
                    }}
                    className={`w-full flex items-center justify-between px-3 py-2 rounded-[4px] cursor-pointer text-xs font-semibold uppercase tracking-wider transition-all border-r-2 ${
                      activeTab === item.id 
                        ? "bg-[#18181b] text-[#fca5a5] border-[#b91c1c] shadow-[0_0_8px_rgba(127,29,29,0.12)]" 
                        : "text-[#a1a1aa] hover:text-[#fca5a5] hover:bg-[#18181b]/70 border-transparent"
                    }`}
                  >
                    <div className="flex items-center gap-2.5">
                      {item.icon}
                      <span className="text-[11px]">{item.label}</span>
                    </div>
                    {item.count ? (
                      <span className="bg-[#7f1d1d]/20 text-[#fca5a5] text-[9.5px] font-mono font-bold px-1.5 py-0.2 rounded-[2px] shadow-[0_0_4px_rgba(127,29,29,0.2)]">
                        {item.count}
                      </span>
                    ) : null}
                  </button>
                ))}
              </div>
            </div>

            <div>
              <span className="text-[10px] text-[var(--masterd-smoke)] uppercase tracking-[0.16em] font-mono px-3">Triage & Supervision</span>
              <div className="space-y-1 mt-2.5">
                {[
                  { id: "review", label: "Review Queue", icon: <ShieldAlert className="w-4 h-4" />, count: pendingReviewCount, alert: pendingReviewCount > 0 },
                  { id: "audit", label: "Journal Audit Log", icon: <History className="w-4 h-4" /> }
                ].map(item => (
                  <button
                    key={item.id}
                    onClick={() => setActiveTab(item.id)}
                    className={`w-full flex items-center justify-between px-3 py-2 rounded-[4px] cursor-pointer text-xs font-semibold uppercase tracking-wider transition-all border-r-2 ${
                      activeTab === item.id 
                        ? "bg-[#18181b] text-[#fca5a5] border-[#b91c1c] shadow-[0_0_8px_rgba(127,29,29,0.12)]" 
                        : "text-[#a1a1aa] hover:text-[#fca5a5] hover:bg-[#18181b]/70 border-transparent"
                    }`}
                  >
                    <div className="flex items-center gap-2.5">
                      {item.icon}
                      <span className="text-[11px]">{item.label}</span>
                    </div>
                    {item.count ? (
                      <span className={`text-[9.5px] font-mono font-bold px-1.5 py-0.2 rounded-[2px] ${
                        item.alert ? "bg-amber-400/10 text-amber-500 border border-amber-500/20 shadow-[0_0_4px_rgba(245,158,11,0.2)]" : "bg-gray-500/10 text-gray-400"
                      }`}>
                        {item.count}
                      </span>
                    ) : null}
                  </button>
                ))}
              </div>
            </div>

            <div>
              <span className="text-[10px] text-[var(--masterd-smoke)] uppercase tracking-[0.16em] font-mono px-3">System Settings</span>
              <div className="space-y-1 mt-2.5">
                <button
                  onClick={() => setActiveTab("settings")}
                  className={`w-full flex items-center justify-between px-3 py-2 rounded-[4px] cursor-pointer text-xs font-semibold uppercase tracking-wider transition-all border-r-2 ${
                    activeTab === "settings" 
                      ? "bg-[#18181b] text-[#fca5a5] border-[#b91c1c] shadow-[0_0_8px_rgba(127,29,29,0.12)]" 
                      : "text-[#a1a1aa] hover:text-[#fca5a5] hover:bg-[#18181b]/70 border-transparent"
                  }`}
                >
                  <div className="flex items-center gap-2.5">
                    <Settings className="w-4 h-4" />
                    <span className="text-[11px]">Configuration settings</span>
                  </div>
                </button>
              </div>
            </div>
          </div>
        </div>

        {/* User badge */}
        <div className="p-4 border-t border-[var(--masterd-border)] flex flex-col gap-2 bg-[var(--masterd-surface)]">
          <div className="flex items-center gap-2.5 px-1">
            <div className="w-8 h-8 rounded-[4px] bg-[#7f1d1d]/10 border border-[#7f1d1d]/20 flex items-center justify-center font-mono font-bold text-xs text-[#fca5a5] shadow-[0_0_6px_rgba(127,29,29,0.12)]">
              SE
            </div>
            <div className="min-w-0 flex-1">
              <div className="text-[11px] font-semibold text-[#f4f4f5] truncate">sentseven@gmail.com</div>
              <div className="text-[9px] uppercase font-mono text-[var(--masterd-smoke)]">Supervisor Level</div>
            </div>
          </div>

          <div className="bg-[#09090b] p-2.5 border border-[var(--masterd-border)] rounded-[4px] flex items-center justify-between text-[10px] text-[var(--masterd-smoke)] font-mono">
            <span className="flex items-center gap-1">
              <Lock className="w-3 h-3 text-green-500" /> Sandboxed
            </span>
            <span className="text-green-400 font-bold bg-green-500/10 px-1 py-0.2 rounded text-[8px] uppercase">LOCAL-ONLY</span>
          </div>
        </div>
      </aside>

      {/* Main content + optional Chat panel */}
      <div className="flex-grow flex min-w-0 overflow-hidden">

        {/* Main workspace column */}
        <div className={`flex flex-col min-w-0 bg-[var(--masterd-bg)] relative overflow-hidden transition-all duration-300 ${chatOpen ? "flex-[2]" : "flex-1"}`}>
          
          {/* Futuristic grid background */}
          <div className="absolute inset-0 pointer-events-none opacity-[0.03] z-0" style={{ backgroundImage: "linear-gradient(#b91c1c 1px, transparent 1px), linear-gradient(90deg, #b91c1c 1px, transparent 1px)", backgroundSize: "40px 40px" }} />

          {/* Top bar */}
          <header id="app-topbar" className="h-14 bg-[var(--masterd-surface)]/85 backdrop-blur-md border-b border-[var(--masterd-border)] flex items-center justify-between px-6 shrink-0 z-10 relative">
            <div className="flex items-center gap-4">
              <span className="font-bold tracking-wider text-xs text-[#E6F7FF] uppercase">System Dashboard</span>
              <div className="h-4 w-[1px] bg-[#183040]"></div>
              <div className="flex items-center gap-2">
                <div className="w-2 h-2 rounded-full bg-[#b91c1c] animate-pulse shadow-[0_0_8px_#b91c1c]"></div>
                <span className="text-[10px] font-mono text-[#fca5a5] tracking-wide">LOCAL ENGINE: ONLINE</span>
              </div>
            </div>

            <div className="flex items-center gap-4 text-xs font-mono">
              <div className="hidden sm:flex items-center gap-1.5 text-[#a1a1aa]">
                <span className="text-[var(--masterd-smoke)]">Active Core:</span>
                <span className="text-green-400 font-bold bg-green-500/10 px-1.5 py-0.2 border border-green-500/15 rounded-[2px] uppercase text-[9px]">
                  {status?.engine || "Starting..."}
                </span>
              </div>

              <div className="hidden md:flex items-center gap-1.5 text-[#a1a1aa]">
                <span className="text-[var(--masterd-smoke)]">Active Models:</span>
                <span className="text-[#fca5a5] font-bold bg-[#7f1d1d]/10 px-1.5 py-0.2 border border-[#7f1d1d]/15 rounded-[2px] uppercase text-[9px]">
                  {status?.models.filter(m => m.status === "online").length || 4} On-Duty
                </span>
              </div>

              <div className="text-[10px] font-mono text-[var(--masterd-smoke)] hidden lg:block">v1.4.2-stable</div>

              {errorCount > 0 && (
                <div className="bg-red-500/10 border border-red-500/20 text-red-400 text-[9px] uppercase font-bold px-2 py-0.2 rounded-[2px] animate-pulse flex items-center gap-1 shadow-[0_0_8px_rgba(239,68,68,0.2)]">
                  <AlertTriangle className="w-3.5 h-3.5" />
                  <span>{errorCount} Sandbox Error</span>
                </div>
              )}

              {/* Chat toggle */}
              <button
                onClick={() => setChatOpen((o) => !o)}
                title="Toggle MASTERd Intelligence chat"
                className={`flex items-center gap-1.5 px-2.5 py-1 rounded-[4px] border text-[9px] font-mono uppercase transition-all ${
                  chatOpen
                    ? "border-[#b91c1c]/50 text-[#fca5a5] bg-[#7f1d1d]/10 shadow-[0_0_8px_rgba(127,29,29,0.15)]"
                    : "border-[var(--masterd-border)] text-[var(--masterd-smoke)] hover:text-[#fca5a5] hover:border-[#b91c1c]/30"
                }`}
              >
                <MessageSquare className="w-3.5 h-3.5" />
                <span className="hidden sm:inline">Intelligence</span>
              </button>

              <button
                onClick={replayTutorial}
                title="Replay welcome tour"
                className="flex items-center gap-1.5 px-2.5 py-1 rounded-[4px] border border-[var(--masterd-border)] text-[9px] font-mono uppercase text-[var(--masterd-smoke)] hover:text-[#fca5a5] hover:border-[#b91c1c]/30 transition-all"
              >
                <BookOpen className="w-3.5 h-3.5" />
                <span className="hidden sm:inline">Guide</span>
              </button>
            </div>
          </header>

          {/* View workspace */}
          <div id="app-workspace" className="flex-1 bg-transparent flex flex-col overflow-y-auto p-6 min-w-0 relative z-10">
            
            <div className="flex items-center gap-2.5 text-[10px] text-[var(--masterd-smoke)] font-mono uppercase tracking-widest mb-4 border-b border-[var(--masterd-border)]/30 pb-2">
              <span>MASTERD</span>
              <ChevronRight className="w-3.5 h-3.5 opacity-50" />
              <span className="text-[#fca5a5] font-medium">{activeTab.replace(/_/g, " ")} workspace</span>
            </div>

            {!initLoaded ? (
              <div className="flex-grow flex flex-col justify-center items-center text-center">
                <Activity className="w-10 h-10 text-[#fca5a5] animate-spin mb-3" />
                <p className="font-mono text-xs">Initializing MASTERd engine...</p>
              </div>
            ) : (
              <div className="flex-1">
                {activeTab === "dashboard" && (
                  <Dashboard status={status} health={health} documents={documents} reviewQueue={reviewQueue} setActiveTab={setActiveTab} setSelectedDocument={setSelectedDocument} />
                )}
                {activeTab === "intake" && (
                  <Intake intakeQueue={intakeQueue} watchFolders={activeWatchFolders} refreshState={() => refreshState()} />
                )}
                {activeTab === "documents" && (
                  <Documents bridge={bridge} documents={documents} selectedDocument={selectedDocument} setSelectedDocument={setSelectedDocument} refreshState={() => refreshState()} />
                )}
                {activeTab === "pipeline" && (
                  <Pipeline bridge={bridge} jobs={jobs} refreshState={() => refreshState()} />
                )}
                {activeTab === "review" && (
                  <ReviewQueue bridge={bridge} reviewQueue={reviewQueue} documents={documents} refreshState={() => refreshState()} setSelectedDocument={setSelectedDocument} setActiveTab={setActiveTab} />
                )}
                {activeTab === "rules" && (
                  <Rules bridge={bridge} rules={rules} documents={documents} refreshState={() => refreshState()} />
                )}
                {activeTab === "audit" && (
                  <AuditLog bridge={bridge} auditLog={auditLog} documents={documents} refreshState={() => refreshState()} />
                )}
                {activeTab === "settings" && (
                  <SettingsScreen refreshState={() => refreshState()} />
                )}
              </div>
            )}
          </div>

          {/* Status bar */}
          <footer id="app-statusbar" className="h-8 bg-[#09090b] border-t border-[var(--masterd-border)] px-6 flex items-center justify-between text-[10px] font-mono tracking-wide text-[var(--masterd-smoke)] shrink-0 z-10">
            <div className="flex items-center gap-4">
              <span className="flex items-center gap-1.5">
                <span className="w-2 h-2 rounded-full bg-[#b91c1c] animate-pulse inline-block" />
                PENDING: <span className="text-[#fca5a5] font-bold">{(status?.queues.pending ?? 0) + (status?.queues.processing ?? 0)}</span>
              </span>
              <span>|</span>
              <span className="hover:text-[#f4f4f5] cursor-pointer" onClick={() => setActiveTab("review")}>
                REVIEW: <span className={`${pendingReviewCount > 0 ? "text-amber-400 font-bold" : "text-[#6C8798]"}`}>{pendingReviewCount}</span>
              </span>
              <span>|</span>
              <span className="hidden sm:inline-block">
                WATCHERS: <span className="text-[#fca5a5]">{activeWatchFolders.filter((w: WatchFolder) => w.enabled).length} ACTIVE</span>
              </span>
            </div>

            <div className="flex items-center gap-4">
              <span className="hidden md:inline-block">
                LAST OP:{" "}
                <span className="text-[#fca5a5]">
                  {auditLog[0]?.action ? `${auditLog[0].action.toUpperCase()} · ${auditLog[0].summary}` : "No recent actions"}
                </span>
              </span>
              <span>|</span>
              <span>DATABASE: <span className="text-green-400 font-medium">{status?.database?.toUpperCase() || "CONNECTED"}</span></span>
            </div>
          </footer>
        </div>

        {/* Chat panel (slide in from right) */}
        {chatOpen && (
          <div className="flex-1 min-w-[320px] max-w-[480px] flex flex-col border-l border-[#183040] overflow-hidden">
            <ChatPanel bridge={bridge} />
          </div>
        )}

      </div>
      <WelcomeTour
        open={tutorialOpen}
        step={tutorialStep}
        totalSteps={4}
        onClose={closeTutorial}
        onNext={() => setTutorialStep((s) => Math.min(s + 1, 3))}
        onBack={() => setTutorialStep((s) => Math.max(s - 1, 0))}
      />
    </main>
    )
  );
}
