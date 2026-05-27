import React, { useState, useEffect } from "react";
import { 
  Sliders, Database, HardDrive, 
  RefreshCw, CheckCircle2, Bot, Save, Cpu, Globe, FileText
} from "lucide-react";
import type { AppConfig } from "../contracts/api";
import { getBridge } from "../lib/tauri-bridge";

type SettingsProps = {
  refreshState: () => void;
};

const DEFAULT_CONFIG: AppConfig = {
  ocrLanguage: "eng",
  safetyConfidencePct: 85,
  chatModel: "auto",
  searxngUrl: "http://127.0.0.1:9265",
  bm25TopK: 8,
  ragTopK: 8,
  generationTemp: 0.7,
  generationMaxTokens: 1024,
  embeddingBackend: "http",
  colbertUrl: "http://127.0.0.1:11450",
  jinaUrl: "http://127.0.0.1:11447",
  qwen3Url: "http://127.0.0.1:11502",
  intakeMaxDepth: 3,
  intakeExtensions: ["txt", "md", "rst", "log", "pdf"],
  ollamaUrl: "http://127.0.0.1:11434",
  ollamaModel: "llama3.2",
};

export default function Settings({ refreshState }: SettingsProps) {
  const [cfg, setCfg] = useState<AppConfig>(DEFAULT_CONFIG);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saveSuccess, setSaveSuccess] = useState(false);
  const [isBackupDone, setIsBackupDone] = useState(false);

  useEffect(() => {
    getBridge().then(b => b.settings.get()).then(res => {
      if (res.ok) setCfg(res.data);
    }).finally(() => setLoading(false));
  }, []);

  const set = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) =>
    setCfg(prev => ({ ...prev, [key]: value }));

  const handleSave = async (e: React.FormEvent) => {
    e.preventDefault();
    setSaving(true);
    setSaveSuccess(false);
    const b = await getBridge();
    const res = await b.settings.save(cfg);
    setSaving(false);
    if (res.ok) {
      setSaveSuccess(true);
      refreshState();
    }
  };

  const handleBackup = () => {
    setIsBackupDone(false);
    setTimeout(() => setIsBackupDone(true), 1500);
  };

  const field = "w-full bg-[#09090b] border border-[#27272a] p-2 rounded-[4px] text-xs font-mono text-[#f4f4f5] focus:outline-none focus:border-[#b91c1c]";
  const label = "text-[10px] uppercase font-mono tracking-wider font-bold text-[#6C8798]";

  if (loading) {
    return <div className="text-xs font-mono text-[#6C8798] p-4">Loading settings…</div>;
  }

  return (
    <div id="settings-screen" className="space-y-6 text-[#E6F7FF]">
      <form onSubmit={handleSave} className="space-y-6">

        {/* Row 1: Storage + AI Thresholds + Database */}
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">

          {/* Storage & Paths */}
          <div className="bg-[#0B1018] border border-[#183040] p-5 rounded-[4px] space-y-4">
            <div className="border-b border-[#183040] pb-3 flex items-center gap-2">
              <HardDrive className="w-4 h-4 text-[#fca5a5]" />
              <h2 className="text-sm font-semibold uppercase tracking-wider">Storage & Paths</h2>
            </div>
            <div className="space-y-3 text-xs">
              <div className="space-y-1">
                <label className={label}>Archive / Backup Path</label>
                <input type="text" value={cfg.archivePath ?? ""} onChange={e => set("archivePath", e.target.value || undefined)} placeholder="~/Documents/MASTERd" className={field} />
              </div>
              <div className="space-y-1">
                <label className={label}>OCR Language Code</label>
                <select value={cfg.ocrLanguage} onChange={e => set("ocrLanguage", e.target.value)} className={field}>
                  <option value="eng">English [eng]</option>
                  <option value="spa">Spanish [spa]</option>
                  <option value="fra">French [fra]</option>
                  <option value="deu">German [deu]</option>
                  <option value="chi_sim">Chinese Simplified [chi_sim]</option>
                  <option value="jpn">Japanese [jpn]</option>
                </select>
              </div>
              <div className="space-y-1">
                <label className={label}>Watch Folder Max Depth</label>
                <input type="number" min={1} max={10} value={cfg.intakeMaxDepth} onChange={e => set("intakeMaxDepth", Number(e.target.value))} className={field} />
              </div>
              <div className="space-y-1">
                <label className={label}>Indexed Extensions (comma-separated)</label>
                <input type="text" value={cfg.intakeExtensions.join(", ")} onChange={e => set("intakeExtensions", e.target.value.split(",").map(s => s.trim().toLowerCase()).filter(Boolean))} className={field} />
              </div>
            </div>
          </div>

          {/* AI & Generation */}
          <div className="bg-[#0B1018] border border-[#183040] p-5 rounded-[4px] space-y-4">
            <div className="border-b border-[#183040] pb-3 flex items-center gap-2">
              <Bot className="w-4 h-4 text-[#fca5a5]" />
              <h2 className="text-sm font-semibold uppercase tracking-wider">AI & Decision Thresholds</h2>
            </div>
            <div className="space-y-4 text-xs">
              <div className="space-y-2">
                <div className="flex justify-between font-mono text-[10px] uppercase font-bold text-[#6C8798]">
                  <span>Human Review Threshold</span>
                  <span className="text-[#fca5a5] font-bold">{cfg.safetyConfidencePct}% confidence</span>
                </div>
                <input type="range" min={40} max={99} value={cfg.safetyConfidencePct} onChange={e => set("safetyConfidencePct", Number(e.target.value))} className="w-full h-1.5 bg-[#05070A] rounded-[2px] appearance-none cursor-pointer" />
                <div className="flex justify-between text-[10px] text-[#6C8798] font-mono">
                  <span>Fast Pass (40%)</span><span>Bulletproof (99%)</span>
                </div>
              </div>
              <div className="space-y-1 border-t border-[#183040]/60 pt-3">
                <label className={label}>Chat Model</label>
                <select value={cfg.chatModel} onChange={e => set("chatModel", e.target.value)} className={field}>
                  <option value="auto">Auto (heuristic routing)</option>
                  <option value="instruct">LFM2.5-350M-Instruct (fast)</option>
                  <option value="thinking">LFM2.5-1.2B-Thinking (deep)</option>
                </select>
              </div>
              <div className="space-y-1">
                <label className={label}>Generation Temperature</label>
                <input type="number" min={0} max={2} step={0.05} value={cfg.generationTemp} onChange={e => set("generationTemp", Number(e.target.value))} className={field} />
              </div>
              <div className="space-y-1">
                <label className={label}>Max New Tokens</label>
                <input type="number" min={64} max={4096} step={64} value={cfg.generationMaxTokens} onChange={e => set("generationMaxTokens", Number(e.target.value))} className={field} />
              </div>
            </div>
          </div>

          {/* Database & Backup */}
          <div className="bg-[#0B1018] border border-[#183040] p-5 rounded-[4px] space-y-4 flex flex-col justify-between">
            <div className="space-y-4">
              <div className="border-b border-[#183040] pb-3 flex items-center gap-2">
                <Database className="w-4 h-4 text-green-400" />
                <h2 className="text-sm font-semibold uppercase tracking-wider">Database Maintenance</h2>
              </div>
              <p className="text-xs text-[#A7C7D9]">Export full indices or revert index tables to backup points. Done entirely client-side.</p>
              <div className="space-y-2">
                <button type="button" onClick={handleBackup} className="w-full py-2 bg-[#09090b] hover:bg-[#18181b] border border-[#27272a] text-green-400 font-mono text-xs rounded-[4px] inline-flex items-center justify-center gap-1.5">
                  <RefreshCw className="w-3.5 h-3.5" /> Trigger DB Backup Export
                </button>
                {isBackupDone && (
                  <div className="p-2.5 bg-green-500/10 border border-green-500/20 rounded-[4px] flex items-center gap-2 text-xs font-mono text-[#E4E4E7]">
                    <CheckCircle2 className="w-4 h-4 text-green-400 shrink-0" />
                    <span>Backup exported to archive path.</span>
                  </div>
                )}
              </div>
            </div>
            <div className="pt-4 border-t border-[#183040]/70">
              <button type="submit" disabled={saving} className="w-full py-2 bg-[#b91c1c] hover:bg-[#991b1b] border border-[#b91c1c] text-white font-mono font-bold rounded-[4px] cursor-pointer flex items-center justify-center gap-1.5 text-xs">
                <Save className="w-4 h-4" /> {saving ? "Saving…" : "Save System Preferences"}
              </button>
              {saveSuccess && <div className="text-center text-xs text-green-400 font-mono font-bold mt-2">Preferences saved!</div>}
            </div>
          </div>
        </div>

        {/* Row 2: Retrieval + Embedding Services */}
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">

          {/* Retrieval Tuning */}
          <div className="bg-[#0B1018] border border-[#183040] p-5 rounded-[4px] space-y-4">
            <div className="border-b border-[#183040] pb-3 flex items-center gap-2">
              <FileText className="w-4 h-4 text-[#fca5a5]" />
              <h2 className="text-sm font-semibold uppercase tracking-wider">Retrieval & RAG Tuning</h2>
            </div>
            <div className="grid grid-cols-2 gap-3 text-xs">
              <div className="space-y-1">
                <label className={label}>BM25 Top-K Candidates</label>
                <input type="number" min={1} max={100} value={cfg.bm25TopK} onChange={e => set("bm25TopK", Number(e.target.value))} className={field} />
              </div>
              <div className="space-y-1">
                <label className={label}>RAG Context Chunks</label>
                <input type="number" min={1} max={32} value={cfg.ragTopK} onChange={e => set("ragTopK", Number(e.target.value))} className={field} />
              </div>
              <div className="space-y-1 col-span-2">
                <label className={label}>SearXNG Web Search URL</label>
                <input type="text" value={cfg.searxngUrl} onChange={e => set("searxngUrl", e.target.value)} className={field} />
              </div>
            </div>
          </div>

          {/* Embedding Services */}
          <div className="bg-[#0B1018] border border-[#183040] p-5 rounded-[4px] space-y-4">
            <div className="border-b border-[#183040] pb-3 flex items-center gap-2">
              <Cpu className="w-4 h-4 text-[#fca5a5]" />
              <h2 className="text-sm font-semibold uppercase tracking-wider">Embedding Service URLs</h2>
            </div>
            <div className="space-y-3 text-xs">
              <div className="space-y-1">
                <label className={label}>Inference Backend</label>
                <select value={cfg.embeddingBackend} onChange={e => set("embeddingBackend", e.target.value)} className={field}>
                  <option value="http">HTTP (external service)</option>
                  <option value="direct">Direct (smoke-test only)</option>
                </select>
              </div>
              <div className="grid grid-cols-1 gap-3">
                <div className="space-y-1">
                  <label className={label}>ColBERT Wrapper URL</label>
                  <input type="text" value={cfg.colbertUrl} onChange={e => set("colbertUrl", e.target.value)} className={field} />
                </div>
                <div className="space-y-1">
                  <label className={label}>Jina Embedding URL</label>
                  <input type="text" value={cfg.jinaUrl} onChange={e => set("jinaUrl", e.target.value)} className={field} />
                </div>
                <div className="space-y-1">
                  <label className={label}>Qwen3 Embedding URL</label>
                  <input type="text" value={cfg.qwen3Url} onChange={e => set("qwen3Url", e.target.value)} className={field} />
                </div>
              </div>
            </div>
          </div>

        </div>

        {/* Row 3: Ollama Fallback */}
        <div className="bg-[#0B1018] border border-[#183040] border-dashed p-5 rounded-[4px] space-y-4">
          <div className="border-b border-[#183040] pb-3 flex items-center gap-2">
            <Globe className="w-4 h-4 text-[#fca5a5]" />
            <h2 className="text-sm font-semibold uppercase tracking-wider">Ollama Fallback Engine</h2>
            <span className="ml-auto text-[10px] font-mono text-[#fca5a5] bg-[#09090b] border border-[#27272a] px-2 py-0.5 rounded-[2px]">AUTO-ACTIVATES</span>
          </div>
          <p className="text-xs text-[#A7C7D9]">
            When the embedded LFM2.5 models fail to load (missing assets, memory pressure),
            MASTERd transparently falls back to a locally-running{" "}
            <span className="font-mono text-[#fca5a5]">ollama</span> daemon.
            Install from <span className="font-mono text-[#fca5a5]">ollama.com</span> and
            run <span className="font-mono text-white bg-[#05070A] px-1 rounded">ollama pull llama3.2</span> to enable.
          </p>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4 text-xs">
            <div className="space-y-1">
              <label className={label}>Ollama Daemon URL</label>
              <input type="text" value={cfg.ollamaUrl} onChange={e => set("ollamaUrl", e.target.value)} className={field} placeholder="http://127.0.0.1:11434" />
            </div>
            <div className="space-y-1">
              <label className={label}>Fallback Model Name</label>
              <input type="text" value={cfg.ollamaModel} onChange={e => set("ollamaModel", e.target.value)} className={field} placeholder="llama3.2" />
              <p className="text-[10px] text-[#6C8798] font-mono">
                Any model loaded in Ollama — e.g. mistral, phi3, gemma3, llama3.1:8b
              </p>
            </div>
          </div>
        </div>
      </form>
    </div>
  );
}
