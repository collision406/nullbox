import type { ErrorCode } from "../api/generated/ErrorCode";
import type { Severity } from "../api/generated/Severity";
import type { TicketStatus } from "../api/generated/TicketStatus";

/**
 * English copy. This object defines the `Copy` type: every other dictionary
 * (ja.ts, and later Pirate, Corporate Speak or "Serious Mode") must match it.
 * Interpolation is done with functions, e.g. `openCount(3)`.
 */
const en = {
  app: {
    name: "Nullbox",
    subtitle: "Support, but make it void.",
    nav: {
      submit: "Report a problem",
      inbox: "Worker inbox",
      feed: "Feed the Beast",
      observability: "Observability",
    },
    language: "Language",
    footer: "No tickets were harmed in the making of this inbox.",
  },

  feed: {
    title: "Feed the Backlog Beast",
    pitch:
      "The Beast is hungry and the backlog is not a balanced diet. One tasty snack keeps it chewing instead of looking at you. No refunds; it already ate the receipt.",
    price: "¥500",
    button: "Feed the Beast",
    busy: "Unwrapping the snack…",
    thanksTitle: "The Beast is chewing. Thank you.",
    thanksBody: "Your snack has been eaten with great enthusiasm. The tickets are safe. For now.",
    back: "Report a problem",
  },

  status: {
    open: "Freshly Hatched",
    in_progress: "Being Stared At",
    waiting_on_customer: "Ball's in Your Court",
    resolved: "Yeeted",
    closed: "Sent to the Shadow Realm",
  } satisfies Record<TicketStatus, string>,

  severity: {
    p1: "Everything is on fire",
    p2: "Rage-typing",
    p3: "My cat is now in IT",
    p4: "Mildly inconvenienced",
  } satisfies Record<Severity, string>,

  /** Keys come from the backend (`nullbox_domain::ANON_ANIMALS`). */
  animals: {
    platypus: "Platypus",
    capybara: "Capybara",
    axolotl: "Axolotl",
    narwhal: "Narwhal",
    quokka: "Quokka",
    pangolin: "Pangolin",
    tardigrade: "Tardigrade",
    red_panda: "Red Panda",
    otter: "Otter",
    tanuki: "Tanuki",
    alpaca: "Alpaca",
    sloth: "Sloth",
  } as Record<string, string>,
  anonName: (animal: string, number: number) => `Anonymous ${animal} #${number}`,

  ticket: {
    number: (n: number) => `#${n}`,
    notFound: "Not Found",
    notFoundHint: "Ticket #404 exists. We just can't find it. Classic.",
    unassigned: "Nobody (yet)",
    severity: "Severity",
    status: "Status",
    assignee: "Assignee",
    customer: "From",
    opened: (when: string) => `opened ${when}`,
    aging: {
      fresh: "Fresh",
      yellowed: "Starting to yellow",
      cobwebs: "Cobwebs detected",
      fossil: "Officially a fossil",
    },
  },

  customer: {
    title: "What broke?",
    intro: "Tell us everything. We read every ticket. Eventually. Probably.",
    subject: "Subject",
    subjectPlaceholder: "The printer is making eye contact again",
    body: "What happened?",
    bodyPlaceholder: "Steps to reproduce, error messages, the sound it made…",
    severity: "How bad is it?",
    name: "Your name (optional)",
    email: "Email (optional)",
    anonymousHint: "Skip both and you'll get a dignified anonymous alias.",
    submit: "Send it into the void",
    submitting: "Launching…",
    createdTitle: "Ticket received!",
    createdBody: (n: number) => `Ticket #${n} is now officially somebody else's problem.`,
    secretLinkTitle: "Your secret link",
    secretLinkWarning:
      "This link is the only way back to your ticket. Bookmark it. We can't resend it: we don't even know who you are.",
    copyLink: "Copy link",
    copied: "Copied!",
    openTicket: "Open my ticket",
    missingToken:
      "This link is missing its secret part. Use the full link you got when you submitted the ticket.",
    threadTitle: "Conversation",
    reply: "Add a reply",
    replyPlaceholder: "Any updates? New symptoms? Fresh smoke?",
    send: "Send reply",
    closedNotice: "This ticket was sent to the Shadow Realm. Please open a new one.",
    sendHint: "Ctrl + Enter to send",
  },

  worker: {
    inboxTitle: "Inbox",
    filter: "Show",
    filterActive: "Everything still alive",
    search: "Search subjects",
    searchPlaceholder: "Filter by subject…",
    noMatches: "Nothing matches. Suspicious.",
    you: "You are",
    back: "Back to inbox",
    reply: "Reply to customer",
    internalNote: "Internal note",
    internalBadge: "Internal",
    replyPlaceholder: "Write something reassuring…",
    notePlaceholder: "Only workers can see this. Vent responsibly.",
    send: "Send",
    yeet: "Yeet it",
    yeetHint: "Marks the ticket as resolved.",
    yeeted: "Yeeted into the sun.",
    assigneePlaceholder: "Name or empty",
    assign: "Assign",
    assignToMe: "Take it",
  },

  beast: {
    title: "Backlog Beast",
    moods: {
      asleep: "Asleep. Do not wake it.",
      peckish: "Peckish. It has noticed you.",
      grumpy: "Grumpy. Snacks recommended.",
      kaiju: "KAIJU MODE. Evacuate the city.",
    },
    openCount: (n: string) => `${n} open`,
    oldest: (age: string) => `Oldest: ${age}`,
  },

  seo: {
    mainNav: "Primary navigation",
    skipToMain: "Skip to main content",
    ticketActions: "Ticket controls",
    homeTitle: "Nullbox — Support Ticket Portal | Report a Problem",
    homeDescription:
      "Nullbox is a privacy-first customer support and issue tracking portal. Report a problem with or without an account and track updates with a secret capability link.",
    createdTitle: (n: number) => `Ticket #${n} Received — Nullbox`,
    createdDescription:
      "Your support ticket has been created. Bookmark your one-time secret capability link to track replies.",
    customerTicketTitle: (n?: number) =>
      n !== undefined ? `Ticket #${n} — Nullbox Support` : "Your Support Ticket — Nullbox",
    customerTicketDescription:
      "Private customer support conversation thread accessed via secret capability link.",
    buyTitle: "Feed the Backlog Beast — Nullbox",
    buyDescription:
      "Support the Nullbox queue by feeding the Backlog Beast a ¥500 snack via Stripe Checkout.",
    thanksTitle: "Thank You for Feeding the Beast — Nullbox",
    thanksDescription: "Your snack has been delivered to the Nullbox Backlog Beast.",
    loginTitle: "Worker Sign In — Nullbox Inbox",
    loginDescription: "Sign in or create a worker account to access the Nullbox support inbox.",
    ssoTitle: "Completing Single Sign-On — Nullbox",
    ssoDescription: "Verifying your worker single sign-on session.",
    inboxTitle: "Worker Inbox & Backlog Beast — Nullbox",
    inboxDescription:
      "Internal Nullbox support queue for triaging, replying to, and resolving customer tickets.",
    workerTicketTitle: (n: number, subject: string) =>
      `Ticket #${n}: ${subject} — Worker Inbox | Nullbox`,
    workerTicketDescription:
      "Worker ticket management view for status updates, internal notes, and customer replies.",
    notFoundTitle: "404 Not Found — Nullbox",
    notFoundDescription: "The requested page could not be found in the Nullbox support portal.",
  },

  zero: {
    celebration: "INBOX ZERO!",
    void: "Please enjoy this complimentary void.",
  },

  thread: {
    empty: "No replies yet. The silence is deafening.",
    statusChanged: (from: string, to: string) => `Status: ${from} → ${to}`,
    itsAlive: "IT'S ALIVE! The customer replied and the ticket rose from the dead.",
    severityChanged: (from: string, to: string) => `Severity: ${from} → ${to}`,
    assigned: (to: string) => `Assigned to ${to}`,
    unassigned: "Unassigned. Free as a bird.",
    system: "Nullbox",
    you: "You",
  },

  auth: {
    loginTitle: "Log in to the inbox",
    signupTitle: "Join the inbox crew",
    intro: "Only logged-in workers may see other people's problems.",
    username: "Username",
    password: "Password",
    passwordHint: "At least 10 characters. Longer is better, a famous one is not.",
    inviteCode: "Invite code",
    login: "Log in",
    signup: "Sign up",
    busy: "One moment…",
    toSignup: "No account yet? Sign up",
    toLogin: "Already have an account? Log in",
    sso: (name: string) => `Sign in with ${name}`,
    logout: "Log out",
    ssoWorking: "Checking your badge…",
    ssoFailed: "Single sign-on didn't work out.",
    ssoBack: "Back to the inbox",
  },

  errors: {
    not_found: "We looked everywhere. Even under the couch. It's not there.",
    missing_ticket_token: "This page needs your secret link.",
    missing_actor: "Please log in to continue.",
    invalid_credentials: "Wrong username or password. The beast is not fooled.",
    username_taken: "Someone already took that username. Be more original.",
    password_too_short: "That password is too short. Ten characters minimum.",
    password_too_weak: "Too guessable. Don't use a common password or your username.",
    password_leaked: "That password turned up in a known data breach. Pick a fresh one.",
    forbidden: "You shall not pass.",
    validation_failed: "Something in the form doesn't look right. Please check the fields.",
    invalid_status_transition: "That status change isn't allowed. Time only flows one way.",
    ticket_closed: "This ticket is closed for good.",
    service_unavailable: "The server is taking a nap. Try again in a moment.",
    internal: "Something broke on our side. It works on our machine, though.",
    im_a_teapot: "I'm a teapot. Short and stout.",
    rate_limited: "Whoa there, speedracer. Take a breath and try again in a moment.",
    payload_too_large: "That's a lot of words. Trim it down and try again.",
    network_error: "Can't reach the server. Is it plugged in?",
  } satisfies Record<ErrorCode | "network_error", string>,

  loading: [
    "Bribing the database…",
    "Reticulating splines…",
    "Asking the hamsters to run faster…",
    "Consulting the rubber duck…",
    "Warming up the tubes…",
  ],

  eggs: {
    sudo: "This incident will be reported.",
    capsLock: "Priority: Loud",
    enterprise: {
      title: "Enterprise Mode™",
      intro: "Before you continue, please complete these mandatory fields.",
      fields: [
        "Synergy alignment quotient",
        "Paradigm shift reference ID",
        "Stakeholder's favorite font",
        "Fax number (required)",
        "Number of meetings this could have been an email",
      ],
      submit: "Submit for approval",
      synergizing: "Synergizing…",
      exit: "Return to reality",
    },
  },

  time: {
    unknown: "—",
  },

  observability: {
    title: "Observability & Void Telemetry",
    subtitle: "Live HTTP traffic, error rates, database readiness, and alert routing.",
    healthy: "Healthy",
    degraded: "Degraded",
    dbReady: "Database Ready (/api/readyz)",
    dbDown: "Database Unreachable",
    uptime: "Uptime",
    dbSize: "SQLite DB Size",
    processRss: "Process Memory (RSS)",
    requests: "Total Requests",
    errors4xx: "4xx Client Errors",
    errors5xx: "5xx Server Errors",
    latency: "Latency (Avg / P95)",
    routesTitle: "HTTP Traffic & Errors by Route",
    routeCol: "Route",
    requestsCol: "Requests",
    errorsCol: "Errors (4xx / 5xx)",
    latencyCol: "Avg / P95 (ms)",
    noRoutes: "No requests recorded yet.",
    eventsTitle: "Domain Events & Audit Stream",
    noEvents: "No domain events recorded yet.",
    alertsTitle: "Uptime & Alert Router (/api/alerts)",
    noAlerts: "No alerts fired. The void is calm.",
    triggerTestAlert: "Fire synthetic test alert",
    rawMetrics: "Prometheus /api/metrics",
    rawReadyz: "Readiness /api/readyz",
  },
};

export type Copy = typeof en;
export default en;
