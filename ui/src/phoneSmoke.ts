// The phone smoke test (cross-platform Phase 4). WebDriver can't reach a
// phone's webview, so a smoke build (VITE_PHONE_SMOKE=1) drives its own UI:
// it checks the capability report hides the desktop-only features, sends one
// chat to the mock model and waits for the reply, then reports through
// `phone_smoke_report`. ui/e2e/phone.mjs reads the report off the device.
import { invoke } from "@tauri-apps/api/core";
import { getCapabilities } from "./api";

export const PHONE_PROMPT = "phone smoke: say hello";
export const PHONE_REPLY = "Hello from the phone smoke test";

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));

async function waitFor<T>(what: string, fn: () => T | null | undefined, ms = 60000): Promise<T> {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    const value = fn();
    if (value) return value;
    await sleep(250);
  }
  throw new Error(`timed out waiting for ${what}`);
}

function tabs(): string[] {
  return [...document.querySelectorAll("nav.tabs button")].map((b) => b.textContent ?? "");
}

/** What the chat shows, for a failed run's report. */
function chatState(): string {
  const alert = document.querySelector("[role='alert']")?.textContent ?? "none";
  const bubbles = [...document.querySelectorAll(".bubble")].map(
    (b) => `${b.className}: ${(b.textContent ?? "").slice(0, 80)}`,
  );
  return `alert=${alert} bubbles=[${bubbles.join(" | ")}]`;
}

/** Sets a React-controlled textarea the way typing would. */
function type(box: HTMLTextAreaElement, text: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
  setter?.call(box, text);
  box.dispatchEvent(new Event("input", { bubbles: true }));
}

export async function phoneSmoke(): Promise<string> {
  const caps = await getCapabilities();
  if (caps.desktopControl || caps.pulse || caps.localAi || caps.tray) {
    throw new Error(`a phone got desktop features: ${JSON.stringify(caps)}`);
  }
  const box = await waitFor("the message box", () =>
    document.querySelector<HTMLTextAreaElement>("textarea[aria-label='Message']"),
  );
  // The tabs settle once App has the report.
  await sleep(1000);
  if (tabs().includes("Pulse")) throw new Error(`Pulse is shown: ${tabs().join(", ")}`);
  type(box, PHONE_PROMPT);
  const send = await waitFor("the Send button", () =>
    document.querySelector<HTMLButtonElement>("form.composer button[type='submit']:not(:disabled)"),
  );
  send.click();
  // A cold simulator can take minutes over its first request.
  const reply = await waitFor(
    "the mocked reply",
    () => {
      const el = document.querySelector(".bubble.assistant:not(.streaming)");
      return el?.textContent === PHONE_REPLY ? el.textContent : null;
    },
    180000,
  ).catch((err: unknown) => {
    throw new Error(`${String(err)}; ${chatState()}`);
  });
  return `ok tabs=${tabs().join(",")} reply=${reply}`;
}

export function runPhoneSmoke() {
  phoneSmoke().then(
    (text) => invoke("phone_smoke_report", { text }),
    (err) => invoke("phone_smoke_report", { text: `fail ${String(err)}` }),
  );
}
