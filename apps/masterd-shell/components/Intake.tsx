import React, { useState } from "react";
import { 
  Upload, FolderPlus, Trash2, RefreshCw, X, Play, FileText, CheckCircle2, 
  AlertOctagon, Check, MonitorPlay, AlertTriangle, PlayCircle
} from "lucide-react";
import { IntakeItem, WatchFolder } from "../contracts/api";
import { getBridge } from "../lib/tauri-bridge";

type IntakeProps = {
  intakeQueue: IntakeItem[];
  watchFolders: WatchFolder[];
  refreshState: () => void;
};

export default function Intake({
  intakeQueue,
  watchFolders,
  refreshState
}: IntakeProps) {
  // Add folder path input
  const [folderPathInput, setFolderPathInput] = useState("");
  const [selectedProfile, setSelectedProfile] = useState("Full Analysis");
  
  // Drag over state simulation
  const [isDragOver, setIsDragOver] = useState(false);

  // Ingest sample document files list
  const sampleDocuments = [
    { name: "tax_return_form_1040.pdf", size: "1.4 MB", ext: "pdf" },
    { name: "stripe_subscription_invoice_may.pdf", size: "340 KB", ext: "pdf" },
    { name: "walmart_grocery_receipt_paper.png", size: "2.1 MB", ext: "png" },
    { name: "academic_paper_gpt_attention.txt", size: "48 KB", ext: "txt" }
  ];

  const handleIngestFile = async (fileName: string) => {
    const virtualPath = `/Users/username/Desktop/Tax2025/${fileName}`;
    const bridge = await getBridge();
    const res = await bridge.intake.addFiles([virtualPath], selectedProfile);
    if (res.ok) {
      refreshState();
    }
  };

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragOver(true);
  };

  const handleDragLeave = () => {
    setIsDragOver(false);
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragOver(false);
    const randomSample = sampleDocuments[Math.floor(Math.random() * sampleDocuments.length)];
    handleIngestFile(randomSample.name);
  };

  const handleAddWatchFolder = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!folderPathInput.trim()) return;
    const bridge = await getBridge();
    const res = await bridge.intake.addWatchFolder(folderPathInput.trim(), selectedProfile);
    if (res.ok) {
      setFolderPathInput("");
      refreshState();
    }
  };

  const handleRemoveWatchFolder = async (id: string) => {
    const bridge = await getBridge();
    const res = await bridge.intake.removeWatchFolder(id);
    if (res.ok) {
      refreshState();
    }
  };

  const handleRetryItem = async (id: string) => {
    const bridge = await getBridge();
    const res = await bridge.intake.retryItem(id);
    if (res.ok) {
      refreshState();
    }
  };

  const handleCancelItem = async (id: string) => {
    const bridge = await getBridge();
    const res = await bridge.intake.cancelItem(id);
    if (res.ok) {
      refreshState();
    }
  };

  return (
    <div id="intake-screen" className="space-y-6 text-[#f4f4f5]">
      
      {/* Visual Workspace Row: Drop Zone + Watch Folder Settings */}
      <div id="intake-top-grid" className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        
        {/* Drop zone segment */}
        <div id="dropzone-and-samples" className="bg-[#111113] border border-[#27272a] p-5 rounded-[4px] lg:col-span-2 space-y-4">
          <div className="border-b border-[#27272a] pb-3">
            <h2 className="text-sm font-semibold uppercase tracking-wider text-[#E6F7FF] flex items-center gap-2">
              <Upload className="w-4 h-4 text-[#fca5a5]" /> Document Intake
            </h2>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            
            {/* Interactive Drag & Drop box */}
            <div 
              onDragOver={handleDragOver}
              onDragLeave={handleDragLeave}
              onDrop={handleDrop}
              className={`md:col-span-2 aspect-video flex flex-col justify-center items-center p-6 border border-dashed rounded-[4px] cursor-pointer transition-all ${
                isDragOver 
                  ? "border-[#b91c1c] bg-[#7f1d1d]/5 scale-[0.99]" 
                  : "border-[#27272a] bg-[#09090b]/80 hover:bg-[#18181b]/70 hover:border-[#b91c1c]/70"
              }`}
              onClick={() => handleIngestFile(sampleDocuments[0].name)}
            >
              <div className="text-center space-y-2">
                  <Upload className={`w-8 h-8 mx-auto transition-transform ${isDragOver ? 'scale-110 text-[#fca5a5] animate-bounce' : 'text-[#71717a]'}`} />
                <div className="space-y-1">
                  <p className="text-xs font-semibold text-[#E6F7FF]">
                      Drag & drop records here or <span className="text-[#fca5a5] underline">click to select</span>
                  </p>
                    <p className="text-[10px] text-[#a1a1aa] max-w-xs mx-auto">
                      Supports PDF, JPG, PNG, and TXT files with local hashing and extraction.
                  </p>
                </div>
              </div>
            </div>

            {/* Sample files */}
            <div className="bg-[#09090b] border border-[#27272a]/70 p-3.5 rounded-[4px] space-y-3">
              <div>
                  <h4 className="text-[10px] uppercase font-mono tracking-wider font-bold text-[#71717a]">Starter samples</h4>
                  <p className="text-[10px] text-[#a1a1aa] mt-0.5 leading-tight">Use these examples to explore ingestion behavior.</p>
              </div>

              <div className="space-y-1.5 max-h-[140px] overflow-y-auto pr-1">
                {sampleDocuments.map((s, idx) => (
                  <button
                    key={idx}
                    onClick={() => handleIngestFile(s.name)}
                    className="w-full text-left p-1.5 bg-[#111113] hover:bg-[#7f1d1d]/5 hover:border-[#b91c1c] border border-[#27272a] rounded-[2px] transition-colors flex items-center gap-2 text-[11px] font-mono"
                  >
                    <PlayCircle className="w-3.5 h-3.5 text-[#fca5a5]" />
                    <div className="truncate flex-1">
                      <div className="truncate text-[#f4f4f5]">{s.name}</div>
                      <div className="text-[9px] text-[#71717a]">{s.size}</div>
                    </div>
                  </button>
                ))}
              </div>
            </div>

          </div>

          {/* Import profiles select configuration link */}
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2.5 pt-2 border-t border-[#183040]/60">
            <div className="flex items-center gap-1.5 text-xs text-[#a1a1aa]">
              <span className="font-semibold text-xs">Active Import Profile:</span>
              <select
                value={selectedProfile}
                onChange={e => setSelectedProfile(e.target.value)}
                className="bg-[#09090b] border border-[#27272a] text-xs font-mono text-[#fca5a5] rounded-[2px] p-1 focus:outline-none"
              >
                <option value="Fast Scan">Fast Scan (Metadata + SHA Check)</option>
                <option value="Full Analysis">Full Analysis (OCR + MASTERd Classifier + Tagging)</option>
                <option value="Legal Document Intake">Legal Document Intake Profile</option>
                <option value="Photo Archive Intake">Photo Archive Intake Profile</option>
                <option value="Receipts & Financial Docs">Receipts & Financial Docs Profile</option>
                <option value="Research Archive">Academic Research Archive Profile</option>
              </select>
            </div>
            <span className="font-mono text-[10px] text-[#fca5a5] bg-[#7f1d1d]/10 px-1.5 py-0.5 border border-[#7f1d1d]/20 rounded-[2px] flex items-center gap-1">
              <Check className="w-3 h-3" /> Local engine ready
            </span>
          </div>

        </div>

        {/* Watch Folders Settings */}
        <div id="watch-folders-panel" className="bg-[#111113] border border-[#27272a] p-5 rounded-[4px] space-y-4">
          <div className="border-b border-[#27272a] pb-3">
            <h2 className="text-sm font-semibold uppercase tracking-wider text-[#E6F7FF] flex items-center gap-2">
              <MonitorPlay className="w-4 h-4 text-[#fca5a5]" /> Watch folders
            </h2>
          </div>

          <p className="text-xs text-[#a1a1aa]">
            Files placed inside watch folders are scanned into the queue using the selected import profile.
          </p>

          <form onSubmit={handleAddWatchFolder} className="flex gap-1.5">
            <input
              type="text"
              placeholder="E.g., /Users/user/Downloads"
              value={folderPathInput}
              onChange={e => setFolderPathInput(e.target.value)}
              className="flex-1 bg-[#09090b] border border-[#27272a] px-2.5 py-1.5 text-xs font-mono rounded-[4px] text-[#f4f4f5] focus:outline-none focus:border-[#b91c1c] placeholder-[#71717a]"
            />
            <button
              type="submit"
              className="px-3 bg-[#b91c1c] hover:bg-[#991b1b] border border-[#b91c1c] text-white rounded-[4px] flex items-center justify-center p-1 cursor-pointer"
              title="Add Watch Folder"
            >
              <FolderPlus className="w-4 h-4" />
            </button>
          </form>

          {/* Active watchlist list */}
          <div className="space-y-2 max-h-[160px] overflow-y-auto pr-1">
            {watchFolders.map(wf => (
              <div 
                key={wf.id}
                className="p-2 bg-[#09090b] border border-[#27272a] rounded-[4px] flex justify-between items-center text-xs group"
              >
                <div className="space-y-0.5 min-w-0 flex-1 pr-2">
                  <div className="font-mono text-[#f4f4f5] truncate text-[11px]" title={wf.path}>
                    {wf.path}
                  </div>
                  <div className="text-[9px] text-[#a1a1aa] flex items-center gap-1 font-mono">
                    <span>{wf.profileId}</span>
                    <span>•</span>
                    <span>{wf.fileCount} files scanned</span>
                  </div>
                </div>
                <button
                  onClick={() => handleRemoveWatchFolder(wf.id)}
                  className="p-1 text-[#71717a] hover:text-[#fca5a5] hover:bg-[#7f1d1d]/5 rounded-[4px] shrink-0"
                  title="Remove Watcher Folder pointer"
                >
                  <Trash2 className="w-3.5 h-3.5" />
                </button>
              </div>
            ))}
            {watchFolders.length === 0 && (
              <div className="text-center py-4 text-[#71717a] italic font-mono text-[11px]">
                0 directories monitored.
              </div>
            )}
          </div>
        </div>

      </div>

      {/* Queue items Table segment */}
      <div id="intake-queue-table-block" className="bg-[#111113] border border-[#27272a] p-5 rounded-[4px] space-y-4">
        
        <div className="flex justify-between items-center border-b border-[#27272a] pb-3">
          <h2 className="text-sm font-semibold uppercase tracking-wider text-[#E6F7FF] flex items-center gap-2">
            <RefreshCw className="w-4 h-4 text-[#fca5a5] animate-spin-slow" /> Ingestion queue
          </h2>
          <span className="font-mono text-xs text-[#fca5a5] bg-[#7f1d1d]/10 px-2.5 py-0.5 rounded-[2px]">
            {intakeQueue.filter(i => i.status !== "complete" && i.status !== "error").length} In-Flight Jobs
          </span>
        </div>

        <div className="overflow-x-auto">
          <table className="w-full text-left border-collapse">
            <thead>
              <tr className="border-b border-[#183040] bg-[#05070A] text-[10px] uppercase font-mono tracking-wider text-[#6C8798]">
                <th className="py-2.5 px-4">Filename / Virtual System Path</th>
                <th className="py-2.5 px-4">Size</th>
                <th className="py-2.5 px-4 font-mono">Progress status</th>
                <th className="py-2.5 px-4">Dupe State</th>
                <th className="py-2.5 px-4 text-right">Job Management</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-[#183040]/40 text-xs font-mono">
              {intakeQueue.map(item => (
                <tr key={item.id} className="hover:bg-[#101827]/40 bg-[#0B1018] transition-colors">
                  <td className="py-3 px-4 space-y-1">
                    <div className="flex items-center gap-2 text-[#E6F7FF]">
                      <FileText className="w-3.5 h-3.5 text-[#fca5a5] shrink-0" />
                      <span className="font-semibold truncate max-w-xs">{item.fileName}</span>
                    </div>
                    <div className="text-[10px] text-[#6C8798] truncate max-w-md break-all" title={item.path}>
                      {item.path}
                    </div>
                  </td>
                  <td className="py-3 px-4 text-[#A7C7D9] text-[11px]">
                    {(item.sizeBytes / 1024 / 1024).toFixed(2)} MB
                  </td>
                  <td className="py-3 px-4 space-y-1.5 w-64">
                    <div className="flex justify-between items-center text-[10px]">
                      <span className={`px-1.5 py-0.2 rounded-[1.5px] font-bold text-[9px] uppercase ${
                        item.status === "complete" ? "bg-green-500/10 text-green-400" :
                        item.status === "error" ? "bg-red-500/10 text-red-400" :
                        "bg-[#7f1d1d]/10 text-[#fca5a5]"
                      }`}>
                        {item.status}
                      </span>
                      <span className="text-[#A7C7D9] font-mono">{item.progress}%</span>
                    </div>
                    
                    {/* Progress bar scale */}
                    <div className="h-1 bg-[#05070A] border border-[#183040] p-0.2 rounded-[2px] overflow-hidden">
                      <div 
                        className={`h-full rounded-[1px] transition-all duration-300 ${
                          item.status === "complete" ? "bg-green-400" :
                          item.status === "error" ? "bg-red-400" :
                          "bg-[#fca5a5]"
                        }`}
                        style={{ width: `${item.progress}%` }}
                      />
                    </div>
                  </td>
                  <td className="py-3 px-4">
                    <span className={`px-1.5 py-0.2 text-[10px] rounded-[1.5px] font-medium border ${
                      item.duplicateStatus === "unique" ? "bg-green-500/15 text-green-400 border-green-500/20" :
                      item.duplicateStatus === "possible_duplicate" ? "bg-amber-500/15 text-amber-400 border-amber-500/20" :
                      item.duplicateStatus === "exact_duplicate" ? "bg-red-500/15 text-red-400 border-red-500/20" :
                      "bg-gray-500/10 text-gray-400 border-gray-500/25"
                    }`}>
                      {item.duplicateStatus || "unknown"}
                    </span>
                  </td>
                  <td className="py-3 px-4 text-right">
                    {item.status === "error" ? (
                      <button
                        onClick={() => handleRetryItem(item.id)}
                        className="text-[10px] text-green-400 border border-green-500/20 bg-green-500/5 hover:bg-green-500/15 px-2.5 py-1 rounded-[4px] inline-flex items-center gap-1 font-mono"
                      >
                        <RefreshCw className="w-3 h-3" /> Retry Parse
                      </button>
                    ) : item.status !== "complete" ? (
                      <button
                        onClick={() => handleCancelItem(item.id)}
                        className="text-[10px] text-red-400 border border-red-500/20 bg-red-500/5 hover:bg-red-500/15 px-2 py-1 rounded-[4px] inline-flex items-center gap-1 font-mono"
                      >
                        <X className="w-3 h-3" /> Abort Job
                      </button>
                    ) : (
                      <span className="text-green-400 text-[11px] font-mono inline-flex items-center gap-1">
                        <Check className="w-3.5 h-3.5 stroke-2" /> Synced
                      </span>
                    )}
                  </td>
                </tr>
              ))}
              {intakeQueue.length === 0 && (
                <tr>
                  <td colSpan={5} className="py-10 text-center text-[#6C8798] bg-[#05070A]/20">
                    Active ingestion queues are completely clear
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>

      </div>

    </div>
  );
}
