import { readFile } from "node:fs/promises";
import { expect, test } from "@playwright/test";

for (const project of ["desktop", "mobile", "mobile-narrow"]) {
  test(`${project} renders the generated workbook`, async ({ page }, testInfo) => {
    test.skip(testInfo.project.name !== project);
    await page.goto("/");

    await expect(page.getByTestId("runtime-status")).toContainText("WASM");
    await expect(page.getByTestId("runtime-status")).toContainText("Worker");
    const implementation = page.getByRole("navigation", { name: "Implementation" });
    await expect(implementation.getByRole("link", { name: ".NET" })).toHaveAttribute("href", "/MiniExcel/");
    await expect(implementation.getByRole("link", { name: "Rust" })).toHaveAttribute("href", "/MiniExcel-Rust/");
    await expect(implementation.getByRole("link", { name: "Rust" })).toHaveAttribute("aria-current", "page");
    const githubLink = page.getByRole("link", { name: "View MiniExcel Rust on GitHub" });
    await expect(githubLink).toHaveAttribute(
      "href",
      "https://github.com/mini-software/MiniExcel-Rust",
    );
    await expect(githubLink).toHaveAttribute("target", "_blank");
    await expect(githubLink).toHaveAttribute("rel", "noopener noreferrer");
    await expect(page.getByTestId("file-name")).toHaveText("miniexcel-browser-demo.xlsx");
    await expect(page.getByRole("cell", { name: "MiniExcel", exact: true })).toBeVisible();
    await expect(page.getByRole("cell", { name: "Browser WASM", exact: true })).toBeVisible();
    await expect(page.locator("#previewTable tbody tr")).toHaveCount(6);
    await expect
      .poll(() =>
        page.evaluate(
          () => document.documentElement.scrollWidth === document.documentElement.clientWidth,
        ),
      )
      .toBe(true);

    await page.screenshot({ path: testInfo.outputPath(`${project}.png`), fullPage: true });
  });
}

test("query controls refresh the worker preview", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("file-name")).toHaveText("miniexcel-browser-demo.xlsx");

  await page.getByLabel("Header row").uncheck();
  await page.getByRole("button", { name: "Refresh preview" }).click();

  await expect(page.locator("#previewTable thead th").filter({ hasText: /^A$/ })).toBeVisible();
  await expect(page.locator("#previewTable tbody tr")).toHaveCount(7);
});

test("end cell limits the preview range", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("file-name")).toHaveText("miniexcel-browser-demo.xlsx");

  await page.getByLabel("End cell").fill("B2");
  await page.getByRole("button", { name: "Refresh preview" }).click();

  await expect(page.locator("#previewTable thead th")).toHaveCount(3);
  await expect(page.locator("#previewTable tbody tr")).toHaveCount(1);
  await expect(page.getByRole("cell", { name: "MiniExcel", exact: true })).toBeVisible();
});

test("RAG export buttons wrap without clipping", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("file-name")).toHaveText("miniexcel-browser-demo.xlsx");

  await page.getByRole("tab", { name: "RAG", exact: true }).click();
  await expect(page.getByRole("button", { name: "Chunks JSONL" })).toBeVisible();

  const layout = await page.locator(".export-section .command-row-stack").evaluate((container) => {
    const buttons = [...container.querySelectorAll("button:not([hidden])")];
    const containerRect = container.getBoundingClientRect();
    const rects = buttons.map((button) => button.getBoundingClientRect());
    return {
      buttonCount: buttons.length,
      rowCount: new Set(rects.map((rect) => Math.round(rect.top))).size,
      maxRight: Math.max(...rects.map((rect) => rect.right)),
      containerRight: containerRect.right,
      scrollWidth: container.scrollWidth,
      clientWidth: container.clientWidth,
    };
  });

  expect(layout.buttonCount).toBe(5);
  expect(layout.rowCount).toBeGreaterThan(1);
  expect(layout.maxRight).toBeLessThanOrEqual(layout.containerRight + 1);
  expect(layout.scrollWidth).toBeLessThanOrEqual(layout.clientWidth + 1);
});

