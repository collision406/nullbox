import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CopyProvider, type Locale } from "../copy/CopyProvider";
import en from "../copy/en";
import ja from "../copy/ja";
import { BuyPage } from "./BuyPage";
import { ThanksPage } from "./ThanksPage";

const URL_OUT = "https://checkout.stripe.com/c/pay/cs_test_123";
const assign = vi.fn();
const originalLocation = window.location;

function renderPage(locale: Locale = "en", ui = <BuyPage />) {
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  return render(
    <CopyProvider locale={locale}>
      <QueryClientProvider client={client}>
        <MemoryRouter>{ui}</MemoryRouter>
      </QueryClientProvider>
    </CopyProvider>,
  );
}

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

beforeEach(() => {
  assign.mockReset();
  Object.defineProperty(window, "location", {
    configurable: true,
    value: { ...originalLocation, assign },
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
  Object.defineProperty(window, "location", { configurable: true, value: originalLocation });
});

describe("BuyPage", () => {
  it("POSTs /api/checkout without auth headers, then redirects", async () => {
    const fetchMock = vi.fn().mockResolvedValue(json({ url: URL_OUT }));
    vi.stubGlobal("fetch", fetchMock);
    renderPage();
    await userEvent.click(screen.getByRole("button", { name: en.feed.button }));

    await waitFor(() => expect(assign).toHaveBeenCalledWith(URL_OUT));
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toMatch(/\/api\/checkout$/);
    expect(init.method).toBe("POST");
    const names = Object.keys(init.headers).map((h) => h.toLowerCase());
    expect(names).not.toContain("authorization");
    expect(names).not.toContain("x-actor-name");
  });

  it("shows the service_unavailable copy and does not redirect", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(json({ code: "service_unavailable", message: "stripe down" }, 503)),
    );
    renderPage();
    await userEvent.click(screen.getByRole("button", { name: en.feed.button }));

    expect(await screen.findByRole("alert")).toHaveTextContent(en.errors.service_unavailable);
    expect(assign).not.toHaveBeenCalled();
  });

  it("disables the button while the request is pending", async () => {
    vi.stubGlobal("fetch", vi.fn().mockReturnValue(new Promise(() => {})));
    renderPage();
    await userEvent.click(screen.getByRole("button", { name: en.feed.button }));

    const busy = await screen.findByRole("button", { name: en.feed.busy });
    expect(busy).toBeDisabled();
  });

  it.each([
    ["en", en],
    ["ja", ja],
  ] as const)("renders in %s", (locale, dict) => {
    renderPage(locale);
    expect(screen.getByRole("heading", { name: dict.feed.title })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: dict.feed.button })).toBeEnabled();
  });
});

describe("ThanksPage", () => {
  it.each([
    ["en", en],
    ["ja", ja],
  ] as const)("renders in %s with a link home", (locale, dict) => {
    renderPage(locale, <ThanksPage />);
    expect(screen.getByRole("heading", { name: dict.feed.thanksTitle })).toBeInTheDocument();
    expect(screen.getByRole("link")).toHaveAttribute("href", "/");
  });
});
