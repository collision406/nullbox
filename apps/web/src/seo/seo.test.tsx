import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { beforeEach, describe, expect, it } from "vitest";
import indexHtml from "../../index.html?raw";
import agentManifestRaw from "../../public/.well-known/agent.json?raw";
import aiPluginRaw from "../../public/.well-known/ai-plugin.json?raw";
import llmsTxt from "../../public/llms.txt?raw";
import openapiRaw from "../../public/openapi.json?raw";
import robotsTxt from "../../public/robots.txt?raw";
import sitemapXml from "../../public/sitemap.xml?raw";
import { clearSession, setSession } from "../api/session";
import { AppShell } from "../components/AppShell";
import { RequireWorker } from "../components/RequireWorker";
import { CopyProvider } from "../copy/CopyProvider";
import { BuyPage } from "../routes/BuyPage";
import { CustomerTicketPage } from "../routes/customer/CustomerTicketPage";
import { NewTicketPage } from "../routes/customer/NewTicketPage";
import { NotFoundPage } from "../routes/NotFoundPage";
import { SsoDonePage } from "../routes/SsoDonePage";
import { ThanksPage } from "../routes/ThanksPage";
import { InboxPage } from "../routes/worker/InboxPage";
import { WorkerTicketPage } from "../routes/worker/WorkerTicketPage";

function renderAtRoute(initialEntry: string, locale: "en" | "ja" = "en") {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const testRouter = createMemoryRouter(
    [
      {
        element: <AppShell />,
        children: [
          { path: "/", element: <NewTicketPage /> },
          { path: "/t/:id", element: <CustomerTicketPage /> },
          { path: "/buy", element: <BuyPage /> },
          { path: "/thanks", element: <ThanksPage /> },
          { path: "/sso/done", element: <SsoDonePage /> },
          {
            element: <RequireWorker />,
            children: [
              { path: "/inbox", element: <InboxPage /> },
              { path: "/inbox/:id", element: <WorkerTicketPage /> },
            ],
          },
          { path: "*", element: <NotFoundPage /> },
        ],
      },
    ],
    { initialEntries: [initialEntry] },
  );

  return render(
    <QueryClientProvider client={queryClient}>
      <CopyProvider locale={locale}>
        <RouterProvider router={testRouter} />
      </CopyProvider>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  clearSession();
});

describe("Lighthouse SEO & Static Discovery Files", () => {
  it("ships complete head metadata, Open Graph, Twitter Card, JSON-LD, and noscript in index.html", () => {
    const doc = new DOMParser().parseFromString(indexHtml, "text/html");

    expect(doc.documentElement.getAttribute("lang")).toBe("en");
    expect(doc.title).toContain("Nullbox");
    expect(doc.querySelector('meta[name="description"]')?.getAttribute("content")).toContain(
      "Nullbox",
    );
    expect(doc.querySelector('meta[name="robots"]')?.getAttribute("content")).toBe("index, follow");
    expect(doc.querySelector('link[rel="canonical"]')?.getAttribute("href")).toBe(
      "https://team-09.fasthacks.cloud/",
    );
    expect(doc.querySelector('meta[property="og:title"]')?.getAttribute("content")).toContain(
      "Nullbox",
    );
    expect(doc.querySelector('meta[property="og:url"]')?.getAttribute("content")).toBe(
      "https://team-09.fasthacks.cloud/",
    );
    expect(doc.querySelector('meta[property="og:image"]')?.getAttribute("content")).toBe(
      "https://team-09.fasthacks.cloud/og-image.svg",
    );
    expect(doc.querySelector('meta[name="twitter:card"]')?.getAttribute("content")).toBe(
      "summary_large_image",
    );

    const jsonLdRaw = doc.querySelector('script[type="application/ld+json"]')?.textContent ?? "";
    const jsonLd = JSON.parse(jsonLdRaw);
    expect(jsonLd["@context"]).toBe("https://schema.org");
    const appNode = jsonLd["@graph"].find(
      (node: { "@type": string }) => node["@type"] === "WebApplication",
    );
    expect(appNode).toBeDefined();
    expect(appNode.potentialAction[0]["@type"]).toBe("CreateAction");
    expect(appNode.potentialAction[0].target.urlTemplate).toBe(
      "https://team-09.fasthacks.cloud/api/tickets",
    );

    expect(indexHtml).toContain("<noscript>");
    expect(indexHtml).toContain("/llms.txt");
    expect(indexHtml).toContain("/openapi.json");
  });

  it("provides valid robots.txt, sitemap.xml, llms.txt, openapi.json, and .well-known manifests", () => {
    expect(robotsTxt).toContain("User-agent: *");
    expect(robotsTxt).toContain("Allow: /");
    expect(robotsTxt).toContain("Disallow: /t/");
    expect(robotsTxt).toContain("Disallow: /inbox");
    expect(robotsTxt).toContain("Sitemap: https://team-09.fasthacks.cloud/sitemap.xml");

    expect(sitemapXml).toContain("<loc>https://team-09.fasthacks.cloud/</loc>");
    expect(sitemapXml).toContain("<loc>https://team-09.fasthacks.cloud/buy</loc>");

    expect(llmsTxt).toContain("# Nullbox");
    expect(llmsTxt).toContain("POST /api/tickets");
    expect(llmsTxt).toContain("#new-ticket-form");

    const openapi = JSON.parse(openapiRaw);
    expect(openapi.openapi).toBe("3.1.0");
    expect(openapi.paths["/api/tickets"]).toBeDefined();
    expect(openapi.paths["/api/public/tickets/{id}"]).toBeDefined();

    const aiPlugin = JSON.parse(aiPluginRaw);
    expect(aiPlugin.api.url).toBe("https://team-09.fasthacks.cloud/openapi.json");

    const agentManifest = JSON.parse(agentManifestRaw);
    expect(agentManifest.openapi_url).toBe("https://team-09.fasthacks.cloud/openapi.json");
  });
});

