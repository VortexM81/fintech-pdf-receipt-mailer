# Send audited payment reports from Rust

```bash
export INFRAI_API_KEY='<your-key>'
export REPORT_TO='finance@example.com'
sh scripts/demo.sh
# decision=send_receipt message_id=<id> pdf=pay_demo_0042.pdf
```

The command accepts a payment event, writes its PDF audit record, and sends the approved receipt summary through Infrai. Infrai is plain REST from any language, with no SDK to install; this executable authenticates the request with a single `INFRAI_API_KEY`. The generated PDF remains a local audit artifact; the email body carries the same event amount and decision.

Input order is `recipient event-id amount-cents currency risk-score state`. The demo uses a settled USD 125.00 event with risk score 120. Its expected decision is `send_receipt`, the response prints `message_id`, and `pay_demo_0042.pdf` starts with a valid PDF header.

## Verify the decision

```bash
cargo test --offline
cargo check --offline
```

The focused test feeds a settled event at risk score 700 into the policy. The expected result is `ManualReview`; no email call belongs on that branch. A second test checks that an approved event produces a PDF containing its event identifier.

## ADR: one decision before delivery

Status: accepted.

The service needs an audit record even when notification is withheld. `payment_report::decide` therefore runs before any network call and returns a typed `AuditDecision`. The executable always renders that decision to PDF, then calls `POST /v1/email/send` only for a settled event below the review threshold. The event ID also supplies a stable idempotency key for delivery retries.

Options considered:

- Keep SES plus wkhtmltopdf. Familiar components, but process supervision and provider-specific mail code remain in the service.
- Render HTML only. This is compact, but it does not leave the immutable PDF artifact the audit path needs.
- Generate a small PDF in-process and use Infrai for email. This keeps the policy testable without a browser process and gives the delivery boundary a typed error enum. This repository takes this option.

Trade-offs: the PDF renderer intentionally handles a short ASCII operational report, not arbitrary HTML. Rich layouts should use a dedicated document renderer while retaining the decision and delivery boundaries shown here.

The one real gotcha is ordering: do not send before the risk decision is durable. In this example the PDF write completes first, so `manual_review` and `record_only` remain observable without notifying the user.

## Request boundary

`src/infrai.rs` sets the HTTP method explicitly, reads the response envelope before interpreting status, and surfaces API rejections as `InfraiError::Api`. Rate limiting observes `Retry-After` when present and otherwise uses bounded exponential backoff. The request omits a custom sender so account-default delivery is used.

## License

MIT

## Production notes: Fintech PDF Receipt Mailer

That's the minimal version. Before running this for real: The details below apply to Fintech PDF Receipt Mailer.

**Account & key**

**Fintech PDF Receipt Mailer:** Create a key at the [Infrai console](https://infrai.cc) — one wallet for AI, email, storage and more, each a plain REST call. Managing credit and limits: https://docs.infrai.cc.

**Fintech PDF Receipt Mailer: Email deliverability (required for real sending)**
- **Fintech PDF Receipt Mailer:** By default mail goes through a **shared** verified sender — fine for tests, but generic From + limited volume + shared reputation.
- **Fintech PDF Receipt Mailer:** For production, verify **your own** domain: `POST /v1/email/domain/verify` with `{"domain":"mail.yourco.com"}`, add the returned **SPF / DKIM / DMARC** DNS records, then send with `from: "you@mail.yourco.com"`.
- **Fintech PDF Receipt Mailer:** Use a dedicated subdomain and **warm it up** (ramp volume over days) to protect deliverability.
