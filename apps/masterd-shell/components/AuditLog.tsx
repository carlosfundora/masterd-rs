import React, { useState, useMemo } from "react";
import { 
  History, RotateCcw, ShieldCheck, Check, AlertTriangle, User, Bot, 
  Settings, ArrowRight, CornerDownRight, HelpCircle, FileText, Search
} from "lucide-react";
import { AuditEntry, DocumentRecord, MasterdFrontendBridge } from "../contracts/api";

type AuditLogProps = {
  bridge: MasterdFrontendBridge | null;
  auditLog: AuditEntry[];
  documents: DocumentRecord[];
  refreshState: () => void;
};

export default function AuditLog({
  bridge,
  auditLog,
  documents,
  refreshState
}: AuditLogProps) {
  const [searchTerm, setSearchTerm] = useState("");
  const [actorFilter, setActorFilter] = useState("");

  const handleRevert = async (entryId: string) => {
    if (!bridge) return;
    const res = await bridge.audit.revert(entryId);
    if (res.ok) {
      refreshState();
    } else {
      alert(res.error.message);
    }
  };

  const filteredAudits = useMemo(() => {
    const searchLower = searchTerm.toLowerCase();
    return auditLog.filter(entry => {
      const matchesSearch = searchTerm === "" ||
        entry.summary.toLowerCase().includes(searchLower) ||
        entry.action.toLowerCase().includes(searchLower) ||
        (entry.documentId && entry.documentId.toLowerCase().includes(searchLower));

      const matchesActor = actorFilter === "" || entry.actor === actorFilter;

      return matchesSearch && matchesActor;
    });
  }, [auditLog, searchTerm, actorFilter]);

  const getActorIcon = (actor: string) => {
    switch (actor) {
      case "system":
        return <Bot className="w-4 h-4 text-[#fca5a5]" />;
      case "user":
        return <User className="w-4 h-4 text-[#fca5a5]" />;
      default:
        return <Settings className="w-4 h-4 text-amber-500" />;
    }
  };

  return (
    <div id="audit-log-screen" className="space-y-6 text-[#E6F7FF]">
      
      {/* Top filter toolbar panel */}
      <div id="audit-filter-toolbar" className="bg-[#0B1018] border border-[#183040] p-4 rounded-[4px] flex flex-col md:flex-row gap-3">
        <div className="relative flex-1">
          <span className="absolute inset-y-0 left-0 flex items-center pl-3 text-[#6C8798]">
            <Search className="w-4 h-4" />
          </span>
          <input
            type="text"
            placeholder="Search matching ledger summaries, entry IDs, action terms..."
            value={searchTerm}
            onChange={e => setSearchTerm(e.target.value)}
            className="w-full bg-[#09090b] border border-[#27272a] pl-9 pr-4 py-1.5 rounded-[4px] text-xs font-mono text-[#f4f4f5] focus:outline-none focus:border-[#b91c1c] placeholder-[#71717a]"
          />
        </div>

        <div className="flex gap-2">
          <select
            value={actorFilter}
            onChange={e => setActorFilter(e.target.value)}
            className="bg-[#09090b] border border-[#27272a] text-xs px-2.5 py-1.5 rounded-[4px] font-mono text-[#f4f4f5] focus:outline-none focus:border-[#b91c1c]"
          >
            <option value="">-- Actor Classification --</option>
            <option value="system">Engine Core (AI / Rules)</option>
            <option value="user">Human Supervisor (User)</option>
          </select>
        </div>
      </div>

      {/* Audit ledgers main container */}
      <div id="audit-table-segment" className="bg-[#0B1018] border border-[#183040] rounded-[4px] p-5 space-y-4">
        
        <div className="border-b border-[#183040] pb-3 flex justify-between items-center">
          <h2 className="text-sm font-semibold uppercase tracking-wider text-[#E6F7FF] flex items-center gap-2">
            <History className="w-4 h-4 text-[#fca5a5]" /> Ledger Transaction Log Registry
          </h2>
          <span className="font-mono text-xs text-[#fca5a5] bg-[#7f1d1d]/10 px-2.5 py-0.5 rounded-[2px] uppercase">
            Append-only secure system audit
          </span>
        </div>

        {/* Audit timeline table */}
        <div className="overflow-x-auto">
          <table className="w-full text-left border-collapse">
            <thead>
              <tr className="border-b border-[#183040] bg-[#05070A] text-[10px] uppercase font-mono tracking-wider text-[#6C8798]">
                <th className="py-2.5 px-4 w-40">Timestamp (Local)</th>
                <th className="py-2.5 px-4 w-28">Entity ID</th>
                <th className="py-2.5 px-4 w-32">Ledger Action</th>
                <th className="py-2.5 px-4 w-32">Actor Flag</th>
                <th className="py-2.5 px-4">Event Description / Summary</th>
                <th className="py-2.5 px-4 text-right">Reverse state</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-[#183040]/40 text-xs font-mono text-[#E4E4E7]">
              {filteredAudits.map(entry => {
                const doc = documents.find(d => d.id === entry.documentId);
                return (
                  <tr key={entry.id} className="hover:bg-[#101827]/40 transition-colors">
                    <td className="py-3.5 px-4 text-[#6C8798] text-[11px] whitespace-nowrap">
                      {new Date(entry.createdAt).toLocaleString()}
                    </td>
                    <td className="py-3.5 px-4 text-[#fca5a5] font-bold font-mono text-[11px]">
                      {entry.documentId || <span className="text-[#3E5360]">-</span>}
                    </td>
                    <td className="py-3.5 px-4">
                      <span className={`px-1.5 py-0.2 text-[9px] font-bold rounded-[1.5px] uppercase ${
                        entry.action === "error" ? "bg-red-500/10 text-red-400" :
                        entry.action === "renamed" ? "bg-[#7f1d1d]/10 text-[#fca5a5]" :
                        entry.action === "reverted" ? "bg-[#7f1d1d]/10 text-[#fca5a5]" :
                        "bg-green-500/10 text-green-400"
                      }`}>
                        {entry.action}
                      </span>
                    </td>
                    <td className="py-3.5 px-4">
                      <div className="flex items-center gap-1.5">
                        {getActorIcon(entry.actor)}
                        <span className="text-[#A7C7D9] text-[11px] uppercase">{entry.actor}</span>
                      </div>
                    </td>
                    <td className="py-3.5 px-4 font-sans text-xs text-[#E6F7FF] leading-relaxed max-w-sm sm:max-w-md">
                      <div>{entry.summary}</div>
                      
                      {/* Before/after metadata display if present */}
                      {entry.before && (
                        <div className="mt-1.5 bg-black/30 border border-[#183040]/50 p-2 rounded-[4px] font-mono text-[10px] space-y-1 text-[#A7C7D9]">
                          <div className="text-red-400 text-[9px] uppercase font-bold">PREVIOUS STRUCT DATA:</div>
                          <div className="truncate">{JSON.stringify(entry.before)}</div>
                          <div className="text-green-400 text-[9px] uppercase font-bold mt-1">MUTATED STRUCT DATA:</div>
                          <div className="truncate">{JSON.stringify(entry.after)}</div>
                        </div>
                      )}
                    </td>
                    <td className="py-3.5 px-4 text-right">
                      {entry.reversible ? (
                        <button
                          onClick={() => handleRevert(entry.id)}
                          className="px-2.5 py-1 text-[11px] font-mono border border-[#7f1d1d] bg-[#7f1d1d]/5 hover:bg-[#7f1d1d]/15 text-[#fca5a5] rounded-[4px] inline-flex items-center gap-1"
                          title="Restore parameters before change"
                        >
                          <RotateCcw className="w-3.5 h-3.5" /> Undo (Revert)
                        </button>
                      ) : (
                        <span className="text-[#3E5360] text-[10px] font-semibold italic flex items-center gap-1 justify-end">
                          <ShieldCheck className="w-3.5 h-3.5 text-green-500/40" /> Locked (Permanent)
                        </span>
                      )}
                    </td>
                  </tr>
                );
              })}
              {filteredAudits.length === 0 && (
                <tr>
                  <td colSpan={6} className="py-10 text-center text-[#6C8798] bg-[#05070A]/20 font-mono text-xs">
                    0 system audit indexes resolved
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
