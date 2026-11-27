import { useEffect, useRef, useState, type FormEvent } from "react";
import { getQueue, onTasksChanged, steerTask, stopTask, type Queue, type QueueTask } from "./api";

/** Helpers of this chat that still run: a task with a model is a helper. */
function liveHelpers(tasks: QueueTask[]): QueueTask[] {
  return tasks.filter((task) => task.live && task.model !== undefined);
}

function progressLine(task: QueueTask): string {
  const p = task.progress;
  if (!p) return "";
  const steps = p.steps === 1 ? "1 step" : `${p.steps} steps`;
  return p.lastStep ? `${steps}, last ${p.lastStep}` : steps;
}

/**
 * The chat's running helpers, above the composer, each with Steer and Stop.
 * Reads the queue when the chat's tasks change, so it adds no polling, and
 * shows nothing while no helper runs. Desktop only: a browser can't steer.
 */
export function HelperStrip({ conversationId }: { conversationId: string }) {
  // Rows are kept with the chat they were read for, so a switch to another
  // chat shows nothing until that chat's queue is read.
  const [read, setRead] = useState<{ chat: string; helpers: QueueTask[] }>({ chat: "", helpers: [] });
  const [open, setOpen] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const asked = useRef(0);
  const helpers = read.chat === conversationId ? read.helpers : [];

  useEffect(() => {
    let dropped = false;
    let stop: (() => void) | undefined;
    function load() {
      const mine = ++asked.current;
      getQueue(conversationId).then(
        (queue) => {
          if (!dropped && mine === asked.current) {
            setRead({ chat: conversationId, helpers: liveHelpers(queue.tasks) });
          }
        },
        () => {},
      );
    }
    onTasksChanged((changed) => {
      if (changed === conversationId) load();
    }).then(
      (unlisten) => {
        if (dropped) unlisten();
        else stop = unlisten;
        load();
      },
      () => load(),
    );
    return () => {
      dropped = true;
      stop?.();
    };
  }, [conversationId]);

  function show(queue: Queue) {
    setRead({ chat: conversationId, helpers: liveHelpers(queue.tasks) });
  }

  if (helpers.length === 0) return null;

  function steer(event: FormEvent, id: string) {
    event.preventDefault();
    const words = text.trim();
    if (!words) return;
    steerTask(conversationId, id, words).then(
      (queue) => {
        setText("");
        setOpen(null);
        setNote(null);
        show(queue);
      },
      (err) => setNote(String(err)),
    );
  }

  function halt(id: string) {
    stopTask(conversationId, id).then(
      (queue) => show(queue),
      (err) => setNote(String(err)),
    );
  }

  return (
    <section className="helper-strip" aria-label="Running helpers">
      <ul>
        {helpers.map((task) => {
          const line = progressLine(task);
          return (
            <li key={task.id} className="helper-strip-row">
              <span className="helper-strip-title">{task.title}</span>
              {task.model && <span className="helper-strip-model">{task.model}</span>}
              {line && <span className="helper-strip-progress">{line}</span>}
              <button
                type="button"
                aria-expanded={open === task.id}
                aria-label={`Steer ${task.title}`}
                onClick={() => {
                  setOpen(open === task.id ? null : task.id);
                  setText("");
                }}
              >
                Steer
              </button>
              <button type="button" aria-label={`Stop ${task.title}`} onClick={() => halt(task.id)}>
                Stop
              </button>
              {open === task.id && (
                <form className="helper-strip-steer" onSubmit={(event) => steer(event, task.id)}>
                  <input
                    autoFocus
                    aria-label={`What ${task.title} should do next`}
                    placeholder="Tell this helper what to do next"
                    value={text}
                    onChange={(event) => setText(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Escape") setOpen(null);
                    }}
                  />
                </form>
              )}
            </li>
          );
        })}
      </ul>
      {note && (
        <p className="helper-strip-note" role="status">
          {note}
        </p>
      )}
    </section>
  );
}
