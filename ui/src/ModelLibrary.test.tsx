import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ModelLibrary } from "./ModelLibrary";
import type { LibraryStatus, LibraryView } from "./api";

const api = vi.hoisted(() => ({
  getModelLibrary: vi.fn<() => Promise<LibraryView>>(),
  downloadModels: vi.fn<(ids: string[]) => Promise<LibraryView>>(),
  pauseModelDownloads: vi.fn<() => Promise<void>>(),
  removeLibraryModel: vi.fn<(id: string) => Promise<LibraryView>>(),
  onModelLibraryEvent: vi.fn(async (cb: (s: LibraryStatus) => void) => {
    events.push(cb);
    return () => undefined;
  }),
}));
const events = vi.hoisted(() => [] as ((s: LibraryStatus) => void)[]);
vi.mock("./api", () => api);

const idle: LibraryStatus = { job: null, queued: 0, message: null, errors: {}, running: false };

function library(over: Partial<LibraryView> = {}): LibraryView {
  return {
    tabs: [
      {
        label: "Chat",
        rows: [
          {
            id: "g12",
            name: "Gemma 4 12B",
            line: "Gemma 4 12B · 6.9 GB · Fits this PC · Uncensored · Gemma",
            about: "Longer tasks and stronger answers.",
            state: "download",
          },
          {
            id: "e2b",
            name: "Gemma 4 E2B",
            line: "Gemma 4 E2B · 3.2 GB · Fits this PC · Uncensored · Gemma",
            about: "Quick chats and background tasks, even on a laptop.",
            state: "inUse",
          },
        ],
        empty: null,
        all: { ids: ["g12", "g12-mtp"], line: "2 models, 8.4 GB in all." },
      },
      {
        label: "Drafts",
        rows: [
          {
            id: "e2b-mtp",
            name: "Gemma 4 E2B draft model",
            line: "Gemma 4 E2B draft model · 0.4 GB · Fits this PC · Uncensored · Gemma",
            about: "Makes Gemma 4 E2B reply faster.",
            state: "downloaded",
          },
        ],
        empty: null,
        all: null,
      },
      { label: "Image", rows: [], empty: "No image models yet. They arrive with on-device image making.", all: null },
      { label: "Voice", rows: [], empty: "No voice models yet. They arrive with on-device voice.", all: null },
      { label: "Other", rows: [], empty: "Nothing here yet.", all: null },
    ],
    lineup: {
      ids: ["g12", "g12-mtp", "zimg"],
      line: "3 models, 14.6 GB in all. Speed: Gemma 4 E2B (downloaded) · Everyday: Gemma 4 12B · Video: No on-device video models yet.",
    },
    everything: { ids: ["g12", "g12-mtp"], line: "2 models, 8.4 GB in all." },
    status: idle,
    ...over,
  };
}

describe("ModelLibrary", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.onModelLibraryEvent.mockImplementation(async (cb) => {
      events.push(cb);
      return () => undefined;
    });
    events.length = 0;
  });

  it("offers everything that fits once the lineup is downloaded", async () => {
    api.getModelLibrary.mockResolvedValue(
      library({ lineup: { ids: [], line: "This PC has its best lineup. Everyday: Gemma 4 12B (downloaded)." } }),
    );
    render(<ModelLibrary />);
    expect(await screen.findByText(/This PC has its best lineup\./)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Download lineup" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Download everything that fits (2 models, 8.4 GB in all.)" }));
    expect(api.downloadModels).toHaveBeenCalledWith(["g12", "g12-mtp"]);
  });

  it("shows the tabs and the rows, with In use and Download", async () => {
    api.getModelLibrary.mockResolvedValue(library());
    render(<ModelLibrary />);
    const tabs = await screen.findByRole("tablist", { name: "Model kinds" });
    expect(within(tabs).getAllByRole("tab").map((t) => t.textContent)).toEqual([
      "Chat (2)",
      "Drafts (1)",
      "Image (0)",
      "Voice (0)",
      "Other (0)",
    ]);
    const inUse = screen.getByRole("listitem", { name: "Gemma 4 E2B" });
    expect(inUse).toHaveTextContent("In use.");
    await userEvent.click(screen.getByRole("tab", { name: "Image (0)" }));
    expect(screen.getByText(/No image models yet/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("tab", { name: "Drafts (1)" }));
    api.removeLibraryModel.mockResolvedValue(library());
    await userEvent.click(screen.getByRole("button", { name: "Remove Gemma 4 E2B draft model" }));
    expect(api.removeLibraryModel).toHaveBeenCalledWith("e2b-mtp");
  });

  it("downloads the lineup it described and shows progress until the queue ends", async () => {
    api.getModelLibrary.mockResolvedValue(library());
    const busy: LibraryStatus = {
      job: { id: "g12", line: "Downloading Gemma 4 12B: 1.0 of 6.9 GB", pct: 14 },
      queued: 1,
      message: null,
      errors: {},
      running: true,
    };
    api.downloadModels.mockResolvedValue(library({ status: busy }));
    render(<ModelLibrary />);
    expect(await screen.findByText(/^Best lineup for this PC: 3 models, 14.6 GB in all\. Speed: /)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Download everything/ })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Download lineup" }));
    expect(api.downloadModels).toHaveBeenCalledWith(["g12", "g12-mtp", "zimg"]);
    expect(await screen.findByText("1 more after this one.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Pause downloads" }));
    expect(api.pauseModelDownloads).toHaveBeenCalled();
    api.getModelLibrary.mockResolvedValue(library({ status: { ...idle, message: "Downloaded Gemma 4 12B." } }));
    act(() => {
      for (const cb of events) cb({ ...idle, message: "Downloaded Gemma 4 12B." });
    });
    expect(await screen.findByText("Downloaded Gemma 4 12B.")).toBeInTheDocument();
    expect(api.getModelLibrary).toHaveBeenCalledTimes(2);
  });
  it("shows a failed download's reason on its own row, under its button", async () => {
    api.getModelLibrary.mockResolvedValue(library());
    render(<ModelLibrary />);
    const row = await screen.findByRole("listitem", { name: "Gemma 4 12B" });
    expect(within(row).queryByRole("alert")).toBeNull();
    const failed: LibraryStatus = {
      ...idle,
      message: "Gemma 4 12B didn't download. Its row says why.",
      errors: {
        g12: {
          step: "free space",
          message:
            "Not enough free space for Gemma 4 12B. To make room, remove a downloaded model you don't use with the Remove button on its row, then download this one again.",
          detail: "Failed at: free space (3.1 GB free, 8.9 GB needed)",
        },
      },
    };
    api.getModelLibrary.mockResolvedValue(library({ status: failed }));
    act(() => {
      for (const cb of events) cb(failed);
    });
    const alert = await within(row).findByRole("alert");
    expect(alert).toHaveTextContent(/^Not enough free space for Gemma 4 12B\. To make room, remove a downloaded model/);
    expect(alert).toHaveTextContent("Failed at: free space (3.1 GB free, 8.9 GB needed)");
    // The reason follows its own row's button, inside the row.
    const button = within(row).getByRole("button", { name: "Download Gemma 4 12B" });
    expect(button.compareDocumentPosition(alert) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // The other row says nothing.
    expect(within(screen.getByRole("listitem", { name: "Gemma 4 E2B" })).queryByRole("alert")).toBeNull();
  });
});