test("grouped analysis runs from the visual query plan", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("file-name")).toHaveText("miniexcel-browser-demo.xlsx");

  await page.getByRole("tab", { name: "Analyze" }).click();
  await expect(page.locator("#resultEyebrow")).toHaveText("Grouped analysis");
  await expect(page.locator("#metricRows")).toHaveText("4");
  await expect(page.locator("#resultNotice")).toContainText("4 of 6 rows matched");
  await expect(page.locator("#previewTable tbody tr")).toHaveCount(4);
  await expect(page.getByRole("cell", { name: "Core", exact: true })).toBeVisible();
  await expect(page.getByRole("cell", { name: "1200", exact: true })).toBeVisible();

  await page.getByRole("tab", { name: "JSON" }).click();
  await expect(page.locator("#jsonView")).toContainText("miniexcel.query-plan/v1");
});

test("RAG mode downloads valid JSONL, Markdown chunks, and manifest", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("file-name")).toHaveText("miniexcel-browser-demo.xlsx");

  await page.getByRole("tab", { name: "RAG", exact: true }).click();
  await expect(page.locator("#resultEyebrow")).toHaveText("RAG evidence");
  await expect(page.locator("#metricRows")).toHaveText("1");
  await expect(page.locator("#metricColumns")).toHaveText("6");

  const chunksPromise = page.waitForEvent("download");
  await page.getByRole("button", { name: "Chunks JSONL" }).click();
  const chunksDownload = await chunksPromise;
  expect(chunksDownload.suggestedFilename()).toBe("miniexcel-browser-demo.chunks.jsonl");
  const chunksText = await readFile(await chunksDownload.path(), "utf8");
  const chunks = chunksText.trim().split("\n").map((line) => JSON.parse(line));
  expect(chunks).toHaveLength(1);
  expect(chunks[0].version).toBe("miniexcel.rag-chunk/v1");
  expect(chunks[0].rows).toHaveLength(6);
  expect(chunks[0].header.cells[0].address).toBe("A1");

  await page.getByRole("tablist", { name: "Result format" }).getByRole("tab", { name: "Markdown" }).click();
  await expect(page.locator("#markdownView")).toContainText("<!-- miniexcel:stream-start");
  await expect(page.locator("#markdownView")).toContainText("| Source file | miniexcel-browser-demo.xlsx |");
  await expect(page.locator("#markdownView")).toContainText("| Worksheet visibility | visible |");
  await expect(page.locator("#markdownView")).toContainText("<!-- miniexcel:chunk-start");
  await expect(page.locator("#markdownView")).toContainText("| _row | Name | Category | Region |");
  await expect(page.locator("#markdownView")).toContainText("<!-- miniexcel:stream-end");

  const markdownPromise = page.waitForEvent("download");
  await page.getByRole("button", { name: "Chunks Markdown" }).click();
  const markdownDownload = await markdownPromise;
  expect(markdownDownload.suggestedFilename()).toBe("miniexcel-browser-demo.chunks.md");
  const markdown = await readFile(await markdownDownload.path(), "utf8");
  expect(markdown).toContain("<!-- miniexcel:stream-start");
  expect(markdown).toContain("| Source SHA-256 | ");
  expect(markdown).toContain("<!-- miniexcel:chunk-start");
  expect(markdown).toContain("| _row | Name | Category | Region |");
  expect(markdown).toContain("<!-- miniexcel:stream-end");

  const manifestPromise = page.waitForEvent("download");
  await page.getByRole("button", { name: "Manifest JSON" }).click();
  const manifestDownload = await manifestPromise;
  const manifest = JSON.parse(await readFile(await manifestDownload.path(), "utf8"));
  expect(manifest.version).toBe("miniexcel.rag-manifest/v1");
  expect(manifest.emittedRows).toBe(6);
  expect(manifest.sourceSha256).toMatch(/^[a-f0-9]{64}$/);
});

