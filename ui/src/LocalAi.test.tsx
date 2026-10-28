import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  LocalAiConfig,
  LocalAiView,
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
  startLocalEngineDownload: vi.fn(),
  cancelLocalEngineDownload: vi.fn(),
  startLocalDrafterDownload: vi.fn(),
  cancelLocalDrafterDownload: vi.fn(),
  onLocalEngineEvent: vi.fn(async () => () => undefined),
  onLocalDrafterEvent: vi.fn(async () => () => undefined),
}));

vi.mock("./api", () => api);

import { LocalAi, MODEL_NOTICE } from "./LocalAi";

const IDS: Record<string, string> = {
  "Gemma 4 31B": "gemma-4-31b-it-q4_k_m",
  "Muse Glimmer 30B": "muse-glimmer-30b-q4_k_m",
  "Gemma 4 26B-A4B": "gemma-4-26b-a4b-it-q4_k_m",
  "Gemma 4 12B": "gemma-4-12b-it-q4_k_m",
  "Gemma 4 E4B": "gemma-4-e4b-it-q4_k_m",
  "Gemma 4 E2B": "gemma-4-e2b-it-q4_k_m",
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
    mode: "auto",
    pauseForGames: true,
    useIdleTime: true,
    throttleOnBattery: true,
    manual: {
      backend: "auto",
      draft: "auto",
      ngl: null,
      ctx: null,
      threads: null,
      kv: "auto",
      idleMinutes: 10,
      caps: { rampUp: 90, normal: 80, throttle: 50 },
      neverUnload: false,
      allowApps: [],
      denyApps: [],
    },
    localModel: { selected: "" },
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

/** The pre-selected row and the names under one heading. */
function bucket(title: string): string[] {
  return within(screen.getByRole("list", { name: title }))
    .getAllByRole("listitem")
    .map((li) => li.getAttribute("aria-label") ?? "");
}

function checkedName(): string | null {
  const radio = screen
    .getAllByRole("radio")
    .find((r) => (r as HTMLInputElement).checked);
  return radio?.closest("li")?.getAttribute("aria-label") ?? null;
}

async function openManual() {
  await userEvent.click(await screen.findByRole("button", { name: "Manual" }));
}

describe("LocalAi", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it.each([
    ["T1 laptop", "Gemma 4 E2B", ["Gemma 4 E2B"], ["Gemma 4 E4B"], 4],
    ["8 GB GPU", "Gemma 4 E4B", ["Gemma 4 E4B", "Gemma 4 E2B"], ["Gemma 4 12B"], 0],
    [
      "16 GB GPU",
      "Gemma 4 26B-A4B",
      ["Gemma 4 26B-A4B", "Gemma 4 12B"],
      ["Muse Glimmer 30B"],
      0,
    ],
    ["24 GB GPU", "Gemma 4 31B", ["Gemma 4 31B", "Muse Glimmer 30B"], null, 0],
    ["Apple 16 GB", "Gemma 4 E4B", ["Gemma 4 E4B", "Gemma 4 E2B"], ["Gemma 4 12B"], 1],
  ] as const)(
    "%s pre-selects %s with its buckets and reasons",
    async (profile, preselected, recommended, maySlow, hidden) => {
      const v = PROFILES[profile];
      api.getLocalAi.mockResolvedValue(v);
      render(<LocalAi />);
      await openManual();

      expect(checkedName()).toBe(preselected);
      expect(bucket("Recommended")).toEqual(recommended);
      if (maySlow) expect(bucket("May be slow")).toEqual(maySlow);
      else expect(screen.queryByRole("list", { name: "May be slow" })).toBeNull();
      for (const row of v.picker.models.filter((m) => m.bucket !== "hidden")) {
        expect(screen.getByText(row.reason)).toBeInTheDocument();
      }
      // Hidden models only under Show all, with their reasons.
      expect(screen.queryByRole("list", { name: "Won't run well here" })).toBeNull();
      if (hidden > 0) {
        await userEvent.click(screen.getByRole("button", { name: "Show all" }));
        expect(bucket("Won't run well here")).toHaveLength(hidden);
        for (const row of v.picker.models.filter((m) => m.bucket === "hidden")) {
          expect(screen.getByText(row.reason)).toBeInTheDocument();
        }
      }
    },
  );

  it("labels Muse Glimmer optional and never pre-selects it on a 24 GB GPU", async () => {
    api.getLocalAi.mockResolvedValue(PROFILES["24 GB GPU"]);
    render(<LocalAi />);
    await openManual();
    const muse = screen.getByRole("listitem", { name: "Muse Glimmer 30B" });
    expect(within(muse).getByText("Optional")).toBeInTheDocument();
    expect(within(muse).getByRole("radio")).not.toBeChecked();
    expect(muse).toHaveTextContent("5 GB · Q4_K_M · 8K ctx · about 40 tok/s · Apache-2.0");
    const top = screen.getByRole("listitem", { name: "Gemma 4 31B" });
    expect(within(top).getByText("Default for this PC")).toBeInTheDocument();
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
    expect(screen.getByRole("button", { name: "Auto" })).toHaveAttribute("aria-pressed", "true");
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

  it("a model file with safety training removed asks once before it is picked", async () => {
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
    await openManual();

    await userEvent.type(screen.getByLabelText("Model file path"), ` ${path} `);
    await userEvent.click(screen.getByRole("button", { name: "Use a model file" }));
    expect(api.checkModelFile).toHaveBeenCalledWith(path);
    const notice = await screen.findByRole("alertdialog", { name: "About this model" });
    expect(notice).toHaveTextContent(MODEL_NOTICE);
    // Cancel keeps the tier default.
    await userEvent.click(within(notice).getByRole("button", { name: "Cancel" }));
    expect(checkedName()).toBe("Gemma 4 E4B");

    const row = screen.getByRole("listitem", { name: file.name });
    expect(within(row).getByText("Safety training removed")).toBeInTheDocument();
    await userEvent.click(within(row).getByRole("radio"));
    await userEvent.click(screen.getByRole("button", { name: "I understand" }));
    expect(checkedName()).toBe(file.name);

    await userEvent.click(screen.getByRole("button", { name: "Save local AI" }));
    const saved = api.saveLocalAi.mock.calls[0][0];
    expect(saved.mode).toBe("manual");
    expect(saved.localModel).toEqual({ selected: path });
    expect(saved.noticeSeen).toEqual([path]);
  });

  it("a file whose notice was read is picked without asking", async () => {
    const path = "/models/qwen3-8b.gguf";
    api.getLocalAi.mockResolvedValue({
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
    });
    render(<LocalAi />);
    await openManual();
    const row = screen.getByRole("listitem", { name: "qwen3-8b.gguf" });
    expect(row).toHaveTextContent("Ollama · 4.9 GB · /models/qwen3-8b.gguf");
    expect(within(row).getByText("Third-party")).toBeInTheDocument();
    await userEvent.click(within(row).getByRole("radio"));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(checkedName()).toBe("qwen3-8b.gguf");
  });

  it("manual values save with caps held to 90% and app lists by line", async () => {
    const v = PROFILES["16 GB GPU"];
    api.getLocalAi.mockResolvedValue(v);
    api.saveLocalAi.mockResolvedValue(v);
    render(<LocalAi />);
    await openManual();
    await userEvent.selectOptions(screen.getByLabelText("Device"), "vulkan");
    await userEvent.selectOptions(screen.getByLabelText("Draft model"), "off");
    await userEvent.type(screen.getByLabelText("GPU layers"), "20");
    await userEvent.type(screen.getByLabelText("Context"), "16384");
    await userEvent.selectOptions(screen.getByLabelText("KV cache"), "q4");
    const idle = screen.getByLabelText("Idle");
    await userEvent.clear(idle);
    await userEvent.type(idle, "95");
    await userEvent.click(screen.getByLabelText("Never unload the model"));
    await userEvent.type(
      screen.getByLabelText(/Never pause for these apps/),
      "vlc.exe\n\n mpv ",
    );
    await userEvent.type(screen.getByLabelText(/Always pause for these apps/), "blender");
    await userEvent.click(screen.getByRole("radio", { name: /Gemma 4 12B/ }));
    await userEvent.click(screen.getByRole("button", { name: "Save local AI" }));

    expect(api.saveLocalAi).toHaveBeenCalledWith(
      config({
        mode: "manual",
        localModel: { selected: "gemma-4-12b-it-q4_k_m" },
        manual: {
          backend: "vulkan",
          draft: "off",
          ngl: 20,
          ctx: 16384,
          threads: null,
          kv: "q4",
          idleMinutes: 10,
          caps: { rampUp: 90, normal: 80, throttle: 50 },
          neverUnload: true,
          allowApps: ["vlc.exe", "mpv"],
          denyApps: ["blender"],
        },
      }),
    );
  });

  it("re-runs the speed test and resets to Auto", async () => {
    const v = PROFILES["8 GB GPU"];
    api.getLocalAi.mockResolvedValue(v);
    api.runLocalSpeedTest.mockRejectedValue(
      "Set up a local engine and model first, then run the speed test.",
    );
    api.resetLocalAi.mockResolvedValue(v);
    render(<LocalAi />);
    await openManual();
    await userEvent.click(screen.getByRole("button", { name: "Re-run speed test" }));
    expect(
      await screen.findByText("Set up a local engine and model first, then run the speed test."),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Reset to Auto" }));
    expect(api.resetLocalAi).toHaveBeenCalledTimes(1);
    expect(await screen.findByText("Back to Auto.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Auto" })).toHaveAttribute("aria-pressed", "true");
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
    await userEvent.click(screen.getByRole("button", { name: /Get engine b11602/ }));
    expect(api.startLocalEngineDownload).toHaveBeenCalledTimes(1);
  });

  it("picking Muse shows the draft install offer when its drafter is missing", async () => {
    const museOffer = {
      id: "muse-glimmer-30b-dflash-q4_k_m",
      name: "Muse Glimmer DFlash drafter",
      sizeLabel: "1.6 GB",
      line: "Optional: Muse Glimmer DFlash drafter can draft replies for Muse Glimmer 30B, which can make them faster on some machines. GrokHub only downloads it if you ask, and keeps it only if the speed test finds it helps.",
      button: "Install and run draft model (1.6 GB)",
      targetId: "muse-glimmer-30b-q4_k_m",
      targetName: "Muse Glimmer 30B",
    };
    api.getLocalAi.mockResolvedValue({
      ...PROFILES["24 GB GPU"],
      draftOffers: [museOffer],
      draftOffer: null,
    });
    api.startLocalDrafterDownload.mockResolvedValue(museOffer);
    render(<LocalAi />);
    await openManual();
    expect(screen.queryByLabelText("Draft model offer")).toBeNull();
    await userEvent.click(screen.getByRole("radio", { name: /Muse Glimmer 30B/ }));
    const offer = await screen.findByLabelText("Draft model offer");
    expect(offer).toHaveTextContent(/Optional: Muse Glimmer DFlash drafter/);
    await userEvent.click(
      screen.getByRole("button", { name: "Install and run draft model (1.6 GB)" }),
    );
    expect(api.startLocalDrafterDownload).toHaveBeenCalledWith("muse-glimmer-30b-q4_k_m");
  });
});
