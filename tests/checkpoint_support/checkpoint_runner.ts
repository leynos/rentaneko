/** @file Throwaway Simulacat Core server for the 1.1.1 checkpoint. */
import { simulation, type InitialState } from "simulacat-core";

const LISTEN_TIMEOUT_MS = 5_000;
const SHUTDOWN_TIMEOUT_MS = 3_000;

const initialState: InitialState = {
  users: [],
  installations: [{ id: 2000, account: "rentaneko", app_id: 1 }],
  organizations: [{ login: "rentaneko" }],
  repositories: [],
  branches: [],
  blobs: [],
};

try {
  const app = simulation({ initialState });
  let handle: Awaited<ReturnType<typeof app.listen>> | undefined;
  let isShuttingDown = false;
  let isClosing = false;

  const closeHandle = async () => {
    if (!handle || isClosing) {
      return;
    }
    isClosing = true;
    try {
      await withTimeout(handle.ensureClose(), SHUTDOWN_TIMEOUT_MS, "checkpoint shutdown");
    } catch (error) {
      // Surface a non-zero exit so the Rust harness treats the failed shutdown
      // as an error rather than a clean stop.
      process.stderr.write(`checkpoint shutdown failure: ${errorMessage(error)}\n`);
      process.exit(1);
    }
    process.exit(0);
  };

  const shutdown = async () => {
    if (isShuttingDown) {
      return;
    }
    isShuttingDown = true;
    await closeHandle();
  };

  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);

  handle = await withTimeout(
    app.listen(0, "127.0.0.1"),
    LISTEN_TIMEOUT_MS,
    "Simulacat Core listen",
  );
  if (isShuttingDown) {
    await closeHandle();
  }

  const address = handle.server.address();
  const port = extractPort(address, handle.port);
  process.stdout.write(
    `${JSON.stringify({ version: 1, event: "listening", host: "127.0.0.1", port })}\n`,
  );
} catch (error) {
  const message = errorMessage(error);
  const stack = error instanceof Error ? error.stack : undefined;
  process.stdout.write(
    `${JSON.stringify({ version: 1, event: "error", message, stack })}\n`,
  );
  process.exit(1);
}

function extractPort(address: unknown, fallbackPort: unknown): number {
  let port: unknown = fallbackPort;
  if (typeof address === "object" && address && "port" in address) {
    port = address.port;
  }

  if (typeof port !== "number" || !Number.isFinite(port) || port < 1 || port > 65_535) {
    throw new Error("Simulacat Core did not report a listening port");
  }
  return port;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

// Throwaway checkpoint support: extract only if a second timeout call-site survives 1.3.2.
function withTimeout<T>(promise: Promise<T>, milliseconds: number, operation: string): Promise<T> {
  let timeoutId: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timeoutId = setTimeout(
      () => reject(new Error(`${operation} timed out after ${milliseconds}ms`)),
      milliseconds,
    );
  });
  return Promise.race([promise, timeout]).finally(() => {
    if (timeoutId) {
      clearTimeout(timeoutId);
    }
  });
}
