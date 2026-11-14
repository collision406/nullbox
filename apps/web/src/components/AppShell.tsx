import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useState, useSyncExternalStore } from "react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router";
import { api } from "../api/client";
import { clearSession, getSession, subscribeSession } from "../api/session";
import { dictionaries, type Locale, useCopy, useLocale } from "../copy/CopyProvider";
import { useKonami } from "../hooks/useKonami";
import styles from "./AppShell.module.css";
import { EnterpriseMode } from "./EnterpriseMode";

const LANGUAGE_NAMES: Record<Locale, string> = { en: "English", ja: "日本語" };

export function AppShell() {
  const copy = useCopy();
  const { pathname } = useLocation();
  const [enterprise, setEnterprise] = useState(false);
  useKonami(useCallback(() => setEnterprise(true), []));

  return (
    <div className={styles.shell}>
      <a href="#main-content" className={styles.skipLink}>
        {copy.seo.skipToMain}
      </a>
      <header className={styles.header}>
        <NavLink to="/" className={styles.brand}>
          <span className={styles.logo} aria-hidden="true">
            ∅
          </span>
          <span>
            <strong>{copy.app.name}</strong>
            <small>{copy.app.subtitle}</small>
          </span>
        </NavLink>
        <nav className={styles.nav} aria-label={copy.seo.mainNav}>
          <NavLink to="/" end>
            {copy.app.nav.submit}
          </NavLink>
          <NavLink to="/buy">{copy.app.nav.feed}</NavLink>
          <NavLink to="/inbox">{copy.app.nav.inbox}</NavLink>
          <NavLink to="/observability">{copy.app.nav.observability}</NavLink>
        </nav>
        <div className={styles.tools}>
          {pathname.startsWith("/inbox") && <WorkerSession />}
          <LanguageSwitcher />
        </div>
      </header>
      <main id="main-content" className={styles.main}>
        <Outlet />
      </main>
      <footer className={styles.footer}>{copy.app.footer}</footer>
      {enterprise && <EnterpriseMode onExit={() => setEnterprise(false)} />}
    </div>
  );
}

function LanguageSwitcher() {
  const copy = useCopy();
  const [locale, setLocale] = useLocale();
  return (
    <fieldset className={styles.switcher} aria-label={copy.app.language}>
      {(Object.keys(dictionaries) as Locale[]).map((option) => (
        <button
          key={option}
          type="button"
          lang={option}
          aria-pressed={option === locale}
          onClick={() => setLocale(option)}
        >
          {LANGUAGE_NAMES[option]}
        </button>
      ))}
    </fieldset>
  );
}

/** "You are <name>" plus a log out button, shown in the worker area. */
function WorkerSession() {
  const copy = useCopy();
  const session = useSyncExternalStore(subscribeSession, getSession);
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  if (!session) return null;

  async function logout() {
    try {
      await api.logout();
    } catch {
      // the local session goes away either way
    }
    clearSession();
    queryClient.clear();
    navigate("/inbox");
  }

  return (
    <div className={styles.actor}>
      <span>
        {copy.worker.you} <strong>{session.name}</strong>
      </span>
      <button type="button" onClick={logout}>
        {copy.auth.logout}
      </button>
    </div>
  );
}
