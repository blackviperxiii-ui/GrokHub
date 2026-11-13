// The router's budget card over the chat box: 80% of the week, the ask at
// 100% and the upgrade nudge. Click only; nothing answers it on its own.
import { call } from "./transport";

export interface BudgetCardData {
  key: "eighty" | "ask" | "nudge";
  eyebrow: string;
  title: string;
  note: string;
  primary: string;
  secondary: string;
}

export interface BudgetAnswered {
  status: string;
  /** Change cap: Settings opens. */
  openAiModels: boolean;
  /** The nudge's Details, shown in place. */
  details: string | null;
}

export function budgetCard(): Promise<BudgetCardData | null> {
  return call<BudgetCardData | null>("budget_card");
}

export function answerBudgetCard(key: BudgetCardData["key"], primary: boolean): Promise<BudgetAnswered> {
  return call<BudgetAnswered>("answer_budget_card", { key, primary });
}