for (const project of ["desktop", "mobile"]) {
  test(`${project} converts the workbook to both Markdown formats`, async ({ page }, testInfo) => {
    test.skip(testInfo.project.name !== project);
    await page.goto("/");
    await expect(page.getByTestId("file-name")).toHaveText("miniexcel-browser-demo.xlsx");

    await page.getByRole("tablist", { name: "Workflow mode" }).getByRole("tab", { name: "Markdown" }).click();
    await expect(page.locator("#resultEyebrow")).toHaveText("Markdown conversion");
    await expect(page.locator("#markdownView")).toContainText("| Name | Category | Region |");
    await expect(page.locator("#markdownView")).toContainText("| MiniExcel | Core | East |");
    await expect(page.locator("#markdownView")).not.toContainText("miniexcel:chunk-start");

    const simplePromise = page.waitForEvent("download");
    await page.locator("#downloadMarkdownButton").click();
    const simpleDownload = await simplePromise;
    expect(simpleDownload.suggestedFilename()).toBe("miniexcel-browser-demo.simple.md");
    const simple = await readFile(await simpleDownload.path(), "utf8");
    expect(simple).toContain("| Name | Category | Region |");
    expect(simple).not.toContain("miniexcel:stream-start");

    await page.getByRole("radio", { name: "LLM-friendly" }).click();
    await expect(page.locator("#markdownView")).toContainText("<!-- miniexcel:stream-start");
    await expect(page.locator("#markdownView")).toContainText("| Source file | miniexcel-browser-demo.xlsx |");
    await expect(page.locator("#markdownView")).toContainText("<!-- miniexcel:chunk-start");
    await expect(page.locator("#markdownView")).toContainText("<!-- miniexcel:stream-end");

    const llmPromise = page.waitForEvent("download");
    await page.locator("#downloadMarkdownButton").click();
    const llmDownload = await llmPromise;
    expect(llmDownload.suggestedFilename()).toBe("miniexcel-browser-demo.llm-friendly.md");
    const llm = await readFile(await llmDownload.path(), "utf8");
    expect(llm).toContain("<!-- miniexcel:stream-start");
    expect(llm).toContain("<!-- miniexcel:stream-end");

    await expect
      .poll(() =>
        page.evaluate(
          () => document.documentElement.scrollWidth === document.documentElement.clientWidth,
        ),
      )
      .toBe(true);
    await page.screenshot({ path: testInfo.outputPath(`${project}-markdown.png`), fullPage: true });
  });
}

test("uploaded workbook shows metadata and requires hidden-sheet RAG opt-in", async ({ page }) => {
  await page.goto("/");
  await page.locator("#fileInput").setInputFiles(
    "../tests/data/xlsx/TestMultiSheetWithHiddenSheet.xlsx",
  );

  await expect(page.getByTestId("file-name")).toHaveText("TestMultiSheetWithHiddenSheet.xlsx");
  await expect(page.locator("#sheetCount")).toHaveText("4");
  await expect(page.locator("#sheetSelect option", { hasText: "HiddenSheet4 (hidden)" })).toHaveCount(1);
  await page.locator("#sheetSelect").selectOption("HiddenSheet4");
  await expect(page.locator("#previewTitle")).toContainText("HiddenSheet4");

  await page.getByRole("tab", { name: "RAG", exact: true }).click();
  await expect(page.locator("#allowHiddenToggle")).toBeVisible();
  await expect(page.getByRole("button", { name: "Build RAG export" })).toBeDisabled();
  await page.locator("#allowHiddenToggle").check();
  await expect(page.getByRole("button", { name: "Build RAG export" })).toBeEnabled();
  await page.getByRole("button", { name: "Build RAG export" }).click();
  await expect(page.locator("#resultEyebrow")).toHaveText("RAG evidence");

  await page.getByRole("tablist", { name: "Workflow mode" }).getByRole("tab", { name: "Markdown" }).click();
  await expect(page.locator("#markdownAllowHiddenToggle")).toBeVisible();
  await expect(page.getByRole("button", { name: "Convert to Markdown" })).toBeDisabled();
  await page.locator("#markdownAllowHiddenToggle").check();
  await expect(page.getByRole("button", { name: "Convert to Markdown" })).toBeEnabled();
  await page.getByRole("button", { name: "Convert to Markdown" }).click();
  await expect(page.locator("#resultEyebrow")).toHaveText("Markdown conversion");

  const visibleSheet = await page.locator("#sheetSelect option").first().getAttribute("value");
  await page.locator("#sheetSelect").selectOption(visibleSheet);
  await expect(page.locator("#previewTitle")).toContainText(visibleSheet);
  await expect(page.locator("#resultEyebrow")).toHaveText("Markdown conversion");
  await expect(page.locator("#downloadMarkdownButton")).toBeEnabled();
});

