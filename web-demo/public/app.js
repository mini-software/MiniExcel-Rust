const MAX_FILE_SIZE = 64 * 1024 * 1024;
const worker = new Worker(new URL("./analysis-worker.js", import.meta.url), { type: "module" });
let nextRequestId = 1;
const pendingRequests = new Map();

const state = {
  fileName: "",
  fileSize: 0,
  mode: "rows",
  result: null,
  ragResult: null,
  activeTab: "grid",
  sheetInfo: [],
  columns: [],
  columnTypes: new Map(),
  builderInitialized: false,
  toastTimer: null,
};

const elements = Object.fromEntries(
  [
    "runtimeStatus", "fileInput", "openFileButton", "loadDemoButton", "downloadDemoButton",
    "downloadJsonButton", "downloadChunksButton", "downloadMarkdownChunksButton",
    "downloadManifestButton", "dropZone",
    "fileName", "fileSize", "sheetCount", "sheetSelect", "startCellInput", "endCellInput",
    "rowLimitInput", "headerToggle", "emptyRowsToggle", "refreshButton", "rowsModeButton",
    "analyzeModeButton", "ragModeButton", "rowsControls", "analyzeControls", "ragControls",
    "conditionJoin", "conditionsList", "addConditionButton", "groupBySelect", "aggregatesList",
    "addAggregateButton", "maxGroupsInput", "analysisLimitInput", "runAnalysisButton",
    "chunkRowsInput", "ragMaxRowsInput", "hiddenSheetOptIn", "allowHiddenToggle", "runRagButton",
    "memoryModeText", "resultEyebrow", "previewTitle", "metricRows", "metricRowsLabel",
    "metricColumns", "metricColumnsLabel", "metricTime", "resultNotice", "loadingState",
    "emptyState", "gridView", "jsonView", "markdownView", "previewTable", "previewTab",
    "jsonTab", "markdownTab", "toast",
  ].map((id) => [id, document.getElementById(id)]),
);

worker.addEventListener("message", (event) => {
  const request = pendingRequests.get(event.data.id);
  if (!request) return;
  pendingRequests.delete(event.data.id);
  if (event.data.ok) request.resolve(event.data.result);
  else request.reject(new Error(event.data.error));
});
worker.addEventListener("error", (event) => {
  for (const request of pendingRequests.values()) request.reject(event.error ?? new Error(event.message));
  pendingRequests.clear();
});

bindEvents();
boot();

async function boot() {
  try {
    const runtime = await requestWorker("initialize");
    elements.runtimeStatus.textContent = `Rust WASM v${runtime.wasmVersion} · Worker`;
    elements.runtimeStatus.classList.add("is-ready");
    await loadDemo();
  } catch (error) {
    fail(error, "WebAssembly worker could not start");
  }
}

