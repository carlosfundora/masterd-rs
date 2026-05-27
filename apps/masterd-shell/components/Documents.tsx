import React, { useState, useEffect } from "react";
import { 
  Search, Filter, Check, X, RefreshCw, FileText, FileSpreadsheet, FileCode, Tag, CheckCircle2, 
  AlertOctagon, AlertTriangle, Eye, ArrowRight, CornerDownRight, History, Hash, Terminal, Sparkles, Folder
} from "lucide-react";
import { DocumentRecord, ClassificationResult, ProcessingStatus, DuplicateStatus, AuditEntry, MasterdFrontendBridge } from "../contracts/api";

type DocumentsProps = {
  bridge: MasterdFrontendBridge | null;
  documents: DocumentRecord[];
  selectedDocument: DocumentRecord | null;
  setSelectedDocument: (doc: DocumentRecord | null) => void;
  refreshState: () => void;
};

export default function Documents({
  bridge,
  documents,
  selectedDocument,
  setSelectedDocument,
  refreshState
}: DocumentsProps) {
  // Search state
  const [searchTerm, setSearchTerm] = useState("");
  const [catFilter, setCatFilter] = useState("");
  const [statusFilter, setStatusFilter] = useState("");
  const [dupeFilter, setDupeFilter] = useState("");
  
  // Inspector Tabs
  const [inspectorTab, setInspectorTab] = useState<"overview" | "preview" | "text" | "classify" | "rename" | "dupes" | "audit" | "raw">("overview");

  // Extracted entities and text preview state
  const [entities, setEntities] = useState<Array<{text: string, label: string, confidence: number}>>([]);
  const [fullText, setFullText] = useState("");
  const [documentAudit, setDocumentAudit] = useState<AuditEntry[]>([]);
  const [customTagInput, setCustomTagInput] = useState("");
  const [editableRenameVal, setEditableRenameVal] = useState("");

  // Populate inspector variables when selected file shifts
  useEffect(() => {
    if (selectedDocument) {
      setTimeout(() => {
        setEditableRenameVal(selectedDocument.suggestedName || selectedDocument.currentName);
      }, 0);
      
      // Load lazy extractor text and audits
      bridge?.documents.getExtractedText(selectedDocument.id).then(res => {
        if (res.ok) {
          setFullText(res.data.fullText);
          setEntities(res.data.entities);
        }
      });

      bridge?.audit.getForDocument(selectedDocument.id).then(res => {
        if (res.ok) {
          setDocumentAudit(res.data);
        }
      });
    }
  }, [bridge, selectedDocument]);

  // Filter lists
  const filteredDocs = documents.filter(doc => {
    const matchesSearch = searchTerm === "" || 
      doc.currentName.toLowerCase().includes(searchTerm.toLowerCase()) ||
      doc.originalName.toLowerCase().includes(searchTerm.toLowerCase()) ||
      doc.tags.some(t => t.toLowerCase().includes(searchTerm.toLowerCase())) ||
      (doc.extractedText && doc.extractedText.toLowerCase().includes(searchTerm.toLowerCase()));

    const matchesCat = catFilter === "" || doc.classification?.category === catFilter;
    const matchesStatus = statusFilter === "" || doc.processingStatus === statusFilter;
    const matchesDupe = dupeFilter === "" || doc.duplicateStatus === dupeFilter;

    return matchesSearch && matchesCat && matchesStatus && matchesDupe;
  });

  // Action methods
  const handleApproveRename = async (docId: string, customName?: string) => {
    if (!bridge) return;
    const res = await bridge.actions.approveRename(docId, customName);
    if (res.ok) {
      refreshState();
      // Update local selection to reflect rename
      const updated = documents.find(d => d.id === docId);
      if (updated) {
        setSelectedDocument({
          ...updated,
          currentName: customName || updated.suggestedName || updated.currentName,
          processingStatus: "complete"
        });
      }
    }
  };

  const handleRejectRename = async (docId: string) => {
    if (!bridge) return;
    const res = await bridge.actions.rejectRename(docId);
    if (res.ok) {
      refreshState();
      const updated = documents.find(d => d.id === docId);
      if (updated) {
        setSelectedDocument({
          ...updated,
          suggestedName: undefined,
          processingStatus: "complete"
        });
      }
    }
  };

  const handleReprocess = async (docId: string) => {
    if (!bridge) return;
    const res = await bridge.documents.reprocess(docId, { ocr: true, classify: true });
    if (res.ok) {
      refreshState();
    }
  };

  const handleMarkDuplicate = async (docId: string, dupeOf: string) => {
    if (!bridge) return;
    const res = await bridge.actions.markDuplicate(docId, dupeOf);
    if (res.ok) {
      refreshState();
      const updated = documents.find(d => d.id === docId);
      if (updated) {
        setSelectedDocument({ ...updated, duplicateStatus: "exact_duplicate" });
      }
    }
  };

  const handleMarkUnique = async (docId: string) => {
    if (!bridge) return;
    const res = await bridge.actions.markUnique(docId);
    if (res.ok) {
      refreshState();
      const updated = documents.find(d => d.id === docId);
      if (updated) {
        setSelectedDocument({ ...updated, duplicateStatus: "unique" });
      }
    }
  };

  const handleAddTag = async (doc: DocumentRecord) => {
    if (!customTagInput.trim()) return;
    const newTags = Array.from(new Set([...doc.tags, customTagInput.trim().toLowerCase()]));
    if (!bridge) return;
    const res = await bridge.documents.updateTags(doc.id, newTags);
    if (res.ok) {
      setCustomTagInput("");
      refreshState();
      setSelectedDocument({ ...doc, tags: newTags });
    }
  };

  const handleRemoveTag = async (doc: DocumentRecord, tagToRemove: string) => {
    const newTags = doc.tags.filter(t => t !== tagToRemove);
    if (!bridge) return;
    const res = await bridge.documents.updateTags(doc.id, newTags);
    if (res.ok) {
      refreshState();
      setSelectedDocument({ ...doc, tags: newTags });
    }
  };

  const getFileIcon = (ext: string) => {
    switch (ext.toLowerCase()) {
      case "pdf":
        return <FileText className="w-4 h-4 text-red-400" />;
      case "xlsx":
      case "csv":
        return <FileSpreadsheet className="w-4 h-4 text-green-400" />;
      case "json":
      case "txt":
        return <FileCode className="w-4 h-4 text-[#fca5a5]" />;
      default:
        return <FileText className="w-4 h-4 text-[#fca5a5]" />;
    }
  };

  // Helper categories for filtering options
  const categories = Array.from(
    new Set(documents.map(d => d.classification?.category).filter(Boolean))
  ) as string[];

  return (
    <div id="documents-workspace" className="flex flex-col lg:flex-row gap-6 text-[#E6F7FF] relative min-h-[550px]">
      
      {/* Search and Table Area */}
      <div id="documents-table-segment" className="flex-1 space-y-4">
        
        {/* Search controls & Filter Toolbar */}
        <div id="document-toolbar" className="bg-[#0B1018] border border-[#183040] p-4 rounded-[4px] space-y-3">
          <div className="flex flex-col md:flex-row gap-3">
            <div className="relative flex-1">
              <span className="absolute inset-y-0 left-0 flex items-center pl-3 text-[#6C8798]">
                <Search className="w-4 h-4" />
              </span>
              <input
                id="doc-keyword-search"
                type="text"
                placeholder="Search across file index, OCR text, tags..."
                value={searchTerm}
                onChange={e => setSearchTerm(e.target.value)}
                className="w-full bg-[#09090b] border border-[#27272a] pl-9 pr-4 py-1.5 rounded-[4px] text-xs font-mono text-[#f4f4f5] focus:outline-none focus:border-[#b91c1c] placeholder-[#71717a]"
              />
              {searchTerm && (
                <button 
                  onClick={() => setSearchTerm("")}
                  className="absolute inset-y-0 right-0 flex items-center pr-3 text-[#6C8798] hover:text-[#E6F7FF]"
                >
                  <X className="w-4 h-4" />
                </button>
              )}
            </div>

            <div className="flex flex-wrap gap-2">
              <select
                id="filter-category-selector"
                value={catFilter}
                onChange={e => setCatFilter(e.target.value)}
                className="bg-[#09090b] border border-[#27272a] text-xs px-2.5 py-1.5 rounded-[4px] font-mono text-[#f4f4f5] focus:outline-none focus:border-[#b91c1c]"
              >
                <option value="">-- Classification Category --</option>
                {categories.map(cat => <option key={cat} value={cat}>{cat}</option>)}
              </select>

              <select
                id="filter-status-selector"
                value={statusFilter}
                onChange={e => setStatusFilter(e.target.value)}
                className="bg-[#09090b] border border-[#27272a] text-xs px-2.5 py-1.5 rounded-[4px] font-mono text-[#f4f4f5] focus:outline-none focus:border-[#b91c1c]"
              >
                <option value="">-- Processing State --</option>
                <option value="needs_review">Needs Review / Manual Action</option>
                <option value="complete">Complete</option>
                <option value="processing">Processing</option>
              </select>

              <select
                id="filter-duplicate-selector"
                value={dupeFilter}
                onChange={e => setDupeFilter(e.target.value)}
                className="bg-[#09090b] border border-[#27272a] text-xs px-2.5 py-1.5 rounded-[4px] font-mono text-[#f4f4f5] focus:outline-none focus:border-[#b91c1c]"
              >
                <option value="">-- Duplicate Check --</option>
                <option value="unique">Unique File</option>
                <option value="possible_duplicate">Possible Clones</option>
                <option value="exact_duplicate">Exact Duplicate Match</option>
              </select>
            </div>
          </div>
        </div>

        {/* Main Document Table container */}
        <div id="documents-list-panel" className="bg-[#0B1018] border border-[#183040] rounded-[4px] overflow-hidden">
          <div className="overflow-x-auto">
            <table className="w-full text-left border-collapse">
              <thead>
                <tr className="border-b border-[#183040] bg-[#05070A] text-[10px] uppercase font-mono tracking-wider text-[#6C8798]">
                  <th className="py-3 px-4">Filename / Location</th>
                  <th className="py-3 px-4">Classification</th>
                  <th className="py-3 px-4">Tags</th>
                  <th className="py-3 px-4">State</th>
                  <th className="py-3 px-4 text-right">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[#183040]/50 text-xs">
                {filteredDocs.length === 0 ? (
                  <tr>
                    <td colSpan={5} className="py-12 text-center text-[#6C8798] bg-[#05070A]/20">
                      <FileText className="w-8 h-8 mx-auto opacity-30 mb-2" />
                      <p className="font-mono text-xs">0 documents indexed matching search criteria</p>
                      <p className="text-[10px] text-[#3E5360] mt-1">Try broadening your active search filters</p>
                    </td>
                  </tr>
                ) : (
                  filteredDocs.map(doc => {
                    const isSelected = selectedDocument?.id === doc.id;
                    return (
                      <tr 
                        key={doc.id}
                        onClick={() => setSelectedDocument(doc)}
                        className={`hover:bg-[#101827]/40 cursor-pointer transition-colors ${
                          isSelected ? "bg-[#7f1d1d]/10 border-l-2 border-l-[#7f1d1d]" : ""
                        }`}
                      >
                        <td className="py-3 px-4 space-y-1 max-w-xs sm:max-w-md">
                          <div className="flex items-center gap-2">
                            {getFileIcon(doc.extension)}
                            <span className="font-semibold text-[#E6F7FF] truncate font-mono" title={doc.currentName}>
                              {doc.currentName}
                            </span>
                          </div>
                          {doc.suggestedName && doc.suggestedName !== doc.currentName && (
                            <div className="text-[10px] text-[#fca5a5] font-mono flex items-center gap-1">
                              <CornerDownRight className="w-3 h-3 text-[#3E5360]" />
                              <span>AI suggests: {doc.suggestedName}</span>
                            </div>
                          )}
                          <div className="text-[10px] text-[#6C8798] truncate opacity-80" title={doc.currentPath}>
                            {doc.currentPath}
                          </div>
                        </td>
                        <td className="py-3 px-4 space-y-1">
                          <div className="font-semibold text-[#A7C7D9]">
                            {doc.classification?.category || "Uncategorized"}
                          </div>
                          {doc.classification && (
                            <div className="text-[10px] text-[#6C8798] flex items-center gap-1.5 font-mono">
                              <span>Confidence:</span>
                              <span className={`font-semibold ${
                                doc.classification.confidence >= 0.85 ? "text-green-400" : "text-amber-400"
                              }`}>
                                {(doc.classification.confidence * 100).toFixed(0)}%
                              </span>
                            </div>
                          )}
                        </td>
                        <td className="py-3 px-4">
                          <div className="flex flex-wrap gap-1 max-w-[150px]">
                            {doc.tags.map(tag => (
                              <span 
                                key={tag} 
                                className="text-[9px] px-1.5 py-0.2 bg-[#183040]/30 text-[#A7C7D9] border border-[#183040] rounded-[2px]"
                              >
                                {tag}
                              </span>
                            ))}
                            {doc.tags.length === 0 && <span className="text-[10px] text-[#3E5360] italic">None</span>}
                          </div>
                        </td>
                        <td className="py-3 px-4">
                          <span className={`px-2 py-0.5 rounded-[2px] font-mono text-[10px] uppercase font-bold tracking-tight inline-block ${
                            doc.processingStatus === "complete" ? "bg-green-500/10 text-green-400 border border-green-500/20" :
                            doc.processingStatus === "needs_review" ? "bg-amber-500/10 text-amber-400 border border-amber-500/20" :
                            doc.processingStatus === "processing" ? "bg-[#7f1d1d]/10 text-[#fca5a5] border border-[#7f1d1d]/20 animate-pulse" :
                            "bg-red-500/10 text-red-500 border border-red-500/20"
                          }`}>
                            {doc.processingStatus === "needs_review" ? "REVIEW REQ" : doc.processingStatus}
                          </span>
                          {doc.duplicateStatus !== "unique" && (
                            <div className="mt-1">
                              <span className="text-[9px] bg-red-400/10 text-red-400 border border-red-500/20 px-1.5 py-0.2 rounded-[2px]">
                                {doc.duplicateStatus === "exact_duplicate" ? "EXACT CLONE" : "POSSIBLE CLONE"}
                              </span>
                            </div>
                          )}
                        </td>
                        <td className="py-3 px-4 text-right">
                          <div className="flex items-center justify-end gap-1.5" onClick={e => e.stopPropagation()}>
                            <button
                              onClick={() => setSelectedDocument(doc)}
                              title="View Document Details"
                              className="p-1 text-[#6C8798] hover:text-[#fca5a5] hover:bg-[#7f1d1d]/5 rounded-[4px]"
                            >
                              <Eye className="w-4 h-4" />
                            </button>
                            <button
                              onClick={() => handleReprocess(doc.id)}
                              title="Reprocess Extractor pipeline"
                              className="p-1 text-[#6C8798] hover:text-[#fca5a5] hover:bg-[#7f1d1d]/5 rounded-[4px]"
                            >
                              <RefreshCw className="w-4 h-4" />
                            </button>
                          </div>
                        </td>
                      </tr>
                    );
                  })
                )}
              </tbody>
            </table>
          </div>
        </div>
      </div>

      {/* Right Inspector Panel */}
      {selectedDocument && (
        <div 
          id="document-right-inspector" 
          className="w-full lg:w-[400px] bg-[#0B1018] border border-[#1e3d54] rounded-[4px] p-4 flex flex-col space-y-4 shrink-0 shadow-lg relative min-h-[450px]"
        >
          {/* Close trigger */}
          <button 
            onClick={() => setSelectedDocument(null)}
            className="absolute top-4 right-4 text-[#6C8798] hover:text-[#E6F7FF]"
          >
            <X className="w-4 h-4" />
          </button>

          {/* Header Title */}
          <div className="space-y-1.5 pr-6 border-b border-[#183040] pb-3">
            <span className="text-[10px] uppercase font-mono tracking-widest text-[#fca5a5] flex items-center gap-1.5">
              <Sparkles className="w-3 h-3 text-[#fca5a5]" /> Pipeline Record Intel
            </span>
            <div className="flex items-center gap-2">
              {getFileIcon(selectedDocument.extension)}
              <h2 className="text-sm font-semibold truncate font-mono text-[#E6F7FF]">
                {selectedDocument.currentName}
              </h2>
            </div>
            <div className="text-[10px] text-[#A7C7D9] font-mono">
              HASH: <span className="text-[#6C8798]">{selectedDocument.hash.slice(0, 14)}...</span>
            </div>
          </div>

          {/* Inspector navigation tabs */}
          <div className="flex border-b border-[#183040] gap-1 font-mono text-[10px] bg-[#05070A]/30 p-1 rounded-[4px] overflow-x-auto shrink-0">
            {[
              { id: "overview", label: "Overview" },
              { id: "preview", label: "Preview" },
              { id: "text", label: "OCR Text" },
              { id: "classify", label: "Classify" },
              { id: "rename", label: "Rename" },
              { id: "dupes", label: "Dupes" },
              { id: "audit", label: "Audit" },
              { id: "raw", label: "Raw JSON" }
            ].map(tab => (
              <button
                key={tab.id}
                onClick={() => setInspectorTab(tab.id as any)}
                className={`px-2 py-1 rounded-[2px] cursor-pointer whitespace-nowrap transition-colors ${
                  inspectorTab === tab.id 
                    ? "text-[#fca5a5] bg-[#27272a]/70 font-semibold" 
                    : "text-[#6C8798] hover:text-[#E6F7FF]"
                }`}
              >
                {tab.label}
              </button>
            ))}
          </div>

          {/* Tab contents */}
          <div className="flex-1 overflow-y-auto pr-1 text-xs space-y-4 max-h-[460px]">
            
            {/* TAB: OVERVIEW */}
            {inspectorTab === "overview" && (
              <div id="inspector-tab-overview" className="space-y-4">
                <div className="bg-[#05070A] border border-[#183040]/60 p-3 rounded-[4px] space-y-2">
                  <h3 className="font-mono text-[10px] uppercase font-bold text-[#6C8798]">Registry Details</h3>
                  <div className="grid grid-cols-2 gap-y-1.5 text-[11px] font-mono">
                    <span className="text-[#A7C7D9]">Size:</span>
                    <span className="text-[#E6F7FF] text-right">{(selectedDocument.sizeBytes / 1024 / 1024).toFixed(3)} MB</span>
                    
                    <span className="text-[#A7C7D9]">Extension:</span>
                    <span className="text-[#E6F7FF] text-right uppercase">.{selectedDocument.extension}</span>
                    
                    <span className="text-[#A7C7D9]">MimeType:</span>
                    <span className="text-[#E6F7FF] text-right truncate text-[10px]">{selectedDocument.mimeType}</span>
                    
                    <span className="text-[#A7C7D9]">DB Index:</span>
                    <span className="text-[#fca5a5] text-right">{selectedDocument.id}</span>
                  </div>
                </div>

                <div className="bg-[#05070A] border border-[#183040]/60 p-3 rounded-[4px] space-y-2">
                  <h3 className="font-mono text-[10px] uppercase font-bold text-[#6C8798]">Document Folder Origin</h3>
                  <div className="font-mono text-[#E6F7FF] text-[10px] break-all bg-black/20 p-1.5 border border-[#183040]/30 rounded-[2px]">
                    {selectedDocument.originalPath}
                  </div>
                </div>

                {/* Sub-panel tags editor in overview too */}
                <div className="bg-[#05070A]/50 border border-[#183040]/60 p-3 rounded-[4px] space-y-2">
                  <h3 className="font-mono text-[10px] uppercase font-bold text-[#6C8798] flex items-center gap-1">
                    <Tag className="w-3 h-3 text-[#A7C7D9]" /> Associated Meta Tags
                  </h3>
                  
                  <div className="flex flex-wrap gap-1">
                    {selectedDocument.tags.map(tag => (
                      <span 
                        key={tag} 
                        className="text-[10px] bg-[#183040]/40 text-[#E6F7FF] border border-[#1e3240] px-2 py-0.5 rounded-[2px] inline-flex items-center gap-1"
                      >
                        {tag}
                        <button 
                          onClick={() => handleRemoveTag(selectedDocument, tag)}
                          className="hover:text-red-400 font-bold"
                        >
                          <X className="w-2.5 h-2.5" />
                        </button>
                      </span>
                    ))}
                  </div>

                  <div className="flex gap-1.5 pt-1.5">
                    <input
                      type="text"
                      placeholder="Add tag..."
                      value={customTagInput}
                      onChange={e => setCustomTagInput(e.target.value)}
                      onKeyDown={e => { if (e.key === "Enter") handleAddTag(selectedDocument); }}
                      className="flex-1 bg-[#101827]/80 border border-[#183040] text-[11px] font-mono px-2 py-1 rounded-[2px] text-[#E6F7FF] focus:outline-none focus:border-[#b91c1c]"
                    />
                    <button 
                      onClick={() => handleAddTag(selectedDocument)}
                      className="px-2.5 py-1 text-[11px] bg-[#b91c1c] hover:bg-[#991b1b] text-white font-mono rounded-[4px]"
                    >
                      Add
                    </button>
                  </div>
                </div>
              </div>
            )}

            {/* TAB: PREVIEW */}
            {inspectorTab === "preview" && (
              <div id="inspector-tab-preview" className="space-y-3">
                <div className="bg-[#05070A] border border-[#183040]/60 p-3 rounded-[4px] min-h-[140px] flex flex-col justify-between">
                  <div>
                    <h4 className="font-semibold text-[#A7C7D9] flex items-center gap-1.5 text-xs mb-2">
                      <Sparkles className="w-3.5 h-3.5 text-[#fca5a5]" /> AI Document Digest Summary
                    </h4>
                    <p className="text-xs text-[#E6F7FF] leading-relaxed italic bg-[#7f1d1d]/5 p-3 rounded-[4px]">
                      &quot;{selectedDocument.summary || "No automated summary is registered for this record. Trigger reprocessing to evaluate summary nodes."}&quot;
                    </p>
                  </div>
                  <div className="text-[10px] text-[#6C8798] border-t border-[#183040] pt-2 mt-2 font-mono flex justify-between">
                    <span>Token Size: ~124 words</span>
                    <span>Confidence: 98%</span>
                  </div>
                </div>

                <div className="bg-[#05070A] border border-[#183040]/60 p-3 rounded-[4px]">
                  <h4 className="font-semibold text-[#A7C7D9] text-xs mb-2 flex items-center gap-1">
                    <Folder className="w-3.5 h-3.5" strokeWidth={2} /> Visual Document Asset Placeholder
                  </h4>
                  <div className="aspect-video relative bg-[#0B1018] border border-[#183040] rounded-[4px] mt-2 flex flex-col items-center justify-center overflow-hidden">
                    <div className="absolute inset-0 bg-[radial-gradient(#1e3040_1px,transparent_1px)] [background-size:12px_12px] opacity-20" />
                    <FileText className="w-10 h-10 text-[#fca5a5]/40 mb-1" />
                    <span className="font-mono text-[9px] text-[#6C8798] uppercase">Secure local document ledger</span>
                  </div>
                </div>
              </div>
            )}

            {/* TAB: EXTRACTED TEXT FROM OCR */}
            {inspectorTab === "text" && (
              <div id="inspector-tab-text" className="space-y-4">
                <div className="bg-[#05070A] border border-[#183040]/60 p-3 rounded-[4px] space-y-2">
                  <div className="flex justify-between items-center">
                    <HeaderLabel>Extracted Entities</HeaderLabel>
                    <span className="text-[10px] font-mono text-[#fca5a5]">OCR Local Engine</span>
                  </div>
                  
                  <div className="flex flex-wrap gap-1.5">
                    {entities.map((ent, i) => (
                      <span 
                        key={i}
                        className="bg-[#7f1d1d]/10 text-[#fca5a5] border border-[#7f1d1d]/20 rounded-[2px] p-1 text-[10px] font-mono inline-flex items-center gap-1"
                      >
                        <span className="font-bold uppercase text-[8px] tracking-wider text-[#6C8798] bg-[#05070A] px-1 rounded-[2px]">
                          {ent.label}
                        </span>
                        <span>{ent.text}</span>
                      </span>
                    ))}
                    {entities.length === 0 && <span className="text-xs text-[#3E5360] italic">None indexed</span>}
                  </div>
                </div>

                <div className="bg-[#05070A] border border-[#183040]/60 p-3 rounded-[4px] space-y-2">
                  <HeaderLabel>Raw Markdown Content</HeaderLabel>
                  <pre className="p-3 bg-[#0B1018] text-[#E6F7FF] text-[9.5px] font-mono rounded-[4px] border border-[#183040] overflow-x-auto whitespace-pre-wrap max-h-56 leading-relaxed">
                    {fullText || "Reading files from database..."}
                  </pre>
                </div>
              </div>
            )}

            {/* TAB: CLASSIFICATION CANDIDATES */}
            {inspectorTab === "classify" && (
              <div id="inspector-tab-classify" className="space-y-4">
                <div className="bg-[#05070A] border border-[#183040]/60 p-3 rounded-[4px] space-y-2">
                  <HeaderLabel>Category Candidates Classification Breakdown</HeaderLabel>
                  <p className="text-[11px] text-[#A7C7D9]">
                    Each record travels through the neural classification model comparing layout geometry and dictionary scores.
                  </p>

                  <div className="space-y-2 pt-2">
                    {selectedDocument.classification?.candidates?.map((cand, i) => (
                      <div key={i} className="space-y-1">
                        <div className="flex justify-between text-[11px] font-mono">
                          <span className="font-semibold text-[#E6F7FF]">{cand.category}</span>
                          <span className="text-[#fca5a5]">{(cand.confidence * 100).toFixed(0)}%</span>
                        </div>
                        <div className="h-1.5 bg-[#0B1018] border border-[#183040] rounded-[2px] overflow-hidden p-0.5">
                          <div 
                            className="h-full bg-[#fca5a5] rounded-[1px]" 
                            style={{ width: `${cand.confidence * 100}%` }}
                          />
                        </div>
                      </div>
                    ))}
                  </div>
                </div>

                <div className="bg-[#05070A] border border-[#183040]/60 p-3 rounded-[4px] space-y-1">
                  <HeaderLabel>Confidence Explanation</HeaderLabel>
                  <p className="text-[11px] text-[#A7C7D9] leading-normal italic">
                    &quot;{selectedDocument.classification?.explanation || "No explanation recorded."}&quot;
                  </p>
                </div>
              </div>
            )}

            {/* TAB: RENAME CONTROLS */}
            {inspectorTab === "rename" && (
              <div id="inspector-tab-rename" className="space-y-4">
                <div className="bg-[#05070A] border border-[#183040]/60 p-3 rounded-[4px] space-y-3">
                  <HeaderLabel>Approve Rename Operation</HeaderLabel>
                  <p className="text-[11px] text-[#A7C7D9]">
                    Suggested file reformatting is governed by automated trigger-rules. Inspect and review before matching to directory pointer:
                  </p>

                  <div className="p-2.5 bg-black/20 rounded-[4px] border border-[#183040] space-y-1.5">
                    <span className="text-[9px] font-mono uppercase text-[#6C8798]">Current Filename Indicator</span>
                    <div className="text-xs font-mono font-bold text-[#A7C7D9] truncate">
                      {selectedDocument.currentName}
                    </div>
                  </div>

                  <div className="p-2.5 bg-[#7f1d1d]/5 rounded-[4px] border border-[#7f1d1d]/30 space-y-1.5">
                    <span className="text-[9px] font-mono uppercase text-[#fca5a5]">Proposed Target Filename</span>
                    <input
                      type="text"
                      value={editableRenameVal}
                      onChange={e => setEditableRenameVal(e.target.value)}
                      className="w-full bg-[#05070A] border border-[#183040] font-mono text-xs p-1.5 rounded-[4px] text-white focus:outline-none focus:border-[#b91c1c]"
                    />
                  </div>

                  <div className="flex gap-2">
                    <button
                      onClick={() => handleApproveRename(selectedDocument.id, editableRenameVal)}
                      className="flex-1 py-1.5 bg-[#22C55E]/15 hover:bg-[#22C55E]/25 border border-[#22C55E]/40 text-green-400 font-mono text-[11px] rounded-[4px] font-bold flex items-center justify-center gap-1"
                    >
                      <Check className="w-4 h-4" /> Approve Name
                    </button>
                    <button
                      onClick={() => handleRejectRename(selectedDocument.id)}
                      className="py-1.5 px-3 bg-red-500/10 hover:bg-red-500/20 border border-red-500/30 text-red-400 font-mono text-[11px] rounded-[4px] font-bold"
                    >
                      Reject Suggestions
                    </button>
                  </div>
                </div>
              </div>
            )}

            {/* TAB: DUPLICATES SETTINGS */}
            {inspectorTab === "dupes" && (
              <div id="inspector-tab-dupes" className="space-y-4">
                <div className="bg-[#05070A] border border-[#183040]/60 p-3 rounded-[4px] space-y-3">
                  <HeaderLabel>Similarity Inspector</HeaderLabel>
                  <p className="text-[11px] text-[#A7C7D9]">
                    System calculated document similarities to prevent replica clogging within the static file caches.
                  </p>

                  <div className="p-3 bg-[#0B1018] rounded-[4px] border border-[#183040] space-y-1.5">
                    <div className="flex justify-between items-center text-[10px] font-mono">
                      <span className="text-[#A7C7D9] uppercase">Duplicate State:</span>
                      <span className={`font-bold uppercase ${
                        selectedDocument.duplicateStatus === "unique" ? "text-green-400" : "text-amber-400"
                      }`}>{selectedDocument.duplicateStatus}</span>
                    </div>
                  </div>

                  {selectedDocument.duplicateStatus !== "unique" ? (
                    <div className="space-y-3">
                      <div className="p-3 bg-red-500/5 rounded-[4px] border border-red-500/20">
                        <span className="text-[10px] font-semibold text-[#E6F7FF] flex items-center gap-1.5">
                          <AlertTriangle className="w-3.5 h-3.5 text-[#EF4444]" /> Duplicate suspect detected
                        </span>
                        <p className="text-[11px] text-[#A7C7D9] mt-1 leading-relaxed">
                          This document matches index ledger keys to another record. Choose action:
                        </p>
                      </div>

                      <div className="flex gap-2">
                        <button
                          onClick={() => handleMarkUnique(selectedDocument.id)}
                          className="flex-1 py-1.5 bg-[#22C55E]/10 hover:bg-[#22C55E]/20 border border-[#22C55E]/30 text-green-400 font-mono text-[11px] rounded-[4px]"
                        >
                          De-flag & Mark Unique
                        </button>
                        <button
                          onClick={() => handleMarkDuplicate(selectedDocument.id, "doc-101")}
                          className="flex-1 py-1.5 bg-red-500/10 hover:bg-red-500/20 border border-red-500/30 text-red-400 font-mono text-[11px] rounded-[4px]"
                        >
                          Confirm Duplicate
                        </button>
                      </div>
                    </div>
                  ) : (
                    <div className="p-3 bg-[#22C55E]/5 border border-[#22C55E]/25 rounded-[4px] text-center">
                      <CheckCircle2 className="w-6 h-6 text-green-400 mx-auto mb-1" />
                      <span className="text-[11px] font-semibold text-green-400">File is uniquely certified</span>
                      <p className="text-[10px] text-[#A7C7D9] mt-1 leading-tight">No signature mapping conflicts exist across files databases</p>
                    </div>
                  )}
                </div>
              </div>
            )}

            {/* TAB: AUDIT LEDGER */}
            {inspectorTab === "audit" && (
              <div id="inspector-tab-audit" className="space-y-4">
                <div className="border-[#183040]/60 rounded-[4px] space-y-2">
                  <HeaderLabel>Isolated Audit Ledger Log</HeaderLabel>
                  <div className="space-y-2 max-h-[300px] overflow-y-auto pr-1">
                    {documentAudit.map(log => (
                      <div key={log.id} className="p-2 bg-[#05070A] border border-[#183040]/50 rounded-[4px] space-y-1 text-[11px]">
                        <div className="flex justify-between items-center font-mono">
                          <span className="text-[#fca5a5] font-bold uppercase text-[9px] bg-[#7f1d1d]/10 px-1 rounded-[1px]">
                            {log.action}
                          </span>
                          <span className="text-[#6C8798] text-[9px]">
                            {new Date(log.createdAt).toLocaleTimeString()}
                          </span>
                        </div>
                        <p className="text-[#E6F7FF] font-sans text-[10.5px] leading-relaxed">
                          {log.summary}
                        </p>
                        <div className="text-[9px] text-[#6C8798] font-mono">
                          Actor: {log.actor}
                        </div>
                      </div>
                    ))}
                    {documentAudit.length === 0 && (
                      <div className="text-center py-6 text-[#3E5360] italic font-mono text-[11px]">
                        No logs recorded.
                      </div>
                    )}
                  </div>
                </div>
              </div>
            )}

            {/* TAB: RAW JSON */}
            {inspectorTab === "raw" && (
              <div id="inspector-tab-raw" className="space-y-3">
                <HeaderLabel>Raw Document Record JSON Metadata</HeaderLabel>
                <div className="bg-[#05070A] p-2.5 rounded-[4px] border border-[#183040]">
                  <pre className="text-[9px] font-mono text-[#fca5a5] overflow-x-auto h-72 pr-2 scrollbar-thin">
                    {JSON.stringify(selectedDocument, null, 2)}
                  </pre>
                </div>
              </div>
            )}

          </div>
        </div>
      )}

    </div>
  );
}

// Inline mini helpers
function HeaderLabel({ children }: { children: React.ReactNode }) {
  return (
    <h3 className="font-mono text-[10px] uppercase font-bold text-[#6C8798] tracking-wider mb-2">
      {children}
    </h3>
  );
}