const LAYOUT_KEY = "miniexcel.browser-lab.layout/v1";

const readLayout = (page) =>
  page.evaluate((key) => {
    const raw = window.localStorage.getItem(key);
    return raw ? JSON.parse(raw) : null;
  }, LAYOUT_KEY);

const seedLayout = (page, layout) =>
  page.evaluate(
    ([key, value]) => window.localStorage.setItem(key, value),
    [LAYOUT_KEY, JSON.stringify(layout)],
  );

const hasNoHorizontalOverflow = (page) =>
  page.evaluate(
    () => document.documentElement.scrollWidth === document.documentElement.clientWidth,
  );

const railWidth = async (page) => {
  const box = await page.locator("#controlRail").boundingBox();
  return Math.round(box.width);
};

const panelWidth = async (page, selector) => {
  const box = await page.locator(selector).boundingBox();
  return Math.round(box.width);
};

test("control rail toggle hides and restores the panel across reloads", async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== "desktop", "Two-column layout is desktop-only");
  await page.goto("/");
  const rail = page.locator("#controlRail");
  const toggle = page.getByTestId("rail-toggle");
  await expect(page.getByTestId("file-name")).toHaveText("miniexcel-browser-demo.xlsx");

  await expect(rail).toBeVisible();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await expect(toggle).toHaveAttribute("title", "Hide workbook controls");
  const expandedPreview = await panelWidth(page, ".preview-panel");

  await toggle.click();

  await expect(rail).toBeHidden();
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await expect(toggle).toHaveAttribute("title", "Show workbook controls");
  expect(await panelWidth(page, ".preview-panel")).toBeGreaterThan(expandedPreview);
  expect(await readLayout(page)).toEqual({ width: 380, collapsed: true });
  await expect.poll(() => hasNoHorizontalOverflow(page)).toBe(true);

  await page.reload();

  await expect(page.getByTestId("file-name")).toHaveText("miniexcel-browser-demo.xlsx");
  await expect(page.locator("#controlRail")).toBeHidden();
  await expect(page.getByTestId("rail-toggle")).toHaveAttribute("aria-expanded", "false");

  await page.getByTestId("rail-toggle").click();

  await expect(page.locator("#controlRail")).toBeVisible();
  await expect(page.getByTestId("rail-toggle")).toHaveAttribute("aria-expanded", "true");
  await expect.poll(() => railWidth(page)).toBe(380);
  expect(await readLayout(page)).toEqual({ width: 380, collapsed: false });

  await page.reload();
  await expect(page.locator("#controlRail")).toBeVisible();
});

const invalidLayouts = [
  { width: "wide", collapsed: "yes" },
  { width: 0, collapsed: true },
  { width: 900, collapsed: true },
  { width: null, collapsed: false },
  { width: 380, collapsed: "yes" },
  { width: 380 },
  { collapsed: true },
  [],
  "380",
];

