import { Link } from "react-router";
import { useCopy } from "../copy/CopyProvider";
import styles from "./pages.module.css";

/** Where Stripe sends the buyer back. Deliberately static: no API call, no session id shown. */
export function ThanksPage() {
  const copy = useCopy();
  return (
    <section className={styles.page}>
      <div className={styles.card}>
        <h1>{copy.feed.thanksTitle}</h1>
        <p className={styles.intro}>{copy.feed.thanksBody}</p>
        <Link to="/">← {copy.feed.back}</Link>
      </div>
    </section>
  );
}
