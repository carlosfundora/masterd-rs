import React, { useState } from "react";
import { 
  AlertTriangle, Check, X, ShieldAlert, BadgeInfo, Eye, ThumbsUp, FileText, 
  Settings, Sliders, CheckSquare, Edit, RefreshCw, CheckCircle2
} from "lucide-react";
import { ReviewItem, DocumentRecord, MasterdFrontendBridge } from "../contracts/api";

type ReviewQueueProps = {
  bridge: MasterdFrontendBridge | null;
  reviewQueue: ReviewItem[];
  documents: DocumentRecord[];
  refreshState: () => void;
  setSelectedDocument: (doc: DocumentRecord | null) => void;
  setActiveTab: (tab: string) => void;
};

export default function ReviewQueue({
  bridge,
  reviewQueue,
  documents,
  refreshState,
  setSelectedDocument,
  setActiveTab
}: ReviewQueueProps) {
  const [selectedReviewId, setSelectedReviewId] = useState<string | null>(
    reviewQueue.find(r => !r.resolved)?.id || null
  );

  // Active severity filters
  const [severityFilter, setSeverityFilter] = useState<string>("");

  const pendingReviews = reviewQueue.filter(r => !r.resolved);

  const filteredReviews = pendingReviews.filter(r => {
    return severityFilter === "" || r.severity === severityFilter;
  });

  const selectedReview = filteredReviews.find(r => r.id === selectedReviewId) || filteredReviews[0];

  // Action methods
  const handleResolveAction = async (approved: boolean, customCategory?: string, customName?: string) => {
    if (!bridge || !selectedReview) return;

    let fieldsPatch: Record<string, unknown> = {};
    if (selectedReview.proposedAction?.type === "classify" && customCategory) {
      fieldsPatch = { category: customCategory };
    }
    if (selectedReview.proposedAction?.type === "rename" && customName) {
      fieldsPatch = { suggestedName: customName };
    }

    const res = await bridge.review.resolve(selectedReview.id, {
      approved,
      editedFields: fieldsPatch,
      notes: approved ? "Approved via manual supervisor dashboard review." : "Rejected by supervisor action."
    });

    if (res.ok) {
      setSelectedReviewId(null);
      refreshState();
    }
  };

  // State to hold manual edits inside review box
  const [customEditVal, setCustomEditVal] = useState("");
  const [isEditingValue, setIsEditingValue] = useState(false);

  const matchedDocument = selectedReview 
    ? documents.find(d => d.id === selectedReview.documentId) 
    : null;

  const handleEditClick = () => {
    if (!selectedReview) return;
    setIsEditingValue(true);
    if (selectedReview.proposedAction?.type === "classify") {
      setCustomEditVal((selectedReview.proposedAction.after?.category as string) || "");
    } else if (selectedReview.proposedAction?.type === "rename") {
      setCustomEditVal((selectedReview.proposedAction.after?.suggestedName as string) || "");
    }
  };

  const handleSaveEditResolve = () => {
    if (selectedReview?.proposedAction?.type === "classify") {
      handleResolveAction(true, customEditVal);
    } else if (selectedReview?.proposedAction?.type === "rename") {
      handleResolveAction(true, undefined, customEditVal);
    }
    setIsEditingValue(false);
  };

  const getSeverityIcon = (sev: string) => {
    switch (sev) {
      case "critical":
        return <ShieldAlert className="w-4 h-4 text-red-500" />;
      case "warning":
        return <AlertTriangle className="w-4 h-4 text-amber-500" />;
      default:
        return <BadgeInfo className="w-4 h-4 text-[#3B82F6]" />;
    }
  };

  const currentTabNavigate = () => {
    if (matchedDocument) {
      setSelectedDocument(matchedDocument);
      setActiveTab("documents");
    }
  };

  return (
    <div id="review-screen" className="space-y-6 text-[#E6F7FF]">
      
      {/* Table grid and Resolution details side window */}
      <div id="review-grid-layout" className="grid grid-cols-1 lg:grid-cols-5 gap-6">
        
        {/* Unresolved Review Items list */}
        <div className="bg-[#0B1018] border border-[#183040] p-5 rounded-[4px] lg:col-span-3 space-y-4 flex flex-col min-h-[450px]">
          <div className="border-b border-[#183040] pb-3 flex flex-col sm:flex-row justify-between sm:items-center gap-2">
            <h2 className="text-sm font-semibold uppercase tracking-wider text-[#E6F7FF] flex items-center gap-2">
              <CheckSquare className="w-4 h-4 text-[#00E5FF]" /> Human Intelligence Docket Queue
            </h2>

            {/* Filter toolbar */}
            <div className="flex gap-1.5 font-mono text-[10px]">
              <button
                onClick={() => setSeverityFilter("")}
                className={`px-2 py-0.5 rounded-[2px] border ${
                  severityFilter === "" ? "border-[#00E5FF] text-[#00E5FF] bg-[#00E5FF]/5" : "border-[#183040] text-[#6C8798]"
                }`}
              >
                ALL ({pendingReviews.length})
              </button>
              <button
                onClick={() => setSeverityFilter("critical")}
                className={`px-2 py-0.5 rounded-[2px] border ${
                  severityFilter === "critical" ? "border-red-500 text-red-400 bg-red-500/5" : "border-[#183040] text-[#6C8798]"
                }`}
              >
                CRITICAL ({pendingReviews.filter(r => r.severity === "critical").length})
              </button>
              <button
                onClick={() => setSeverityFilter("warning")}
                className={`px-2 py-0.5 rounded-[2px] border ${
                  severityFilter === "warning" ? "border-amber-400 text-amber-400 bg-amber-400/5" : "border-[#183040] text-[#6C8798]"
                }`}
              >
                WARNINGS ({pendingReviews.filter(r => r.severity === "warning").length})
              </button>
            </div>
          </div>

          <div className="overflow-y-auto flex-1 max-h-[450px] space-y-2 pr-1">
            {filteredReviews.length === 0 ? (
              <div className="text-center py-20 text-[#6C8798]">
                <CheckCircle2 className="w-10 h-10 mx-auto text-green-400 mb-2 opacity-85" />
                <p className="font-mono text-sm uppercase font-bold text-[#E6F7FF]">Docket fully verified</p>
                <p className="text-[11px] text-[#A7C7D9] mt-1">Outstanding classifications have satisfied certainty thresholds.</p>
              </div>
            ) : (
              filteredReviews.map(review => {
                const isSelected = selectedReview?.id === review.id;
                return (
                  <div 
                    key={review.id}
                    onClick={() => {
                      setSelectedReviewId(review.id);
                      setIsEditingValue(false);
                    }}
                    className={`p-3.5 bg-[#05070A] border rounded-[4px] cursor-pointer transition-colors space-y-2 ${
                      isSelected 
                        ? "border-[#3B82F6] bg-[#3B82F6]/5" 
                        : "border-[#183040] hover:border-[#28576C]"
                    }`}
                  >
                    <div className="flex justify-between items-start gap-3">
                      <div className="flex items-center gap-2">
                        {getSeverityIcon(review.severity)}
                        <h4 className="text-xs font-semibold text-[#E6F7FF]">
                          {review.title}
                        </h4>
                      </div>
                      <span className="font-mono text-[9px] text-[#6C8798]">
                        ID: {review.id}
                      </span>
                    </div>

                    <p className="text-[11px] text-[#A7C7D9] leading-relaxed italic">
                      &quot;{review.explanation}&quot;
                    </p>

                    <div className="flex justify-between items-center text-[10px] font-mono border-t border-[#183040]/30 pt-1.5 mt-1 text-[#6C8798]">
                      <span>Trigger Core: {review.reason.replace(/_/g, " ")}</span>
                      <span className="text-[#3B82F6] hover:underline" onClick={currentTabNavigate}>
                        Inspect file →
                      </span>
                    </div>
                  </div>
                );
              })
            )}
          </div>
        </div>

        {/* Selected item analysis panel and controls */}
        <div id="review-panel-actions" className="bg-[#0B1018] border border-[#183040] p-5 rounded-[4px] lg:col-span-2 flex flex-col h-[510px]">
          <div className="border-b border-[#183040] pb-3 flex justify-between items-center shrink-0">
            <h2 className="text-sm font-semibold uppercase tracking-wider text-[#E6F7FF] flex items-center gap-2 font-mono">
              <Sliders className="w-4 h-4 text-[#00E5FF]" /> Decisional Panel
            </h2>
          </div>

          {selectedReview ? (
            <div className="flex-1 flex flex-col justify-between overflow-hidden pt-4">
              
              <div className="space-y-4 overflow-y-auto pr-1">
                {/* Meta details */}
                <div className="p-3 bg-[#05070A] border border-[#183040] rounded-[4px] space-y-2 font-mono text-[11px]">
                  <div className="text-[10px] uppercase font-bold text-[#6C8798]">Target Document Association</div>
                  <div className="flex items-center gap-2 text-[#E6F7FF] font-semibold truncate">
                    <FileText className="w-3.5 h-3.5 text-cyan-400" />
                    <span className="truncate">{matchedDocument?.currentName}</span>
                  </div>
                  <div className="text-[10px] text-[#A7C7D9] truncate">
                    Origin Path: {matchedDocument?.originalPath}
                  </div>
                </div>

                {/* Proposed actions visualization diff */}
                {selectedReview.proposedAction && (
                  <div className="space-y-3">
                    <div className="text-[11px] font-mono uppercase font-bold text-[#6C8798]">Proposed Modification Audit</div>
                    
                    {/* Before parameters */}
                    <div className="p-2.5 bg-black/20 border border-[#183040] rounded-[4px]">
                      <span className="text-[9px] uppercase font-mono tracking-wider text-rose-400">Before Change State:</span>
                      <pre className="text-[10px] font-mono text-[#A7C7D9] mt-1 break-all truncate">
                        {selectedReview.proposedAction.type === "classify" 
                          ? `Classification: ${JSON.stringify(selectedReview.proposedAction.before || "Uncategorized")}`
                          : `Filename: ${JSON.stringify(selectedReview.proposedAction.before || matchedDocument?.currentName)}`
                        }
                      </pre>
                    </div>

                    {/* Proposed changes */}
                    <div className="p-2.5 bg-[#00E5FF]/5 border border-[#00E5FF]/20 rounded-[4px]">
                      <span className="text-[9px] uppercase font-mono tracking-wider text-[#00E5FF]">Proposed Target State:</span>
                      
                      {isEditingValue ? (
                        <div className="mt-1 flex gap-1.5">
                          <input 
                            type="text" 
                            value={customEditVal} 
                            onChange={e => setCustomEditVal(e.target.value)}
                            className="bg-[#05070A] border border-[#00E5FF] text-xs font-mono p-1 rounded-[2px] flex-1 text-white focus:outline-none"
                          />
                          <button 
                            onClick={handleSaveEditResolve}
                            className="px-2 bg-[#22C55E]/10 border border-green-500/20 text-green-400 font-bold text-[10px] rounded-[2px]"
                          >
                            Save
                          </button>
                        </div>
                      ) : (
                        <pre className="text-[10px] font-mono text-cyan-400 mt-1 break-all truncate font-bold flex justify-between items-center bg-black/10 py-1 px-1.5 rounded-[2px]">
                          <span>
                            {selectedReview.proposedAction.type === "classify" 
                              ? `Classification: "${selectedReview.proposedAction.after?.category || ''}"`
                              : `Filename: "${selectedReview.proposedAction.after?.suggestedName || ''}"`
                            }
                          </span>
                          <button 
                            onClick={handleEditClick}
                            className="p-1 hover:text-white"
                            title="Edit value manually"
                          >
                            <Edit className="w-3 h-3" />
                          </button>
                        </pre>
                      )}
                    </div>
                  </div>
                )}
              </div>

              {/* Action buttons footer */}
              <div className="pt-4 border-t border-[#183040]/70 space-y-2 shrink-0">
                <button
                  onClick={() => handleResolveAction(true)}
                  className="w-full py-2 bg-[#22C55E] hover:bg-green-600 text-white font-mono rounded-[4px] flex items-center justify-center gap-1.5 text-xs font-bold"
                >
                  <Check className="w-4 h-4 text-white" /> Commit Proposed Changes
                </button>

                <div className="grid grid-cols-2 gap-2 text-xs font-mono">
                  <button
                    onClick={() => handleResolveAction(false)}
                    className="py-1.5 bg-red-500/10 hover:bg-red-500/15 border border-red-500/30 text-red-400 rounded-[4px]"
                  >
                    Reject Proposal
                  </button>
                  <button
                    onClick={currentTabNavigate}
                    className="py-1.5 bg-[#05070A] hover:bg-[#101827] border border-[#183040] text-[#A7C7D9] rounded-[4px]"
                  >
                    View Document
                  </button>
                </div>
              </div>

            </div>
          ) : (
            <div className="flex-1 flex flex-col justify-center items-center py-10 text-center text-[#6C8798]">
              <ShieldAlert className="w-10 h-10 opacity-30 mb-2" />
              <p className="font-mono text-xs">Awaiting decisional docket query selection</p>
            </div>
          )}
        </div>

      </div>

    </div>
  );
}
