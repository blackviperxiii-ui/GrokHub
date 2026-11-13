import { useEffect, useState } from "react";
import { answerBudgetCard, budgetCard, type BudgetCardData } from "./spendApi";

/** How often an idle chat looks for a new budget card. */
const LOOK_MS = 60_000;

/** The budget card over the chat box. It looks when the chat goes idle and
 * once a minute; a chat that paused at 100% raises the ask at its next look. */
export function BudgetCard({ busy, onOpenSettings }: { busy: boolean; onOpenSettings: () => void }) {
  const [card, setCard] = useState<BudgetCardData | null>(null);
  const [said, setSaid] = useState<string | null>(null);
  const [details, setDetails] = useState<string | null>(null);
  const [pressing, setPressing] = useState(false);

  useEffect(() => {
    if (busy) return;
    let dropped = false;
    const look = () =>
      budgetCard()
        .then((next) => {
          if (!dropped && next) {
            setCard(next);
            setSaid(null);
            setDetails(null);
          }
        })
        .catch(() => {});
    void look();
    const timer = setInterval(() => void look(), LOOK_MS);
    return () => {
      dropped = true;
      clearInterval(timer);
    };
  }, [busy]);

  async function press(primary: boolean) {
    if (!card) return;
    setPressing(true);
    try {
      const done = await answerBudgetCard(card.key, primary);
      setCard(null);
      if (done.openAiModels) onOpenSettings();
      setDetails(done.details);
      setSaid(done.status || null);
    } catch (err) {
      setCard(null);
      setSaid(String(err));
    } finally {
      setPressing(false);
    }
  }

  if (details) {
    return (
      <div className="goal-ask budget-card" role="note" aria-label="Plan details">
        <span>{details}</span>
        <button type="button" className="quiet" onClick={() => setDetails(null)}>
          Close
        </button>
      </div>
    );
  }
  if (said) {
    return (
      <p className="goal-ask home-muted" role="status">
        {said}
      </p>
    );
  }
  if (!card) return null;
  return (
    <div className="goal-ask budget-card" role="group" aria-label={card.eyebrow}>
      <span>
        <small className="home-muted">{card.eyebrow}</small> <strong>{card.title}</strong>{" "}
        <small className="home-muted">{card.note}</small>
      </span>
      <button type="button" disabled={pressing} onClick={() => void press(true)}>
        {card.primary}
      </button>
      <button type="button" className="quiet" disabled={pressing} onClick={() => void press(false)}>
        {card.secondary}
      </button>
    </div>
  );
}
