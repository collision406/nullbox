import { useQuery } from "@tanstack/react-query";
import { type FormEvent, useState, useSyncExternalStore } from "react";
import { Outlet } from "react-router";
import { API_BASE_URL, api } from "../api/client";
import { getSession, setSession, subscribeSession } from "../api/session";
import { useCopy } from "../copy/CopyProvider";
import styles from "../routes/pages.module.css";
import { ErrorMessage } from "./Feedback";

/** Worker routes: with a session render the children, otherwise the log in / sign up screen. */
export function RequireWorker() {
  const session = useSyncExternalStore(subscribeSession, getSession);
  return session ? <Outlet /> : <AuthScreen />;
}

function AuthScreen() {
  const copy = useCopy();
  const [mode, setMode] = useState<"login" | "signup">("login");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [invite, setInvite] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const config = useQuery({ queryKey: ["auth-config"], queryFn: api.authConfig, retry: false });
  const signup = mode === "signup";
  const sso = config.data?.sso ?? null;

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const code = invite.trim();
      const res = signup
        ? await api.signup({
            username,
            password,
            ...(config.data?.invite_required && code ? { invite_code: code } : {}),
          })
        : await api.login({ username, password });
      setSession({ token: res.token, name: res.name });
    } catch (err) {
      setError(err);
      setBusy(false);
    }
  }

  return (
    <section className={styles.page}>
      <div className={styles.card}>
        <h1>{signup ? copy.auth.signupTitle : copy.auth.loginTitle}</h1>
        <p className={styles.intro}>{copy.auth.intro}</p>
        <form className={styles.form} onSubmit={submit}>
          <label className={styles.field}>
            {copy.auth.username}
            <input
              value={username}
              autoComplete="username"
              maxLength={100}
              required
              onChange={(e) => setUsername(e.target.value)}
            />
          </label>
          <label className={styles.field}>
            {copy.auth.password}
            <input
              type="password"
              value={password}
              autoComplete={signup ? "new-password" : "current-password"}
              required
              onChange={(e) => setPassword(e.target.value)}
            />
          </label>
          {signup && <p className={styles.hint}>{copy.auth.passwordHint}</p>}
          {signup && config.data?.invite_required && (
            <label className={styles.field}>
              {copy.auth.inviteCode}
              <input
                value={invite}
                autoComplete="off"
                onChange={(e) => setInvite(e.target.value)}
              />
            </label>
          )}
          {error !== null && <ErrorMessage error={error} />}
          <div className={styles.row}>
            <button type="submit" disabled={busy}>
              {busy ? copy.auth.busy : signup ? copy.auth.signup : copy.auth.login}
            </button>
            <button
              type="button"
              onClick={() => {
                setMode(signup ? "login" : "signup");
                setError(null);
              }}
            >
              {signup ? copy.auth.toLogin : copy.auth.toSignup}
            </button>
          </div>
        </form>
        {sso && (
          <button
            type="button"
            onClick={() => window.location.assign(`${API_BASE_URL}${sso.login_url}`)}
          >
            {copy.auth.sso(sso.name)}
          </button>
        )}
      </div>
    </section>
  );
}
