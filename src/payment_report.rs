use std::fmt::Write as _;
use thiserror::Error;

pub const REVIEW_THRESHOLD: u16 = 700;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentState {
    Settled,
    Declined,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentEvent {
    pub event_id: String,
    pub account_ref: String,
    pub amount_cents: u64,
    pub currency: String,
    pub risk_score: u16,
    pub state: PaymentState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationDecision {
    SendReceipt,
    ManualReview,
    RecordOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditDecision {
    pub event_id: String,
    pub decision: NotificationDecision,
    pub reason: &'static str,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReportError {
    #[error("event_id and account_ref must be non-empty")]
    MissingIdentity,
    #[error("currency must be a three-letter ASCII code")]
    InvalidCurrency,
    #[error("risk_score must be between 0 and 1000")]
    InvalidRiskScore,
}

pub fn decide(event: &PaymentEvent) -> Result<AuditDecision, ReportError> {
    validate(event)?;
    let (decision, reason) = match (event.state, event.risk_score >= REVIEW_THRESHOLD) {
        (PaymentState::Settled, false) => (
            NotificationDecision::SendReceipt,
            "settled payment below review threshold",
        ),
        (PaymentState::Settled, true) => (
            NotificationDecision::ManualReview,
            "settled payment requires risk review",
        ),
        (PaymentState::Declined, _) => (
            NotificationDecision::RecordOnly,
            "declined payment is recorded without a receipt",
        ),
    };
    Ok(AuditDecision {
        event_id: event.event_id.clone(),
        decision,
        reason,
    })
}

fn validate(event: &PaymentEvent) -> Result<(), ReportError> {
    if event.event_id.trim().is_empty() || event.account_ref.trim().is_empty() {
        return Err(ReportError::MissingIdentity);
    }
    if event.currency.len() != 3 || !event.currency.bytes().all(|b| b.is_ascii_alphabetic()) {
        return Err(ReportError::InvalidCurrency);
    }
    if event.risk_score > 1000 {
        return Err(ReportError::InvalidRiskScore);
    }
    Ok(())
}

pub fn render_pdf(event: &PaymentEvent, audit: &AuditDecision) -> Vec<u8> {
    let decision = match audit.decision {
        NotificationDecision::SendReceipt => "SEND RECEIPT",
        NotificationDecision::ManualReview => "MANUAL REVIEW",
        NotificationDecision::RecordOnly => "RECORD ONLY",
    };
    let lines = [
        "Payment event report".to_owned(),
        format!("Event: {}", event.event_id),
        format!("Account: {}", event.account_ref),
        format!(
            "Amount: {} {} minor units",
            event.amount_cents, event.currency
        ),
        format!("Risk score: {}", event.risk_score),
        format!("Decision: {decision}"),
        format!("Reason: {}", audit.reason),
    ];
    minimal_pdf(&lines)
}

pub fn render_email_html(event: &PaymentEvent, audit: &AuditDecision) -> String {
    format!(
        "<h1>Payment report</h1><p>Event <code>{}</code> settled for {} {} minor units.</p><p>Audit decision: {}.</p>",
        escape_html(&event.event_id),
        event.amount_cents,
        escape_html(&event.currency),
        escape_html(audit.reason),
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn escape_pdf(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii() && !ch.is_ascii_control())
        .flat_map(|ch| match ch {
            '(' => "\\(".chars().collect::<Vec<_>>(),
            ')' => "\\)".chars().collect::<Vec<_>>(),
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            other => vec![other],
        })
        .collect()
}

fn minimal_pdf(lines: &[String]) -> Vec<u8> {
    let mut stream = String::from("BT\n/F1 12 Tf\n72 760 Td\n");
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            stream.push_str("0 -20 Td\n");
        }
        let _ = writeln!(stream, "({}) Tj", escape_pdf(line));
    }
    stream.push_str("ET\n");

    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{}endstream", stream.len(), stream),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];

    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settled_event(risk_score: u16) -> PaymentEvent {
        PaymentEvent {
            event_id: "pay_2026_0042".into(),
            account_ref: "acct_71".into(),
            amount_cents: 12_500,
            currency: "USD".into(),
            risk_score,
            state: PaymentState::Settled,
        }
    }

    #[test]
    fn holds_a_settled_payment_at_the_review_threshold() {
        let event = settled_event(REVIEW_THRESHOLD);
        let audit = decide(&event).unwrap();

        assert_eq!(audit.decision, NotificationDecision::ManualReview);
        assert_eq!(audit.reason, "settled payment requires risk review");
    }

    #[test]
    fn produces_a_pdf_for_an_approved_receipt() {
        let event = settled_event(120);
        let audit = decide(&event).unwrap();
        let pdf = render_pdf(&event, &audit);

        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(pdf
            .windows(event.event_id.len())
            .any(|w| w == event.event_id.as_bytes()));
    }
}