function bindEvents() {
  elements.openFileButton.addEventListener("click", () => elements.fileInput.click());
  elements.dropZone.addEventListener("click", () => elements.fileInput.click());
  elements.dropZone.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      elements.fileInput.click();
    }
  });
  elements.fileInput.addEventListener("change", async () => {
    const [file] = elements.fileInput.files;
    if (file) await loadFile(file);
    elements.fileInput.value = "";
  });
  for (const eventName of ["dragenter", "dragover"]) {
    elements.dropZone.addEventListener(eventName, (event) => {
      event.preventDefault();
      elements.dropZone.classList.add("is-dragging");
    });
  }
  for (const eventName of ["dragleave", "drop"]) {
    elements.dropZone.addEventListener(eventName, (event) => {
      event.preventDefault();
      elements.dropZone.classList.remove("is-dragging");
    });
  }
  elements.dropZone.addEventListener("drop", async (event) => {
    const [file] = event.dataTransfer.files;
    if (file) await loadFile(file);
  });

  elements.loadDemoButton.addEventListener("click", loadDemo);
  elements.downloadDemoButton.addEventListener("click", downloadDemo);
  elements.downloadJsonButton.addEventListener("click", downloadResultJson);
  elements.downloadChunksButton.addEventListener("click", downloadChunks);
  elements.downloadMarkdownChunksButton.addEventListener("click", downloadMarkdownChunks);
  elements.downloadManifestButton.addEventListener("click", downloadManifest);
  elements.refreshButton.addEventListener("click", runCurrentWorkflow);
  elements.runAnalysisButton.addEventListener("click", runAnalysis);
  elements.runRagButton.addEventListener("click", runRagExport);
  elements.rowsModeButton.addEventListener("click", () => setMode("rows"));
  elements.analyzeModeButton.addEventListener("click", () => setMode("analyze"));
  elements.ragModeButton.addEventListener("click", () => setMode("rag"));
  elements.sheetSelect.addEventListener("change", handleSheetChange);
  elements.headerToggle.addEventListener("change", runCurrentWorkflow);
  elements.emptyRowsToggle.addEventListener("change", runCurrentWorkflow);
  elements.allowHiddenToggle.addEventListener("change", () => {
    elements.runRagButton.disabled = isSelectedSheetHidden() && !elements.allowHiddenToggle.checked;
  });
  for (const input of [elements.startCellInput, elements.endCellInput, elements.rowLimitInput]) {
    input.addEventListener("keydown", (event) => {
      if (event.key === "Enter") runCurrentWorkflow();
    });
  }
  elements.addConditionButton.addEventListener("click", () => addCondition());
  elements.addAggregateButton.addEventListener("click", () => addAggregate());
  elements.previewTab.addEventListener("click", () => setTab("grid"));
  elements.jsonTab.addEventListener("click", () => setTab("json"));
  elements.markdownTab.addEventListener("click", () => setTab("markdown"));
}

async function loadFile(file) {
  if (!file.name.toLowerCase().endsWith(".xlsx")) {
    showToast("Choose an .xlsx workbook.", true);
    return;
  }
  if (file.size > MAX_FILE_SIZE) {
    showToast(`File exceeds the ${formatBytes(MAX_FILE_SIZE)} browser limit.`, true);
    return;
  }
  setLoading(`Transferring ${file.name} to the worker...`);
  try {
    const buffer = await file.arrayBuffer();
    await requestWorker("loadWorkbook", { buffer }, [buffer]);
    await setWorkbook(file.name, file.size);
  } catch (error) {
    fail(error, "Workbook could not be read");
  }
}

async function loadDemo() {
  setLoading("Generating workbook in the Rust worker...");
  try {
    const result = await requestWorker("loadDemo");
    await setWorkbook("miniexcel-browser-demo.xlsx", result.byteLength);
  } catch (error) {
    fail(error, "Demo workbook could not be generated");
  }
}

async function setWorkbook(name, size) {
  state.fileName = name;
  state.fileSize = size;
  state.result = null;
  state.ragResult = null;
  state.builderInitialized = false;
  state.columns = [];
  elements.fileName.textContent = name;
  elements.fileSize.textContent = formatBytes(size);
  elements.sheetCount.textContent = "—";
  elements.sheetSelect.replaceChildren(new Option("First worksheet", ""));
  elements.sheetSelect.disabled = true;
  setMode("rows", false);
  await refreshRows(true);
}

async function runCurrentWorkflow() {
  if (state.mode === "analyze") return runAnalysis();
  if (state.mode === "rag") return runRagExport();
  return refreshRows(false);
}

