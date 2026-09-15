export interface User {
  id: string;
  email?: string;
}
export interface Device {
  deviceId: string;
  deviceName: string;
  online: boolean;
  pairedAt?: string;
}
export interface Bootstrap {
  identity: User | null;
  authReady: boolean;
  returnTo: string;
  error?: { title: string; message: string };
  authorization?: {
    params: Record<string, string>;
    scopes: string[];
    csrf: string;
  };
}
export const bootstrap = JSON.parse(
  document.getElementById("latch-bootstrap")?.textContent || "null",
) as Bootstrap | null;
export const DOWNLOAD =
  "https://github.com/amaansyed27/Latch/releases/download/v0.4.5-beta.1/LatchSetup-x64.msi";
export const RELEASE =
  "https://github.com/amaansyed27/Latch/releases/tag/v0.4.5-beta.1";
export const MCP = "https://latch-router.vercel.app/mcp";
export function safeReturn(value: string | null): string {
  return value?.startsWith("/") &&
    !value.startsWith("//") &&
    !value.includes("\\")
    ? value
    : "/dashboard";
}
export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}
export async function request<T>(
  path: string,
  method = "GET",
  body?: unknown,
): Promise<T> {
  let response: Response;
  try {
    response = await fetch(path, {
      method,
      credentials: "same-origin",
      headers: body === undefined ? {} : { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(15000),
    });
  } catch {
    throw new ApiError(
      0,
      "Could not reach Latch. Check your connection and try again.",
    );
  }
  if (!response.ok) {
    const messages: Record<number, string> = {
      401: "Your session has ended. Please sign in again.",
      403: "This request could not be authorized. Refresh the page and try again.",
      404: "This item is no longer available.",
      409: "This item has changed. Refresh and try again.",
      422: "An account already exists for this email. Try signing in.",
      429: "Too many attempts. Wait a minute before trying again.",
    };
    throw new ApiError(
      response.status,
      messages[response.status] ||
        "Latch could not complete that request. Please try again.",
    );
  }
  return response.status === 204
    ? (undefined as T)
    : (response.json() as Promise<T>);
}
export function message(error: unknown): string {
  return error instanceof Error
    ? error.message
    : "Something went wrong. Please try again.";
}
