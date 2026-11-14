import { Link, useLocation } from "react-router";
import { useCopy } from "../copy/CopyProvider";
import { usePageMeta } from "../seo/usePageMeta";

export function NotFoundPage() {
  const copy = useCopy();
  const { pathname } = useLocation();

  usePageMeta({
    title: copy.seo.notFoundTitle,
    description: copy.seo.notFoundDescription,
    canonicalPath: pathname || "/404",
    robots: "noindex, nofollow",
    referrer: "no-referrer",
  });

  return (
    <section aria-labelledby="not-found-heading">
      <h1 id="not-found-heading">404</h1>
      <p>{copy.errors.not_found}</p>
      <Link to="/">← {copy.app.nav.submit}</Link>
    </section>
  );
}
