/**
 * The single API client: the only module that calls `fetch`.
 *
 * - Base URL from `VITE_API_BASE_URL`, so the same bundle works behind any
 *   origin (Tauri, a CDN, a different domain). No cookies, no same-origin assumptions.
 * - Worker requests carry `Authorization: Bearer <session token>` from `authHeaders()`.
 *   A 401 on such a request clears the session, so the UI falls back to the login screen.
 */
import type { AlertRecord } from "./generated/AlertRecord";
import type { AuthConfig } from "./generated/AuthConfig";
import type { AuthResponse } from "./generated/AuthResponse";
import type { BacklogStats } from "./generated/BacklogStats";
import type { CheckoutResponse } from "./generated/CheckoutResponse";
import type { CreateAlertRequest } from "./generated/CreateAlertRequest";
import type { CreateTicketRequest } from "./generated/CreateTicketRequest";
import type { CreateTicketResponse } from "./generated/CreateTicketResponse";
import type { ErrorBody } from "./generated/ErrorBody";
import type { ErrorCode } from "./generated/ErrorCode";
import type { Me } from "./generated/Me";
import type { Message } from "./generated/Message";
import type { ObservabilitySummary } from "./generated/ObservabilitySummary";
import type { Ticket } from "./generated/Ticket";
import type { TicketStatus } from "./generated/TicketStatus";
import type { TicketThread } from "./generated/TicketThread";
import type { UpdateTicketRequest } from "./generated/UpdateTicketRequest";
import type { WorkerMessageRequest } from "./generated/WorkerMessageRequest";

import { clearSession, getSession } from "./session";

export const API_BASE_URL: string = (
  import.meta.env.VITE_API_BASE_URL ?? "http://localhost:3000"
).replace(/\/$/, "");

export type ClientErrorCode = ErrorCode | "network_error";

export class ApiError extends Error {
  constructor(
    readonly code: ClientErrorCode,
    readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

/** Worker identity: the session bearer token. Nothing is sent when logged out. */
function authHeaders(): Record<string, string> {
  const session = getSession();
  return session ? { Authorization: `Bearer ${session.token}` } : {};
}

function ticketTokenHeaders(token: string): Record<string, string> {
  return { "X-Ticket-Token": token };
}

async function request<T>(
  method: "GET" | "POST" | "PATCH",
  path: string,
  options: { body?: unknown; headers?: Record<string, string>; explicitToken?: boolean } = {},
): Promise<T> {
  const headers: Record<string, string> = { Accept: "application/json", ...options.headers };
  if (options.body !== undefined) headers["Content-Type"] = "application/json";

  let res: Response;
  try {
    res = await fetch(`${API_BASE_URL}/api${path}`, {
      method,
      headers,
      body: options.body === undefined ? undefined : JSON.stringify(options.body),
    });
  } catch (err) {
    throw new ApiError("network_error", 0, String(err));
  }

  if (!res.ok) {
    if (res.status === 401 && "Authorization" in headers && !options.explicitToken) clearSession();
    const error = (await res.json().catch(() => null)) as ErrorBody | null;
    throw new ApiError(error?.code ?? "internal", res.status, error?.message ?? res.statusText);
  }
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

export const api = {
  // customers
  createTicket: (body: CreateTicketRequest) =>
    request<CreateTicketResponse>("POST", "/tickets", { body }),
  getCustomerTicket: (id: string, token: string) =>
    request<TicketThread>("GET", `/public/tickets/${id}`, { headers: ticketTokenHeaders(token) }),
  replyAsCustomer: (id: string, token: string, body: string) =>
    request<Message>("POST", `/public/tickets/${id}/messages`, {
      body: { body },
      headers: ticketTokenHeaders(token),
    }),

  // payments: no auth; the server owns the price and the Stripe call
  createCheckout: () => request<CheckoutResponse>("POST", "/checkout"),

  // auth
  authConfig: () => request<AuthConfig>("GET", "/auth/config"),
  signup: (body: { username: string; password: string; invite_code?: string }) =>
    request<AuthResponse>("POST", "/auth/signup", { body }),
  login: (body: { username: string; password: string }) =>
    request<AuthResponse>("POST", "/auth/login", { body }),
  logout: () => request<void>("POST", "/auth/logout", { headers: authHeaders() }),
  /** Explicit token: used by the SSO landing page before a session exists. */
  me: (token: string) =>
    request<Me>("GET", "/me", {
      headers: { Authorization: `Bearer ${token}` },
      explicitToken: true,
    }),

  // workers
  listTickets: (status?: TicketStatus) =>
    request<Ticket[]>("GET", status ? `/tickets?status=${status}` : "/tickets", {
      headers: authHeaders(),
    }),
  getTicket: (id: string) =>
    request<TicketThread>("GET", `/tickets/${id}`, { headers: authHeaders() }),
  addMessage: (id: string, body: WorkerMessageRequest) =>
    request<Message>("POST", `/tickets/${id}/messages`, { body, headers: authHeaders() }),
  updateTicket: (id: string, body: UpdateTicketRequest) =>
    request<Ticket>("PATCH", `/tickets/${id}`, { body, headers: authHeaders() }),
  /** Yeet = close (status becomes `resolved`). */
  yeetTicket: (id: string) =>
    request<Ticket>("POST", `/tickets/${id}/yeet`, { headers: authHeaders() }),
  backlogStats: () => request<BacklogStats>("GET", "/stats/backlog", { headers: authHeaders() }),

  // observability
  observabilitySummary: () =>
    request<ObservabilitySummary>("GET", "/observability/summary", { headers: authHeaders() }),
  triggerAlert: (body: CreateAlertRequest) =>
    request<AlertRecord>("POST", "/alerts/webhook", { body, headers: authHeaders() }),
};