test("invalid stored layouts fall back to the expanded default", async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== "desktop", "Two-column layout is desktop-only");
  await page.goto("/");

  for (const invalid of invalidLayouts) {
    const payload = JSON.stringify(invalid);
    await seedLayout(page, invalid);
    await page.reload();

    await expect(page.getByTestId("file-name")).toHaveText("miniexcel-browser-demo.xlsx");
    await expect(page.locator("#controlRail"), payload).toBeVisible();
    await expect(page.getByTestId("rail-toggle"), payload).toHaveAttribute("aria-expanded", "true");
    expect(await railWidth(page), payload).toBe(380);

    await page.getByTestId("rail-toggle").click();
    await expect.poll(() => readLayout(page), { message: payload }).toEqual({ width: 380, collapsed: true });
    await page.getByTestId("rail-toggle").click();
    await expect.poll(() => railWidth(page), { message: payload }).toBe(380);
  }
});

test("splitter drag resizes the control rail and persists the width", async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== "desktop", "The splitter only exists in the two-column layout");
  await page.goto("/");
  const splitter = page.getByTestId("rail-splitter");
  await expect(page.getByTestId("file-name")).toHaveText("miniexcel-browser-demo.xlsx");
  await expect(splitter).toBeVisible();
  expect(await railWidth(page)).toBe(380);

  const start = await splitter.boundingBox();
  const handleY = start.y + start.height / 2;
  await page.mouse.move(start.x + start.width / 2, handleY);
  await page.mouse.down();
  await page.mouse.move(start.x + start.width / 2 + 160, handleY, { steps: 10 });
  await page.mouse.up();

  const dragged = await railWidth(page);
  expect(dragged).toBeGreaterThan(520);
  expect(dragged).toBeLessThanOrEqual(560);
  await expect.poll(async () => (await readLayout(page)).width).toBe(dragged);
  expect(await panelWidth(page, ".preview-panel")).toBeGreaterThan(320);

  await page.reload();
  await expect.poll(() => railWidth(page)).toBe(dragged);

  const wideHandle = await page.getByTestId("rail-splitter").boundingBox();
  await page.mouse.move(wideHandle.x + wideHandle.width / 2, handleY);
  await page.mouse.down();
  await page.mouse.move(wideHandle.x + 2000, handleY, { steps: 10 });
  await page.mouse.up();

  const clamped = await railWidth(page);
  expect(clamped).toBeLessThanOrEqual(720);
  expect(clamped).toBeGreaterThan(600);
  expect(await panelWidth(page, ".preview-panel")).toBeGreaterThanOrEqual(320);
  await expect.poll(() => hasNoHorizontalOverflow(page)).toBe(true);

  const resetHandle = await page.getByTestId("rail-splitter").boundingBox();
  await page.mouse.dblclick(resetHandle.x + resetHandle.width / 2, handleY);

  await expect.poll(() => railWidth(page)).toBe(380);
  await expect.poll(async () => (await readLayout(page)).width).toBe(380);

  await splitter.focus();
  await page.keyboard.press("ArrowLeft");

  await expect.poll(() => railWidth(page)).toBe(364);
  await expect.poll(async () => (await readLayout(page)).width).toBe(364);

  await page.reload();
  await expect.poll(() => railWidth(page)).toBe(364);
});

