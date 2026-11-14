import { useCreateCheckout } from "../api/queries";
import { ErrorMessage } from "../components/Feedback";
import { useCopy } from "../copy/CopyProvider";
import { usePageMeta } from "../seo/usePageMeta";
import styles from "./pages.module.css";

/** Starts a Stripe Checkout. The server owns the price; we only follow the URL it returns. */
export function BuyPage() {
  const copy = useCopy();
  const checkout = useCreateCheckout();

  usePageMeta({
    title: copy.seo.buyTitle,
    description: copy.seo.buyDescription,
    canonicalPath: "/buy",
    robots: "index, follow",
    referrer: "strict-origin-when-cross-origin",
  });

  return (
    <section className={styles.page} aria-labelledby="buy-heading">
      <div className={styles.card}>
        <h1 id="buy-heading">{copy.feed.title}</h1>
        <p className={styles.intro}>{copy.feed.pitch}</p>
        <p className={styles.muted}>{copy.feed.price}</p>
        <button
          id="feed-beast-button"
          type="button"
          data-action="start-checkout"
          disabled={checkout.isPending}
          onClick={() =>
            checkout.mutate(undefined, {
              onSuccess: ({ url }) => window.location.assign(url),
            })
          }
        >
          {checkout.isPending ? copy.feed.busy : copy.feed.button}
        </button>
        {checkout.isError && <ErrorMessage error={checkout.error} />}
      </div>
    </section>
  );
}
