import { readFile } from "node:fs/promises";
import { expect, test } from "@playwright/test";

for (const project of ["desktop", "mobile"]) {
  test(`${project} renders the generated workbook`, async ({ page }, testInfo) => {
    test.skip(testInfo.project.name !== project);
    await page.goto("/");

    await expect(page.getByTestId("runtime-status")).toContainText("WASM");
    await expect(page.getByTestId("runtime-status")).toContainText("Worker");
    const implementation = page.getByRole("navigation", { name: "Implementation" });
    await expect(implementation.getByRole("link", { name: ".NET" })).toHaveAttribute("href", "/MiniExcel/");
    await expect(implementation.getByRole("link", { name: "Rust" })).toHaveAttribute("href", "/MiniExcel-Rust/");
    await expect(implementation.getByRole("link", { name: "Rust" })).toHaveAttribute("aria-current", "page");
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