test("splitter drag keeps the resize cursor over interactive descendants", async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== "desktop", "The splitter only exists in the two-column layout");
  await page.goto("/");
  await expect(page.getByTestId("file-name")).toHaveText("miniexcel-browser-demo.xlsx");

  const probe = page.locator("#rowsModeButton");
  const cursorOf = (locator) => locator.evaluate((element) => getComputedStyle(element).cursor);
  await expect.poll(() => cursorOf(probe)).toBe("pointer");

  const splitter = page.getByTestId("rail-splitter");
  const handle = await splitter.boundingBox();
  const handleX = handle.x + handle.width / 2;
  const handleY = handle.y + handle.height / 2;
  const dragX = handleX + 120;

  await page.mouse.move(handleX, handleY);
  await page.mouse.down();
  await page.mouse.move(dragX, handleY, { steps: 6 });

  const dragging = await page.evaluate(
    ([x, y, selector]) => {
      const underPointer = document.elementFromPoint(x, y);
      return {
        isResizing: document.body.classList.contains("is-resizing"),
        bodyCursor: getComputedStyle(document.body).cursor,
        bodyUserSelect: getComputedStyle(document.body).userSelect,
        probeCursor: getComputedStyle(document.querySelector(selector)).cursor,
        probeUserSelect: getComputedStyle(document.querySelector(selector)).userSelect,
        underPointerCursor: underPointer ? getComputedStyle(underPointer).cursor : null,
      };
    },
    [dragX, handleY, "#rowsModeButton"],
  );

  expect(dragging.isResizing).toBe(true);
  expect(dragging.bodyCursor).toBe("col-resize");
  expect(dragging.bodyUserSelect).toBe("none");
  expect(dragging.probeCursor).toBe("col-resize");
  expect(dragging.probeUserSelect).toBe("none");
  expect(dragging.underPointerCursor).toBe("col-resize");
  expect(await railWidth(page)).toBeGreaterThan(380);

  await page.mouse.up();

  await expect.poll(() => page.evaluate(() => document.body.classList.contains("is-resizing"))).toBe(false);
  await expect.poll(() => cursorOf(probe)).toBe("pointer");
  await expect.poll(() => cursorOf(page.getByTestId("rail-splitter"))).toBe("col-resize");
});

test("splitter keyboard bounds update ARIA state and persist across reloads", async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== "desktop", "The splitter only exists in the two-column layout");
  await page.goto("/");
  await expect(page.getByTestId("file-name")).toHaveText("miniexcel-browser-demo.xlsx");

  const splitter = page.getByTestId("rail-splitter");
  await expect(splitter).toHaveAttribute("aria-valuemin", "260");
  await expect(splitter).toHaveAttribute("aria-valuemax", "720");
  await splitter.focus();

  await page.keyboard.press("Home");
  await expect.poll(() => railWidth(page)).toBe(260);
  await expect(splitter).toHaveAttribute("aria-valuenow", "260");
  await expect.poll(async () => (await readLayout(page)).width).toBe(260);

  await page.keyboard.press("End");
  await expect.poll(() => railWidth(page)).toBe(720);
  await expect(splitter).toHaveAttribute("aria-valuenow", "720");
  await expect.poll(async () => (await readLayout(page)).width).toBe(720);

  await page.reload();
  await expect.poll(() => railWidth(page)).toBe(720);
  await expect(page.getByTestId("rail-splitter")).toHaveAttribute("aria-valuenow", "720");
});

for (const project of ["mobile", "mobile-narrow"]) {
  test(`${project} toggles the stacked control rail without horizontal overflow`, async ({ page }, testInfo) => {
    test.skip(testInfo.project.name !== project);
    await page.goto("/");
    await expect(page.getByTestId("file-name")).toHaveText("miniexcel-browser-demo.xlsx");
    await expect(page.getByTestId("rail-splitter")).toBeHidden();

    await seedLayout(page, { width: 720, collapsed: false });
    await page.reload();

    await expect(page.locator("#controlRail")).toBeVisible();
    await expect.poll(() => hasNoHorizontalOverflow(page)).toBe(true);

    await page.getByTestId("rail-toggle").click();

    await expect(page.locator("#controlRail")).toBeHidden();
    await expect.poll(() => hasNoHorizontalOverflow(page)).toBe(true);

    await page.reload();

    await expect(page.locator("#controlRail")).toBeHidden();
    expect((await readLayout(page)).collapsed).toBe(true);
    await expect.poll(() => hasNoHorizontalOverflow(page)).toBe(true);

    await page.getByTestId("rail-toggle").click();
    await expect(page.locator("#controlRail")).toBeVisible();
    await expect.poll(() => hasNoHorizontalOverflow(page)).toBe(true);
  });
}
