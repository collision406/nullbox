import { type FormEvent, useState } from "react";
import { Link } from "react-router";
import type { CreateTicketResponse } from "../../api/generated/CreateTicketResponse";
import type { Severity } from "../../api/generated/Severity";
import { useCreateTicket } from "../../api/queries";
import { ErrorMessage } from "../../components/Feedback";
import { useCopy } from "../../copy/CopyProvider";
import { usePageMeta } from "../../seo/usePageMeta";
import styles from "../pages.module.css";

const SEVERITIES: Severity[] = ["p4", "p3", "p2", "p1"];

export function NewTicketPage() {
  const copy = useCopy();
  const create = useCreateTicket();
  const [created, setCreated] = useState<CreateTicketResponse | null>(null);

  usePageMeta({
    title: created ? copy.seo.createdTitle(created.ticket.number) : copy.seo.homeTitle,
    description: created ? copy.seo.createdDescription : copy.seo.homeDescription,
    canonicalPath: "/",
    robots: created ? "noindex, nofollow" : "index, follow",
    referrer: created ? "no-referrer" : "strict-origin-when-cross-origin",
  });

  const onSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const text = (name: string) => String(form.get(name) ?? "").trim();
    create.mutate(
      {
        subject: text("subject"),
        body: text("body"),
        severity: text("severity") as Severity,
        customer_name: text("customer_name") || undefined,
        customer_email: text("customer_email") || undefined,
      },
      { onSuccess: setCreated },
    );
  };

  if (created) return <TicketCreated created={created} />;

  const describedBy = create.error ? "new-ticket-hint new-ticket-error" : "new-ticket-hint";

  return (
    <section className={styles.page} aria-labelledby="new-ticket-heading">
      <div>
        <h1 id="new-ticket-heading">{copy.customer.title}</h1>
        <p id="new-ticket-intro" className={styles.intro}>
          {copy.customer.intro}
        </p>
      </div>
      <form
        id="new-ticket-form"
        className={`${styles.card} ${styles.form}`}
        aria-labelledby="new-ticket-heading"
        aria-describedby={describedBy}
        aria-busy={create.isPending || undefined}
        onSubmit={onSubmit}
      >
        <label htmlFor="ticket-subject" className={styles.field}>
          {copy.customer.subject}
          <input
            id="ticket-subject"
            name="subject"
            required
            maxLength={200}
            placeholder={copy.customer.subjectPlaceholder}
          />
        </label>
        <label htmlFor="ticket-body" className={styles.field}>
          {copy.customer.body}
          <textarea
            id="ticket-body"
            name="body"
            required
            rows={6}
            maxLength={10000}
            placeholder={copy.customer.bodyPlaceholder}
          />
        </label>
        <label htmlFor="ticket-severity" className={styles.field}>
          {copy.customer.severity}
          <select id="ticket-severity" name="severity" defaultValue="p3">
            {SEVERITIES.map((severity) => (
              <option key={severity} value={severity}>
                {copy.severity[severity]}
              </option>
            ))}
          </select>
        </label>
        <div className={styles.row}>
          <label htmlFor="ticket-customer-name" className={styles.field}>
            {copy.customer.name}
            <input
              id="ticket-customer-name"
              name="customer_name"
              maxLength={100}
              autoComplete="name"
              aria-describedby="new-ticket-hint"
            />
          </label>
          <label htmlFor="ticket-customer-email" className={styles.field}>
            {copy.customer.email}
            <input
              id="ticket-customer-email"
              name="customer_email"
              type="email"
              maxLength={254}
              autoComplete="email"
              aria-describedby="new-ticket-hint"
            />
          </label>
        </div>
        <p id="new-ticket-hint" className={styles.hint}>
          {copy.customer.anonymousHint}
        </p>
        {create.error && <ErrorMessage id="new-ticket-error" error={create.error} />}
        <div>
          <button
            id="ticket-submit-button"
            type="submit"
            data-action="create-ticket"
            disabled={create.isPending}
          >
            {create.isPending ? copy.customer.submitting : copy.customer.submit}
          </button>
        </div>
      </form>
    </section>
  );
}

function TicketCreated({ created }: { created: CreateTicketResponse }) {
  const copy = useCopy();
  const [copied, setCopied] = useState(false);
  const path = `/t/${created.ticket.id}#${created.access_token}`;

  return (
    <section className={`${styles.card} ${styles.page}`} aria-labelledby="ticket-created-heading">
      <h1 id="ticket-created-heading">{copy.customer.createdTitle}</h1>
      <p>{copy.customer.createdBody(created.ticket.number)}</p>
      <h2>{copy.customer.secretLinkTitle}</h2>
      <div className={styles.linkBox}>
        <code id="secret-access-link">{created.access_link}</code>
        <button
          id="copy-secret-link-button"
          type="button"
          data-action="copy-secret-link"
          onClick={() =>
            navigator.clipboard.writeText(created.access_link).then(() => setCopied(true))
          }
        >
          {copied ? copy.customer.copied : copy.customer.copyLink}
        </button>
      </div>
      <p className={styles.warning}>{copy.customer.secretLinkWarning}</p>
      <div>
        <Link id="open-created-ticket-link" to={path}>
          {copy.customer.openTicket} →
        </Link>
      </div>
    </section>
  );
}
