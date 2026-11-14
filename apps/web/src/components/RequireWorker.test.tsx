import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../api/client";
import { getSession, setSession } from "../api/session";
import { CopyProvider } from "../copy/CopyProvider";
import en from "../copy/en";
import { AppShell } from "./AppShell";
import { RequireWorker } from "./RequireWorker";

const TOKEN = "tok_secret_123";
const AUTH = { token: TOKEN, name: "alice", expires_at: "2030-01-01T00:00:00Z" };

type Config = { invite_required: boolean; sso: { name: string; login_url: string } | null };

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

/** Routes fetch calls by "METHOD /path". */
function stubApi(routes: Record<string, () => Response>) {
  const fetchMock = vi.fn(async (url: string, init: RequestInit = {}) => {
    const key = `${init.method ?? "GET"} ${new URL(url).pathname.replace(/^\/api/, "")}`;
    const handler = routes[key];
    if (!handler) throw new Error(`unexpected request: ${key}`);
    return handler();
  });
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

function config(over: Partial<Config> = {}) {
  return () => json({ invite_required: false, sso: null, ...over });
}

function setup(client = new QueryClient({ defaultOptions: { queries: { retry: false } } })) {
  render(
    <CopyProvider locale="en">
      <QueryClientProvider client={client}>
        <MemoryRouter initialEntries={["/inbox"]}>
          <Routes>
            <Route element={<AppShell />}>
              <Route element={<RequireWorker />}>
                <Route path="/inbox" element={<p>secret inbox</p>} />
              </Route>
            </Route>
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>
    </CopyProvider>,
  );
  return client;
}

async function fillAndSubmit(button: string, user = "alice", pass = "correct horse battery") {
  await userEvent.type(screen.getByLabelText(en.auth.username), user);
  await userEvent.type(screen.getByLabelText(en.auth.password), pass);
  await userEvent.click(screen.getByRole("button", { name: button }));
}

beforeEach(() => {
  sessionStorage.clear();
  localStorage.clear();
  vi.spyOn(window.history, "replaceState");
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  // reset the module-level session
  sessionStorage.clear();
  return import("../api/session").then((m) => m.clearSession());
});

describe("RequireWorker", () => {
  it("shows the auth screen, not the children, when logged out", async () => {
    stubApi({ "GET /auth/config": config() });
    setup();
    expect(screen.getByRole("heading", { name: en.auth.loginTitle })).toBeInTheDocument();
    expect(screen.queryByText("secret inbox")).not.toBeInTheDocument();
  });

  it("logs in, keeps the token in sessionStorage only, and sends Bearer afterwards", async () => {
    const fetchMock = stubApi({
      "GET /auth/config": config(),
      "POST /auth/login": () => json(AUTH),
      "GET /tickets": () => json([]),
    });
    setup();
    await fillAndSubmit(en.auth.login);

    expect(await screen.findByText("secret inbox")).toBeInTheDocument();
    expect(getSession()).toEqual({ token: TOKEN, name: "alice" });
    expect(sessionStorage.getItem("nullbox.session")).toContain(TOKEN);
    expect(JSON.stringify({ ...localStorage })).not.toContain(TOKEN);

    await api.listTickets();
    const call = fetchMock.mock.calls.find(([u]) => String(u).endsWith("/api/tickets"));
    const headers = ((call?.[1] as RequestInit | undefined)?.headers ?? {}) as Record<
      string,
      string
    >;
    expect(headers.Authorization).toBe(`Bearer ${TOKEN}`);
    expect(Object.keys(headers).map((h) => h.toLowerCase())).not.toContain("x-actor-name");
  });

  it("shows invalid_credentials and stores nothing", async () => {
    stubApi({
      "GET /auth/config": config(),
      "POST /auth/login": () => json({ code: "invalid_credentials", message: "no" }, 401),
    });
    setup();
    await fillAndSubmit(en.auth.login, "alice", "wrong password!");

    expect(await screen.findByRole("alert")).toHaveTextContent(en.errors.invalid_credentials);
    expect(getSession()).toBeNull();
    expect(sessionStorage.length).toBe(0);
    expect(screen.queryByText("secret inbox")).not.toBeInTheDocument();
  });

  it.each([
    ["password_too_short", 422],
    ["password_too_weak", 422],
    ["password_leaked", 422],
    ["username_taken", 409],
  ] as const)("sign up shows the %s message", async (code, status) => {
    stubApi({
      "GET /auth/config": config(),
      "POST /auth/signup": () => json({ code, message: "x" }, status),
    });
    setup();
    await userEvent.click(screen.getByRole("button", { name: en.auth.toSignup }));
    await fillAndSubmit(en.auth.signup);

    expect(await screen.findByRole("alert")).toHaveTextContent(en.errors[code]);
    expect(getSession()).toBeNull();
  });

  it("shows the invite input only when invite_required", async () => {
    stubApi({ "GET /auth/config": config({ invite_required: true }) });
    setup();
    await userEvent.click(screen.getByRole("button", { name: en.auth.toSignup }));
    expect(await screen.findByLabelText(en.auth.inviteCode)).toBeInTheDocument();
  });

  it("hides the invite input when not required", async () => {
    stubApi({ "GET /auth/config": config() });
    setup();
    await userEvent.click(screen.getByRole("button", { name: en.auth.toSignup }));
    await waitFor(() => expect(fetch).toHaveBeenCalled());
    expect(screen.queryByLabelText(en.auth.inviteCode)).not.toBeInTheDocument();
  });

  it("shows the SSO button only when configured", async () => {
    stubApi({
      "GET /auth/config": config({ sso: { name: "Corp", login_url: "/api/auth/sso/login" } }),
    });
    setup();
    expect(await screen.findByRole("button", { name: en.auth.sso("Corp") })).toBeInTheDocument();
  });

  it("has no SSO button without sso", async () => {
    stubApi({ "GET /auth/config": config() });
    setup();
    await waitFor(() => expect(fetch).toHaveBeenCalled());
    expect(screen.queryByRole("button", { name: /Sign in with/ })).not.toBeInTheDocument();
  });

  it("clears the session on a 401 from a worker request", async () => {
    stubApi({
      "GET /auth/config": config(),
      "GET /tickets": () => json({ code: "missing_actor", message: "no" }, 401),
    });
    setSession({ token: TOKEN, name: "alice" });
    setup();
    expect(await screen.findByText("secret inbox")).toBeInTheDocument();

    await expect(api.listTickets()).rejects.toMatchObject({ status: 401 });
    expect(getSession()).toBeNull();
    expect(sessionStorage.getItem("nullbox.session")).toBeNull();
    expect(await screen.findByRole("heading", { name: en.auth.loginTitle })).toBeInTheDocument();
  });

  it("log out calls POST /auth/logout, clears the session and the query cache", async () => {
    const fetchMock = stubApi({
      "GET /auth/config": config(),
      "POST /auth/logout": () => new Response(null, { status: 204 }),
    });
    setSession({ token: TOKEN, name: "alice" });
    const client = setup();
    client.setQueryData(["tickets"], [{ id: "t1" }]);

    expect(screen.getByText("alice")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: en.auth.logout }));

    await waitFor(() => expect(getSession()).toBeNull());
    const call = fetchMock.mock.calls.find(([u]) => String(u).endsWith("/api/auth/logout"));
    expect((call?.[1] as RequestInit | undefined)?.method).toBe("POST");
    expect(client.getQueryData(["tickets"])).toBeUndefined();
    expect(sessionStorage.getItem("nullbox.session")).toBeNull();
  });
});
