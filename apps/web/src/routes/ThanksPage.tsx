import { Link } from "react-router";
import { useCopy } from "../copy/CopyProvider";
import { usePageMeta } from "../seo/usePageMeta";
import styles from "./pages.module.css";

/** Where Stripe sends the buyer back. Deliberately static: no API call, no session id shown. */
export function ThanksPage() {
  const copy = useCopy();

  usePageMeta({
    title: copy.seo.thanksTitle,
    description: copy.seo.thanksDescription,
    canonicalPath: "/thanks",
    robots: "noindex, nofollow",
    referrer: "no-referrer",
  });

  return (
    <section className={styles.page} aria-labelledby="thanks-heading">
      <div className={styles.card}>
        <h1 id="thanks-heading">{copy.feed.thanksTitle}</h1>
        <p className={styles.intro}>{copy.feed.thanksBody}</p>
        <Link to="/">← {copy.feed.back}</Link>
      </div>
    </section>
  );
}