async function refreshRows(resetSheet) {
  if (!state.fileName) return;
  let options;
  try {
    options = readOptions(resetSheet);
  } catch (error) {
    showValidationError(error);
    return;
  }
  setLoading("Scanning rows in the WebAssembly worker...");
  const started = performance.now();
  try {
    const result = await requestWorker("preview", { options });
    state.sheetInfo = result.sheetInfo;
    state.columns = result.columns;
    updateColumnTypes(result);
    updateSheets(result.sheetInfo, result.selectedSheet);
    updateBuilders();
    elements.sheetCount.textContent = String(result.sheetInfo.length);
    const raw = result.rows.map((values) =>
      Object.fromEntries(result.columns.map((column, index) => [column, values[index]])),
    );
    renderResult(
      { columns: result.columns, rows: result.rows, cellTypes: result.cellTypes },
      raw,
      performance.now() - started,
      {
        eyebrow: "Selected rows",
        title: `${state.fileName} · ${result.selectedSheet || "No worksheet"}`,
        rows: result.totalRows,
        columns: result.columns.length,
        notice: result.truncated ? `Showing ${result.displayedRows} of ${result.totalRows}` : `${result.displayedRows} rows`,
      },
    );
  } catch (error) {
    fail(error, "Row query failed");
  }
}

async function runAnalysis() {
  let options;
  let plan;
  try {
    options = readOptions(false);
    plan = buildPlan();
  } catch (error) {
    showValidationError(error);
    return;
  }
  setLoading("Aggregating rows in the WebAssembly worker...");
  const started = performance.now();
  try {
    const result = await requestWorker("analyze", { options, plan });
    const stats = result.stats;
    renderResult(
      {
        columns: result.columns,
        rows: result.rows.map((row) => row.values),
        cellTypes: result.rows.map((row) => row.cellTypes),
      },
      result,
      performance.now() - started,
      {
        eyebrow: "Grouped analysis",
        title: `${result.selectedSheet} · ${plan.groupBy.length ? plan.groupBy.join(" + ") : "All rows"}`,
        rows: stats.totalGroups,
        columns: result.columns.length,
        rowsLabel: "groups",
        notice: `${stats.matchedRows} of ${stats.seenRows} rows matched${stats.truncated ? " · result limited" : ""}`,
      },
    );
  } catch (error) {
    fail(error, "Analysis failed");
  }
}

async function runRagExport() {
  let options;
  let exportOptions;
  try {
    options = readOptions(false);
    const chunkRows = boundedInteger(elements.chunkRowsInput, 1, 500, "Rows per chunk");
    const maxRows = boundedInteger(elements.ragMaxRowsInput, 1, 100000, "RAG max rows");
    if (isSelectedSheetHidden() && !elements.allowHiddenToggle.checked) {
      throw new Error("Enable the hidden-sheet opt-in before exporting this worksheet.");
    }
    exportOptions = {
      chunkRows,
      maxRows,
      allowHiddenSheets: elements.allowHiddenToggle.checked,
      sourceName: state.fileName,
    };
  } catch (error) {
    showValidationError(error);
    return;
  }
  setLoading("Building addressed RAG chunks in the WebAssembly worker...");
  const started = performance.now();
  try {
    const result = await requestWorker("exportRag", { options, exportOptions });
    state.ragResult = result;
    const rows = result.chunks.map((chunk) => [
      chunk.chunkId,
      chunk.dataRange,
      chunk.rows.length,
      chunk.rows.reduce((total, row) => total + row.cells.length, 0),
      chunk.rows.flatMap((row) => row.cells.slice(0, 2).map((cell) => cell.address)).join(", "),
    ]);
    const cellTypes = rows.map(() => ["string", "string", "integer", "integer", "string"]);
    renderResult(
      { columns: ["Chunk", "Range", "Rows", "Cells", "Evidence"], rows, cellTypes },
      result,
      performance.now() - started,
      {
        eyebrow: "RAG evidence",
        title: `${result.manifest.sheetName} · ${result.manifest.sourceSha256.slice(0, 12)}`,
        rows: result.manifest.emittedChunks,
        columns: result.manifest.emittedRows,
        rowsLabel: "chunks",
        columnsLabel: "source rows",
        notice: `${result.manifest.approximateTokens} estimated tokens${result.manifest.truncated ? " · truncated" : ""}`,
      },
    );
    updateExportButtons();
  } catch (error) {
    fail(error, "RAG export failed");
  }
}

