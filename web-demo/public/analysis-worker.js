import init, { create_demo_xlsx, version, WorkbookSession } from "./pkg/miniexcel_wasm.js";

let session = null;
let initialized = false;

self.addEventListener("message", async (event) => {
  const { id, type, payload = {} } = event.data;
  try {
    const result = await handle(type, payload);
    const transfer = result?.transfer ?? [];
    self.postMessage({ id, ok: true, result: result?.value ?? result }, transfer);
  } catch (error) {
    self.postMessage({
      id,
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    });
  }
});

async function handle(type, payload) {
  if (type === "initialize") {
    if (!initialized) {
      await init();
      initialized = true;
    }
    return { wasmVersion: version() };
  }

  ensureInitialized();
  if (type === "loadDemo") {
    const bytes = new Uint8Array(create_demo_xlsx());
    session = new WorkbookSession(bytes);
    return { byteLength: bytes.byteLength };
  }
  if (type === "loadWorkbook") {
    const bytes = new Uint8Array(payload.buffer);
    session = new WorkbookSession(bytes);
    return { byteLength: bytes.byteLength };
  }
  if (type === "downloadDemo") {
    const bytes = new Uint8Array(create_demo_xlsx());
    return { value: bytes.buffer, transfer: [bytes.buffer] };
  }

  ensureWorkbook();
  if (type === "preview") {
    return JSON.parse(session.inspect(JSON.stringify(payload.options)));
  }
  if (type === "analyze") {
    return JSON.parse(
      session.analyze(JSON.stringify(payload.options), JSON.stringify(payload.plan)),
    );
  }
  if (type === "exportRag") {
    return JSON.parse(
      session.exportRag(
        JSON.stringify(payload.options),
        JSON.stringify(payload.exportOptions),
      ),
    );
  }
  if (type === "exportSimpleMarkdown") {
    return JSON.parse(session.exportSimpleMarkdown(JSON.stringify(payload.options)));
  }
  throw new Error(`Unknown worker request: ${type}`);
}

function ensureInitialized() {
  if (!initialized) throw new Error("WebAssembly is not initialized");
}

function ensureWorkbook() {
  if (!session) throw new Error("No workbook is loaded");
}
