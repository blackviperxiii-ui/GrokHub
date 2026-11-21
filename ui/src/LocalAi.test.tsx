import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  LocalAiConfig,
  LocalAiView,
  LocalDrafterEvent,
  LocalEngineEvent,
  LocalFileRow,
  LocalModelRow,
  LocalStatus,
} from "./api";

const api = vi.hoisted(() => ({
  getLocalAi: vi.fn<() => Promise<LocalAiView>>(),
  saveLocalAi: vi.fn<(config: LocalAiConfig) => Promise<LocalAiView>>(),
  resetLocalAi: vi.fn<() => Promise<LocalAiView>>(),
  checkModelFile: vi.fn<(path: string) => Promise<LocalFileRow>>(),
  runLocalSpeedTest: vi.fn<() => Promise<string>>(),
  setLocalKeepLoaded: vi.fn<(on: boolean) => Promise<LocalAiView>>(),
  setLocalDynamic: vi.fn<(on: boolean) => Promise<LocalAiView>>(),
  setPromptTidy: vi.fn<(on: boolean) => Promise<LocalAiView>>(),
  setLocalUse: vi.fn<(on: boolean) => Promise<LocalAiView>>(),
  unloadLocalModel: vi.fn<() => Promise<LocalAiView>>(),
  startLocalEngineDownload: vi.fn(),
  cancelLocalEngineDownload: vi.fn(),
  startLocalDrafterDownload: vi.fn(),
  cancelLocalDrafterDownload: vi.fn(),
  onLocalEngineEvent: vi.fn(async (cb: (event: LocalEngineEvent) => void) => {
    engineEvents.push(cb);
    return () => undefined;
  }),
  onLocalDrafterEvent: vi.fn(async (cb: (event: LocalDrafterEvent) => void) => {
    drafterEvents.push(cb);
    return () => undefined;
  }),
}));

/** The listeners the panel registered, so a test can play a backend event. */
const engineEvents = vi.hoisted(() => [] as ((event: LocalEngineEvent) => void)[]);
const drafterEvents = vi.hoisted(() => [] as ((event: LocalDrafterEvent) => void)[]);

function engineEvent(event: LocalEngineEvent) {
  act(() => {
    for (const cb of engineEvents) cb(event);
  });
}

function drafterEvent(event: LocalDrafterEvent) {
  act(() => {
    for (const cb of drafterEvents) cb(event);
  });
}

vi.mock("./api", () => api);
// Download models and voice routing have their own tests.
vi.mock("./ModelLibrary", () => ({ ModelLibrary: () => null }));
vi.mock("./VoiceRouteSettings", () => ({ VoiceRouteSettings: () => null }));

import { ABLITERATED_NOTICE, LocalAi, MODEL_NOTICE } from "./LocalAi";

// Catalog ids as `local_setup` names them (every build is abliterated); Muse
// stands in for an optional model the catalog no longer carries.
const IDS: Record<string, string> = {
  "Gemma 4 31B": "gemma-4-31b-it-abliterated-q4_k",
  "Muse Glimmer 30B": "muse-glimmer-30b-q4_k_m",
  "Gemma 4 26B-A4B": "gemma-4-26b-a4b-it-abliterated-q4_k",
  "Gemma 4 12B": "gemma-4-12b-it-abliterated-q4_k",
  "Gemma 4 E4B": "gemma-4-e4b-it-abliterated-q4_k",
  "Gemma 4 E2B": "gemma-4-e2b-it-abliterated-q4_k",
};

/** A Gemma 4 E2B drafter offer for the 12B target, as the backend words it. */
const GEMMA_OFFER = {
  id: "gemma-4-e2b-it-abliterated-q4_k",
  name: "Gemma 4 E2B",
  sizeLabel: "3.2 GB",
  line: "Optional: Gemma 4 E2B can draft replies for Gemma 4 12B, which can make them faster on some machines. Loadout only downloads it if you ask, and keeps it only if the speed test finds it helps.",
  button: "Install and run draft model (3.2 GB)",
  targetId: "gemma-4-12b-it-abliterated-q4_k",
  targetName: "Gemma 4 12B",
};

const GEMMA_PROGRESS = {
  id: "gemma-4-e2b-it-abliterated-q4_k",
  done: 1_200_000_000,
  total: 3_200_000_000,
  verifying: false,
  line: "Downloading Gemma 4 E2B: 1.2 of 3.2 GB",
};

function model(
  name: string,
  bucket: LocalModelRow["bucket"],
  reason: string,
  over: Partial<LocalModelRow> = {},
): LocalModelRow {
  return {
    id: IDS[name],
    name,
    size: "5 GB",
    quant: "Q4_K_M",
    ctx: 8192,
    tokS: "about 40 tok/s",
    license: "Apache-2.0",
    badges: [],
    bucket,
    reason,
    preselected: false,
    optional: name === "Muse Glimmer 30B",
    ...over,
  };
}