function readOptions(resetSheet) {
  const startCell = elements.startCellInput.value.trim().toUpperCase();
  const endCell = elements.endCellInput.value.trim().toUpperCase();
  if (!/^\$?[A-Z]{1,3}\$?[1-9]\d*$/.test(startCell)) {
    elements.startCellInput.focus();
    throw new Error("Start cell must use A1 notation, for example B2.");
  }
  if (endCell && !/^\$?[A-Z]{1,3}\$?[1-9]\d*$/.test(endCell)) {
    elements.endCellInput.focus();
    throw new Error("End cell must use A1 notation, for example E20.");
  }
  return {
    sheetName: resetSheet ? null : elements.sheetSelect.value || null,
    hasHeader: elements.headerToggle.checked,
    startCell,
    endCell: endCell || null,
    ignoreEmptyRows: elements.emptyRowsToggle.checked,
    limit: boundedInteger(elements.rowLimitInput, 1, 2000, "Preview limit"),
  };
}

function buildPlan() {
  const expressions = [...elements.conditionsList.querySelectorAll(".condition-row")].map((row) => {
    const column = row.querySelector("[data-role=column]").value;
    const op = row.querySelector("[data-role=operator]").value;
    if (!column) throw new Error("Every condition needs a column.");
    if (op === "isEmpty" || op === "isNotEmpty") return { kind: op, column };
    const input = row.querySelector("[data-role=value]");
    return { kind: "compare", column, op, value: queryLiteral(column, op, input.value) };
  });
  const aggregates = [...elements.aggregatesList.querySelectorAll(".aggregate-row")].map((row) => {
    const op = row.querySelector("[data-role=aggregate]").value;
    const selectedColumn = row.querySelector("[data-role=column]").value;
    const alias = row.querySelector("[data-role=alias]").value.trim();
    if (!alias) throw new Error("Every aggregate needs an alias.");
    const column = op === "count" && selectedColumn === "*" ? null : selectedColumn;
    if (!column && op !== "count") throw new Error(`${op} requires a column.`);
    return { op, column, alias };
  });
  if (!aggregates.length) throw new Error("Add at least one aggregate.");
  const groupBy = [...elements.groupBySelect.selectedOptions].map((option) => option.value);
  const filter = expressions.length === 0
    ? null
    : expressions.length === 1
      ? expressions[0]
      : { kind: elements.conditionJoin.value, expressions };
  return {
    version: "miniexcel.query-plan/v1",
    filter,
    groupBy,
    aggregates,
    maxGroups: boundedInteger(elements.maxGroupsInput, 1, 1000000, "Max groups"),
    limit: boundedInteger(elements.analysisLimitInput, 1, 100000, "Result limit"),
    evidenceRowsPerGroup: 5,
  };
}

function queryLiteral(column, op, input) {
  if (op === "contains") return { type: "string", value: input };
  const type = state.columnTypes.get(column) ?? "string";
  if (type === "boolean") {
    if (!/^(true|false)$/i.test(input)) throw new Error(`${column} expects true or false.`);
    return { type: "bool", value: input.toLowerCase() === "true" };
  }
  if (type === "integer") {
    const value = Number.parseInt(input, 10);
    if (!Number.isSafeInteger(value)) throw new Error(`${column} expects a safe integer.`);
    return { type: "int", value };
  }
  if (type === "number") {
    const value = Number(input);
    if (!Number.isFinite(value)) throw new Error(`${column} expects a number.`);
    return { type: "float", value };
  }
  const literalType = { date: "date", time: "time", datetime: "dateTime" }[type] ?? "string";
  return { type: literalType, value: input };
}

