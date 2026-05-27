import React, { useState } from "react";
import { 
  Terminal, Server, Activity, RefreshCw, X, Play, Clock, CheckCircle2, 
  AlertOctagon, Cpu, Workflow, List, Binary, Eye
} from "lucide-react";
import { PipelineJob, PipelineStage, PipelineLogEntry, MasterdFrontendBridge } from "../contracts/api";

type PipelineProps = {
  bridge: MasterdFrontendBridge | null;
  jobs: PipelineJob[];
  refreshState: () => void;
};

export default function Pipeline({
  bridge,
  jobs,
  refreshState
}: PipelineProps) {
  const [selectedJobId, setSelectedJobId] = useState<string | null>(jobs[0]?.id || null);

  const selectedJob = jobs.find(j => j.id === selectedJobId) || jobs[0];

  const handleRetryJob = async (id: string) => {
    if (!bridge) return;
    const res = await bridge.pipeline.retryJob(id);
    if (res.ok) {
      refreshState();
    }
  };

  const handleCancelJob = async (id: string) => {
    if (!bridge) return;
    const res = await bridge.pipeline.cancelJob(id);
    if (res.ok) {
      refreshState();
    }
  };

  // Define full pipeline sequential flowchart helper for visual reference
  const fullStages: Array<{ name: PipelineStage, label: string }> = [
    { name: "ingest", label: "Ingest" },
    { name: "normalize", label: "Normalize" },
    { name: "hash", label: "Hashing" },
    { name: "dedupe", label: "Deduplication" },
    { name: "extract_text", label: "Text Extraction" },
    { name: "ocr", label: "OCR Layer" },
    { name: "classify", label: "Classification" },
    { name: "extract_entities", label: "Named Entities" },
    { name: "suggest_tags", label: "Tag suggestions" },
    { name: "suggest_rename", label: "Rename evaluation" },
    { name: "route_storage", label: "Storage routing" },
    { name: "write_audit", label: "Write audit trail" },
    { name: "complete", label: "Success" }
  ];

  return (
    <div id="pipeline-screen" className="space-y-6 text-[#E6F7FF]">
      
      {/* Top Banner: Local Sandbox Workers & Health */}
      <div id="workers-grid-layout" className="grid grid-cols-1 md:grid-cols-3 gap-6">
        
        {/* Worker 1 Console Status */}
        <div className="bg-[#0B1018] border border-[#183040] p-4 rounded-[4px] space-y-3 relative overflow-hidden">
          <div className="absolute top-0 left-0 w-1 h-full bg-[#b91c1c]" />
          <div className="flex justify-between items-center text-xs">
            <span className="font-mono text-[#6C8798]">THREAD PROCESSOR 01</span>
            <span className="text-green-400 font-mono flex items-center gap-1.5 font-bold">
              <span className="w-2 h-2 rounded-full bg-green-400 animate-pulseinline-block" /> ONLINE
            </span>
          </div>

          <div className="flex items-center gap-3">
            <div className="p-2.5 bg-[#7f1d1d]/10 text-[#fca5a5] rounded-[4px]">
              <Cpu className="w-5 h-5" />
            </div>
            <div>
              <h4 className="text-xs font-semibold text-[#E6F7FF]">local-worker-1</h4>
              <p className="text-[10px] text-[#A7C7D9] font-mono">TASK: Deduplication Hash queries</p>
            </div>
          </div>

          <div className="bg-[#05070A] p-2 border border-[#183040] rounded-[4px] font-mono text-[9px] text-[#6C8798] space-y-1">
            <div className="truncate">➔ [09:12] loaded thread space hash indices</div>
            <div className="truncate">➔ [09:14] query SHA matches for doc-104</div>
          </div>
        </div>

        {/* Worker 2 Console Status */}
        <div className="bg-[#0B1018] border border-[#183040] p-4 rounded-[4px] space-y-3 relative overflow-hidden">
          <div className="absolute top-0 left-0 w-1 h-full bg-[#7f1d1d]" />
          <div className="flex justify-between items-center text-xs">
            <span className="font-mono text-[#6C8798]">THREAD PROCESSOR 02</span>
            <span className="text-green-400 font-mono flex items-center gap-1.5 font-bold">
              <span className="w-2 h-2 rounded-full bg-green-400 animate-pulse inline-block" /> STANDBY
            </span>
          </div>

          <div className="flex items-center gap-3">
            <div className="p-2.5 bg-[#7f1d1d]/10 text-[#fca5a5] rounded-[4px]">
              <Cpu className="w-5 h-5" />
            </div>
            <div>
              <h4 className="text-xs font-semibold text-[#E6F7FF]">local-worker-2</h4>
              <p className="text-[10px] text-[#A7C7D9] font-mono">TASK: OCR Pipeline Re-indexer</p>
            </div>
          </div>

          <div className="bg-[#05070A] p-2 border border-[#183040] rounded-[4px] font-mono text-[9px] text-[#6C8798] space-y-1">
            <div className="truncate">➔ [09:10] idle thread pool sleep state initiated</div>
            <div className="truncate">➔ [09:12] flush extraction outputs cache storage</div>
          </div>
        </div>

        {/* Sandbox worker orchestration info */}
        <div className="bg-[#0B1018] border border-[#183040] p-4 rounded-[4px] flex flex-col justify-between">
          <div>
            <h4 className="text-xs uppercase font-mono tracking-wider font-bold text-[#6C8798] flex items-center gap-1.5">
              <Workflow className="w-4 h-4 text-[#fca5a5]" /> Local Worker Sandbox
            </h4>
            <p className="text-[11px] text-[#A7C7D9] mt-2.5 leading-relaxed">
              MASTERd utilizes standard concurrent worker loops running isolated within browser sandbox sandboxes. No personal data ever exits your system.
            </p>
          </div>
          <div className="text-[10px] text-[#fca5a5] font-mono border-t border-[#183040] pt-2 mt-2">
            Active Core Threads: 2 / Memory Lock: Approved
          </div>
        </div>

      </div>

      {/* Main split row: active pipelines jobs list AND active terminal workspace log */}
      <div id="jobs-and-logs-workspace" className="grid grid-cols-1 lg:grid-cols-5 gap-6">
        
        {/* Active background jobs table */}
        <div className="bg-[#0B1018] border border-[#183040] p-5 rounded-[4px] lg:col-span-3 space-y-4 flex flex-col">
          <div className="border-b border-[#183040] pb-3 flex justify-between items-center">
            <h2 className="text-sm font-semibold uppercase tracking-wider text-[#E6F7FF] flex items-center gap-2">
              <List className="w-4 h-4 text-[#fca5a5]" /> Active Pipeline Worker Jobs
            </h2>
          </div>

          <div className="overflow-x-auto flex-1">
            <table className="w-full text-left border-collapse">
              <thead>
                <tr className="border-b border-[#183040] bg-[#05070A] text-[10px] uppercase font-mono tracking-wider text-[#6C8798]">
                  <th className="py-2.5 px-4">Filename Target</th>
                  <th className="py-2.5 px-4">Active Stage</th>
                  <th className="py-2.5 px-4">Job Status / Progress</th>
                  <th className="py-2.5 px-4 text-right">Action</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[#183040]/40 text-xs">
                {jobs.map(job => {
                  const isSelected = selectedJobId === job.id;
                  return (
                    <tr 
                      key={job.id} 
                      onClick={() => setSelectedJobId(job.id)}
                      className={`hover:bg-[#101827]/40 cursor-pointer transition-colors ${
                        isSelected ? "bg-[#7f1d1d]/10 border-l border-[#7f1d1d]" : ""
                      }`}
                    >
                      <td className="py-3 px-4">
                        <div className="text-[#E6F7FF] font-semibold truncate max-w-[140px] font-mono" title={job.fileName}>
                          {job.fileName}
                        </div>
                        <div className="text-[10px] text-[#6C8798] font-mono">ID: {job.id}</div>
                      </td>
                      <td className="py-3 px-4">
                        <span className="text-[10px] font-mono text-[#fca5a5] bg-[#7f1d1d]/5 border border-[#7f1d1d]/20 px-1.5 py-0.5 rounded-[2px] uppercase">
                          {job.stage}
                        </span>
                      </td>
                      <td className="py-3 px-4 space-y-1 w-44 font-mono">
                        <div className="flex justify-between text-[10px]">
                          <span className={`${
                            job.status === "complete" ? "text-green-400" :
                            job.status === "error" ? "text-red-400" :
                            "text-[#fca5a5]"
                          }`}>{job.status}</span>
                          <span>{job.progress}%</span>
                        </div>
                        <div className="h-1 bg-[#05070A] border border-[#183040] rounded-[2px] overflow-hidden p-0.2">
                          <div 
                            className={`h-full rounded-[1px] ${
                              job.status === "complete" ? "bg-green-400" :
                              job.status === "error" ? "bg-red-400" : "bg-[#fca5a5]"
                            }`}
                            style={{ width: `${job.progress}%` }}
                          />
                        </div>
                      </td>
                      <td className="py-3 px-4 text-right" onClick={e => e.stopPropagation()}>
                        {job.status === "running" ? (
                          <button
                            onClick={() => handleCancelJob(job.id)}
                            className="p-1 px-2 border border-red-500/20 text-red-400 bg-red-500/5 hover:bg-red-500/15 rounded-[4px] text-[10px] font-mono"
                          >
                            Halt
                          </button>
                        ) : job.status === "error" ? (
                          <button
                            onClick={() => handleRetryJob(job.id)}
                            className="p-1 px-2 border border-green-500/20 text-green-400 bg-green-500/5 hover:bg-green-500/15 rounded-[4px] text-[10px] font-mono"
                          >
                            Reset
                          </button>
                        ) : (
                          <button
                            onClick={() => setSelectedJobId(job.id)}
                            className="p-1 px-2 text-[#6C8798] hover:text-[#fca5a5] text-[10px] font-mono flex items-center gap-1 ml-auto"
                          >
                            <Terminal className="w-3.5 h-3.5" /> Log
                          </button>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </div>

        {/* Selected Job log terminal reader */}
        <div className="bg-[#0B1018] border border-[#183040] p-5 rounded-[4px] lg:col-span-2 flex flex-col h-[380px]">
          <div className="border-b border-[#183040] pb-3 flex justify-between items-center shrink-0">
            <h2 className="text-sm font-semibold uppercase tracking-wider text-[#E6F7FF] flex items-center gap-2">
              <Terminal className="w-4 h-4 text-green-400" /> Active Job Terminal Logger
            </h2>
            {selectedJob && (
              <span className="font-mono text-[9px] text-[#6C8798]">
                ID: {selectedJob.id}
              </span>
            )}
          </div>

          {selectedJob ? (
            <div className="flex-1 flex flex-col space-y-3 mt-4 overflow-hidden">
              {/* Job running status indicator dashboard */}
              <div className="p-2.5 bg-[#05070A] border border-[#183040] rounded-[4px] font-mono text-[10px] grid grid-cols-2 gap-2 text-[#A7C7D9]">
                <div>File Target: <span className="text-[#E6F7FF] font-semibold">{selectedJob.fileName}</span></div>
                <div>Worker Engine: <span className="text-[#E6F7FF]">{selectedJob.workerId || "unassigned"}</span></div>
                <div>Workflow Stage: <span className="text-[#fca5a5] uppercase font-bold">{selectedJob.stage}</span></div>
                <div>Operations Check: <span className={`${selectedJob.status === "complete" ? "text-green-400" : "text-[#fca5a5]"}`}>{selectedJob.status}</span></div>
              </div>

              {/* Terminal list logs */}
              <div className="flex-1 bg-black p-4 text-[10.5px] font-mono text-[#E4E4E7] overflow-y-auto rounded-[4px] border border-[#183040] leading-relaxed scrollbar-thin">
                <div className="text-green-400 uppercase font-bold text-[9px] border-b border-[#183040] pb-1 mb-1.5 flex justify-between">
                  <span>SANDBOX LOG CONSOLE WORK DIRECTORY</span>
                  <span>TIME UTC</span>
                </div>
                {selectedJob.logs.map((log: PipelineLogEntry) => (
                  <div key={log.id} className="space-x-1 flex items-start">
                    <span className="text-[#6C8798] shrink-0">[{new Date(log.createdAt).toLocaleTimeString()}]</span>
                    <span className={`shrink-0 ${
                      log.level === "error" ? "text-red-400" :
                      log.level === "warning" ? "text-amber-400" :
                      log.level === "debug" ? "text-[#fca5a5]" : "text-green-400"
                    }`}>
                      [{log.level.toUpperCase()}]
                    </span>
                    <span className="text-[#E6F7FF] break-all">{log.message}</span>
                  </div>
                ))}
                {selectedJob.status === "running" && (
                  <div className="text-[#fca5a5] font-bold animate-pulse text-[11px] mt-2">
                    █ Worker pipeline process active... awaiting events...
                  </div>
                )}
              </div>
            </div>
          ) : (
            <div className="flex-1 flex flex-col justify-center items-center py-10 text-center text-[#6C8798]">
              <Terminal className="w-10 h-10 opacity-30 mb-2 text-[#6C8798]" />
              <p className="font-mono text-xs">Verify active jobs to trace logs</p>
            </div>
          )}
        </div>

      </div>

    </div>
  );
}