describe("Per-Route Head Metadata & Agentic Browsing DOM Semantics", () => {
  it("sets indexable metadata and accessible form controls on /", () => {
    renderAtRoute("/");

    expect(document.title).toContain("Nullbox");
    expect(document.head.querySelector('meta[name="robots"]')?.getAttribute("content")).toBe(
      "index, follow",
    );
    expect(document.head.querySelector('meta[name="referrer"]')?.getAttribute("content")).toBe(
      "strict-origin-when-cross-origin",
    );

    expect(screen.getByRole("navigation", { name: "Primary navigation" })).toBeInTheDocument();
    expect(screen.getByRole("main")).toHaveAttribute("id", "main-content");

    const subjectInput = screen.getByLabelText("Subject");
    expect(subjectInput).toHaveAttribute("id", "ticket-subject");
    expect(subjectInput).toHaveAttribute("name", "subject");
    expect(subjectInput).toBeRequired();

    const bodyInput = screen.getByLabelText("What happened?");
    expect(bodyInput).toHaveAttribute("id", "ticket-body");
    expect(bodyInput).toHaveAttribute("name", "body");
    expect(bodyInput).toBeRequired();

    const severitySelect = screen.getByLabelText("How bad is it?");
    expect(severitySelect).toHaveAttribute("id", "ticket-severity");

    const nameInput = screen.getByLabelText("Your name (optional)");
    expect(nameInput).toHaveAttribute("aria-describedby", "new-ticket-hint");

    const submitButton = screen.getByRole("button", { name: "Send it into the void" });
    expect(submitButton).toHaveAttribute("id", "ticket-submit-button");
    expect(submitButton).toHaveAttribute("data-action", "create-ticket");
  });

  it("emits noindex, nofollow and no-referrer on private customer ticket routes (/t/:id)", () => {
    renderAtRoute("/t/123e4567-e89b-12d3-a456-426614174000");

    expect(document.head.querySelector('meta[name="robots"]')?.getAttribute("content")).toBe(
      "noindex, nofollow",
    );
    expect(document.head.querySelector('meta[name="referrer"]')?.getAttribute("content")).toBe(
      "no-referrer",
    );
    expect(
      document.head.querySelector<HTMLLinkElement>('link[rel="canonical"]')?.getAttribute("href"),
    ).toContain("/t/123e4567-e89b-12d3-a456-426614174000");
  });

  it("emits noindex, nofollow, no-referrer, and labeled controls on worker /inbox", () => {
    setSession({ token: "test-session-token", name: "agent-09" });
    renderAtRoute("/inbox");

    expect(document.title).toContain("Worker Inbox");
    expect(document.head.querySelector('meta[name="robots"]')?.getAttribute("content")).toBe(
      "noindex, nofollow",
    );
    expect(document.head.querySelector('meta[name="referrer"]')?.getAttribute("content")).toBe(
      "no-referrer",
    );

    expect(screen.getByLabelText("Show")).toHaveAttribute("id", "inbox-filter-status");
    expect(screen.getByLabelText("Search subjects")).toHaveAttribute("id", "inbox-search-subject");
  });

  it("emits noindex, nofollow on 404 routes", () => {
    renderAtRoute("/nonexistent-route");

    expect(document.title).toContain("404");
    expect(document.head.querySelector('meta[name="robots"]')?.getAttribute("content")).toBe(
      "noindex, nofollow",
    );
  });
});
