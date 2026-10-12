import { test, expect } from "@playwright/test";

// Renderer checks deliberately mock IPC. Native Windows tests cover the real bridge separately.
test.beforeEach(async ({ page }, info) => {
  await page.addInitScript(
    ({ failure }) => {
      let preferences = { language: "zh", theme: "dark" };
      const profiles: { profile: any; configured: boolean; models: any[] }[] =
        [];
      let selected: string | null = null;
      const chats: { id: string; title: string; project_id: null }[] = [];
      const drafts: Record<string, string> = {};
      const messages: Record<string, unknown[]> = {};
      const calls: { cmd: string; args: Record<string, unknown> }[] = [];
      Object.defineProperty(window, "__TEST_CALLS__", { value: calls });
      Object.defineProperty(window, "__TAURI_INTERNALS__", {
        value: {
          invoke: async (cmd: string, args: Record<string, any> = {}) => {
            if (failure) throw "secret-bearing-platform-error";
            calls.push({ cmd, args });
            if (cmd === "bootstrap")
              return { preferences, profiles, selected, conversations: chats };
            if (cmd === "preferences") {
              preferences = args.preferences;
              return;
            }
            if (cmd === "save_profile") {
              profiles.push({
                profile: args.profile,
                configured: !!args.key,
                models: [],
              });
              return;
            }
            if (cmd === "discover_models") {
              const row = profiles.find((row) => row.profile.id === args.id)!;
              const ids =
                row.profile.kind === "deepseek"
                  ? ["deepseek-flash", "deepseek-v4-pro"]
                  : ["synthetic", "synthetic-fast"];
              row.models = ids.map((id) => ({
                id,
                name: `${row.profile.name} ${id}`,
                efforts: ["none", "low", "high", "max"],
                tiers: [],
              }));
              return row.models;
            }
            if (cmd === "select_profile") {
              selected = args.id;
              const row = profiles.find((row) => row.profile.id === args.id)!;
              if (args.model)
                row.profile = {
                  ...row.profile,
                  model: args.model,
                  effort: args.effort,
                  tier: args.tier,
                };
              return;
            }
            if (cmd === "create") {
              const id = crypto.randomUUID();
              chats.push({ id, title: args.title, project_id: null });
              messages[id] = [];
              return id;
            }
            if (cmd === "draft") {
              if (args.body !== null) drafts[args.conversation] = args.body;
              return drafts[args.conversation] ?? "";
            }
            if (cmd === "snapshot")
              return {
                messages: messages[args.conversation] ?? [],
                operation: null,
                draft: null,
              };
            if (cmd === "send") {
              const existing = messages[args.conversation] ?? [];
              const index = existing.length / 2 + 1;
              const user = {
                id: `${args.operation}-user`,
                parent: args.parent,
                sequence: existing.length + 1,
                role: "user",
                content: { text: args.text, attachments: [] },
                end: "complete",
              };
              const reply = {
                id: `${args.operation}-reply`,
                parent: user.id,
                sequence: existing.length + 2,
                role: "assistant",
                content: {
                  text:
                    index === 1
                      ? "Synthetic UI response"
                      : `Synthetic UI response ${index}`,
                  attachments: [],
                },
                end: "complete",
              };
              messages[args.conversation] = [...existing, user, reply];
              return reply;
            }
            return null;
          },
        },
      });
    },
    { failure: info.title.includes("unavailable native bridge") },
  );
  await page.goto("/");
});
test("professional shell, persisted appearance/language and modal keyboard focus", async ({
  page,
}) => {
  await expect(
    page.getByRole("heading", { name: "随时可以开始。" }),
  ).toBeVisible();
  const greeting = await page.locator(".welcome h1").boundingBox();
  const composer = await page.locator(".composer").boundingBox();
  expect(composer!.y).toBeGreaterThan(greeting!.y + greeting!.height);
  await page.screenshot({ path: "test-results/zh-dark-welcome.png" });
  await page.getByRole("button", { name: "设置", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("主题", { exact: true }).selectOption("light");
  await dialog.getByLabel("语言", { exact: true }).selectOption("en");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await expect(page.locator("html")).toHaveAttribute("lang", "en");
  await page.screenshot({ path: "test-results/en-light-settings.png" });
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(
    page.getByRole("heading", { name: "Ready when you are." }),
  ).toBeVisible();
  expect(
    await page
      .locator(".main")
      .evaluate((node) => getComputedStyle(node).backdropFilter),
  ).toContain("blur");
  await page.screenshot({ path: "test-results/en-light-welcome.png" });
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page
    .getByRole("dialog")
    .getByLabel("Language", { exact: true })
    .selectOption("zh");
  await page.getByRole("button", { name: "关闭", exact: true }).click();
  await page.setViewportSize({ width: 1148, height: 860 });
  await page.screenshot({ path: "test-results/reference-4-zh-light.png" });
  await expect(
    page.getByRole("button", { name: "工作", exact: true }),
  ).toBeDisabled();
  await page.setViewportSize({ width: 760, height: 560 });
  const compact = await page.locator(".composer").boundingBox();
  expect(compact!.y + compact!.height).toBeLessThanOrEqual(560);
  await page.screenshot({ path: "test-results/minimum-window-zh-light.png" });
  await page.getByRole("button", { name: "收起侧栏" }).click();

  await expect(page.getByRole("button", { name: "展开侧栏" })).toBeVisible();
});
test("model/key form is masked, IME does not submit, ordinary Chat does", async ({
  page,
}) => {
  await page.getByRole("button", { name: "配置模型", exact: true }).click();
  await page.getByRole("button", { name: "添加供应商" }).click();
  const key = page.getByLabel("API Key", { exact: true });
  await expect(key).toHaveAttribute("type", "password");
  await key.fill("synthetic-ui-secret");
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect(page.getByText("密钥已配置", { exact: true })).toBeVisible();
  await expect(page.locator("body")).not.toContainText("synthetic-ui-secret");
  await page.getByRole("button", { name: "关闭", exact: true }).click();
  await page.locator(".model-button").click();
  await page
    .getByRole("menuitem")
    .filter({ hasText: "OpenAI synthetic" })
    .first()
    .click();
  const input = page.getByRole("textbox", { name: "向 CAIdex 发送消息" });
  await input.fill("picker draft");
  await page.locator(".model-button").click();
  await page
    .getByRole("menuitem")
    .filter({ hasText: "OpenAI synthetic" })
    .first()
    .click();
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST_CALLS__.filter((c: any) => c.cmd === "send")
          .length,
    ),
  ).toBe(0);
  await input.fill("你好");
  await input.dispatchEvent("compositionstart");
  await input.press("Enter");
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST_CALLS__.filter((c: any) => c.cmd === "send")
          .length,
    ),
  ).toBe(0);
  await input.dispatchEvent("compositionend");
  await input.press("Enter");
  await expect(
    page.getByText("Synthetic UI response", { exact: true }),
  ).toBeVisible();
  await expect(page.locator(".user .message-content")).toHaveText("你好");
  await page.screenshot({ path: "test-results/zh-dark-chat.png" });
  expect(
    await page.evaluate(
      () =>
        (window as any).__TEST_CALLS__.filter((c: any) => c.cmd === "send")
          .length,
    ),
  ).toBe(1);
});
test("selecting an earlier reply creates a branch and switching shows its complete history", async ({
  page,
}) => {
  await page.getByRole("button", { name: "配置模型", exact: true }).click();
  await page.getByRole("button", { name: "添加供应商" }).click();
  await page.getByLabel("API Key", { exact: true }).fill("synthetic-ui-secret");
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect(page.getByText("密钥已配置", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "关闭", exact: true }).click();
  await page.locator(".model-button").click();
  await page
    .getByRole("menuitem")
    .filter({ hasText: "OpenAI synthetic" })
    .first()
    .click();
  const input = page.getByRole("textbox", { name: "向 CAIdex 发送消息" });
  await input.fill("first");
  await input.press("Enter");
  await expect(
    page.getByText("Synthetic UI response", { exact: true }),
  ).toBeVisible();
  await input.fill("second");
  await input.press("Enter");
  await expect(
    page.getByText("Synthetic UI response 2", { exact: true }),
  ).toBeVisible();
  await page.locator(".assistant .branch-button").first().click();
  await input.fill("third");
  await input.press("Enter");
  await expect(
    page.getByText("Synthetic UI response 3", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("Synthetic UI response 2", { exact: true }),
  ).toHaveCount(0);
  await page.locator(".user .branch-button").click();
  await expect(
    page.getByText("Synthetic UI response 2", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("Synthetic UI response 3", { exact: true }),
  ).toHaveCount(0);
  const calls = await page.evaluate(() =>
    (window as any).__TEST_CALLS__
      .filter((c: any) => c.cmd === "send")
      .map((c: any) => ({
        operation: c.args.operation,
        parent: c.args.parent,
      })),
  );
  expect(calls[2].parent).toBe(`${calls[0].operation}-reply`);
});
test("new-chat navigation retains drafts, search and Codex boundary are explicit", async ({
  page,
}) => {
  await page.getByRole("button", { name: "新建聊天" }).click();
  await page
    .getByRole("textbox", { name: "向 CAIdex 发送消息" })
    .fill("unsent draft");
  await page.keyboard.press("Control+n");
  const history = page.locator(".history-item");
  await expect(history).toHaveCount(2);
  await history.first().click();
  await expect(
    page.getByRole("textbox", { name: "向 CAIdex 发送消息" }),
  ).toHaveValue("unsent draft");
  await page.getByRole("button", { name: "搜索聊天", exact: true }).click();
  await page.getByRole("textbox", { name: "搜索聊天" }).fill("missing");
  await expect(page.getByText("没有匹配的聊天")).toBeVisible();
  await page.getByRole("button", { name: "切换模式" }).click();
  await page.getByRole("menuitem", { name: "Codex", exact: true }).click();
  await expect(page.getByText("Codex 将在后续阶段接入")).toBeVisible();
  await expect(
    page.getByRole("textbox", { name: "向 CAIdex 发送消息" }),
  ).toHaveCount(0);
});
test("unavailable native bridge shows a safe recoverable startup error", async ({
  page,
}) => {
  await expect(
    page.getByRole("heading", { name: "无法打开本地数据" }),
  ).toBeVisible();
  await expect(page.locator("body")).not.toContainText(
    "secret-bearing-platform-error",
  );
  await expect(page.getByRole("button", { name: "重试加载" })).toBeVisible();
});

test("provider key loads the catalog, composer selects models and effort without entering IDs", async ({
  page,
}) => {
  await page.getByRole("button", { name: "配置模型", exact: true }).click();
  await page.getByRole("button", { name: "添加供应商" }).click();
  await page.getByLabel("提供商", { exact: true }).selectOption("deepseek");
  await expect(page.getByLabel("模型 ID")).not.toBeVisible();
  await page.getByLabel("API Key", { exact: true }).fill("synthetic-ui-secret");
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect(page.getByText("2 个模型", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "关闭", exact: true }).click();
  await page.locator(".model-button").click();
  await page
    .getByRole("menuitem")
    .filter({ hasText: "deepseek-flash" })
    .click();
  await page.getByLabel("推理强度", { exact: true }).selectOption("low");
  await expect(page.getByLabel("推理强度", { exact: true })).toHaveValue("low");
  await page.locator(".model-button").click();
  await page.getByLabel("搜索模型", { exact: true }).fill("v4-pro");
  await page
    .getByRole("menuitem")
    .filter({ hasText: "deepseek-v4-pro" })
    .click();
  await expect(page.locator(".model-button")).toContainText("deepseek-v4-pro");
  await expect(page.getByLabel("推理强度", { exact: true })).toHaveValue("");
  await page.getByLabel("推理强度", { exact: true }).selectOption("high");
  const calls = await page.evaluate(() => (window as any).__TEST_CALLS__);
  expect(calls.filter((c: any) => c.cmd === "save_profile")).toHaveLength(1);
  expect(calls.filter((c: any) => c.cmd === "discover_models")).toHaveLength(1);
  expect(
    calls.filter((c: any) => c.cmd === "select_profile").at(-1).args,
  ).toMatchObject({ model: "deepseek-v4-pro", effort: "high" });
  expect(calls.filter((c: any) => c.cmd === "send")).toHaveLength(0);
  await page.screenshot({ path: "test-results/composer-model-effort.png" });
});
