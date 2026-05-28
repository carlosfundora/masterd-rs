import React, { useState } from "react";
import { 
  Plus, Sliders, ToggleLeft, ToggleRight, Trash2, Edit2, Play, Terminal, 
  HelpCircle, CheckCircle2, ChevronRight, CornerDownRight, Workflow, Check, AlertTriangle, Hammer
} from "lucide-react";
import { AutomationRule, DocumentRecord, AutomationRuleDraft, RuleCondition, RuleAction, RuleTrigger, MasterdFrontendBridge } from "../contracts/api";

type RulesProps = {
  bridge: MasterdFrontendBridge | null;
  rules: AutomationRule[];
  documents: DocumentRecord[];
  refreshState: () => void;
};

export default function Rules({
  bridge,
  rules,
  documents,
  refreshState
}: RulesProps) {
  // Simulator selection states
  const [selectedSimDocId, setSelectedSimDocId] = useState("");
  const [selectedSimRuleId, setSelectedSimRuleId] = useState("");
  const [simLog, setSimLog] = useState<string[]>([]);
  const [simRunning, setSimRunning] = useState(false);

  // Rule Builder variables
  const [ruleName, setRuleName] = useState("");
  const [ruleDesc, setRuleDesc] = useState("");
  const [rulePriority, setRulePriority] = useState(5);
  const [ruleTrigger, setRuleTrigger] = useState<RuleTrigger["event"]>("classification_complete");
  const [ruleCondField, setRuleCondField] = useState("classification.category");
  const [ruleCondOp, setRuleCondOp] = useState<RuleCondition["operator"]>("equals");
  const [ruleCondVal, setRuleCondVal] = useState("");
  const [ruleActType, setRuleActType] = useState<RuleAction["type"]>("suggest_rename");
  const [ruleActVal, setRuleActVal] = useState("");

  const handleToggleRule = async (rule: AutomationRule) => {
    if (!bridge) return;
    const res = await bridge.rules.update(rule.id, { enabled: !rule.enabled });
    if (res.ok) {
      refreshState();
    }
  };

  const handleDeleteRule = async (id: string) => {
    if (!bridge) return;
    const res = await bridge.rules.delete(id);
    if (res.ok) {
      refreshState();
    }
  };

  const handleCreateRuleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!ruleName.trim()) return;

    // Compile values
    const draft: AutomationRuleDraft = {
      name: ruleName.trim(),
      description: ruleDesc.trim() || undefined,
      enabled: true,
      priority: Number(rulePriority),
      trigger: { event: ruleTrigger },
      conditions: [
        { field: ruleCondField, operator: ruleCondOp, value: ruleCondVal }
      ],
      actions: [
        ruleActType === "suggest_rename" 
          ? { type: "suggest_rename", template: ruleActVal || "{date}_{sender}_manual.{ext}" }
          : ruleActType === "suggest_tag"
          ? { type: "suggest_tag", tag: ruleActVal || "manual-tag" }
          : { type: "route_storage", destinationFolder: ruleActVal || "/Users/username/Desktop" }
      ],
      safetyLevel: ruleActType === "route_storage" ? "destructive" : "review_required"
    };

    if (!bridge) return;
    const res = await bridge.rules.create(draft);
    if (res.ok) {
      // Clear fields
      setRuleName("");
      setRuleDesc("");
      setRuleCondVal("");
      setRuleActVal("");
      refreshState();
    }
  };

  const handleSimulateDeploy = async () => {
    if (!selectedSimDocId || !selectedSimRuleId) {
      setSimLog(["[ABORT] Select both a document and a rule to evaluate the live check."]);
      return;
    }

    if (!bridge) return;

    const doc = documents.find(d => d.id === selectedSimDocId);
    const rule = rules.find(r => r.id === selectedSimRuleId);
    if (!doc || !rule) {
      setSimLog(["[FATAL] Selected rule or document is unavailable in the live workspace."]);
      return;
    }

    setSimRunning(true);
    setSimLog(["[INIT] Dispatching live rule evaluation against the selected document..."]);

    const res = await bridge.rules.test({
      name: rule.name,
      description: rule.description,
      enabled: rule.enabled,
      priority: rule.priority,
      trigger: rule.trigger,
      conditions: rule.conditions,
      actions: rule.actions,
      safetyLevel: rule.safetyLevel
    }, selectedSimDocId);

    if (res.ok) {
      setSimLog([
        `[LIVE] Rule test completed for "${doc.currentName}"`,
        `[MATCH] ${res.data.matched ? "Rule matched" : "Rule did not match"}`,
        ...res.data.actionsEvaluated.map(action => `[ACTION] ${action.type}: ${action.resultSummary}`)
      ]);
    } else {
      setSimLog([`[ERROR] ${res.error.code}: ${res.error.message}`]);
    }
    setSimRunning(false);
  };

  return (
    <div id="rules-screen" className="space-y-6 text-[#E6F7FF]">
      
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        
        {/* Rule Builder GUI Box */}
        <div id="rule-builder-panel" className="bg-[#0B1018] border border-[#183040] p-5 rounded-[4px] space-y-4">
          <div className="border-b border-[#183040] pb-3">
            <h2 className="text-sm font-semibold uppercase tracking-wider text-[#E6F7FF] flex items-center gap-2">
              <Sliders className="w-4 h-4 text-[#fca5a5]" /> Automation Rule Dictator
            </h2>
          </div>

          <form onSubmit={handleCreateRuleSubmit} className="space-y-3.5 text-xs">
            {/* Rule Name */}
            <div className="space-y-1">
              <label className="text-[10px] uppercase font-mono tracking-wider font-bold text-[#6C8798]">Rule Title</label>
              <input
                type="text"
                required
                placeholder="E.g., Autotag expense invoices"
                value={ruleName}
                onChange={e => setRuleName(e.target.value)}
                className="w-full bg-[#09090b] border border-[#27272a] p-2 rounded-[4px] text-xs font-mono text-white focus:outline-none focus:border-[#b91c1c]"
              />
            </div>

            {/* Rule Description */}
            <div className="space-y-1">
              <label className="text-[10px] uppercase font-mono tracking-wider font-bold text-[#6C8798]">Summary Description</label>
              <input
                type="text"
                placeholder="Brief summary of why this rule fires"
                value={ruleDesc}
                onChange={e => setRuleDesc(e.target.value)}
                className="w-full bg-[#05070A] border border-[#183040] p-2 rounded-[4px] text-xs font-mono text-white focus:outline-none"
              />
            </div>

            {/* Multi inputs Row */}
            <div className="grid grid-cols-2 gap-2">
              <div className="space-y-1">
                <label className="text-[10px] uppercase font-mono tracking-wider font-bold text-[#6C8798]">Core Trigger</label>
                <select
                  value={ruleTrigger}
                  onChange={e => setRuleTrigger(e.target.value as any)}
                  className="w-full bg-[#05070A] border border-[#183040] p-2 rounded-[4px] font-mono text-[#A7C7D9] focus:outline-none text-[11px]"
                >
                  <option value="classification_complete">Classification Complete</option>
                  <option value="extraction_complete">Extraction OCR Complete</option>
                  <option value="file_imported">When Imported</option>
                </select>
              </div>

              <div className="space-y-1">
                <label className="text-[10px] uppercase font-mono tracking-wider font-bold text-[#6C8798]">Priority Rank</label>
                <input
                  type="number"
                  min="1"
                  max="50"
                  value={rulePriority}
                  onChange={e => setRulePriority(Number(e.target.value))}
                  className="w-full bg-[#05070A] border border-[#183040] p-2 rounded-[4px] font-mono text-white focus:outline-none text-[11px]"
                >
                </input>
              </div>
            </div>

            {/* IF Conditions block */}
            <div className="bg-[#05070A] p-3 border border-[#183040] rounded-[4px] space-y-2">
              <span className="text-[9px] uppercase font-mono tracking-widest text-[#fca5a5]">IF Condition Met:</span>
              <div className="grid grid-cols-1 gap-2 font-mono text-[11px]">
                <div className="flex gap-1">
                  <select 
                    value={ruleCondField} 
                    onChange={e => setRuleCondField(e.target.value)}
                    className="flex-1 bg-[#05070A] border border-[#183040] rounded-[2px] p-1 text-[#A7C7D9] focus:outline-none"
                  >
                    <option value="classification.category">Classification Category</option>
                    <option value="tags">Associated tags</option>
                    <option value="currentName">Active Name</option>
                  </select>
                  
                  <select 
                    value={ruleCondOp} 
                    onChange={e => setRuleCondOp(e.target.value as any)}
                    className="bg-[#05070A] border border-[#183040] rounded-[2px] p-1 text-[#A7C7D9] focus:outline-none"
                  >
                    <option value="equals">Equals</option>
                    <option value="contains">Contains</option>
                    <option value="exists">Exists</option>
                  </select>
                </div>

                <input
                  type="text"
                  required
                  placeholder="Matching criteria value..."
                  value={ruleCondVal}
                  onChange={e => setRuleCondVal(e.target.value)}
                  className="w-full bg-[#101827] border border-[#183040] rounded-[2px] p-1 focus:outline-none text-white text-[11px]"
                />
              </div>
            </div>

            {/* THEN Actions block */}
            <div className="bg-[#05070A] p-3 border border-[#183040] rounded-[4px] space-y-2">
              <span className="text-[9px] uppercase font-mono tracking-widest text-green-400">THEN Automated Action:</span>
              <div className="grid grid-cols-1 gap-2 font-mono text-[11px]">
                <select 
                  value={ruleActType} 
                  onChange={e => setRuleActType(e.target.value as any)}
                  className="w-full bg-[#05070A] border border-[#183040] rounded-[2px] p-1 text-[#A7C7D9] focus:outline-none"
                >
                  <option value="suggest_rename">Suggest Document Rename</option>
                  <option value="suggest_tag">Append metadata tag</option>
                  <option value="route_storage">Route to storage folder</option>
                </select>

                <input
                  type="text"
                  required
                  placeholder={
                    ruleActType === "suggest_rename" ? "Template: {date}_{sender}_invoice.{ext}" :
                    ruleActType === "suggest_tag" ? "Tag text, e.g., legal" : "Target path, e.g., /Users/docs/Archive"
                  }
                  value={ruleActVal}
                  onChange={e => setRuleActVal(e.target.value)}
                  className="w-full bg-[#101827] border border-[#183040] rounded-[2px] p-1 focus:outline-none text-white text-[11px]"
                />
              </div>
            </div>

            <button
              type="submit"
              className="w-full py-2 bg-[#b91c1c] hover:bg-[#991b1b] border border-[#b91c1c] text-white font-mono font-bold rounded-[4px] cursor-pointer flex items-center justify-center gap-1 text-[11px]"
            >
              <Plus className="w-4 h-4" /> Enforce Active Rule (Order is mandatory)
            </button>
          </form>
        </div>

        {/* Rules Registry Directory */}
        <div id="rules-registry-list" className="bg-[#0B1018] border border-[#183040] p-5 rounded-[4px] space-y-4 flex flex-col h-[525px] overflow-hidden">
          <div className="border-b border-[#183040] pb-3">
            <h2 className="text-sm font-semibold uppercase tracking-wider text-[#E6F7FF] flex items-center gap-2">
              <Workflow className="w-4 h-4 text-[#fca5a5]" /> Trigger Rules Registry
            </h2>
          </div>

          <p className="text-xs text-[#A7C7D9] shrink-0">
            Automations occur when trigger events are satisfied. Priority values sort conflicting rule applications.
          </p>

          <div className="flex-1 overflow-y-auto space-y-3 pr-1 max-h-[400px]">
            {rules.map((rule, idx) => (
              <div 
                key={rule.id}
                className="p-3 bg-[#05070A] border border-[#183040] hover:border-[#1e3240] rounded-[4px] relative space-y-2 flex flex-col"
              >
                {/* Rule title and Toggle trigger */}
                <div className="flex justify-between items-start gap-4">
                  <div className="space-y-0.5 min-w-0">
                    <h3 className="text-xs font-semibold text-[#E6F7FF] truncate flex items-center gap-1">
                      <ChevronRight className="w-3.5 h-3.5 text-[#fca5a5] shrink-0" />
                      {rule.name}
                    </h3>
                    {rule.description && (
                      <p className="text-[10px] text-[#A7C7D9] line-clamp-1 italic">{rule.description}</p>
                    )}
                  </div>
                  <button 
                    onClick={() => handleToggleRule(rule)}
                    className="text-[#6C8798] hover:text-[#fca5a5] shrink-0"
                  >
                    {rule.enabled ? (
                      <ToggleRight className="w-7 h-7 text-green-400" />
                    ) : (
                      <ToggleLeft className="w-7 h-7 text-gray-500" />
                    )}
                  </button>
                </div>

                {/* IF - THEN conditions summary labels */}
                <div className="p-2 bg-[#0B1018] rounded-[2px] border border-[#183040]/50 space-y-1 font-mono text-[10px] text-[#6C8798]">
                  <div className="flex items-start gap-1">
                    <span className="text-[#fca5a5] font-bold shrink-0">IF:</span>
                    <span className="truncate text-[#A7C7D9]" title={rule.conditions[0]?.field}>
                      [{rule.conditions[0]?.field.replace(/(classification|suggested)/g, "")}] {rule.conditions[0]?.operator} &quot;{String(rule.conditions[0]?.value)}&quot;
                    </span>
                  </div>
                  <div className="flex items-start gap-1 border-t border-[#183040]/30 pt-1 mt-1">
                    <span className="text-green-400 font-bold shrink-0">THEN:</span>
                    <span className="truncate text-green-300 font-bold uppercase shrink-0">
                      [{rule.actions[0]?.type}]
                    </span>
                    <span className="truncate text-[#6C8798]" title={String(Object.values(rule.actions[0] || {})[1] || '')}>
                      {String(Object.values(rule.actions[0] || {})[1] || '')}
                    </span>
                  </div>
                </div>

                {/* Tags footer details */}
                <div className="flex justify-between items-center text-[10px] font-mono border-t border-[#183040]/30 pt-1.5 text-xs text-[#6C8798]">
                  <span>Priority: <span className="font-bold text-white font-mono">{rule.priority}</span></span>
                  
                  <div className="flex items-center gap-1.5">
                    <span className={`px-1 rounded-[1px] text-[8.5px] uppercase font-bold ${
                      rule.safetyLevel === "safe" ? "bg-green-500/10 text-green-400 border border-green-500/20" :
                      rule.safetyLevel === "review_required" ? "bg-amber-500/10 text-amber-400 border border-amber-500/20" :
                      "bg-red-500/10 text-red-400 border border-red-500/20"
                    }`}>
                      {rule.safetyLevel}
                    </span>
                    <button 
                      onClick={() => handleDeleteRule(rule.id)}
                      className="p-1 hover:text-red-400"
                      title="Archive rule"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                </div>

              </div>
            ))}
          </div>
        </div>

        {/* Live Rule Test */}
        <div id="rule-dryrun-simulator" className="bg-[#0B1018] border border-[#183040] p-5 rounded-[4px] space-y-4 flex flex-col h-[525px]">
          <div className="border-b border-[#183040] pb-3">
            <h2 className="text-sm font-semibold uppercase tracking-wider text-[#E6F7FF] flex items-center gap-2">
              <Terminal className="w-4 h-4 text-green-400" /> Live Rule Test
            </h2>
          </div>

          <p className="text-xs text-[#A7C7D9] shrink-0">
            Evaluate the rule's absolute authority over the selected document.
          </p>

          {/* Live test forms */}
          <div className="space-y-2 text-xs shrink-0 bg-[#05070A] p-3 rounded-[4px] border border-[#183040]/70">
            <div className="space-y-1">
              <label className="text-[10px] font-mono text-[#6C8798]">Select Target Document</label>
              <select
                value={selectedSimDocId}
                onChange={e => setSelectedSimDocId(e.target.value)}
                className="w-full bg-[#0B1018] border border-[#183040] p-1.5 rounded-[4px] font-mono text-[#E6F7FF] focus:outline-none"
              >
                <option value="">-- Choose document --</option>
                {documents.map(d => <option key={d.id} value={d.id}>{d.currentName} ({d.id})</option>)}
              </select>
            </div>

            <div className="space-y-1 pt-1">
              <label className="text-[10px] font-mono text-[#6C8798]">Select Test Automation Rule</label>
              <select
                value={selectedSimRuleId}
                onChange={e => setSelectedSimRuleId(e.target.value)}
                className="w-full bg-[#0B1018] border border-[#183040] p-1.5 rounded-[4px] font-mono text-[#E6F7FF] focus:outline-none"
              >
                <option value="">-- Choose rule --</option>
                {rules.map(r => <option key={r.id} value={r.id}>{r.name} ({r.id})</option>)}
              </select>
            </div>

            <button
              onClick={handleSimulateDeploy}
              disabled={simRunning}
              className="w-full mt-2.5 py-1.5 bg-green-500 hover:bg-green-600 text-white font-mono rounded-[4px] flex items-center justify-center gap-1.5 font-bold cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed text-[11px]"
            >
              <Play className="w-3.5 h-3.5" /> run live rule test
            </button>
          </div>

          {/* Live test output */}
          <div className="flex-1 bg-black p-3.5 font-mono text-[10px] text-[#A7C7D9] rounded-[4px] border border-[#183040] overflow-y-auto leading-relaxed scrollbar-thin max-h-[220px]">
            <div className="text-green-400 font-bold uppercase text-[9px] border-b border-[#183040] pb-1 mb-1.5 flex justify-between">
              <span>LIVE RULE TEST OUTPUT</span>
              <span>STATE</span>
            </div>
            {simLog.map((log, i) => (
              <div key={i} className="flex gap-1 items-start">
                <ChevronRight className="w-3 h-3 text-[#3E5360] mt-0.5 shrink-0" />
                <span className="break-all whitespace-pre-wrap">{log}</span>
              </div>
            ))}
            {simLog.length === 0 && (
              <p className="text-[#3E5360] italic text-center py-10">Choose targets for evaluation. The core demands input.</p>
            )}
            {simRunning && (
              <p className="text-[#fca5a5] font-bold animate-pulse mt-1">█ evaluating live rule logic...</p>
            )}
          </div>

        </div>

      </div>

    </div>
  );
}
export {};
