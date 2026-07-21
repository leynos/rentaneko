/** @file Managed Simulacat Core runner owned by the Rentaneko `Simulator`. */
import { readFileSync } from "node:fs";
import { simulation, type InitialState } from "simulacat-core";

const LISTEN_TIMEOUT_MS = 5_000;
const SHUTDOWN_TIMEOUT_MS = 3_000;

interface RunnerConfig {
  version: number;
  initialState: InitialState;
  bind: { host: string; port: number };
}

try {
  const config = readConfig();
  const app = simulation({ initialState: config.initialState });
  let handle: Awaited<ReturnType<typeof app.listen>> | undefined;
  let isShuttingDown = false;
  let isClosing = false;

  const closeHandle = async () => {
    if (!handle || isClosing) {
      return;
    }
    isClosing = true;
    try {
      await withTimeout(handle.ensureClose(), SHUTDOWN_TIMEOUT_MS, "runner shutdown");
    } catch (error) {
      // Surface a non-zero exit so the Rust handle treats a failed shutdown as
      // an error rather than a clean stop.
      process.stderr.write(`runner shutdown failure: ${errorMessage(error)}\n`);
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
  // Self-terminate when the parent closes the owned stdin pipe (EOF).
  process.stdin.on("end", shutdown);
  process.stdin.on("close", shutdown);
  process.stdin.resume();

  handle = await withTimeout(
    app.listen(config.bind.port, config.bind.host),
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

function readConfig(): RunnerConfig {
  const path = process.env.RENTANEKO_RUNNER_CONFIG;
  if (!path) {
    throw new Error("RENTANEKO_RUNNER_CONFIG environment variable is not set");
  }
  const parsed = JSON.parse(readFileSync(path, "utf8")) as RunnerConfig;
  validateConfig(parsed);
  return parsed;
}

function validateConfig(config: RunnerConfig): void {
  if (config.version !== 1) {
    throw new Error(`unsupported config version: ${String(config.version)}`);
  }
  if (!config.initialState) {
    throw new Error("config.initialState is required");
  }
  if (!config.bind || config.bind.host !== "127.0.0.1") {
    throw new Error("config.bind.host must be 127.0.0.1");
  }
  if (config.bind.port !== 0) {
    throw new Error("config.bind.port must be 0; the runner owns port selection");
  }
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