function config(over: Partial<LocalAiConfig> = {}): LocalAiConfig {
  return {
    pauseForGames: true,
    useIdleTime: true,
    throttleOnBattery: true,
    manual: {
      ngl: null,
      ctx: null,
      threads: null,
      kv: "auto",
      idleMinutes: 10,
      neverUnload: false,
      allowApps: [],
      denyApps: [],
    },
    ownFiles: [],
    noticeSeen: [],
    ...over,
  };
}

function missingEngine(): LocalAiView["engine"] {
  return {
    state: "missing",
    version: "b11602",
    sizeLabel: "32 MB",
    hint: "llama.cpp b11602 · 32 MB · from github.com",
    button: "Get engine b11602 (32 MB)",
    progress: null,
    error: null,
  };
}

const LOCAL_ON =
  "On. While the model is loaded, small background jobs run on this computer, and chats use it when you're offline or the cloud can't answer.";
const LOCAL_OFF = "Off. Nothing runs on the on-device model.";
const TIDY_LOCAL_OFF = "Off while Local AI is off. Turn on Use Local AI on this computer first.";

const READY: LocalAiView["engine"] = {
  state: "ready",
  version: "b11602",
  sizeLabel: "",
  hint: "llama-server is ready at /home/user/.config/Loadout/bin/llama-server",
  button: "",
  progress: null,
  error: null,
};

function view(
  models: LocalModelRow[],
  status: Partial<LocalStatus>,
  over: Partial<LocalAiView> = {},
): LocalAiView {
  const pre = models.find((m) => m.preselected);
  return {
    config: config(),
    status: {
      tier: "T2",
      model: pre?.name ?? "",
      state: "Normal",
      reason: "normal use",
      tokS: "about 40 tok/s",
      memory: "8 GB of GPU memory",
      ...status,
    },
    picker: { models, onThisPc: [], selected: pre?.id ?? "" },
    limits: [],
    maxThreads: 16,
    engine: missingEngine(),
    draftOffer: null,
    draftOffers: [],
    draftProgress: null,
    running: {
      lines: ["Running now: nothing. The model loads with the next reply."],
      loaded: false,
      keptLoaded: false,
      dynamic: true,
      dynamicHint: "On: each task runs on the model on disk that's best at it.",
    },
    localUse: { on: true, hint: LOCAL_ON },
    tidy: { on: false, available: true, hint: "Off. Your messages go to the model exactly as you type them." },
    ...over,
  };
}

/** The five machines the card names, as the shell scores them. */
const PROFILES: Record<string, LocalAiView> = {
  "T1 laptop": view(
    [
      model("Gemma 4 E2B", "recommended", "Fits in 4 GB of spare RAM, about 7 tok/s.", {
        preselected: true,
      }),
      model(
        "Gemma 4 E4B",
        "maySlow",
        "Needs 6 GB of spare RAM; this PC has 4 GB, so it may be slow.",
      ),
      model("Gemma 4 31B", "hidden", "Needs 20.6 GB of memory; this PC has 6 GB."),
      model("Muse Glimmer 30B", "hidden", "Needs 18.3 GB of memory; this PC has 6 GB."),
      model("Gemma 4 26B-A4B", "hidden", "Needs 17 GB of memory; this PC has 6 GB."),
      model("Gemma 4 12B", "hidden", "Needs 8.2 GB of memory; this PC has 6 GB."),
    ],
    { tier: "T1", memory: "4 GB of spare RAM" },
  ),
  "8 GB GPU": view(
    [
      model("Gemma 4 E4B", "recommended", "Fits in 8 GB of GPU memory, about 40 tok/s.", {
        preselected: true,
      }),
      model("Gemma 4 E2B", "recommended", "Fits in 8 GB of GPU memory, about 60 tok/s."),
      model(
        "Gemma 4 12B",
        "maySlow",
        "Needs 10 GB of GPU memory; this PC has 8 GB, so part of it runs on the CPU and it may be slow.",
      ),
    ],
    { tier: "T2" },
  ),
  "16 GB GPU": view(
    [
      model(
        "Gemma 4 26B-A4B",
        "recommended",
        "Fits in 15.9 GB of GPU memory, about 60 tok/s.",
        { preselected: true },
      ),
      model("Gemma 4 12B", "recommended", "Fits in 15.9 GB of GPU memory, about 45 tok/s."),
      model(
        "Muse Glimmer 30B",
        "maySlow",
        "Needs 22 GB of GPU memory; this PC has 15.9 GB, so part of it runs on the CPU and it may be slow.",
      ),
    ],
    { tier: "T3", memory: "15.9 GB of GPU memory" },
  ),
  "24 GB GPU": view(
    [
      model("Gemma 4 31B", "recommended", "Fits in 24 GB of GPU memory, about 35 tok/s.", {
        preselected: true,
      }),
      model(
        "Muse Glimmer 30B",
        "recommended",
        "Fits in 24 GB of GPU memory, about 40 tok/s. Optional: GrokHub never picks it for you.",
      ),
    ],
    { tier: "T4", memory: "24 GB of GPU memory" },
  ),
  "Apple 16 GB": view(
    [
      model(
        "Gemma 4 E4B",
        "recommended",
        "Fits in 11.2 GB of unified memory, about 30 tok/s.",
        { preselected: true },
      ),
      model("Gemma 4 E2B", "recommended", "Fits in 11.2 GB of unified memory, about 35 tok/s."),
      model(
        "Gemma 4 12B",
        "maySlow",
        "Needs 14 GB of unified memory; this PC has 11.2 GB, so it may be slow.",
      ),
      model("Gemma 4 31B", "hidden", "Needs 20.6 GB of memory; this PC has 14 GB."),
    ],
    { tier: "T2", memory: "11.2 GB of unified memory" },
  ),
};

