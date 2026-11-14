import { useLocation, useParams } from "react-router";
import { useCustomerReply, useCustomerTicket } from "../../api/queries";
import { SeverityBadge, StatusBadge, TicketNumber } from "../../components/Badges";
import { Composer } from "../../components/Composer";
import { ErrorMessage, Loading } from "../../components/Feedback";
import { Thread } from "../../components/Thread";
import { useCopy } from "../../copy/CopyProvider";
import { usePageMeta } from "../../seo/usePageMeta";
import styles from "../pages.module.css";

/**
 * `/t/:id#token`. The token lives in the URL fragment, which browsers never
 * send to servers, and goes to the API as `X-Ticket-Token`.
 */
export function CustomerTicketPage() {
  const copy = useCopy();
  const { id = "" } = useParams();
  const token = useLocation().hash.slice(1) || null;
  const thread = useCustomerTicket(id, token);
  const reply = useCustomerReply(id, token ?? "");

  usePageMeta({
    title: copy.seo.customerTicketTitle(thread.data?.ticket.number),
    description: copy.seo.customerTicketDescription,
    canonicalPath: `/t/${id}`,
    robots: "noindex, nofollow",
    referrer: "no-referrer",
  });

  if (!token) return <p className={styles.warning}>{copy.customer.missingToken}</p>;
  if (thread.isPending) return <Loading />;
  if (thread.error) return <ErrorMessage error={thread.error} />;

  const { ticket, messages } = thread.data;
  return (
    <section className={styles.page} aria-labelledby="customer-ticket-heading">
      <div className={styles.titleRow}>
        <span className={styles.muted}>
          <TicketNumber number={ticket.number} />
        </span>
        <h1 id="customer-ticket-heading">{ticket.subject}</h1>
        <StatusBadge status={ticket.status} />
        <SeverityBadge severity={ticket.severity} />
      </div>
      <h2>{copy.customer.threadTitle}</h2>
      <Thread ticket={ticket} messages={messages} />
      {ticket.status === "closed" ? (
        <p className={styles.warning}>{copy.customer.closedNotice}</p>
      ) : (
        <div className={styles.card}>
          <Composer
            label={copy.customer.reply}
            placeholder={copy.customer.replyPlaceholder}
            submitLabel={copy.customer.send}
            busy={reply.isPending}
            onSubmit={(body) => reply.mutateAsync(body)}
          />
          {reply.error && <ErrorMessage error={reply.error} />}
        </div>
      )}
    </section>
  );
}
