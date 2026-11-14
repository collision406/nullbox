import { API_BASE_URL } from "../../api/client";
import { useObservabilitySummary, useTriggerAlert } from "../../api/queries";
import { ErrorMessage, Loading } from "../../components/Feedback";
import { useCopy, useFormat } from "../../copy/CopyProvider";
import styles from "../pages.module.css";

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

export function ObservabilityPage() {
  const copy = useCopy();
  const format = useFormat();
  const summary = useObservabilitySummary();
  const triggerAlert = useTriggerAlert();

  if (summary.isPending) return <Loading />;
  if (summary.error) return <ErrorMessage error={summary.error} />;

  const data = summary.data;
  const healthy = data.status === "healthy" && data.db_ready;

  return (
    <section className={styles.page}>
      <div className={styles.titleRow}>
        <h1>{copy.observability.title}</h1>
        <span className={healthy ? styles.pillOk : styles.pillWarn}>
          {healthy ? copy.observability.healthy : copy.observability.degraded}
        </span>
        <span className={data.db_ready ? styles.pillOk : styles.pillWarn}>
          {data.db_ready ? copy.observability.dbReady : copy.observability.dbDown}
          {data.db_latency_us !== null ? ` (${data.db_latency_us} µs)` : ""}
        </span>
      </div>
      <p className={styles.intro}>{copy.observability.subtitle}</p>

      <div className={styles.metricsGrid}>
        <div className={styles.metricCard}>
          <span className={styles.metricLabel}>{copy.observability.requests}</span>
          <span className={styles.metricValue}>{format.number(data.total_requests)}</span>
        </div>
        <div className={styles.metricCard}>
          <span className={styles.metricLabel}>{copy.observability.errors4xx}</span>
          <span className={styles.metricValue}>{format.number(data.total_errors_4xx)}</span>
        </div>
        <div className={styles.metricCard}>
          <span className={styles.metricLabel}>{copy.observability.errors5xx}</span>
          <span className={styles.metricValue}>{format.number(data.total_errors_5xx)}</span>
        </div>
        <div className={styles.metricCard}>
          <span className={styles.metricLabel}>{copy.observability.latency}</span>
          <span className={styles.metricValue}>
            {data.avg_latency_ms.toFixed(2)} / {data.p95_latency_ms.toFixed(2)} ms
          </span>
        </div>
        <div className={styles.metricCard}>
          <span className={styles.metricLabel}>{copy.observability.dbSize}</span>
          <span className={styles.metricValue}>{formatBytes(data.db_size_bytes)}</span>
        </div>
        <div className={styles.metricCard}>
          <span className={styles.metricLabel}>{copy.observability.processRss}</span>
          <span className={styles.metricValue}>
            {formatBytes(data.process_rss_bytes)} ({data.process_open_fds} FDs)
          </span>
        </div>
        <div className={styles.metricCard}>
          <span className={styles.metricLabel}>{copy.observability.uptime}</span>
          <span className={styles.metricValue}>{data.uptime_secs}s</span>
        </div>
      </div>

      <div className={styles.card}>
        <div className={styles.titleRow}>
          <h2>{copy.observability.routesTitle}</h2>
          <a href={`${API_BASE_URL}/api/metrics`} target="_blank" rel="noreferrer">
            {copy.observability.rawMetrics} ↗
          </a>
          <a href={`${API_BASE_URL}/api/readyz`} target="_blank" rel="noreferrer">
            {copy.observability.rawReadyz} ↗
          </a>
        </div>
        {data.routes.length === 0 ? (
          <p className={styles.muted}>{copy.observability.noRoutes}</p>
        ) : (
          <table className={styles.table}>
            <thead>
              <tr>
                <th>{copy.observability.routeCol}</th>
                <th>{copy.observability.requestsCol}</th>
                <th>{copy.observability.errorsCol}</th>
                <th>{copy.observability.latencyCol}</th>
              </tr>
            </thead>
            <tbody>
              {data.routes.map((route) => (
                <tr key={`${route.method}:${route.route}`}>
                  <td>
                    <code>
                      {route.method} {route.route}
                    </code>
                  </td>
                  <td>{format.number(route.requests)}</td>
                  <td>
                    {format.number(route.errors_4xx)} / {format.number(route.errors_5xx)}
                  </td>
                  <td>
                    {route.avg_latency_ms.toFixed(2)} / {route.p95_latency_ms.toFixed(2)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <div className={styles.card}>
        <div className={styles.titleRow}>
          <h2>{copy.observability.alertsTitle}</h2>
          <button
            type="button"
            disabled={triggerAlert.isPending}
            onClick={() =>
              triggerAlert.mutate({
                title: "Synthetic Uptime Alert",
                message: "Manual test alert triggered from Observability dashboard",
                severity: "warning",
                source: "dashboard",
              })
            }
          >
            {copy.observability.triggerTestAlert}
          </button>
        </div>
        {data.recent_alerts.length === 0 ? (
          <p className={styles.muted}>{copy.observability.noAlerts}</p>
        ) : (
          <ul className={styles.list}>
            {data.recent_alerts.map((alert) => (
              <li key={alert.id} className={styles.ticketRow}>
                <span className={styles.pillWarn}>{alert.severity}</span>
                <span className={styles.ticketSubject}>
                  [{alert.source}] {alert.title} — {alert.message}
                </span>
                <span className={styles.muted}>{format.dateTime(alert.created_at)}</span>
              </li>
            ))}
          </ul>
        )}
      </div>

      <div className={styles.card}>
        <h2>{copy.observability.eventsTitle}</h2>
        {data.recent_events.length === 0 ? (
          <p className={styles.muted}>{copy.observability.noEvents}</p>
        ) : (
          <ul className={styles.list}>
            {data.recent_events.map((event) => (
              <li key={event.id} className={styles.ticketRow}>
                <code>{event.kind}</code>
                <span className={styles.ticketSubject}>
                  {event.summary}
                  {event.actor ? ` (${event.actor})` : ""}
                </span>
                <span className={styles.muted}>{format.dateTime(event.created_at)}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}
