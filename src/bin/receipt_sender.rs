use fintech_pdf_receipt_mailer::{
    infrai::{InfraiClient, InfraiError},
    payment_report::{
        decide, render_email_html, render_pdf, NotificationDecision, PaymentEvent, PaymentState,
        ReportError,
    },
};
use std::{env, path::PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
enum RunError {
    #[error("usage: receipt_sender <recipient> <event-id> <amount-cents> <currency> <risk-score> <settled|declined>")]
    Usage,
    #[error("invalid integer: {0}")]
    Integer(#[from] std::num::ParseIntError),
    #[error("report decision failed: {0}")]
    Report(#[from] ReportError),
    #[error("email delivery failed: {0}")]
    Infrai(#[from] InfraiError),
    #[error("could not write PDF: {0}")]
    Io(#[from] std::io::Error),
}

#[tokio::main]
async fn main() -> Result<(), RunError> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() != 6 {
        return Err(RunError::Usage);
    }
    let state = match args[5].as_str() {
        "settled" => PaymentState::Settled,
        "declined" => PaymentState::Declined,
        _ => return Err(RunError::Usage),
    };
    let event = PaymentEvent {
        event_id: args[1].clone(),
        account_ref: format!("recipient:{}", args[0]),
        amount_cents: args[2].parse()?,
        currency: args[3].to_uppercase(),
        risk_score: args[4].parse()?,
        state,
    };
    let audit = decide(&event)?;
    let output = PathBuf::from(format!("{}.pdf", event.event_id));
    tokio::fs::write(&output, render_pdf(&event, &audit)).await?;

    match audit.decision {
        NotificationDecision::SendReceipt => {
            let infrai = InfraiClient::from_env()?;
            let sent = infrai
                .send_email(
                    &args[0],
                    &format!("Payment report {}", event.event_id),
                    &render_email_html(&event, &audit),
                    &format!("payment-report:{}", event.event_id),
                )
                .await?;
            println!(
                "decision=send_receipt message_id={} pdf={}",
                sent.message_id,
                output.display()
            );
        }
        NotificationDecision::ManualReview => {
            println!("decision=manual_review pdf={}", output.display());
        }
        NotificationDecision::RecordOnly => {
            println!("decision=record_only pdf={}", output.display());
        }
    }
    Ok(())
}
