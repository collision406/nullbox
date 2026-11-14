import { useEffect, useRef, useState } from "react";
import { Link, useNavigate } from "react-router";
import { api } from "../api/client";
import { setSession } from "../api/session";
import { ErrorMessage } from "../components/Feedback";
import { useCopy } from "../copy/CopyProvider";
import styles from "./pages.module.css";

/** Where the server sends the browser after SSO. The token arrives in the URL fragment only. */
export function SsoDonePage() {
  const copy = useCopy();
  const navigate = useNavigate();
  const started = useRef(false);
  const [failure, setFailure] = useState<{ error?: unknown } | null>(null);

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    const params = new URLSearchParams(window.location.hash.replace(/^#/, ""));
    const token = params.get("token");
    const hasError = params.has("error");
    // Drop the fragment right away so the token never stays in the address bar.
    window.history.replaceState(null, "", window.location.pathname);
    if (!token || hasError) {
      setFailure({});
      return;
    }
    api
      .me(token)
      .then((me) => {
        setSession({ token, name: me.name });
        navigate("/inbox", { replace: true });
      })
      .catch((error) => setFailure({ error }));
  }, [navigate]);

  return (
    <section className={styles.page}>
      <div className={styles.card}>
        {failure ? (
          <>
            <p className={styles.warning}>{copy.auth.ssoFailed}</p>
            {failure.error !== undefined && <ErrorMessage error={failure.error} />}
            <Link to="/inbox">{copy.auth.ssoBack}</Link>
          </>
        ) : (
          <p role="status">{copy.auth.ssoWorking}</p>
        )}
      </div>
    </section>
  );
}