async function openAdvanced() {
  await userEvent.click(await screen.findByText("Advanced"));
}

/** The names in the Added model files list, or [] when it isn't shown. */
function added(): string[] {
  const list = screen.queryByRole("list", { name: "Added model files" });
  if (!list) return [];
  return within(list)
    .getAllByRole("listitem")
    .map((li) => li.getAttribute("aria-label") ?? "");
}

describe("LocalAi", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    engineEvents.length = 0;
    drafterEvents.length = 0;
  });

  // Auto picks on merit; nothing on the page picks a model, a tier or a mode.
  it.each([
    ["T1 laptop", "Gemma 4 E2B"],
    ["8 GB GPU", "Gemma 4 E4B"],
    ["16 GB GPU", "Gemma 4 26B-A4B"],
    ["24 GB GPU", "Gemma 4 31B"],
    ["Apple 16 GB", "Gemma 4 E4B"],
  ] as const)("%s shows Auto's pick, %s, with no model picker", async (profile, pick) => {
    const v = PROFILES[profile];
    api.getLocalAi.mockResolvedValue(v);
    render(<LocalAi />);
    await openAdvanced();
    expect(document.querySelector(".local-status")).toHaveTextContent(` · ${pick} · `);
    expect(screen.queryAllByRole("radio")).toEqual([]);
    expect(screen.queryByRole("button", { name: "Manual" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Auto" })).toBeNull();
    expect(screen.queryByRole("group", { name: "Local AI mode" })).toBeNull();
    expect(screen.queryByText("Show all")).toBeNull();
    for (const row of v.picker.models) {
      expect(screen.queryByRole("listitem", { name: row.name })).toBeNull();
    }
    const options = screen.queryAllByRole("option").map((o) => o.textContent ?? "");
    expect(options.filter((o) => o in IDS || /gemma|model|tier/i.test(o))).toEqual([]);
    expect(
      screen.getByText("Auto picks the model for this PC. These settings only change how it runs."),
    ).toBeInTheDocument();
  });

  it("shows what runs now and keeps it loaded or unloads it on a press", async () => {
    const v = PROFILES["16 GB GPU"];
    const running = {
      lines: [
        "Running now: Gemma 4 12B (Q4_K), loaded on the GPU, 6.9 GB.",
        "Unloads after 4 minutes without replies.",
      ],
      loaded: true,
      keptLoaded: false,
      dynamic: true,
      dynamicHint: "On: each task runs on the model on disk that's best at it.",
    };
    api.getLocalAi.mockResolvedValue({ ...v, running });
    api.setLocalKeepLoaded.mockResolvedValue({ ...v, running: { ...running, keptLoaded: true } });
    api.unloadLocalModel.mockResolvedValue(v);
    render(<LocalAi />);
    const rows = await screen.findByLabelText("Running now");
    expect(rows).toHaveTextContent("Running now: Gemma 4 12B (Q4_K), loaded on the GPU, 6.9 GB.");
    await userEvent.click(within(rows).getByRole("button", { name: "Keep loaded" }));
    expect(api.setLocalKeepLoaded).toHaveBeenCalledWith(true);
    expect(await within(rows).findByRole("button", { name: "Let it unload" })).toBeInTheDocument();
    await userEvent.click(within(rows).getByRole("button", { name: "Unload now" }));
    expect(api.unloadLocalModel).toHaveBeenCalled();
    expect(await screen.findByText("Unloaded the model. It loads again with the next reply.")).toBeInTheDocument();
    expect(within(rows).queryByRole("button", { name: "Unload now" })).toBeNull();
  });

  it("unloads without promising a reload while Local AI is off", async () => {
    const off = {
      ...PROFILES["16 GB GPU"],
      localUse: { on: false, hint: LOCAL_OFF },
      tidy: { on: false, available: false, hint: TIDY_LOCAL_OFF },
    };
    api.getLocalAi.mockResolvedValue({
      ...off,
      running: {
        lines: ["Running now: Gemma 4 12B (Q4_K), loaded on the GPU, 6.9 GB."],
        loaded: true,
        keptLoaded: false,
        dynamic: false,
        dynamicHint: "Off: your pick answers every task.",
      },
    });
    api.unloadLocalModel.mockResolvedValue(off);
    render(<LocalAi />);
    const rows = await screen.findByLabelText("Running now");
    await userEvent.click(within(rows).getByRole("button", { name: "Unload now" }));
    expect(api.unloadLocalModel).toHaveBeenCalled();
    expect(await screen.findByText("Unloaded the model.")).toBeInTheDocument();
    expect(screen.queryByText("Unloaded the model. It loads again with the next reply.")).toBeNull();
  });

  it("turns Dynamic off and on from the running rows", async () => {
    const v = PROFILES["16 GB GPU"];
    const running = {
      lines: ["Running now: nothing. The model loads with the next reply."],
      loaded: false,
      keptLoaded: false,
      dynamic: true,
      dynamicHint: "On: each task runs on the model on disk that's best at it.",
    };
    api.getLocalAi.mockResolvedValue({ ...v, running });
    api.setLocalDynamic.mockResolvedValue({
      ...v,
      running: { ...running, dynamic: false, dynamicHint: "Off: your pick answers every task." },
    });
    render(<LocalAi />);
    const rows = await screen.findByLabelText("Running now");
    expect(rows).toHaveTextContent("best at it");
    await userEvent.click(within(rows).getByRole("button", { name: "Turn off Dynamic" }));
    expect(api.setLocalDynamic).toHaveBeenCalledWith(false);
    expect(await within(rows).findByRole("button", { name: "Turn on Dynamic" })).toBeInTheDocument();
    expect(rows).toHaveTextContent("Off: your pick answers every task.");
  });

  it("turns Local AI off at once and pauses the tidy, without saving the page", async () => {
    const v = PROFILES["16 GB GPU"];
    api.getLocalAi.mockResolvedValue(v);
    api.setLocalUse.mockResolvedValue({
      ...v,
      localUse: { on: false, hint: LOCAL_OFF },
      tidy: { on: false, available: false, hint: TIDY_LOCAL_OFF },
    });
    render(<LocalAi />);
    const group = await screen.findByRole("group", { name: "Use Local AI" });
    const sw = within(group).getByRole("switch", { name: "Use Local AI on this computer" });
    expect(sw).toHaveAttribute("aria-checked", "true");
    expect(group).toHaveTextContent(LOCAL_ON);
    await userEvent.click(sw);
    expect(api.setLocalUse).toHaveBeenCalledWith(false);
    await waitFor(() => expect(sw).toHaveAttribute("aria-checked", "false"));
    expect(group).toHaveTextContent(LOCAL_OFF);
    expect(await screen.findByText("Local AI is off: nothing runs on the on-device model.")).toBeInTheDocument();
    const tidy = screen.getByRole("group", { name: "Typo tidy" });
    expect(within(tidy).getByRole("switch")).toBeDisabled();
    expect(tidy).toHaveTextContent(TIDY_LOCAL_OFF);
    expect(api.saveLocalAi).not.toHaveBeenCalled();
  });

  it("turns Local AI back on after an older version switched it off", async () => {
    const off = {
      ...PROFILES["16 GB GPU"],
      localUse: { on: false, hint: LOCAL_OFF },
      tidy: { on: false, available: false, hint: TIDY_LOCAL_OFF },
    };
    api.getLocalAi.mockResolvedValue(off);
    api.setLocalUse.mockResolvedValue(PROFILES["16 GB GPU"]);
    render(<LocalAi />);
    const group = await screen.findByRole("group", { name: "Use Local AI" });
    const sw = within(group).getByRole("switch", { name: "Use Local AI on this computer" });
    expect(sw).toHaveAttribute("aria-checked", "false");
    await userEvent.click(sw);
    expect(api.setLocalUse).toHaveBeenCalledWith(true);
    await waitFor(() => expect(sw).toHaveAttribute("aria-checked", "true"));
    expect(await screen.findByText("Local AI is on for this computer.")).toBeInTheDocument();
    expect(within(screen.getByRole("group", { name: "Typo tidy" })).getByRole("switch")).toBeEnabled();
  });

  it("says why Local AI didn't change", async () => {
    api.getLocalAi.mockResolvedValue(PROFILES["16 GB GPU"]);
    api.setLocalUse.mockRejectedValue("app.json is not valid JSON");
    render(<LocalAi />);
    const group = await screen.findByRole("group", { name: "Use Local AI" });
    await userEvent.click(within(group).getByRole("switch"));
    expect(await screen.findByText("app.json is not valid JSON")).toBeInTheDocument();
    expect(within(group).getByRole("switch")).toHaveAttribute("aria-checked", "true");
  });

  it("turns typo tidy on at once, without saving the page", async () => {
    const v = PROFILES["16 GB GPU"];
    api.getLocalAi.mockResolvedValue(v);
    const on =
      "On. While the on-device model is loaded, it fixes typos first (up to 0.3 s); Keep loaded tidies every message. Your chat keeps what you typed; numbers, names, paths and links are never changed.";
    api.setPromptTidy.mockResolvedValue({ ...v, tidy: { on: true, available: true, hint: on } });
    render(<LocalAi />);
    const group = await screen.findByRole("group", { name: "Typo tidy" });
    const sw = within(group).getByRole("switch", { name: "Tidy typos on this computer (can change wording)" });
    expect(sw).toHaveAttribute("aria-checked", "false");
    expect(group).toHaveTextContent("Off. Your messages go to the model exactly as you type them.");
    await userEvent.click(sw);
    expect(api.setPromptTidy).toHaveBeenCalledWith(true);
    await waitFor(() => expect(sw).toHaveAttribute("aria-checked", "true"));
    expect(group).toHaveTextContent(on);
    expect(await screen.findByText("Typo tidy is on: the on-device model fixes typos while it's loaded.")).toBeInTheDocument();
    expect(api.saveLocalAi).not.toHaveBeenCalled();
  });

  it("keeps typo tidy off until a local model is set up", async () => {
    api.getLocalAi.mockResolvedValue({
      ...PROFILES["16 GB GPU"],
      tidy: { on: false, available: false, hint: "Off. Set up a local engine and model first." },
    });
    render(<LocalAi />);
    const group = await screen.findByRole("group", { name: "Typo tidy" });
    const sw = within(group).getByRole("switch", { name: "Tidy typos on this computer (can change wording)" });
    expect(sw).toBeDisabled();
    await userEvent.click(sw);
    expect(api.setPromptTidy).not.toHaveBeenCalled();
    expect(group).toHaveTextContent("Off. Set up a local engine and model first.");
  });

  it("lets a paused typo tidy be turned off without a model", async () => {
    const paused = { on: true, available: false, hint: "On, but paused. Set up a local engine and model first." };
    const v = { ...PROFILES["16 GB GPU"], tidy: paused };
    api.getLocalAi.mockResolvedValue(v);
    api.setPromptTidy.mockResolvedValue({
      ...v,
      tidy: { on: false, available: false, hint: "Off. Set up a local engine and model first." },
    });
    render(<LocalAi />);
    const group = await screen.findByRole("group", { name: "Typo tidy" });
    const sw = within(group).getByRole("switch", { name: "Tidy typos on this computer (can change wording)" });
    expect(sw).toHaveAttribute("aria-checked", "true");
    expect(group).toHaveTextContent("On, but paused. Set up a local engine and model first.");
    await userEvent.click(sw);
    expect(api.setPromptTidy).toHaveBeenCalledWith(false);
    await waitFor(() => expect(sw).toHaveAttribute("aria-checked", "false"));
    expect(sw).toBeDisabled();
  });

  it("says why typo tidy didn't change", async () => {
    api.getLocalAi.mockResolvedValue(PROFILES["16 GB GPU"]);
    api.setPromptTidy.mockRejectedValue("Set up a local engine and model first.");
    render(<LocalAi />);
    const group = await screen.findByRole("group", { name: "Typo tidy" });
    await userEvent.click(within(group).getByRole("switch"));
    expect(await screen.findByText("Set up a local engine and model first.")).toBeInTheDocument();
    expect(within(group).getByRole("switch")).toHaveAttribute("aria-checked", "false");
  });

  it("shows the Auto status line and what detection couldn't see", async () => {
    api.getLocalAi.mockResolvedValue({
      ...PROFILES["16 GB GPU"],
      limits: ["On Wayland GrokHub can't see which app is fullscreen."],
    });
    render(<LocalAi />);
    expect(
      await screen.findByText(
        "T3 · Gemma 4 26B-A4B · Normal (normal use) · about 40 tok/s · 15.9 GB of GPU memory",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Detection is limited on this PC:")).toBeInTheDocument();
    expect(
      screen.getByText("On Wayland GrokHub can't see which app is fullscreen."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Recommended" })).toBeNull();
  });

  it("saves the Auto switches", async () => {
    const v = PROFILES["8 GB GPU"];
    api.getLocalAi.mockResolvedValue(v);
    api.saveLocalAi.mockResolvedValue(v);
    render(<LocalAi />);
    await userEvent.click(await screen.findByLabelText(/Pause while gaming/));
    await userEvent.click(screen.getByLabelText(/Slow down on battery/));
    await userEvent.click(screen.getByRole("button", { name: "Save local AI" }));
    expect(api.saveLocalAi).toHaveBeenCalledWith(
      config({ pauseForGames: false, throttleOnBattery: false }),
    );
    expect(await screen.findByText("Local AI saved.")).toBeInTheDocument();
  });

  it("a model file with safety training removed asks once before it is added", async () => {
    const v = PROFILES["8 GB GPU"];
    const path = "/models/Llama-3-8B-abliterated.Q4_K_M.gguf";
    const file: LocalFileRow = {
      path,
      name: "Llama-3-8B-abliterated.Q4_K_M.gguf",
      size: "4.6 GB",
      source: "Model file",
      thirdParty: true,
      abliterated: true,
      needsNotice: true,
    };
    api.getLocalAi.mockResolvedValue(v);
    api.checkModelFile.mockResolvedValue(file);
    api.saveLocalAi.mockResolvedValue(v);
    render(<LocalAi />);
    await openAdvanced();

    await userEvent.type(screen.getByLabelText("Model file path"), ` ${path} `);
    await userEvent.click(screen.getByRole("button", { name: "Add a model file" }));
    expect(api.checkModelFile).toHaveBeenCalledWith(path);
    const notice = await screen.findByRole("alertdialog", { name: "About this model" });
    expect(notice).toHaveTextContent(MODEL_NOTICE);
    // Cancel adds nothing; the file waits under On this PC.
    await userEvent.click(within(notice).getByRole("button", { name: "Cancel" }));
    expect(added()).toEqual([]);

    const row = within(screen.getByRole("list", { name: "On this PC" })).getByRole("listitem", {
      name: file.name,
    });
    expect(within(row).getByText("Safety training removed")).toBeInTheDocument();
    await userEvent.click(within(row).getByRole("button", { name: "Add" }));
    await userEvent.click(screen.getByRole("button", { name: "I understand" }));
    expect(added()).toEqual([file.name]);
    expect(screen.queryByRole("list", { name: "On this PC" })).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: "Save local AI" }));
    expect(api.saveLocalAi).toHaveBeenCalledWith(config({ ownFiles: [path], noticeSeen: [path] }));
  });

  it("a file whose notice was read is added without asking, and Remove takes it off", async () => {
    const path = "/models/qwen3-8b.gguf";
    const v = {
      ...PROFILES["8 GB GPU"],
      picker: {
        ...PROFILES["8 GB GPU"].picker,
        onThisPc: [
          {
            path,
            name: "qwen3-8b.gguf",
            size: "4.9 GB",
            source: "Ollama",
            thirdParty: true,
            abliterated: false,
            needsNotice: false,
          },
        ],
      },
    };
    api.getLocalAi.mockResolvedValue(v);
    api.saveLocalAi.mockResolvedValue(v);
    render(<LocalAi />);
    await openAdvanced();
    const row = screen.getByRole("listitem", { name: "qwen3-8b.gguf" });
    expect(row).toHaveTextContent("Ollama · 4.9 GB · /models/qwen3-8b.gguf");
    expect(within(row).getByText("Third-party")).toBeInTheDocument();
    await userEvent.click(within(row).getByRole("button", { name: "Add" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(added()).toEqual(["qwen3-8b.gguf"]);
    const kept = within(screen.getByRole("list", { name: "Added model files" })).getByRole(
      "listitem",
      { name: "qwen3-8b.gguf" },
    );
    await userEvent.click(within(kept).getByRole("button", { name: "Remove" }));
    expect(added()).toEqual([]);
    expect(screen.getByRole("listitem", { name: "qwen3-8b.gguf" })).toHaveTextContent("Add");
    await userEvent.click(screen.getByRole("button", { name: "Save local AI" }));
    expect(api.saveLocalAi).toHaveBeenCalledWith(config());
  });

  it("Advanced values save idle unload, never unload, and app lists by line", async () => {
    const v = PROFILES["16 GB GPU"];
    api.getLocalAi.mockResolvedValue(v);
    api.saveLocalAi.mockResolvedValue(v);
    render(<LocalAi />);
    await openAdvanced();
    expect(screen.queryByLabelText("Device")).toBeNull();
    expect(screen.queryByText(/Memory caps/)).toBeNull();
    await userEvent.type(screen.getByLabelText("GPU layers"), "20");
    await userEvent.type(screen.getByLabelText("Context"), "16384");
    await userEvent.selectOptions(screen.getByLabelText("KV cache"), "q4");
    const idle = screen.getByLabelText("Unload after idle (minutes)");
    await userEvent.tripleClick(idle);
    await userEvent.keyboard("3");
    await userEvent.click(screen.getByLabelText("Never unload the model"));
    await userEvent.type(
      screen.getByLabelText(/Never pause for these apps/),
      "vlc.exe\n\n mpv ",
    );
    await userEvent.type(screen.getByLabelText(/Always pause for these apps/), "blender");
    await userEvent.click(screen.getByRole("button", { name: "Save local AI" }));

    expect(api.saveLocalAi).toHaveBeenCalledWith(
      config({
        manual: {
          ngl: 20,
          ctx: 16384,
          threads: null,
          kv: "q4",
          idleMinutes: 3,
          neverUnload: true,
          allowApps: ["vlc.exe", "mpv"],
          denyApps: ["blender"],
        },
      }),
    );
  });

  it("keeps the speed test and Keep loaded off while Local AI is off", async () => {
    api.getLocalAi.mockResolvedValue({
      ...PROFILES["8 GB GPU"],
      localUse: { on: false, hint: LOCAL_OFF },
      tidy: { on: false, available: false, hint: TIDY_LOCAL_OFF },
    });
    render(<LocalAi />);
    await openAdvanced();
    expect(screen.getByRole("button", { name: "Re-run speed test" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Keep loaded" })).toBeDisabled();
  });

  it("re-runs the speed test and resets Advanced", async () => {
    const v = PROFILES["8 GB GPU"];
    api.getLocalAi.mockResolvedValue(v);
    api.runLocalSpeedTest.mockRejectedValue(
      "Set up a local engine and model first, then run the speed test.",
    );
    api.resetLocalAi.mockResolvedValue(v);
    render(<LocalAi />);
    await openAdvanced();
    await userEvent.click(screen.getByRole("button", { name: "Re-run speed test" }));
    const failed = await screen.findByText(
      "Set up a local engine and model first, then run the speed test.",
    );
    expect(failed).toHaveAttribute("role", "status");
    expect(failed).toHaveClass("note", "local-note", "error");
    await userEvent.click(screen.getByRole("button", { name: "Reset Advanced" }));
    expect(api.resetLocalAi).toHaveBeenCalledTimes(1);
    const back = await screen.findByText("Advanced settings are back to their defaults.");
    expect(back).toHaveAttribute("role", "status");
    expect(back).not.toHaveClass("error");
  });

  it("offers Get engine when the local engine is missing", async () => {
    api.getLocalAi.mockResolvedValue(PROFILES["8 GB GPU"]);
    api.startLocalEngineDownload.mockResolvedValue({
      ...missingEngine(),
      state: "downloading",
      button: "Cancel",
      progress: {
        done: 0,
        total: 32_000_000,
        verifying: false,
        line: "Downloading engine b11602: 0 of 32 MB",
      },
    });
    render(<LocalAi />);
    expect(await screen.findByLabelText("Local engine")).toBeInTheDocument();
    expect(screen.getByText(/llama\.cpp b11602/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Get engine b11602 (32 MB)" }));
    expect(api.startLocalEngineDownload).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveClass("get-engine");
    expect(screen.getByText("Downloading engine b11602: 0 of 32 MB")).toHaveClass(
      "local-engine-progress",
    );
    expect(screen.queryByRole("button", { name: "Get engine b11602 (32 MB)" })).toBeNull();

    // The ready event clears Cancel on its own, before the panel re-reads.
    api.getLocalAi.mockResolvedValue({ ...PROFILES["8 GB GPU"], engine: READY });
    engineEvent({
      state: "ready",
      progress: null,
      error: null,
      message: "Installed llama.cpp b11602 (32 MB) at /home/user/.config/Loadout/bin/llama-server",
    });
    expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
    expect(screen.queryByText(/Downloading engine/)).toBeNull();
    const note = screen.getByText(/Installed llama\.cpp b11602/);
    expect(note).toHaveAttribute("role", "status");
    expect(note).not.toHaveClass("error");
    expect(await screen.findByText(/llama-server is ready at/)).toBeInTheDocument();
    expect(api.getLocalAi).toHaveBeenCalledTimes(2);
  });

  it("a failed engine download shows its error in the error colour and offers Get engine again", async () => {
    api.getLocalAi.mockResolvedValue(PROFILES["8 GB GPU"]);
    api.startLocalEngineDownload.mockResolvedValue({
      ...missingEngine(),
      state: "downloading",
      button: "Cancel",
      progress: { done: 0, total: 32_000_000, verifying: false, line: "Downloading engine b11602: 0 of 32 MB" },
    });
    render(<LocalAi />);
    await userEvent.click(await screen.findByRole("button", { name: "Get engine b11602 (32 MB)" }));
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
    const failed =
      "The engine download didn't match its published checksum, so it was deleted. Try Get engine again.";
    engineEvent({ state: "missing", progress: null, error: failed, message: failed });
    const line = screen.getByText(failed, { selector: ".local-engine-error" });
    expect(line).toHaveClass("note", "error");
    expect(line).toHaveAttribute("role", "status");
    expect(screen.getByText(failed, { selector: ".local-note" })).toHaveClass("error");
    expect(screen.getByRole("button", { name: "Get engine b11602 (32 MB)" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
  });

  it("a catalog build with its safety training removed asks once, in its own words", async () => {
    const path = "/home/user/Loadout/models/Huihui-gemma-4-E4B-it-qat-q4_0-unquantized-abliterated-Q4_K.gguf";
    const file: LocalFileRow = {
      path,
      name: "Huihui-gemma-4-E4B-it-qat-q4_0-unquantized-abliterated-Q4_K.gguf",
      size: "5 GB",
      source: "Loadout models",
      thirdParty: false,
      abliterated: true,
      needsNotice: true,
    };
    api.getLocalAi.mockResolvedValue({
      ...PROFILES["8 GB GPU"],
      picker: { ...PROFILES["8 GB GPU"].picker, onThisPc: [file] },
    });
    render(<LocalAi />);
    await openAdvanced();
    const row = screen.getByRole("listitem", { name: file.name });
    expect(within(row).getByText("Safety training removed")).toBeInTheDocument();
    expect(within(row).queryByText("Third-party")).toBeNull();
    await userEvent.click(within(row).getByRole("button", { name: "Add" }));
    const notice = await screen.findByRole("alertdialog", { name: "About this model" });
    expect(notice).toHaveTextContent(ABLITERATED_NOTICE);
    expect(notice).not.toHaveTextContent(MODEL_NOTICE);
    await userEvent.click(screen.getByRole("button", { name: "I understand" }));
    expect(added()).toEqual([file.name]);
  });

  it("a running draft download keeps its progress and Cancel in view", async () => {
    api.getLocalAi.mockResolvedValue({
      ...PROFILES["16 GB GPU"],
      draftOffers: [GEMMA_OFFER],
      draftProgress: GEMMA_PROGRESS,
    });
    api.cancelLocalDrafterDownload.mockResolvedValue(undefined);
    render(<LocalAi />);
    const offer = await screen.findByLabelText("Draft model offer");
    expect(offer.closest("details")).toBeNull();
    expect(within(offer).getByText("Downloading Gemma 4 E2B: 1.2 of 3.2 GB")).toHaveClass(
      "local-draft-progress",
    );
    await userEvent.click(within(offer).getByRole("button", { name: "Cancel draft download" }));
    expect(api.cancelLocalDrafterDownload).toHaveBeenCalledTimes(1);
    expect(await screen.findByText("Cancelled the draft model download.")).toHaveAttribute(
      "role",
      "status",
    );
  });

  it("a paused draft download offers Resume until the panel re-reads", async () => {
    const v = { ...PROFILES["16 GB GPU"], draftOffers: [GEMMA_OFFER], draftProgress: GEMMA_PROGRESS };
    api.getLocalAi.mockResolvedValueOnce(v).mockReturnValue(new Promise<LocalAiView>(() => {}));
    render(<LocalAi />);
    expect(await screen.findByRole("button", { name: "Cancel draft download" })).toBeInTheDocument();
    drafterEvent({
      state: "paused",
      progress: null,
      error: null,
      message: "Paused Gemma 4 E2B at 1.2 of 3.2 GB. Resume picks up where it stopped.",
    });
    expect(screen.getByRole("button", { name: "Resume…" })).toHaveClass("install-draft");
    api.startLocalDrafterDownload.mockResolvedValue(GEMMA_OFFER);
    await userEvent.click(screen.getByRole("button", { name: "Resume…" }));
    expect(api.startLocalDrafterDownload).toHaveBeenCalledWith("gemma-4-12b-it-abliterated-q4_k");
    expect(screen.queryByText(/Downloading Gemma 4 E2B/)).toBeNull();
    expect(api.getLocalAi).toHaveBeenCalledTimes(2);
  });

  it("offers the draft install for Auto's pick under Advanced when its drafter is missing", async () => {
    api.getLocalAi.mockResolvedValue({
      ...PROFILES["16 GB GPU"],
      draftOffers: [GEMMA_OFFER],
      draftOffer: GEMMA_OFFER,
    });
    api.startLocalDrafterDownload.mockResolvedValue(GEMMA_OFFER);
    render(<LocalAi />);
    await openAdvanced();
    const offer = await screen.findByLabelText("Draft model offer");
    expect(offer.closest("details")).toHaveClass("local-advanced");
    expect(offer).toHaveTextContent(
      "Optional: Gemma 4 E2B can draft replies for Gemma 4 12B, which can make them faster on some machines.",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Install and run draft model (3.2 GB)" }),
    );
    expect(api.startLocalDrafterDownload).toHaveBeenCalledWith("gemma-4-12b-it-abliterated-q4_k");
  });
});
