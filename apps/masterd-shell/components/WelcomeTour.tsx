"use client";

import React from "react";
import { BookOpen, ChevronLeft, ChevronRight, Sparkles, X } from "lucide-react";

type WelcomeTourProps = {
  open: boolean;
  step: number;
  totalSteps: number;
  onClose: () => void;
  onNext: () => void;
  onBack: () => void;
};

const steps = [
  {
    title: "Start with intake",
    body: "Drag files into the intake area or add a watch folder to let MASTERd pick up new documents automatically.",
  },
  {
    title: "Review the workspace",
    body: "Use Documents, Review, and Audit to inspect classifications, rename suggestions, and action history.",
  },
  {
    title: "Tune the assistant",
    body: "Use Chat for RAG-powered questions and Settings to adjust retrieval, embeddings, and fallback behavior.",
  },
  {
    title: "You’re ready",
    body: "The app now starts from a clean, focused workspace. Reopen this guide any time from the header.",
  },
];

export default function WelcomeTour({
  open,
  step,
  totalSteps,
  onClose,
  onNext,
  onBack,
}: WelcomeTourProps) {
  if (!open) return null;

  const current = steps[Math.min(step, steps.length - 1)];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm p-4">
      <div className="w-full max-w-xl rounded-[18px] border border-[#3f3f46] bg-[#09090b] shadow-[0_24px_80px_rgba(0,0,0,0.55)]">
        <div className="flex items-center justify-between border-b border-[#27272a] px-5 py-4">
          <div className="flex items-center gap-2 text-[#f5f5f5]">
            <Sparkles className="h-4 w-4 text-[#b91c1c]" />
            <span className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#e4e4e7]">
              Welcome to MASTERd
            </span>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded-full border border-[#27272a] p-1.5 text-[#a1a1aa] hover:text-[#f5f5f5]"
            aria-label="Close welcome tour"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="space-y-5 px-5 py-6">
          <div className="flex items-center gap-2 text-[10px] uppercase tracking-[0.28em] text-[#71717a]">
            <BookOpen className="h-3.5 w-3.5 text-[#b91c1c]" />
            <span>
              Step {Math.min(step, totalSteps - 1) + 1} of {totalSteps}
            </span>
          </div>

          <div className="space-y-2">
            <h2 className="text-xl font-semibold text-[#f5f5f5]">{current.title}</h2>
            <p className="max-w-prose text-sm leading-6 text-[#a1a1aa]">{current.body}</p>
          </div>

          <div className="grid gap-2 sm:grid-cols-4">
            {steps.map((item, idx) => (
              <div
                key={item.title}
                className={`rounded-[14px] border px-3 py-3 text-xs ${
                  idx === step
                    ? "border-[#b91c1c]/60 bg-[#7f1d1d]/20 text-[#f5f5f5]"
                    : "border-[#27272a] bg-[#111113] text-[#a1a1aa]"
                }`}
              >
                <div className="mb-1 text-[10px] uppercase tracking-[0.22em] text-[#71717a]">
                  {idx + 1}
                </div>
                <div className="font-medium">{item.title}</div>
              </div>
            ))}
          </div>
        </div>

        <div className="flex items-center justify-between border-t border-[#27272a] px-5 py-4">
          <button
            type="button"
            onClick={onBack}
            disabled={step === 0}
            className="inline-flex items-center gap-1 rounded-[10px] border border-[#27272a] px-3 py-2 text-xs text-[#a1a1aa] disabled:opacity-40"
          >
            <ChevronLeft className="h-4 w-4" />
            Back
          </button>

          <div className="flex items-center gap-2 text-[10px] uppercase tracking-[0.22em] text-[#71717a]">
            <span className="rounded-full bg-[#7f1d1d]/20 px-2 py-1 text-[#fca5a5]">
              Fresh start
            </span>
          </div>

          {step >= totalSteps - 1 ? (
            <button
              type="button"
              onClick={onClose}
              className="inline-flex items-center gap-1 rounded-[10px] bg-[#b91c1c] px-3 py-2 text-xs font-semibold text-white hover:bg-[#991b1b]"
            >
              Finish
              <ChevronRight className="h-4 w-4" />
            </button>
          ) : (
            <button
              type="button"
              onClick={onNext}
              className="inline-flex items-center gap-1 rounded-[10px] bg-[#b91c1c] px-3 py-2 text-xs font-semibold text-white hover:bg-[#991b1b]"
            >
              Next
              <ChevronRight className="h-4 w-4" />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