function setMode(mode, refresh = true) {
  state.mode = mode;
  for (const [name, button] of [
    ["rows", elements.rowsModeButton], ["analyze", elements.analyzeModeButton], ["rag", elements.ragModeButton],
  ]) {
    const active = mode === name;
    button.classList.toggle("is-active", active);
    button.setAttribute("aria-selected", String(active));
  }
  elements.rowsControls.hidden = mode !== "rows";
  elements.analyzeControls.hidden = mode !== "analyze";
  elements.ragControls.hidden = mode !== "rag";
  elements.markdownTab.hidden = mode !== "rag";
  elements.downloadChunksButton.hidden = mode !== "rag";
  elements.downloadMarkdownChunksButton.hidden = mode !== "rag";
  elements.downloadManifestButton.hidden = mode !== "rag";
  elements.memoryModeText.textContent = {
    rows: "Worker · bounded row preview",
    analyze: "Worker · memory capped by max groups",
    rag: "Worker · addressed JSONL + Markdown chunks",
  }[mode];
  updateHiddenSheetControl();
  updateExportButtons();
  if (refresh && state.fileName) runCurrentWorkflow();
}

async function handleSheetChange() {
  elements.allowHiddenToggle.checked = false;
  updateHiddenSheetControl();
  if (state.mode === "rows") return refreshRows(false);
  await refreshRows(false);
  setMode(state.mode, false);
}

function updateSheets(sheetInfo, selectedSheet) {
  elements.sheetSelect.replaceChildren();
  for (const sheet of sheetInfo) {
    const suffix = sheet.visibility === "visible" ? "" : ` (${sheet.visibility})`;
    elements.sheetSelect.add(new Option(`${sheet.name}${suffix}`, sheet.name));
  }
  elements.sheetSelect.value = selectedSheet || sheetInfo[0]?.name || "";
  elements.sheetSelect.disabled = sheetInfo.length === 0;
  updateHiddenSheetControl();
}

function updateColumnTypes(result) {
  state.columnTypes.clear();
  result.columns.forEach((column, columnIndex) => {
    const type = result.cellTypes.map((row) => row[columnIndex]).find((value) => value !== "empty");
    state.columnTypes.set(column, type ?? "string");
  });
}

function updateBuilders() {
  updateSelectOptions(elements.groupBySelect, state.columns, true);
  for (const select of elements.conditionsList.querySelectorAll("[data-role=column]")) {
    updateSelectOptions(select, state.columns);
  }
  for (const select of elements.aggregatesList.querySelectorAll("[data-role=column]")) {
    updateSelectOptions(select, ["*", ...state.columns]);
  }
  if (!state.builderInitialized && state.columns.length) {
    state.builderInitialized = true;
    addCondition({ column: state.columns.includes("Status") ? "Status" : state.columns[0], value: state.columns.includes("Status") ? "Ready" : "" });
    const group = state.columns.includes("Category") ? "Category" : state.columns[0];
    for (const option of elements.groupBySelect.options) option.selected = option.value === group;
    addAggregate({ op: "count", column: "*", alias: "rows" });
    const numeric = state.columns.includes("Amount") ? "Amount" : state.columns.find((column) => ["integer", "number"].includes(state.columnTypes.get(column)));
    if (numeric) addAggregate({ op: "sum", column: numeric, alias: `total${numeric}` });
  }
}

function addCondition(defaults = {}) {
  const row = document.createElement("div");
  row.className = "builder-row condition-row";
  const column = selectControl("column", state.columns);
  const operator = selectControl("operator", ["eq", "notEq", "lt", "le", "gt", "ge", "contains", "isEmpty", "isNotEmpty"]);
  const value = document.createElement("input");
  value.dataset.role = "value";
  value.value = defaults.value ?? "";
  value.placeholder = "Value";
  const remove = removeButton(() => row.remove());
  row.append(column, operator, value, remove);
  elements.conditionsList.append(row);
  column.value = defaults.column ?? state.columns[0] ?? "";
  operator.value = defaults.op ?? "eq";
  operator.addEventListener("change", () => {
    value.disabled = operator.value === "isEmpty" || operator.value === "isNotEmpty";
  });
}

function addAggregate(defaults = {}) {
  const row = document.createElement("div");
  row.className = "builder-row aggregate-row";
  const aggregate = selectControl("aggregate", ["count", "sum", "average", "min", "max"]);
  const column = selectControl("column", ["*", ...state.columns]);
  const alias = document.createElement("input");
  alias.dataset.role = "alias";
  alias.placeholder = "Alias";
  alias.value = defaults.alias ?? "value";
  row.append(aggregate, column, alias, removeButton(() => row.remove()));
  elements.aggregatesList.append(row);
  aggregate.value = defaults.op ?? "count";
  column.value = defaults.column ?? "*";
}

