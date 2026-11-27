// Safe defaults for Chat / ChatTabs hangout + tab-strip API exports in vitest
// mocks. Spread into every `vi.mock("./api", …)` that mounts `<Chat />` (or
// App, which keeps Chat mounted).

export type HangoutModeStub = "talk" | "type";

const emptyTab = { id: "", title: "", pinned: false, updated: 0, main: false };

/** Plain promise stubs — safe inside `vi.mock` factories (no vi.fn required). */
export function chatTabApiStubs() {
  return {
    getHangoutMode: () => Promise.resolve("type" as HangoutModeStub),
    setHangoutMode: (mode: HangoutModeStub) => Promise.resolve(mode),
    listChatTabs: () => Promise.resolve([] as typeof emptyTab[]),
    listArchivedChats: () => Promise.resolve([] as typeof emptyTab[]),
    listDeletedChats: () => Promise.resolve([] as typeof emptyTab[]),
    chatTabSwitch: () => Promise.resolve({ ...emptyTab }),
    chatTabPin: () => Promise.resolve(false),
    chatTabRename: () => Promise.resolve(),
    chatTabArchive: () => Promise.resolve({ ...emptyTab }),
    chatTabUnarchive: () => Promise.resolve({ ...emptyTab }),
    chatTabSoftDelete: () => Promise.resolve({ ...emptyTab }),
    chatTabRestoreDeleted: () => Promise.resolve({ ...emptyTab }),
    clearChat: () => Promise.resolve(null),
    // No profile: the rail shows no avatar unless a suite gives one.
    getProfile: () => Promise.reject(new Error("no profile")),
    // No hub and no screenshot: the empty chat shows no device glance.
    getDeviceGlance: () => Promise.resolve(null),
    // Sentence autocomplete stays off in tests unless a suite turns it on.
    getBehavior: () =>
      Promise.resolve({
        closeToTray: true,
        behavior: {
          autocomplete: false,
          composerGlow: false,
          suggestions: true,
          quietStart: "",
          quietEnd: "",
          dailyTokenBudget: 0,
          pauseOverBudget: false,
          lane: "coding",
        },
        budgets: [] as { value: number; label: string }[],
      }),
    composerGhostSuggest: () => Promise.resolve(""),
    // The top bar's pills read these; a suite that cares mocks its own.
    getPersonalities: () => Promise.reject(new Error("not mocked")),
    getPrivacy: () => Promise.reject(new Error("not mocked")),
    // The helper strip above the composer: no helper runs unless a suite says so.
    getQueue: () => Promise.resolve({ todos: [], tasks: [] }),
    onTasksChanged: () => Promise.resolve(() => {}),
    steerTask: () => Promise.resolve({ todos: [], tasks: [] }),
    stopTask: () => Promise.resolve({ todos: [], tasks: [] }),
  };
}
