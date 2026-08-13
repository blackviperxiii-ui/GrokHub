import { expect, test } from "@playwright/test";

test.describe("GrokHub smoke", () => {
  test("chat composer is present and is not sent to xAI", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByRole("button", { name: "Agent" }).first()).toBeVisible();
    const composer = page.locator("[data-composer]");
    await expect(composer).toBeVisible();
    await expect(composer).toHaveAttribute("placeholder", /Message Grok/);
    await composer.fill("e2e smoke — do not send");
    await expect(composer).toHaveValue("e2e smoke — do not send");
    await expect(page.getByRole("button", { name: "Send" })).toBeVisible();
    await expect(page.locator("[data-connect-grok]")).toBeVisible();
    await expect(page.getByText("Bind folder")).toHaveCount(0);
    await expect(page.getByText(/connect OAuth or API key/i)).toHaveCount(0);
    await expect(page.getByText("$ shell")).toHaveCount(0);
    await expect(page.locator("[data-conn]").first()).toHaveAttribute("data-conn", /setup|offline|live/);
  });

  test("Agent, Queue, and Settings nav", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator('[data-hydrated="1"]')).toBeVisible();
    await expect(page.locator('[data-nav="chat"]').first()).toBeVisible();

    const toolsToggle = page.getByRole("button", { name: "Tools" });
    const queueBtn = page.getByRole("button", { name: "Queue" }).first();
    if (await toolsToggle.isVisible()) {
      if (!(await queueBtn.isVisible())) await toolsToggle.click();
    }

    await expect(page.locator('[data-nav="queue"]').first()).toHaveAttribute("data-queue-count", /\d+/);

    await queueBtn.click();
    await expect(page.getByRole("heading", { name: "Agent queue" })).toBeVisible();
    await expect(page.locator("[data-next-up]")).toBeVisible();
    await expect(page.locator("[data-next-up]")).toContainText(/Queue empty|paused|HOST_CMD waiting|queued|running|waiting/);

    await page.getByRole("button", { name: "Settings" }).first().click();
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();
    await page
      .getByRole("navigation", { name: "Settings categories" })
      .getByRole("button", { name: /^Agent/ })
      .click();
    await expect(page.getByText("Host tools (HOST_CMD)")).toBeVisible();
    await expect(page.getByText("Host CLI / files / apps", { exact: true })).toBeVisible();
  });

  test("Imagine local preview is honest when disconnected", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator('[data-hydrated="1"]')).toBeVisible();
    await page.locator('[data-nav="imagine"]').first().click();
    await expect(page.getByRole("heading", { name: "Imagine" })).toBeVisible();
    await expect(page.getByText(/not an xAI image/)).toBeVisible();
  });

  test("host info and exec via /api/host", async ({ request }) => {
    const infoRes = await request.post("/api/host", { data: { action: "info" } });
    expect(infoRes.ok()).toBeTruthy();
    const info = (await infoRes.json()) as { platform?: string; homedir?: string };
    expect(info.platform).toBeTruthy();
    expect(info.homedir).toBeTruthy();

    const execRes = await request.post("/api/host", {
      data: { action: "exec", command: "echo grokhub-smoke" },
    });
    expect(execRes.ok()).toBeTruthy();
    const exec = (await execRes.json()) as { ok?: boolean; stdout?: string };
    expect(exec.ok).toBe(true);
    expect(String(exec.stdout)).toMatch(/grokhub-smoke/);
  });
});
