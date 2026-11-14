import { createBrowserRouter } from "react-router";
import { AppShell } from "./components/AppShell";
import { RequireWorker } from "./components/RequireWorker";
import { BuyPage } from "./routes/BuyPage";
import { CustomerTicketPage } from "./routes/customer/CustomerTicketPage";
import { NewTicketPage } from "./routes/customer/NewTicketPage";
import { NotFoundPage } from "./routes/NotFoundPage";
import { SsoDonePage } from "./routes/SsoDonePage";
import { ThanksPage } from "./routes/ThanksPage";
import { InboxPage } from "./routes/worker/InboxPage";
import { ObservabilityPage } from "./routes/worker/ObservabilityPage";
import { WorkerTicketPage } from "./routes/worker/WorkerTicketPage";

/**
 * All routes, in one place. For Tauri (no server to rewrite deep links),
 * swap `createBrowserRouter` for `createHashRouter`: that's the only change.
 */
export const router = createBrowserRouter([
  {
    element: <AppShell />,
    children: [
      // customer portal
      { path: "/", element: <NewTicketPage /> },
      { path: "/t/:id", element: <CustomerTicketPage /> },
      // paid checkout
      { path: "/buy", element: <BuyPage /> },
      { path: "/thanks", element: <ThanksPage /> },
      // worker inbox
      { path: "/sso/done", element: <SsoDonePage /> },
      {
        element: <RequireWorker />,
        children: [
          { path: "/inbox", element: <InboxPage /> },
          { path: "/inbox/:id", element: <WorkerTicketPage /> },
          // ticket subjects and worker names: workers only, like the inbox
          { path: "/observability", element: <ObservabilityPage /> },
        ],
      },
      { path: "*", element: <NotFoundPage /> },
    ],
  },
]);