function selectControl(role, values) {
  const select = document.createElement("select");
  select.dataset.role = role;
  updateSelectOptions(select, values);
  return select;
}

function updateSelectOptions(select, values, preserveMultiple = false) {
  const selected = preserveMultiple
    ? new Set([...select.selectedOptions].map((option) => option.value))
    : new Set([select.value]);
  select.replaceChildren(...values.map((value) => new Option(value, value, false, selected.has(value))));
}

function removeButton(onClick) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "remove-button";
  button.title = "Remove";
  button.setAttribute("aria-label", "Remove");
  button.textContent = "×";
  button.addEventListener("click", onClick);
  return button;
}

function renderResult(table, raw, elapsed, metadata) {
  state.result = { table, raw };
  elements.resultEyebrow.textContent = metadata.eyebrow;
  elements.previewTitle.textContent = metadata.title;
  elements.metricRows.textContent = String(metadata.rows);
  elements.metricRowsLabel.textContent = metadata.rowsLabel ?? "rows";
  elements.metricColumns.textContent = String(metadata.columns);
  elements.metricColumnsLabel.textContent = metadata.columnsLabel ?? "columns";
  elements.metricTime.textContent = `${elapsed.toFixed(elapsed < 10 ? 1 : 0)} ms`;
  elements.resultNotice.textContent = metadata.notice;
  renderTable(table);
  elements.jsonView.textContent = JSON.stringify(raw, null, 2);
  elements.markdownView.textContent = state.mode === "rag" ? raw.chunksMarkdown : "";
  elements.loadingState.hidden = true;
  elements.emptyState.hidden = table.rows.length !== 0;
  setTab(state.activeTab);
  updateExportButtons();
}

function renderTable(result) {
  const headRow = document.createElement("tr");
  headRow.append(createCell("th", "#", "row-index"));
  for (const column of result.columns) headRow.append(createCell("th", column));
  elements.previewTable.tHead.replaceChildren(headRow);
  const body = document.createDocumentFragment();
  result.rows.forEach((row, rowIndex) => {
    const tr = document.createElement("tr");
    tr.append(createCell("td", String(rowIndex + 1), "row-index"));
    row.forEach((value, columnIndex) => {
      const cell = createCell("td", displayValue(value));
      cell.dataset.type = result.cellTypes[rowIndex]?.[columnIndex] ?? typeof value;
      cell.title = `${result.columns[columnIndex]} · ${cell.dataset.type}`;
      tr.append(cell);
    });
    body.append(tr);
  });
  elements.previewTable.tBodies[0].replaceChildren(body);
}

function setTab(tab) {
  state.activeTab = tab === "markdown" && state.mode !== "rag" ? "grid" : tab;
  const hasRows = Boolean(state.result?.table.rows.length);
  const isGrid = state.activeTab === "grid";
  const isJson = state.activeTab === "json";
  const isMarkdown = state.activeTab === "markdown";
  elements.previewTab.classList.toggle("is-active", isGrid);
  elements.previewTab.setAttribute("aria-selected", String(isGrid));
  elements.jsonTab.classList.toggle("is-active", isJson);
  elements.jsonTab.setAttribute("aria-selected", String(isJson));
  elements.markdownTab.classList.toggle("is-active", isMarkdown);
  elements.markdownTab.setAttribute("aria-selected", String(isMarkdown));
  elements.gridView.hidden = !hasRows || !isGrid;
  elements.jsonView.hidden = !hasRows || !isJson;
  elements.markdownView.hidden = !hasRows || !isMarkdown;
  elements.emptyState.hidden = hasRows || !state.result;
}

