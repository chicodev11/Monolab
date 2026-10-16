import { expect, test, type Page } from "@playwright/test";
import { ACCOUNT } from "./app";

test.describe.configure({ mode: "serial" });

let page: Page;
const serverName = process.env.SE_E2E_SERVER;

function card() {
  const cards = page.locator(".server-card:not(.drag-ghost)");
  return serverName ? cards.filter({ hasText: serverName }).first() : cards.first();
}

test.beforeAll(async ({ browser }) => {
  page = await browser.newPage();
  await page.addInitScript(() => localStorage.setItem("monolab-language", "en"));
});

test.afterAll(async () => {
  await page.close();
});

test("1. the app starts and shows the login first", async () => {
  await page.goto("/");
  await expect(page.getByRole("button", { name: "Sign in" })).toBeVisible();
  await expect(page.getByText("My servers")).toHaveCount(0);
});

test("2. signing in shows the server list", async () => {
  const inputs = page.locator("input");
  await inputs.nth(0).fill(ACCOUNT.name);
  await inputs.nth(1).fill(ACCOUNT.password);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByText("My servers").first()).toBeVisible();
  await expect(card()).toBeVisible();
  await expect(card()).toContainClass("stopped");
});

test("3. starting a server makes it running", async () => {
  await card().getByRole("button", { name: "Start" }).click();
  const anyway = page.getByRole("button", { name: "Start anyway" });
  if (await anyway.isVisible({ timeout: 3_000 }).catch(() => false)) await anyway.click();
  await expect(card()).toContainClass("running", { timeout: 60_000 });
});

test("4. the console receives server output", async () => {
  await card().locator(".card-header").click();
  await page.locator(".instance-nav button", { hasText: "Console" }).click();
  const output = page.locator(".console-output");
  await expect(output).toBeVisible();
  await expect(output).toContainText(/Server started!|Done \(/, { timeout: 120_000 });
});

test("5. stopping returns it to stopped", async () => {
  const head = page.locator("header.instance-topbar");
  await head.locator(".run-button", { hasText: "Stop" }).click();
  await expect(head).toContainText("Stopped", { timeout: 90_000 });
});
