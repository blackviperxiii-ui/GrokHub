import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ScopeAnswer, ScopesView } from "./scopesApi";

const api = vi.hoisted(() => ({
  getScopes: vi.fn<() => Promise<ScopesView>>(),
  allowScope: vi.fn<(kind: string, value: string) => Promise<ScopeAnswer>>(),
  revokeScope: vi.fn<(id: string) => Promise<ScopeAnswer>>(),
  pickScopeFolder: vi.fn<() => Promise<string | null>>(),
  forgetScopeFacts: vi.fn<(id: string) => Promise<ScopeAnswer>>(),
}));
vi.mock("./scopesApi", () => api);

import { Scopes } from "./Scopes";

const HEAD = "What Loadout can read";
const NOTHING = {
  head: null,
  lines: [],
  more: null,
  note: "Nothing learned to show. Loadout reads it on a heartbeat, never on battery or in quiet hours.",
  canForget: false,
};

function view(over: Partial<ScopesView> = {}): ScopesView {
  return {
    heading: HEAD,
    note: "Each is off until you allow it here.",
    folderHint: "/home/you/Notes",
    browsers: [
      ["firefox", "Firefox"],
      ["brave", "Brave"],
    ],
    rows: [
      {
        kind: "files",
        granted: [],
        add: { title: "Files in a folder", hint: "Off. A folder you pick. Never your whole home folder, keys or logins." },
      },
      {
        kind: "apps",
        granted: [{ id: "g-apps", title: "Installed apps", hint: "On since 1m ago. Installed apps and how often you open them.", detail: null, learned: NOTHING }],
        add: null,
      },
      {
        kind: "browser_history",
        granted: [],
        add: { title: "Browser history", hint: "Off. Pages you visited in one browser. Never cookies or saved logins." },
      },
    ],
    locked: null,
    lockHover: null,
    ...over,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  api.getScopes.mockResolvedValue(view());
});

describe("Scopes", () => {
  it("lists each scope, off until allowed, with Revoke on an allowed one", async () => {
    render(<Scopes />);
    const list = within(await screen.findByRole("list", { name: HEAD }));
    expect(list.getByText("Off. A folder you pick. Never your whole home folder, keys or logins.")).toBeInTheDocument();
    const apps = within(list.getByRole("listitem", { name: "Installed apps" }));
    expect(apps.getByRole("button", { name: "Revoke" })).toBeEnabled();
    expect(list.getAllByRole("button", { name: "Allow" })).toHaveLength(2);
  });

  it("allows a typed folder from a click and clears the field", async () => {
    api.allowScope.mockResolvedValue({ view: view(), status: "Files in notes allowed. Revoke it here any time." });
    render(<Scopes />);
    const files = within(await screen.findByRole("listitem", { name: "Files in a folder" }));
    await userEvent.type(files.getByRole("textbox", { name: "Folder" }), "/srv/notes");
    await userEvent.click(files.getByRole("button", { name: "Allow" }));
    expect(api.allowScope).toHaveBeenCalledWith("files", "/srv/notes");
    expect(await screen.findByRole("status")).toHaveTextContent("Files in notes allowed. Revoke it here any time.");
    expect(files.getByRole("textbox", { name: "Folder" })).toHaveValue("");
  });

  it("never allows from the keyboard", async () => {
    render(<Scopes />);
    const b = within(await screen.findByRole("listitem", { name: "Browser history" }));
    b.getByRole("button", { name: "Allow" }).focus();
    await userEvent.keyboard("{Enter}");
    await userEvent.keyboard(" ");
    expect(api.allowScope).not.toHaveBeenCalled();
    await userEvent.selectOptions(b.getByRole("combobox", { name: "Browser" }), "brave");
    api.allowScope.mockResolvedValue({ view: view(), status: "Browser history (Brave) allowed." });
    await userEvent.click(b.getByRole("button", { name: "Allow" }));
    expect(api.allowScope).toHaveBeenCalledWith("browser_history", "brave");
  });

  it("revokes and shows the line", async () => {
    api.revokeScope.mockResolvedValue({
      view: view(),
      status: "Installed apps revoked. It's off until you allow it again.",
    });
    render(<Scopes />);
    const apps = within(await screen.findByRole("listitem", { name: "Installed apps" }));
    await userEvent.click(apps.getByRole("button", { name: "Revoke" }));
    expect(api.revokeScope).toHaveBeenCalledWith("g-apps");
    expect(await screen.findByRole("status")).toHaveTextContent("Installed apps revoked. It's off until you allow it again.");
  });

  it("fills the field from the folder dialog without allowing", async () => {
    api.pickScopeFolder.mockResolvedValue("/srv/plans");
    render(<Scopes />);
    const files = within(await screen.findByRole("listitem", { name: "Files in a folder" }));
    await userEvent.click(files.getByRole("button", { name: "Choose folder…" }));
    expect(files.getByRole("textbox", { name: "Folder" })).toHaveValue("/srv/plans");
    expect(api.allowScope).not.toHaveBeenCalled();
  });

  it("disables every pill while private data is locked", async () => {
    api.getScopes.mockResolvedValue(
      view({ locked: "Your private data is locked. Nothing was deleted.", lockHover: "Private data is unavailable" }),
    );
    render(<Scopes />);
    expect(await screen.findByText("Your private data is locked. Nothing was deleted.")).toBeInTheDocument();
    for (const b of screen.getAllByRole("button", { name: /Allow|Revoke/ })) {
      expect(b).toBeDisabled();
      expect(b).toHaveAttribute("title", "Private data is unavailable");
    }
  });

  it("lists what an allowed scope taught, and Forget these asks first", async () => {
    const taught = {
      head: "Learned 5 things, sealed on this computer:",
      lines: ["App: Blender, opened 12 times", "App: Steam, opened 9 times", "App: Krita, opened 4 times"],
      more: "and 2 more",
      note: null,
      canForget: true,
    };
    const shown = view();
    shown.rows[1].granted[0] = { ...shown.rows[1].granted[0], learned: taught };
    api.getScopes.mockResolvedValue(shown);
    api.forgetScopeFacts.mockResolvedValue({
      view: view(),
      status: "Forgot 5 things learned from Installed apps. It stays allowed; revoke it to stop reading.",
    });
    render(<Scopes />);
    const apps = within(await screen.findByRole("listitem", { name: "Installed apps" }));
    const facts = within(apps.getByLabelText("Learned from Installed apps"));
    expect(facts.getByText("Learned 5 things, sealed on this computer:")).toBeInTheDocument();
    expect(facts.getByText("App: Krita, opened 4 times")).toBeInTheDocument();
    expect(facts.getByText("and 2 more")).toBeInTheDocument();
    await userEvent.click(apps.getByRole("button", { name: "Forget these" }));
    expect(api.forgetScopeFacts).not.toHaveBeenCalled();
    const ask = within(apps.getByRole("group", { name: "Forget what Installed apps taught?" }));
    await userEvent.click(ask.getByRole("button", { name: "Forget" }));
    expect(api.forgetScopeFacts).toHaveBeenCalledWith("g-apps");
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Forgot 5 things learned from Installed apps. It stays allowed; revoke it to stop reading.",
    );
  });

  it("says why nothing is listed and offers no Forget these", async () => {
    render(<Scopes />);
    const apps = within(await screen.findByRole("listitem", { name: "Installed apps" }));
    expect(
      apps.getByText("Nothing learned to show. Loadout reads it on a heartbeat, never on battery or in quiet hours."),
    ).toBeInTheDocument();
    expect(apps.queryByRole("button", { name: "Forget these" })).toBeNull();
  });
});
