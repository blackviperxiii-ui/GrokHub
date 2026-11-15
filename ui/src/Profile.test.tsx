import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Profile } from "./api";

const api = vi.hoisted(() => ({
  getProfile: vi.fn<() => Promise<Profile>>(),
  setProfileName: vi.fn<(name: string) => Promise<Profile>>(),
  pickProfilePicture: vi.fn<() => Promise<Profile | null>>(),
  clearProfilePicture: vi.fn<() => Promise<Profile>>(),
}));
vi.mock("./api", () => api);

import { NAME_HINT, PICTURE_SET, PICTURE_UNSET, ProfileSettings, REMOVE_HINT } from "./Profile";

const NONE: Profile = { name: "", shownName: "You", initial: "Y", picture: null };
const PIC = "data:image/png;base64,iVBORw0KGgo=";

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

describe("ProfileSettings", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    api.getProfile.mockResolvedValue(NONE);
  });

  it("shows the name field with the shown name as a hint and the initial", async () => {
    api.getProfile.mockResolvedValue({ ...NONE, shownName: "Grok Name", initial: "G" });
    render(<ProfileSettings canPick />);
    const field = await screen.findByLabelText("Name");
    expect(field).toHaveValue("");
    expect(field).toHaveAttribute("placeholder", "Grok Name");
    expect(field).toHaveAttribute("maxLength", "64");
    expect(screen.getByText(NAME_HINT)).toBeVisible();
    expect(NAME_HINT).toBe("Shown on the rail. Leave this blank to use your Grok name.");
    expect(screen.getByText("G")).toBeVisible();
    expect(screen.getByText(PICTURE_UNSET)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Remove" })).toBeNull();
  });

  it("saves the name trimmed when the field is left, once, and passes it on", async () => {
    const saved = { name: "Pat", shownName: "Pat", initial: "P", picture: null };
    api.setProfileName.mockResolvedValue(saved);
    const onProfile = vi.fn();
    render(<ProfileSettings canPick onProfile={onProfile} />);
    await userEvent.type(await screen.findByLabelText("Name"), "  Pat ");
    await userEvent.tab();
    expect(api.setProfileName).toHaveBeenCalledWith("Pat");
    expect(await screen.findByText("Saved")).toBeVisible();
    expect(onProfile).toHaveBeenCalledWith(saved);
    expect(screen.getByLabelText("Name")).toHaveValue("Pat");
    // Enter then leaving with nothing changed saves nothing more.
    await userEvent.type(screen.getByLabelText("Name"), "{Enter}");
    await userEvent.tab();
    expect(api.setProfileName).toHaveBeenCalledTimes(1);
  });

  it("Choose shows the picture and offers Remove", async () => {
    const pick = deferred<Profile | null>();
    api.pickProfilePicture.mockReturnValue(pick.promise);
    render(<ProfileSettings canPick />);
    await userEvent.click(await screen.findByRole("button", { name: "Choose" }));
    expect(screen.getByText("Choose a picture…")).toBeVisible();
    expect(screen.getByRole("button", { name: "Choose" })).toBeDisabled();
    pick.resolve({ ...NONE, picture: PIC });
    expect(await screen.findByText(PICTURE_SET)).toBeVisible();
    expect(screen.getByText("Saved")).toBeVisible();
    expect(document.querySelector(".profile-avatar")).toHaveAttribute("src", PIC);
    expect(screen.getByText(REMOVE_HINT)).toBeVisible();
    expect(screen.getByRole("button", { name: "Remove" })).toBeEnabled();
  });

  it("a closed picker changes nothing and clears the note", async () => {
    api.pickProfilePicture.mockResolvedValue(null);
    const onProfile = vi.fn();
    render(<ProfileSettings canPick onProfile={onProfile} />);
    await userEvent.click(await screen.findByRole("button", { name: "Choose" }));
    await waitFor(() => expect(screen.queryByText("Choose a picture…")).toBeNull());
    expect(screen.getByRole("button", { name: "Choose" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Remove" })).toBeNull();
    // Only the first read; the closed picker hands nothing on.
    expect(onProfile).toHaveBeenCalledTimes(1);
  });

  it("a name save in flight never blocks Choose, and typing waits for it", async () => {
    const save = deferred<Profile>();
    api.setProfileName.mockReturnValue(save.promise);
    api.pickProfilePicture.mockResolvedValue(null);
    render(<ProfileSettings canPick />);
    const field = await screen.findByLabelText("Name");
    await userEvent.type(field, "Pat");
    // Leaving the field for Choose: the blur starts the save, the click still lands.
    await userEvent.click(screen.getByRole("button", { name: "Choose" }));
    expect(api.setProfileName).toHaveBeenCalledWith("Pat");
    expect(api.pickProfilePicture).toHaveBeenCalledTimes(1);
    expect(field).toHaveAttribute("readonly");
    await userEvent.type(field, "x");
    expect(field).toHaveValue("Pat");
    save.resolve({ name: "Pat", shownName: "Pat", initial: "P", picture: null });
    await waitFor(() => expect(field).not.toHaveAttribute("readonly"));
    await userEvent.type(field, "s");
    expect(field).toHaveValue("Pats");
  });

  it("reads the profile again after a Grok sign-in and passes it on", async () => {
    const onProfile = vi.fn();
    const { rerender } = render(<ProfileSettings canPick onProfile={onProfile} signedIn="" />);
    expect(await screen.findByLabelText("Name")).toHaveAttribute("placeholder", "You");
    expect(onProfile).toHaveBeenLastCalledWith(NONE);
    const grok = { ...NONE, shownName: "Robin", initial: "R" };
    api.getProfile.mockResolvedValue(grok);
    rerender(<ProfileSettings canPick onProfile={onProfile} signedIn="Robin Lee|" />);
    await waitFor(() => expect(screen.getByLabelText("Name")).toHaveAttribute("placeholder", "Robin"));
    expect(onProfile).toHaveBeenLastCalledWith(grok);
    expect(api.getProfile).toHaveBeenCalledTimes(2);
  });

  it("a sign-in that lands while a name is typed keeps the typed name", async () => {
    const { rerender } = render(<ProfileSettings canPick signedIn="" />);
    const field = await screen.findByLabelText("Name");
    await userEvent.type(field, "Pat");
    api.getProfile.mockResolvedValue({ ...NONE, shownName: "Robin", initial: "R" });
    rerender(<ProfileSettings canPick signedIn="Robin Lee|" />);
    await waitFor(() => expect(field).toHaveAttribute("placeholder", "Robin"));
    expect(field).toHaveValue("Pat");
    expect(field).toHaveFocus();
  });

  it("a device with no file picker shows the initial but no Choose", async () => {
    render(<ProfileSettings canPick={false} />);
    expect(await screen.findByLabelText("Name")).toBeVisible();
    expect(screen.getByText("Y")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Choose" })).toBeNull();
    expect(screen.queryByText(PICTURE_UNSET)).toBeNull();
  });

  it("a refused image shows why", async () => {
    api.pickProfilePicture.mockRejectedValue("Choose a smaller image");
    render(<ProfileSettings canPick />);
    await userEvent.click(await screen.findByRole("button", { name: "Choose" }));
    expect(await screen.findByText("Choose a smaller image")).toHaveClass("status--error");
  });

  it("Remove goes back to the initial", async () => {
    api.getProfile.mockResolvedValue({ ...NONE, picture: PIC });
    api.clearProfilePicture.mockResolvedValue(NONE);
    render(<ProfileSettings canPick />);
    await userEvent.click(await screen.findByRole("button", { name: "Remove" }));
    expect(api.clearProfilePicture).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(PICTURE_UNSET)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Remove" })).toBeNull();
    expect(screen.getByText("Y")).toBeVisible();
  });
});
