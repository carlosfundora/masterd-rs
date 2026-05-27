import React from "react";
import { motion } from "motion/react";
import { 
  Activity, Database, Cpu, HardDrive, AlertTriangle, CheckCircle2, Play, ArrowRight, Clock, ShieldCheck, Box
} from "lucide-react";
import { SystemStatus, SystemHealth, DocumentRecord, ReviewItem } from "../contracts/api";

type DashboardProps = {
  status: SystemStatus | null;
  health: SystemHealth | null;
  documents: DocumentRecord[];
  reviewQueue: ReviewItem[];
  setActiveTab: (tab: string) => void;
  setSelectedDocument: (doc: DocumentRecord | null) => void;
};

export default function Dashboard({ 
  status, 
  health, 
  documents, 
  reviewQueue, 
  setActiveTab,
  setSelectedDocument 
}: DashboardProps) {
  
  const pendingReviews = reviewQueue.filter(r => !r.resolved);

  return (
    <div id="dashboard-screen" className="space-y-6 text-[#f4f4f5]">
      {/* Overview stats cards */}
      <div id="stats-grid" className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
        <div id="stat-card-files" className="bg-[#111113] border border-[#27272a] p-4 rounded-[4px] relative overflow-hidden">
          <div className="absolute top-0 left-0 w-1 h-full bg-[#b91c1c]" />
          <div className="flex justify-between items-start">
            <div>
              <p className="text-xs uppercase tracking-[0.16em] text-[#71717a]">Files Indexed</p>
              <h3 className="text-3xl font-semibold tracking-tight text-[#f4f4f5] mt-1">
                {status?.storage.indexedFiles ?? 0}
              </h3>
            </div>
            <div className="p-2 bg-[#7f1d1d]/10 rounded-[4px] text-[#fca5a5]">
              <Database className="w-5 h-5" />
            </div>
          </div>
          <div className="mt-4 flex items-center text-xs text-[#a1a1aa]">
            <span className="text-green-400 font-mono mr-1.5">● Connected</span>
            <span>Local DB Storage</span>
          </div>
        </div>

        <div id="stat-card-pending" className="bg-[#111113] border border-[#27272a] p-4 rounded-[4px] relative overflow-hidden">
          <div className="absolute top-0 left-0 w-1 h-full bg-[#7f1d1d]" />
          <div className="flex justify-between items-start">
            <div>
              <p className="text-xs uppercase tracking-[0.16em] text-[#71717a]">Files Processing</p>
              <h3 className="text-3xl font-semibold tracking-tight text-[#fca5a5] mt-1 font-mono">
                {(status?.queues.pending ?? 0) + (status?.queues.processing ?? 0)}
              </h3>
            </div>
            <div className="p-2 bg-[#7f1d1d]/10 rounded-[4px] text-[#fca5a5]">
              <Activity className="w-5 h-5" />
            </div>
          </div>
          <div className="mt-4 flex items-center text-xs text-[#a1a1aa]">
            <span className="text-cyan-400 font-mono mr-1.5 animate-pulse">● {status?.queues.processing ?? 0} Active Workers</span>
            <span>In processing pipeline</span>
          </div>
        </div>

        <div id="stat-card-review" className="bg-[#111113] border border-[#27272a] p-4 rounded-[4px] relative overflow-hidden">
          <div className="absolute top-0 left-0 w-1 h-full bg-[#7f1d1d]" />
          <div className="flex justify-between items-start">
            <div>
              <p className="text-xs uppercase tracking-[0.16em] text-[#71717a]">Review Queue</p>
              <h3 className="text-3xl font-semibold tracking-tight text-[#fca5a5] mt-1 font-mono">
                {pendingReviews.length}
              </h3>
            </div>
            <div className="p-2 bg-[#7f1d1d]/10 rounded-[4px] text-[#fca5a5]">
              <AlertTriangle className="w-5 h-5" />
            </div>
          </div>
          <div className="mt-4 flex items-center text-xs text-[#a1a1aa]">
            <span className="text-amber-400 font-semibold mr-1.5">{pendingReviews.filter(r => r.severity === "critical").length} Critical</span>
            <span>Needs human resolve</span>
          </div>
        </div>

        <div id="stat-card-size" className="bg-[#111113] border border-[#27272a] p-4 rounded-[4px] relative overflow-hidden">
          <div className="absolute top-0 left-0 w-1 h-full bg-[#7f1d1d]" />
          <div className="flex justify-between items-start">
            <div>
              <p className="text-xs uppercase tracking-[0.16em] text-[#71717a]">Storage Saved</p>
              <h3 className="text-3xl font-semibold tracking-tight text-[#fca5a5] mt-1 font-mono">
                {(((status?.storage.savedBytes ?? 0) / 1024 / 1024)).toFixed(1)} MB
              </h3>
            </div>
            <div className="p-2 bg-[#7f1d1d]/10 rounded-[4px] text-[#fca5a5]">
              <CheckCircle2 className="w-5 h-5" />
            </div>
          </div>
          <div className="mt-4 flex items-center text-xs text-[#a1a1aa]">
            <span className="text-green-400 font-semibold mr-1">
              -{(((status?.storage.savedBytes ?? 0) / (status?.storage.totalBytes ?? 1)) * 100).toFixed(0)}%
            </span>
            <span>compression in duplicate pooling</span>
          </div>
        </div>
      </div>

      {/* Main Row: System state flowchart and resources health */}
      <div id="main-metrics-row" className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        <div id="metrics-flowchart-panel" className="bg-[#111113] border border-[#27272a] p-5 rounded-[4px] lg:col-span-2 space-y-4">
          <div className="flex justify-between items-center border-b border-[#27272a] pb-3">
            <h2 className="text-sm font-semibold text-[#f4f4f5] uppercase tracking-wider flex items-center gap-2">
              <Box className="w-4 h-4 text-[#fca5a5]" /> Document Pipeline
            </h2>
            <span className="text-xs font-mono text-[#fca5a5] bg-[#7f1d1d]/10 px-2 py-0.5 rounded-[2px] uppercase">
              Local-first automation
            </span>
          </div>

          <p className="text-xs text-[#a1a1aa]">
            The local document flow keeps intake, extraction, classification, and review synchronized.
          </p>

          {/* Sequential flowchart diagram */}
          <div id="pipeline-timeline-flow" className="pt-4 pb-2">
            <div className="grid grid-cols-2 sm:grid-cols-4 lg:grid-cols-7 gap-2">
              {[
                { stage: "Ingest", desc: "Watch / Import", color: "bg-[#7f1d1d]", border: 'border-[#7f1d1d]', active: true },
                { stage: "Hash", desc: "SHA-256 Check", color: "bg-[#7f1d1d]", border: 'border-[#7f1d1d]', active: true },
                { stage: "Dedupe", desc: "Index Match", color: "bg-[#fca5a5]", border: 'border-[#fca5a5]', active: true, pulse: true },
                { stage: "Extract", desc: "Parser Engine", color: "bg-[#27272a]", border: 'border-[#27272a]', active: false },
                { stage: "Classify", desc: "Routing Rules", color: "bg-[#27272a]", border: 'border-[#27272a]', active: false },
                { stage: "Rename", desc: "Naming Policy", color: "bg-[#27272a]", border: 'border-[#27272a]', active: false },
                { stage: "Storage", desc: "Local Archive", color: "bg-[#27272a]", border: 'border-[#27272a]', active: false },
              ].map((item, idx) => (
                <div key={idx} className="relative flex flex-col justify-between p-2.5 bg-[#05070A] border border-[#183040] rounded-[4px]">
                  <div>
                    <div className="flex justify-between items-center">
                      <span className="text-[10px] font-mono text-[#6C8798]">S-0{idx+1}</span>
                      <div className={`w-2 h-2 rounded-full ${item.color} ${item.pulse ? 'animate-ping' : ''}`} />
                    </div>
                    <div className="text-xs font-semibold text-[#E6F7FF] mt-1.5">{item.stage}</div>
                    <div className="text-[10px] text-[#A7C7D9] mt-0.5">{item.desc}</div>
                  </div>
                  {idx < 6 && (
                    <div className="hidden lg:block absolute top-1/2 -right-1.5 -translate-y-1/2 z-10 text-[#00E5FF]">
                      <ArrowRight className="w-3.5 h-3.5 opacity-50 text-[#b91c1c]" />
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>

          <div className="flex justify-between items-center text-xs text-[#6C8798] pt-2">
            <span className="flex items-center gap-1.5">
              <span className="w-2.5 h-2.5 rounded-full bg-[#7f1d1d] inline-block" /> Completed stage
            </span>
            <span className="flex items-center gap-1.5">
              <span className="w-2.5 h-2.5 rounded-full bg-[#fca5a5] inline-block animate-pulse" /> Actively running execution task
            </span>
            <span className="flex items-center gap-1.5">
              <span className="w-2.5 h-2.5 rounded-full bg-[#27272a] inline-block" /> Queued sequence code
            </span>
          </div>
        </div>

        {/* Resources container */}
        <div id="metrics-hardware-panel" className="bg-[#111113] border border-[#27272a] p-5 rounded-[4px] space-y-4">
          <div className="border-b border-[#27272a] pb-3">
            <h2 className="text-sm font-semibold text-[#f4f4f5] uppercase tracking-wider flex items-center gap-2">
              <Cpu className="w-4 h-4 text-[#fca5a5]" /> Hardware Health
            </h2>
          </div>

          {/* CPU use */}
          <div className="space-y-1.5">
            <div className="flex justify-between text-xs font-mono">
              <span className="text-[#a1a1aa]">Local CPU load</span>
              <span className="text-[#fca5a5] font-semibold">{health?.cpuUsage ?? 0}%</span>
            </div>
            <div className="h-2 bg-[#05070A] border border-[#183040] p-0.5 rounded-[4px] overflow-hidden">
              <motion.div 
                className="h-full bg-gradient-to-r from-[#7f1d1d] to-[#fca5a5] rounded-[2px]" 
                style={{ width: `${health?.cpuUsage ?? 15}%` }}
                animate={{ width: `${health?.cpuUsage ?? 15}%` }}
                transition={{ duration: 0.5 }}
              />
            </div>
          </div>

          {/* Memory limit */}
          <div className="space-y-1.5">
            <div className="flex justify-between text-xs font-mono">
              <span className="text-[#a1a1aa]">Memory utilization</span>
              <span className="text-[#fca5a5] font-semibold">{health?.memoryUsage ?? 0}%</span>
            </div>
            <div className="h-2 bg-[#05070A] border border-[#183040] p-0.5 rounded-[4px] overflow-hidden">
              <motion.div 
                className="h-full bg-[#7f1d1d] rounded-[2px]"
                style={{ width: `${health?.memoryUsage ?? 40}%` }}
                animate={{ width: `${health?.memoryUsage ?? 40}%` }}
                transition={{ duration: 0.5 }}
              />
            </div>
          </div>

          {/* Disk storage */}
          <div className="space-y-1.5">
            <div className="flex justify-between text-xs font-mono">
              <span className="text-[#a1a1aa]">Disk storage left</span>
              <span className="text-[#fca5a5] font-semibold">
                {health ? (health.diskFreeBytes / 1024 / 1024 / 1024).toFixed(1) : 148} GB Free
              </span>
            </div>
            <div className="h-2 bg-[#05070A] border border-[#183040] p-0.5 rounded-[4px] overflow-hidden">
              <div className="h-full bg-[#22C55E] rounded-[2px] w-[78%]" />
            </div>
          </div>

          <div className="bg-[#09090b] p-2.5 rounded-[4px] border border-[#27272a] mt-4">
            <div className="flex items-center gap-2 text-xs">
              <Lock className="w-3.5 h-3.5 text-[#fca5a5]" />
              <span className="text-[#a1a1aa] font-mono">Database Latency:</span>
              <span className="text-[#f4f4f5] font-bold font-mono ml-auto">{health?.dbLatencyMs ?? 4} ms</span>
            </div>
          </div>
        </div>
      </div>

      {/* Grid: Active models index and Urgent Tasks review queue shortcut */}
      <div id="lower-dash-grid" className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        
        {/* Active models */}
        <div id="active-models-terminal" className="bg-[#0B1018] border border-[#183040] p-5 rounded-[4px] space-y-4">
          <div className="border-b border-[#183040] pb-3 flex justify-between items-center">
            <h2 className="text-sm font-semibold text-[#E6F7FF] uppercase tracking-wider flex items-center gap-2">
              <ShieldCheck className="w-4 h-4 text-[#00E5FF]" /> Active Local Models
            </h2>
          </div>

          <div className="space-y-2 max-h-[240px] overflow-y-auto pr-1">
            {status?.models.map(model => (
              <div key={model.id} className="flex justify-between items-center p-2 bg-[#05070A] border border-[#183040] rounded-[4px] text-xs">
                <div className="space-y-0.5">
                  <div className="font-mono text-[#E6F7FF] font-medium">{model.name}</div>
                  <div className="text-[10px] uppercase text-[#6C8798] tracking-wider">{model.role}</div>
                </div>
                <div className="text-right">
                  <span className={`px-2 py-0.5 rounded-[2px] font-mono text-[10px] ${
                    model.status === "online" 
                      ? "bg-green-500/10 text-green-400" 
                      : (model.status === "loading" ? "bg-amber-500/10 text-amber-400 animate-pulse" : "bg-red-500/10 text-red-400")
                  }`}>
                    {model.status}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* Needs attention queue panel */}
        <div id="urgent-review-cards" className="bg-[#0B1018] border border-[#183040] p-5 rounded-[4px] lg:col-span-2 space-y-4">
          <div className="border-b border-[#183040] pb-3 flex justify-between items-center">
            <h2 className="text-sm font-semibold text-[#E6F7FF] uppercase tracking-wider flex items-center gap-2">
              <Clock className="w-4 h-4 text-[#F59E0B]" /> Critical Approval Backlog
            </h2>
            <button 
              onClick={() => setActiveTab("review")}
              className="text-xs text-[#00E5FF] hover:underline font-mono"
            >
              Goto Review Queue →
            </button>
          </div>

          {pendingReviews.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-10 bg-[#05070A] border border-dashed border-[#183040] rounded-[4px] text-center">
              <CheckCircle2 className="w-8 h-8 text-green-400 mb-2" />
              <p className="text-xs font-semibold text-[#E6F7FF]">Docket fully approved in high memory mode</p>
              <p className="text-[11px] text-[#A7C7D9] mt-1">No outstanding items require verification</p>
            </div>
          ) : (
            <div className="space-y-2 max-h-[240px] overflow-y-auto pr-1">
              {pendingReviews.map(review => {
                const doc = documents.find(d => d.id === review.documentId);
                return (
                  <div 
                    key={review.id} 
                    className="p-3 bg-[#05070A] border border-[#183040] hover:border-[#3B82F6] rounded-[4px] flex flex-col sm:flex-row justify-between items-start sm:items-center transition-colors gap-2"
                  >
                    <div className="space-y-1">
                      <div className="flex items-center gap-2">
                        <span className={`text-[9px] font-mono uppercase px-1.5 py-0.2 rounded-[2px] ${
                          review.severity === "critical" ? "bg-red-500/15 text-red-400" : "bg-amber-500/15 text-amber-400"
                        }`}>
                          {review.severity}
                        </span>
                        <span className="text-xs font-medium text-[#E6F7FF]">
                          {review.title}
                        </span>
                      </div>
                      <p className="text-[11px] text-[#A7C7D9] line-clamp-1 italic">
                        {review.explanation}
                      </p>
                    </div>

                    <div className="flex items-center gap-2 self-end sm:self-center">
                      <button 
                        onClick={() => {
                          if (doc) {
                            setSelectedDocument(doc);
                            setActiveTab("documents");
                          } else {
                            setActiveTab("review");
                          }
                        }}
                        className="px-2.5 py-1 text-[11px] text-[#00E5FF] border border-[#00E5FF] hover:bg-[#00E5FF]/10 font-mono rounded-[4px] inline-flex items-center gap-1.5"
                      >
                        Inspect
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

// Simple Lock icon to keep standard code compiling beautifully
function Lock(props: React.SVGProps<SVGSVGElement>) {
  return (
    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" {...props}>
      <rect width="18" height="11" x="3" y="11" rx="2" ry="2" />
      <path d="M7 11V7a5 5 0 0 1 10 0v4" />
    </svg>
  );
}