function updateHiddenSheetControl() {
  const hidden = isSelectedSheetHidden();
  elements.hiddenSheetOptIn.hidden = !hidden || state.mode !== "rag";
  elements.runRagButton.disabled = hidden && !elements.allowHiddenToggle.checked;
}

function isSelectedSheetHidden() {
  const selected = state.sheetInfo.find((sheet) => sheet.name === elements.sheetSelect.value);
  return Boolean(selected && selected.visibility !== "visible");
}

function updateExportButtons() {
  elements.downloadJsonButton.disabled = !state.result;
  const hasRag = state.mode === "rag" && state.ragResult;
  elements.downloadChunksButton.disabled = !hasRag;
  elements.downloadMarkdownChunksButton.disabled = !hasRag;
  elements.downloadManifestButton.disabled = !hasRag;
}

async function downloadDemo() {
  try {
    const buffer = await requestWorker("downloadDemo");
    downloadBlob(new Blob([buffer], { type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" }), "miniexcel-browser-demo.xlsx");
  } catch (error) {
    fail(error, "Demo workbook could not be downloaded");
  }
}

function downloadResultJson() {
  if (!state.result) return;
  downloadBlob(new Blob([JSON.stringify(state.result.raw, null, 2)], { type: "application/json" }), `${baseFileName()}-${state.mode}.json`);
}

function downloadChunks() {
  if (!state.ragResult) return;
  downloadBlob(new Blob([state.ragResult.chunksJsonl], { type: "application/x-ndjson" }), `${baseFileName()}.chunks.jsonl`);
}

function downloadMarkdownChunks() {
  if (!state.ragResult) return;
  downloadBlob(new Blob([state.ragResult.chunksMarkdown], { type: "text/markdown;charset=utf-8" }), `${baseFileName()}.chunks.md`);
}

function downloadManifest() {
  if (!state.ragResult) return;
  downloadBlob(new Blob([JSON.stringify(state.ragResult.manifest, null, 2)], { type: "application/json" }), `${baseFileName()}.manifest.json`);
}

function baseFileName() {
  return state.fileName.replace(/\.xlsx$/i, "") || "miniexcel";
}

function requestWorker(type, payload = {}, transfer = []) {
  const id = nextRequestId++;
  return new Promise((resolve, reject) => {
    pendingRequests.set(id, { resolve, reject });
    worker.postMessage({ id, type, payload }, transfer);
  });
}

function setLoading(message) {
  elements.loadingState.hidden = false;
  elements.loadingState.lastElementChild.textContent = message;
  elements.emptyState.hidden = true;
  elements.gridView.hidden = true;
  elements.jsonView.hidden = true;
  elements.resultNotice.textContent = "Working in worker";
}

function showValidationError(error) {
  showToast(error instanceof Error ? error.message : String(error), true);
}

function fail(error, prefix) {
  const message = error instanceof Error ? error.message : String(error);
  elements.loadingState.hidden = true;
  elements.resultNotice.textContent = "Error";
  showToast(`${prefix}: ${message}`, true);
  console.error(error);
}

function showToast(message, isError = false) {
  clearTimeout(state.toastTimer);
  elements.toast.textContent = message;
  elements.toast.classList.toggle("is-error", isError);
  elements.toast.hidden = false;
  state.toastTimer = setTimeout(() => { elements.toast.hidden = true; }, 5000);
}

function boundedInteger(input, minimum, maximum, label) {
  const value = Number.parseInt(input.value, 10);
  if (!Number.isInteger(value) || value < minimum || value > maximum) {
    input.focus();
    throw new Error(`${label} must be between ${minimum} and ${maximum}.`);
  }
  return value;
}

function createCell(tagName, text, className) {
  const cell = document.createElement(tagName);
  cell.textContent = text;
  if (className) cell.className = className;
  return cell;
}

function displayValue(value) {
  if (value === null || value === undefined) return "—";
  if (typeof value === "boolean") return value ? "true" : "false";
  return String(value);
}

function downloadBlob(blob, name) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

function formatBytes(bytes) {
  if (!Number.isFinite(bytes)) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
}